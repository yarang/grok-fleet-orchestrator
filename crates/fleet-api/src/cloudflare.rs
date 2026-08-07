//! Cloudflare Access 인증 미들웨어.
//!
//! Cloudflare Zero Trust 환경에서 Cloudflare Access가 HTTP 요청에
//! `Cf-Access-Jwt-Assertion` 헤더를 추가합니다. 이 헤더의 JWT를 검증하여
//! 합법적인 Cloudflare Access 세션임을 확인합니다.
//!
//! ## 검증 단계
//!
//! 1. `Cf-Access-Jwt-Assertion` 헤더 추출
//! 2. JWT 구조 파싱 (header.payload.signature)
//! 3. payload의 `aud` 클레임이 TEAM_AUDIENCE와 일치
//! 4. payload의 `exp`가 만료되지 않음
//! 5. (권장) Cloudflare 공개키로 서명 검증 — Phase 4에서는 exp/aud만.
//!    Phase 5 이후 `jsonwebtoken` 크레이트로 서명 검증 추가 예정.
//!
//! ## 우회 조건
//!
//! - `AppState.allow_no_auth == true`면 통과 (개발 모드).
//! - `/v1/health` 경로는 항상 허용 (LB 프로브).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::app::AppState;

/// Cloudflare Access JWT의 페이로드 (검증에 필요한 클레임만).
#[derive(Debug, Deserialize, serde::Serialize)]
struct CfAccessClaims {
    /// 만료 시각 (Unix epoch 초).
    exp: u64,
    /// 청중(Cloudflare Access Application AUD).
    aud: String,
    /// 발행자.
    iss: Option<String>,
    /// 이메일 (있는 경우).
    #[serde(default)]
    email: Option<String>,
}

struct JwksCache {
    keys: HashMap<String, (JwkSet, Instant)>,
}

static JWKS_CACHE: OnceLock<RwLock<JwksCache>> = OnceLock::new();

fn get_jwks_cache() -> &'static RwLock<JwksCache> {
    JWKS_CACHE.get_or_init(|| {
        RwLock::new(JwksCache {
            keys: HashMap::new(),
        })
    })
}

/// Cloudflare Access JWT 검증 결과.
#[derive(Debug, Clone)]
pub struct VerifiedUser {
    pub email: String,
    pub audience: String,
    pub expires_at: u64,
}

/// 미들웨어 본문. `axum::middleware::from_fn`으로 등록.
pub async fn cloudflare_access_middleware(
    state: Arc<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // 개발 모드: 인증 생략.
    if state.allow_no_auth {
        return Ok(next.run(req).await);
    }

    // 헬스체크 엔드포인트는 항상 허용.
    if req.uri().path() == "/v1/health" || req.uri().path() == "/health" {
        return Ok(next.run(req).await);
    }

    // CF-Access-Jwt-Assertion 헤더 추출.
    let jwt = req
        .headers()
        .get("cf-access-jwt-assertion")
        .or_else(|| req.headers().get("CF-Access-Jwt-Assertion"))
        .and_then(|v| v.to_str().ok());

    let Some(jwt) = jwt else {
        tracing::warn!(
            path = %req.uri().path(),
            "missing Cf-Access-Jwt-Assertion header"
        );
        return Err(StatusCode::UNAUTHORIZED);
    };

    // 1. 헤더 파싱하여 kid 추출
    let header = decode_header(jwt).map_err(|e| {
        tracing::warn!(error = %e, path = %req.uri().path(), "failed to decode JWT header");
        StatusCode::UNAUTHORIZED
    })?;

    let Some(kid) = &header.kid else {
        tracing::warn!(path = %req.uri().path(), "JWT header missing kid");
        return Err(StatusCode::UNAUTHORIZED);
    };

    // 2. Unsafe 파싱으로 iss 획득 (키 찾기 위함)
    let temp_claims = parse_jwt_unsafe(jwt).map_err(|e| {
        tracing::warn!(error = %e, path = %req.uri().path(), "failed to parse JWT payload unsafely");
        StatusCode::UNAUTHORIZED
    })?;

    let Some(iss) = &temp_claims.iss else {
        tracing::warn!(path = %req.uri().path(), "JWT payload missing iss claim");
        return Err(StatusCode::UNAUTHORIZED);
    };

    // 3. JWK 로드 (캐시 및 원격 fetch)
    let jwk = get_jwk(iss, kid).await.map_err(|e| {
        tracing::warn!(error = %e, path = %req.uri().path(), "failed to resolve JWK");
        StatusCode::UNAUTHORIZED
    })?;

    // 4. 서명 및 클레임 최종 검증
    let claims = verify_jwt(jwt, &jwk, state.cf_audience.as_deref()).map_err(|e| {
        tracing::warn!(error = %e, path = %req.uri().path(), "JWT verification failed");
        StatusCode::UNAUTHORIZED
    })?;

    // 검증된 사용자 정보를 요청 확장에 추가.
    let user = VerifiedUser {
        email: claims.email.unwrap_or_default(),
        audience: claims.aud,
        expires_at: claims.exp,
    };

    tracing::debug!(email = %user.email, path = %req.uri().path(), "CF Access verified");

    let mut req = req;
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

/// JWT 페이로드를 파싱. 서명 검증은 하지 않음 (unsafe).
/// Phase 5에서 `jsonwebtoken`로 교체 예정.
fn parse_jwt_unsafe(jwt: &str) -> Result<CfAccessClaims, String> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return Err(format!("expected 3 JWT parts, got {}", parts.len()));
    }
    let payload_b64 = parts[1];
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|e| format!("base64 decode: {e}"))?;
    let claims: CfAccessClaims =
        serde_json::from_slice(&payload_bytes).map_err(|e| format!("json decode: {e}"))?;
    Ok(claims)
}

