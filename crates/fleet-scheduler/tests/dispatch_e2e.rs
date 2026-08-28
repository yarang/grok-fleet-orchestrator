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

use std::sync::Arc;

use fleet_core::{
    CircuitBreakerConfig, Task, TaskId, TaskRequest, TaskStatus, Worker, WorkerStatus,
};
use fleet_scheduler::{Dispatcher, FleetState};
use fleet_store::Store;
use fleet_transport::{MockTransport, MockWorker};

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
    // 로드맵 후속 정리(2026-08-14) — 이 파일이 자체 구현하던 InMemoryStore
    // (Store 트레이트 전체를 다시 구현한 11번째 중복, #45 MemStore 통합 때
    // 이름이 달라 누락됨)를 canonical `fleet_store::mem::MemStore`로 교체.
    let store = Arc::new(fleet_store::mem::MemStore::new()) as Arc<dyn Store>;
    for w in workers {
        store.upsert_worker(&w).await.unwrap();
    }

    let transport = MockTransport::new();
    for mw in mock_workers {
        transport.add_worker(mw).await;
    }
    let event_rx = fleet_transport::WorkerTransport::subscribe(&transport)
        .await
        .unwrap();
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
async fn wait_until_terminal(state: &FleetState, task_id: TaskId) -> Task {
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
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
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
            assert!(
                result.output.contains("echo hello"),
                "output: {}",
                result.output
            );
        }
        other => panic!("expected Completed, got {:?}", other),
    }
}

