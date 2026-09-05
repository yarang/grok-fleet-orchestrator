//! `AcpTransport` end-to-end 테스트 (2026-08-11 SDK 전환 이후 재작성).
//!
//! mock ACP 서버(axum WebSocket)가 실제 grok과 동일한 wire-format으로 응답한다.
//! `AcpTransport`는 공식 `agent-client-protocol`/`agent-client-protocol-http`
//! SDK 위에서 동작하며, **태스크마다 새 ACP 세션**을 연다 — 그래서 이 mock도
//! `session/new`을 여러 번(태스크당 1회) 받을 준비가 되어 있어야 한다(옛
//! 구현은 워커당 세션 1개를 공유했다).

#![cfg(feature = "acp")]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
use fleet_transport::{AcpTransport, FailureObservation, WorkerEvent, WorkerTransport};
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
    /// 켜면 `session/prompt`에 **응답하지 않는다** — 클라이언트 쪽
    /// 타임아웃 경로를 재현하기 위한 스위치. 응답을 보내지 않을 뿐
    /// 리더 루프는 계속 돌아야 한다: 여기서 sleep이나 await로 멈추면
    /// 소켓을 드레인하는 주체가 사라져 뒤이어 오는 `session/cancel`을
    /// 아예 읽지 못하고, 그러면 이 스위치를 쓰는 테스트가 검증하려는
    /// 바로 그것이 보이지 않는다.
    stall_prompt: Arc<AtomicBool>,
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
                if state.stall_prompt.load(Ordering::SeqCst) {
                    continue;
                }
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
        // 로드맵 #69 — dispatch 게이트가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        model: None,
        max_turns: None,
        timeout_secs: Some(30),
        checkpoint_branch: None,
        skills_required: vec![],
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
    let first = transport.ping(worker).await.expect("ping");
    let second = transport.ping(worker).await.expect("ping");

    // **왕복이 아니라는 사실을 여기서 못박는다.** `ping`은 이름도
    // 반환형(`Duration`)도 probe처럼 보이지만 supervisor가 든 연결 상태를 읽고
    // 상수를 돌려줄 뿐이다. 그 사실이 중요한 이유는 `#70` 게이트 ⑤ 때문이다 —
    // 그 게이트는 on_demand 워커에 dispatch하기 전 살아 있음을 **확인**할 것을
    // 요구하는데, 연결만 서 있고 응답하지 않는 워커는 이 함수를 그대로
    // 통과한다. 이 단정이 있으면 누군가 진짜 왕복을 넣을 때 시험이 붉어지며
    // "그 변경이 게이트 ⑤의 probe를 만든 것인지"를 의도적으로 판단하게 된다.
    assert_eq!(
        first, second,
        "두 번의 ping이 같은 값이어야 한다 — 측정이라면 값이 흔들린다"
    );
    assert_eq!(
        first,
        std::time::Duration::from_millis(1),
        "성공 값은 측정이 아니라 상수다"
    );
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
            Ok(Some(WorkerEvent::Failed {
                task_id: t, error, ..
            })) => {
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

/// 인접 결함 2(#67) — `session/prompt`가 타임아웃하면 워커 쪽 실행은 그대로
/// 돌고 있는데 우리는 permit만 놓고 떠났다. 이제 떠나기 전에 워커에
/// `session/cancel`을 보낸다.
///
/// **두 가지를 한 테스트에서 함께 단정한다.** cancel이 실제로 워커에
/// 닿았다는 것과, 그럼에도 관측은 여전히 `ResultLost`라는 것. 나누어 두면
/// 나중에 읽는 사람이 "cancel을 보냈으니 결과가 확정됐다"로 읽을 수 있다 —
/// `session/cancel`은 ack 없는 notification이라 워커가 받았는지도, 받고
/// 멈췄는지도 알 수 없다. 초과 점유 창은 닫히는 게 아니라 좁아질 뿐이고,
/// 그 사실이 두 단정 사이에 있다.
#[tokio::test]
async fn prompt_timeout_cancels_the_session_but_still_reports_result_lost() {
    let (state, addr) = start_mock_server().await;
    state.stall_prompt.store(true, Ordering::SeqCst);

    let transport = Arc::new(AcpTransport::new());
    let mut events = transport.subscribe().await.expect("subscribe");

    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint(addr), 1)
        .await
        .expect("register");

    let task_id = TaskId::new();
    let mut req = dispatch_req(task_id, worker, "hi");
    // 기본값 30초는 테스트가 기다릴 수 있는 시간이 아니다.
    req.timeout_secs = Some(1);
    transport.dispatch(req).await.expect("dispatch");

    let mut observed: Option<FailureObservation> = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Some(WorkerEvent::Failed {
                task_id: t,
                error,
                observation,
            })) => {
                assert_eq!(t, task_id);
                assert!(
                    error.contains("timed out"),
                    "expected the prompt-timeout failure, got {error}"
                );
                observed = Some(observation);
                break;
            }
            Ok(Some(WorkerEvent::Completed { .. })) => {
                panic!("mock never answered session/prompt — Completed must not appear")
            }
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    // 관측값을 따지기 전에 프롬프트가 실제로 목까지 닿았는지부터 본다. `timeout_secs`는
    // `session/new`와 `session/prompt` 두 타이머에 같이 먹히므로, 러너가 굶주려
    // `session/new`가 1초를 넘기면 코드는 **다른 arm**(`NotDelivered`, cancel 없음)을
    // 탄다. 그때 먼저 깨지는 것이 아래 단정이면 실패 메시지가 "관측값이 틀렸다"를
    // 가리켜 엉뚱한 곳을 보게 만든다. 여기서 먼저 끊으면 "프롬프트가 목에 닿지도
    // 않았다"고 직접 말한다.
    let methods: Vec<String> = state
        .received
        .lock()
        .await
        .iter()
        .filter_map(|m| m.get("method").and_then(|v| v.as_str()).map(str::to_string))
        .collect();
    assert!(
        methods.iter().any(|m| m == "session/prompt"),
        "타임아웃이 프롬프트 경로에서 났어야 한다 — 받은 메시지: {methods:?}"
    );

    assert_eq!(
        observed,
        Some(FailureObservation::ResultLost),
        "프롬프트는 전달됐고 답만 오지 않았다 — cancel을 보냈다고 이것이 Reported나 NotDelivered가 되지는 않는다"
    );

    // cancel은 fire-and-forget이라 위 `Failed` 이벤트와 순서가 정해져 있지
    // 않다. 이벤트 직후의 스냅샷 단정은 그 자체로 flaky하므로 마감 시한을
    // 두고 폴링한다(§3.3).
    let mut cancelled = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let seen = state
            .received
            .lock()
            .await
            .iter()
            .any(|m| m.get("method").and_then(|v| v.as_str()) == Some("session/cancel"));
        if seen {
            cancelled = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        cancelled,
        "prompt 타임아웃 뒤 워커에 session/cancel이 도착해야 한다 — 받은 메시지: {:?}",
        state
            .received
            .lock()
            .await
            .iter()
            .filter_map(|m| m.get("method").and_then(|v| v.as_str()).map(str::to_string))
            .collect::<Vec<_>>()
    );
}
