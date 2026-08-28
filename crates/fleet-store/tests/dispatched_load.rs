//! `Store::count_dispatched_tasks_by_worker()`의 백엔드 공유 행동 (로드맵 `#67` 3단계).
//!
//! 이 카운트는 스케줄러의 **용량 판단 근거**다. 예전에는 워커가 하트비트로
//! 신고한 `Worker::active_tasks`를 읽었는데, 그 값은 최대 health interval만큼
//! 낡고 워커가 위조할 수도 있었다. 이제 오케스트레이터 자신이 기록한
//! `Dispatched` 행을 센다.
//!
//! 두 백엔드가 완전히 다른 방식으로 답한다 — PgStore는
//! `WHERE status_phase = 'dispatched' GROUP BY status->>'worker_id'` SQL로,
//! MemStore는 `TaskStatus::Dispatched { worker_id, .. }` 패턴 매칭으로. 그래서
//! **총합만 단정하면 부족하다**: GROUP BY 키가 틀려도 총합은 맞을 수 있다.
//! 워커별 분해까지 맞대야 한다.
//!
//! JSONB 경로가 특히 취약한 자리다. `TaskStatus`는 내부 태그
//! (`#[serde(tag = "phase")]`)라 페이로드가 최상위로 평탄화되므로 경로는
//! `status->>'worker_id'`이지 `status->'Dispatched'->>'worker_id'`가 아니다 —
//! 후자로 적어서 한 행도 매칭되지 않았던 것이 `3b0a846`의 버그였고, MemStore만
//! 쓰는 테스트로는 그것이 드러나지 않았다.
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --all-features --test dispatched_load -- --test-threads=1
//! ```

use std::sync::Arc;

use chrono::Utc;
use fleet_core::{FailureKind, Task, TaskFailure, TaskRequest, TaskResult, TaskStatus, WorkerId};
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

// ── 픽스처 ──────────────────────────────────────────────────────────────

async fn seed_task(store: &Arc<dyn Store>, prompt: &str, status: TaskStatus) -> Task {
    let mut task = Task::from_request(TaskRequest {
        prompt: prompt.into(),
        created_by: "test".into(),
        ..Default::default()
    });
    task.status = status;
    store.insert_task(&task).await.unwrap();
    task
}

fn dispatched(worker_id: WorkerId) -> TaskStatus {
    TaskStatus::Dispatched {
        worker_id,
        started_at: Utc::now(),
    }
}

fn completed(worker_id: WorkerId) -> TaskStatus {
    TaskStatus::Completed(TaskResult {
        output: "ok".into(),
        exit_code: 0,
        duration_secs: 1.0,
        token_usage: None,
        worker_id,
        finished_at: Utc::now(),
    })
}

fn failed(worker_id: WorkerId) -> TaskStatus {
    TaskStatus::Failed(TaskFailure {
        error: "worker vanished".into(),
        kind: FailureKind::WorkerUnavailable,
        worker_id: Some(worker_id),
        attempts: 0,
    })
}

// ── 테스트 ──────────────────────────────────────────────────────────────

both_backends!(empty_store_reports_no_load, |store| async move {
    let counts = store.count_dispatched_tasks_by_worker().await.unwrap();
    assert!(
        counts.is_empty(),
        "task가 없으면 빈 맵이어야 한다, got {counts:?}"
    );
});

both_backends!(counts_are_broken_down_per_worker, |store| async move {
    // 총합만 보면 GROUP BY 키가 틀려도 통과한다. 워커마다 다른 개수를 넣어
    // 분해가 맞는지 확인한다.
    let a = WorkerId::new();
    let b = WorkerId::new();
    seed_task(&store, "a1", dispatched(a)).await;
    seed_task(&store, "a2", dispatched(a)).await;
    seed_task(&store, "a3", dispatched(a)).await;
    seed_task(&store, "b1", dispatched(b)).await;

    let counts = store.count_dispatched_tasks_by_worker().await.unwrap();
    assert_eq!(counts.get(&a).copied(), Some(3), "counts={counts:?}");
    assert_eq!(counts.get(&b).copied(), Some(1), "counts={counts:?}");
    assert_eq!(counts.len(), 2, "다른 키가 섞이면 안 된다: {counts:?}");
});

both_backends!(terminal_tasks_do_not_occupy_capacity, |store| async move {
    // `Completed`/`Failed`도 페이로드에 `worker_id`를 갖는다. 그래서
    // `status->>'worker_id' IS NOT NULL`만으로 세면 끝난 작업까지 부하로
    // 잡히고, 그 워커는 시간이 갈수록 영영 선택되지 않게 된다.
    // `status_phase = 'dispatched'` 술어가 그것을 가른다.
    let w = WorkerId::new();
    seed_task(&store, "running", dispatched(w)).await;
    seed_task(&store, "done", completed(w)).await;
    seed_task(&store, "broken", failed(w)).await;

    let counts = store.count_dispatched_tasks_by_worker().await.unwrap();
    assert_eq!(
        counts.get(&w).copied(),
        Some(1),
        "진행 중인 1건만 세어야 한다, got {counts:?}"
    );
});

both_backends!(
    workers_without_load_are_absent_not_zero,
    |store| async move {
        // 반환은 희소 맵이다. 부하 없는 워커의 키를 만들지 않는다 —
        // 호출자는 `get(..).unwrap_or(0)`으로 읽는다.
        let busy = WorkerId::new();
        let idle = WorkerId::new();
        seed_task(&store, "t", dispatched(busy)).await;

        let counts = store.count_dispatched_tasks_by_worker().await.unwrap();
        assert_eq!(counts.get(&busy).copied(), Some(1));
        assert_eq!(counts.get(&idle).copied(), None);
        assert_eq!(counts.get(&idle).copied().unwrap_or(0), 0);
    }
);

both_backends!(pending_and_cancelled_carry_no_worker, |store| async move {
    // `Pending`은 `worker_id` 키 자체가 없고, `Cancelled`도 마찬가지다.
    // PgStore 쪽에서 이들이 NULL 그룹을 만들지 않는지 확인한다.
    seed_task(&store, "waiting", TaskStatus::Pending).await;
    seed_task(
        &store,
        "gone",
        TaskStatus::Cancelled {
            reason: "user asked".into(),
            cancelled_at: Utc::now(),
        },
    )
    .await;

    let counts = store.count_dispatched_tasks_by_worker().await.unwrap();
    assert!(counts.is_empty(), "counts={counts:?}");
});