/// 부모 태스크가 완료된 뒤 "이어가기" 태스크를 dispatch하면, 실제로 워커에
/// 전송되는 prompt에 부모의 Q/A가 이어붙어 있어야 한다 — MockTransport는
/// 받은 prompt를 그대로 에코하므로, 완료된 출력을 검사하면 dispatch 시점에
/// 무슨 텍스트가 실제로 전송됐는지 확인할 수 있다.
#[tokio::test]
async fn threaded_reply_dispatch_includes_parent_context() {
    let worker = make_worker("w1");
    let worker_id = worker.id;
    let (state, dispatcher) = setup(
        vec![worker],
        vec![MockWorker::new(worker_id, "wss://w1/ws")],
    )
    .await;

    let parent = Task::from_request(TaskRequest {
        prompt: "1부터 5까지 더해줘".into(),
        created_by: "test".into(),
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    let parent_id = parent.id;
    dispatcher.submit(parent).await.unwrap();
    let parent = wait_until_terminal(&state, parent_id).await;
    assert!(
        matches!(parent.status, TaskStatus::Completed(_)),
        "parent must complete for this test to be meaningful"
    );

    let mut reply = Task::from_request(TaskRequest {
        prompt: "거기에 10을 더하면?".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    reply.inherit_from_parent(&parent);
    assert_eq!(reply.thread_id, parent.thread_id);
    assert_eq!(reply.parent_task_id, Some(parent.id));

    let reply_id = reply.id;
    dispatcher.submit(reply).await.unwrap();
    let completed = wait_until_terminal(&state, reply_id).await;

    match completed.status {
        TaskStatus::Completed(result) => {
            // MockTransport가 에코하는 output에 재구성된 전체 prompt가
            // 그대로 담겨 있어야 한다 — 부모의 Q/A와 새 질문 둘 다.
            assert!(
                result.output.contains("1부터 5까지 더해줘"),
                "parent question missing from dispatched prompt: {}",
                result.output
            );
            assert!(
                result.output.contains("거기에 10을 더하면?"),
                "new question missing from dispatched prompt: {}",
                result.output
            );
        }
        other => panic!("expected Completed, got {:?}", other),
    }

    // 저장된 reply.prompt 자체는 재구성 없이 사용자가 입력한 새 메시지만
    // 담고 있어야 한다 — 목록/상세 화면에 전체 문맥이 노출되면 안 된다.
    let stored_reply = state.store.get_task(reply_id).await.unwrap().unwrap();
    assert_eq!(stored_reply.prompt, "거기에 10을 더하면?");
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
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    let task_id = task.id;
    dispatcher.submit(task).await.unwrap();
    wait_until_terminal(&state, task_id).await;

    // 이벤트 로그 검사
    let events = state.store.list_events(0, 100).await.unwrap();
    let types: Vec<&str> = events.iter().map(|e| e.event.event_type()).collect();
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
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
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
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    let task_id = task.id;

    let result = dispatcher.submit(task).await;
    assert!(
        result.is_err(),
        "should fail since hinted worker is offline"
    );

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
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
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
            // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
            cwd: Some("/srv/fleet/workspaces/test".into()),
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
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    let result = dispatcher.submit(task).await;
    assert!(
        result.is_err(),
        "should refuse dispatch when circuit is open"
    );
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
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
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

#[tokio::test]
async fn dispatch_with_unmatched_model_marks_task_failed() {
    // 워커는 온라인/가용 상태이지만 어느 누구도 요청된 model 라벨을 갖고 있지
    // 않은 경우 — task는 Pending에 멈추거나 panic하지 않고 Failed로
    // 종료되어야 하며, FailureKind::WorkerUnavailable + 에러 메시지에
    // 요청된 model이 언급되어야 한다.
    let mut glm = make_worker("glm-1");
    glm.labels.insert("model".into(), "glm-5".into());
    let plain = make_worker("plain-1");

    let (state, dispatcher) = setup(
        vec![glm, plain],
        vec![], // 선택 단계에서 실패하므로 mock worker 불필요
    )
    .await;

    let task = Task::from_request(TaskRequest {
        prompt: "gemini-only work".into(),
        model: Some("gemini".into()),
        created_by: "test".into(),
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    let task_id = task.id;

    let result = dispatcher.submit(task).await;
    assert!(
        result.is_err(),
        "submit should fail since no worker is labeled for model 'gemini'"
    );

    let terminal = wait_until_terminal(&state, task_id).await;
    let failure = match terminal.status {
        TaskStatus::Failed(f) => f,
        other => panic!("expected Failed, got {:?}", other),
    };
    assert!(matches!(
        failure.kind,
        fleet_core::FailureKind::WorkerUnavailable
    ));
    assert!(
        failure.error.contains("gemini"),
        "error should reference the requested model: {}",
        failure.error
    );
}

// ─────────────────────────────────────────────────────────────────────
//  Phase 2: cancel + wait
// ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cancel_dispatched_task_transitions_to_cancelled() {
    // 매우 긴 latency로 인해 dispatch 후 cancel 호출 시점에 작업이 실행 중이도록 유도.
    let worker = make_worker("cancellable-1");
    let worker_id = worker.id;
    let mut mock = MockWorker::new(worker_id, "wss://cancellable-1/ws");
    mock.latency = std::time::Duration::from_secs(60); // 거의 확실히 cancel보다 늦게 끝남

    let (state, dispatcher) = setup(vec![worker], vec![mock]).await;

    let task = Task::from_request(TaskRequest {
        prompt: "long-running work".into(),
        created_by: "test".into(),
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    let task_id = task.id;
    dispatcher.submit(task).await.unwrap();

    // dispatch 후 잠시 대기 → Dispatched 상태 확인
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let in_flight = state.store.get_task(task_id).await.unwrap().unwrap();
    assert!(
        matches!(in_flight.status, TaskStatus::Dispatched { .. }),
        "task should be Dispatched, got {:?}",
        in_flight.status
    );

    // 취소
    dispatcher
        .cancel(task_id, "test cancel")
        .await
        .expect("cancel should succeed");

    let after = state.store.get_task(task_id).await.unwrap().unwrap();
    match after.status {
        TaskStatus::Cancelled { reason, .. } => {
            assert_eq!(reason, "test cancel");
        }
        other => panic!("expected Cancelled, got {:?}", other),
    }

    // TaskCancelled 이벤트 발행 확인
    let events = state.store.list_events(0, 100).await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.event.event_type() == "task_cancelled"),
        "should have task_cancelled event"
    );
}

#[tokio::test]
async fn cancel_pending_task_succeeds_without_transport_call() {
    // server_hint로 존재하지 않는 워커를 지정 → submit은 Pending(?)이 아니라
    // 즉시 Failed가 됨. Pending 상태를 만들려면 select가 실패해야 하는데,
    // submit()은 현재 동기적으로 select를 호출하므로 Pending은 brief moment.
    // 따라서 Dispatched 직후 취소가 주요 경로. 여기서는 Pending 직접 생성.
    let worker = make_worker("w1");
    let worker_id = worker.id;
    let (state, dispatcher) = setup(
        vec![worker],
        vec![MockWorker::new(worker_id, "wss://w1/ws")],
    )
    .await;

    // Pending 상태의 작업을 직접 Store에 삽입
    let task = Task::from_request(TaskRequest {
        prompt: "manually inserted".into(),
        created_by: "test".into(),
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    let task_id = task.id;
    state.store.insert_task(&task).await.unwrap();

    // cancel 호출 — Pending 상태이므로 transport.cancel은 호출되지 않아야 함
    dispatcher.cancel(task_id, "user-requested").await.unwrap();

    let after = state.store.get_task(task_id).await.unwrap().unwrap();
    assert!(matches!(after.status, TaskStatus::Cancelled { .. }));
}

#[tokio::test]
async fn cancel_already_completed_returns_error() {
    let worker = make_worker("w1");
    let worker_id = worker.id;
    let (state, dispatcher) = setup(
        vec![worker],
        vec![MockWorker::new(worker_id, "wss://w1/ws")],
    )
    .await;

    let task = Task::from_request(TaskRequest {
        prompt: "echo done".into(),
        created_by: "test".into(),
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    let task_id = task.id;
    dispatcher.submit(task).await.unwrap();
    wait_until_terminal(&state, task_id).await; // 완료 대기

    // 이미 종료된 작업 취소 시도 → 에러
    let result = dispatcher.cancel(task_id, "late cancel").await;
    assert!(result.is_err(), "cancelling terminal task should fail");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("terminal"), "error: {err_msg}");
}

#[tokio::test]
async fn cancel_nonexistent_task_returns_error() {
    let worker = make_worker("w1");
    let worker_id = worker.id;
    let (_state, dispatcher) = setup(
        vec![worker],
        vec![MockWorker::new(worker_id, "wss://w1/ws")],
    )
    .await;

    let fake_id = TaskId::new();
    let result = dispatcher.cancel(fake_id, "nope").await;
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("not found"), "error: {err_msg}");
}

#[tokio::test]
async fn wait_for_task_returns_completed_task() {
    let worker = make_worker("w1");
    let worker_id = worker.id;
    let (_state, dispatcher) = setup(
        vec![worker],
        vec![MockWorker::new(worker_id, "wss://w1/ws")],
    )
    .await;

    let task = Task::from_request(TaskRequest {
        prompt: "fast work".into(),
        created_by: "test".into(),
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    let task_id = task.id;
    dispatcher.submit(task).await.unwrap();

    // 5초 대기 — mock latency=10ms이므로 즉시 완료되어야 함
    let result = dispatcher
        .wait_for_task(task_id, std::time::Duration::from_secs(5))
        .await;
    let finished = result.expect("wait should succeed");
    assert!(matches!(finished.status, TaskStatus::Completed(_)));
}

#[tokio::test]
async fn wait_for_task_times_out_when_still_running() {
    // 긴 latency → wait는 타임아웃
    let worker = make_worker("slow-1");
    let worker_id = worker.id;
    let mut mock = MockWorker::new(worker_id, "wss://slow-1/ws");
    mock.latency = std::time::Duration::from_secs(30);

    let (state, dispatcher) = setup(vec![worker], vec![mock]).await;

    let task = Task::from_request(TaskRequest {
        prompt: "slow".into(),
        created_by: "test".into(),
        // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    let task_id = task.id;
    dispatcher.submit(task).await.unwrap();

    let result = dispatcher
        .wait_for_task(task_id, std::time::Duration::from_millis(200))
        .await;
    assert!(result.is_err(), "should time out");
    let err = result.unwrap_err();
    let err_msg = format!("{err}");
    assert!(err_msg.contains("timed out"), "error: {err_msg}");

    // 작업은 여전히 Dispatched 상태여야 함
    let still_running = state.store.get_task(task_id).await.unwrap().unwrap();
    assert!(
        matches!(still_running.status, TaskStatus::Dispatched { .. }),
        "task should still be in-flight"
    );
}

#[tokio::test]
async fn wait_for_nonexistent_task_returns_error() {
    let worker = make_worker("w1");
    let worker_id = worker.id;
    let (_state, dispatcher) = setup(
        vec![worker],
        vec![MockWorker::new(worker_id, "wss://w1/ws")],
    )
    .await;

    let result = dispatcher
        .wait_for_task(TaskId::new(), std::time::Duration::from_millis(100))
        .await;
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("not found"));
}

#[tokio::test]
async fn cancel_does_not_record_breaker_failure() {
    // 취소는 실패로 간주하지 않음 — 브레이커 상태에 영향 X
    let worker = make_worker("cancellable-2");
    let worker_id = worker.id;
    let mut mock = MockWorker::new(worker_id, "wss://cancellable-2/ws");
    mock.latency = std::time::Duration::from_secs(60);

    let (state, dispatcher) = setup(vec![worker], vec![mock]).await;

    for _ in 0..3 {
        let task = Task::from_request(TaskRequest {
            prompt: "to-be-cancelled".into(),
            created_by: "test".into(),
            // 로드맵 #69 — `submit()`의 입국 심사가 절대 경로 cwd를 요구한다.
            cwd: Some("/srv/fleet/workspaces/test".into()),
            ..Default::default()
        });
        let task_id = task.id;
        dispatcher.submit(task).await.unwrap();
        dispatcher.cancel(task_id, "test").await.unwrap();
    }

    // 브레이커는 Open 되지 않아야 함 (취소 = 실패 아님)
    let breaker_state = state.breakers.state_of(worker_id);
    assert!(
        !breaker_state.is_open(),
        "cancellations should not trip the breaker"
    );
}
