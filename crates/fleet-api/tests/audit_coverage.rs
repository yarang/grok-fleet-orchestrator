//! 감사 범위 확장 통합 테스트 (로드맵 #76).
//!
//! `#76` 이전에는 bootstrap/admin token 발급·회수, worker 등록·등록해제,
//! host 등록, capability 거절이 `AuditEvent`를 전혀 남기지 않고 `tracing`
//! 출력으로만 남았다(사후 조회 불가). 이 테스트는:
//!
//! 1. 위 mutation들이 실제로 `AuditEvent`를 남기는지,
//! 2. secret을 새로 발급하는 mutation(bootstrap/admin token 발급·회전)이
//!    감사 기록 실패 시 방금 발급한 자격을 즉시 회수하고 거절하는지
//!    (`#66`의 export fail-closed 패턴을 발급 쪽에도 적용했는지),
//! 3. 이미 반영된 mutation(회수·등록·등록해제)은 감사 기록 실패로 응답을
//!    뒤집지 않는지(log-only),
//! 4. capability 거절 자체도 감사되는지
//!
//! 를 실제 HTTP 라운드트립으로 검증한다.

use std::net::SocketAddr;
use std::sync::Arc;

use fleet_api::{build_app, ApiTokenCredential, AppState};
use fleet_core::PermissionKind;
use fleet_store::mem::MemStore;
use fleet_store::Store;
use serde_json::json;
use tokio::task::JoinHandle;

struct Server {
    url: String,
    _handle: JoinHandle<()>,
}

async fn spawn(state: AppState) -> Server {
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(Arc::new(state));
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Server {
        url: format!("http://{addr}"),
        _handle: handle,
    }
}

/// 모든 capability를 가진 admin bearer 토큰으로 서버를 띄운다.
async fn spawn_with_admin(store: Arc<dyn Store>, token: &str) -> Server {
    let state = AppState::new(store).with_tokens(vec![ApiTokenCredential {
        principal_id: "root".into(),
        token: token.into(),
        capabilities: PermissionKind::all().to_vec(),
    }]);
    spawn(state).await
}

async fn find_events(store: &Arc<dyn Store>, action: &str) -> Vec<fleet_core::AuditEvent> {
    store
        .list_audit_events(&fleet_core::AuditFilter {
            actor_user_id: None,
            action: Some(action.to_string()),
            limit: 100,
            offset: 0,
        })
        .await
        .expect("list_audit_events")
}

// ── bootstrap token ──────────────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_token_issue_and_revoke_are_audited() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let srv = spawn_with_admin(store.clone(), "root-secret").await;
    let client = reqwest::Client::new();

    let create: serde_json::Value = client
        .post(format!("{}/v1/bootstrap-tokens", srv.url))
        .header("authorization", "Bearer root-secret")
        .json(&json!({"prefix": "audit-test", "bytes": 16, "max_uses": 1}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let raw_token = create["token"].as_str().unwrap().to_string();
    let token_id = create["token_id"].as_str().unwrap().to_string();

    let issued = find_events(&store, fleet_core::audit::action::TOKEN_BOOTSTRAP_ISSUE).await;
    assert_eq!(issued.len(), 1);
    assert_eq!(issued[0].outcome, fleet_core::AuditOutcome::Success);
    assert_eq!(issued[0].target_id.as_deref(), Some(token_id.as_str()));
    // 원문 토큰은 감사 detail에 절대 실리지 않는다.
    assert!(!issued[0].detail.to_string().contains(&raw_token));
    assert_eq!(issued[0].detail["prefix"], "audit-test");

    let revoke_resp = client
        .delete(format!("{}/v1/bootstrap-tokens/{}", srv.url, token_id))
        .header("authorization", "Bearer root-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(revoke_resp.status(), 200);

    let revoked = find_events(&store, fleet_core::audit::action::TOKEN_BOOTSTRAP_REVOKE).await;
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].target_id.as_deref(), Some(token_id.as_str()));
}

