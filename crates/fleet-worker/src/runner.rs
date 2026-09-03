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
//!   ├── AgentProcessManager (상태만 보유 — 태스크를 따로 띄우지 않는다)
//!   │     └── heartbeat 루프가 beat마다 reconcile()을 호출한다 (`#67` 4c-A)
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
//!         → Agent 프로세스 종료
//!         → mtls proxy 종료
//!         (deregister 하지 않음 — 로드맵 #78. control plane의 heartbeat
//!          timeout이 Offline 전이를 담당하고, 영구 제거는 관리자 명령만.)
//! ```

use std::sync::Arc;

use tokio::signal;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::agent_process::AgentProcessManager;
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

        // 0. Agent 프로세스 매니저 생성 (로드맵 `#67` 4c-A).
        //    grok을 띄우기 **전에** 만든다 — 포트 범위 파싱이 여기서 실패할 수
        //    있고, 그 시점에는 아직 정리할 자식 프로세스가 없다.
        let agent_manager = Arc::new(AgentProcessManager::new(config.clone())?);

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

        // 3. heartbeat 루프 백그라운드 시작 — `liveness_mode == periodic`일 때만
        //    (로드맵 #61 2단계). `on_demand`는 idle 시 heartbeat 트래픽을 내지
        //    않는 것이 계약이므로 루프 자체를 시작하지 않는다. shutdown 채널은
        //    두 모드 모두 필요 없으므로 periodic 분기 안에서만 만든다.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let hb_handle = if config.worker.liveness_mode == fleet_core::WorkerLivenessMode::Periodic {
            let hb_client = client.clone();
            let hb_grok_bind = config.grok.bind_addr.clone();
            let hb_interval = register_resp
                .heartbeat_interval_secs
                .max(config.worker.heartbeat_interval_secs);
            let hb_fence_after = config.worker.agent_fence_after_secs;
            let hb_shutdown_rx = shutdown_rx.clone();
            let hb_agent_manager = agent_manager.clone();
            Some(tokio::spawn(async move {
                hb_client
                    .run_heartbeat_loop(
                        hb_interval,
                        hb_fence_after,
                        hb_grok_bind,
                        hb_agent_manager,
                        hb_shutdown_rx,
                    )
                    .await;
            }))
        } else {
            // Agent 명령의 유일한 전달 경로가 heartbeat 응답이므로, on_demand
            // Worker에는 Agent가 배정되지 않는다 — 4a의 배치 선택기가 이미
            // 그런 Worker를 후보에서 제외한다.
            info!("liveness_mode=on_demand — periodic heartbeat loop not started");
            None
        };

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

        // heartbeat 루프 정리 (최대 5초). on_demand 모드에서는 애초에 spawn되지
        // 않았으므로 (`hb_handle` == None) 대기할 것이 없다.
        if let Some(hb_handle) = hb_handle {
            let hb_join = tokio::time::timeout(std::time::Duration::from_secs(5), hb_handle).await;
            match hb_join {
                Ok(Ok(())) => info!("heartbeat loop exited"),
                _ => warn!("heartbeat loop did not exit cleanly"),
            }
        }

        // Agent 프로세스 정리. heartbeat 루프를 join한 **뒤에** 부른다 — 그
        // 전에 부르면 진행 중이던 beat이 방금 종료한 프로세스를 다시 띄운다.
        agent_manager.shutdown_all().await;

        // mTLS proxy 정리 (최대 5초). shutdown 신호는 shutdown_rx 채널을 통해
        // 전달되므로, mtls_handle 은 shutdown_tx drop 후 자연 종료.
        if let Some(handle) = mtls_handle {
            let mtls_join = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
            match mtls_join {
                Ok(Ok(())) => info!("mTLS proxy exited"),
                _ => warn!("mTLS proxy did not exit cleanly"),
            }
        }

        // 7. 의도적으로 deregister 하지 않는다 (로드맵 #78).
        //
        // 예전에는 여기서 `client.deregister()`를 호출했다. 그 요청은
        // `DELETE /v1/workers/{id}` → `DELETE FROM workers`로 이어지고,
        // `worker_operational_credentials`(018, ON DELETE CASCADE)와
        // `worker_credentials`(005, ON DELETE CASCADE — 암호화된 LLM 키)가
        // 함께 삭제된다. 그 결과 `systemctl restart`나 호스트 재부팅 한 번으로
        // 워커가 자기 신원과 LLM credential을 영구히 잃고, 재기동 뒤에는
        // worker.toml의 `operational_token`이 어떤 digest와도 매치되지 않아
        // register가 영구 401이 된다(복구 경로는 새 bootstrap token으로
        // 재-join뿐). 인증이 구성된 모든 배포가 해당됐다.
        //
        // "이 워커는 이제 없다"는 신호는 control plane이 비파괴적으로 이미
        // 만든다 — `fleet-scheduler`의 HealthChecker가 heartbeat timeout으로
        // Offline 전이와 `WorkerLeft` 이벤트를 낸다. 영구 제거는 관리자가
        // `DELETE /v1/workers/{id}`(= `fleet workers delete`)를 명시적으로
        // 호출할 때만 일어나야 한다.
        //
        // `WorkerClient::deregister`는 그 관리 경로용으로 남겨둔다.
        info!(reason = %shutdown_reason, "fleet-worker shutdown complete");
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

    // 로드맵 #36 — cert_reload_interval_secs가 설정된 경우, 서버 인증서를
    // ServerConfig에 고정하지 않고 RotatingCertResolver에 위임한 뒤 별도
    // 백그라운드 루프가 주기적으로 reload()를 호출한다. 미설정(기본값,
    // 하위 호환)이면 기존처럼 기동 시 한 번만 읽는다.
    let (server_config, reload_handle) = match mtls.cert_reload_interval_secs {
        Some(secs) if secs > 0 => {
            let (config, resolver) = server_tls
                .build_rotating_server_config()
                .map_err(|e| WorkerError::Config(format!("mtls server config: {e}")))?;
            let mut reload_shutdown = shutdown.clone();
            let reload_tls = server_tls.clone();
            let interval = std::time::Duration::from_secs(secs);
            let handle = tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                ticker.tick().await; // 최초 tick은 즉시 완료 — 실제 첫 갱신은 한 주기 뒤.
                loop {
                    tokio::select! {
                        biased;
                        _ = reload_shutdown.changed() => {
                            if *reload_shutdown.borrow() {
                                break;
                            }
                        }
                        _ = ticker.tick() => {
                            match resolver.reload(&reload_tls) {
                                Ok(()) => info!("mTLS server certificate reloaded"),
                                Err(e) => warn!(
                                    error = %e,
                                    "mTLS certificate reload failed — continuing to serve the last-good certificate"
                                ),
                            }
                        }
                    }
                }
            });
            (config, Some(handle))
        }
        _ => {
            let config = server_tls
                .build_server_config()
                .map_err(|e| WorkerError::Config(format!("mtls server config: {e}")))?;
            (config, None)
        }
    };

    let proxy = MtlsProxy::bind(listen_addr, upstream_addr, Arc::new(server_config))
        .await
        .map_err(|e| WorkerError::Config(format!("mtls proxy bind: {e}")))?;
    let bound = proxy.local_addr().ok();
    info!(
        listen = ?bound,
        upstream = %upstream_addr,
        cert_reload_interval_secs = ?mtls.cert_reload_interval_secs,
        "starting mTLS proxy"
    );
    let handle = tokio::spawn(async move {
        if let Err(e) = proxy.run(shutdown).await {
            error!(error = %e, "mTLS proxy exited with error");
        }
        // reload 루프도 proxy와 생사를 같이 한다 — shutdown 신호가 이미
        // 공유되므로 별도 join으로 정리만 기다린다(에러는 무시: 로깅용 백그라운드
        // 루프가 실패해도 proxy 자체의 종료 흐름을 막지 않는다).
        if let Some(h) = reload_handle {
            let _ = h.await;
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
