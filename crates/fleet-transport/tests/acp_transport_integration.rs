//! AcpTransport end-to-end 테스트.
//!
//! mock ACP 서버 (axum WebSocket) ↔ AcpTransport (WorkerTransport trait 구현체).
//! Phase 7 p7-6의 일환으로, dispatch → Output 스트리밍 → Completed 흐름을 검증.

#![cfg(feature = "acp")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use fleet_core::{TaskId, WorkerId};
use fleet_transport::{AcpTransport, WorkerEvent, WorkerTransport};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::timeout;

#[derive(Clone, Default)]
struct MockState {
    received: Arc<Mutex<Vec<Value>>>,
    next_prompt_id: Arc<Mutex<u64>>,
    scripted_output: Arc<Mutex<Vec<String>>>,
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    #[serde(rename = "server-key", default)]
    #[allow(dead_code)]
    server_key: Option<String>,
}

async fn ws_handler(
    Query(_q): Query<WsQuery>,
    ws: WebSocketUpgrade,
    State(state): State<MockState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_acp_socket(socket, state))
}

async fn handle_acp_socket(socket: WebSocket, state: MockState) {
    use futures_util::{SinkExt, StreamExt};

    let (mut writer, mut reader) = socket.split();

    while let Some(msg) = reader.next().await {
        let text = match msg {
            Ok(WsMessage::Text(t)) => t,
            Ok(WsMessage::Close(_)) | Err(_) => break,
            _ => continue,
        };

        let req: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let method = req
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let id = req.get("id").cloned();

        state.received.lock().await.push(req);

        let response = match method.as_str() {
            "initialize" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": 1,
                    "serverCapabilities": { "streaming": true },
                },
            })),
            "session/new" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "sessionId": "test-session-1",
                },
            })),
            "session/prompt" => {
                let prompt_id = {
                    let mut next = state.next_prompt_id.lock().await;
                    *next += 1;
                    *next
                };

                let chunks: Vec<String> = state.scripted_output.lock().await.clone();
                for chunk in &chunks {
                    let update = json!({
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": "test-session-1",
                            "promptId": prompt_id,
                            "update": {
                                "type": "agent_message_chunk",
                                "content": {
                                    "agent_message": [{
                                        "type": "text",
                                        "text": chunk,
                                    }],
                                },
                            },
                        },
                    });
                    let _ = writer.send(WsMessage::Text(update.to_string())).await;
                }

                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "prompt_id": prompt_id,
                        "agent_message": [{
                            "type": "text",
                            "text": chunks.join(""),
                        }],
                        "end_of_turn": true,
                        "usage": {
                            "input_tokens": 5,
                            "output_tokens": 10,
                        },
                    },
                }))
            }
            "session/cancel" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {},
            })),
            _ => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "not found" },
            })),
        };

        if let Some(resp) = response {
            let _ = writer.send(WsMessage::Text(resp.to_string())).await;
        }
    }
}

async fn start_mock_server() -> (MockState, SocketAddr) {
    let state = MockState::default();
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (state, addr)
}

fn endpoint(addr: SocketAddr) -> String {
    format!("ws://{addr}/ws?server-key=test")
}

fn dispatch_req(task_id: TaskId, worker_id: WorkerId, prompt: &str) -> fleet_transport::DispatchRequest {
    fleet_transport::DispatchRequest {
        task_id,
        worker_id,
        prompt: prompt.to_string(),
        cwd: None,
        model: None,
        max_turns: None,
        timeout_secs: Some(30),
    }
}

#[tokio::test]
async fn register_unregister_worker() {
    let (_state, addr) = start_mock_server().await;
    let transport = AcpTransport::new();

    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint(addr))
        .await
        .expect("register");

    assert!(transport.is_connected(worker).await);

    transport.unregister(worker).await.expect("unregister");
    assert!(!transport.is_connected(worker).await);
}

#[tokio::test]
async fn duplicate_register_rejected() {
    let (_state, addr) = start_mock_server().await;
    let transport = AcpTransport::new();

    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint(addr))
        .await
        .expect("register");
    let second = transport.register(worker, &endpoint(addr)).await;
    assert!(second.is_err(), "duplicate register should fail");
}

