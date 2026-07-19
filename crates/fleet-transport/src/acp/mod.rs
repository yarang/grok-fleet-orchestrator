//! ACP (Agent Client Protocol) 클라이언트.
//!
//! WebSocket 기반 JSON-RPC 2.0 클라이언트. `grok agent serve`가 노출하는
//! `/ws` 엔드포인트와 통신.
//!
//! ## 아키텍처
//!
//! ```text
//! [AcpClient]
//!   │
//!   ├─ WsConn::connect() ─────────► tokio_tungstenite WebSocket
//!   │
//!   ├─ send_request() ──► pending request table (id → oneshot)
//!   │       ▲                              │
//!   │       │                              ▼
//!   │       │                  spawn(reader_loop)
//!   │       │                          │
//!   │       │   ┌──── text frame ───────┤
//!   │       │   ▼                       ▼
//!   │       │  response → resolve      notification → emit AcpEvent
//!   │       │  pending oneshot         to event channel
//!   │       │
//!   └─ open_session() / prompt() / cancel() / close()
//! ```
//!
//! ## 제한 (Phase 7 MVP)
//!
//! - 단일 WebSocket 연결 (재연결 없음, Phase 8 예정).
//! - 단일 세션 (`open_session` 최초 1회).
//! - 동시 prompt는 직렬 처리 (queue 미사용).

pub mod error;
pub mod messages;
pub mod transport;

pub use error::AcpError;
pub use messages::{PromptResult, TokenUsage};

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, warn};

use super::acp::messages::{
    build_initialize, build_session_cancel, build_session_new, build_session_prompt, RpcMessage,
    SessionUpdate, UpdateContent,
};
use super::acp::transport::{WsConn, WsStream};

/// 특수 RPC 에러 코드 — WebSocket 종료로 인해 pending 요청이 실패했음을 표시.
/// 표준 JSON-RPC 에러 코드 범위 (-32000 ~ -32099) 내에서 임의 선택.
/// 상위 dispatch 레이어가 이 코드를 보고 supervisor의 fail_all에 위임 가능.
pub const ACP_ERR_CONNECTION_CLOSED: i64 = -32001;

/// ACP 세션 식별자 (서버가 `session/new` 응답에서 발급).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// ACP 프롬프트 식별자 (서버가 `session/prompt` 응답에서 발급).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PromptId(pub u64);

/// ACP 서버 → 클라이언트 이벤트.
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// 에이전트 출력 청크 (스트리밍).
    Output {
        prompt_id: Option<PromptId>,
        seq: u64,
        chunk: String,
    },
    /// 프롬프트 완료.
    Completed {
        prompt_id: Option<PromptId>,
        result: PromptResult,
    },
    /// 프롬프트 실패.
    Failed {
        prompt_id: Option<PromptId>,
        error: String,
    },
}

/// 백그라운드 reader 태스크의 핸들.
struct ReaderHandle {
    join: JoinHandle<()>,
}

impl ReaderHandle {
    /// reader 종료 대기 (드롭 시 자동 detach).
    async fn join(self) {
        let _ = self.join.await;
    }
}

/// 클라이언트 공유 상태.
struct ClientInner {
    /// JSON-RPC id 생성기.
    next_id: AtomicU64,
    /// pending 요청: id → oneshot sender.
    /// reader task가 응답을 받으면 해당 sender를 꺼내서 resolve.
    pending: Mutex<HashMap<u64, oneshot::Sender<RpcMessage>>>,
    /// 모든 AcpEvent를 fan-out.
    /// `Option` + std Mutex — reader_loop가 종료될 때 take하여 채널을 닫음.
    /// 그러면 구독자 (event_rx)가 recv()에서 None을 받고 종료 가능.
    /// None이 된 이후의 send 호출은 no-op.
    event_tx: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedSender<AcpEvent>>>,
    /// 출력 시퀀스 번호 (Output 이벤트의 seq용).
    output_seq: AtomicU64,
    /// WebSocket writer.
    ws: WsConn,
    /// close 여부 (idempotency).
    closed: AtomicBool,
}

impl ClientInner {
    /// event_tx로 이벤트 전송. 채널이 닫혀 있으면 no-op.
    fn emit_event(&self, event: AcpEvent) {
        if let Some(tx) = self
            .event_tx
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().cloned())
        {
            let _ = tx.send(event);
        }
    }

    /// reader_loop 종료 시 호출 — event_tx를 take하여 채널 닫기.
    fn close_event_channel(&self) {
        if let Ok(mut guard) = self.event_tx.lock() {
            *guard = None;
        }
    }
}

