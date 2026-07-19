//! WorkerRunner — grok 서브프로세스와 등록/하트비트 루프를 조립.
//!
//! ```text
//! [WorkerRunner]
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
            "fleet-worker starting"
        );

        // 1. GrokRunner 백그라운드 시작.
        let (grok_runner, grok_shutdown_tx) = GrokRunner::new(config.clone());
        let grok_handle = tokio::spawn(async move { grok_runner.run().await });

        // 2. orchestrator에 등록 (재시도 포함).
        let client = Arc::new(RegistrationClient::new(config.clone())?);
        let register_resp = match client.register_with_retry().await {
            Ok(r) => r,
            Err(e) => {
                error!(error = %e, "failed to register with orchestrator — shutting down");
                let _ = grok_shutdown_tx.send(true);
                let _ = grok_handle.await;
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

        // 6. 백그라운드 태스크 정리.
        // grok이 종료될 때까지 최대 10초 대기.
        let grok_join =
            tokio::time::timeout(std::time::Duration::from_secs(10), grok_handle).await;
        match grok_join {
            Ok(Ok(Ok(()))) => info!("grok runner exited cleanly"),
            Ok(Ok(Err(e))) => warn!(error = %e, "grok runner exited with error"),
            Ok(Err(_)) => warn!("grok runner task panicked"),
            Err(_) => warn!("grok runner did not exit within 10s — abandoning"),
        }

        // heartbeat 루프 정리 (최대 5초).
        let hb_join =
            tokio::time::timeout(std::time::Duration::from_secs(5), hb_handle).await;
        match hb_join {
            Ok(Ok(())) => info!("heartbeat loop exited"),
            _ => warn!("heartbeat loop did not exit cleanly"),
        }

        // 7. deregister (best-effort).
        client.deregister(&shutdown_reason).await;

        info!("fleet-worker shutdown complete");
        Ok(())
    }
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
