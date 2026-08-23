//! `Store::acquire_control_lease` / `renew_control_lease` / `release_control_lease` /
//! `get_control_lease` PostgreSQL 통합 테스트 (로드맵 #63, 1단계).
//!
//! Fleet는 유효한 dispatch lease를 가진 Orchestrator가 최대 하나여야 한다
//! (`docs/architecture/control-plane-authority-and-failover.md`). 이 파일은
//! 그 불변식을 강제하는 CAS(조건부 UPDATE/INSERT) 자체가 실제 Postgres
//! concurrency 아래서도 정확히 동작하는지 검증한다 — 특히 "동시에 둘이
//! lease를 획득하지 못한다"(구현 게이트 1)는 mock이 아니라 실제 DB에 대고
//! 동시 요청을 날려서 확인해야 의미가 있다.
//!
//! 실제 PostgreSQL 데이터베이스가 필요합니다. `DATABASE_URL` 환경변수가
//! 설정되지 않으면 모든 테스트가 자동으로 skip됩니다 (`tests/admin_token_rotation.rs`와
//! 동일한 규약).
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test control_plane_lease -- --test-threads=1
//! ```

use std::time::Duration as StdDuration;

use fleet_store::{PgStore, Store, StoreError};
use sqlx::postgres::PgPoolOptions;

fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

async fn try_connect() -> Option<PgStore> {
    let url = database_url()?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
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
        let _ = sqlx::query("TRUNCATE control_plane_lease")
            .execute($store.pool())
            .await;
    };
}

/// 테스트마다 다른 cluster_id를 써서 (혹시 남을 수 있는) 서로 다른 테스트
/// 간 간섭을 피한다. `#[tokio::test]`는 병렬 실행이 기본이라, 같은
/// cluster_id를 공유하면 이 파일 자체가 테스트하려는 "동시 획득 경합"을
/// 의도치 않게 트리거해 flaky해진다.
fn cluster(name: &str) -> String {
    format!("test-{name}-{}", uuid::Uuid::new_v4())
}

#[tokio::test]
async fn first_acquire_succeeds_with_epoch_one() {
    require_db!(store);
    let cluster_id = cluster("first-acquire");

    let lease = store
        .acquire_control_lease(&cluster_id, "instance-a", StdDuration::from_secs(30))
        .await
        .unwrap();

    assert_eq!(lease.cluster_id, cluster_id);
    assert_eq!(lease.active_instance_id, "instance-a");
    assert_eq!(lease.epoch, 1);
    assert!(lease.expires_at > lease.acquired_at);
}

#[tokio::test]
async fn second_acquire_by_different_instance_is_refused_while_valid() {
    require_db!(store);
    let cluster_id = cluster("refuse-while-valid");

    store
        .acquire_control_lease(&cluster_id, "instance-a", StdDuration::from_secs(30))
        .await
        .unwrap();

    let err = store
        .acquire_control_lease(&cluster_id, "instance-b", StdDuration::from_secs(30))
        .await
        .expect_err("a second instance must not acquire a still-valid lease");
    assert!(matches!(err, StoreError::Conflict(_)));

    // 거절됐다고 해서 기존 소유권이 바뀌면 안 된다.
    let current = store
        .get_control_lease(&cluster_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.active_instance_id, "instance-a");
    assert_eq!(current.epoch, 1);
}

#[tokio::test]
async fn acquire_after_expiry_is_taken_over_with_incremented_epoch() {
    require_db!(store);
    let cluster_id = cluster("takeover-after-expiry");

    store
        .acquire_control_lease(&cluster_id, "instance-a", StdDuration::from_millis(1))
        .await
        .unwrap();

    // DB 서버 시각 기준 만료를 기다린다 — 애플리케이션 시각이 아니라 실제
    // NOW()가 지나야 한다는 걸 확인하려는 것이므로 넉넉히 잡는다.
    tokio::time::sleep(StdDuration::from_millis(200)).await;

    let taken_over = store
        .acquire_control_lease(&cluster_id, "instance-b", StdDuration::from_secs(30))
        .await
        .expect("an expired lease must be takeable by a new instance");
    assert_eq!(taken_over.active_instance_id, "instance-b");
    assert_eq!(taken_over.epoch, 2, "epoch must increment on takeover");
}

#[tokio::test]
async fn renew_extends_expiry_and_keeps_epoch() {
    require_db!(store);
    let cluster_id = cluster("renew-extends");

    let first = store
        .acquire_control_lease(&cluster_id, "instance-a", StdDuration::from_secs(5))
        .await
        .unwrap();

    let renewed = store
        .renew_control_lease(
            &cluster_id,
            "instance-a",
            first.epoch,
            StdDuration::from_secs(60),
        )
        .await
        .unwrap();

    assert_eq!(renewed.epoch, first.epoch, "renew must not change the epoch");
    assert_eq!(renewed.active_instance_id, "instance-a");
    assert!(
        renewed.expires_at > first.expires_at,
        "renew must push expires_at further out"
    );
}

