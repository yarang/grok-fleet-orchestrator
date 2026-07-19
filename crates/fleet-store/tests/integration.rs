//! fleet-store 통합 테스트.
//!
//! 실제 PostgreSQL 데이터베이스가 필요합니다. `DATABASE_URL` 환경변수가
//! 설정되지 않거나 연결할 수 없으면 모든 테스트가 자동으로 skip됩니다.
//!
//! ## 실행 방법
//!
//! ```bash
//! # 1. Postgres 시작 (Homebrew)
//! brew services start postgresql@16
//!
//! # 2. 테스트용 데이터베이스 생성
//! createdb fleet_test
//!
//! # 3. 환경변수 설정 후 테스트 실행 (직렬 필수 — TRUNCATE 경쟁 방지)
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test integration -- --test-threads=1
//! ```

use std::collections::HashMap;

use chrono::Utc;
use fleet_core::{
    FleetEvent, Task, TaskFilter, TaskId, TaskPriority, TaskRequest, TaskResult, TaskStatus,
    TaskStatusFilter, Worker, WorkerFilter, WorkerHeartbeat, WorkerId, WorkerStatus,
};
use fleet_store::{PgStore, Store, StoreError};
use sqlx::postgres::PgPoolOptions;

/// 테스트용 데이터베이스 URL. `DATABASE_URL` 환경변수가 설정된 경우에만 사용.
/// 설정되지 않으면 모든 테스트가 자동으로 skip됩니다.
fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

/// DB 연결 가능 여부 확인. `DATABASE_URL`이 없거나 연결 불가하면 None (테스트 skip).
async fn try_connect() -> Option<PgStore> {
    let url = database_url()?;
    match PgPoolOptions::new().max_connections(2).connect(&url).await {
        Ok(pool) => {
            let store = PgStore::from_pool(pool);
            // 마이그레이션 실행 (실패해도 None 반환)
            match store.migrate().await {
                Ok(()) => Some(store),
                Err(e) => {
                    eprintln!("⚠ migration failed, skipping tests: {e}");
                    None
                }
            }
        }
        Err(e) => {
            eprintln!("⚠ DATABASE_URL={url} connection failed: {e}");
            None
        }
    }
}

/// 테스트 헬퍼: 스토어 초기화 + 클린업. 연결 불가 시 early return.
macro_rules! require_db {
    ($store:ident) => {
        let $store = match try_connect().await {
            Some(s) => s,
            None => return,
        };
        // 각 테스트 전 테이블 비움
        let _ = sqlx::query("TRUNCATE task_outputs, events, tasks, workers CASCADE")
            .execute($store.pool())
            .await;
    };
}

fn sample_task(prompt: &str, created_by: &str) -> Task {
    let req = TaskRequest {
        prompt: prompt.into(),
        cwd: Some("/tmp/work".into()),
        model: Some("grok-4".into()),
        server_hint: None,
        required_labels: vec!["linux".into()],
        max_turns: Some(10),
        timeout_secs: Some(600),
        priority: TaskPriority::Normal,
        created_by: created_by.into(),
    };
    Task::from_request(req)
}

fn sample_worker(name: &str) -> Worker {
    let mut w = Worker::new(name, format!("wss://{name}.fleet.example.com/ws"));
    w.labels.insert("arch".into(), "x86_64".into());
    w
}

