//! `Store::rotate_worker_operational_credential` / `revoke_worker_operational_credential`
//! PostgreSQL 통합 테스트 (로드맵 #60 6단계).
//!
//! 실제 PostgreSQL 데이터베이스가 필요합니다. `DATABASE_URL` 환경변수가
//! 설정되지 않으면 모든 테스트가 자동으로 skip됩니다 (`tests/enroll_worker.rs`와
//! 동일한 규약).
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test worker_credential_rotation -- --test-threads=1
//! ```

use chrono::{Duration, Utc};
use fleet_core::Worker;
use fleet_store::{PgStore, Store, StoreError, WorkerOperationalCredential};
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
        let _ =
            sqlx::query("TRUNCATE task_outputs, events, tasks, workers, bootstrap_tokens CASCADE")
                .execute($store.pool())
                .await;
    };
}

fn worker(name: &str) -> Worker {
    Worker::new(name, format!("wss://{name}.fleet.example.com/ws"))
}

async fn seed_credential(store: &PgStore, worker_id: fleet_core::WorkerId, digest: &str) {
    store
        .upsert_worker_operational_credential(&WorkerOperationalCredential {
            worker_id,
            credential_digest: digest.to_string(),
            issued_at: Utc::now(),
            expires_at: None,
            revoked_at: None,
            rotation_generation: 1,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn rotate_replaces_digest_and_increments_generation() {
    require_db!(store);
    let w = worker("rotate-pg");
    store.upsert_worker(&w).await.unwrap();
    seed_credential(&store, w.id, "rotate-pg-old-digest").await;

    let rotated = store
        .rotate_worker_operational_credential(w.id, "rotate-pg-new-digest", None)
        .await
        .expect("rotate should succeed");
    assert_eq!(rotated.credential_digest, "rotate-pg-new-digest");
    assert_eq!(rotated.rotation_generation, 2);
    assert!(rotated.revoked_at.is_none());

    // old digest는 더 이상 활성이 아니다.
    assert!(store
        .find_active_worker_operational_credential("rotate-pg-old-digest")
        .await
        .unwrap()
        .is_none());
    // new digest는 활성.
    assert!(store
        .find_active_worker_operational_credential("rotate-pg-new-digest")
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn rotate_unknown_worker_returns_not_found() {
    require_db!(store);
    let result = store
        .rotate_worker_operational_credential(
            fleet_core::WorkerId::new(),
            "irrelevant-digest",
            None,
        )
        .await;
    assert!(
        matches!(result, Err(StoreError::NotFound)),
        "got {result:?}"
    );
}

#[tokio::test]
async fn revoke_marks_inactive_and_reports_not_found_on_second_call() {
    require_db!(store);
    let w = worker("revoke-pg");
    store.upsert_worker(&w).await.unwrap();
    seed_credential(&store, w.id, "revoke-pg-digest").await;

    let revoked = store
        .revoke_worker_operational_credential(w.id)
        .await
        .unwrap();
    assert!(revoked);

    assert!(store
        .find_active_worker_operational_credential("revoke-pg-digest")
        .await
        .unwrap()
        .is_none());

    // 이미 회수된 credential을 다시 회수하면 false.
    let revoked_again = store
        .revoke_worker_operational_credential(w.id)
        .await
        .unwrap();
    assert!(!revoked_again);
}

#[tokio::test]
async fn expired_credential_is_excluded_from_active_lookup() {
    require_db!(store);
    let w = worker("expired-pg");
    store.upsert_worker(&w).await.unwrap();
    store
        .upsert_worker_operational_credential(&WorkerOperationalCredential {
            worker_id: w.id,
            credential_digest: "expired-pg-digest".to_string(),
            issued_at: Utc::now() - Duration::hours(2),
            expires_at: Some(Utc::now() - Duration::hours(1)),
            revoked_at: None,
            rotation_generation: 1,
        })
        .await
        .unwrap();

    assert!(store
        .find_active_worker_operational_credential("expired-pg-digest")
        .await
        .unwrap()
        .is_none());
}
