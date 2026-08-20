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

/// DB 연결 가능 여부 확인. `DATABASE_URL`이 아예 설정되지 않은 경우에만
/// `None`(테스트 skip)을 반환한다.
///
/// **중요**: `DATABASE_URL`이 설정되어 있는데 연결이나 마이그레이션이
/// 실패하면 여기서 `panic!`한다 — 절대 `None`을 반환해 조용히 skip시키지
/// 않는다. 예전에는 두 경우(URL 미설정 vs 연결/마이그레이션 실패)를 구분하지
/// 않고 둘 다 `None`으로 처리했는데, 그 결과 마이그레이션이 깨져도 이 파일의
/// 모든 테스트가 실행 없이 `... ok`로 "통과" 표시되는 구조적 위험이 있었다
/// (실제로 `migrations/004_rbac.sql`의 부분 인덱스 술어 버그로 이 문제가
/// 발생한 적이 있다 — auth_integration.rs 참고).
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
        required_labels: vec!["linux".into()],
        max_turns: Some(10),
        timeout_secs: Some(600),
        created_by: created_by.into(),
        ..Default::default()
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
    assert_eq!(fetched.retry_count, 0, "retry_count must default to 0");
}

/// 로드맵 #38 — dispatch 재시도 카운터가 실제 Postgres에서 원자적으로
/// 증가하고, `get_task`로 다시 읽었을 때도 반영되는지 확인.
#[tokio::test]
async fn task_increment_retry_count_persists_and_accumulates() {
    require_db!(store);

    let task = sample_task("Flaky dispatch", "carol");
    let task_id = task.id;
    store.insert_task(&task).await.unwrap();

    let first = store.increment_task_retry_count(task_id).await.unwrap();
    assert_eq!(first, 1);
    let second = store.increment_task_retry_count(task_id).await.unwrap();
    assert_eq!(second, 2);

    let fetched = store.get_task(task_id).await.unwrap().unwrap();
    assert_eq!(fetched.retry_count, 2, "increment must persist across reads");
}

