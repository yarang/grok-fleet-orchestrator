//! WebSocket I/O 래퍼. `tokio-tungstenite` 연결을 캡슐화.
//!
//! text 프레임 송수신에 집중하고, JSON-RPC 파싱은 [`crate::acp`]가 담당.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{handshake::client::generate_key, http::Request, Message},
    MaybeTlsStream, WebSocketStream,
};

use super::error::AcpError;

/// WebSocket writer (SplitSink).
pub type WsSink =
    futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, Message>;
/// WebSocket reader (SplitStream).
pub type WsStream =
    futures_util::stream::SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>;

/// WebSocket 연결 핸들. writer는 mutex로 보호, reader는 분리해서 백그라운드 태스크로.
pub struct WsConn {
    pub writer: Arc<Mutex<WsSink>>,
}

impl WsConn {
    /// `ws://` 또는 `wss://` URL에 연결.
    /// 요청에는 `Sec-WebSocket-Protocol` 등 ACP에 필요한 헤더는 포함하지 않음
    /// (grok agent serve는 추가 헤더 없이도 `?server-key=` 쿼리 파라미터로 인증).
    pub async fn connect(url: &str) -> Result<(Self, WsStream), AcpError> {
        // URL 유효성 검사 (간단히 scheme 확인).
        if !url.starts_with("ws://") && !url.starts_with("wss://") {
            return Err(AcpError::InvalidEndpoint(format!(
                "endpoint must be ws:// or wss:// URL: {url}"
            )));
        }

        let req = Request::builder()
            .method("GET")
            .uri(url)
            .header("Host", host_of(url).unwrap_or_else(|| "localhost".into()))
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key())
            // server-key는 URL 쿼리 파라미터로 전달됨. 별도 subprotocol 없음.
            .body(())
            .map_err(|e| AcpError::Connect(format!("invalid request: {e}")))?;

        tracing::debug!(url = %sanitize_url(url), "connecting to ACP WebSocket");

        let (ws_stream, response) = connect_async(req)
            .await
            .map_err(|e| AcpError::Connect(format!("{e}")))?;

        tracing::debug!(status = ?response.status(), "ACP WebSocket connected");

        let (writer, reader) = ws_stream.split();
        Ok((
            Self {
                writer: Arc::new(Mutex::new(writer)),
            },
            reader,
        ))
    }

    /// JSON-RPC 요청 (raw JSON)을 text 프레임으로 전송.
    pub async fn send_text(&self, json: &str) -> Result<(), AcpError> {
        let mut writer = self.writer.lock().await;
        writer.send(Message::Text(json.to_string())).await?;
        Ok(())
    }

    /// 정상 종료 (Close 프레임 전송).
    pub async fn close(&self) -> Result<(), AcpError> {
        let mut writer = self.writer.lock().await;
        writer.send(Message::Close(None)).await?;
        Ok(())
    }
}

/// URL에서 host 부분 추출 (간단한 파서).
fn host_of(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let host_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    Some(after_scheme[..host_end].to_string())
}

/// URL에서 `server-key=...` 값을 마스킹 (로깅용).
fn sanitize_url(url: &str) -> String {
    if let Some(idx) = url.find("server-key=") {
        let start = idx + "server-key=".len();
        let end = url[start..]
            .find(['&', '#'])
            .map(|e| start + e)
            .unwrap_or(url.len());
        format!("{}<redacted>{}", &url[..start], &url[end..])
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_of_extracts_authority() {
        assert_eq!(host_of("ws://localhost:2419/ws").as_deref(), Some("localhost:2419"));
        assert_eq!(
            host_of("wss://worker.example.com/path?x=1").as_deref(),
            Some("worker.example.com")
        );
        assert_eq!(host_of("http://nope").as_deref(), Some("nope"));
        assert_eq!(host_of("not-a-url"), None);
    }

    #[test]
    fn sanitize_url_masks_server_key() {
        let out = sanitize_url("ws://h:1/ws?server-key=topsecret&other=1");
        assert!(!out.contains("topsecret"));
        assert!(out.contains("<redacted>"));
        assert!(out.contains("other=1"));
    }

    #[test]
    fn sanitize_url_without_key_unchanged() {
        let url = "ws://h:1/ws";
        assert_eq!(sanitize_url(url), url);
    }
}
