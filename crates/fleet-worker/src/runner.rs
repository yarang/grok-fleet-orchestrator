//! WorkerRunner — grok 서브프로세스와 등록/하트비트 루프를 조립.
//!
//! ```text
//! [WorkerRunner]
//!   │
//!   ├── (선택) MtlsProxy 백그라운드 (Phase 8.5)
//!   │     └── 외부 wss:// → grok agent serve (평문 TCP)
//!   │
//!   ├── GrokRunner (백그라운드)
//!   │     └── grok agent serve (재시작 루프)
//!   │
//!   ├── register_with_retry (1회)
//!   │     ↓
//!   ├── run_heartbeat_loop (백그라운드)
//!   │
//!   └── tokio::signal::ctrl_c / SIGTERM 대기
//!         ↓
//!         shutdown_tx.send(true)
//!         → grok 종료
//!         → heartbeat 루프 종료
//!         → mtls proxy 종료
//!         → deregister (best-effort)
//! ```

use std::sync::Arc;

use tokio::signal;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::config::WorkerConfig;
use crate::error::WorkerError;
use crate::grok_process::GrokRunner;
use crate::registration::RegistrationClient;
use fleet_transport::mtls_proxy::MtlsProxy;
use fleet_transport::tls::ServerTlsConfig;

/// fleet-worker 메인 runner.
pub struct WorkerRunner {
    config: Arc<WorkerConfig>,
}

