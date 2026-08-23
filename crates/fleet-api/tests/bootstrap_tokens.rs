//! Phase 8.3 — bootstrap token + worker join 엔드포인트 통합 테스트.
//!
//! 검증 항목:
//! - POST /v1/bootstrap-tokens 로 토큰 발급
//! - GET /v1/bootstrap-tokens 로 목록 조회
//! - POST /v1/workers/join 로 토큰 소비 + worker 생성
//! - 토큰 재사용 시 거부 (단일 사용)
//! - DELETE /v1/bootstrap-tokens/:token_id 으로 공개 식별자 회수

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use fleet_api::{build_app, AppState};
use fleet_core::{
    AuditEvent, AuditFilter, BootstrapToken, EventEntry, FleetEvent, Task, TaskFilter, TaskId,
    TaskOutput, TaskStatus, Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
};
use fleet_store::{Store, StoreError, WorkerOperationalCredential};
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
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
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
    assert!(json["token_id"].as_str().unwrap().starts_with("bt_"));
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
    assert_eq!(tokens[0].token_digest, BootstrapToken::digest_for(token));
    assert_eq!(tokens[0].max_uses, 3);
    assert_eq!(tokens[0].use_count, 0);
}

#[tokio::test]
async fn list_tokens_returns_all() {
    let store = make_store();
    seed_token(&store, "alpha-1", 1).await;
    seed_token(&store, "beta-2", 5).await;

    let (status, json) =
        api_call(store, axum::http::Method::GET, "/v1/bootstrap-tokens", None).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(arr.iter().all(|entry| entry.get("token").is_none()));
    assert!(arr
        .iter()
        .all(|entry| entry["token_id"].as_str().unwrap().starts_with("bt_")));
}

