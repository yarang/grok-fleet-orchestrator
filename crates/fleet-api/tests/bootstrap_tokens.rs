//! Phase 8.3 — bootstrap token + worker join 엔드포인트 통합 테스트.
//!
//! 검증 항목:
//! - POST /v1/bootstrap-tokens 로 토큰 발급
//! - GET /v1/bootstrap-tokens 로 목록 조회
//! - POST /v1/workers/join 로 토큰 소비 + worker 생성
//! - 토큰 재사용 시 거부 (단일 사용)
//! - DELETE /v1/bootstrap-tokens/:token 으로 회수

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use fleet_api::{build_app, AppState};
use fleet_core::{
    BootstrapToken, EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput, TaskStatus,
    Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
};
use fleet_store::{Store, StoreError};
use serde_json::json;
use tower::ServiceExt;

/// API 호출 헬퍼.
async fn api_call(
    store: Arc<dyn Store>,
    method: axum::http::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> (axum::http::StatusCode, serde_json::Value) {
    let state = Arc::new(AppState::new(store));
    let app = build_app(state);
    let req = if let Some(b) = body {
        let bytes = serde_json::to_vec(&b).unwrap();
        axum::http::Request::builder()
            .method(method.clone())
            .uri(path)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(bytes))
            .unwrap()
    } else {
        axum::http::Request::builder()
            .method(method.clone())
            .uri(path)
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, json)
}

#[tokio::test]
async fn create_token_returns_token_string() {
    let store = make_store();
    let body = json!({"prefix": "test", "bytes": 16, "max_uses": 1});
    let (status, json) = api_call(
        store,
        axum::http::Method::POST,
        "/v1/bootstrap-tokens",
        Some(body),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let token = json["token"].as_str().expect("token in response");
    assert!(token.starts_with("test_"));
    assert!(token.len() > "test_".len() + 10);
}

#[tokio::test]
async fn create_token_rejects_invalid_prefix() {
    let store = make_store();
    let body = json!({"prefix": "bad prefix!", "bytes": 16});
    let (status, json) = api_call(
        store,
        axum::http::Method::POST,
        "/v1/bootstrap-tokens",
        Some(body),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("prefix"));
}

#[tokio::test]
async fn create_token_persists_to_store() {
    let store = make_store();
    let body = json!({"prefix": "fleet", "bytes": 24, "max_uses": 3});
    let (_, json) = api_call(
        store.clone(),
        axum::http::Method::POST,
        "/v1/bootstrap-tokens",
        Some(body),
    )
    .await;
    let token = json["token"].as_str().unwrap();

    let tokens = store.list_bootstrap_tokens().await.unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token, token);
    assert_eq!(tokens[0].max_uses, 3);
    assert_eq!(tokens[0].use_count, 0);
}

#[tokio::test]
async fn list_tokens_returns_all() {
    let store = make_store();
    seed_token(&store, "alpha-1", 1).await;
    seed_token(&store, "beta-2", 5).await;

    let (status, json) = api_call(
        store,
        axum::http::Method::GET,
        "/v1/bootstrap-tokens",
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 2);
}

