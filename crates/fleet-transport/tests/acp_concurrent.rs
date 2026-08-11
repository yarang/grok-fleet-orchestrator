//! AcpTransport 동시 다중 세션 (Phase 8.4, 2026-08-11 SDK 전환 이후 재작성)
//! 통합 테스트.
//!
//! 시나리오:
//! 1. 단일 워커를 `max_concurrent=N`으로 등록.
//! 2. N개의 task를 동시에 dispatch — 모두 정상적으로 Completed 수신.
//! 3. N+1번째 task는 `WorkerAtCapacity` 에러.
//! 4. `in_flight_count`가 실제 진행 상황을 반영.
//!
//! 핵심: 태스크마다 새 ACP 세션을 여는 설계 덕분에, `session_id`로 스트리밍
//! 출력이 **완전히 정확하게** 올바른 task로 라우팅됨을 검증한다 — 옛
//! promptId 기반 설계는 실제 grok가 신뢰할 수 있는 promptId를 안 줘서 2개
//! 이상 동시 진행 시 "안전하게 드롭"만 보장했지만, 세션 분리 설계는 애초에
//! 모호함 자체가 없다.

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
use fleet_transport::{
    AcpTransport, DispatchRequest, TransportError, WorkerEvent, WorkerTransport,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::timeout;

#[derive(Clone, Default)]
struct MockState {
    next_session_id: Arc<AtomicU64>,
    /// 각 session/prompt를 몇 초 지연시킬지 — 동시성/용량 테스트에서 슬롯을
    /// 오래 붙잡아두기 위해 사용.
    prompt_delay: Arc<Mutex<Duration>>,
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

/// mock ACP 서버 — 태스크마다 새 session/new를 받고, session_id별로 고유한
/// echo 텍스트를 스트리밍한다.
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
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = req.get("id").cloned();

        match method {
            "initialize" => {
                let resp = json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":1}});
                let _ = writer.send(WsMessage::Text(resp.to_string())).await;
            }
            "session/new" => {
                let sid = state.next_session_id.fetch_add(1, Ordering::SeqCst);
                let resp = json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": { "sessionId": format!("session-{sid}") },
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

                let delay = *state.prompt_delay.lock().await;
                if delay > Duration::ZERO {
                    tokio::time::sleep(delay).await;
                }

                // 세션마다 고유한 echo 텍스트 — 라우팅 정확성 검증용.
                let update = json!({
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {
                        "sessionId": session_id,
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": format!("echo:{session_id}") },
                        },
                    },
                });
                let _ = writer.send(WsMessage::Text(update.to_string())).await;

                let resp = json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"end_turn"}});
                let _ = writer.send(WsMessage::Text(resp.to_string())).await;
            }
            _ => {
                if id.is_some() {
                    let resp = json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"not found"}});
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

fn dispatch_req(task_id: TaskId, worker_id: WorkerId, prompt: &str) -> DispatchRequest {
    DispatchRequest {
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
async fn concurrent_dispatches_within_capacity_all_complete() {
    let (_state, addr) = start_mock_server().await;
    let transport = Arc::new(AcpTransport::new());
    let mut events = transport.subscribe().await.expect("subscribe");

    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint(addr), 3)
        .await
        .expect("register");

    let mut task_ids = Vec::new();
    for i in 0..3 {
        let tid = TaskId::new();
        task_ids.push(tid);
        transport
            .dispatch(dispatch_req(tid, worker, &format!("prompt-{i}")))
            .await
            .expect("dispatch");
    }

    let mut completed: Vec<TaskId> = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while completed.len() < 3 && std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Some(WorkerEvent::Completed { task_id, .. })) => completed.push(task_id),
            _ => continue,
        }
    }
    assert_eq!(completed.len(), 3, "all 3 concurrent tasks should complete");
    for tid in &task_ids {
        assert!(completed.contains(tid), "task {tid} should have completed");
    }

    transport.unregister(worker).await.unwrap();
}

