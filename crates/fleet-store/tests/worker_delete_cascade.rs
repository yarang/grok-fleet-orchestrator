//! `Store::delete_worker`의 CASCADE 범위 통합 테스트 (로드맵 #78).
//!
//! `DELETE FROM workers`는 두 개의 `ON DELETE CASCADE`를 함께 발동시킨다 —
//! `worker_operational_credentials`(018, `worker_id`)와
//! `worker_credentials`(005, `worker_name`, 암호화된 LLM 키). 이 파괴 범위는
//! **관리자가 명시적으로 워커를 제거할 때만** 일어나야 하며, 워커 데몬이
//! 정상 종료하면서 스스로 트리거해서는 안 된다(`fleet-worker`의 shutdown
//! 경로에서 `deregister` 호출을 제거한 이유).
//!
//! 같은 계약을 MemStore에 대해 검증하는 테스트가
//! `crates/fleet-store/src/mem.rs`의 `delete_worker_cascade_tests`에 있다. 두
//! 구현이 갈라지면 이 결함이 다시 인메모리 테스트를 통과하게 된다.
//!
//! 실제 PostgreSQL이 필요하다. `DATABASE_URL`이 없으면 skip한다.
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test worker_delete_cascade -- --test-threads=1
//! ```

use chrono::Utc;
use fleet_core::Worker;
use fleet_store::{PgStore, Store, StoreError, WorkerOperationalCredential};
use sqlx::postgres::PgPoolOptions;

async fn try_connect() -> Option<PgStore> {
    let url = std::env::var("DATABASE_URL").ok()?;
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
        let _ = sqlx::query(
            "TRUNCATE task_outputs, events, tasks, worker_credentials, workers, bootstrap_tokens CASCADE",
        )
        .execute($store.pool())
        .await;
    };
}

fn worker(name: &str) -> Worker {
    Worker::new(name, format!("wss://{name}.fleet.example.com/ws"))
}

async fn put_credential(store: &PgStore, worker_name: &str, model_id: &str) {
    store
        .upsert_worker_credential(
            worker_name,
            model_id,
            "encrypted",
            "https://api.example.test",
            "chat_completions",
            200_000,
            None,
        )
        .await
        .unwrap();
}

/// 관리자 삭제는 신원과 LLM credential을 함께 제거하고, 다른 워커는 건드리지 않는다.
#[tokio::test]
async fn delete_worker_cascades_both_credential_tables() {
    require_db!(store);

    let target = worker("cascade-target");
    let bystander = worker("cascade-bystander");
    store.upsert_worker(&target).await.unwrap();
    store.upsert_worker(&bystander).await.unwrap();

    for (w, digest) in [
        (&target, "cascade-target-digest"),
        (&bystander, "cascade-bystander-digest"),
    ] {
        store
            .upsert_worker_operational_credential(&WorkerOperationalCredential {
                worker_id: w.id,
                credential_digest: digest.to_string(),
                issued_at: Utc::now(),
                expires_at: None,
                revoked_at: None,
                rotation_generation: 1,
            })
            .await
            .unwrap();
    }

    put_credential(&store, "cascade-target", "grok-4").await;
    put_credential(&store, "cascade-target", "claude-opus").await;
    put_credential(&store, "cascade-bystander", "grok-4").await;

    store.delete_worker(target.id).await.unwrap();

    assert!(
        store
            .find_active_worker_operational_credential("cascade-target-digest")
            .await
            .unwrap()
            .is_none(),
        "deleted worker's operational credential must not authenticate"
    );
    assert!(
        store
            .list_worker_credentials("cascade-target")
            .await
            .unwrap()
            .is_empty(),
        "deleted worker's encrypted LLM credentials are destroyed by the CASCADE"
    );

    assert!(
        store
            .find_active_worker_operational_credential("cascade-bystander-digest")
            .await
            .unwrap()
            .is_some(),
        "unrelated worker's operational credential must survive"
    );
    assert_eq!(
        store
            .list_worker_credentials("cascade-bystander")
            .await
            .unwrap()
            .len(),
        1,
        "unrelated worker's LLM credentials must survive"
    );
}

/// MemStore가 맞춰야 할 기준: 존재하지 않는 id는 `NotFound`.
#[tokio::test]
async fn delete_worker_returns_not_found_for_unknown_id() {
    require_db!(store);
    let result = store.delete_worker(fleet_core::WorkerId::new()).await;
    assert!(matches!(result, Err(StoreError::NotFound)));
}