#[tokio::test]
async fn dispatch_streams_output_and_completes() {
    let (state, addr) = start_mock_server().await;
    *state.scripted_output.lock().await = vec!["Hello ".to_string(), "world".to_string()];

    let transport = Arc::new(AcpTransport::new());
    let mut events = transport.subscribe().await.expect("subscribe");

    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint(addr))
        .await
        .expect("register");

    let task_id = TaskId::new();
    transport
        .dispatch(dispatch_req(task_id, worker, "hi"))
        .await
        .expect("dispatch");

    // 이벤트 수집: Output 2개 + Completed 1개 예상.
    let mut output = String::new();
    let mut completed = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Some(WorkerEvent::Output { task_id: t, chunk, .. })) => {
                assert_eq!(t, task_id);
                output.push_str(&chunk);
            }
            Ok(Some(WorkerEvent::Completed { task_id: t, .. })) => {
                assert_eq!(t, task_id);
                completed = true;
                break;
            }
            Ok(Some(WorkerEvent::Failed { task_id: t, error })) => {
                panic!("unexpected Failed for {t}: {error}");
            }
            Ok(None) => break,
            Err(_) => continue,
        }
    }
    assert!(completed, "should receive Completed");
    assert_eq!(output, "Hello world");
}

#[tokio::test]
async fn completed_includes_token_usage() {
    let (state, addr) = start_mock_server().await;
    *state.scripted_output.lock().await = vec!["x".to_string()];

    let transport = Arc::new(AcpTransport::new());
    let mut events = transport.subscribe().await.expect("subscribe");

    let worker = WorkerId::new();
    transport.register(worker, &endpoint(addr)).await.unwrap();

    let task_id = TaskId::new();
    transport.dispatch(dispatch_req(task_id, worker, "x")).await.unwrap();

    let mut found = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Some(WorkerEvent::Completed { result, .. })) => {
                found = Some(result);
                break;
            }
            Ok(Some(_)) | Ok(None) => continue,
            Err(_) => continue,
        }
    }
    let result = found.expect("Completed event");
    let usage = result.token_usage.expect("token_usage");
    assert_eq!(usage.input_tokens, 5);
    assert_eq!(usage.output_tokens, 10);
    assert_eq!(result.output, "x");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn dispatch_unknown_worker_errors() {
    let transport = AcpTransport::new();
    let req = dispatch_req(TaskId::new(), WorkerId::new(), "x");
    let result = transport.dispatch(req).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn ping_registered_worker_ok() {
    let (_state, addr) = start_mock_server().await;
    let transport = AcpTransport::new();

    let worker = WorkerId::new();
    transport.register(worker, &endpoint(addr)).await.unwrap();

    let dur = transport.ping(worker).await.expect("ping");
    assert!(dur.as_millis() <= 1);
}

#[tokio::test]
async fn unregister_unknown_worker_errors() {
    let transport = AcpTransport::new();
    let result = transport.unregister(WorkerId::new()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn cancel_unknown_task_is_noop() {
    let (_state, addr) = start_mock_server().await;
    let transport = AcpTransport::new();

    let worker = WorkerId::new();
    transport.register(worker, &endpoint(addr)).await.unwrap();

    // 활성 task가 없으므로 cancel은 idempotent success.
    transport
        .cancel(TaskId::new())
        .await
        .expect("cancel no-op");
}

#[tokio::test]
async fn multiple_workers_dispatched_independently() {
    let (state, addr) = start_mock_server().await;
    *state.scripted_output.lock().await = vec!["from-w1".to_string()];

    let transport = Arc::new(AcpTransport::new());
    let mut events = transport.subscribe().await.expect("subscribe");

    let w1 = WorkerId::new();
    transport.register(w1, &endpoint(addr)).await.unwrap();

    // 두 번째 워커 등록을 위해 다른 서버 인스턴스 (scripted_output 다르게).
    let (state2, addr2) = start_mock_server().await;
    *state2.scripted_output.lock().await = vec!["from-w2".to_string()];
    let w2 = WorkerId::new();
    transport.register(w2, &endpoint(addr2)).await.unwrap();

    let t1 = TaskId::new();
    let t2 = TaskId::new();
    transport.dispatch(dispatch_req(t1, w1, "x")).await.unwrap();
    transport.dispatch(dispatch_req(t2, w2, "y")).await.unwrap();

    // 두 task 모두 Completed 수신.
    let mut seen = std::collections::HashSet::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while seen.len() < 2 && std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Some(WorkerEvent::Completed { task_id, .. })) => {
                seen.insert(task_id);
            }
            _ => continue,
        }
    }
    assert!(seen.contains(&t1), "t1 should complete");
    assert!(seen.contains(&t2), "t2 should complete");
}
