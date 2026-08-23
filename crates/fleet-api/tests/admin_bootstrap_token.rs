//! 최초 admin 토큰 발급 경로 통합 테스트 (로드맵 #80).
//!
//! `fleet admin-tokens create`/`fleet token issue`는 둘 다 기존 admin bearer를
//! 요구하므로, `issue_admin_bootstrap_token_if_needed` 없이는 최초 admin
//! 토큰을 API로 만들 방법이 없었다(운영자가 `FLEET_API_TOKENS` JSON을 손으로
//! 작성하는 것 외에는). 이 파일은 그 함수의 계약을 검증한다:
//! - 빈 store에서 1회 발급하고, 발급된 토큰이 실제로 전체 capability로 인증됨.
//! - 이미 admin 토큰이 있으면(수동이든 env sync든) 재발급하지 않음.
//! - 두 번째 호출은 아무 것도 하지 않는 멱등한 no-op.

use std::sync::Arc;

use fleet_api::{
    build_app, issue_admin_bootstrap_token_if_needed, sync_env_admin_tokens_to_store,
    ApiTokenCredential, AppState,
};
use fleet_core::PermissionKind;
use fleet_store::mem::MemStore;
use fleet_store::Store;

async fn spawn_server(store: Arc<dyn Store>) -> String {
    let state = AppState::new(store);
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
async fn issues_exactly_one_token_from_an_empty_store() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());

    let issued = issue_admin_bootstrap_token_if_needed(&*store)
        .await
        .expect("issuing against an empty store must succeed");
    assert!(issued.is_some(), "empty store must get a bootstrap token");

    let tokens = store.list_admin_tokens().await.unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].principal_id, "bootstrap");
    assert_eq!(tokens[0].capabilities, PermissionKind::all().to_vec());
}

#[tokio::test]
async fn issued_token_actually_authenticates_with_full_capability() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let token = issue_admin_bootstrap_token_if_needed(&*store)
        .await
        .unwrap()
        .expect("must issue a token");

    let url = spawn_server(store).await;
    let resp = reqwest::Client::new()
        .get(format!("{url}/v1/workers"))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "the bootstrap token must actually authenticate a real request"
    );

    // 전체 capability이므로 admin 전용 표면도 통과해야 한다.
    let resp = reqwest::Client::new()
        .get(format!("{url}/v1/admin/tokens"))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn does_not_reissue_when_admin_tokens_already_exist() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());

    let first = issue_admin_bootstrap_token_if_needed(&*store)
        .await
        .unwrap();
    assert!(first.is_some());

    // 재기동을 시뮬레이션 — 두 번째 호출은 멱등한 no-op이어야 한다.
    let second = issue_admin_bootstrap_token_if_needed(&*store)
        .await
        .unwrap();
    assert!(
        second.is_none(),
        "a second call against a non-empty store must not issue another token"
    );

    let tokens = store.list_admin_tokens().await.unwrap();
    assert_eq!(tokens.len(), 1, "exactly one token must ever exist");
}

#[tokio::test]
async fn skips_bootstrap_when_env_tokens_were_already_synced() {
    // #72의 env→DB 자동 전환이 먼저 DB를 채운 경우, #80의 bootstrap 경로는
    // 관여하지 않아야 한다 — env로 admin을 이미 구성한 배포에
    // 최소 권한 원칙을 벗어난 추가 전체-권한 토큰을 몰래 심으면 안 된다.
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let env_tokens = vec![ApiTokenCredential {
        principal_id: "ops-cli".into(),
        token: "env-configured-token".into(),
        capabilities: vec![PermissionKind::WorkerList],
    }];
    sync_env_admin_tokens_to_store(&*store, &env_tokens)
        .await
        .unwrap();

    let issued = issue_admin_bootstrap_token_if_needed(&*store)
        .await
        .unwrap();
    assert!(
        issued.is_none(),
        "an env-provisioned deployment must not also get a full-capability bootstrap token"
    );

    let tokens = store.list_admin_tokens().await.unwrap();
    assert_eq!(tokens.len(), 1, "only the env-synced token should exist");
    assert_eq!(tokens[0].principal_id, "ops-cli");
}
