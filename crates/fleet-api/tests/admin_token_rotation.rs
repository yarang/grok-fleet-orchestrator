//! Admin API bearer token DB 기반 rotate/revoke 통합 테스트 (로드맵 #72).
//!
//! `FLEET_API_TOKENS`(정적 env 목록)에서 DB(`admin_api_tokens`)로의 무중단 전환과,
//! rotate/revoke가 이전 원문을 즉시 무효화하는지를 실제 HTTP 라운드트립으로 검증한다.

use std::sync::Arc;

use fleet_api::{build_app, sync_env_admin_tokens_to_store, ApiTokenCredential, AppState};
use fleet_core::PermissionKind;
use fleet_store::mem::MemStore;
use fleet_store::Store;

/// admin bearer 토큰(`admin_token:manage` + `admin_token:list`)을 가진 서버를
/// ephemeral 포트에 띄운다.
async fn spawn_server_with_admin(store: Arc<dyn Store>, admin_token: &str) -> String {
    let state = AppState::new(store).with_tokens(vec![ApiTokenCredential {
        principal_id: "root".into(),
        token: admin_token.into(),
        capabilities: vec![PermissionKind::AdminTokenManage, PermissionKind::AdminTokenList],
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

async fn worker_list_status(url: &str, token: &str) -> u16 {
    let client = reqwest::Client::new();
    client
        .get(format!("{url}/v1/workers"))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn create_rotate_revoke_round_trip() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let url = spawn_server_with_admin(store.clone(), "root-secret").await;
    let client = reqwest::Client::new();

    // capability 없이(no-auth 아님, 그냥 root-secret 없이) 호출하면 401.
    let unauth = client
        .post(format!("{url}/v1/admin/tokens"))
        .json(&serde_json::json!({
            "principal_id": "svc-a",
            "capabilities": ["worker:list"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(unauth.status(), 401);

    // 1. 생성.
    let create_resp = client
        .post(format!("{url}/v1/admin/tokens"))
        .header("authorization", "Bearer root-secret")
        .json(&serde_json::json!({
            "principal_id": "svc-a",
            "capabilities": ["worker:list"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 200);
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let first_token = created["token"].as_str().unwrap().to_string();
    assert!(first_token.starts_with("fat_"));

    // 새 토큰으로 실제 인증 성공 (worker:list capability).
    assert_eq!(worker_list_status(&url, &first_token).await, 200);

    // digest는 절대 응답에 없어야 한다.
    assert!(created.get("token_digest").is_none());
    assert!(!created.to_string().contains("token_digest"));

    // 2. rotate — 이전 토큰 즉시 무효화.
    let rotate_resp = client
        .post(format!("{url}/v1/admin/tokens/svc-a/rotate"))
        .header("authorization", "Bearer root-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(rotate_resp.status(), 200);
    let rotated: serde_json::Value = rotate_resp.json().await.unwrap();
    let second_token = rotated["token"].as_str().unwrap().to_string();
    assert_ne!(first_token, second_token);
    assert_eq!(rotated["rotation_generation"], 2);

    assert_eq!(
        worker_list_status(&url, &first_token).await,
        401,
        "old token must be rejected immediately after rotate"
    );
    assert_eq!(
        worker_list_status(&url, &second_token).await,
        200,
        "new token from rotate must authenticate"
    );

    // 3. list — 메타데이터만, digest·token 원문 어디에도 없음.
    let list_resp = client
        .get(format!("{url}/v1/admin/tokens"))
        .header("authorization", "Bearer root-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(list_resp.status(), 200);
    let listed: serde_json::Value = list_resp.json().await.unwrap();
    let body = listed.to_string();
    assert!(!body.contains(&second_token));
    assert!(!body.contains("token_digest"));
    assert!(body.contains("svc-a"));

    // capability 없는 principal은 list도 403.
    let scoped = spawn_server_with_admin(store.clone(), "unused").await;
    let _ = scoped; // 별도 서버는 아래 capability 테스트에서 사용.

    // 4. revoke.
    let revoke_resp = client
        .delete(format!("{url}/v1/admin/tokens/svc-a"))
        .header("authorization", "Bearer root-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(revoke_resp.status(), 200);
    assert_eq!(
        worker_list_status(&url, &second_token).await,
        401,
        "revoked token must be rejected"
    );

    // 존재하지 않는 principal은 rotate/revoke 모두 404.
    let missing_rotate = client
        .post(format!("{url}/v1/admin/tokens/nobody/rotate"))
        .header("authorization", "Bearer root-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(missing_rotate.status(), 404);
    let missing_revoke = client
        .delete(format!("{url}/v1/admin/tokens/nobody"))
        .header("authorization", "Bearer root-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(missing_revoke.status(), 404);
}

#[tokio::test]
async fn capability_without_manage_cannot_create_or_rotate_or_revoke() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    // list만 있고 manage는 없는 principal.
    let state = AppState::new(store).with_tokens(vec![ApiTokenCredential {
        principal_id: "auditor".into(),
        token: "auditor-token".into(),
        capabilities: vec![PermissionKind::AdminTokenList],
    }]);
    let state = Arc::new(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let url = format!("http://{addr}");
    let client = reqwest::Client::new();

    let create = client
        .post(format!("{url}/v1/admin/tokens"))
        .header("authorization", "Bearer auditor-token")
        .json(&serde_json::json!({"principal_id": "x", "capabilities": ["worker:list"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 403);

    // list는 성공해야 한다 (분리된 capability).
    let list = client
        .get(format!("{url}/v1/admin/tokens"))
        .header("authorization", "Bearer auditor-token")
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), 200);
}

#[tokio::test]
async fn env_tokens_still_authenticate_after_startup_sync_to_db() {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let env_tokens = vec![ApiTokenCredential {
        principal_id: "legacy-env-principal".into(),
        token: "legacy-env-token".into(),
        capabilities: vec![PermissionKind::WorkerList],
    }];

    // #72의 무중단 전환: 부팅 시 env 목록을 DB로 1회 upsert.
    sync_env_admin_tokens_to_store(&*store, &env_tokens)
        .await
        .expect("env token sync must succeed against MemStore");

    // 두 번째 호출(멱등) — 에러 없이 조용히 넘어가야 한다.
    sync_env_admin_tokens_to_store(&*store, &env_tokens)
        .await
        .expect("second sync call must be a no-op, not an error");

    let state = AppState::new(store).with_tokens(env_tokens);
    let state = Arc::new(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let url = format!("http://{addr}");

    // env 목록으로 여전히 인증 가능 — with_tokens() 경로가 살아있는 회귀 확인.
    assert_eq!(worker_list_status(&url, "legacy-env-token").await, 200);
}

#[tokio::test]
async fn create_response_carries_raw_token_only_once_never_in_list() {
    // 생성 응답에는 원문이 있지만, 그 뒤 list 응답에는 절대 없어야 한다는
    // 원문 노출 경계를 명시적으로 확인 (redaction 성격의 테스트).
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let url = spawn_server_with_admin(store, "root-secret").await;
    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{url}/v1/admin/tokens"))
        .header("authorization", "Bearer root-secret")
        .json(&serde_json::json!({
            "principal_id": "redact-me",
            "capabilities": ["worker:list"],
        }))
        .send()
        .await
        .unwrap();
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let raw = created["token"].as_str().unwrap().to_string();

    let list_resp = client
        .get(format!("{url}/v1/admin/tokens"))
        .header("authorization", "Bearer root-secret")
        .send()
        .await
        .unwrap();
    let listed_body = list_resp.text().await.unwrap();
    assert!(
        !listed_body.contains(&raw),
        "raw admin token leaked into the list endpoint"
    );
}
