//! End-to-end dispatch 흐름 테스트.
//!
//! 이 테스트는 데이터베이스 없이 MockStore + MockTransport + Dispatcher를
//! 조립하여 단일 워커 dispatch → poll → result 전체 플로우를 검증합니다.
//!
//! 검증 항목:
//! 1. `Dispatcher::submit`이 task_id를 반환하고 store에 저장
//! 2. 백그라운드 이벤트 루프가 WorkerEvent::Completed를 수신하여
//!    task status를 Dispatched → Completed로 전이
//! 3. server_hint가 지정된 경우 해당 워커로 dispatch
//! 4. 워커가 강제로 실패한 경우 task status가 Failed로 전이
//! 5. CircuitBreaker에 결과가 기록되어 실패 후 state가 Open으로 전이 가능
//!
//! ## 실행
//!
//! ```bash
//! cargo test -p fleet-scheduler --test dispatch_e2e
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use fleet_core::{
    CircuitBreakerConfig, EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput,
    TaskRequest, TaskStatus, Worker, WorkerFilter, WorkerHeartbeat, WorkerId, WorkerStatus,
};
use fleet_scheduler::{Dispatcher, FleetState};
use fleet_store::{Store, StoreError};
use fleet_transport::{MockTransport, MockWorker};

// ───────────────────────────────────────────────────────────────────────
//  In-memory MockStore (full impl for e2e tests)
// ───────────────────────────────────────────────────────────────────────

/// 통합 테스트용 인메모리 Store. 모든 Store trait 메서드를 단순 HashMap으로 구현.
struct InMemoryStore {
    tasks: Mutex<HashMap<TaskId, Task>>,
    workers: Mutex<HashMap<WorkerId, Worker>>,
    events: Mutex<Vec<EventEntry>>,
    outputs: Mutex<HashMap<TaskId, Vec<(u64, String)>>>,
    next_event_seq: Mutex<u64>,
    next_output_seq: Mutex<HashMap<TaskId, u64>>,
}

impl InMemoryStore {
    fn new() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
            workers: Mutex::new(HashMap::new()),
            events: Mutex::new(Vec::new()),
            outputs: Mutex::new(HashMap::new()),
            next_event_seq: Mutex::new(0),
            next_output_seq: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl Store for InMemoryStore {
    async fn insert_task(&self, task: &Task) -> Result<(), StoreError> {
        let mut tasks = self.tasks.lock().await;
        if tasks.contains_key(&task.id) {
            return Err(StoreError::Conflict(format!("task {} already exists", task.id)));
        }
        tasks.insert(task.id, task.clone());
        Ok(())
    }

    async fn get_task(&self, id: TaskId) -> Result<Option<Task>, StoreError> {
        Ok(self.tasks.lock().await.get(&id).cloned())
    }

    async fn update_task_status(&self, id: TaskId, status: &TaskStatus) -> Result<(), StoreError> {
        let mut tasks = self.tasks.lock().await;
        let Some(task) = tasks.get_mut(&id) else {
            return Err(StoreError::NotFound);
        };
        task.status = status.clone();
        Ok(())
    }

    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        let tasks = self.tasks.lock().await;
        let mut out: Vec<Task> = tasks.values().cloned().collect();
        // limit 만 적용 (간단화)
        out.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        out.truncate(filter.limit);
        Ok(out)
    }

