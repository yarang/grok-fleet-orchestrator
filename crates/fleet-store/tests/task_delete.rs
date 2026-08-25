//! `delete_task`/`list_task_threads`의 백엔드 공유 행동 + PgStore 전용 cascade
//! 테스트 (로드맵 #96, 삭제 계약 `docs/architecture/tasks/management.md`).
//!
//! 기존 `dashboard_api.rs` 통합 테스트는 전부 `MemStore`만 거친다 — HTTP 계층의
//! 응답 코드/바디는 검증하지만 `PgStore::delete_task`/`PgStore::list_task_threads`의
//! 실제 SQL(0행 판정, `dependency_ids` 부분 GIN 조건, cascade)은 어떤 스위트에서도
//! 실행된 적이 없었다. 이 파일이 그 간극을 닫는다.
//!
//! `DATABASE_URL`이 없으면 PgStore 케이스만 skip되고 MemStore 케이스는 항상
//! 실행된다 (PG 전용 cascade 테스트는 예외 — 아래 참고).
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --all-features --test task_delete -- --test-threads=1
//! ```

use std::sync::Arc;

use chrono::Utc;
use fleet_core::{Task, TaskDeleteOutcome, TaskPhase, TaskRequest, TaskStatus};
use fleet_store::mem::MemStore;
use fleet_store::{PgStore, Store, StoreError};
use sqlx::postgres::PgPoolOptions;
use sqlx::Row;
use uuid::Uuid;

// ── 백엔드 준비 (task_cas.rs와 동일한 패턴) ────────────────────────────

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
    sqlx::query("TRUNCATE issue_task_links, issue_comments, issues, tasks, projects CASCADE")
        .execute(store.pool())
        .await
        .expect("truncate");
    Some(Arc::new(store))
}

/// PG 전용 cascade 검증은 raw pool 접근이 필요해 `Arc<dyn Store>`로는 못 판다
/// — `PgStore`를 그대로 돌려준다.
async fn pg_backend_concrete() -> Option<PgStore> {
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
    Some(store)
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

fn cancelled() -> TaskStatus {
    TaskStatus::Cancelled {
        reason: "test setup".into(),
        cancelled_at: Utc::now(),
    }
}

// ── 게이트 1: 비terminal 삭제는 0행 DELETE 판정으로 거부된다 ───────────

both_backends!(
    delete_task_rejects_a_pending_task_and_survives,
    |store| async move {
        let task = seed_task(&store, "still pending", TaskStatus::Pending).await;

        let outcome = store.delete_task(task.id).await.unwrap();
        assert_eq!(
            outcome,
            TaskDeleteOutcome::NotTerminal {
                current: TaskPhase::Pending
            }
        );

        // 거부는 아무것도 지우지 않았다는 뜻이다 — 선검사가 아니라 DELETE 자체의
        // WHERE 절이 0행이었기 때문에 거부됐다면, 행은 그대로 남아 있어야 한다.
        let survives = store.get_task(task.id).await.unwrap();
        assert!(survives.is_some(), "거부된 삭제가 행을 지웠다");
    }
);

both_backends!(delete_task_removes_a_terminal_task, |store| async move {
    let task = seed_task(&store, "done already", cancelled()).await;

    let outcome = store.delete_task(task.id).await.unwrap();
    assert_eq!(outcome, TaskDeleteOutcome::Deleted);

    let gone = store.get_task(task.id).await.unwrap();
    assert!(gone.is_none(), "삭제된 Task가 여전히 조회된다");
});

both_backends!(delete_task_unknown_id_is_not_found, |store| async move {
    let ghost = Task::from_request(TaskRequest {
        prompt: "never inserted".into(),
        created_by: "test".into(),
        ..Default::default()
    });

    let err = store
        .delete_task(ghost.id)
        .await
        .expect_err("존재하지 않는 Task는 Err(NotFound)여야 한다");
    assert!(matches!(err, StoreError::NotFound), "got {err:?}");
});

// ── 게이트 2: Pending 의존자가 있으면 거부, terminal뿐이면 허용 ────────

both_backends!(
    delete_task_blocked_by_a_pending_dependent,
    |store| async move {
        let blocker = seed_task(&store, "everyone depends on this", cancelled()).await;
        let dependent = Task::from_request(TaskRequest {
            prompt: "waiting on blocker".into(),
            created_by: "test".into(),
            dependency_ids: vec![blocker.id],
            ..Default::default()
        });
        store.insert_task(&dependent).await.unwrap();

        let outcome = store.delete_task(blocker.id).await.unwrap();
        match outcome {
            TaskDeleteOutcome::BlockedByDependents { dependent_ids } => {
                assert_eq!(dependent_ids, vec![dependent.id]);
            }
            other => panic!("expected BlockedByDependents, got {other:?}"),
        }
        assert!(store.get_task(blocker.id).await.unwrap().is_some());
    }
);

both_backends!(
    delete_task_allowed_once_the_dependent_is_terminal,
    |store| async move {
        // 문서가 명시하는 비대칭: terminal 의존자는 검사 대상이 아니다 — 이미
        // 실행이 끝나 ready 판정을 다시 지나지 않기 때문. Pending일 때만
        // 막고, 그 의존자가 terminal이 되면 삭제가 통과해야 한다.
        let blocker = seed_task(&store, "everyone depended on this", cancelled()).await;
        let mut dependent = Task::from_request(TaskRequest {
            prompt: "used to wait on blocker".into(),
            created_by: "test".into(),
            dependency_ids: vec![blocker.id],
            ..Default::default()
        });
        dependent.status = cancelled();
        store.insert_task(&dependent).await.unwrap();

        let outcome = store.delete_task(blocker.id).await.unwrap();
        assert_eq!(
            outcome,
            TaskDeleteOutcome::Deleted,
            "terminal 의존자가 삭제를 막으면 안 된다"
        );
    }
);

// ── 게이트 3: 루트 삭제 후 parent_task_id는 NULL, thread_id는 유지 ─────

both_backends!(
    delete_task_root_clears_parent_link_but_keeps_thread_id,
    |store| async move {
        let root = seed_task(&store, "will be deleted", cancelled()).await;
        let mut reply = Task::from_request(TaskRequest {
            prompt: "outlives its root".into(),
            created_by: "test".into(),
            ..Default::default()
        });
        reply.inherit_from_parent(&root);
        store.insert_task(&reply).await.unwrap();
        assert_eq!(reply.thread_id, root.id);

        let outcome = store.delete_task(root.id).await.unwrap();
        assert_eq!(outcome, TaskDeleteOutcome::Deleted);

        let child = store.get_task(reply.id).await.unwrap().unwrap();
        assert_eq!(
            child.parent_task_id, None,
            "루트가 지워졌으면 parent_task_id는 NULL이어야 한다"
        );
        assert_eq!(
            child.thread_id, root.id,
            "thread_id는 루트 삭제와 무관하게 유지돼야 한다"
        );

        // 목록 UI가 "루트가 사라진 스레드"를 여전히 표시 대상으로 삼는지 —
        // thread_id로 조회하면 남은 멤버가 그대로 나와야 한다.
        let members = store.list_thread_tasks(root.id).await.unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].id, reply.id);
    }
);

