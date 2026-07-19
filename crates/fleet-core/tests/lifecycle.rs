//! fleet-core 통합 테스트.
//!
//! 도메인 모델을 사용한 작업 라이프사이클 시나리오를 검증합니다.
//! - 작업 생성 → dispatch → 완료/실패 상태 전이
//! - 모든 상태의 JSON 직렬화 라운드트립
//! - FleetEvent 로그 생성

use chrono::Utc;
use fleet_core::{
    CircuitState, FailureKind, FleetEvent, Labels, Task, TaskFailure, TaskFilter, TaskId,
    TaskPriority, TaskRequest, TaskResult, TaskStatus, Worker, WorkerId, WorkerStatus,
};

/// 전형적인 작업 라이프사이클: Pending → Dispatched → Completed.
#[test]
fn task_lifecycle_pending_to_completed() {
    let req = TaskRequest {
        prompt: "Build the project".into(),
        cwd: Some("/srv/app".into()),
        model: Some("grok-4".into()),
        server_hint: Some("build-farm-1".into()),
        required_labels: vec!["linux".into()],
        max_turns: Some(10),
        timeout_secs: Some(600),
        priority: TaskPriority::High,
        created_by: "admin@org".into(),
    };
    let task = Task::from_request(req);
    assert!(matches!(task.status, TaskStatus::Pending));

    // 1. Pending 상태 직렬화/역직렬화
    let json = serde_json::to_string(&task).unwrap();
    let back: Task = serde_json::from_str(&json).unwrap();
    assert_eq!(task.id, back.id);
    assert_eq!(task.prompt, back.prompt);
    assert!(matches!(back.status, TaskStatus::Pending));

    // 2. Dispatched로 전이
    let worker_id = WorkerId::new();
    let dispatched = TaskStatus::Dispatched {
        worker_id,
        started_at: Utc::now(),
    };
    let mut task = task;
    task.status = dispatched.clone();
    assert!(task.is_running());
    assert!(!task.is_terminal());

    // 3. Completed로 전이
    let result = TaskResult {
        output: "Build OK".into(),
        exit_code: 0,
        duration_secs: 12.5,
        token_usage: None,
        worker_id,
        finished_at: Utc::now(),
    };
    task.status = TaskStatus::Completed(result.clone());
    assert!(task.is_terminal());
    assert!(!task.is_running());

    // 최종 상태 직렬화 검증
    let final_json = serde_json::to_value(&task.status).unwrap();
    assert_eq!(final_json["phase"], "completed");
}

/// 실패 라이프사이클: Dispatched → Failed(CircuitOpen).
#[test]
fn task_lifecycle_failed_with_circuit() {
    let worker_id = WorkerId::new();
    let failure = TaskFailure {
        error: "worker circuit opened after 5 failures".into(),
        kind: FailureKind::CircuitOpen,
        worker_id: Some(worker_id),
        attempts: 3,
    };

    let status = TaskStatus::Failed(failure.clone());
    let json = serde_json::to_string(&status).unwrap();
    let back: TaskStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(status, back);

    if let TaskStatus::Failed(f) = back {
        assert_eq!(f.kind, FailureKind::CircuitOpen);
        assert_eq!(f.attempts, 3);
        assert_eq!(f.worker_id, Some(worker_id));
    } else {
        panic!("expected Failed status");
    }
}

/// 모든 FleetEvent 변형의 JSON 라운드트립.
#[test]
fn all_fleet_events_roundtrip() {
    let tid = TaskId::new();
    let wid = WorkerId::new();

    let events = vec![
        FleetEvent::task_created(tid, None, "admin@org"),
        FleetEvent::task_dispatched(tid, wid),
        FleetEvent::TaskProgress {
            task_id: tid,
            worker_id: wid,
            seq: 1,
            chunk: "stdout chunk".into(),
            at: Utc::now(),
        },
        FleetEvent::task_completed(
            tid,
            wid,
            TaskResult {
                output: "done".into(),
                exit_code: 0,
                duration_secs: 1.0,
                token_usage: None,
                worker_id: wid,
                finished_at: Utc::now(),
            },
        ),
        FleetEvent::task_failed(
            tid,
            TaskFailure {
                error: "boom".into(),
                kind: FailureKind::WorkerError,
                worker_id: Some(wid),
                attempts: 1,
            },
        ),
        FleetEvent::task_cancelled(tid, "user request"),
        FleetEvent::worker_joined(wid, "build-farm-1", "wss://worker/ws"),
        FleetEvent::worker_left(wid, "heartbeat timeout"),
        FleetEvent::worker_circuit_changed(wid, CircuitState::Closed, CircuitState::Open),
    ];

    for event in &events {
        let json = serde_json::to_string(event).unwrap();
        let back: FleetEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_type(), event.event_type());
        assert_eq!(back.at(), event.at());
    }
}

/// 워커 디스패치 가능성 판별.
#[test]
fn worker_dispatchability_matrix() {
    let mut w = Worker::new("w1", "wss://w1");
    assert!(w.is_dispatchable());

    // 용량 초과
    w.active_tasks = w.max_concurrent;
    assert!(!w.is_dispatchable());

    // 용량 복구, 회로 열림
    w.active_tasks = 0;
    w.circuit_state = CircuitState::Open;
    assert!(!w.is_dispatchable());

    // 회로 복구, 오프라인
    w.circuit_state = CircuitState::Closed;
    w.status = WorkerStatus::Offline;
    assert!(!w.is_dispatchable());

    // 정상 복귀
    w.status = WorkerStatus::Online;
    assert!(w.is_dispatchable());
}

/// 작업/워커 필터 JSON 호환성 (Store API에서 사용).
#[test]
fn filter_serialization() {
    let tf = TaskFilter {
        status: None,
        worker_id: Some(WorkerId::new()),
        created_by: Some("admin@org".into()),
        limit: 50,
    };
    let json = serde_json::to_string(&tf).unwrap();
    assert!(json.contains("\"limit\":50"));
    let _back: TaskFilter = serde_json::from_str(&json).unwrap();
}

/// 라벨 맵 직렬화.
#[test]
fn labels_serialize_as_object() {
    let mut labels: Labels = Labels::new();
    labels.insert("arch".into(), "arm64".into());
    labels.insert("gpu".into(), "true".into());

    let json = serde_json::to_value(&labels).unwrap();
    assert_eq!(json["arch"], "arm64");
    assert_eq!(json["gpu"], "true");
}