#[allow(dead_code)]
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn fetch_jwk_set(iss: &str) -> Result<JwkSet, String> {
    let url = format!("{}/cdn-cgi/access/certs", iss);

    if !iss.starts_with("https://") || !iss.ends_with(".cloudflareaccess.com") {
        return Err(format!("untrusted issuer domain: {}", iss));
    }

    let client = reqwest::Client::new();
    let res = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("failed to fetch JWKS: {e}"))?;

    if !res.status().is_success() {
        return Err(format!("JWKS fetch returned status {}", res.status()));
    }

    let jwks = res
        .json::<JwkSet>()
        .await
        .map_err(|e| format!("failed to parse JWKS JSON: {e}"))?;

    Ok(jwks)
}

async fn get_jwk(iss: &str, kid: &str) -> Result<Jwk, String> {
    let cache = get_jwks_cache();

    {
        let r_lock = cache.read().await;
        if let Some((jwks, last_updated)) = r_lock.keys.get(iss) {
            if last_updated.elapsed() < Duration::from_secs(3600) {
                if let Some(jwk) = jwks.find(kid) {
                    return Ok(jwk.clone());
                }
            }
        }
    }

    let mut w_lock = cache.write().await;

    if let Some((jwks, last_updated)) = w_lock.keys.get(iss) {
        if last_updated.elapsed() < Duration::from_secs(3600) {
            if let Some(jwk) = jwks.find(kid) {
                return Ok(jwk.clone());
            }
        }
    }

    let jwks = fetch_jwk_set(iss).await?;
    let jwk = jwks
        .find(kid)
        .ok_or_else(|| format!("key id {} not found in JWKS for issuer {}", kid, iss))?
        .clone();

    w_lock.keys.insert(iss.to_string(), (jwks, Instant::now()));

    Ok(jwk)
}

fn verify_jwt(jwt: &str, jwk: &Jwk, expected_aud: Option<&str>) -> Result<CfAccessClaims, String> {
    let decoding_key = DecodingKey::from_jwk(jwk)
        .map_err(|e| format!("failed to create decoding key from JWK: {e}"))?;

    let mut validation = Validation::new(jsonwebtoken::Algorithm::RS256);
    if let Some(aud) = expected_aud {
        validation.set_audience(&[aud]);
    } else {
        validation.validate_aud = false;
    }

    let token_data = decode::<CfAccessClaims>(jwt, &decoding_key, &validation)
        .map_err(|e| format!("JWT validation failed: {e}"))?;

    Ok(token_data.claims)
}

/// 통합 테스트용 JWKS 모킹 API.
#[allow(dead_code)]
pub async fn setup_test_jwks_for_testing(iss: &str, jwk_json: &str) {
    let jwk: Jwk = serde_json::from_str(jwk_json).unwrap();
    let jwks = JwkSet { keys: vec![jwk] };
    let cache = get_jwks_cache();
    let mut w_lock = cache.write().await;
    w_lock.keys.insert(iss.to_string(), (jwks, Instant::now()));
}

