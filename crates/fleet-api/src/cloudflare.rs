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

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;

use crate::app::AppState;

/// Cloudflare Access JWT의 페이로드 (검증에 필요한 클레임만).
#[derive(Debug, Deserialize)]
struct CfAccessClaims {
    /// 만료 시각 (Unix epoch 초).
    exp: u64,
    /// 청중(Cloudflare Access Application AUD).
    aud: String,
    /// 이메일 (있는 경우).
    #[serde(default)]
    email: Option<String>,
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

    // JWT 페이로드 파싱 (서명 검증 없이 — Phase 5에서 추가).
    let claims = parse_jwt_unsafe(jwt).map_err(|e| {
        tracing::warn!(error = %e, path = %req.uri().path(), "invalid CF Access JWT");
        StatusCode::UNAUTHORIZED
    })?;

    // 만료 검증.
    let now = unix_now();
    if claims.exp <= now {
        tracing::warn!(exp = claims.exp, now, "CF Access JWT expired");
        return Err(StatusCode::UNAUTHORIZED);
    }

    // AUD 검증 (설정된 경우).
    if let Some(expected_aud) = &state.cf_audience {
        if &claims.aud != expected_aud {
            tracing::warn!(
                expected = %expected_aud,
                actual = %claims.aud,
                "CF Access JWT audience mismatch"
            );
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

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

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    fn make_jwt(aud: &str, exp: u64, email: Option<&str>) -> String {
        // 헤더 (미사용이지만 형식상 필요).
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"RS256\",\"typ\":\"JWT\"}");
        // 페이로드.
        let email_json = email
            .map(|e| format!(",\"email\":\"{e}\""))
            .unwrap_or_default();
        let payload_str = format!("{{\"exp\":{exp},\"aud\":\"{aud}\"{email_json}}}");
        let payload = URL_SAFE_NO_PAD.encode(payload_str.as_bytes());
        // 서명 (가짜).
        let sig = URL_SAFE_NO_PAD.encode(b"fakesig");
        format!("{header}.{payload}.{sig}")
    }

    #[test]
    fn parses_valid_jwt_payload() {
        let jwt = make_jwt("abc123", unix_now() + 3600, Some("user@example.com"));
        let claims = parse_jwt_unsafe(&jwt).unwrap();
        assert_eq!(claims.aud, "abc123");
        assert_eq!(claims.email.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn rejects_malformed_jwt() {
        assert!(parse_jwt_unsafe("not.a.jwt.format").is_err());
        assert!(parse_jwt_unsafe("onlyonepart").is_err());
        assert!(parse_jwt_unsafe("").is_err());
    }

    #[test]
    fn rejects_invalid_base64() {
        // 세 부분이지만 base64가 아님.
        let jwt = "header.!!!.sig";
        assert!(parse_jwt_unsafe(jwt).is_err());
    }

    #[test]
    fn rejects_payload_missing_claims() {
        let payload = URL_SAFE_NO_PAD.encode(b"{\"foo\":\"bar\"}");
        let jwt = format!("header.{payload}.sig");
        let result = parse_jwt_unsafe(&jwt);
        assert!(result.is_err()); // exp, aud 필수 필드 누락
    }

    #[test]
    fn parses_jwt_without_email() {
        let jwt = make_jwt("aud", unix_now() + 100, None);
        let claims = parse_jwt_unsafe(&jwt).unwrap();
        assert!(claims.email.is_none());
    }
}
