//! `compare_and_set_task_status`의 백엔드 공유 행동 테스트 (로드맵 #62 stage 1).
//!
//! 조건 없는 `update_task_status`는 마지막에 도착한 쓰기가 이긴다. 그래서 이미
//! `Failed`로 확정된 작업에 늦은 `Completed`가 덮어써지는 일이 실제로 가능했다
//! (`fleet-scheduler`의 `reconcile.rs` 모듈 주석이 이 간극을 명시적으로 기록해
//! 두고 있었다). 이 파일은 CAS가 그 창을 닫는지를 **두 백엔드에서 같은
//! 시나리오로** 확인한다 — MemStore는 락 안에서, PgStore는 `WHERE` 절로 서로
//! 다르게 구현하므로, 행동이 갈라지면 여기서 잡혀야 한다.
//!
//! `DATABASE_URL`이 없으면 PgStore 케이스만 skip되고 MemStore 케이스는 항상
//! 실행된다.
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --all-features --test task_cas -- --test-threads=1
//! ```
//!
//! `--all-features` 없이 돌리면 `MemStore`(`test-support` 피처)가 사라진다.

use std::sync::Arc;

use chrono::Utc;
use fleet_core::{
    FailureKind, Task, TaskFailure, TaskPhase, TaskRequest, TaskResult, TaskStatus,
    TransitionOutcome, WorkerId,
};
use fleet_store::mem::MemStore;
use fleet_store::{ControlFence, PgStore, Store, StoreError};
use sqlx::postgres::PgPoolOptions;

// ── 백엔드 준비 ─────────────────────────────────────────────────────────

async fn mem_backend() -> Arc<dyn Store> {
    Arc::new(MemStore::new())
}

/// `DATABASE_URL`이 없으면 `None` — 호출부가 skip한다.
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
    sqlx::query("TRUNCATE issue_task_links, issue_comments, issues, tasks, projects CASCADE")
        .execute(store.pool())
        .await
        .expect("truncate");
    Some(Arc::new(store))
}

/// 두 백엔드에 같은 시나리오를 돌린다.
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

fn cancelled() -> TaskStatus {
    TaskStatus::Cancelled {
        reason: "user asked".into(),
        cancelled_at: Utc::now(),
    }
}

// ── 기본 계약 ───────────────────────────────────────────────────────────

both_backends!(cas_applies_when_the_phase_matches, |store| async move {
    let task = seed_task(&store, "match", TaskStatus::Pending).await;
    let worker = WorkerId::new();

    let outcome = store
        .compare_and_set_task_status(task.id, &[TaskPhase::Pending], &dispatched(worker), None)
        .await
        .unwrap();

    assert_eq!(outcome, TransitionOutcome::Applied);
    let stored = store.get_task(task.id).await.unwrap().unwrap();
    assert_eq!(stored.status.phase(), TaskPhase::Dispatched);
});

both_backends!(cas_rejects_when_the_phase_differs, |store| async move {
    let worker = WorkerId::new();
    // 작업은 이미 종료 상태다. `Dispatched`를 기대하는 쓰기는 통과하면 안 된다.
    let task = seed_task(&store, "differs", failed(worker)).await;

    let outcome = store
        .compare_and_set_task_status(task.id, &[TaskPhase::Dispatched], &completed(worker), None)
        .await
        .unwrap();

    assert_eq!(
        outcome,
        TransitionOutcome::Rejected {
            current: TaskPhase::Failed
        }
    );
    // 거절은 **아무것도 쓰지 않았다**는 뜻이다. 상태가 그대로인지 확인한다 —
    // 이 단언이 없으면 "결과값만 Rejected이고 실제로는 덮어썼다"를 통과시킨다.
    let stored = store.get_task(task.id).await.unwrap().unwrap();
    assert_eq!(stored.status.phase(), TaskPhase::Failed);
});