#[tokio::test]
async fn task_increment_retry_count_nonexistent_returns_not_found() {
    require_db!(store);

    let bogus = TaskId::new();
    let err = store.increment_task_retry_count(bogus).await.unwrap_err();
    assert!(matches!(err, StoreError::NotFound));
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

#[tokio::test]
async fn task_list_respects_limit_and_offset() {
    // 로드맵 #23 — 대시보드 프론트엔드의 "Load more" 페이지네이션은
    // `list_tasks`가 `limit`을 정확히 지키고 `created_at DESC`(최신순) +
    // `offset`으로 안정적인 페이지 경계를 준다는 것에 의존한다. `created_at`을
    // 명시적으로 벌려서 실행 타이밍에 좌우되지 않는 결정적 순서를 만든다.
    require_db!(store);

    let mut tasks = Vec::new();
    for i in 0..5 {
        let mut t = sample_task(&format!("Task {i}"), "alice");
        t.created_at = chrono::Utc::now() - chrono::Duration::seconds(5 - i);
        tasks.push(t);
    }
    for t in &tasks {
        store.insert_task(t).await.unwrap();
    }

    // limit=2 — 정확히 2개만, 최신순이므로 가장 나중에 생성된 "Task 4"부터.
    let page1 = store
        .list_tasks(&TaskFilter {
            limit: 2,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page1.len(), 2, "limit must cap the result count");
    assert_eq!(page1[0].prompt, "Task 4");
    assert_eq!(page1[1].prompt, "Task 3");

    // limit=2, offset=2 — 다음 페이지, 겹치지 않아야 함.
    let page2 = store
        .list_tasks(&TaskFilter {
            limit: 2,
            offset: 2,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page2.len(), 2);
    assert_eq!(page2[0].prompt, "Task 2");
    assert_eq!(page2[1].prompt, "Task 1");

    // 프론트엔드의 "더 있음" 판단 방식(limit+1 요청)이 실제로 동작하는지 —
    // 5개 중 limit=5로 요청하면 전부, limit=4면 "더 있음"을 알 수 있게 4개만.
    let almost_all = store
        .list_tasks(&TaskFilter {
            limit: 4,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(almost_all.len(), 4, "limit < total must not include the last row");
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

/// 로드맵 #61 1단계 회귀 테스트 — `Worker::new`(및 `sample_worker`)가 만드는
/// 워커는 `liveness_mode`를 명시하지 않아도 `periodic`으로 저장/조회되어야
/// 한다 (기존 배포와의 하위 호환).
#[tokio::test]
async fn worker_upsert_defaults_liveness_mode_to_periodic() {
    require_db!(store);

    let worker = sample_worker("legacy-worker-1");
    store.upsert_worker(&worker).await.unwrap();

    let fetched = store
        .get_worker(worker.id)
        .await
        .unwrap()
        .expect("worker should exist");
    assert_eq!(
        fetched.liveness_mode,
        fleet_core::WorkerLivenessMode::Periodic
    );
}

/// 로드맵 #61 1단계 — 신규 워커를 `on_demand`로 등록하고 그대로 저장/조회할
/// 수 있어야 한다.
#[tokio::test]
async fn worker_upsert_persists_on_demand_liveness_mode() {
    require_db!(store);

    let mut worker = sample_worker("on-demand-worker-1");
    worker.liveness_mode = fleet_core::WorkerLivenessMode::OnDemand;
    store.upsert_worker(&worker).await.unwrap();

    let fetched = store
        .get_worker(worker.id)
        .await
        .unwrap()
        .expect("worker should exist");
    assert_eq!(
        fetched.liveness_mode,
        fleet_core::WorkerLivenessMode::OnDemand
    );

    // list_workers 경로도 동일하게 반영되는지 확인.
    let listed = store
        .list_workers(&WorkerFilter::default())
        .await
        .unwrap();
    let found = listed
        .iter()
        .find(|w| w.id == worker.id)
        .expect("worker should be listed");
    assert_eq!(found.liveness_mode, fleet_core::WorkerLivenessMode::OnDemand);
}

/// 로드맵 #61 1단계 마이그레이션 회귀 테스트 — 마이그레이션 019 적용 전에
/// 존재했을 법한 행을 흉내내어, `liveness_mode` 컬럼을 명시하지 않고 직접
/// INSERT해도 `DEFAULT 'periodic'`이 적용되는지 확인한다 (컬럼 자체가 없던
/// 시절의 행이 아니라, 마이그레이션 직후 기존 행이 채워지는 경로를 검증).
#[tokio::test]
async fn pre_existing_worker_row_backfilled_to_periodic_by_column_default() {
    require_db!(store);

    let worker_id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO workers
             (id, name, endpoint, labels, status, circuit_state,
              active_tasks, max_concurrent, registered_at)
           VALUES ($1, $2, $3, '{}'::jsonb, 'online', 'closed', 0, 4, NOW())"#,
    )
    .bind(worker_id)
    .bind("raw-insert-worker")
    .bind("wss://raw-insert/ws")
    .execute(store.pool())
    .await
    .unwrap();

    let fetched = store
        .get_worker(fleet_core::WorkerId::from(worker_id))
        .await
        .unwrap()
        .expect("worker should exist");
    assert_eq!(
        fetched.liveness_mode,
        fleet_core::WorkerLivenessMode::Periodic,
        "rows inserted without an explicit liveness_mode must default to periodic"
    );
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

/// `limit`/`offset` 페이지네이션이 겹침·누락 없이 동작해야 한다.
#[tokio::test]
async fn worker_list_pagination_with_offset() {
    require_db!(store);

    // registered_at DESC 정렬이므로 등록 순서의 역순으로 반환된다.
    for i in 0..5 {
        store
            .upsert_worker(&sample_worker(&format!("page-worker-{i}")))
            .await
            .unwrap();
    }

    let page = |limit: usize, offset: usize| WorkerFilter {
        limit,
        offset,
        ..Default::default()
    };

    let all = store.list_workers(&page(100, 0)).await.unwrap();
    assert_eq!(all.len(), 5);

    let first = store.list_workers(&page(2, 0)).await.unwrap();
    let second = store.list_workers(&page(2, 2)).await.unwrap();
    let third = store.list_workers(&page(2, 4)).await.unwrap();

    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    assert_eq!(third.len(), 1, "마지막 페이지는 남은 1건만");

    // 페이지 간 중복이 없어야 하고, 합치면 전체와 같아야 한다.
    let paged: Vec<String> = first
        .iter()
        .chain(second.iter())
        .chain(third.iter())
        .map(|w| w.name.clone())
        .collect();
    let expected: Vec<String> = all.iter().map(|w| w.name.clone()).collect();
    assert_eq!(
        paged, expected,
        "페이지를 이어붙이면 전체 목록과 같아야 한다"
    );
}

/// 라벨 필터가 LIMIT보다 **먼저** 적용되어야 한다.
///
/// 회귀: 이전 구현은 LIMIT으로 자른 뒤 Rust에서 라벨을 걸러냈다. 그래서
/// 조건에 맞는 워커가 충분히 있어도 limit보다 훨씬 적게 반환됐고,
/// 라벨 필터와 페이지네이션을 함께 쓸 수 없었다.
#[tokio::test]
async fn worker_list_label_filter_applied_before_limit() {
    require_db!(store);

    // 라벨 없는 워커를 먼저 등록 (DESC 정렬에서 뒤로 밀리도록).
    for i in 0..5 {
        store
            .upsert_worker(&sample_worker(&format!("plain-{i}")))
            .await
            .unwrap();
    }
    // 라벨이 붙은 워커 3대.
    for i in 0..3 {
        let mut w = sample_worker(&format!("tagged-{i}"));
        w.labels.insert("role".into(), "builder".into());
        store.upsert_worker(&w).await.unwrap();
    }

    let mut labels = HashMap::new();
    labels.insert("role".into(), "builder".into());
    let filter = WorkerFilter {
        labels,
        limit: 2,
        ..Default::default()
    };

    let workers = store.list_workers(&filter).await.unwrap();
    assert_eq!(
        workers.len(),
        2,
        "라벨 조건을 만족하는 워커가 3대이므로 limit=2면 정확히 2건이어야 한다"
    );
    assert!(workers.iter().all(|w| w.name.starts_with("tagged-")));
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
        cpu_usage: Some(10.0),
        ram_usage: Some(30.0),
        agent_healthy: true,
        grok_version: None,
        fleet_worker_version: None,
        os_info: None,
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
