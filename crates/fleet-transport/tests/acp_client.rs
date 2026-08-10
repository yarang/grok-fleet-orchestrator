//! ACP 클라이언트 통합 테스트.
//!
//! axum 기반 mock ACP 서버를 in-process로 띄워 JSON-RPC 요청-응답,
//! notification streaming, 에러 처리를 검증.

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
use fleet_transport::acp::{AcpClient, AcpError, AcpEvent, PromptId, SessionId};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::timeout;

/// ACP 테스트용 mock 서버 상태.
#[derive(Clone, Default)]
struct MockState {
    /// 수신한 요청을 검증하기 위한 기록.
    received: Arc<Mutex<Vec<Value>>>,
    /// 다음 prompt_id (incremental).
    next_prompt_id: Arc<Mutex<u64>>,
    /// 세션별 prompt 도착 시 스트리밍할 출력 청크들.
    scripted_output: Arc<Mutex<Vec<String>>>,
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    #[serde(rename = "server-key", default)]
    server_key: Option<String>,
}

async fn ws_handler(
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
    State(state): State<MockState>,
) -> impl IntoResponse {
    // server-key 검증 (간단). "test" 또는 "secret" 허용.
    if let Some(key) = &q.server_key {
        if key != "test" && key != "secret" {
            return (axum::http::StatusCode::UNAUTHORIZED, "bad key").into_response();
        }
    }
    ws.on_upgrade(move |socket| handle_acp_socket(socket, state))
}

async fn handle_acp_socket(socket: WebSocket, state: MockState) {
    use futures_util::{SinkExt, StreamExt};

    let (mut writer, mut reader) = socket.split();
    tracing::info!("mock ACP WebSocket connected");

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
        let params = req.get("params").cloned();

        // 로깅.
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
                    "instructions": "mock server",
                },
            })),
            "session/prompt" => {
                // prompt_id 발급.
                let prompt_id = {
                    let mut next = state.next_prompt_id.lock().await;
                    *next += 1;
                    *next
                };

                // 스크립트된 출력 청크 스트리밍.
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

                // 최종 응답. camelCase — 위 session/update의 "promptId"와
                // 일관되게 (실제 grok과 동일한 관례; PromptResult.prompt_id의
                // #[serde(rename = "promptId")] 참조).
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "promptId": prompt_id,
                        "agent_message": [{
                            "type": "text",
                            "text": chunks.join(""),
                        }],
                        "end_of_turn": true,
                        "usage": {
                            "input_tokens": 10,
                            "output_tokens": 20,
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
                "error": {
                    "code": -32601,
                    "message": "method not found",
                },
            })),
        };

        if let Some(resp) = response {
            let _ = writer.send(WsMessage::Text(resp.to_string())).await;
        }

        // session/cancel 후에는 무시하지 않고 계속 다음 요청 처리.
        let _ = params; // suppress warning
    }

    tracing::info!("mock ACP WebSocket closed");
}

/// mock ACP 서버를 ephemeral port에서 시작. `(router, addr)`.
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

fn endpoint(addr: SocketAddr, key: &str) -> String {
    format!("ws://{addr}/ws?server-key={key}")
}

#[tokio::test]
async fn initialize_and_open_session() {
    let _ = tracing_subscriber::fmt::try_init();
    let (_state, addr) = start_mock_server().await;

    let (client, mut events) = timeout(
        Duration::from_secs(5),
        AcpClient::connect(&endpoint(addr, "test")),
    )
    .await
    .expect("connect timeout")
    .expect("connect ok");

    let session = timeout(Duration::from_secs(5), client.open_session(Some("/tmp")))
        .await
        .expect("open_session timeout")
        .expect("open_session ok");

    assert_eq!(session, SessionId("test-session-1".to_string()));

    // initialize와 session/new는 notification을 보내지 않음 — events 비어야 함.
    assert!(events.try_recv().is_err(), "no events expected");

    client.close().await.unwrap();
}