// ═══════════════════════════════════════════════════════════════════════
//  Task CRUD
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn task_insert_and_get() {
    require_db!(store);

    let task = sample_task("Build the project", "alice");
    let task_id = task.id;

    store.insert_task(&task).await.unwrap();

    let fetched = store
        .get_task(task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(fetched.id, task_id);
    assert_eq!(fetched.prompt, "Build the project");
    assert_eq!(fetched.created_by, "alice");
    assert!(matches!(fetched.status, TaskStatus::Pending));
    assert_eq!(fetched.required_labels, vec!["linux".to_string()]);
}

#[tokio::test]
async fn task_get_nonexistent_returns_none() {
    require_db!(store);

    let result = store.get_task(TaskId::new()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn task_update_status() {
    require_db!(store);

    let task = sample_task("Run tests", "bob");
    let task_id = task.id;
    store.insert_task(&task).await.unwrap();

    // Dispatched로 전이
    let worker_id = WorkerId::new();
    let dispatched = TaskStatus::Dispatched {
        worker_id,
        started_at: Utc::now(),
    };
    store
        .update_task_status(task_id, &dispatched)
        .await
        .unwrap();

    let fetched = store.get_task(task_id).await.unwrap().unwrap();
    assert!(matches!(fetched.status, TaskStatus::Dispatched { .. }));

    // Completed로 전이
    let result = TaskResult {
        output: "All tests passed".into(),
        exit_code: 0,
        duration_secs: 5.3,
        token_usage: None,
        worker_id,
        finished_at: Utc::now(),
    };
    store
        .update_task_status(task_id, &TaskStatus::Completed(result))
        .await
        .unwrap();

    let fetched = store.get_task(task_id).await.unwrap().unwrap();
    assert!(matches!(fetched.status, TaskStatus::Completed(_)));
}

#[tokio::test]
async fn task_list_with_filters() {
    require_db!(store);

    // 3개 작업 생성 (2개 alice, 1개 bob)
    let t1 = sample_task("Task 1", "alice");
    let t2 = sample_task("Task 2", "alice");
    let t3 = sample_task("Task 3", "bob");
    store.insert_task(&t1).await.unwrap();
    store.insert_task(&t2).await.unwrap();
    store.insert_task(&t3).await.unwrap();

    // alice 작업만 조회
    let filter = TaskFilter {
        created_by: Some("alice".into()),
        limit: 100,
        ..Default::default()
    };
    let tasks = store.list_tasks(&filter).await.unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|t| t.created_by == "alice"));

    // 상태 필터: Pending만
    let filter = TaskFilter {
        status: Some(TaskStatusFilter::Pending),
        limit: 100,
        ..Default::default()
    };
    let tasks = store.list_tasks(&filter).await.unwrap();
    assert_eq!(tasks.len(), 3); // 모두 Pending
}

// ═══════════════════════════════════════════════════════════════════════
//  Worker CRUD
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn worker_upsert_and_get() {
    require_db!(store);

    let worker = sample_worker("build-farm-1");
    let worker_id = worker.id;

    store.upsert_worker(&worker).await.unwrap();

    let fetched = store
        .get_worker(worker_id)
        .await
        .unwrap()
        .expect("worker should exist");
    assert_eq!(fetched.name, "build-farm-1");
    assert_eq!(fetched.endpoint, "wss://build-farm-1.fleet.example.com/ws");
    assert_eq!(fetched.labels.get("arch").unwrap(), "x86_64");
    assert!(matches!(fetched.status, WorkerStatus::Online));
}

#[tokio::test]
async fn worker_get_by_name() {
    require_db!(store);

    let worker = sample_worker("gpu-runner-1");
    store.upsert_worker(&worker).await.unwrap();

    let fetched = store
        .get_worker_by_name("gpu-runner-1")
        .await
        .unwrap()
        .expect("worker should exist");
    assert_eq!(fetched.id, worker.id);
}

#[tokio::test]
async fn worker_upsert_updates_existing() {
    require_db!(store);

    let mut worker = sample_worker("ci-runner-1");
    let worker_id = worker.id;
    store.upsert_worker(&worker).await.unwrap();

    // 상태 변경 후 다시 upsert
    worker.status = WorkerStatus::Degraded;
    worker.active_tasks = 3;
    store.upsert_worker(&worker).await.unwrap();

    let fetched = store.get_worker(worker_id).await.unwrap().unwrap();
    assert!(matches!(fetched.status, WorkerStatus::Degraded));
    assert_eq!(fetched.active_tasks, 3);
}

#[tokio::test]
async fn worker_list_with_status_filter() {
    require_db!(store);

    let w1 = sample_worker("online-1");
    let mut w2 = sample_worker("offline-1");
    w2.status = WorkerStatus::Offline;

    store.upsert_worker(&w1).await.unwrap();
    store.upsert_worker(&w2).await.unwrap();

    // Online만 조회
    let filter = WorkerFilter {
        status: Some(WorkerStatus::Online),
        ..Default::default()
    };
    let workers = store.list_workers(&filter).await.unwrap();
    assert!(workers.iter().any(|w| w.name == "online-1"));
    assert!(!workers.iter().any(|w| w.name == "offline-1"));
}

#[tokio::test]
async fn worker_list_with_label_filter() {
    require_db!(store);

    let mut w1 = sample_worker("gpu-1");
    w1.labels.insert("gpu".into(), "true".into());
    let w2 = sample_worker("cpu-1");

    store.upsert_worker(&w1).await.unwrap();
    store.upsert_worker(&w2).await.unwrap();

    let mut label_filter = HashMap::new();
    label_filter.insert("gpu".into(), "true".into());
    let filter = WorkerFilter {
        labels: label_filter,
        ..Default::default()
    };
    let workers = store.list_workers(&filter).await.unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].name, "gpu-1");
}