both_backends!(
    cas_accepts_any_of_several_expected_phases,
    |store| async move {
        // 취소 경로는 `[Pending, Dispatched]`를 기대한다. 두 위상 모두에서
        // 적용돼야 한다 — 한쪽만 통과하면 `= ANY($3)` 바인딩이 잘못된 것이다.
        for seed in [TaskStatus::Pending, dispatched(WorkerId::new())] {
            let task = seed_task(&store, "several", seed).await;
            let outcome = store
                .compare_and_set_task_status(
                    task.id,
                    &[TaskPhase::Pending, TaskPhase::Dispatched],
                    &cancelled(),
                    None,
                )
                .await
                .unwrap();
            assert_eq!(outcome, TransitionOutcome::Applied);
        }
    }
);

both_backends!(
    cas_reports_not_found_for_an_unknown_task,
    |store| async move {
        // 0행의 두 원인(행 없음 / 위상 불일치)을 구분하는지 확인한다. PgStore는
        // UPDATE가 0행일 때 한 번 더 SELECT해서 이를 가르므로, 이 케이스가 없으면
        // 존재하지 않는 작업이 조용히 `Rejected`로 보고될 수 있다.
        let ghost = Task::from_request(TaskRequest {
            prompt: "never inserted".into(),
            created_by: "test".into(),
            ..Default::default()
        });

        let err = store
            .compare_and_set_task_status(ghost.id, &[TaskPhase::Pending], &cancelled(), None)
            .await
            .expect_err("존재하지 않는 작업은 Err(NotFound)여야 한다");

        assert!(
            matches!(err, StoreError::NotFound),
            "expected NotFound, got {err:?}"
        );
    }
);

// ── dispatched_at 특례 보존 ─────────────────────────────────────────────

both_backends!(
    cas_stamps_dispatched_at_on_the_dispatch_transition,
    |store| async move {
        // 조건 없는 `update_task_status`가 `Dispatched` 전이에만 붙이던
        // `dispatched_at = NOW()`는 CAS 버전에서도 그대로여야 한다. 두 분기를
        // 하나의 쿼리로 합치면 조용히 사라지는 자리다.
        let task = seed_task(&store, "stamp", TaskStatus::Pending).await;
        assert!(task.dispatched_at.is_none());

        store
            .compare_and_set_task_status(
                task.id,
                &[TaskPhase::Pending],
                &dispatched(WorkerId::new()),
                None,
            )
            .await
            .unwrap();

        let stored = store.get_task(task.id).await.unwrap().unwrap();
        assert!(
            stored.dispatched_at.is_some(),
            "Dispatched 전이가 dispatched_at을 남기지 않았다"
        );
    }
);

both_backends!(
    cas_leaves_dispatched_at_untouched_on_terminal_transitions,
    |store| async move {
        let worker = WorkerId::new();
        let task = seed_task(&store, "no restamp", TaskStatus::Pending).await;
        store
            .compare_and_set_task_status(task.id, &[TaskPhase::Pending], &dispatched(worker), None)
            .await
            .unwrap();
        let after_dispatch = store.get_task(task.id).await.unwrap().unwrap();
        let stamped = after_dispatch.dispatched_at.expect("dispatched_at");

        store
            .compare_and_set_task_status(
                task.id,
                &[TaskPhase::Dispatched],
                &completed(worker),
                None,
            )
            .await
            .unwrap();

        let after_complete = store.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(
            after_complete.dispatched_at,
            Some(stamped),
            "종료 전이가 dispatched_at을 다시 찍었다"
        );
    }
);

// ── 로드맵 #62가 지목한 경합 ────────────────────────────────────────────

both_backends!(
    a_late_completion_cannot_overwrite_a_reconciled_failure,
    |store| async move {
        // 이것이 stage 1이 닫으려는 바로 그 경합이다.
        //
        // 1. 작업이 dispatch된다.
        // 2. reconciler가 담당 워커를 잃었다고 판단해 Failed로 확정한다.
        // 3. 워커가 실제로는 살아 있었고 뒤늦게 완료를 보고한다.
        //
        // 조건 없는 쓰기에서는 3번이 이겨 이미 확정된 실패가 완료로 뒤집혔다.
        let worker = WorkerId::new();
        let task = seed_task(&store, "late completion", dispatched(worker)).await;

        // 2. reconciler의 orphan 스윕 — `[Dispatched]`를 기대한다.
        let sweep = store
            .compare_and_set_task_status(task.id, &[TaskPhase::Dispatched], &failed(worker), None)
            .await
            .unwrap();
        assert_eq!(sweep, TransitionOutcome::Applied);

        // 3. 늦게 도착한 완료 이벤트 — 같은 `[Dispatched]`를 기대하므로 거절된다.
        let late = store
            .compare_and_set_task_status(
                task.id,
                &[TaskPhase::Dispatched],
                &completed(worker),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            late,
            TransitionOutcome::Rejected {
                current: TaskPhase::Failed
            }
        );

        let stored = store.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(
            stored.status.phase(),
            TaskPhase::Failed,
            "늦은 완료가 확정된 실패를 덮어썼다"
        );
    }
);

