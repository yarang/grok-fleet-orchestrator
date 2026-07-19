//! Cloudflare Access 미들웨어 통합 테스트.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

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
}

fn make_jwt(aud: &str, exp: u64, email: Option<&str>) -> String {
    let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"RS256\",\"typ\":\"JWT\"}");
    let email_json = email
        .map(|e| format!(",\"email\":\"{e}\""))
        .unwrap_or_default();
    let payload_str = format!("{{\"exp\":{exp},\"aud\":\"{aud}\"{email_json}}}");
    let payload = URL_SAFE_NO_PAD.encode(payload_str.as_bytes());
    let sig = URL_SAFE_NO_PAD.encode(b"fakesig");
    format!("{header}.{payload}.{sig}")
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
    tokio::spawn(async move { let _ = axum::serve(listener, app).await;
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
    tokio::spawn(async move { let _ = axum::serve(listener, app).await;
    });

    let resp = reqwest::get(format!("http://{addr}/v1/workers"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn cf_access_accepts_valid_jwt() {
    let state = spawn_with_cf_audience("my-aud-123").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { let _ = axum::serve(listener, app).await;
    });

    let jwt = make_jwt("my-aud-123", unix_now() + 3600, Some("user@example.com"));
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
    let state = spawn_with_cf_audience("my-aud-123").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { let _ = axum::serve(listener, app).await;
    });

    let jwt = make_jwt("my-aud-123", unix_now() - 100, Some("user@example.com"));
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
    let state = spawn_with_cf_audience("correct-aud").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { let _ = axum::serve(listener, app).await;
    });

    let jwt = make_jwt("wrong-aud", unix_now() + 3600, Some("user@example.com"));
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
    tokio::spawn(async move { let _ = axum::serve(listener, app).await;
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
    tokio::spawn(async move { let _ = axum::serve(listener, app).await;
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
    let state = spawn_with_cf_audience("aud").await;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { let _ = axum::serve(listener, app).await;
    });

    let jwt = make_jwt("aud", unix_now() + 3600, None);
    // 대문자 헤더
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/v1/workers"))
        .header("CF-ACCESS-JWT-ASSERTION", jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}