#[tokio::test]
async fn revoke_token_removes_it() {
    let store = make_store();
    seed_token(&store, "doomed", 1).await;

    let (status, _) = api_call(
        store.clone(),
        axum::http::Method::DELETE,
        "/v1/bootstrap-tokens/doomed",
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let tokens = store.list_bootstrap_tokens().await.unwrap();
    assert!(tokens.is_empty());
}

#[tokio::test]
async fn revoke_unknown_token_returns_404() {
    let store = make_store();
    let (status, json) = api_call(
        store,
        axum::http::Method::DELETE,
        "/v1/bootstrap-tokens/nonexistent",
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not found"));
}

#[tokio::test]
async fn join_with_valid_token_creates_worker() {
    let store = make_store();
    seed_token(&store, "valid-token", 1).await;

    let body = json!({
        "token": "valid-token",
        "name": "worker-via-join",
        "agent_endpoint": "ws://localhost:2419/ws?server-key=secret",
        "labels": {"arch": "arm64"},
        "max_concurrent_tasks": 2,
    });
    let (status, json) = api_call(
        store.clone(),
        axum::http::Method::POST,
        "/v1/workers/join",
        Some(body),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(json["worker_id"].as_str().is_some());
    assert!(json["worker_config_toml"].as_str().is_some());

    // Worker가 실제로 store에 존재하는지 확인.
    let worker = store
        .get_worker_by_name("worker-via-join")
        .await
        .unwrap()
        .expect("worker created");
    assert_eq!(worker.labels.get("arch").unwrap(), "arm64");
    assert_eq!(worker.max_concurrent, 2);

    // 토큰이 소비되었는지 확인.
    let tokens = store.list_bootstrap_tokens().await.unwrap();
    assert_eq!(tokens[0].use_count, 1);
    assert_eq!(tokens[0].last_used_by.as_deref(), Some("worker-via-join"));
}

#[tokio::test]
async fn join_with_invalid_token_returns_401() {
    let store = make_store();
    let body = json!({
        "token": "no-such-token",
        "name": "x",
        "agent_endpoint": "ws://h/ws",
    });
    let (status, json) = api_call(
        store,
        axum::http::Method::POST,
        "/v1/workers/join",
        Some(body),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("token"));
}

#[tokio::test]
async fn join_with_exhausted_token_returns_401() {
    let store = make_store();
    // 직접 소비된 상태로 시드.
    store
        .create_bootstrap_token(&BootstrapToken {
            token: "used-up".into(),
            created_at: Utc::now(),
            created_by: None,
            expires_at: None,
            max_uses: 1,
            use_count: 1,
            notes: None,
            last_used_by: Some("prev".into()),
            last_used_at: Some(Utc::now()),
        })
        .await
        .unwrap();

    let body = json!({"token": "used-up", "name": "x", "agent_endpoint": "ws://h/ws"});
    let (status, _) = api_call(
        store,
        axum::http::Method::POST,
        "/v1/workers/join",
        Some(body),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn join_with_expired_token_returns_401() {
    let store = make_store();
    let past = Utc::now() - chrono::Duration::seconds(3600);
    store
        .create_bootstrap_token(&BootstrapToken {
            token: "expired".into(),
            created_at: Utc::now(),
            created_by: None,
            expires_at: Some(past),
            max_uses: 10,
            use_count: 0,
            notes: None,
            last_used_by: None,
            last_used_at: None,
        })
        .await
        .unwrap();

    let body = json!({"token": "expired", "name": "x", "agent_endpoint": "ws://h/ws"});
    let (status, _) = api_call(
        store,
        axum::http::Method::POST,
        "/v1/workers/join",
        Some(body),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn join_with_duplicate_name_returns_409() {
    let store = make_store();
    seed_token(&store, "tok", 1).await;

    // 먼저 worker를 만들어둠.
    let existing = fleet_core::Worker::new("dup-name", "ws://h/ws");
    store.upsert_worker(&existing).await.unwrap();

    let body = json!({
        "token": "tok",
        "name": "dup-name",
        "agent_endpoint": "ws://h/ws",
    });
    let (status, json) = api_call(
        store,
        axum::http::Method::POST,
        "/v1/workers/join",
        Some(body),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("already exists"));
}

#[tokio::test]
async fn join_response_config_toml_contains_required_fields() {
    let store = make_store();
    seed_token(&store, "tok", 1).await;

    let body = json!({
        "token": "tok",
        "name": "w1",
        "agent_endpoint": "ws://host:2419/ws?server-key=sekret",
        "labels": {"gpu": "true"},
        "max_concurrent_tasks": 8,
    });
    let (_, json) = api_call(
        store,
        axum::http::Method::POST,
        "/v1/workers/join",
        Some(body),
    )
    .await;
    let toml = json["worker_config_toml"].as_str().unwrap();
    assert!(toml.contains("name = \"w1\""));
    assert!(toml.contains("existing_worker_id = "));
    assert!(toml.contains("bootstrap_token = \"tok\""));
    assert!(toml.contains("secret = \"sekret\""));
    assert!(toml.contains("bind_addr = \"host:2419\""));
    assert!(toml.contains("max_concurrent_tasks = 8"));
    assert!(toml.contains("gpu = \"true\""));
}

#[tokio::test]
async fn multi_use_token_supports_multiple_joins() {
    let store = make_store();
    seed_token(&store, "multi", 3).await;

    for i in 0..3 {
        let body = json!({
            "token": "multi",
            "name": format!("w-{i}"),
            "agent_endpoint": format!("ws://h-{i}/ws"),
        });
        let (status, _) = api_call(
            store.clone(),
            axum::http::Method::POST,
            "/v1/workers/join",
            Some(body),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::OK, "join {i} should succeed");
    }

    // 4번째는 거부.
    let body = json!({"token": "multi", "name": "w-4", "agent_endpoint": "ws://h/ws"});
    let (status, _) = api_call(
        store,
        axum::http::Method::POST,
        "/v1/workers/join",
        Some(body),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

// ── 픽스처 ──────────────────────────────────────────────────────────────

async fn seed_token(store: &Arc<dyn Store>, token: &str, max_uses: u32) {
    store
        .create_bootstrap_token(&BootstrapToken {
            token: token.into(),
            created_at: Utc::now(),
            created_by: Some("test".into()),
            expires_at: None,
            max_uses,
            use_count: 0,
            notes: None,
            last_used_by: None,
            last_used_at: None,
        })
        .await
        .expect("seed token");
}

fn make_store() -> Arc<dyn Store> {
    Arc::new(BsStore::default())
}

/// 테스트용 Store — BootstrapToken을 실제로 저장/조회하는 minimal 구현.
#[derive(Default)]
struct BsStore {
    workers: Mutex<HashMap<WorkerId, Worker>>,
    tokens: Mutex<HashMap<String, BootstrapToken>>,
    events: Mutex<Vec<EventEntry>>,
}

#[async_trait]
impl Store for BsStore {
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
    async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError> {
        Ok(self
            .workers
            .lock()
            .unwrap()
            .values()
            .find(|w| w.name == name)
            .cloned())
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
            w.last_seen = Some(Utc::now());
        }
        Ok(())
    }
    async fn append_event(&self, e: &FleetEvent) -> Result<u64, StoreError> {
        let mut events = self.events.lock().unwrap();
        let seq = (events.len() + 1) as u64;
        events.push(EventEntry {
            seq,
            event: e.clone(),
        });
        Ok(seq)
    }
    async fn list_events(&self, _: u64, _: u32) -> Result<Vec<EventEntry>, StoreError> {
        Ok(self.events.lock().unwrap().clone())
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
    async fn create_bootstrap_token(&self, t: &BootstrapToken) -> Result<(), StoreError> {
        let mut tokens = self.tokens.lock().unwrap();
        if tokens.contains_key(&t.token) {
            return Err(StoreError::Conflict("exists".into()));
        }
        tokens.insert(t.token.clone(), t.clone());
        Ok(())
    }
    async fn consume_bootstrap_token(
        &self,
        token: &str,
        used_by: &str,
    ) -> Result<(), StoreError> {
        let mut tokens = self.tokens.lock().unwrap();
        let entry = tokens.get_mut(token).ok_or_else(|| {
            StoreError::BootstrapTokenInvalid(format!("token not found: {token}"))
        })?;
        if !entry.is_usable() {
            return Err(StoreError::BootstrapTokenInvalid(format!(
                "exhausted/expired: {token}"
            )));
        }
        entry.use_count += 1;
        entry.last_used_by = Some(used_by.to_string());
        entry.last_used_at = Some(Utc::now());
        Ok(())
    }
    async fn list_bootstrap_tokens(&self) -> Result<Vec<BootstrapToken>, StoreError> {
        Ok(self
            .tokens
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect())
    }
    async fn revoke_bootstrap_token(&self, token: &str) -> Result<bool, StoreError> {
        Ok(self.tokens.lock().unwrap().remove(token).is_some())
    }
}