    async fn upsert_worker(&self, worker: &Worker) -> Result<(), StoreError> {
        self.workers.lock().await.insert(worker.id, worker.clone());
        Ok(())
    }

    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
        Ok(self.workers.lock().await.get(&id).cloned())
    }

    async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError> {
        Ok(self
            .workers
            .lock()
            .await
            .values()
            .find(|w| w.name == name)
            .cloned())
    }

    async fn list_workers(&self, filter: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
        let workers = self.workers.lock().await;
        let mut out: Vec<Worker> = workers
            .values()
            .filter(|w| filter.status.map_or(true, |s| w.status == s))
            .filter(|w| {
                filter
                    .labels
                    .iter()
                    .all(|(k, v)| w.labels.get(k) == Some(v))
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out.truncate(filter.limit);
        Ok(out)
    }

    async fn delete_worker(&self, id: WorkerId) -> Result<(), StoreError> {
        self.workers.lock().await.remove(&id);
        Ok(())
    }

    async fn update_worker_heartbeat(
        &self,
        id: WorkerId,
        hb: &WorkerHeartbeat,
    ) -> Result<(), StoreError> {
        let mut workers = self.workers.lock().await;
        if let Some(w) = workers.get_mut(&id) {
            w.active_tasks = hb.active_tasks;
            w.last_seen = Some(chrono::Utc::now());
        }
        Ok(())
    }

    async fn append_event(&self, event: &FleetEvent) -> Result<u64, StoreError> {
        let mut seq = self.next_event_seq.lock().await;
        *seq += 1;
        let entry = EventEntry {
            seq: *seq,
            event: event.clone(),
        };
        self.events.lock().await.push(entry);
        Ok(*seq)
    }

    async fn list_events(&self, after_seq: u64, limit: u32) -> Result<Vec<EventEntry>, StoreError> {
        Ok(self
            .events
            .lock()
            .await
            .iter()
            .filter(|e| e.seq > after_seq)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn append_output(&self, task_id: TaskId, chunk: &str) -> Result<u64, StoreError> {
        let mut seqs = self.next_output_seq.lock().await;
        let next = *seqs.entry(task_id).or_insert(0) + 1;
        let seq = next;
        *seqs.get_mut(&task_id).unwrap() = seq;
        self.outputs
            .lock()
            .await
            .entry(task_id)
            .or_default()
            .push((seq, chunk.to_string()));
        Ok(seq)
    }

    async fn get_output(&self, task_id: TaskId, after_seq: u64) -> Result<TaskOutput, StoreError> {
        let outputs = self.outputs.lock().await;
        let chunks: Vec<_> = outputs
            .get(&task_id)
            .map(|v| {
                v.iter()
                    .filter(|(seq, _)| *seq > after_seq)
                    .map(|(seq, chunk)| fleet_core::TaskOutputChunk {
                        task_id,
                        seq: *seq,
                        chunk: chunk.clone(),
                        written_at: chrono::Utc::now(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let next_offset = chunks.last().map(|c| c.seq + 1).unwrap_or(after_seq + 1);
        Ok(TaskOutput {
            task_id,
            chunks,
            next_offset,
        })
    }

    async fn migrate(&self) -> Result<(), StoreError> {
        Ok(())
    }
}

// ───────────────────────────────────────────────────────────────────────
//  Test fixtures
// ───────────────────────────────────────────────────────────────────────

/// 테스트용 워커 생성. 기본적으로 online + circuit closed.
fn make_worker(name: &str) -> Worker {
    let mut w = Worker::new(name, format!("wss://{name}/ws"));
    w.status = WorkerStatus::Online;
    w
}

/// FleetState + Dispatcher + 이벤트 루프를 함께 조립.
async fn setup(
    workers: Vec<Worker>,
    mock_workers: Vec<MockWorker>,
) -> (Arc<FleetState>, Arc<Dispatcher>) {
    let store = Arc::new(InMemoryStore::new()) as Arc<dyn Store>;
    for w in workers {
        store.upsert_worker(&w).await.unwrap();
    }

    let (transport, event_rx) = MockTransport::new();
    for mw in mock_workers {
        transport.add_worker(mw).await;
    }
    let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(transport);

    let state = Arc::new(FleetState::new(
        store,
        transport,
        // 쉽게 trip하도록 민감하게 설정 (테스트용)
        CircuitBreakerConfig {
            enabled: true,
            min_samples: 2,
            error_rate_threshold: 0.5,
            ..Default::default()
        },
    ));

    let dispatcher = Arc::new(Dispatcher::new(state.clone()));
    dispatcher.attach_event_receiver(event_rx).await;

    // 백그라운드에서 이벤트 루프 실행
    let dispatcher_bg = dispatcher.clone();
    tokio::spawn(async move {
        dispatcher_bg.run_event_loop().await;
    });

    (state, dispatcher)
}

/// 작업이 종료 상태(Completed/Failed/Cancelled)가 될 때까지 폴링.
/// 타임아웃 2초.
async fn wait_until_terminal(
    state: &FleetState,
    task_id: TaskId,
) -> Task {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Ok(Some(task)) = state.store.get_task(task_id).await {
            if task.is_terminal() {
                return task;
            }
        }
        if std::time::Instant::now() > deadline {
            panic!("task {task_id} did not reach terminal state within 2s");
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

// ───────────────────────────────────────────────────────────────────────
//  Tests
// ───────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_completes_successfully() {
    let worker = make_worker("w1");
    let worker_id = worker.id;
    let (state, dispatcher) = setup(
        vec![worker],
        vec![MockWorker::new(worker_id, "wss://w1/ws")],
    )
    .await;

    let task = Task::from_request(TaskRequest {
        prompt: "echo hello".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    let task_id = task.id;

    let returned_id = dispatcher.submit(task).await.expect("submit failed");
    assert_eq!(returned_id, task_id);

    let completed = wait_until_terminal(&state, task_id).await;
    match completed.status {
        TaskStatus::Completed(result) => {
            assert_eq!(result.worker_id, worker_id);
            assert_eq!(result.exit_code, 0);
            assert!(result.output.contains("echo hello"), "output: {}", result.output);
        }
        other => panic!("expected Completed, got {:?}", other),
    }
}

#[tokio::test]
async fn dispatch_records_completed_event() {
    let worker = make_worker("w1");
    let worker_id = worker.id;
    let (state, dispatcher) = setup(
        vec![worker],
        vec![MockWorker::new(worker_id, "wss://w1/ws")],
    )
    .await;

    let task = Task::from_request(TaskRequest {
        prompt: "work".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    let task_id = task.id;
    dispatcher.submit(task).await.unwrap();
    wait_until_terminal(&state, task_id).await;

    // 이벤트 로그 검사
    let events = state.store.list_events(0, 100).await.unwrap();
    let types: Vec<&str> = events
        .iter()
        .map(|e| e.event.event_type())
        .collect();
    assert!(types.contains(&"task_created"), "events: {types:?}");
    assert!(types.contains(&"task_dispatched"), "events: {types:?}");
    assert!(types.contains(&"task_completed"), "events: {types:?}");
}

#[tokio::test]
async fn dispatch_with_server_hint_picks_hinted_worker() {
    let w1 = make_worker("idle-a");
    let w2 = make_worker("gpu-1");
    let w1_id = w1.id;
    let w2_id = w2.id;

    let (state, dispatcher) = setup(
        vec![w1, w2],
        vec![
            MockWorker::new(w1_id, "wss://idle-a/ws"),
            MockWorker::new(w2_id, "wss://gpu-1/ws"),
        ],
    )
    .await;

    let task = Task::from_request(TaskRequest {
        prompt: "gpu work".into(),
        server_hint: Some("gpu-1".into()),
        created_by: "test".into(),
        ..Default::default()
    });
    let task_id = task.id;
    dispatcher.submit(task).await.unwrap();

    let completed = wait_until_terminal(&state, task_id).await;
    match completed.status {
        TaskStatus::Completed(result) => {
            assert_eq!(result.worker_id, w2_id, "should have gone to gpu-1");
        }
        other => panic!("expected Completed, got {:?}", other),
    }
}

#[tokio::test]
async fn dispatch_with_unavailable_hint_fails() {
    // 힌트 워커가 offline — select가 에러를 반환해야 함
    let mut hinted = make_worker("offline-1");
    hinted.status = WorkerStatus::Offline;
    let online = make_worker("online-1");

    let (state, dispatcher) = setup(
        vec![hinted, online],
        vec![], // 어차피 dispatch 전에 실패하므로 mock worker 불필요
    )
    .await;

    let task = Task::from_request(TaskRequest {
        prompt: "work".into(),
        server_hint: Some("offline-1".into()),
        created_by: "test".into(),
        ..Default::default()
    });
    let task_id = task.id;

    let result = dispatcher.submit(task).await;
    assert!(result.is_err(), "should fail since hinted worker is offline");

    // Store에는 Failed로 기록되어야 함
    let stored = state.store.get_task(task_id).await.unwrap().unwrap();
    assert!(matches!(stored.status, TaskStatus::Failed(_)));
}

#[tokio::test]
async fn dispatch_failure_marks_task_failed_and_records_breaker() {
    let worker = make_worker("flaky-1");
    let worker_id = worker.id;

    // 강제 실패하는 mock worker
    let mut mock = MockWorker::new(worker_id, "wss://flaky-1/ws");
    mock.force_fail = true;

    let (state, dispatcher) = setup(vec![worker], vec![mock]).await;

    let task = Task::from_request(TaskRequest {
        prompt: "doomed".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    let task_id = task.id;

    dispatcher.submit(task).await.unwrap();
    let terminal = wait_until_terminal(&state, task_id).await;

    let failure = match terminal.status {
        TaskStatus::Failed(f) => f,
        other => panic!("expected Failed, got {:?}", other),
    };
    assert_eq!(failure.worker_id, Some(worker_id));
    assert!(failure.error.contains("forced failure"));

    // CircuitBreaker에 실패가 기록되었는지 확인 — 즉시 trip은 아니지만 (min_samples=2)
    // 첫 실패 후에도 state는 Closed여야 함 (샘플 부족)
    let breaker_state = state.breakers.state_of(worker_id);
    assert!(
        !breaker_state.is_open(),
        "first failure should not trip breaker yet (min_samples=2)"
    );
}

#[tokio::test]
async fn multiple_failures_trip_circuit_breaker() {
    let worker = make_worker("breaker-test");
    let worker_id = worker.id;

    let mut mock = MockWorker::new(worker_id, "wss://breaker-test/ws");
    mock.force_fail = true;

    let (state, dispatcher) = setup(vec![worker], vec![mock]).await;

    // 두 번의 실패 (min_samples=2, error_rate_threshold=0.5 → 100% 실패 → trip)
    for i in 0..2 {
        let task = Task::from_request(TaskRequest {
            prompt: format!("fail-{i}"),
            created_by: "test".into(),
            ..Default::default()
        });
        let task_id = task.id;
        dispatcher.submit(task).await.unwrap();
        wait_until_terminal(&state, task_id).await;
    }

    let breaker_state = state.breakers.state_of(worker_id);
    assert!(
        breaker_state.is_open(),
        "breaker should be open after 2 failures (min_samples=2)"
    );

    // 세 번째 작업은 CircuitOpen으로 즉시 실패해야 함
    let task = Task::from_request(TaskRequest {
        prompt: "blocked".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    let result = dispatcher.submit(task).await;
    assert!(result.is_err(), "should refuse dispatch when circuit is open");
}

#[tokio::test]
async fn label_filtering_selects_only_matching_worker() {
    // 라벨 없는 워커 + gpu 라벨 워커
    let mut cpu = make_worker("cpu-1");
    cpu.active_tasks = 5; // busy여도 라벨 매칭 안 되면 선택 안 됨
    let mut gpu = make_worker("gpu-1");
    gpu.labels.insert("gpu".into(), "true".into());
    let cpu_id = cpu.id;
    let gpu_id = gpu.id;

    let (state, dispatcher) = setup(
        vec![cpu, gpu],
        vec![
            MockWorker::new(cpu_id, "wss://cpu-1/ws"),
            MockWorker::new(gpu_id, "wss://gpu-1/ws"),
        ],
    )
    .await;

    let task = Task::from_request(TaskRequest {
        prompt: "train model".into(),
        required_labels: vec!["gpu".into()],
        created_by: "test".into(),
        ..Default::default()
    });
    let task_id = task.id;

    dispatcher.submit(task).await.unwrap();
    let completed = wait_until_terminal(&state, task_id).await;
    match completed.status {
        TaskStatus::Completed(result) => {
            assert_eq!(result.worker_id, gpu_id, "must pick gpu worker");
        }
        other => panic!("expected Completed, got {:?}", other),
    }
}
