//! `authorize_http_endpoint` 기본값 deny 전환 통합 테스트 (로드맵 `#73`).
//!
//! 이전에는 `required_capability`가 `None`을 반환하는 route(=행렬 미등록)를
//! 인증만 통과하면 누구나 호출할 수 있었다. 이 파일은 세 가지를 검증한다:
//!
//! 1. 이전에 실제로 뚫려 있던 두 route(`GET /workers/{id}`, `POST
//!    /hosts/register`)가 이제 capability 없이는 403이고, capability를 갖추면
//!    통과한다.
//! 2. `#58`/`#66`이 반복한 "행렬 미등록 = 허용" 결함을, **행렬에 없는 임의의
//!    route도 인증만으로는 절대 통과하지 못한다**는 형태로 회귀 방지한다.
//! 3. `POST /workers/join`은 이 전환 이후에도(그리고 `allow_no_auth` 모드
//!    에서도) 여전히 body의 bootstrap token만으로 동작한다 — 예외 처리를
//!    빠뜨리면 join이 항상 403이 되는 회귀가 나므로 별도로 고정한다.

use std::sync::Arc;

use fleet_api::{build_app, AppState};
use fleet_core::{BootstrapToken, PermissionKind, Worker};
use fleet_store::mem::MemStore;
use fleet_store::{Store, WorkerOperationalCredential};

async fn spawn_server(state: AppState) -> String {
    let state = Arc::new(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

fn scoped_token(token: &str, capabilities: Vec<PermissionKind>) -> fleet_api::ApiTokenCredential {
    fleet_api::ApiTokenCredential {
        principal_id: "test".into(),
        token: token.into(),
        capabilities,
    }
}

async fn seed_worker(store: &Arc<dyn Store>, name: &str) -> fleet_core::WorkerId {
    let worker = Worker::new(name, format!("wss://{name}.local/ws?server-key=leaked-secret"));
    let worker_id = worker.id;
    store.upsert_worker(&worker).await.unwrap();
    worker_id
}

// ── 1a. GET /v1/workers/{id} ────────────────────────────────────────────

#[tokio::test]
async fn get_worker_by_id_without_capability_is_forbidden() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let worker_id = seed_worker(&store, "target-worker").await;

    let mut state = AppState::new(store);
    state.allow_no_auth = false;
    // 아무 capability도 없는 토큰 — 이전에는 행렬 미등록이라 이것으로도 통과했다.
    state = state.with_tokens(vec![scoped_token("bare-token", vec![])]);
    let url = spawn_server(state).await;

    let resp = reqwest::Client::new()
        .get(format!("{url}/v1/workers/{worker_id}"))
        .header("authorization", "Bearer bare-token")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "GET /workers/{{id}} must require a capability now (#73)"
    );
}

#[tokio::test]
async fn get_worker_by_id_with_worker_list_capability_succeeds() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let worker_id = seed_worker(&store, "target-worker").await;

    let mut state = AppState::new(store);
    state.allow_no_auth = false;
    state = state.with_tokens(vec![scoped_token(
        "listing-token",
        vec![PermissionKind::WorkerList],
    )]);
    let url = spawn_server(state).await;

    let resp = reqwest::Client::new()
        .get(format!("{url}/v1/workers/{worker_id}"))
        .header("authorization", "Bearer listing-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "worker:list must still be sufficient");
}

/// 워커 A의 operational credential(=`WorkerRegister`/`WorkerDelete`만 가짐)
/// 로는 워커 B를 조회할 수 없다 — 이전에는 행렬 미등록이라 가능했고, 이것이
/// `endpoint`(ACP `server-key`)를 통한 워커 간 시크릿 노출 경로였다.
#[tokio::test]
async fn worker_operational_credential_cannot_read_another_worker() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let victim_id = seed_worker(&store, "victim-worker").await;

    let attacker = Worker::new("attacker-worker", "wss://attacker.local/ws");
    let attacker_id = attacker.id;
    store.upsert_worker(&attacker).await.unwrap();
    let plaintext = "fwo_test_attacker";
    store
        .upsert_worker_operational_credential(&WorkerOperationalCredential {
            worker_id: attacker_id,
            credential_digest: BootstrapToken::digest_for(plaintext),
            issued_at: chrono::Utc::now(),
            expires_at: None,
            revoked_at: None,
            rotation_generation: 1,
        })
        .await
        .unwrap();

    let mut state = AppState::new(store);
    state.allow_no_auth = false;
    let url = spawn_server(state).await;

    let resp = reqwest::Client::new()
        .get(format!("{url}/v1/workers/{victim_id}"))
        .header("authorization", format!("Bearer {plaintext}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "a worker's own operational credential must not read another worker's record"
    );
}

// ── 1b. POST /v1/hosts/register ─────────────────────────────────────────

fn host_register_body() -> serde_json::Value {
    serde_json::json!({
        "hostname": "attacker-controlled-host",
        "ssh_host": "10.0.0.1",
        "ssh_port": 22,
        "ssh_user": "root",
        "succeeded": true,
    })
}