/// 인증 실패 응답 JSON.
impl IntoResponse for VerifiedUser {
    fn into_response(self) -> Response {
        // 실제로는 extensions에서 추출하므로 IntoResponse는 사용되지 않음.
        // 이 impl은 디버깅 편의를 위함.
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PRIVATE_KEY_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----\n\
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQDC5lIpyZgLqiHI\n\
Gci6HavUD0dF2yL48wefcDc+xrlDk0CSg9iFCWsffagYSg2RH/fue4SwKriySovd\n\
bi1PDIf41LT+01XioPRG3JB9wutkOlrRLsQiUeNF5jMykMGFx2ij8FKPQ5i9tuU4\n\
897RkA8iGPfG5Bj4wNrMlG6oTQOaesvkbTPfEq7OHCg0DDS4d3ErulHcSkyCM3ye\n\
SuoKSAGWjIs2N4OwehR4gUEaR8kJJtqK7cC9N1NiFy7P5qJf3mwnerimNfqJFYih\n\
lfbzWOtSbnLVskkpOL8BpQPw1EuFDuDYArjWXCLH8VOCiexDM8aRPLuensNKyrr3\n\
CENBK4+zAgMBAAECggEAF4rz3NldtPcBqqa2sQi5g12vGdidMl5FCvTmr34Yzflh\n\
IPGtO3DGUGEA56I2XlPywouIHTGj6uGHYKGY9oYIfL3Q+UE1DDGuIEsvZwmfHpXP\n\
95nDFnQ21HA4ugBzaAIM+VSj3GtpbW1E5irLPRw+P7utXoiaHZ5KL9E0Rr860rOR\n\
qHntlD1aKoAmSR1BbkHpqEqXLM9wWdIbMEK9RFjssv5dO3E8Tvq7czcT71Nxvkaa\n\
RmpvBMBgAf3a4oK2vsjgRHOsLkbqw4GMQnnvvp48UHM9r+CeopG1MEkMKIPPGKeM\n\
x9X32xc8J6sLZDlv6KV96Ap+SlceKDAzHWXvAD4SOQKBgQDkMUsZy6HCg2ALt6xI\n\
qtRdqjsEz/s1Ww0YiLriL/kxJWf7UDT8jwiGlGesVqFFBekwGqIyAPGEnH51zDRW\n\
tbav/pms8Zko2t03zbpWrjGlxLlcuCxwRvdNBQsuF/2a/D0ozrt6hdZK4vV5QTcJ\n\
qaZjx/NQW5/IysaabIgk/TRgqwKBgQDapmv5os/N6mQgtqloW7iVWb8We1HqLeCn\n\
oB11O0xOeugdsEg5KG1aSDMnvc0f2zIixBdRg7fBul0cy9SIYN/c+L8isvnzJwG4\n\
rcIKOOXrdIfj6FFUpkpxY4HoSycgJgoeHQMmi2gRlnY7Y23h925rxml4jsxSZlQe\nzsu139BdGQKBgAOtR6iCv3iC5WlK7Fu/ZOydcZYCQ+n4LZ3XlitO2pUQJTzHbhMj\nut9wRLtiKfcSwU8lHrfvi/S3ENKVF8LN6sOrNo6y1eTyod3kUrxS0jn5kYMM9Kpa\nemGjUyrK+CsnJVUi/6JZxbovLgVmJ5zgPu4cqq8AyvJRUiHq3ca6zb1BAoGAPjHs\nsNvhJH+x76RF2AuPG9ylgG2fxW87YjMnbftqH0DS2e8U/D1FrdKvynQw7wjY4A7L\nW0KOeKrcZZ6NXCXCSAbxx5sFgmbsFG5IrcO1kx5YsTmaOOv8bPiTMVJ/VKO9aQdz\np/krpyUXiJkl3osVe866nbJw6Fd3QjQsuhVqHbECgYBPZhAYIkBx4L+1oL8xePxR\nGXwx1U3FCY9dNabwUi9LTWyJtT2tF6VN6yDMF3dUjtCr+jtkuhowsM5aARqHwQwX\n/ZLi6VHS55zn501BE5QG0grXPzOLIYXsAIc866BVMFXxSpr0A4aWRmAMrMHbKCZM\nbZKe5K+40FcNrD8AbIOSJw==\n-----END PRIVATE KEY-----";

    const TEST_JWK_JSON: &str = r#"{
        "kty": "RSA",
        "kid": "test-kid",
        "alg": "RS256",
        "n": "wuZSKcmYC6ohyBnIuh2r1A9HRdsi-PMHn3A3Psa5Q5NAkoPYhQlrH32oGEoNkR_37nuEsCq4skqL3W4tTwyH-NS0_tNV4qD0RtyQfcLrZDpa0S7EIlHjReYzMpDBhcdoo_BSj0OYvbblOPPe0ZAPIhj3xuQY-MDazJRuqE0DmnrL5G0z3xKuzhwoNAw0uHdxK7pR3EpMgjN8nkrqCkgBloyLNjeDsHoUeIFBGkfJCSbaiu3AvTdTYhcuz-aiX95sJ3q4pjX6iRWIoZX281jrUm5y1bJJKTi_AaUD8NRLhQ7g2AK41lwix_FTgonsQzPGkTy7np7DSsq69whDQSuPsw",
        "e": "AQAB"
    }"#;