both_backends!(only_one_of_two_racing_cancels_applies, |store| async move {
    // 취소 경쟁(stage 1 게이트 2). CLI의 `tasks cancel`과 스케줄러의
    // `Dispatcher::cancel`은 서로 다른 프로세스라 상대를 볼 수 없다.
    let task = seed_task(&store, "double cancel", dispatched(WorkerId::new())).await;
    let expected = [TaskPhase::Pending, TaskPhase::Dispatched];

    let first = store
        .compare_and_set_task_status(task.id, &expected, &cancelled(), None)
        .await
        .unwrap();
    let second = store
        .compare_and_set_task_status(task.id, &expected, &cancelled(), None)
        .await
        .unwrap();

    assert_eq!(first, TransitionOutcome::Applied);
    assert_eq!(
        second,
        TransitionOutcome::Rejected {
            current: TaskPhase::Cancelled
        },
        "두 번째 취소도 적용되면 cancelled_at이 덮어써진다"
    );
});

both_backends!(
    a_completion_cannot_land_on_a_cancelled_task,
    |store| async move {
        let worker = WorkerId::new();
        let task = seed_task(&store, "cancel then complete", dispatched(worker)).await;

        store
            .compare_and_set_task_status(
                task.id,
                &[TaskPhase::Pending, TaskPhase::Dispatched],
                &cancelled(),
                None,
            )
            .await
            .unwrap();

        let late = store
            .compare_and_set_task_status(
                task.id,
                &[TaskPhase::Dispatched],
                &completed(worker),
                None,
            )
            .await
            .unwrap();

        assert_eq!(
            late,
            TransitionOutcome::Rejected {
                current: TaskPhase::Cancelled
            }
        );
    }
);

// ── control-plane epoch fence (로드맵 #62 3단계) ────────────────────────
//
// 위상 CAS는 "같은 Task를 두고 경쟁하는 writer"를 가른다. 아래 테스트들이
// 다루는 건 다른 경합이다 — **제어권을 잃은 인스턴스 전체**를 막는 것.
// 그 둘은 독립적이라 위상이 완벽히 맞아떨어져도 fence가 깨졌으면 거절돼야
// 한다.
//
// `control_plane_lease`는 `pg_backend()`의 TRUNCATE 대상이 아니므로 클러스터
// ID를 테스트마다 새로 만들어 격리한다 — 마이그레이션 021이 `cluster_id`를
// PK로 둔 이유 그대로다.

const TTL: std::time::Duration = std::time::Duration::from_secs(60);

fn unique_cluster() -> String {
    format!("cas-fence-{}", WorkerId::new())
}

/// lease를 한 번 획득하고 그 epoch로 fence를 만든다.
async fn acquire_fence(store: &Arc<dyn Store>, cluster: &str, instance: &str) -> ControlFence {
    let lease = store
        .acquire_control_lease(cluster, instance, TTL)
        .await
        .expect("lease acquisition must succeed on a fresh cluster id");
    ControlFence {
        cluster_id: cluster.to_string(),
        epoch: lease.epoch,
    }
}

both_backends!(
    cas_applies_under_the_current_control_epoch,
    |store| async move {
        let cluster = unique_cluster();
        let fence = acquire_fence(&store, &cluster, "instance-a").await;
        let task = seed_task(&store, "current epoch", TaskStatus::Pending).await;
        let worker = WorkerId::new();

        let outcome = store
            .compare_and_set_task_status(
                task.id,
                &[TaskPhase::Pending],
                &dispatched(worker),
                Some(&fence),
            )
            .await
            .unwrap();

        // fence는 정상 경로를 막지 않는다. 이 케이스가 없으면 아래 거절 테스트가
        // "항상 거절한다"는 구현으로도 통과한다.
        assert_eq!(outcome, TransitionOutcome::Applied);
        let stored = store.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(stored.status.phase(), TaskPhase::Dispatched);
    }
);