impl WorkerRunner {
    /// 새 runner.
    pub fn new(config: WorkerConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    /// 메인 진입점. SIGINT/SIGTERM 수신 시 graceful shutdown.
    pub async fn run(self) -> Result<(), WorkerError> {
        let config = self.config.clone();
        info!(
            name = %config.worker.name,
            orchestrator = %config.worker.orchestrator_url,
            bind_addr = %config.grok.bind_addr,
            mtls_enabled = config.mtls.as_ref().map(|m| m.enabled).unwrap_or(false),
            "fleet-worker starting"
        );

        // 1. GrokRunner 백그라운드 시작.
        let (grok_runner, grok_shutdown_tx) = GrokRunner::new(config.clone());
        let grok_handle = tokio::spawn(async move { grok_runner.run().await });

        // 2. (선택) mTLS proxy 백그라운드 시작. grok이 bind_addr 에서 준비될 때까지
        //    짧게 대기.
        // mTLS proxy용 shutdown 채널. heartbeat 루프와 별개로 관리.
        let (mtls_shutdown_tx, mtls_shutdown_rx) = watch::channel(false);
        let mtls_handle = spawn_mtls_proxy_if_enabled(&config, mtls_shutdown_rx).await?;

        // 2. orchestrator에 등록 (재시도 포함).
        let client = Arc::new(RegistrationClient::new(config.clone())?);
        let register_resp = match client.register_with_retry().await {
            Ok(r) => r,
            Err(e) => {
                error!(error = %e, "failed to register with orchestrator — shutting down");
                let _ = grok_shutdown_tx.send(true);
                let _ = mtls_shutdown_tx.send(true);
                let _ = grok_handle.await;
                // mTLS proxy도 함께 종료.
                if let Some(handle) = mtls_handle {
                    let _ = handle.await;
                }
                return Err(e);
            }
        };

        // 3. heartbeat 루프 백그라운드 시작.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let hb_client = client.clone();
        let hb_grok_bind = config.grok.bind_addr.clone();
        let hb_interval = register_resp
            .heartbeat_interval_secs
            .max(config.worker.heartbeat_interval_secs);
        let hb_shutdown_rx = shutdown_rx.clone();
        let hb_handle = tokio::spawn(async move {
            hb_client
                .run_heartbeat_loop(hb_interval, hb_grok_bind, hb_shutdown_rx)
                .await;
        });

        // 4. 신호 대기.
        let shutdown_reason = wait_for_signal().await;
        info!(reason = %shutdown_reason, "shutdown signal received");

        // 5. shutdown 전파.
        let _ = shutdown_tx.send(true);
        let _ = grok_shutdown_tx.send(true);
        let _ = mtls_shutdown_tx.send(true);

        // 6. 백그라운드 태스크 정리.
        // grok이 종료될 때까지 최대 10초 대기.
        let grok_join = tokio::time::timeout(std::time::Duration::from_secs(10), grok_handle).await;
        match grok_join {
            Ok(Ok(Ok(()))) => info!("grok runner exited cleanly"),
            Ok(Ok(Err(e))) => warn!(error = %e, "grok runner exited with error"),
            Ok(Err(_)) => warn!("grok runner task panicked"),
            Err(_) => warn!("grok runner did not exit within 10s — abandoning"),
        }

        // heartbeat 루프 정리 (최대 5초).
        let hb_join = tokio::time::timeout(std::time::Duration::from_secs(5), hb_handle).await;
        match hb_join {
            Ok(Ok(())) => info!("heartbeat loop exited"),
            _ => warn!("heartbeat loop did not exit cleanly"),
        }

        // mTLS proxy 정리 (최대 5초). shutdown 신호는 shutdown_rx 채널을 통해
        // 전달되므로, mtls_handle 은 shutdown_tx drop 후 자연 종료.
        if let Some(handle) = mtls_handle {
            let mtls_join = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
            match mtls_join {
                Ok(Ok(())) => info!("mTLS proxy exited"),
                _ => warn!("mTLS proxy did not exit cleanly"),
            }
        }

        // 7. deregister (best-effort).
        client.deregister(&shutdown_reason).await;

        info!("fleet-worker shutdown complete");
        Ok(())
    }
}

/// mTLS 가 활성화된 경우 `MtlsProxy` 를 spawn 한다. 비활성 시 None 반환.
///
/// grok agent serve 가 bind_addr 에서 청취할 때까지 폴링으로 대기한 뒤
/// upstream_addr 을 bind_addr 으로 지정해 proxy 를 시작한다.
///
/// 에러: TLS 설정 파일 읽기/파싱 실패 시 `WorkerError::Config`.
async fn spawn_mtls_proxy_if_enabled(
    config: &WorkerConfig,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<Option<tokio::task::JoinHandle<()>>, WorkerError> {
    let mtls = match &config.mtls {
        Some(m) if m.enabled => m,
        _ => return Ok(None),
    };

    let listen_addr: std::net::SocketAddr = mtls
        .listen_addr
        .parse()
        .map_err(|e| WorkerError::Config(format!("mtls.listen_addr parse: {e}")))?;
    let upstream_addr: std::net::SocketAddr =
        format!("127.0.0.1:{}", grok_port(&config.grok.bind_addr))
            .parse()
            .map_err(|e| WorkerError::Config(format!("upstream parse: {e}")))?;

    // grok이 bind_addr 에 바인딩할 때까지 대기 (최대 5초).
    let grok_bind = config.grok.bind_addr.clone();
    let wait_start = std::time::Instant::now();
    loop {
        if tokio::net::TcpStream::connect(&grok_bind).await.is_ok() {
            break;
        }
        if wait_start.elapsed() > std::time::Duration::from_secs(5) {
            warn!(
                bind = %grok_bind,
                "grok did not bind within 5s; mTLS proxy will still start"
            );
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let server_tls = ServerTlsConfig::from_paths(
        &mtls.client_ca_path,
        &mtls.server_cert_path,
        &mtls.server_key_path,
    );
    let server_config = server_tls
        .build_server_config()
        .map_err(|e| WorkerError::Config(format!("mtls server config: {e}")))?;

    let proxy = MtlsProxy::bind(listen_addr, upstream_addr, Arc::new(server_config))
        .await
        .map_err(|e| WorkerError::Config(format!("mtls proxy bind: {e}")))?;
    let bound = proxy.local_addr().ok();
    info!(listen = ?bound, upstream = %upstream_addr, "starting mTLS proxy");
    let handle = tokio::spawn(async move {
        if let Err(e) = proxy.run(shutdown).await {
            error!(error = %e, "mTLS proxy exited with error");
        }
    });
    Ok(Some(handle))
}

/// grok bind_addr 문자열 ("127.0.0.1:2419") 에서 포트 추출.
fn grok_port(bind_addr: &str) -> u16 {
    bind_addr
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .unwrap_or(2419)
}

/// SIGINT 또는 SIGTERM 대기. 반환값은 종료 사유 문자열.
async fn wait_for_signal() -> String {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
        let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");

        tokio::select! {
            _ = sigint.recv() => "SIGINT".to_string(),
            _ = sigterm.recv() => "SIGTERM".to_string(),
            _ = signal::ctrl_c() => "ctrl_c".to_string(),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = signal::ctrl_c().await;
        "ctrl_c".to_string()
    }
}