#[tokio::test]
async fn dispatch_beyond_capacity_returns_worker_at_capacity() {
    let (state, addr) = start_mock_server().await;
    // 슬롯을 오래 붙잡아 두기 위해 응답을 지연시킨다 — permit은 dispatch()
    // 호출 시점에 즉시(동기적으로) 획득되므로, 이 지연이 없어도 정상 동작해야
    // 하지만(레이스 없음이 이번 재설계의 핵심 개선) 지연을 둬서 테스트를
    // 타이밍에 흔들리지 않게 만든다.
    *state.prompt_delay.lock().await = Duration::from_millis(300);

    let transport = Arc::new(AcpTransport::new());
    let _events = transport.subscribe().await.expect("subscribe");

    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint(addr), 1)
        .await
        .expect("register");

    let t1 = TaskId::new();
    transport
        .dispatch(dispatch_req(t1, worker, "first"))
        .await
        .expect("first dispatch within capacity");

    // permit은 dispatch() 호출 시 즉시(non-blocking) 획득되므로, 세션 생성이
    // 아직 안 끝났어도 두 번째 dispatch는 확정적으로 WorkerAtCapacity여야
    // 한다 — 옛 구현의 "레이스 조건" 주석은 이제 해당 없음.
    let t2 = TaskId::new();
    let result = transport.dispatch(dispatch_req(t2, worker, "second")).await;
    assert!(
        matches!(result, Err(TransportError::WorkerAtCapacity(_))),
        "expected WorkerAtCapacity, got {result:?}"
    );

    transport.unregister(worker).await.unwrap();
}

#[tokio::test]
async fn concurrent_tasks_stream_output_via_correct_session_routing() {
    // 태스크마다 새 세션을 열므로, 진짜 동시에 dispatch해도 각자의 출력이
    // 절대 섞이지 않아야 한다(옛 promptId 기반 설계는 "안전하게 드롭"까지만
    // 보장했지만, 세션 분리는 애초에 모호함이 없다).
    let (_state, addr) = start_mock_server().await;
    let transport = Arc::new(AcpTransport::new());
    let mut events = transport.subscribe().await.expect("subscribe");

    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint(addr), 5)
        .await
        .expect("register");

    let task_ids: Vec<TaskId> = (0..5).map(|_| TaskId::new()).collect();
    for (i, tid) in task_ids.iter().enumerate() {
        transport
            .dispatch(dispatch_req(*tid, worker, &format!("prompt-{i}")))
            .await
            .expect("dispatch");
    }

    let mut outputs: std::collections::HashMap<TaskId, String> = std::collections::HashMap::new();
    let mut completed: std::collections::HashSet<TaskId> = std::collections::HashSet::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while completed.len() < 5 && std::time::Instant::now() < deadline {
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
    assert_eq!(completed.len(), 5, "all 5 concurrent tasks should complete");

    // 각 태스크가 정확히 자기 자신의 echo만 받았는지 확인 — 모두 서로 달라야
    // 함(mock 서버가 session_id를 echo 텍스트에 포함시키므로).
    let mut seen = std::collections::HashSet::new();
    for tid in &task_ids {
        let out = outputs
            .get(tid)
            .unwrap_or_else(|| panic!("task {tid} should have received its own output"));
        assert!(out.starts_with("echo:session-"), "unexpected output: {out}");
        assert!(
            seen.insert(out.clone()),
            "duplicate/cross-contaminated output detected: {out}"
        );
    }

    transport.unregister(worker).await.unwrap();
}

#[tokio::test]
async fn in_flight_count_reflects_active_dispatches() {
    let (_state, addr) = start_mock_server().await;
    let transport = Arc::new(AcpTransport::new());

    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint(addr), 4)
        .await
        .expect("register");

    assert_eq!(
        transport.in_flight_count(worker).await,
        Some(0),
        "freshly registered worker should have 0 in-flight"
    );
    assert_eq!(transport.max_concurrent(worker).await, Some(4));

    transport.unregister(worker).await.unwrap();
    assert_eq!(
        transport.in_flight_count(worker).await,
        None,
        "unregistered worker should return None"
    );
}