    async fn setup_test_jwks(iss: &str) {
        let jwk: Jwk = serde_json::from_str(TEST_JWK_JSON).unwrap();
        let jwks = JwkSet { keys: vec![jwk] };
        let cache = get_jwks_cache();
        let mut w_lock = cache.write().await;
        w_lock.keys.insert(iss.to_string(), (jwks, Instant::now()));
    }

    fn make_jwt(iss: &str, aud: &str, exp: u64, email: Option<&str>) -> String {
        let claims = CfAccessClaims {
            exp,
            aud: aud.to_string(),
            iss: Some(iss.to_string()),
            email: email.map(|s| s.to_string()),
        };
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
        header.kid = Some("test-kid".to_string());
        let encoding_key = jsonwebtoken::EncodingKey::from_rsa_pem(TEST_PRIVATE_KEY_PEM).unwrap();
        jsonwebtoken::encode(&header, &claims, &encoding_key).unwrap()
    }

    #[tokio::test]
    async fn parses_valid_jwt_payload() {
        let iss = "https://test.cloudflareaccess.com";
        setup_test_jwks(iss).await;
        let jwt = make_jwt(iss, "abc123", unix_now() + 3600, Some("user@example.com"));

        // parse_jwt_unsafe는 여전히 정상 동작해야 함
        let claims = parse_jwt_unsafe(&jwt).unwrap();
        assert_eq!(claims.aud, "abc123");
        assert_eq!(claims.email.as_deref(), Some("user@example.com"));
        assert_eq!(claims.iss.as_deref(), Some(iss));

        // 진짜 검증 함수 verify_jwt 도 정상 통과해야 함
        let jwk = get_jwk(iss, "test-kid").await.unwrap();
        let verified = verify_jwt(&jwt, &jwk, Some("abc123")).unwrap();
        assert_eq!(verified.aud, "abc123");
        assert_eq!(verified.email.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn rejects_malformed_jwt() {
        assert!(parse_jwt_unsafe("not.a.jwt.format").is_err());
        assert!(parse_jwt_unsafe("onlyonepart").is_err());
        assert!(parse_jwt_unsafe("").is_err());
    }

    #[test]
    fn rejects_invalid_base64() {
        let jwt = "header.!!!.sig";
        assert!(parse_jwt_unsafe(jwt).is_err());
    }

    #[test]
    fn rejects_payload_missing_claims() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{\"foo\":\"bar\"}");
        let jwt = format!("header.{payload}.sig");
        let result = parse_jwt_unsafe(&jwt);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn parses_jwt_without_email() {
        let iss = "https://test.cloudflareaccess.com";
        setup_test_jwks(iss).await;
        let jwt = make_jwt(iss, "aud", unix_now() + 100, None);
        let claims = parse_jwt_unsafe(&jwt).unwrap();
        assert!(claims.email.is_none());

        let jwk = get_jwk(iss, "test-kid").await.unwrap();
        let verified = verify_jwt(&jwt, &jwk, Some("aud")).unwrap();
        assert!(verified.email.is_none());
    }
}
