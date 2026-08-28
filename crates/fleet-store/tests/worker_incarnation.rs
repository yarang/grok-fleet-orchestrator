//! `workers.incarnation_started_at`의 백엔드 공유 행동 (로드맵 `#67` 2단계).
//!
//! 이 컬럼은 "이 시각보다 앞서 디스패치된 작업은 이전 프로세스의 고아"라는
//! 회수 술어의 한쪽 변이다. 따라서 **언제 움직이고 언제 움직이지 않는가**가
//! 값 자체보다 중요하다:
//!
//! - `upsert_worker`는 절대 움직이지 않는다. heartbeat도 이 경로를 타므로,
//!   여기서 값이 흔들리면 하트비트 한 번에 그 워커의 진행 중 작업이 전부
//!   고아로 판정된다.
//! - `bump_worker_incarnation`만 움직인다. 재등록(=프로세스 재시작)을 감지한
//!   쪽만 호출한다.
//!
//! PgStore는 `ON CONFLICT` 갱신 목록에서 컬럼을 빼는 방식으로, MemStore는 통짜
//! insert 전에 기존 값을 되살리는 방식으로 서로 다르게 구현하므로, 행동이
//! 갈라지면 여기서 잡혀야 한다.
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --all-features --test worker_incarnation -- --test-threads=1
//! ```

use std::sync::Arc;

use fleet_core::{Worker, WorkerId};
use fleet_store::mem::MemStore;
use fleet_store::{PgStore, Store};
use sqlx::postgres::PgPoolOptions;

// ── 백엔드 준비 (task_cas.rs와 같은 형태) ───────────────────────────────

async fn mem_backend() -> Arc<dyn Store> {
    Arc::new(MemStore::new())
}

async fn pg_backend() -> Option<Arc<dyn Store>> {
    let url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but connection failed: {e}"));
    let store = PgStore::from_pool(pool);
    store
        .migrate()
        .await
        .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but migration failed: {e}"));
    sqlx::query("TRUNCATE task_outputs, events, tasks, worker_credentials, workers CASCADE")
        .execute(store.pool())
        .await
        .expect("truncate");
    Some(Arc::new(store))
}

macro_rules! both_backends {
    ($name:ident, $body:expr) => {
        #[tokio::test]
        async fn $name() {
            let scenario: fn(Arc<dyn Store>) -> _ = $body;
            scenario(mem_backend().await).await;
            if let Some(pg) = pg_backend().await {
                scenario(pg).await;
            }
        }
    };
}

fn seed(name: &str) -> Worker {
    Worker::new(name, format!("wss://{name}/ws"))
}

// ── 테스트 ──────────────────────────────────────────────────────────────

both_backends!(insert_stamps_an_incarnation, |store| async move {
    let w = seed("fresh");
    store.upsert_worker(&w).await.unwrap();
    let stored = store.get_worker(w.id).await.unwrap().expect("worker");
    assert_eq!(
        stored.incarnation_started_at, w.incarnation_started_at,
        "최초 INSERT는 구조체가 들고 온 값을 그대로 남겨야 한다"
    );
});

both_backends!(upsert_does_not_move_the_incarnation, |store| async move {
    // heartbeat가 타는 경로. 구조체에 **다른** 값을 실어 보내도 저장된 값은
    // 움직이지 않아야 한다 — 여기가 흔들리면 하트비트마다 고아 판정이 난다.
    let w = seed("heartbeating");
    store.upsert_worker(&w).await.unwrap();
    let first = store
        .get_worker(w.id)
        .await
        .unwrap()
        .unwrap()
        .incarnation_started_at;

    let mut later = w.clone();
    later.incarnation_started_at = w.incarnation_started_at + chrono::Duration::hours(1);
    later.active_tasks = 3;
    store.upsert_worker(&later).await.unwrap();

    let stored = store.get_worker(w.id).await.unwrap().unwrap();
    assert_eq!(
        stored.incarnation_started_at, first,
        "upsert_worker가 incarnation을 움직였다"
    );
    assert_eq!(
        stored.active_tasks, 3,
        "다른 필드는 정상적으로 갱신돼야 한다 — 컬럼 하나만 제외되는 것이 의도"
    );
    assert_eq!(
        stored.registered_at, w.registered_at,
        "registered_at도 최초 값을 보존해야 한다(선례)"
    );
});

both_backends!(bump_moves_the_incarnation_forward, |store| async move {
    let w = seed("restarting");
    store.upsert_worker(&w).await.unwrap();
    let before = store
        .get_worker(w.id)
        .await
        .unwrap()
        .unwrap()
        .incarnation_started_at;

    let returned = store
        .bump_worker_incarnation(w.id)
        .await
        .unwrap()
        .expect("존재하는 워커의 bump는 새 시각을 돌려줘야 한다");
    assert!(
        returned >= before,
        "bump가 시각을 되돌렸다: {returned} < {before}"
    );

    let stored = store.get_worker(w.id).await.unwrap().unwrap();
    assert_eq!(
        stored.incarnation_started_at, returned,
        "반환값과 저장된 값이 달라서는 안 된다 — 호출자가 반환값으로 판정하기 때문"
    );
    assert_eq!(
        stored.registered_at, w.registered_at,
        "bump는 최초 등록 시각을 건드리면 안 된다"
    );
});

both_backends!(bump_on_a_missing_worker_reports_none, |store| async move {
    // 조회와 bump 사이에 row가 삭제된 경합. 호출자(register 핸들러)가 이
    // `None`을 보고 "신규 등록으로 취급"할 수 있어야 하므로 에러가 아니다.
    let missing = store
        .bump_worker_incarnation(WorkerId::new())
        .await
        .unwrap();
    assert!(missing.is_none());
});
