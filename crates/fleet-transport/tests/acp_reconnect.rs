//! AcpTransport WebSocket 재연결 (Phase 8.2) 통합 테스트.
//!
//! 시나리오:
//! 1. mock ACP 서버 시작 + 워커 등록 (Connected).
//! 2. mock 서버가 WebSocket Close 프레임 전송 (네트워크 단절 시뮬레이션).
//! 3. reader 종료 → ConnState::Disconnected + in-flight task Failed 이벤트.
//! 4. 백오프 후 자동 재연결.
//! 5. dispatch가 다시 동작.
//!
//! 핵심: 서버 abort 대신 명시적 Close 프레임으로 WebSocket 단절을 시뮬레이션.
//! TCP keepalive timeout에 의존하지 않기 위해.

#![cfg(feature = "acp")]

use std::sync::atomic::{AtomicBool, Ordering};
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
    AcpTransport, ConnState, DispatchRequest, ReconnectConfig, WorkerEvent, WorkerTransport,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::timeout;

/// mock 서버 공유 상태.
#[derive(Clone)]
struct MockState {
    received: Arc<Mutex<Vec<Value>>>,
    next_prompt_id: Arc<Mutex<u64>>,
    /// true로 설정 시 다음 요청 처리 후 WebSocket Close 프레임 전송.
    close_after_next: Arc<AtomicBool>,
    /// prompt 처리를 차단할지 여부. true면 session/prompt 응답 없이 대기.
    block_prompt: Arc<AtomicBool>,
    /// 백그라운드 태스크가 폴링 — true로 설정 시 모든 활성 WebSocket을 즉시 닫음.
    /// Phase 8.4에서 추가: 디스패치 없이 close를 트리거하는 깔끔한 방법.
    close_now: Arc<AtomicBool>,
}

impl Default for MockState {
    fn default() -> Self {
        Self {
            received: Arc::new(Mutex::new(Vec::new())),
            next_prompt_id: Arc::new(Mutex::new(0)),
            close_after_next: Arc::new(AtomicBool::new(false)),
            block_prompt: Arc::new(AtomicBool::new(false)),
            close_now: Arc::new(AtomicBool::new(false)),
        }
    }
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

    loop {
        // close_now가 설정되었는지 폴링하면서 메시지를 동시에 기다림.
        // close_now가 설정되면 즉시 Close 프레임 전송 후 종료.
        let next_msg = tokio::select! {
            biased; // close_now를 우선 검사.
            _ = tokio::time::sleep(Duration::from_millis(10)) => {
                if state.close_now.load(Ordering::SeqCst) {
                    let _ = writer.close().await;
                    return;
                }
                continue;
            }
            msg = reader.next() => match msg {
                Some(Ok(WsMessage::Text(t))) => t,
                Some(Ok(WsMessage::Close(_))) | Some(Err(_)) | None => return,
                _ => continue,
            },
        };

        let req: Value = match serde_json::from_str(&next_msg) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let id = req.get("id").cloned();
        state.received.lock().await.push(req);

        let response: Option<Value> = match method.as_str() {
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
                "result": { "sessionId": "session-1" },
            })),
            "session/prompt" => {
                if state.block_prompt.load(Ordering::SeqCst) {
                    None // 응답 없이 대기 — reader 종료 테스트용.
                } else {
                    let prompt_id = {
                        let mut next = state.next_prompt_id.lock().await;
                        *next += 1;
                        *next
                    };
                    let update = json!({
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": "session-1",
                            "promptId": prompt_id,
                            "update": {
                                "type": "agent_message_chunk",
                                "content": {
                                    "agent_message": [{
                                        "type": "text",
                                        "text": "hello",
                                    }],
                                },
                            },
                        },
                    });
                    let _ = writer.send(WsMessage::Text(update.to_string())).await;
                    Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "prompt_id": prompt_id,
                            "agent_message": [{"type": "text", "text": "hello"}],
                            "end_of_turn": true,
                            "usage": {"input_tokens": 1, "output_tokens": 2},
                        },
                    }))
                }
            }
            "session/cancel" => Some(json!({"jsonrpc": "2.0", "id": id, "result": {}})),
            _ => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "not found"},
            })),
        };

        if let Some(resp) = response {
            let _ = writer.send(WsMessage::Text(resp.to_string())).await;
        }

        // close_after_next 플래그가 설정되었으면 Close 프레임 전송 후 종료.
        // writer.close()로 확실히 WebSocket을 닫는다 (Close 전송 + ack 대기).
        if state.close_after_next.load(Ordering::SeqCst) {
            let _ = writer.close().await;
            return;
        }
    }
}