both_backends!(
    cas_is_fenced_when_a_newer_epoch_was_acquired,
    |store| async move {
        // 검증 게이트: "이전 control epoch 이벤트가 거부되는 테스트".
        let cluster = unique_cluster();
        let stale = acquire_fence(&store, &cluster, "instance-a").await;

        // 장애 전환. 반납 후 재획득으로 epoch가 단조 증가한다.
        store
            .release_control_lease(&cluster, "instance-a", stale.epoch)
            .await
            .unwrap();
        let fresh = acquire_fence(&store, &cluster, "instance-b").await;
        assert!(
            fresh.epoch > stale.epoch,
            "재획득은 epoch를 올려야 한다: {} -> {}",
            stale.epoch,
            fresh.epoch
        );

        // 옛 인스턴스의 쓰기다. **위상 조건은 완벽히 맞는다** — 이 작업은
        // 실제로 `Dispatched`이고, 늦은 완료가 아니다. 위상 CAS만으로는
        // 통과했을 쓰기이며, 거절의 근거는 epoch뿐이다.
        let worker = WorkerId::new();
        let task = seed_task(&store, "stale epoch", dispatched(worker)).await;
        let outcome = store
            .compare_and_set_task_status(
                task.id,
                &[TaskPhase::Dispatched],
                &completed(worker),
                Some(&stale),
            )
            .await
            .unwrap();

        assert_eq!(outcome, TransitionOutcome::Fenced);
        let stored = store.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(
            stored.status.phase(),
            TaskPhase::Dispatched,
            "거절된 전이는 아무것도 쓰지 않아야 한다"
        );

        // 같은 쓰기를 현재 epoch로 다시 하면 통과한다 — 거절된 원인이 epoch
        // 하나였음을 못 박는다.
        let retried = store
            .compare_and_set_task_status(
                task.id,
                &[TaskPhase::Dispatched],
                &completed(worker),
                Some(&fresh),
            )
            .await
            .unwrap();
        assert_eq!(retried, TransitionOutcome::Applied);
    }
);

both_backends!(
    cas_is_fenced_when_the_cluster_has_no_lease_at_all,
    |store| async move {
        // lease 행이 아예 없는 것을 "막을 근거가 없으니 통과"로 읽으면,
        // 테이블이 비워진 순간 fence 전체가 조용히 무력해진다. 없음은 곧
        // "이 epoch를 쥔 적이 없다"이므로 거절이어야 한다.
        let fence = ControlFence {
            cluster_id: unique_cluster(),
            epoch: 1,
        };
        let task = seed_task(&store, "no lease row", TaskStatus::Pending).await;

        let outcome = store
            .compare_and_set_task_status(task.id, &[TaskPhase::Pending], &cancelled(), Some(&fence))
            .await
            .unwrap();

        assert_eq!(outcome, TransitionOutcome::Fenced);
    }
);

both_backends!(fenced_takes_precedence_over_not_found, |store| async move {
    // fenced 인스턴스가 읽은 Task 상태에는 권위가 없다. 여기서
    // `Err(NotFound)`를 돌려주면 호출자는 "나는 제어 기관이었고 이 Task만
    // 없었다"는 결론으로 간다 — 두 백엔드가 같은 우선순위를 갖는지 못
    // 박아 둔다.
    let ghost = Task::from_request(TaskRequest {
        prompt: "never inserted".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    let fence = ControlFence {
        cluster_id: unique_cluster(),
        epoch: 99,
    };

    let outcome = store
        .compare_and_set_task_status(ghost.id, &[TaskPhase::Pending], &cancelled(), Some(&fence))
        .await
        .expect("fenced는 Err이 아니라 관측 결과다");

    assert_eq!(outcome, TransitionOutcome::Fenced);
});