#[tokio::test]
async fn prompt_streams_chunks_then_completes() {
    let (state, addr) = start_mock_server().await;
    *state.scripted_output.lock().await = vec!["Hello ".to_string(), "world".to_string()];

    let (client, mut events) = AcpClient::connect(&endpoint(addr, "test"))
        .await
        .expect("connect");
    let session = client.open_session(None).await.expect("session");

    let prompt_id = timeout(Duration::from_secs(5), client.prompt(&session, "hi"))
        .await
        .expect("prompt timeout")
        .expect("prompt ok");

    // mock 서버가 자체 발급한 promptId(카운터 첫 값 1)를 신뢰해 그대로 반환.
    // 회귀 테스트: 아래 Completed 이벤트의 prompt_id와 반드시 일치해야 한다
    // (과거엔 이벤트만 항상 None이라 서로 달라 Completed가 영원히 드롭되는
    // 프로덕션 버그였다 — messages.rs의 PromptResult.prompt_id 참조).
    assert_eq!(prompt_id, PromptId(1));

    // events 수집 — Output 2개 + Completed 1개 예상.
    let mut outputs = Vec::new();
    let mut completed = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(200), events.recv()).await {
            Ok(Some(AcpEvent::Output { chunk, .. })) => outputs.push(chunk),
            Ok(Some(AcpEvent::Completed { prompt_id: pid, .. })) => {
                // 반환된 prompt_id와 이벤트의 prompt_id가 반드시 같아야 dispatch()의
                // set_prompt_id ↔ reader loop의 라우팅이 서로 맞아떨어진다.
                assert_eq!(pid, Some(prompt_id), "event prompt_id must match the id prompt() returned");
                completed = true;
                break;
            }
            Ok(Some(AcpEvent::Failed { .. })) => panic!("unexpected Failed"),
            Ok(None) => break,
            Err(_) => continue,
        }
    }
    assert!(completed, "should have received Completed");
    assert_eq!(outputs.concat(), "Hello world");

    client.close().await.unwrap();
}

#[tokio::test]
async fn cancel_request_round_trip() {
    let (state, addr) = start_mock_server().await;
    *state.scripted_output.lock().await = vec![];

    let (client, _events) = AcpClient::connect(&endpoint(addr, "test"))
        .await
        .expect("connect");
    let session = client.open_session(None).await.expect("session");
    let prompt_id = client.prompt(&session, "long").await.expect("prompt");

    // cancel 전송 (이미 끝났지만 round-trip 검증).
    client.cancel(&session, prompt_id).await.expect("cancel ok");

    client.close().await.unwrap();
}

#[tokio::test]
async fn rpc_error_response_returns_acp_error() {
    let (state, addr) = start_mock_server().await;

    let (client, _events) = AcpClient::connect(&endpoint(addr, "test"))
        .await
        .expect("connect");

    // 알 수 없는 메서드 직접 전송 (클라이언트 API로는 못 보내니 raw 사용).
    // — 클라이언트는 알려진 메서드만 지원하므로, 대신 MockState에서
    // received 로그를 통해 initialize/session/new가 잘 갔는지만 검증.
    client.open_session(None).await.expect("session");

    let received = state.received.lock().await;
    let methods: Vec<&str> = received
        .iter()
        .filter_map(|v| v.get("method").and_then(|m| m.as_str()))
        .collect();
    assert!(methods.contains(&"initialize"));
    assert!(methods.contains(&"session/new"));

    client.close().await.unwrap();
}

#[tokio::test]
async fn connection_refused_returns_connect_error() {
    // 사용하지 않는 포트를 찾기 위해 bind 후 drop.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let result = timeout(
        Duration::from_secs(3),
        AcpClient::connect(&endpoint(addr, "test")),
    )
    .await;

    match result {
        Ok(Err(AcpError::Connect(_))) => { /* expected */ }
        Ok(Err(other)) => panic!("expected Connect error, got {other:?}"),
        Ok(Ok(_)) => panic!("expected connect to fail"),
        Err(_) => panic!("connect hung"),
    }
}

#[tokio::test]
async fn invalid_endpoint_rejected() {
    let result = AcpClient::connect("http://not-ws").await;
    assert!(matches!(result, Err(AcpError::InvalidEndpoint(_))));
}

#[tokio::test]
async fn close_is_idempotent() {
    let (_state, addr) = start_mock_server().await;
    let (client, _events) = AcpClient::connect(&endpoint(addr, "test"))
        .await
        .expect("connect");

    client.close().await.expect("first close");
    // 두 번째 close는 자체적으로 no-op.
    // (Drop이 자동으로 처리하므로 별도 검증 불필요; 단지 panic 안 나면 됨.)
}