#[tokio::test]
async fn revoke_token_removes_it() {
    let store = make_store();
    seed_token(&store, "doomed", 1).await;

    let (status, _) = api_call(
        store.clone(),
        axum::http::Method::DELETE,
        &format!(
            "/v1/bootstrap-tokens/{}",
            BootstrapToken::public_id_for("doomed")
        ),
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
        "/v1/bootstrap-tokens/bt_nonexistent",
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
    assert!(json["error"]["message"].as_str().unwrap().contains("token"));
}

#[tokio::test]
async fn join_with_exhausted_token_returns_401() {
    let store = make_store();
    // 직접 소비된 상태로 시드.
    store
        .create_bootstrap_token(&BootstrapToken {
            token_digest: BootstrapToken::digest_for("used-up"),
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
            token_digest: BootstrapToken::digest_for("expired"),
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
    assert!(toml.contains("operational_token = \"fwo_"));
    assert!(!toml.contains("bootstrap_token"));
    assert!(toml.contains("secret = \"sekret\""));
    // bind_addr는 워커 자신의 로컬 리슨 주소이지 agent_endpoint(orchestrator가
    // 이 워커에 도달하는 공개 주소, 흔히 tunnel 뒤)에서 파생시킬 값이 아니다 —
    // 항상 고정 기본값을 쓴다 (기존 worker-ec1/ec2 배포와 동일).
    assert!(toml.contains("bind_addr = \"0.0.0.0:2419\""));
    assert!(toml.contains("max_concurrent_tasks = 8"));
    assert!(toml.contains("gpu = \"true\""));
    // AppState에 public_base_url을 설정하지 않았으므로 플레이스홀더가 남는다
    // (아래 join_response_uses_configured_public_base_url이 설정된 경우를 검증).
    assert!(toml.contains("orchestrator_url = \"<set-to-your-orchestrator-url>\""));
}

#[tokio::test]
async fn join_response_uses_configured_public_base_url() {
    // AppState::public_base_url(FLEET_BASE_URL env)이 설정되면 join 응답의
    // worker.toml에 실제 orchestrator_url이 채워져야 한다 — 플레이스홀더가
    // 아니라 이 값을 그대로 써야 fleet-worker가 즉시 register/heartbeat 가능.
    let store = make_store();
    seed_token(&store, "tok", 1).await;

    let state =
        Arc::new(AppState::new(store).with_public_base_url("https://fleet.agentthread.dev"));
    let app = build_app(state);

    let body = json!({
        "token": "tok",
        "name": "w2",
        "agent_endpoint": "wss://fleet.agentthread.dev/ws/w2?server-key=sekret",
        "max_concurrent_tasks": 4,
    });
    let req = axum::http::Request::builder()
        .method(axum::http::Method::POST)
        .uri("/v1/workers/join")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let toml = json["worker_config_toml"].as_str().unwrap();
    assert!(toml.contains("orchestrator_url = \"https://fleet.agentthread.dev\""));
    assert!(!toml.contains("<set-to-your-orchestrator-url>"));
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
        assert_eq!(
            status,
            axum::http::StatusCode::OK,
            "join {i} should succeed"
        );
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

// ── API↔CLI 라운드트립 (로드맵 #57) ─────────────────────────────────────

/// `fleet token issue`가 역직렬화하는 응답 형태(+ 공개 식별자).
///
/// 필드 집합이 `crates/fleet-cli/src/token.rs`의 `CreateTokenApiResponse`와
/// 같아야 CLI가 발급 응답을 읽을 수 있다.
#[derive(serde::Deserialize)]
struct CliCreateTokenResponse {
    token: String,
    token_id: String,
    #[allow(dead_code)]
    created_at: String,
    #[allow(dead_code)]
    expires_at: Option<String>,
    #[allow(dead_code)]
    max_uses: u32,
}

/// `fleet token list`가 역직렬화하는 목록 항목.
///
/// `crates/fleet-cli/src/token.rs`의 `TokenListItem`과 동일한 필드 집합.
/// `Option` 필드에 `#[serde(default)]`가 없으므로 서버가 해당 키를
/// 생략하면(예: `skip_serializing_if`) CLI 파싱이 깨진다 — 이 테스트가
/// 그 회귀를 잡는다.
#[derive(serde::Deserialize)]
struct CliTokenListItem {
    token_id: String,
    #[allow(dead_code)]
    created_at: String,
    #[allow(dead_code)]
    expires_at: Option<String>,
    #[allow(dead_code)]
    max_uses: u32,
    #[allow(dead_code)]
    use_count: u32,
    #[allow(dead_code)]
    remaining_uses: u32,
    #[allow(dead_code)]
    notes: Option<String>,
    #[allow(dead_code)]
    last_used_by: Option<String>,
    #[allow(dead_code)]
    last_used_at: Option<String>,
}

/// 로드맵 #57 완료 게이트 — API↔CLI E2E.
///
/// 발급 → 목록에서 `token_id` 노출 → 그 `token_id`로 회수까지가 CLI가 쓰는
/// 필드 이름 그대로 동작해야 한다. 원문 토큰은 발급 응답 이후 어떤 관리
/// API에도 다시 나타나지 않는다.
#[tokio::test]
async fn cli_roundtrip_issue_list_revoke_by_public_token_id() {
    let store = make_store();

    // 1) 발급 — CLI `token issue`.
    let (status, issued_json) = api_call(
        store.clone(),
        axum::http::Method::POST,
        "/v1/bootstrap-tokens",
        Some(json!({"prefix": "fleet", "bytes": 32, "max_uses": 1, "notes": "e2e"})),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let issued: CliCreateTokenResponse =
        serde_json::from_value(issued_json).expect("CLI must parse the issue response");
    assert!(issued.token.starts_with("fleet_"));
    assert_eq!(
        issued.token_id,
        BootstrapToken::public_id_for(&issued.token),
        "token_id must be the public identifier derived from the raw token"
    );

    // 2) 목록 — CLI `token list`. 원문은 어디에도 없어야 한다.
    let (status, list_json) = api_call(
        store.clone(),
        axum::http::Method::GET,
        "/v1/bootstrap-tokens",
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(
        !list_json.to_string().contains(&issued.token),
        "raw bootstrap token must never appear in the list response"
    );
    let items: Vec<CliTokenListItem> =
        serde_json::from_value(list_json).expect("CLI must parse the list response");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].token_id, issued.token_id);

    // 3) 원문을 경로에 넣은 회수는 거부 — 공개 식별자만 받는다.
    let (status, _) = api_call(
        store.clone(),
        axum::http::Method::DELETE,
        &format!("/v1/bootstrap-tokens/{}", issued.token),
        None,
    )
    .await;
    assert_eq!(
        status,
        axum::http::StatusCode::NOT_FOUND,
        "raw token must not be accepted as a revoke identifier"
    );

    // 4) 목록에서 얻은 token_id로 회수 — CLI `token revoke <TOKEN_ID>`.
    let (status, _) = api_call(
        store.clone(),
        axum::http::Method::DELETE,
        &format!("/v1/bootstrap-tokens/{}", items[0].token_id),
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let (_, list_json) = api_call(
        store.clone(),
        axum::http::Method::GET,
        "/v1/bootstrap-tokens",
        None,
    )
    .await;
    assert!(list_json.as_array().unwrap().is_empty());

    // 5) 회수된 토큰 원문으로는 join도 실패해야 한다.
    let (status, _) = api_call(
        store,
        axum::http::Method::POST,
        "/v1/workers/join",
        Some(json!({
            "token": issued.token,
            "name": "revoked-join",
            "agent_endpoint": "ws://h/ws",
        })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

/// 발급받은 원문 토큰이 join에 바로 쓰이고, 그 다음 목록의 `token_id`는
/// 여전히 원문에서 파생된 값과 같아야 한다(사용 이력이 붙어도 식별자 불변).
#[tokio::test]
async fn issued_token_joins_and_public_id_is_stable_after_use() {
    let store = make_store();
    let (_, issued_json) = api_call(
        store.clone(),
        axum::http::Method::POST,
        "/v1/bootstrap-tokens",
        Some(json!({"prefix": "fleet", "bytes": 16, "max_uses": 2})),
    )
    .await;
    let issued: CliCreateTokenResponse = serde_json::from_value(issued_json).unwrap();

    let (status, _) = api_call(
        store.clone(),
        axum::http::Method::POST,
        "/v1/workers/join",
        Some(json!({
            "token": issued.token,
            "name": "joined-worker",
            "agent_endpoint": "ws://h/ws",
        })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let (_, list_json) =
        api_call(store, axum::http::Method::GET, "/v1/bootstrap-tokens", None).await;
    let items: Vec<CliTokenListItem> = serde_json::from_value(list_json).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].token_id, issued.token_id);
    assert_eq!(items[0].use_count, 1);
    assert_eq!(items[0].last_used_by.as_deref(), Some("joined-worker"));
}

// ── 픽스처 ──────────────────────────────────────────────────────────────

async fn seed_token(store: &Arc<dyn Store>, token: &str, max_uses: u32) {
    store
        .create_bootstrap_token(&BootstrapToken {
            token_digest: BootstrapToken::digest_for(token),
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
    operational_credentials: Mutex<HashMap<String, WorkerOperationalCredential>>,
    events: Mutex<Vec<EventEntry>>,
    // 로드맵 #76 — create_bootstrap_token 등은 감사 기록 실패 시 방금 발급한
    // 토큰을 즉시 회수한다(fail-closed). `Store` 트레이트의 기본 구현은
    // `record_audit_event`를 `Unsupported`로 반환하므로, 여기서 실제로
    // 기록하지 않으면 이 테스트 파일의 발급 관련 테스트가 전부 500으로
    // 깨진다.
    audit_events: Mutex<Vec<AuditEvent>>,
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
    async fn increment_task_retry_count(&self, _: TaskId) -> Result<u32, StoreError> {
        unimplemented!()
    }
    async fn update_task_checkpoint(&self, _: TaskId, _: Option<&str>) -> Result<(), StoreError> {
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
        if tokens.contains_key(&t.token_digest) {
            return Err(StoreError::Conflict("exists".into()));
        }
        tokens.insert(t.token_digest.clone(), t.clone());
        Ok(())
    }
    async fn consume_bootstrap_token(&self, token: &str, used_by: &str) -> Result<(), StoreError> {
        let mut tokens = self.tokens.lock().unwrap();
        let token_digest = BootstrapToken::digest_for(token);
        let entry = tokens.get_mut(&token_digest).ok_or_else(|| {
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
        Ok(self.tokens.lock().unwrap().values().cloned().collect())
    }
    async fn revoke_bootstrap_token(&self, token_digest: &str) -> Result<bool, StoreError> {
        Ok(self.tokens.lock().unwrap().remove(token_digest).is_some())
    }
    async fn record_audit_event(&self, event: &AuditEvent) -> Result<(), StoreError> {
        self.audit_events.lock().unwrap().push(event.clone());
        Ok(())
    }
    async fn list_audit_events(&self, _: &AuditFilter) -> Result<Vec<AuditEvent>, StoreError> {
        Ok(self.audit_events.lock().unwrap().clone())
    }
    async fn upsert_worker_operational_credential(
        &self,
        credential: &WorkerOperationalCredential,
    ) -> Result<(), StoreError> {
        self.operational_credentials
            .lock()
            .unwrap()
            .insert(credential.credential_digest.clone(), credential.clone());
        Ok(())
    }
    async fn find_active_worker_operational_credential(
        &self,
        credential_digest: &str,
    ) -> Result<Option<WorkerOperationalCredential>, StoreError> {
        Ok(self
            .operational_credentials
            .lock()
            .unwrap()
            .get(credential_digest)
            .cloned())
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