both_backends!(
    list_task_threads_orders_by_most_recent_member_activity,
    |store| async move {
        let older = seed_task(&store, "older thread root", cancelled()).await;
        let newer = seed_task(&store, "newer thread root", cancelled()).await;

        let threads = store.list_task_threads(10, 0).await.unwrap();
        let older_pos = threads.iter().position(|id| *id == older.id);
        let newer_pos = threads.iter().position(|id| *id == newer.id);
        assert!(older_pos.is_some() && newer_pos.is_some());
        assert!(
            newer_pos < older_pos,
            "최근에 생성된 스레드가 먼저 나와야 한다: {threads:?}"
        );
    }
);

// ── 게이트 4 (PgStore 전용): outputs/telemetry는 지워지고, events는 남되
//    task_id 컬럼만 NULL이 된다 (payload의 task_id는 보존) ──────────────

#[tokio::test]
async fn delete_task_cascades_outputs_and_telemetry_and_nulls_event_task_id_column() {
    let Some(store) = pg_backend_concrete().await else {
        eprintln!("DATABASE_URL not set — skipping PgStore-only cascade test");
        return;
    };

    let mut task = Task::from_request(TaskRequest {
        prompt: "will be deleted, has traces".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    task.status = cancelled();
    store.insert_task(&task).await.unwrap();

    // task_outputs (ON DELETE CASCADE 대상)
    store.append_output(task.id, "hello").await.unwrap();

    // task_telemetry — Store 트레이트에 이 테이블을 다루는 메서드가 없으므로
    // (016 마이그레이션 주석 참고) raw SQL로 직접 심는다.
    sqlx::query(
        "INSERT INTO task_telemetry (task_id, routing_profile, resolved_model) \
         VALUES ($1, 'default', 'grok-test')",
    )
    .bind(task.id.as_uuid())
    .execute(store.pool())
    .await
    .unwrap();

    // events — append_event를 거치면 payload에 task_id가 그대로 직렬화된다.
    let event = fleet_core::FleetEvent::TaskCreated {
        task_id: task.id,
        server_hint: None,
        created_by: "test".into(),
        at: Utc::now(),
    };
    store.append_event(&event).await.unwrap();

    let outcome = store.delete_task(task.id).await.unwrap();
    assert_eq!(outcome, TaskDeleteOutcome::Deleted);

    let outputs_left: i64 =
        sqlx::query("SELECT count(*) AS n FROM task_outputs WHERE task_id = $1")
            .bind(task.id.as_uuid())
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("n")
            .unwrap();
    assert_eq!(outputs_left, 0, "task_outputs가 CASCADE로 지워지지 않았다");

    let telemetry_left: i64 =
        sqlx::query("SELECT count(*) AS n FROM task_telemetry WHERE task_id = $1")
            .bind(task.id.as_uuid())
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("n")
            .unwrap();
    assert_eq!(
        telemetry_left, 0,
        "task_telemetry가 CASCADE로 지워지지 않았다"
    );

    // events 행 자체는 남아 있어야 한다 (SET NULL이지 CASCADE가 아니다).
    let event_row =
        sqlx::query("SELECT task_id, payload FROM events WHERE payload->>'task_id' = $1")
            .bind(task.id.to_string())
            .fetch_one(store.pool())
            .await
            .expect("events 행이 SET NULL이 아니라 통째로 사라졌다");

    let task_id_col: Option<Uuid> = event_row.try_get("task_id").unwrap();
    assert_eq!(
        task_id_col, None,
        "events.task_id 컬럼이 ON DELETE SET NULL로 NULL이 돼야 한다"
    );

    let payload: serde_json::Value = event_row.try_get("payload").unwrap();
    assert_eq!(
        payload.get("task_id").and_then(|v| v.as_str()),
        Some(task.id.to_string().as_str()),
        "payload의 task_id는 컬럼과 무관하게 원본을 보존해야 한다"
    );
}
