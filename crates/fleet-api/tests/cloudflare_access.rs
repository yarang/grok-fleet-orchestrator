//! Cloudflare Access 미들웨어 통합 테스트.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use fleet_api::{build_app, AppState};
use fleet_core::{
    BootstrapToken, EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput, TaskStatus,
    Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
};
use fleet_store::{Store, StoreError};

struct MemStore {
    workers: Mutex<HashMap<WorkerId, Worker>>,
}

impl MemStore {
    fn new() -> Self {
        Self {
            workers: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl Store for MemStore {
    async fn insert_task(&self, _: &Task) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn get_task(&self, _: TaskId) -> Result<Option<Task>, StoreError> {
        unimplemented!()
    }
    async fn update_task_status(&self, _: TaskId, _: &TaskStatus) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn list_tasks(&self, _: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        unimplemented!()
    }
    async fn upsert_worker(&self, w: &Worker) -> Result<(), StoreError> {
        self.workers.lock().unwrap().insert(w.id, w.clone());
        Ok(())
    }
    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
        Ok(self.workers.lock().unwrap().get(&id).cloned())
    }
    async fn get_worker_by_name(&self, _: &str) -> Result<Option<Worker>, StoreError> {
        Ok(None)
    }
    async fn list_workers(&self, _: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
        Ok(self.workers.lock().unwrap().values().cloned().collect())
    }
    async fn delete_worker(&self, id: WorkerId) -> Result<(), StoreError> {
        self.workers.lock().unwrap().remove(&id);
        Ok(())
    }
    async fn update_worker_heartbeat(
        &self,
        id: WorkerId,
        _: &WorkerHeartbeat,
    ) -> Result<(), StoreError> {
        if let Some(w) = self.workers.lock().unwrap().get_mut(&id) {
            w.last_seen = Some(chrono::Utc::now());
        }
        Ok(())
    }
    async fn append_event(&self, _: &FleetEvent) -> Result<u64, StoreError> {
        Ok(1)
    }
    async fn list_events(&self, _: u64, _: u32) -> Result<Vec<EventEntry>, StoreError> {
        Ok(vec![])
    }
    async fn append_output(&self, _: TaskId, _: &str) -> Result<u64, StoreError> {
        unimplemented!()
    }
    async fn get_output(&self, _: TaskId, _: u64) -> Result<TaskOutput, StoreError> {
        unimplemented!()
    }
    async fn migrate(&self) -> Result<(), StoreError> {
        Ok(())
    }
    async fn create_bootstrap_token(&self, _: &BootstrapToken) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn consume_bootstrap_token(&self, _: &str, _: &str) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn list_bootstrap_tokens(&self) -> Result<Vec<BootstrapToken>, StoreError> {
        unimplemented!()
    }
    async fn revoke_bootstrap_token(&self, _: &str) -> Result<bool, StoreError> {
        unimplemented!()
    }
    async fn upsert_worker_credential(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: u32,
        _: Option<&str>,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn get_worker_credential(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<fleet_store::StoredCredential>, StoreError> {
        unimplemented!()
    }
    async fn list_worker_credentials(
        &self,
        _: &str,
    ) -> Result<Vec<fleet_store::StoredCredential>, StoreError> {
        unimplemented!()
    }
    async fn delete_worker_credential(&self, _: &str, _: &str) -> Result<bool, StoreError> {
        unimplemented!()
    }
}

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
