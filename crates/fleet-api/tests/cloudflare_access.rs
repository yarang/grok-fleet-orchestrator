//! Cloudflare Access 미들웨어 통합 테스트.

use std::sync::Arc;

use fleet_api::{build_app, AppState};
use fleet_core::PermissionKind;
use fleet_store::Store;

use fleet_store::mem::MemStore;

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

#[derive(serde::Serialize)]
struct CfAccessClaims {
    exp: u64,
    aud: String,
    iss: Option<String>,
    #[serde(default)]
    email: Option<String>,
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

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn spawn_no_auth() -> Arc<AppState> {
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    Arc::new(AppState::new(store))
}

async fn spawn_with_cf_audience(aud: &str) -> Arc<AppState> {
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    Arc::new(AppState::new(store).with_cf_audience(aud))
}

#[tokio::test]
async fn no_auth_mode_allows_all() {
    let state = spawn_no_auth().await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let resp = reqwest::get(format!("http://{addr}/v1/health"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn cf_access_rejects_missing_jwt() {
    let state = spawn_with_cf_audience("my-aud-123").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let resp = reqwest::get(format!("http://{addr}/v1/workers"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn cf_access_accepts_valid_jwt() {
    let iss = "https://test.cloudflareaccess.com";
    fleet_api::setup_test_jwks_for_testing(iss, TEST_JWK_JSON).await;

    let state = spawn_with_cf_audience("my-aud-123").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let jwt = make_jwt(
        iss,
        "my-aud-123",
        unix_now() + 3600,
        Some("user@example.com"),
    );
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/v1/workers"))
        .header("cf-access-jwt-assertion", jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn cf_access_rejects_expired_jwt() {
    let iss = "https://test.cloudflareaccess.com";
    fleet_api::setup_test_jwks_for_testing(iss, TEST_JWK_JSON).await;

    let state = spawn_with_cf_audience("my-aud-123").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let jwt = make_jwt(
        iss,
        "my-aud-123",
        unix_now() - 100,
        Some("user@example.com"),
    );
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/v1/workers"))
        .header("cf-access-jwt-assertion", jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn cf_access_rejects_wrong_audience() {
    let iss = "https://test.cloudflareaccess.com";
    fleet_api::setup_test_jwks_for_testing(iss, TEST_JWK_JSON).await;

    let state = spawn_with_cf_audience("correct-aud").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let jwt = make_jwt(
        iss,
        "wrong-aud",
        unix_now() + 3600,
        Some("user@example.com"),
    );
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/v1/workers"))
        .header("cf-access-jwt-assertion", jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn cf_access_allows_health_without_jwt() {
    let state = spawn_with_cf_audience("aud").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // /v1/health는 CF 인증 없이 허용.
    let resp = reqwest::get(format!("http://{addr}/v1/health"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn cf_access_rejects_malformed_jwt() {
    let state = spawn_with_cf_audience("aud").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/v1/workers"))
        .header("cf-access-jwt-assertion", "not-a-jwt")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

// ── CF Access principal → capability authorization (로드맵 #58) ─────────

/// CF Access 전용 배포에서 endpoint capability 검사가 실제로 평가되는지
/// 확인하기 위한 서버. principal(JWT email)별 capability를 명시한다.
async fn spawn_with_cf_capabilities(
    aud: &str,
    mapping: Vec<(String, Vec<PermissionKind>)>,
) -> std::net::SocketAddr {
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    let state = Arc::new(
        AppState::new(store)
            .with_cf_audience(aud)
            .with_cf_principal_capabilities(mapping),
    );
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

/// CF JWT를 붙여 GET 요청.
async fn cf_get(addr: std::net::SocketAddr, path: &str, jwt: &str) -> reqwest::StatusCode {
    reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .header("cf-access-jwt-assertion", jwt)
        .send()
        .await
        .unwrap()
        .status()
}

/// CF JWT를 붙여 POST 요청.
async fn cf_post(
    addr: std::net::SocketAddr,
    path: &str,
    jwt: &str,
    body: serde_json::Value,
) -> reqwest::StatusCode {
    reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .header("cf-access-jwt-assertion", jwt)
        .json(&body)
        .send()
        .await
        .unwrap()
        .status()
}

/// 로드맵 #58 — CF Access 세션도 `authorize_http_endpoint`를 통과해야 한다.
///
/// 이전에는 CF 전용 배포(`valid_tokens == None` + `cf_audience` 설정)에서
/// auth_middleware가 `AuthorizationContext`를 만들지 않고 그대로 통과시켜
/// capability 검사가 한 번도 수행되지 않았다.
#[tokio::test]
async fn cf_access_session_is_subject_to_endpoint_capability_check() {
    let iss = "https://test.cloudflareaccess.com";
    fleet_api::setup_test_jwks_for_testing(iss, TEST_JWK_JSON).await;

    // worker 목록 조회만 허용된 principal.
    let addr = spawn_with_cf_capabilities(
        "cap-aud",
        vec![(
            "reader@example.com".to_string(),
            vec![PermissionKind::WorkerList],
        )],
    )
    .await;
    let jwt = make_jwt(
        iss,
        "cap-aud",
        unix_now() + 3600,
        Some("reader@example.com"),
    );

    // 허용된 capability는 통과.
    assert_eq!(cf_get(addr, "/v1/workers", &jwt).await, 200);

    // 허용되지 않은 capability는 403 — 401(인증 실패)이 아니라 인가 실패여야
    // 한다. 즉 principal은 인식됐고 capability만 부족한 상태.
    assert_eq!(
        cf_post(
            addr,
            "/v1/bootstrap-tokens",
            &jwt,
            serde_json::json!({"prefix": "fleet", "bytes": 16}),
        )
        .await,
        403
    );
}

/// principal_id는 CF JWT의 `email` 클레임이다. 매핑 조회가 그 이메일로
/// 이뤄지므로, 이메일이 다르면 권한도 달라진다.
#[tokio::test]
async fn cf_access_principal_is_the_jwt_email() {
    let iss = "https://test.cloudflareaccess.com";
    fleet_api::setup_test_jwks_for_testing(iss, TEST_JWK_JSON).await;

    let addr = spawn_with_cf_capabilities(
        "principal-aud",
        vec![(
            "ops@example.com".to_string(),
            vec![PermissionKind::WorkerList],
        )],
    )
    .await;

    // 매핑에 있는 이메일 → 통과.
    let allowed = make_jwt(
        iss,
        "principal-aud",
        unix_now() + 3600,
        Some("ops@example.com"),
    );
    assert_eq!(cf_get(addr, "/v1/workers", &allowed).await, 200);

    // 대소문자만 다른 같은 이메일 → 동일하게 통과 (정규화).
    let mixed_case = make_jwt(
        iss,
        "principal-aud",
        unix_now() + 3600,
        Some("OPS@Example.com"),
    );
    assert_eq!(cf_get(addr, "/v1/workers", &mixed_case).await, 200);

    // 매핑에 없는 이메일 → 인증은 됐지만 capability 없음(403).
    let stranger = make_jwt(
        iss,
        "principal-aud",
        unix_now() + 3600,
        Some("stranger@example.com"),
    );
    assert_eq!(cf_get(addr, "/v1/workers", &stranger).await, 403);

    // email 클레임이 없는 세션도 매핑에 없으므로 403.
    let anonymous = make_jwt(iss, "principal-aud", unix_now() + 3600, None);
    assert_eq!(cf_get(addr, "/v1/workers", &anonymous).await, 403);
}

/// principal capability 매핑을 설정하지 않은 배포는 기존 동작(전권)을 유지한다.
///
/// **임시 정책이며 최소 권한이 아니다** — cf. `app::cf_access_capabilities`
/// 주석과 docs/roadmap/roadmap.md의 #58 행.
#[tokio::test]
async fn cf_access_without_capability_mapping_keeps_full_access() {
    let iss = "https://test.cloudflareaccess.com";
    fleet_api::setup_test_jwks_for_testing(iss, TEST_JWK_JSON).await;

    let state = spawn_with_cf_audience("legacy-aud").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let jwt = make_jwt(
        iss,
        "legacy-aud",
        unix_now() + 3600,
        Some("any@example.com"),
    );
    assert_eq!(cf_get(addr, "/v1/workers", &jwt).await, 200);
    assert_eq!(
        cf_post(
            addr,
            "/v1/bootstrap-tokens",
            &jwt,
            serde_json::json!({"prefix": "fleet", "bytes": 16}),
        )
        .await,
        200
    );
}

#[tokio::test]
async fn cf_access_case_insensitive_header() {
    let iss = "https://test.cloudflareaccess.com";
    fleet_api::setup_test_jwks_for_testing(iss, TEST_JWK_JSON).await;

    let state = spawn_with_cf_audience("aud").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let jwt = make_jwt(iss, "aud", unix_now() + 3600, None);
    // 대문자 헤더
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/v1/workers"))
        .header("CF-ACCESS-JWT-ASSERTION", jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}
