//! HTTP API ↔ Transport 통합 테스트.
//!
//! `/v1/workers/register`와 `/v1/workers/:id` DELETE가
//! AppState.transport를 통해 실제로 transport.register/unregister를
//! 호출하는지 검증.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fleet_api::{build_app, ApiTokenCredential, AppState};
use fleet_core::{TaskId, WorkerId};
use fleet_store::Store;
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
    async fn register(
        &self,
        worker_id: WorkerId,
        endpoint: &str,
        _max_concurrent_tasks: u32,
    ) -> Result<(), TransportError> {
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

use fleet_store::mem::MemStore;

/// ephemeral port에 HTTP API 서버 시작. base URL 반환.
async fn spawn_server(transport: Arc<dyn WorkerTransport>) -> String {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let state = Arc::new(
        AppState::new(store)
            .with_transport(transport)
            .with_tokens(vec![ApiTokenCredential {
                principal_id: "transport-test".into(),
                token: "test-token".into(),
                capabilities: fleet_core::PermissionKind::all().to_vec(),
            }]),
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

/// 로드맵 #75 — `transport.register`는 워커에 실제로 다이얼해야 하므로
/// `server-key` 원문을 그대로 받아야 한다(위 테스트가 확인). 하지만 그
/// 이후 같은 워커를 조회하면 원문이 나오면 안 된다 — 다이얼에 필요한
/// 소비자와 조회 응답의 소비자는 서로 다른 권한 경계를 갖는다.
#[tokio::test]
async fn registered_worker_endpoint_is_masked_on_read_even_though_transport_got_the_raw_value() {
    let (transport, reg_log, _unreg_log) = RecordingTransport::new_shared();
    let url = spawn_server(transport).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{url}/v1/workers/register"))
        .header("authorization", "Bearer test-token")
        .json(&serde_json::json!({
            "name": "readback-worker",
            "agent_endpoint": "ws://127.0.0.1:9997/ws?server-key=readback-secret",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let worker_id = resp.json::<serde_json::Value>().await.unwrap()["worker_id"]
        .as_str()
        .unwrap()
        .to_string();

    // transport는 여전히 원문을 받는다 — 다이얼에 필요하므로.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        reg_log.lock().unwrap()[0].1,
        "ws://127.0.0.1:9997/ws?server-key=readback-secret"
    );

    // 그러나 GET /v1/workers/{id} 응답에는 마스킹된 값만 있다.
    let get_resp = client
        .get(format!("{url}/v1/workers/{worker_id}"))
        .header("authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), 200);
    let body = get_resp.text().await.unwrap();
    assert!(
        !body.contains("readback-secret"),
        "GET response must not contain the raw server-key: {body}"
    );
    assert!(body.contains("<redacted>"));
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
        async fn register(
            &self,
            _: WorkerId,
            _: &str,
            _max_concurrent_tasks: u32,
        ) -> Result<(), TransportError> {
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
