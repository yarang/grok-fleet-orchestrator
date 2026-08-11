//! `AcpTransport` end-to-end 테스트 (2026-08-11 SDK 전환 이후 재작성).
//!
//! mock ACP 서버(axum WebSocket)가 실제 grok과 동일한 wire-format으로 응답한다.
//! `AcpTransport`는 공식 `agent-client-protocol`/`agent-client-protocol-http`
//! SDK 위에서 동작하며, **태스크마다 새 ACP 세션**을 연다 — 그래서 이 mock도
//! `session/new`을 여러 번(태스크당 1회) 받을 준비가 되어 있어야 한다(옛
//! 구현은 워커당 세션 1개를 공유했다).

#![cfg(feature = "acp")]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
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

/// mock 서버가 세션마다 스트리밍할 텍스트. 세션 생성 순서대로 소비.
#[derive(Clone, Default)]
struct MockState {
    /// 수신한 모든 JSON-RPC 메시지 기록(검증용).
    received: Arc<Mutex<Vec<Value>>>,
    next_session_id: Arc<AtomicU64>,
    /// 세션마다 흘려보낼 텍스트 청크. 큐가 비면 빈 응답.
    scripted_chunks: Arc<Mutex<Vec<String>>>,
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
    use futures::{SinkExt, StreamExt};

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

        state.received.lock().await.push(req.clone());

        match method.as_str() {
            "initialize" => {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "protocolVersion": 1 },
                });
                let _ = writer.send(WsMessage::Text(resp.to_string())).await;
            }
            "session/new" => {
                let sid = state.next_session_id.fetch_add(1, Ordering::SeqCst);
                let session_id = format!("session-{sid}");
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "sessionId": session_id },
                });
                let _ = writer.send(WsMessage::Text(resp.to_string())).await;
            }
            "session/prompt" => {
                let session_id = req
                    .get("params")
                    .and_then(|p| p.get("sessionId"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let chunks: Vec<String> = {
                    let mut q = state.scripted_chunks.lock().await;
                    std::mem::take(&mut *q)
                };
                for chunk in &chunks {
                    let update = json!({
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": session_id,
                            "update": {
                                "sessionUpdate": "agent_message_chunk",
                                "content": { "type": "text", "text": chunk },
                            },
                        },
                    });
                    let _ = writer.send(WsMessage::Text(update.to_string())).await;
                }

                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "stopReason": "end_turn" },
                });
                let _ = writer.send(WsMessage::Text(resp.to_string())).await;
            }
            _ => {
                // session/cancel 등 notification(= id 없음)은 응답하지 않음.
                if id.is_some() {
                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": "method not found" },
                    });
                    let _ = writer.send(WsMessage::Text(resp.to_string())).await;
                }
            }
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

fn dispatch_req(
    task_id: TaskId,
    worker_id: WorkerId,
    prompt: &str,
) -> fleet_transport::DispatchRequest {
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
        .register(worker, &endpoint(addr), 1)
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
        .register(worker, &endpoint(addr), 1)
        .await
        .expect("register");

    let err = transport.register(worker, &endpoint(addr), 1).await;
    assert!(matches!(
        err,
        Err(fleet_transport::TransportError::AlreadyRegistered(_))
    ));
}

#[tokio::test]
async fn dispatch_unknown_worker_errors() {
    let transport = AcpTransport::new();
    let req = dispatch_req(TaskId::new(), WorkerId::new(), "x");
    let result = transport.dispatch(req).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn unregister_unknown_worker_errors() {
    let transport = AcpTransport::new();
    let result = transport.unregister(WorkerId::new()).await;
    assert!(matches!(
        result,
        Err(fleet_transport::TransportError::WorkerNotRegistered(_))
    ));
}

#[tokio::test]
async fn ping_registered_worker_ok() {
    let (_state, addr) = start_mock_server().await;
    let transport = AcpTransport::new();
    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint(addr), 1)
        .await
        .expect("register");
    assert!(transport.ping(worker).await.is_ok());
}

#[tokio::test]
async fn cancel_unknown_task_is_noop() {
    let transport = AcpTransport::new();
    assert!(transport.cancel(TaskId::new()).await.is_ok());
}