/// ACP 클라이언트 핸들.
pub struct AcpClient {
    inner: Arc<ClientInner>,
    reader: Option<ReaderHandle>,
}

impl AcpClient {
    /// WebSocket 연결. `ws://host:port/ws?server-key=...` 형태의 endpoint.
    ///
    /// 반환: `(클라이언트, 이벤트 수신기)`. 이벤트 수신기는
    /// `Output` / `Completed` / `Failed` 이벤트를 수신.
    #[allow(clippy::result_large_err)] // AcpError::Ws 가 크지만 박스화는 추후 과제.
    pub async fn connect(
        endpoint: &str,
    ) -> Result<(Self, tokio::sync::mpsc::UnboundedReceiver<AcpEvent>), AcpError> {
        let (ws, reader) = WsConn::connect(endpoint).await?;
        Self::from_ws(ws, reader)
    }

    /// WebSocket 연결 (mTLS). `wss://host:port/ws?server-key=...` 형태의 endpoint
    /// 에 대해 사설 CA + 클라이언트 인증서로 핸드셰이크 (Phase 8.5).
    ///
    /// `tls` 구성이 신뢰하는 CA로 서버 인증서를 검증하고, `tls` 의 클라이언트
    /// 인증서로 자신을 증명. orchestrator→worker ACP 트래픽 보호용.
    #[cfg(feature = "mtls")]
    #[allow(clippy::result_large_err)] // AcpError::Ws 가 크지만 박스화는 추후 과제.
    pub async fn connect_mtls(
        endpoint: &str,
        tls: &crate::tls::ClientTlsConfig,
    ) -> Result<(Self, tokio::sync::mpsc::UnboundedReceiver<AcpEvent>), AcpError> {
        let (ws, reader) = WsConn::connect_mtls(endpoint, tls).await?;
        Self::from_ws(ws, reader)
    }

    /// 공통 초기화 로직. WebSocket 연결이 완료된 후 reader 태스크를 spawn.
    #[allow(clippy::result_large_err)] // AcpError::Ws 가 크지만 박스화는 추후 과제.
    fn from_ws(
        ws: WsConn,
        reader: WsStream,
    ) -> Result<(Self, tokio::sync::mpsc::UnboundedReceiver<AcpEvent>), AcpError> {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();

        let inner = Arc::new(ClientInner {
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            event_tx: std::sync::Mutex::new(Some(event_tx)),
            output_seq: AtomicU64::new(0),
            ws,
            closed: AtomicBool::new(false),
        });

        // reader 태스크 spawn.
        let inner_for_reader = inner.clone();
        let join = tokio::spawn(async move {
            reader_loop(reader, inner_for_reader).await;
        });

        Ok((
            Self {
                inner,
                reader: Some(ReaderHandle { join }),
            },
            event_rx,
        ))
    }

    /// `initialize` + `session/new` 수행. 세션 ID 반환.
    ///
    /// `cwd=None`이면 서버 기본 워킹 디렉토리 사용.
    pub async fn open_session(&self, cwd: Option<&str>) -> Result<SessionId, AcpError> {
        // 1. initialize (capabilities 교환). 응답 본문은 로깅만.
        let init_resp = self.send_request(build_initialize(self.alloc_id())).await?;
        debug!(protocol_version = ?init_resp.result, "ACP initialize ok");

        // 2. session/new
        let resp = self
            .send_request(build_session_new(self.alloc_id(), cwd))
            .await?;
        let result = resp.result.ok_or_else(|| {
            AcpError::MalformedUpdate("session/new returned no result".to_string())
        })?;
        let new_result: messages::SessionNewResult = serde_json::from_value(result)?;
        Ok(SessionId(new_result.sessionId))
    }

    /// `session/prompt`. 완료를 기다리고, 스트리밍 Output 이벤트를 emit.
    ///
    /// 이 메서드는 서버 응답(prompt_id + end_of_turn=true)을 받을 때까지
    /// 블록. 도중에 도착하는 `session/update` notification은
    /// `Output` 이벤트로 subscriber에게 전달됨.
    ///
    /// 응답 도착 후, 메서드는 자체적으로 `Completed` 이벤트를 emit하고
    /// `PromptId`를 반환.
    pub async fn prompt(
        &self,
        session: &SessionId,
        prompt: &str,
    ) -> Result<PromptId, AcpError> {
        let req = build_session_prompt(self.alloc_id(), session.as_str(), prompt);
        let id = req.id.expect("prompt request must have id");
        let resp = self.send_request(req).await?;

        let result_value = resp.result.ok_or_else(|| {
            AcpError::MalformedUpdate("session/prompt returned no result".to_string())
        })?;
        let result: PromptResult = serde_json::from_value(result_value)?;
        let prompt_id = result.prompt_id.map(PromptId);

        // Completed 이벤트 emit (이 시점까지 도착한 session/update 외의
        // 완료 신호 — end_of_turn=true인 경우에는 ACP가 update로도 end_of_turn을
        // 보냈을 수 있지만, 안전하게 한 번 더 emit하여 구독자가 결과를 받도록).
        self.inner.emit_event(AcpEvent::Completed {
            prompt_id,
            result,
        });

        Ok(prompt_id.unwrap_or(PromptId(id)))
    }

