//! Worker self-binding 통합 테스트 (로드맵 #60 5단계).
//!
//! join에서 발급된 worker operational credential(`fwo_...`)로 인증한 요청이
//! 자기 자신의 `worker_id`만 조작할 수 있고, 다른 worker의 신원으로는
//! register/heartbeat/deregister 모두 403을 받는지 검증한다.

use std::sync::Arc;

use chrono::Utc;
use fleet_api::{build_app, AppState};
use fleet_core::{BootstrapToken, Worker};
use fleet_store::mem::MemStore;
use fleet_store::{Store, WorkerOperationalCredential};

/// 워커를 store에 직접 심고, 평문 operational token을 반환한다.
async fn seed_worker_with_credential(
    store: &Arc<dyn Store>,
    name: &str,
) -> (fleet_core::WorkerId, String) {
    let worker = Worker::new(name, format!("ws://{name}.local/ws"));
    let worker_id = worker.id;
    store.upsert_worker(&worker).await.unwrap();

    let plaintext = format!("fwo_test_{name}");
    store
        .upsert_worker_operational_credential(&WorkerOperationalCredential {
            worker_id,
            credential_digest: BootstrapToken::digest_for(&plaintext),
            issued_at: Utc::now(),
            expires_at: None,
            revoked_at: None,
            rotation_generation: 1,
        })
        .await
        .unwrap();

    (worker_id, plaintext)
}

/// ephemeral port에 HTTP API 서버를 시작한다. `allow_no_auth`는 꺼서 operational
/// credential 검증 경로가 실제로 타도록 한다.
async fn spawn_server(store: Arc<dyn Store>) -> String {
    let mut state = AppState::new(store);
    state.allow_no_auth = false;
    let state = Arc::new(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn heartbeat_for_self_succeeds_but_for_other_worker_is_forbidden() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let (worker_a, token_a) = seed_worker_with_credential(&store, "worker-a").await;
    let (worker_b, _token_b) = seed_worker_with_credential(&store, "worker-b").await;
    let url = spawn_server(store).await;
    let client = reqwest::Client::new();

    // A가 자기 자신의 heartbeat를 보내면 성공.
    let resp = client
        .post(format!("{url}/v1/workers/heartbeat"))
        .header("authorization", format!("Bearer {token_a}"))
        .json(&serde_json::json!({"worker_id": worker_a.to_string(), "active_tasks": 0}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "self heartbeat should succeed");

    // A의 토큰으로 B의 heartbeat를 보내면 403.
    let resp = client
        .post(format!("{url}/v1/workers/heartbeat"))
        .header("authorization", format!("Bearer {token_a}"))
        .json(&serde_json::json!({"worker_id": worker_b.to_string(), "active_tasks": 0}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "cross-worker heartbeat must be forbidden"
    );
}

#[tokio::test]
async fn register_for_self_succeeds_but_impersonating_other_worker_is_forbidden() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let (worker_a, token_a) = seed_worker_with_credential(&store, "reg-a").await;
    let (worker_b, _token_b) = seed_worker_with_credential(&store, "reg-b").await;
    let url = spawn_server(store).await;
    let client = reqwest::Client::new();

    // A가 자기 자신을 (existing_worker_id로) 재등록하면 성공.
    let resp = client
        .post(format!("{url}/v1/workers/register"))
        .header("authorization", format!("Bearer {token_a}"))
        .json(&serde_json::json!({
            "name": "reg-a",
            "agent_endpoint": "ws://reg-a.local/ws",
            "existing_worker_id": worker_a.to_string(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "self re-register should succeed");

    // A의 토큰으로 B의 existing_worker_id를 지정해 등록하면 403 (신원 위장 차단).
    let resp = client
        .post(format!("{url}/v1/workers/register"))
        .header("authorization", format!("Bearer {token_a}"))
        .json(&serde_json::json!({
            "name": "reg-b",
            "agent_endpoint": "ws://reg-b.local/ws",
            "existing_worker_id": worker_b.to_string(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "registering another worker's identity must be forbidden"
    );
}

#[tokio::test]
async fn deregister_self_succeeds_but_deregistering_other_worker_is_forbidden() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let (worker_a, token_a) = seed_worker_with_credential(&store, "dereg-a").await;
    let (worker_b, _token_b) = seed_worker_with_credential(&store, "dereg-b").await;
    let url = spawn_server(store).await;
    let client = reqwest::Client::new();

    // A의 토큰으로 B를 등록 해제하려 하면 403이고, B는 그대로 남아있어야 한다.
    let resp = client
        .delete(format!("{url}/v1/workers/{worker_b}"))
        .header("authorization", format!("Bearer {token_a}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "cross-worker deregister must be forbidden"
    );

    // A가 자기 자신을 등록 해제하면 성공.
    let resp = client
        .delete(format!("{url}/v1/workers/{worker_a}"))
        .header("authorization", format!("Bearer {token_a}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "self deregister should succeed");
}

/// Worker operational credential은 자기 자신의 등록/해제 권한만 가진다
/// (`WorkerRegister` + `WorkerDelete`). 제어평면 관리 endpoint는 capability
/// 밖이므로 403이어야 한다.
///
/// 로드맵 #58 — 이전에는 capability 행렬이 `/v1/...` 경로로 매칭을 시도했지만
/// axum `nest`가 prefix를 제거한 뒤라 어떤 route와도 매칭되지 않았고, 그래서
/// 워커 자격증명으로 bootstrap token 발급·회수까지 가능했다(권한 상승).
#[tokio::test]
async fn worker_credential_cannot_reach_control_plane_endpoints() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let (_worker_id, token) = seed_worker_with_credential(&store, "scoped-worker").await;
    let url = spawn_server(store).await;
    let client = reqwest::Client::new();

    // bootstrap token 발급 — TokenIssue capability 없음.
    let resp = client
        .post(format!("{url}/v1/bootstrap-tokens"))
        .header("authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({"prefix": "fleet", "bytes": 16}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "worker credential must not be able to issue bootstrap tokens"
    );

    // bootstrap token 목록 — TokenList capability 없음.
    let resp = client
        .get(format!("{url}/v1/bootstrap-tokens"))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "worker credential must not list tokens");

    // worker 목록 — WorkerList capability 없음.
    let resp = client
        .get(format!("{url}/v1/workers"))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "worker credential must not list workers"
    );

    // 다른 워커의 credential rotate — WorkerCredentialManage capability 없음.
    let resp = client
        .post(format!("{url}/v1/workers/{_worker_id}/credential/rotate"))
        .header("authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "worker credential must not manage operational credentials"
    );
}
