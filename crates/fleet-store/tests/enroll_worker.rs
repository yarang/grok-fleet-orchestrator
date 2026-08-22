//! `Store::enroll_worker` 원자성(all-or-nothing) 통합 테스트 (로드맵 #60).
//!
//! 실제 PostgreSQL 데이터베이스가 필요합니다. `DATABASE_URL` 환경변수가
//! 설정되지 않으면 모든 테스트가 자동으로 skip됩니다 (`tests/integration.rs`와
//! 동일한 규약 — URL 미설정은 skip, 연결/마이그레이션 실패는 panic).
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test enroll_worker -- --test-threads=1
//! ```

use chrono::Utc;
use fleet_core::{BootstrapToken, Worker};
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

/// 테스트 헬퍼: 스토어 초기화 + 클린업. 연결 불가 시 early return(skip).
macro_rules! require_db {
    ($store:ident) => {
        let $store = match try_connect().await {
            Some(s) => s,
            None => return,
        };
        // workers CASCADE는 worker_operational_credentials(FK ON DELETE CASCADE)도 함께 비운다.
        let _ = sqlx::query("TRUNCATE task_outputs, events, tasks, workers, bootstrap_tokens CASCADE")
            .execute($store.pool())
            .await;
    };
}

fn token(raw: &str, max_uses: u32) -> BootstrapToken {
    BootstrapToken {
        token_digest: BootstrapToken::digest_for(raw),
        created_at: Utc::now(),
        created_by: None,
        expires_at: None,
        max_uses,
        use_count: 0,
        notes: None,
        last_used_by: None,
        last_used_at: None,
    }
}

fn worker(name: &str) -> Worker {
    Worker::new(name, format!("wss://{name}.fleet.example.com/ws"))
}

/// 로드맵 #60 완료 게이트 — 중간 실패 rollback test (PgStore).
///
/// credential digest UNIQUE 제약 위반으로 마지막 INSERT가 실패하면 트랜잭션
/// 전체가 롤백되어야 한다 — bootstrap token도 소비되지 않고 worker도 남지 않는다.
#[tokio::test]
async fn enroll_worker_rolls_back_on_credential_digest_conflict() {
    require_db!(store);

    // 기존 워커 + credential을 미리 심어 digest 충돌을 유도.
    let existing = worker("existing-worker-pg");
    store.upsert_worker(&existing).await.unwrap();
    store
        .upsert_worker_operational_credential(&WorkerOperationalCredential {
            worker_id: existing.id,
            credential_digest: "dup-digest-pg".to_string(),
            issued_at: Utc::now(),
            expires_at: None,
            revoked_at: None,
            rotation_generation: 1,
        })
        .await
        .unwrap();

    store
        .create_bootstrap_token(&token("pg-join-token", 1))
        .await
        .unwrap();

    let new_worker = worker("new-worker-pg");
    let new_credential = WorkerOperationalCredential {
        worker_id: new_worker.id,
        credential_digest: "dup-digest-pg".to_string(), // 기존 credential과 충돌
        issued_at: Utc::now(),
        expires_at: None,
        revoked_at: None,
        rotation_generation: 1,
    };

    let result = store
        .enroll_worker(
            "pg-join-token",
            "new-worker-pg",
            &new_worker,
            &new_credential,
        )
        .await;

    assert!(
        matches!(result, Err(StoreError::Conflict(_))),
        "expected Conflict on digest collision, got {result:?}"
    );

    // (a) 호출은 에러를 반환했고 — 위에서 확인.
    // (b) bootstrap token은 여전히 미소비 상태.
    let tokens = store.list_bootstrap_tokens().await.unwrap();
    let stored = tokens
        .iter()
        .find(|t| t.token_digest == BootstrapToken::digest_for("pg-join-token"))
        .expect("token still exists");
    assert_eq!(
        stored.use_count, 0,
        "token must remain unconsumed after rollback"
    );
    assert!(stored.last_used_by.is_none());

    // (c) worker는 생성되지 않음.
    assert!(store
        .get_worker_by_name("new-worker-pg")
        .await
        .unwrap()
        .is_none());
}

/// 이름 충돌(workers.name UNIQUE)로 두 번째 단계가 실패하는 경우도 동일하게
/// 전체 롤백되어야 한다.
#[tokio::test]
async fn enroll_worker_rolls_back_on_name_conflict() {
    require_db!(store);

    let existing = worker("taken-name-pg");
    store.upsert_worker(&existing).await.unwrap();
    store
        .create_bootstrap_token(&token("pg-join-token-2", 1))
        .await
        .unwrap();

    let mut colliding_worker = worker("taken-name-pg");
    colliding_worker.id = fleet_core::WorkerId::new();
    let credential = WorkerOperationalCredential {
        worker_id: colliding_worker.id,
        credential_digest: "unique-digest-pg".to_string(),
        issued_at: Utc::now(),
        expires_at: None,
        revoked_at: None,
        rotation_generation: 1,
    };

    let result = store
        .enroll_worker(
            "pg-join-token-2",
            "taken-name-pg",
            &colliding_worker,
            &credential,
        )
        .await;
    assert!(
        matches!(result, Err(StoreError::Conflict(_))),
        "expected Conflict on name collision, got {result:?}"
    );

    let tokens = store.list_bootstrap_tokens().await.unwrap();
    let stored = tokens
        .iter()
        .find(|t| t.token_digest == BootstrapToken::digest_for("pg-join-token-2"))
        .unwrap();
    assert_eq!(
        stored.use_count, 0,
        "token must remain unconsumed after rollback"
    );

    assert!(store
        .find_active_worker_operational_credential("unique-digest-pg")
        .await
        .unwrap()
        .is_none());
}

/// 정상 경로 — 세 단계가 모두 하나의 트랜잭션으로 커밋됨을 확인 (rollback
/// 테스트와의 대조군).
#[tokio::test]
async fn enroll_worker_commits_all_three_on_success() {
    require_db!(store);

    store
        .create_bootstrap_token(&token("pg-success-token", 1))
        .await
        .unwrap();

    let new_worker = worker("success-worker-pg");
    let credential = WorkerOperationalCredential {
        worker_id: new_worker.id,
        credential_digest: "success-digest-pg".to_string(),
        issued_at: Utc::now(),
        expires_at: None,
        revoked_at: None,
        rotation_generation: 1,
    };

    store
        .enroll_worker(
            "pg-success-token",
            "success-worker-pg",
            &new_worker,
            &credential,
        )
        .await
        .expect("enroll should succeed");

    let tokens = store.list_bootstrap_tokens().await.unwrap();
    let stored = tokens
        .iter()
        .find(|t| t.token_digest == BootstrapToken::digest_for("pg-success-token"))
        .unwrap();
    assert_eq!(stored.use_count, 1);
    assert_eq!(stored.last_used_by.as_deref(), Some("success-worker-pg"));

    assert!(store
        .get_worker_by_name("success-worker-pg")
        .await
        .unwrap()
        .is_some());
    assert!(store
        .find_active_worker_operational_credential("success-digest-pg")
        .await
        .unwrap()
        .is_some());
}