#[tokio::test]
async fn dispatch_streams_output_and_completes() {
    let (state, addr) = start_mock_server().await;
    *state.scripted_chunks.lock().await = vec!["Hello ".to_string(), "world".to_string()];

    let transport = Arc::new(AcpTransport::new());
    let mut events = transport.subscribe().await.expect("subscribe");

    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint(addr), 1)
        .await
        .expect("register");

    let task_id = TaskId::new();
    transport
        .dispatch(dispatch_req(task_id, worker, "hi"))
        .await
        .expect("dispatch");

    let mut output = String::new();
    let mut completed = false;
    let mut duration_secs = 0.0_f64;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Some(WorkerEvent::Output {
                task_id: t, chunk, ..
            })) => {
                assert_eq!(t, task_id);
                output.push_str(&chunk);
            }
            Ok(Some(WorkerEvent::Completed { task_id: t, result })) => {
                assert_eq!(t, task_id);
                completed = true;
                duration_secs = result.duration_secs;
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
    assert!(
        duration_secs >= 0.0,
        "duration_secs should be a real elapsed measurement, got {duration_secs}"
    );
}

#[tokio::test]
async fn multiple_workers_dispatched_independently() {
    let (state1, addr1) = start_mock_server().await;
    let (state2, addr2) = start_mock_server().await;
    *state1.scripted_chunks.lock().await = vec!["from-worker-1".to_string()];
    *state2.scripted_chunks.lock().await = vec!["from-worker-2".to_string()];

    let transport = Arc::new(AcpTransport::new());
    let mut events = transport.subscribe().await.expect("subscribe");

    let w1 = WorkerId::new();
    let w2 = WorkerId::new();
    transport.register(w1, &endpoint(addr1), 1).await.unwrap();
    transport.register(w2, &endpoint(addr2), 1).await.unwrap();

    let t1 = TaskId::new();
    let t2 = TaskId::new();
    transport.dispatch(dispatch_req(t1, w1, "a")).await.unwrap();
    transport.dispatch(dispatch_req(t2, w2, "b")).await.unwrap();

    let mut outputs: std::collections::HashMap<TaskId, String> = std::collections::HashMap::new();
    let mut completed: std::collections::HashSet<TaskId> = std::collections::HashSet::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while completed.len() < 2 && std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Some(WorkerEvent::Output { task_id, chunk, .. })) => {
                outputs.entry(task_id).or_default().push_str(&chunk);
            }
            Ok(Some(WorkerEvent::Completed { task_id, .. })) => {
                completed.insert(task_id);
            }
            _ => continue,
        }
    }
    assert_eq!(completed.len(), 2);
    assert_eq!(outputs.get(&t1).map(String::as_str), Some("from-worker-1"));
    assert_eq!(outputs.get(&t2).map(String::as_str), Some("from-worker-2"));
}

#[tokio::test]
async fn completed_task_receives_no_further_output_after_session_ends() {
    // 태스크마다 새 세션을 쓰므로, 첫 태스크가 끝난 뒤 같은 워커에 두 번째
    // 태스크를 보내도 서로의 출력이 섞이면 안 된다.
    let (state, addr) = start_mock_server().await;
    let transport = Arc::new(AcpTransport::new());
    let mut events = transport.subscribe().await.expect("subscribe");

    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint(addr), 2)
        .await
        .unwrap();

    *state.scripted_chunks.lock().await = vec!["first".to_string()];
    let t1 = TaskId::new();
    transport
        .dispatch(dispatch_req(t1, worker, "a"))
        .await
        .unwrap();

    let mut out1 = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Some(WorkerEvent::Output { task_id, chunk, .. })) if task_id == t1 => {
                out1.push_str(&chunk);
            }
            Ok(Some(WorkerEvent::Completed { task_id, .. })) if task_id == t1 => break,
            _ => continue,
        }
    }
    assert_eq!(out1, "first");

    *state.scripted_chunks.lock().await = vec!["second".to_string()];
    let t2 = TaskId::new();
    transport
        .dispatch(dispatch_req(t2, worker, "b"))
        .await
        .unwrap();

    let mut out2 = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Some(WorkerEvent::Output { task_id, chunk, .. })) if task_id == t2 => {
                out2.push_str(&chunk);
            }
            Ok(Some(WorkerEvent::Completed { task_id, .. })) if task_id == t2 => break,
            _ => continue,
        }
    }
    assert_eq!(
        out2, "second",
        "second task's output must not contain the first task's text"
    );
}