#[tokio::test]
async fn bootstrap_token_issuance_fails_closed_when_audit_recording_fails() {
    // record_audit_event가 항상 실패하는 store — 발급이 감사되지 못하면
    // 방금 만든 토큰을 즉시 회수하고 500을 반환해야 한다(export의
    // fail-closed 원칙을 발급 쪽에도 적용).
    let store: Arc<dyn Store> = Arc::new(MemStore::new().with_failing(&["record_audit_event"]));
    let srv = spawn_with_admin(store.clone(), "root-secret").await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/bootstrap-tokens", srv.url))
        .header("authorization", "Bearer root-secret")
        .json(&json!({"prefix": "should-not-persist", "bytes": 16, "max_uses": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 500);

    // 방금 발급된 토큰이 즉시 회수되어 살아있는 토큰이 하나도 남지 않는다.
    let remaining = store.list_bootstrap_tokens().await.unwrap();
    assert!(
        remaining.is_empty(),
        "un-audited bootstrap token must not remain live: {remaining:?}"
    );
}

// ── admin API token ───────────────────────────────────────────────────────

#[tokio::test]
async fn admin_token_create_rotate_revoke_are_audited() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let srv = spawn_with_admin(store.clone(), "root-secret").await;
    let client = reqwest::Client::new();

    let create: serde_json::Value = client
        .post(format!("{}/v1/admin/tokens", srv.url))
        .header("authorization", "Bearer root-secret")
        .json(&json!({"principal_id": "svc-audit", "capabilities": ["worker:list"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let raw_token = create["token"].as_str().unwrap().to_string();

    let created = find_events(&store, fleet_core::audit::action::ADMIN_TOKEN_CREATE).await;
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].target_id.as_deref(), Some("svc-audit"));
    assert!(!created[0].detail.to_string().contains(&raw_token));
    assert_eq!(created[0].detail["capabilities"], json!(["worker:list"]));

    let rotate_resp = client
        .post(format!("{}/v1/admin/tokens/svc-audit/rotate", srv.url))
        .header("authorization", "Bearer root-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(rotate_resp.status(), 200);
    let rotated = find_events(&store, fleet_core::audit::action::ADMIN_TOKEN_ROTATE).await;
    assert_eq!(rotated.len(), 1);
    assert_eq!(rotated[0].detail["rotation_generation"], 2);

    let revoke_resp = client
        .delete(format!("{}/v1/admin/tokens/svc-audit", srv.url))
        .header("authorization", "Bearer root-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(revoke_resp.status(), 200);
    let revoked = find_events(&store, fleet_core::audit::action::ADMIN_TOKEN_REVOKE).await;
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].target_id.as_deref(), Some("svc-audit"));
}

#[tokio::test]
async fn admin_token_creation_fails_closed_when_audit_recording_fails() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new().with_failing(&["record_audit_event"]));
    let srv = spawn_with_admin(store.clone(), "root-secret").await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/admin/tokens", srv.url))
        .header("authorization", "Bearer root-secret")
        .json(&json!({"principal_id": "orphan-svc", "capabilities": ["worker:list"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 500);

    // 발급된 토큰이 즉시 회수돼 이 principal로는 어떤 요청도 인증되지 않는다.
    let auth_check = client
        .get(format!("{}/v1/workers", srv.url))
        .header("authorization", "Bearer whatever-was-not-returned")
        .send()
        .await
        .unwrap();
    assert_eq!(auth_check.status(), 401);

    let tokens = store.list_admin_tokens().await.unwrap();
    let orphan = tokens
        .iter()
        .find(|t| t.principal_id == "orphan-svc")
        .expect("token row exists (create succeeded before compensation)");
    assert!(
        orphan.revoked_at.is_some(),
        "un-audited admin token must be revoked immediately"
    );
}

#[tokio::test]
async fn admin_token_rotation_fails_closed_when_audit_recording_fails() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new().with_failing(&["record_audit_event"]));

    // store trait을 직접 통해 시딩 — 이 경로는 handler의 감사 로직을
    // 거치지 않으므로 with_failing이어도 성공한다.
    store
        .create_admin_token(&fleet_store::AdminApiToken {
            principal_id: "rotate-me".into(),
            token_digest: fleet_core::BootstrapToken::digest_for("seed-token"),
            capabilities: vec![PermissionKind::WorkerList],
            created_at: chrono::Utc::now(),
            rotated_at: None,
            revoked_at: None,
            rotation_generation: 1,
        })
        .await
        .expect("seed admin token");

    let srv = spawn_with_admin(store.clone(), "root-secret").await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/admin/tokens/rotate-me/rotate", srv.url))
        .header("authorization", "Bearer root-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 500);

    // 이전 토큰은 이미 rotate 호출 자체로 무효화됐고(store 계층에서 실행됨),
    // 감사 실패로 보상된 새 토큰도 즉시 회수된다 — principal이 무자격
    // 상태로 안전하게 실패한다(권한이 남는 쪽보다 없는 쪽으로 실패).
    let tokens = store.list_admin_tokens().await.unwrap();
    let row = tokens
        .iter()
        .find(|t| t.principal_id == "rotate-me")
        .unwrap();
    assert_eq!(row.rotation_generation, 2, "rotate itself already ran");
    assert!(
        row.revoked_at.is_some(),
        "un-audited rotation must be revoked immediately"
    );
}

// ── worker / host mutation (log-only) ──────────────────────────────────────

#[tokio::test]
async fn worker_register_and_deregister_are_audited() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let state = AppState::new(store.clone()); // allow_no_auth == true (기본)
    let srv = spawn(state).await;
    let client = reqwest::Client::new();

    let reg: serde_json::Value = client
        .post(format!("{}/v1/workers/register", srv.url))
        .json(&json!({
            "name": "audit-worker-1",
            "agent_endpoint": "wss://10.0.9.9:2419/ws?server-key=super-secret-value",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let worker_id = reg["worker_id"].as_str().unwrap().to_string();

    let registered = find_events(&store, fleet_core::audit::action::WORKER_REGISTER).await;
    assert_eq!(registered.len(), 1);
    assert_eq!(registered[0].target_id.as_deref(), Some(worker_id.as_str()));
    assert_eq!(registered[0].detail["is_new"], true);
    assert!(
        !registered[0]
            .detail
            .to_string()
            .contains("super-secret-value"),
        "endpoint secret must never reach the audit log"
    );

    let dereg = client
        .delete(format!("{}/v1/workers/{}", srv.url, worker_id))
        .json(&json!({"reason": "audit test cleanup"}))
        .send()
        .await
        .unwrap();
    assert_eq!(dereg.status(), 200);

    let deregistered = find_events(&store, fleet_core::audit::action::WORKER_DEREGISTER).await;
    assert_eq!(deregistered.len(), 1);
    assert_eq!(
        deregistered[0].target_id.as_deref(),
        Some(worker_id.as_str())
    );
    assert_eq!(deregistered[0].detail["reason"], "audit test cleanup");
}

#[tokio::test]
async fn host_register_is_audited() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let state = AppState::new(store.clone());
    let srv = spawn(state).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/hosts/register", srv.url))
        .json(&json!({
            "hostname": "audit-host-1",
            "ssh_host": "10.0.9.10",
            "ssh_user": "root",
            "succeeded": true,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let events = find_events(&store, fleet_core::audit::action::HOST_REGISTER).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail["hostname"], "audit-host-1");
}

// ── capability 거절 ─────────────────────────────────────────────────────

#[tokio::test]
async fn capability_denial_is_audited() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let state = AppState::new(store.clone()).with_tokens(vec![ApiTokenCredential {
        principal_id: "no-worker-list".into(),
        token: "limited-token".into(),
        // worker:list가 없는 principal — GET /v1/workers는 403이어야 한다.
        capabilities: vec![PermissionKind::TokenIssue],
    }]);
    let srv = spawn(state).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/v1/workers", srv.url))
        .header("authorization", "Bearer limited-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);

    let denials = find_events(&store, fleet_core::audit::action::HTTP_CAPABILITY_DENIED).await;
    assert_eq!(denials.len(), 1);
    assert_eq!(denials[0].outcome, fleet_core::AuditOutcome::Failure);
    assert_eq!(denials[0].actor_label, "no-worker-list");
    assert_eq!(denials[0].detail["required_capability"], "worker:list");
}

#[tokio::test]
async fn capability_denial_audit_failure_does_not_change_the_403_response() {
    // 거절은 권한을 내주는 쪽이 아니므로 감사 기록 실패가 이미 결정된
    // 403을 뒤집지 않는다(log-only) — 발급 쪽의 fail-closed와 대칭적으로
    // 확인해 둔다.
    let store: Arc<dyn Store> = Arc::new(MemStore::new().with_failing(&["record_audit_event"]));
    let state = AppState::new(store).with_tokens(vec![ApiTokenCredential {
        principal_id: "no-worker-list".into(),
        token: "limited-token".into(),
        capabilities: vec![PermissionKind::TokenIssue],
    }]);
    let srv = spawn(state).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/v1/workers", srv.url))
        .header("authorization", "Bearer limited-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

// ── Agent 고아 종료 (로드맵 #70 게이트 ③) ─────────────────────────────────

/// Worker가 배정받지 않은 Agent 프로세스를 죽였다는 사실이 감사에 도달한다.
///
/// **이 경로가 없으면 그 종료는 워커 로그 한 줄로만 남는다.** `#67` 게이트 ②의
/// 술어와 `036`의 트리거는 같은 Agent가 두 Worker에서 도는 것을 막으려 하는데,
/// 그 방어가 뚫렸는지를 관측할 방법이 그때까지 없었다.
#[tokio::test]
async fn agent_orphan_terminations_reported_by_a_worker_are_audited() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let state = AppState::new(store.clone());
    let srv = spawn(state).await;
    let client = reqwest::Client::new();

    let reg: serde_json::Value = client
        .post(format!("{}/v1/workers/register", srv.url))
        .json(&json!({
            "name": "orphan-reporter",
            "agent_endpoint": "wss://10.0.9.9:2419/ws",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let worker_id = reg["worker_id"].as_str().unwrap().to_string();

    // 이 Agent는 **이 Worker에 배정된 적이 없다** — 그것이 orphan의 정의다.
    // 그래서 `agents` 행을 고칠 수 없고, 감사 줄만이 남길 수 있는 자리다.
    let orphan_agent = uuid::Uuid::new_v4().to_string();
    let hb = client
        .post(format!("{}/v1/workers/heartbeat", srv.url))
        .json(&json!({
            "worker_id": worker_id,
            "agent_orphans": [
                {"agent_id": orphan_agent, "reason": "unplaced"}
            ],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        hb.status(),
        200,
        "고아 보고가 heartbeat을 실패시키지 않는다"
    );

    let events = find_events(&store, fleet_core::audit::action::AGENT_ORPHAN_TERMINATED).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].target_id.as_deref(), Some(orphan_agent.as_str()));
    assert_eq!(events[0].detail["worker_id"], worker_id);
    assert_eq!(events[0].detail["reason"], "unplaced");
    // actor는 Worker이지 사람이 아니다. 여기에 사람이 들어가면 "누가 죽였나"에
    // 없는 사람이 적힌다.
    assert!(events[0].actor_user_id.is_none());
    assert_eq!(events[0].actor_label, format!("worker:{worker_id}"));
}

/// Worker가 제어면을 잃어 스스로 Agent를 멈췄다는 사실이 감사에 도달한다
/// (로드맵 `#67` 게이트 ⑥).
///
/// **상태가 아니라 이유를 나르는 경로다.** 멈춘 Agent가 관측 목록에서 빠지면
/// `apply_agent_observations`가 그 관측을 지우므로 상태는 저절로 옳아진다.
/// 그러나 그 경로는 **왜** 멈췄는지를 말하지 못해, 운영자에게는 멀쩡하던
/// Agent가 이유 없이 사라진 것으로 보인다.
#[tokio::test]
async fn agent_self_fencing_reported_by_a_worker_is_audited() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let state = AppState::new(store.clone());
    let srv = spawn(state).await;
    let client = reqwest::Client::new();

    let reg: serde_json::Value = client
        .post(format!("{}/v1/workers/register", srv.url))
        .json(&json!({
            "name": "fencing-reporter",
            "agent_endpoint": "wss://10.0.9.9:2419/ws",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let worker_id = reg["worker_id"].as_str().unwrap().to_string();

    // orphan과 달리 이 Agent는 이 Worker에 **배정돼 있었다**. 그래서 이 보고는
    // 관측이 이미 지운 상태를 다시 쓰지 않고 이유만 남긴다.
    let fenced_agent = uuid::Uuid::new_v4().to_string();
    let hb = client
        .post(format!("{}/v1/workers/heartbeat", srv.url))
        .json(&json!({
            "worker_id": worker_id,
            "agent_fenced": [
                {"agent_id": fenced_agent, "unreachable_secs": 312}
            ],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        hb.status(),
        200,
        "펜싱 보고가 heartbeat을 실패시키지 않는다"
    );

    let events = find_events(&store, fleet_core::audit::action::AGENT_SELF_FENCED).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].target_id.as_deref(), Some(fenced_agent.as_str()));
    assert_eq!(events[0].detail["worker_id"], worker_id);
    // 경과 시간이 실려야 종료 시각을 거슬러 올라갈 수 있다 — 이 줄의
    // `created_at`은 종료가 아니라 **재연결** 시각이다.
    assert_eq!(events[0].detail["unreachable_secs"], 312);
    assert!(events[0].actor_user_id.is_none());
    assert_eq!(events[0].actor_label, format!("worker:{worker_id}"));
}

/// 펜싱 보고도 상한을 넘지 못한다. 두 목록에 각각 걸리며, 그래서 한쪽을
/// 가득 채워 다른 쪽의 상한을 밀어낼 수 없다.
#[tokio::test]
async fn a_single_beat_cannot_write_unbounded_self_fencing_audit_rows() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let state = AppState::new(store.clone());
    let srv = spawn(state).await;
    let client = reqwest::Client::new();

    let reg: serde_json::Value = client
        .post(format!("{}/v1/workers/register", srv.url))
        .json(&json!({"name": "noisy-fencer", "agent_endpoint": "wss://10.0.9.9:2419/ws"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let worker_id = reg["worker_id"].as_str().unwrap().to_string();

    let fenced: Vec<serde_json::Value> = (0..100)
        .map(|_| json!({"agent_id": uuid::Uuid::new_v4().to_string(), "unreachable_secs": 600}))
        .collect();
    let hb = client
        .post(format!("{}/v1/workers/heartbeat", srv.url))
        .json(&json!({"worker_id": worker_id, "agent_fenced": fenced}))
        .send()
        .await
        .unwrap();
    assert_eq!(hb.status(), 200);

    let events = find_events(&store, fleet_core::audit::action::AGENT_SELF_FENCED).await;
    assert_eq!(
        events.len(),
        32,
        "상한(MAX_AGENT_EVENTS_PER_BEAT)까지만 기록한다"
    );
}

/// 한 beat이 남기는 줄 수에 상한이 있다.
///
/// 이 목록은 **Worker가 통제한다.** 인증된 Worker라는 것과 그 Worker가 보내는
/// 값이 정직하다는 것은 다르며, 상한이 없으면 장악된 Worker 하나의 beat이 감사
/// 테이블에 임의 개수의 줄을 쓴다. 감사는 다른 사건을 조회하는 자리이기도
/// 하므로 그 오염은 이 기능이 지키려는 것 자체를 무디게 만든다.
#[tokio::test]
async fn a_single_beat_cannot_write_unbounded_orphan_audit_rows() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let state = AppState::new(store.clone());
    let srv = spawn(state).await;
    let client = reqwest::Client::new();

    let reg: serde_json::Value = client
        .post(format!("{}/v1/workers/register", srv.url))
        .json(&json!({"name": "noisy-worker", "agent_endpoint": "wss://10.0.9.9:2419/ws"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let worker_id = reg["worker_id"].as_str().unwrap().to_string();

    let orphans: Vec<serde_json::Value> = (0..200)
        .map(|_| json!({"agent_id": uuid::Uuid::new_v4().to_string(), "reason": "unplaced"}))
        .collect();
    let hb = client
        .post(format!("{}/v1/workers/heartbeat", srv.url))
        .json(&json!({"worker_id": worker_id, "agent_orphans": orphans}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        hb.status(),
        200,
        "상한 초과가 heartbeat을 실패시키지는 않는다"
    );

    let events = find_events(&store, fleet_core::audit::action::AGENT_ORPHAN_TERMINATED).await;
    assert_eq!(
        events.len(),
        32,
        "상한(MAX_AGENT_EVENTS_PER_BEAT)까지만 기록한다"
    );
}
