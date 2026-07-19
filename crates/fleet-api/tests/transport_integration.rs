//! HTTP API ↔ Transport 통합 테스트.
//!
//! `/v1/workers/register`와 `/v1/workers/:id` DELETE가
//! AppState.transport를 통해 실제로 transport.register/unregister를
//! 호출하는지 검증.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fleet_api::{build_app, AppState};
use fleet_core::{
    EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput, TaskStatus, Worker, WorkerFilter,
    WorkerHeartbeat, WorkerId,
};
use fleet_store::{Store, StoreError};
use fleet_transport::{DispatchRequest, TransportError, WorkerEvent, WorkerTransport};

/// `RecordingTransport::new_shared`의 반환 타입 별칭.
type SharedRecording = (
    Arc<dyn WorkerTransport>,
    Arc<Mutex<Vec<(WorkerId, String)>>>,
    Arc<Mutex<Vec<WorkerId>>>,
);

/// 호출을 기록하는 테스트용 transport.
#[allow(dead_code)]
struct RecordingTransport {
    registrations: Arc<Mutex<Vec<(WorkerId, String)>>>,
    unregistrations: Arc<Mutex<Vec<WorkerId>>>,
}

impl RecordingTransport {
    fn new_shared() -> SharedRecording {
        let reg = Arc::new(Mutex::new(Vec::new()));
        let unreg = Arc::new(Mutex::new(Vec::new()));
        let inner = Arc::new(RecordingTransportShared {
            registrations: reg.clone(),
            unregistrations: unreg.clone(),
        });
        (inner as Arc<dyn WorkerTransport>, reg, unreg)
    }
}

struct RecordingTransportShared {
    registrations: Arc<Mutex<Vec<(WorkerId, String)>>>,
    unregistrations: Arc<Mutex<Vec<WorkerId>>>,
}

#[async_trait]
impl WorkerTransport for RecordingTransportShared {
    async fn register(&self, worker_id: WorkerId, endpoint: &str) -> Result<(), TransportError> {
        self.registrations
            .lock()
            .unwrap()
            .push((worker_id, endpoint.to_string()));
        Ok(())
    }
    async fn unregister(&self, worker_id: WorkerId) -> Result<(), TransportError> {
        self.unregistrations.lock().unwrap().push(worker_id);
        Ok(())
    }
    async fn is_connected(&self, _: WorkerId) -> bool {
        true
    }
    async fn dispatch(&self, _: DispatchRequest) -> Result<(), TransportError> {
        Ok(())
    }
    async fn cancel(&self, _: TaskId) -> Result<(), TransportError> {
        Ok(())
    }
    async fn ping(&self, _: WorkerId) -> Result<Duration, TransportError> {
        Ok(Duration::from_millis(1))
    }
    async fn subscribe(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<WorkerEvent>, TransportError> {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Ok(rx)
    }
}

/// 인메모리 Store (테스트 전용).
struct MemStore {
    workers: Mutex<std::collections::HashMap<WorkerId, Worker>>,
    events: Mutex<Vec<EventEntry>>,
}

impl MemStore {
    fn new() -> Self {
        Self {
            workers: Mutex::new(std::collections::HashMap::new()),
            events: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Store for MemStore {
    async fn insert_task(&self, _: &Task) -> Result<(), StoreError> {
        Ok(())
    }
    async fn get_task(&self, _: TaskId) -> Result<Option<Task>, StoreError> {
        Ok(None)
    }
    async fn update_task_status(&self, _: TaskId, _: &TaskStatus) -> Result<(), StoreError> {
        Ok(())
    }
    async fn list_tasks(&self, _: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        Ok(Vec::new())
    }
    async fn upsert_worker(&self, w: &Worker) -> Result<(), StoreError> {
        self.workers.lock().unwrap().insert(w.id, w.clone());
        Ok(())
    }
    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
        Ok(self.workers.lock().unwrap().get(&id).cloned())
    }
    async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError> {
        Ok(self
            .workers
            .lock()
            .unwrap()
            .values()
            .find(|w| w.name == name)
            .cloned())
    }
    async fn list_workers(&self, _: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
        Ok(self.workers.lock().unwrap().values().cloned().collect())
    }
    async fn delete_worker(&self, id: WorkerId) -> Result<(), StoreError> {
        self.workers.lock().unwrap().remove(&id);
        Ok(())
    }
    async fn update_worker_heartbeat(
        &self,
        _: WorkerId,
        _: &WorkerHeartbeat,
    ) -> Result<(), StoreError> {
        Ok(())
    }
    async fn append_event(&self, e: &FleetEvent) -> Result<u64, StoreError> {
        let mut events = self.events.lock().unwrap();
        let seq = (events.len() + 1) as u64;
        events.push(EventEntry {
            seq,
            event: e.clone(),
        });
        Ok(seq)
    }
    async fn list_events(&self, after: u64, limit: u32) -> Result<Vec<EventEntry>, StoreError> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.seq > after)
            .take(limit as usize)
            .cloned()
            .collect())
    }
    async fn append_output(&self, _: TaskId, _: &str) -> Result<u64, StoreError> {
        Ok(0)
    }
    async fn get_output(&self, task_id: TaskId, _: u64) -> Result<TaskOutput, StoreError> {
        Ok(TaskOutput {
            task_id,
            chunks: Vec::new(),
            next_offset: 0,
        })
    }
    async fn migrate(&self) -> Result<(), StoreError> {
        Ok(())
    }
}