    /// `session/cancel`. 진행 중인 프롬프트 취소.
    pub async fn cancel(&self, session: &SessionId, prompt_id: PromptId) -> Result<(), AcpError> {
        let req = build_session_cancel(self.alloc_id(), session.as_str(), prompt_id.0);
        let resp = self.send_request(req).await?;
        if resp.error.is_some() {
            // 에러는 send_request에서 이미 AcpError로 변환되었을 것.
            return Err(AcpError::MalformedUpdate(
                "cancel returned unexpected response".to_string(),
            ));
        }
        Ok(())
    }

    /// WebSocket 종료. idempotent.
    pub async fn close(mut self) -> Result<(), AcpError> {
        if self.inner.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        // 정상 close.
        let _ = self.inner.ws.close().await;
        // reader 태스크 종료 대기 (WebSocket 닫히면 EOF로 종료됨).
        if let Some(reader) = self.reader.take() {
            // 짧은 타임아웃으로 종료 대기; 그래도 끝나지 않으면 detach.
            if let Ok(()) = tokio::time::timeout(Duration::from_millis(500), reader.join()).await {
                debug!("ACP reader task joined cleanly");
            } else {
                warn!("ACP reader task did not shut down within 500ms — detaching");
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // internals
    // ------------------------------------------------------------------

    fn alloc_id(&self) -> u64 {
        self.inner.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// 요청을 보내고 응답을 대기.
    async fn send_request(
        &self,
        req: messages::RpcRequest,
    ) -> Result<RpcMessage, AcpError> {
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(AcpError::AlreadyClosed);
        }
        let id = req.id.expect("send_request requires an id");

        // pending 테이블에 oneshot 등록 (writer가 실패하기 전에).
        let (tx, rx) = oneshot::channel::<RpcMessage>();
        self.inner.pending.lock().await.insert(id, tx);

        // 직렬화하여 전송.
        let json = serde_json::to_string(&req)?;
        if let Err(e) = self.inner.ws.send_text(&json).await {
            // 전송 실패 — pending 정리.
            self.inner.pending.lock().await.remove(&id);
            return Err(e);
        }

        // 응답 대기 (10분 기본 타임아웃; 실제 worker timeout은 dispatch 레벨에서 적용).
        match tokio::time::timeout(Duration::from_secs(600), rx).await {
            Ok(Ok(msg)) => {
                if let Some(err) = msg.error {
                    Err(AcpError::rpc(err.code, err.message, err.data))
                } else {
                    Ok(msg)
                }
            }
            Ok(Err(_)) => Err(AcpError::Closed(id)),
            Err(_) => {
                // 타임아웃 — pending 정리.
                self.inner.pending.lock().await.remove(&id);
                Err(AcpError::Timeout(Duration::from_secs(600)))
            }
        }
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        // close()가 호출되지 않은 경우 reader는 여전히 실행 중.
        // WebSocket이 drop되면 자동으로 닫힘. reader는 EOF를 받고 종료.
        if let Some(reader) = self.reader.take() {
            reader.join.abort();
        }
    }
}

/// 백그라운드 reader 루프. WebSocket 프레임을 읽고:
/// - JSON-RPC response → pending oneshot resolve
/// - JSON-RPC notification (`session/update`) → AcpEvent emit
async fn reader_loop(mut reader: WsStream, inner: Arc<ClientInner>) {
    while let Some(msg_result) = reader.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "ACP WebSocket read error");
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                if let Err(e) = handle_text_frame(&text, &inner).await {
                    warn!(error = %e, "ACP message handling error");
                }
            }
            Message::Ping(payload) => {
                debug!(?payload, "ACP ping received");
                // tokio-tungstenite가 자동으로 pong 응답.
            }
            Message::Pong(_) | Message::Binary(_) => {
                // 무시.
            }
            Message::Close(reason) => {
                debug!(?reason, "ACP WebSocket closed by server");
                break;
            }
            Message::Frame(_) => {
                // raw frame — 무시.
            }
        }
    }

    // WebSocket 종료 시 모든 pending 요청을 실패 처리.
    // 특수 에러 코드 -32001 ("connection closed")로 표시하여
    // 호출자 (AcpClient::send_request)가 일반적인 RPC 에러와 구분 가능.
    // 상위 dispatch 레이어는 이 에러를 보고 supervisor의 fail_all에
    // 위임할지 결정함.
    let pending: Vec<(u64, oneshot::Sender<RpcMessage>)> = {
        let mut p = inner.pending.lock().await;
        p.drain().collect()
    };
    if !pending.is_empty() {
        debug!(count = pending.len(), "failing pending ACP requests on close");
    }
    for (id, sender) in pending {
        let _ = sender.send(RpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: None,
            result: None,
            error: Some(messages::RpcError {
                code: ACP_ERR_CONNECTION_CLOSED,
                message: "ACP connection closed".to_string(),
                data: None,
            }),
            params: None,
        });
    }

    // event_tx를 take하여 채널 닫기 — 구독자가 recv()에서 None을 받아 종료.
    // supervisor (acp_transport.rs)가 이를 감지하여 재연결 루프로 진입.
    inner.close_event_channel();
    debug!("ACP inner reader_loop fully terminated — event channel closed");
}

