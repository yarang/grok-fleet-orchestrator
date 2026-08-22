//! Worker operational credential rotate/revoke/expiry 통합 테스트 (로드맵 #60 6단계).
//!
//! "이전 token deny" 원칙을 직접 검증한다 — rotate/revoke 뒤 이전 원문 토큰으로는
//! register/heartbeat가 거부되고, 새로 발급된 토큰만 통과해야 한다. 만료된
//! credential도 같은 방식으로 거부됨을 확인한다.

use std::sync::Arc;

use chrono::{Duration, Utc};
use fleet_api::{build_app, ApiTokenCredential, AppState};
use fleet_core::{BootstrapToken, PermissionKind, Worker};
use fleet_store::mem::MemStore;
use fleet_store::{Store, WorkerOperationalCredential};

/// 워커를 store에 직접 심고, 평문 operational token을 반환한다.
async fn seed_worker_with_credential(
    store: &Arc<dyn Store>,
    name: &str,
    expires_at: Option<chrono::DateTime<Utc>>,
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
            expires_at,
            revoked_at: None,
            rotation_generation: 1,
        })
        .await
        .unwrap();

    (worker_id, plaintext)
}

/// admin bearer 토큰(`worker:credential:manage`)을 가진 서버를 ephemeral 포트에 띄운다.
async fn spawn_server_with_admin(store: Arc<dyn Store>, admin_token: &str) -> String {
    let state = AppState::new(store).with_tokens(vec![ApiTokenCredential {
        principal_id: "admin".into(),
        token: admin_token.into(),
        capabilities: vec![PermissionKind::WorkerCredentialManage],
    }]);
    let state = Arc::new(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

async fn heartbeat_status(url: &str, worker_id: fleet_core::WorkerId, token: &str) -> u16 {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{url}/v1/workers/heartbeat"))
        .header("authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({"worker_id": worker_id.to_string(), "active_tasks": 0}))
        .send()
        .await
        .unwrap();
    resp.status().as_u16()
}

#[tokio::test]
async fn rotate_invalidates_previous_token_and_new_token_authenticates() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let (worker_id, old_token) = seed_worker_with_credential(&store, "rotate-a", None).await;
    let url = spawn_server_with_admin(store, "admin-secret").await;
    let client = reqwest::Client::new();

    // rotate 전: 이전 토큰으로 heartbeat 성공.
    assert_eq!(heartbeat_status(&url, worker_id, &old_token).await, 200);

    let resp = client
        .post(format!("{url}/v1/workers/{worker_id}/credential/rotate"))
        .header("authorization", "Bearer admin-secret")
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "rotate should succeed for admin");
    let body: serde_json::Value = resp.json().await.unwrap();
    let new_token = body["operational_token"].as_str().unwrap().to_string();
    assert_ne!(new_token, old_token);
    assert_eq!(body["rotation_generation"], 2);

    // rotate 후: 이전 토큰은 거부되어야 한다 (자동 fallback 없음).
    assert_eq!(
        heartbeat_status(&url, worker_id, &old_token).await,
        401,
        "old token must be rejected after rotation"
    );

    // 새 토큰은 정상 인증되어야 한다.
    assert_eq!(
        heartbeat_status(&url, worker_id, &new_token).await,
        200,
        "new token must authenticate after rotation"
    );
}

#[tokio::test]
async fn rotate_by_worker_itself_is_forbidden_admin_only() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let (worker_id, worker_token) = seed_worker_with_credential(&store, "rotate-b", None).await;
    let url = spawn_server_with_admin(store, "admin-secret").await;
    let client = reqwest::Client::new();

    // worker 자신의 operational credential로는 rotate capability가 없다 (403).
    let resp = client
        .post(format!("{url}/v1/workers/{worker_id}/credential/rotate"))
        .header("authorization", format!("Bearer {worker_token}"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "worker self rotate must be forbidden");
}

#[tokio::test]
async fn revoke_denies_subsequent_authentication() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let (worker_id, token) = seed_worker_with_credential(&store, "revoke-a", None).await;
    let url = spawn_server_with_admin(store, "admin-secret").await;
    let client = reqwest::Client::new();

    assert_eq!(heartbeat_status(&url, worker_id, &token).await, 200);

    let resp = client
        .delete(format!("{url}/v1/workers/{worker_id}/credential"))
        .header("authorization", "Bearer admin-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "revoke should succeed for admin");

    assert_eq!(
        heartbeat_status(&url, worker_id, &token).await,
        401,
        "revoked token must be rejected"
    );

    // 이미 회수된 credential을 다시 회수하면 404.
    let resp = client
        .delete(format!("{url}/v1/workers/{worker_id}/credential"))
        .header("authorization", "Bearer admin-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        404,
        "revoking an already-revoked credential is a 404"
    );
}

#[tokio::test]
async fn expired_credential_is_denied() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let past = Utc::now() - Duration::hours(1);
    let (worker_id, token) = seed_worker_with_credential(&store, "expired-a", Some(past)).await;
    let url = spawn_server_with_admin(store, "admin-secret").await;

    assert_eq!(
        heartbeat_status(&url, worker_id, &token).await,
        401,
        "expired credential must be rejected"
    );
}

#[tokio::test]
async fn rotate_unknown_worker_is_not_found() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let url = spawn_server_with_admin(store, "admin-secret").await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!(
            "{url}/v1/workers/{}/credential/rotate",
            fleet_core::WorkerId::new()
        ))
        .header("authorization", "Bearer admin-secret")
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