/// ephemeral port에 HTTP API 서버 시작. base URL 반환.
async fn spawn_server(
    transport: Arc<dyn WorkerTransport>,
) -> String {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let state = Arc::new(
        AppState::new(store)
            .with_transport(transport)
            .with_tokens(vec!["test-token".to_string()]),
    );

    // 미리 bind하여 addr 확보 후, listener를 그대로 spawn된 task에 전달.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = build_app(state);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn register_calls_transport_register() {
    let (transport, reg_log, _unreg_log) = RecordingTransport::new_shared();
    let url = spawn_server(transport).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{url}/v1/workers/register"))
        .header("authorization", "Bearer test-token")
        .json(&serde_json::json!({
            "name": "test-worker",
            "agent_endpoint": "ws://127.0.0.1:9999/ws?server-key=x",
            "max_concurrent_tasks": 2,
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // transport.register가 호출되었는지 확인.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let reg = reg_log.lock().unwrap();
    assert_eq!(reg.len(), 1, "transport.register should be called once");
    assert_eq!(reg[0].1, "ws://127.0.0.1:9999/ws?server-key=x");
}

#[tokio::test]
async fn deregister_calls_transport_unregister() {
    let (transport, _reg_log, unreg_log) = RecordingTransport::new_shared();
    let url = spawn_server(transport).await;

    let client = reqwest::Client::new();
    // 1. register
    let resp = client
        .post(format!("{url}/v1/workers/register"))
        .header("authorization", "Bearer test-token")
        .json(&serde_json::json!({
            "name": "test-worker-2",
            "agent_endpoint": "ws://127.0.0.1:9998/ws",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let worker_id = body["worker_id"].as_str().unwrap().to_string();

    // 2. deregister
    let resp = client
        .delete(format!("{url}/v1/workers/{worker_id}"))
        .header("authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 3. transport.unregister 호출 검증.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let unreg = unreg_log.lock().unwrap();
    assert_eq!(unreg.len(), 1, "transport.unregister should be called");
}

#[tokio::test]
async fn transport_failure_does_not_break_store_registration() {
    // transport.register이 실패해도 Store upsert는 성공해야 함 (3.5 단계에서 warn만).
    struct FailingTransport;
    #[async_trait]
    impl WorkerTransport for FailingTransport {
        async fn register(&self, _: WorkerId, _: &str) -> Result<(), TransportError> {
            Err(TransportError::Connection("synthetic failure".into()))
        }
        async fn unregister(&self, _: WorkerId) -> Result<(), TransportError> {
            Ok(())
        }
        async fn is_connected(&self, _: WorkerId) -> bool {
            false
        }
        async fn dispatch(&self, _: DispatchRequest) -> Result<(), TransportError> {
            Ok(())
        }
        async fn cancel(&self, _: TaskId) -> Result<(), TransportError> {
            Ok(())
        }
        async fn ping(&self, _: WorkerId) -> Result<Duration, TransportError> {
            Ok(Duration::from_millis(1))
        }
        async fn subscribe(
            &self,
        ) -> Result<tokio::sync::mpsc::UnboundedReceiver<WorkerEvent>, TransportError> {
            let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
            Ok(rx)
        }
    }

    let url = spawn_server(Arc::new(FailingTransport)).await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{url}/v1/workers/register"))
        .header("authorization", "Bearer test-token")
        .json(&serde_json::json!({
            "name": "fail-test",
            "agent_endpoint": "ws://x/ws",
        }))
        .send()
        .await
        .unwrap();

    // transport 실패는 warn 로그만 — HTTP 응답은 여전히 200.
    assert_eq!(resp.status(), 200);
}