fn endpoint(addr: &std::net::SocketAddr) -> String {
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

/// ConnState가 target이 될 때까지 폴링. timeout 초과 시 panic.
async fn wait_for_state(
    transport: &AcpTransport,
    worker: WorkerId,
    target: ConnState,
    label: &str,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let state = transport.conn_state(worker).await;
        if state == Some(target) {
            return;
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "wait_for_state({label}): expected {target:?}, got {state:?} within 10s"
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn connection_failure_during_register_returns_error() {
    // 아무것도 listen하지 않는 포트로 연결 시도.
    let transport = AcpTransport::with_reconnect(ReconnectConfig {
        initial: Duration::from_millis(10),
        max: Duration::from_millis(50),
    });
    let worker = WorkerId::new();
    // 127.0.0.1:9 — discard 포트.
    let result = transport
        .register(worker, "ws://127.0.0.1:9/ws?server-key=x", 1)
        .await;
    assert!(result.is_err(), "register should fail on initial connect");
    let err = result.unwrap_err();
    assert!(format!("{err}").contains("initial ACP connect failed"));
}

#[tokio::test]
async fn close_frame_marks_disconnected() {
    let state = MockState::default();
    let app = Router::new().route("/ws", get(ws_handler)).with_state(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let transport = AcpTransport::with_reconnect(ReconnectConfig {
        initial: Duration::from_millis(20),
        max: Duration::from_millis(100),
    });
    let worker = WorkerId::new();
    transport.register(worker, &endpoint(&addr), 1).await.expect("register");
    assert_eq!(transport.conn_state(worker).await, Some(ConnState::Connected));

    // close_after_next 설정 → 다음 요청 후 Close 전송.
    state.close_after_next.store(true, Ordering::SeqCst);

    // dispatch로 prompt 전송 → mock이 응답 후 close.
    let task = TaskId::new();
    transport.dispatch(dispatch_req(task, worker, "x")).await.unwrap();

    // Disconnected로 전환 대기.
    wait_for_state(&transport, worker, ConnState::Disconnected, "after close frame").await;

    // dispatch 실패해야 함.
    let task2 = TaskId::new();
    let result = transport.dispatch(dispatch_req(task2, worker, "y")).await;
    assert!(result.is_err(), "dispatch after disconnect should fail");

    transport.unregister(worker).await.unwrap();
}

#[tokio::test]
async fn reconnect_after_close_frame() {
    let state = MockState::default();
    let app = Router::new().route("/ws", get(ws_handler)).with_state(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let transport = AcpTransport::with_reconnect(ReconnectConfig {
        initial: Duration::from_millis(30),
        max: Duration::from_millis(200),
    });
    let worker = WorkerId::new();
    transport.register(worker, &endpoint(&addr), 1).await.expect("register");
    assert_eq!(transport.conn_state(worker).await, Some(ConnState::Connected));

    // close 트리거.
    state.close_after_next.store(true, Ordering::SeqCst);
    let t1 = TaskId::new();
    transport.dispatch(dispatch_req(t1, worker, "trigger")).await.unwrap();
    wait_for_state(&transport, worker, ConnState::Disconnected, "after close").await;

    // 플래그 리셋 — 재연결 후에는 close하지 않아야 함.
    state.close_after_next.store(false, Ordering::SeqCst);

    // 백오프 후 자동 재연결.
    wait_for_state(&transport, worker, ConnState::Connected, "after reconnect").await;

    // dispatch 동작 확인.
    let task = TaskId::new();
    transport.dispatch(dispatch_req(task, worker, "post-reconnect")).await.unwrap();

    let mut rx = transport.subscribe().await.unwrap();
    let mut got_completed = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(WorkerEvent::Completed { task_id, .. })) if task_id == task => {
                got_completed = true;
                break;
            }
            _ => continue,
        }
    }
    assert!(got_completed, "expected Completed after reconnect dispatch");

    transport.unregister(worker).await.unwrap();
}

#[tokio::test]
async fn failed_event_emitted_for_in_flight_task_on_close() {
    let state = MockState::default();
    let app = Router::new().route("/ws", get(ws_handler)).with_state(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let transport = AcpTransport::with_reconnect(ReconnectConfig {
        initial: Duration::from_secs(60), // 재연결 방지 — Failed emit에 집중.
        max: Duration::from_secs(120),
    });
    let worker = WorkerId::new();
    // max_concurrent=2 — 두 개의 동시 task를 in-flight로 두어
    // close 시 fail_all이 모두 처리하는지 검증.
    transport
        .register(worker, &endpoint(&addr), 2)
        .await
        .expect("register");

    let mut rx = transport.subscribe().await.unwrap();

    // block_prompt=true → 두 prompt 모두 서버 응답 없이 대기. in_flight 유지.
    state.block_prompt.store(true, Ordering::SeqCst);

    let task1 = TaskId::new();
    let task2 = TaskId::new();
    transport.dispatch(dispatch_req(task1, worker, "blocked-1")).await.unwrap();
    transport.dispatch(dispatch_req(task2, worker, "blocked-2")).await.unwrap();

    // 두 prompt 도달 대기.
    tokio::time::sleep(Duration::from_millis(300)).await;
    {
        let reqs = state.received.lock().await;
        let prompt_count = reqs
            .iter()
            .filter(|r| r.get("method").and_then(|v| v.as_str()) == Some("session/prompt"))
            .count();
        assert_eq!(prompt_count, 2, "expected both prompts to be received by mock");
    }

    // close_now로 WebSocket 즉시 종료 — 디스패치 추가 없이 close 트리거.
    state.close_now.store(true, Ordering::SeqCst);

    // Failed 이벤트 대기 — 두 task 모두 emit되어야 함 (Phase 8.4 fail_all).
    let mut got_failed_1 = false;
    let mut got_failed_2 = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(WorkerEvent::Failed { task_id, error })) => {
                assert!(
                    error.contains("reader exited") || error.contains("connection"),
                    "unexpected error: {error}"
                );
                if task_id == task1 {
                    got_failed_1 = true;
                } else if task_id == task2 {
                    got_failed_2 = true;
                } else {
                    panic!("Failed for unexpected task {task_id}");
                }
                if got_failed_1 && got_failed_2 {
                    break;
                }
            }
            _ => continue,
        }
    }
    assert!(got_failed_1, "expected Failed event for task1 on close");
    assert!(got_failed_2, "expected Failed event for task2 on close");

    wait_for_state(&transport, worker, ConnState::Disconnected, "after close").await;
    transport.unregister(worker).await.unwrap();
}