#[tokio::test]
async fn worker_heartbeat_updates_last_seen() {
    require_db!(store);

    let worker = sample_worker("hb-1");
    let worker_id = worker.id;
    store.upsert_worker(&worker).await.unwrap();

    let hb = WorkerHeartbeat {
        worker_id,
        active_tasks: 2,
        load_avg: vec![0.5, 0.7, 0.8],
        mem_available_mb: 8192,
        disk_free_mb: 50000,
        agent_healthy: true,
    };
    store.update_worker_heartbeat(worker_id, &hb).await.unwrap();

    let fetched = store.get_worker(worker_id).await.unwrap().unwrap();
    assert_eq!(fetched.active_tasks, 2);
    assert!(fetched.last_seen.is_some());
}

#[tokio::test]
async fn worker_delete() {
    require_db!(store);

    let worker = sample_worker("to-delete");
    let worker_id = worker.id;
    store.upsert_worker(&worker).await.unwrap();

    store.delete_worker(worker_id).await.unwrap();
    assert!(store.get_worker(worker_id).await.unwrap().is_none());
}

#[tokio::test]
async fn worker_delete_nonexistent_errors() {
    require_db!(store);

    let result = store.delete_worker(WorkerId::new()).await;
    assert!(matches!(result, Err(StoreError::NotFound)));
}

// ═══════════════════════════════════════════════════════════════════════
//  Event log
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn event_append_and_list() {
    require_db!(store);

    // FK 제약: events.task_id가 tasks.id를 참조하므로 task를 먼저 insert
    let task = sample_task("Event test", "alice");
    let task_id = task.id;
    store.insert_task(&task).await.unwrap();

    // FK 제약: events.worker_id가 workers.id를 참조하므로 worker도 insert
    let worker_id = WorkerId::new();
    let mut worker = sample_worker("evt-dispatch-worker");
    worker.id = worker_id;
    store.upsert_worker(&worker).await.unwrap();

    let seq1 = store
        .append_event(&FleetEvent::task_created(task_id, None, "alice"))
        .await
        .unwrap();

    let seq2 = store
        .append_event(&FleetEvent::task_dispatched(task_id, worker_id))
        .await
        .unwrap();

    assert!(seq2 > seq1, "sequence should be monotonically increasing");

    // seq1 이후 이벤트 조회
    let events = store.list_events(seq1, 100).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, seq2);
    assert_eq!(events[0].event.event_type(), "task_dispatched");
}

#[tokio::test]
async fn event_roundtrip_all_fields() {
    require_db!(store);

    // FK 제약: task를 먼저 insert
    let task = sample_task("Event roundtrip test", "alice");
    let task_id = task.id;
    let worker_id = WorkerId::new();
    store.insert_task(&task).await.unwrap();

    // 워커도 insert (worker_id FK)
    let mut worker = sample_worker("evt-worker");
    worker.id = worker_id;
    store.upsert_worker(&worker).await.unwrap();

    let event = FleetEvent::TaskCompleted {
        task_id,
        worker_id,
        result: TaskResult {
            output: "Build OK".into(),
            exit_code: 0,
            duration_secs: 42.5,
            token_usage: None,
            worker_id,
            finished_at: Utc::now(),
        },
        at: Utc::now(),
    };

    let seq = store.append_event(&event).await.unwrap();
    let events = store.list_events(0, 100).await.unwrap();

    let fetched = events
        .iter()
        .find(|e| e.seq == seq)
        .expect("event should be in list");

    assert_eq!(fetched.event.event_type(), "task_completed");
    assert_eq!(fetched.event.task_id(), Some(task_id));
    assert_eq!(fetched.event.worker_id(), Some(worker_id));
}

// ═══════════════════════════════════════════════════════════════════════
//  Output buffer
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn output_append_and_get() {
    require_db!(store);

    // 작업이 있어야 함 (FK 제약)
    let task = sample_task("Stream test", "carol");
    let task_id = task.id;
    store.insert_task(&task).await.unwrap();

    // 청크 3개 추가
    let seq1 = store
        .append_output(task_id, "Compiling...\n")
        .await
        .unwrap();
    let seq2 = store
        .append_output(task_id, "Running tests...\n")
        .await
        .unwrap();
    let seq3 = store.append_output(task_id, "Done\n").await.unwrap();

    assert!(seq1 < seq2);
    assert!(seq2 < seq3);

    // seq1 이후 조회
    let output = store.get_output(task_id, seq1).await.unwrap();
    assert_eq!(output.chunks.len(), 2); // seq2, seq3
    assert_eq!(output.chunks[0].chunk, "Running tests...\n");
    assert_eq!(output.chunks[1].chunk, "Done\n");
    assert_eq!(output.next_offset, seq3);
}

#[tokio::test]
async fn output_get_empty() {
    require_db!(store);

    let task = sample_task("No output", "dave");
    let task_id = task.id;
    store.insert_task(&task).await.unwrap();

    let output = store.get_output(task_id, 0).await.unwrap();
    assert!(output.chunks.is_empty());
    assert_eq!(output.next_offset, 0);
}
