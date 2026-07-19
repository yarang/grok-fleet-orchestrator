//! mTLS 종단 TCP proxy (Phase 8.5.2).
//!
//! worker 사이드에서 `grok agent serve` (평문 TCP ACP) 앞에 배치하여 외부
//! orchestrator로부터의 연결을 mTLS로 종단한다. orchestrator는 사설 CA로
//! 서명된 클라이언트 인증서로 자신을 증명해야 하며, worker는 같은 CA로
//! 서명된 서버 인증서를 제출한다.
//!
//! ```text
//!   orchestrator  ── wss:// ──►  MtlsProxy (0.0.0.0:2420)
//!                                    │ TLS 종단
//!                                    │ 클라이언트 인증서 검증 (사설 CA)
//!                                    ▼
//!                                평문 TCP 복사
//!                                    │
//!                                    ▼
//!                              grok agent serve (127.0.0.1:2419)
//! ```
//!
//! ## 특징
//!
//! - **완전 비동기** — `tokio::io::copy_bidirectional` 로 좌/우 스트림을 연결.
//! - **graceful shutdown** — `watch::Receiver<bool>` 로 accept 루프 종료.
//!   진행 중인 복사는 클라이언트/업스트림 종료에 의해 자연스럽게 끝남.
//! - **에러 격리** — 단일 연결 실패가 다른 연결에 영향 없음. accept 루프는
//!   영구적 에러(`TcpListener` 닫힘 등)에서만 종료.

use std::net::SocketAddr;
use std::sync::Arc;

use rustls::ServerConfig;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, info, warn};

/// mTLS proxy 생성/실행 중 에러.
#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("failed to bind {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error("failed to build TLS acceptor: {0}")]
    Tls(String),
    #[error("listener closed: {0}")]
    Listener(String),
}

/// mTLS 종단 proxy. 한 번 `run` 하면 `shutdown` 신호 또는 영구 에러까지 실행.
pub struct MtlsProxy {
    listener: TcpListener,
    upstream_addr: SocketAddr,
    acceptor: TlsAcceptor,
}

impl MtlsProxy {
    /// 미리 bind. `run` 전에 호출되어야 하며, 외부에서 `listener.local_addr()`로
    /// 바인딩된 주소를 알 수 있다 (테스트/라우팅용).
    pub async fn bind(
        listen_addr: SocketAddr,
        upstream_addr: SocketAddr,
        server_config: Arc<ServerConfig>,
    ) -> Result<Self, ProxyError> {
        let listener = TcpListener::bind(listen_addr)
            .await
            .map_err(|source| ProxyError::Bind {
                addr: listen_addr,
                source,
            })?;
        Ok(Self {
            listener,
            upstream_addr,
            acceptor: TlsAcceptor::from(server_config),
        })
    }

    /// 바인딩된 리스너의 로컬 주소.
    pub fn local_addr(&self) -> Result<SocketAddr, ProxyError> {
        self.listener
            .local_addr()
            .map_err(|e| ProxyError::Listener(e.to_string()))
    }

    /// accept 루프 실행. `shutdown` 이 true가 되면 listener를 닫고 반환.
    pub async fn run(self, mut shutdown: watch::Receiver<bool>) -> Result<(), ProxyError> {
        let listen_addr = self
            .listener
            .local_addr()
            .map_err(|e| ProxyError::Listener(e.to_string()))?;
        info!(
            listen = %listen_addr,
            upstream = %self.upstream_addr,
            "mTLS proxy listening"
        );

        let listener = self.listener;
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("mTLS proxy shutting down");
                        break;
                    }
                }
                accept = listener.accept() => {
                    let (tcp, peer) = match accept {
                        Ok(v) => v,
                        Err(e) => {
                            // 단일 accept 실패는 로깅하고 계속. 영구적이면 break.
                            warn!(error = %e, "accept failed");
                            if e.kind() == std::io::ErrorKind::Other
                                || e.kind() == std::io::ErrorKind::PermissionDenied {
                                return Err(ProxyError::Listener(e.to_string()));
                            }
                            continue;
                        }
                    };
                    let acceptor = self.acceptor.clone();
                    let upstream = self.upstream_addr;
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(tcp, peer, acceptor, upstream).await {
                            debug!(%peer, error = %e, "connection ended with error");
                        }
                    });
                }
            }
        }
        Ok(())
    }
}

/// 단일 연결 처리. TLS 핸드셰이크 → 업스트림 TCP 연결 → 양방향 복사.
async fn handle_connection(
    tcp: TcpStream,
    peer: SocketAddr,
    acceptor: TlsAcceptor,
    upstream: SocketAddr,
) -> Result<(), ProxyError> {
    let tls = acceptor
        .accept(tcp)
        .await
        .map_err(|e| ProxyError::Tls(format!("handshake: {e}")))?;
    debug!(%peer, "mTLS handshake ok");

    let upstream_tcp = TcpStream::connect(upstream)
        .await
        .map_err(|e| ProxyError::Tls(format!("connect upstream {upstream}: {e}")))?;

    // 양방향 복사.
    let (mut tls_rx, mut tls_tx) = tokio::io::split(tls);
    let (mut up_rx, mut up_tx) = tokio::io::split(upstream_tcp);

    let copy1 = async {
        tokio::io::copy(&mut tls_rx, &mut up_tx).await?;
        up_tx.shutdown().await.ok();
        Ok::<_, std::io::Error>(())
    };
    let copy2 = async {
        tokio::io::copy(&mut up_rx, &mut tls_tx).await?;
        tls_tx.shutdown().await.ok();
        Ok::<_, std::io::Error>(())
    };

    match tokio::try_join!(copy1, copy2) {
        Ok(_) => Ok(()),
        Err(e) => {
            // 일반적인 연결 종료 (peer reset 포함) — warn 레벨로 기록하지만
            // 호출자는 debug 로깅.
            debug!(error = %e, "copy ended");
            Ok(())
        }
    }
}