#[tokio::test]
async fn host_register_without_capability_is_forbidden() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let mut state = AppState::new(store);
    state.allow_no_auth = false;
    state = state.with_tokens(vec![scoped_token("bare-token", vec![])]);
    let url = spawn_server(state).await;

    let resp = reqwest::Client::new()
        .post(format!("{url}/v1/hosts/register"))
        .header("authorization", "Bearer bare-token")
        .json(&host_register_body())
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "POST /hosts/register must require host:provision now (#73)"
    );
}

#[tokio::test]
async fn host_register_with_host_provision_capability_succeeds() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let mut state = AppState::new(store);
    state.allow_no_auth = false;
    state = state.with_tokens(vec![scoped_token(
        "provisioner-token",
        vec![PermissionKind::HostProvision],
    )]);
    let url = spawn_server(state).await;

    let resp = reqwest::Client::new()
        .post(format!("{url}/v1/hosts/register"))
        .header("authorization", "Bearer provisioner-token")
        .json(&host_register_body())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "host:provision must still be sufficient");
}

/// 워커 자신의 operational credential(`WorkerRegister`/`WorkerDelete`뿐)로는
/// Host 레코드를 등록·덮어쓸 수 없다.
#[tokio::test]
async fn worker_operational_credential_cannot_register_host() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let worker = Worker::new("w1", "wss://w1.local/ws");
    let worker_id = worker.id;
    store.upsert_worker(&worker).await.unwrap();
    let plaintext = "fwo_test_w1";
    store
        .upsert_worker_operational_credential(&WorkerOperationalCredential {
            worker_id,
            credential_digest: BootstrapToken::digest_for(plaintext),
            issued_at: chrono::Utc::now(),
            expires_at: None,
            revoked_at: None,
            rotation_generation: 1,
        })
        .await
        .unwrap();

    let mut state = AppState::new(store);
    state.allow_no_auth = false;
    let url = spawn_server(state).await;

    let resp = reqwest::Client::new()
        .post(format!("{url}/v1/hosts/register"))
        .header("authorization", format!("Bearer {plaintext}"))
        .json(&host_register_body())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

// ── 2. 일반 회귀 방지: 실제로 등록된 route라도 메서드가 다르면 행렬에 없다 ──
//
// (완전히 존재하지 않는 경로는 axum의 nest() 라우팅이 미들웨어 자체를 타지
// 않고 404를 반환한다 — 그건 보안 문제가 아니다. `#73`이 실제로 지키려는
// 것은 "router에 등록된 route인데 행렬에 없는" 경우이며, 그 회귀 가드는
// `crates/fleet-api/src/app.rs`의 `authorize_http_endpoint_denies_by_default_for_any_unmatched_route`와
// `capability_matrix_covers_router_routes`가 함수 수준에서 정확히 담당한다.
// 여기서는 실제 HTTP 스택을 통해 동일한 종류의 시도가 여전히 막히는지만
// 한 번 더 확인한다.)

#[tokio::test]
async fn wrong_method_on_registered_path_is_forbidden_even_with_full_capabilities() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let mut state = AppState::new(store);
    state.allow_no_auth = false;
    // 전체 capability를 가진 토큰이라도, 행렬에 없는 (method, path) 조합은
    // 통과하지 못한다 — "행렬 미등록 = 허용"이 다시 살아나지 않았음을 고정한다.
    state = state.with_tokens(vec![scoped_token(
        "omnipotent-token",
        PermissionKind::all().to_vec(),
    )]);
    let url = spawn_server(state).await;

    // PUT /v1/workers — 경로는 실재하지만(GET만 등록됨) PUT은 행렬에 없다.
    let resp = reqwest::Client::new()
        .put(format!("{url}/v1/workers"))
        .header("authorization", "Bearer omnipotent-token")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "a registered path with an unregistered method must deny by default"
    );
}

// ── 3. join은 이 전환 이후에도, allow_no_auth 모드에서도 body 인증으로 동작한다 ──

#[tokio::test]
async fn join_still_works_under_allow_no_auth_after_default_deny() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let (token, _hash) = ("fleet_boot_test_token".to_string(), ());
    let bootstrap = BootstrapToken {
        token_digest: BootstrapToken::digest_for(&token),
        created_at: chrono::Utc::now(),
        created_by: None,
        expires_at: None,
        max_uses: 1,
        use_count: 0,
        notes: None,
        last_used_by: None,
        last_used_at: None,
    };
    store.create_bootstrap_token(&bootstrap).await.unwrap();

    // allow_no_auth == true (기본값) 상태로 둔다 — 이 경로가 join 전용 우회를
    // 거치지 않고 authorize_http_endpoint를 그대로 타므로, `/health`와 같은
    // 명시적 예외 없이는 항상 403이 나는 회귀가 여기서만 재현된다.
    let state = AppState::new(store);
    assert!(state.allow_no_auth, "test assumes the dev default");
    let url = spawn_server(state).await;

    let resp = reqwest::Client::new()
        .post(format!("{url}/v1/workers/join"))
        .json(&serde_json::json!({
            "token": token,
            "name": "joined-worker",
            "agent_endpoint": "ws://joined-worker.local/ws?server-key=x",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "join must still succeed via body token auth, not the capability matrix"
    );
}