#[tokio::test]
async fn unregister_during_backoff_exits_cleanly() {
    let state = MockState::default();
    let app = Router::new().route("/ws", get(ws_handler)).with_state(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let transport = AcpTransport::with_reconnect(ReconnectConfig {
        initial: Duration::from_secs(60), // 긴 백오프 — unregister 테스트.
        max: Duration::from_secs(120),
    });
    let worker = WorkerId::new();
    transport.register(worker, &endpoint(&addr), 1).await.expect("register");

    // close 트리거.
    state.close_after_next.store(true, Ordering::SeqCst);
    let t = TaskId::new();
    transport.dispatch(dispatch_req(t, worker, "trigger")).await.unwrap();
    wait_for_state(&transport, worker, ConnState::Disconnected, "during backoff").await;

    // unregister가 빠르게 반환되어야 함.
    let start = std::time::Instant::now();
    transport.unregister(worker).await.expect("unregister");
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "unregister should return fast during backoff, took {elapsed:?}"
    );

    // unregister 후 None.
    assert_eq!(transport.conn_state(worker).await, None);
}

#[tokio::test]
async fn multiple_workers_reconnect_independently() {
    let state1 = MockState::default();
    let state2 = MockState::default();
    let app1 = Router::new().route("/ws", get(ws_handler)).with_state(state1.clone());
    let app2 = Router::new().route("/ws", get(ws_handler)).with_state(state2.clone());
    let l1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let l2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr1 = l1.local_addr().unwrap();
    let addr2 = l2.local_addr().unwrap();
    let _s1 = tokio::spawn(async move { let _ = axum::serve(l1, app1).await; });
    let _s2 = tokio::spawn(async move { let _ = axum::serve(l2, app2).await; });

    let transport = AcpTransport::with_reconnect(ReconnectConfig {
        initial: Duration::from_millis(30),
        max: Duration::from_millis(200),
    });
    let w1 = WorkerId::new();
    let w2 = WorkerId::new();
    transport.register(w1, &endpoint(&addr1), 1).await.unwrap();
    transport.register(w2, &endpoint(&addr2), 1).await.unwrap();
    assert_eq!(transport.conn_state(w1).await, Some(ConnState::Connected));
    assert_eq!(transport.conn_state(w2).await, Some(ConnState::Connected));

    // w1만 close.
    state1.close_after_next.store(true, Ordering::SeqCst);
    let t = TaskId::new();
    transport.dispatch(dispatch_req(t, w1, "trigger")).await.unwrap();
    wait_for_state(&transport, w1, ConnState::Disconnected, "w1 after close").await;

    // w2는 여전히 Connected.
    assert_eq!(transport.conn_state(w2).await, Some(ConnState::Connected));

    // w1 플래그 리셋 → 재연결 후 Connected 복구.
    state1.close_after_next.store(false, Ordering::SeqCst);
    wait_for_state(&transport, w1, ConnState::Connected, "w1 after reconnect").await;

    transport.unregister(w1).await.unwrap();
    transport.unregister(w2).await.unwrap();
}

#[tokio::test]
async fn exponential_backoff_increases_between_failures() {
    // 127.0.0.1:9에 연결 시도 — 즉시 refused.
    let transport = AcpTransport::with_reconnect(ReconnectConfig {
        initial: Duration::from_millis(50),
        max: Duration::from_millis(400),
    });
    let worker = WorkerId::new();
    let result = transport
        .register(worker, "ws://127.0.0.1:9/ws?server-key=x", 1)
        .await;
    assert!(result.is_err());

    // supervisor는 register() 실패 후에도 계속 재시도 중.
    // 600ms 대기 후 unregister — supervisor가 깔끔히 종료되는지 (교착 없음) 확인.
    tokio::time::sleep(Duration::from_millis(600)).await;

    // worker_id가 clients 맵에 없으므로 Err 반환 — 정상.
    let result = transport.unregister(worker).await;
    assert!(result.is_err(), "worker should not be in map after failed register");
}
