//! WebSocket I/O 래퍼. `tokio-tungstenite` 연결을 캡슐화.
//!
//! text 프레임 송수신에 집중하고, JSON-RPC 파싱은 [`crate::acp`]가 담당.
//!
//! ## TLS 모드 (Phase 8.5)
//!
//! - `ws://` — 일반 TCP. `connect(url)` 로 호출.
//! - `wss://` (공용 CA) — tokio-tungstenite 기본 rustls + webpki-roots 사용.
//!   `connect(url)` 로 호출.
//! - `wss://` (사설 mTLS) — `connect_mtls(url, &ClientTlsConfig)` 로 호출.
//!   사설 CA + 클라이언트 인증서로 풀 핸드셰이크. orchestrator↔worker ACP 트래픽
//!   보호용.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        handshake::client::generate_key,
        http::Request,
        protocol::Message,
    },
    Connector, MaybeTlsStream, WebSocketStream,
};

use super::error::AcpError;

#[cfg(feature = "mtls")]
use crate::tls::ClientTlsConfig;

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
    ///
    /// wss:// 인 경우 tokio-tungstenite의 기본 rustls connector (webpki-roots)를
    /// 사용. 사설 mTLS가 필요하면 [`Self::connect_mtls`] 사용.
    pub async fn connect(url: &str) -> Result<(Self, WsStream), AcpError> {
        let req = build_request(url)?;
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

    /// 사설 mTLS 로 `wss://` URL에 연결 (Phase 8.5).
    ///
    /// `tls` 구성이 제공하는 CA만 신뢰하며, 클라이언트 인증서로 자신을 증명.
    /// orchestrator→worker ACP 트래픽이 중간자 공격이나 스니핑으로부터 보호됨.
    #[cfg(feature = "mtls")]
    pub async fn connect_mtls(url: &str, tls: &ClientTlsConfig) -> Result<(Self, WsStream), AcpError> {
        if !url.starts_with("wss://") {
            return Err(AcpError::InvalidEndpoint(format!(
                "connect_mtls requires wss:// URL: {url}"
            )));
        }
        let client_config = tls
            .build_client_config()
            .map_err(|e| AcpError::Connect(format!("mTLS config: {e}")))?;
        let connector = Connector::Rustls(Arc::new(client_config));

        let req = build_request(url)?;
        tracing::debug!(url = %sanitize_url(url), "connecting to ACP WebSocket via mTLS");

        let (ws_stream, response) =
            tokio_tungstenite::connect_async_tls_with_config(req, None, false, Some(connector))
                .await
                .map_err(|e| AcpError::Connect(format!("mTLS handshake: {e}")))?;

        tracing::debug!(status = ?response.status(), "ACP WebSocket (mTLS) connected");
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

/// WebSocket 핸드셰이크용 HTTP 요청 빌드. 공용 헤더만 설정.
#[allow(clippy::result_large_err)] // AcpError 는 Ws variant 로 크지만 박스화는 추후 과제.
fn build_request(url: &str) -> Result<Request<()>, AcpError> {
    if !url.starts_with("ws://") && !url.starts_with("wss://") {
        return Err(AcpError::InvalidEndpoint(format!(
            "endpoint must be ws:// or wss:// URL: {url}"
        )));
    }
    let mut req = url
        .into_client_request()
        .map_err(|e| AcpError::Connect(format!("invalid request: {e}")))?;
    // Host 와 Sec-WebSocket-* 헤더는 into_client_request 가 자동 설정하지만,
    // 기존 동작 (명시적 Host) 을 보존하기 위해 추가 설정.
    let host = host_of(url).unwrap_or_else(|| "localhost".into());
    req.headers_mut().insert(
        "Host",
        host.parse()
            .map_err(|e| AcpError::Connect(format!("invalid host: {e}")))?,
    );
    req.headers_mut().insert(
        "Connection",
        "Upgrade"
            .parse()
            .map_err(|e| AcpError::Connect(format!("invalid header: {e}")))?,
    );
    req.headers_mut().insert(
        "Upgrade",
        "websocket"
            .parse()
            .map_err(|e| AcpError::Connect(format!("invalid header: {e}")))?,
    );
    req.headers_mut().insert(
        "Sec-WebSocket-Version",
        "13".parse()
            .map_err(|e| AcpError::Connect(format!("invalid header: {e}")))?,
    );
    req.headers_mut().insert(
        "Sec-WebSocket-Key",
        generate_key()
            .parse()
            .map_err(|e| AcpError::Connect(format!("invalid header: {e}")))?,
    );
    Ok(req)
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
