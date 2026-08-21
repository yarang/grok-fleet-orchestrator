//! `Store::create_admin_token` / `rotate_admin_token` / `revoke_admin_token` /
//! `find_active_admin_token_by_digest` / `list_admin_tokens` PostgreSQL 통합
//! 테스트 (로드맵 #72).
//!
//! 실제 PostgreSQL 데이터베이스가 필요합니다. `DATABASE_URL` 환경변수가
//! 설정되지 않으면 모든 테스트가 자동으로 skip됩니다 (`tests/enroll_worker.rs`와
//! 동일한 규약).
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test admin_token_rotation -- --test-threads=1
//! ```

use chrono::Utc;
use fleet_core::PermissionKind;
use fleet_store::{AdminApiToken, PgStore, Store, StoreError};
use sqlx::postgres::PgPoolOptions;

fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

async fn try_connect() -> Option<PgStore> {
    let url = database_url()?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but connection failed: {e}"));
    let store = PgStore::from_pool(pool);
    store
        .migrate()
        .await
        .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but migration failed: {e}"));
    Some(store)
}

macro_rules! require_db {
    ($store:ident) => {
        let $store = match try_connect().await {
            Some(s) => s,
            None => return,
        };
        let _ = sqlx::query("TRUNCATE admin_api_tokens CASCADE")
            .execute($store.pool())
            .await;
    };
}

fn token(principal_id: &str, digest: &str) -> AdminApiToken {
    AdminApiToken {
        principal_id: principal_id.to_string(),
        token_digest: digest.to_string(),
        capabilities: vec![PermissionKind::WorkerList],
        created_at: Utc::now(),
        rotated_at: None,
        revoked_at: None,
        rotation_generation: 1,
    }
}

#[tokio::test]
async fn create_then_find_active_by_digest() {
    require_db!(store);

    store
        .create_admin_token(&token("svc-a", "digest-a"))
        .await
        .unwrap();

    let found = store
        .find_active_admin_token_by_digest("digest-a")
        .await
        .unwrap()
        .expect("just-created token must be found by its digest");
    assert_eq!(found.principal_id, "svc-a");
    assert_eq!(found.capabilities, vec![PermissionKind::WorkerList]);
    assert_eq!(found.rotation_generation, 1);

    assert!(store
        .find_active_admin_token_by_digest("no-such-digest")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn duplicate_principal_id_conflicts() {
    require_db!(store);

    store
        .create_admin_token(&token("svc-dup", "digest-1"))
        .await
        .unwrap();
    let err = store
        .create_admin_token(&token("svc-dup", "digest-2"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::Conflict(_)),
        "duplicate principal_id must be a Conflict, got {err:?}"
    );
}

#[tokio::test]
async fn rotate_invalidates_previous_digest_and_activates_new_one() {
    require_db!(store);

    store
        .create_admin_token(&token("svc-rotate", "digest-old"))
        .await
        .unwrap();

    let rotated = store
        .rotate_admin_token("svc-rotate", "digest-new")
        .await
        .unwrap();
    assert_eq!(rotated.rotation_generation, 2);
    assert!(rotated.rotated_at.is_some());

    assert!(
        store
            .find_active_admin_token_by_digest("digest-old")
            .await
            .unwrap()
            .is_none(),
        "old digest must stop matching immediately after rotate"
    );
    let found = store
        .find_active_admin_token_by_digest("digest-new")
        .await
        .unwrap()
        .expect("new digest must be active");
    assert_eq!(found.rotation_generation, 2);
}

#[tokio::test]
async fn rotate_unknown_principal_returns_not_found() {
    require_db!(store);

    let err = store
        .rotate_admin_token("nobody", "digest-x")
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::NotFound));
}

#[tokio::test]
async fn revoke_marks_inactive_and_reports_not_found_on_second_call() {
    require_db!(store);

    store
        .create_admin_token(&token("svc-revoke", "digest-r"))
        .await
        .unwrap();

    let revoked = store.revoke_admin_token("svc-revoke").await.unwrap();
    assert!(revoked);
    assert!(store
        .find_active_admin_token_by_digest("digest-r")
        .await
        .unwrap()
        .is_none());

    let second = store.revoke_admin_token("svc-revoke").await.unwrap();
    assert!(!second, "revoking an already-revoked principal returns false, not an error");
}

#[tokio::test]
async fn list_never_exposes_token_digest_field_shape() {
    require_db!(store);

    store
        .create_admin_token(&token("svc-list-1", "digest-l1"))
        .await
        .unwrap();
    store
        .create_admin_token(&token("svc-list-2", "digest-l2"))
        .await
        .unwrap();
    store.revoke_admin_token("svc-list-2").await.unwrap();

    let listed = store.list_admin_tokens().await.unwrap();
    let names: Vec<&str> = listed.iter().map(|t| t.principal_id.as_str()).collect();
    assert!(names.contains(&"svc-list-1"));
    assert!(names.contains(&"svc-list-2"));

    let l2 = listed
        .iter()
        .find(|t| t.principal_id == "svc-list-2")
        .unwrap();
    assert!(l2.revoked_at.is_some());
    // AdminApiToken 자체는 digest 필드를 갖지만(store 레이어), API 레이어의
    // AdminTokenSummary 변환에서만 제외된다 — 여기서는 store가 digest를 실제로
    // 저장·반환하는지(즉 필드가 비어있지 않은지)를 확인해 회귀를 잡는다.
    let l1 = listed
        .iter()
        .find(|t| t.principal_id == "svc-list-1")
        .unwrap();
    assert_eq!(l1.token_digest, "digest-l1");
}
