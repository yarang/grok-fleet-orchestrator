//! ACP (Agent Client Protocol) 클라이언트 에러 타입.

use std::io;

use thiserror::Error;

/// ACP 프로토콜 처리 중 발생하는 에러.
#[derive(Debug, Error)]
pub enum AcpError {
    /// WebSocket 연결 실패 (DNS, TLS, refused 등).
    #[error("websocket connect failed: {0}")]
    Connect(String),

    /// WebSocket I/O 에러 (프레임 읽기/쓰기, ping/pong).
    #[error("websocket io error: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),

    /// JSON 직렬화/역직렬화 실패.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// 서버가 JSON-RPC 에러 응답을 반환.
    #[error("rpc error {code}: {message}")]
    Rpc {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },

    /// 서버가 예상치 못한 id의 응답을 반환 (불일치).
    #[error("unexpected response id: got {got}, expected {expected}")]
    UnexpectedResponseId { got: u64, expected: u64 },

    /// 요청에 대한 응답이 타임아웃됨.
    #[error("request timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// WebSocket이 정상적으로 닫혔지만 응답을 기다리는 중이었음.
    #[error("connection closed while waiting for response to request {0}")]
    Closed(u64),

    /// 잘못된 endpoint URL.
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(String),

    /// URL에 `server-key`가 필요하지만 없음.
    #[error("endpoint URL missing required `server-key` query parameter")]
    MissingServerKey,

    /// `session/update` notification이 인식 불가능한 형식.
    #[error("malformed session update: {0}")]
    MalformedUpdate(String),

    /// 클라이언트가 이미 닫힘.
    #[error("client already closed")]
    AlreadyClosed,

    /// 기타 I/O 에러.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

impl AcpError {
    /// JSON-RPC error object에서 `AcpError::Rpc` 생성.
    pub fn rpc(code: i64, message: impl Into<String>, data: Option<serde_json::Value>) -> Self {
        Self::Rpc {
            code,
            message: message.into(),
            data,
        }
    }
}