/// 단일 text 프레임 처리.
async fn handle_text_frame(text: &str, inner: &Arc<ClientInner>) -> Result<(), AcpError> {
    let msg: RpcMessage = serde_json::from_str(text)?;

    if msg.is_response() {
        // pending oneshot resolve.
        if let Some(id) = msg.id {
            let sender_opt = inner.pending.lock().await.remove(&id);
            if let Some(sender) = sender_opt {
                if sender.send(msg).is_err() {
                    warn!(id, "ACP response arrived but request was cancelled");
                }
            } else {
                warn!(id, "ACP response with no pending request");
            }
        }
        return Ok(());
    }

    if msg.is_notification() {
        if msg.method.as_deref() == Some("session/update") {
            if let Some(params) = msg.params {
                handle_session_update(params, inner);
            }
        } else {
            // 다른 notification은 무시 (로깅만).
            debug!(method = ?msg.method, "ignoring ACP notification");
        }
        return Ok(());
    }

    // 알 수 없는 메시지 형식.
    debug!(?msg, "unrecognized ACP message");
    Ok(())
}

/// `session/update` notification 처리.
///
/// ACP 스펙에서 update는 단일 update 객체이거나 update 배열일 수 있음.
/// 두 형태 모두 처리.
fn handle_session_update(params: serde_json::Value, inner: &Arc<ClientInner>) {
    // 1. 배열인 경우: 각 원소 개별 처리.
    if let Some(arr) = params.as_array() {
        for item in arr {
            handle_single_update(item.clone(), inner);
        }
        return;
    }
    // 2. 단일 객체.
    handle_single_update(params, inner);
}

fn handle_single_update(value: serde_json::Value, inner: &Arc<ClientInner>) {
    let update: SessionUpdate = match serde_json::from_value(value) {
        Ok(u) => u,
        Err(e) => {
            warn!(error = %e, "failed to parse session/update");
            return;
        }
    };

    let prompt_id = update.promptId.map(PromptId);

    match update.update {
        UpdateContent::AgentMessageChunk { content } => {
            if let Some(text) = content.extract_text() {
                let seq = inner.output_seq.fetch_add(1, Ordering::Relaxed);
                inner.emit_event(AcpEvent::Output {
                    prompt_id,
                    seq,
                    chunk: text,
                });
            }
        }
        UpdateContent::EndOfTurn => {
            // end_of_turn notification만으로 Completed를 emit하면 안 됨 —
            // PromptResult에 필요한 정보(usage 등)가 없음.
            // prompt() 메서드가 응답을 받아 Completed를 emit.
            debug!(?prompt_id, "ACP end_of_turn notification");
        }
        UpdateContent::Error { content } => {
            inner.emit_event(AcpEvent::Failed {
                prompt_id,
                error: content.message,
            });
        }
        UpdateContent::Unknown => {
            // 무시.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_display() {
        let id = SessionId("abc-123".to_string());
        assert_eq!(id.to_string(), "abc-123");
        assert_eq!(id.as_str(), "abc-123");
    }

    #[test]
    fn prompt_id_copy() {
        let a = PromptId(42);
        let b = a;
        assert_eq!(a, b);
    }
}