#[tokio::test]
async fn renew_with_stale_epoch_fails() {
    require_db!(store);
    let cluster_id = cluster("renew-stale-epoch");

    store
        .acquire_control_lease(&cluster_id, "instance-a", StdDuration::from_millis(1))
        .await
        .unwrap();
    tokio::time::sleep(StdDuration::from_millis(200)).await;
    // instance-b가 만료된 lease를 가로챈다 (epoch 2).
    store
        .acquire_control_lease(&cluster_id, "instance-b", StdDuration::from_secs(30))
        .await
        .unwrap();

    // instance-a는 자기가 여전히 epoch 1의 주인이라고 믿고 갱신을 시도하지만,
    // 이미 다른 instance가 새 epoch를 가져갔으므로 실패해야 한다 — "이전
    // epoch의 늦은 이벤트는 상태를 변경하지 못한다"(구현 게이트 3)의 lease
    // 계층 대응.
    let err = store
        .renew_control_lease(&cluster_id, "instance-a", 1, StdDuration::from_secs(30))
        .await
        .expect_err("renewing with a stale epoch must fail");
    assert!(matches!(err, StoreError::NotFound));
}

#[tokio::test]
async fn renew_after_expiry_fails_even_with_correct_instance_and_epoch() {
    require_db!(store);
    let cluster_id = cluster("renew-after-expiry");

    let lease = store
        .acquire_control_lease(&cluster_id, "instance-a", StdDuration::from_millis(1))
        .await
        .unwrap();
    tokio::time::sleep(StdDuration::from_millis(200)).await;

    // 아무도 아직 가로채지 않았더라도, lease 자체가 이미 만료됐으면 갱신은
    // 실패해야 한다 — instance_id·epoch만 맞으면 만료된 lease를 "갱신"이라는
    // 이름으로 되살릴 수 있다면, 그 사이 다른 instance가 이미 유효하다고
    // 믿고 있는 경우와 경합이 생긴다.
    let err = store
        .renew_control_lease(
            &cluster_id,
            "instance-a",
            lease.epoch,
            StdDuration::from_secs(30),
        )
        .await
        .expect_err("renewing an already-expired lease must fail, not resurrect it");
    assert!(matches!(err, StoreError::NotFound));
}

#[tokio::test]
async fn release_lets_another_instance_acquire_immediately() {
    require_db!(store);
    let cluster_id = cluster("release-immediate");

    let lease = store
        .acquire_control_lease(&cluster_id, "instance-a", StdDuration::from_secs(60))
        .await
        .unwrap();

    let released = store
        .release_control_lease(&cluster_id, "instance-a", lease.epoch)
        .await
        .unwrap();
    assert!(released);

    // release 직후, 아직 60초 TTL이 한참 남았어야 정상인데도 다른 instance가
    // 곧바로 획득할 수 있어야 한다 — TTL 만료를 기다리지 않는 정상 종료 경로.
    let taken = store
        .acquire_control_lease(&cluster_id, "instance-b", StdDuration::from_secs(30))
        .await
        .expect("release must let a new instance acquire without waiting out the TTL");
    assert_eq!(taken.active_instance_id, "instance-b");
    assert_eq!(taken.epoch, 2);
}

#[tokio::test]
async fn release_with_wrong_instance_is_a_noop() {
    require_db!(store);
    let cluster_id = cluster("release-wrong-instance");

    let lease = store
        .acquire_control_lease(&cluster_id, "instance-a", StdDuration::from_secs(60))
        .await
        .unwrap();

    let released = store
        .release_control_lease(&cluster_id, "instance-b", lease.epoch)
        .await
        .unwrap();
    assert!(
        !released,
        "an instance must not be able to release a lease it doesn't own"
    );

    let current = store
        .get_control_lease(&cluster_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.active_instance_id, "instance-a");
    assert!(
        current.expires_at > chrono::Utc::now(),
        "the real owner's lease must remain valid"
    );
}

#[tokio::test]
async fn get_control_lease_returns_none_before_first_acquire() {
    require_db!(store);
    let cluster_id = cluster("get-before-acquire");

    assert!(store
        .get_control_lease(&cluster_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn concurrent_acquire_attempts_only_one_wins() {
    // 구현 게이트 1: "동시에 둘이 lease를 획득하지 못하는 통합 테스트".
    // 실제 Postgres connection pool로 여러 instance의 동시 acquire를
    // 흉내낸다 — mock/in-memory로는 이 보장이 진짜인지 확인할 수 없다.
    require_db!(store);
    let cluster_id = cluster("concurrent-acquire");
    let store = std::sync::Arc::new(store);

    const N: usize = 8;
    let mut handles = Vec::with_capacity(N);
    for i in 0..N {
        let store = store.clone();
        let cluster_id = cluster_id.clone();
        handles.push(tokio::spawn(async move {
            store
                .acquire_control_lease(
                    &cluster_id,
                    &format!("instance-{i}"),
                    StdDuration::from_secs(60),
                )
                .await
        }));
    }

    let mut wins = 0;
    let mut conflicts = 0;
    for h in handles {
        match h.await.unwrap() {
            Ok(_) => wins += 1,
            Err(StoreError::Conflict(_)) => conflicts += 1,
            Err(e) => panic!("unexpected error from concurrent acquire: {e}"),
        }
    }

    assert_eq!(wins, 1, "exactly one concurrent acquirer must win");
    assert_eq!(conflicts, N - 1);

    let final_lease = store
        .get_control_lease(&cluster_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(final_lease.epoch, 1);
}
