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

        // 4. 신호 대기 — 또는 heartbeat 루프가 그보다 먼저 끝나는 것
        //    (로드맵 `#67` 게이트 ⑥).
        let mut hb_handle = hb_handle;
        let (shutdown_reason, hb_fault) = await_shutdown(hb_handle.as_mut()).await;
        match &hb_fault {
            None => info!(reason = %shutdown_reason, "shutdown signal received"),
            Some(_) => error!(
                reason = %shutdown_reason,
                "the heartbeat loop is gone — shutting down so this worker's agent \
                 processes do not keep running unsupervised"
            ),
        }

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
        //
        // `hb_fault`가 있으면 위 `await_shutdown`이 이미 join했다 — 완료된
        // `JoinHandle`을 다시 await하면 패닉이므로 건너뛴다.
        if let (Some(hb_handle), None) = (hb_handle, &hb_fault) {
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
        //
        // 루프가 결함으로 사라진 것이면 **실패로 끝낸다** (로드맵 `#67` 게이트 ⑥).
        // `main`이 `Err`를 `ExitCode::FAILURE`로 옮기므로, systemd 같은 감시자가
        // 이 워커를 다시 띄운다 — 그리고 재기동의 `sweep_stale_incarnation()`이
        // 혹시 살아남은 자식을 걷는다. `Ok(())`로 끝내면 감시자는 정상 종료로
        // 읽고 재기동하지 않으며, 그 호스트는 heartbeat 없는 워커가 된다.
        if let Some(fault) = hb_fault {
            error!(reason = %shutdown_reason, "fleet-worker shutdown complete (fail-closed)");
            return Err(fault);
        }
        info!(reason = %shutdown_reason, "fleet-worker shutdown complete");
        Ok(())
    }
}

/// 종료 사유를 기다린다 — OS 신호, 또는 heartbeat 루프가 먼저 끝나는 것
/// (로드맵 `#67` 게이트 ⑥). 두 번째 값이 `Some`이면 결함이다.
///
/// **이 함수가 없으면 죽은 루프는 보이지 않는다.** 루프는 `tokio::spawn`으로
/// 띄우고 종료 경로에서만 join하므로, 신호가 오기 전에 루프가 끝나도 아무도
/// 알아채지 못한다. 그동안 이 Worker는 heartbeat을 보내지 않고 — 오케스트레이터는
/// 45초(`HealthConfig` 기본 15초 × 3) 뒤 `Offline`으로 판정한다 — Agent 프로세스는
/// 계속 돈다. 게이트 ⑥이 만든 유예가 무의미해지는 경로가 정확히 이것이다:
/// 펜싱은 이 루프 안에서만 일어나므로, 루프가 죽으면 유예를 아무리 넘겨도
/// 프로세스는 멈추지 않는다.
///
/// 그래서 `036`의 트레이드오프에 붙은 운영 규칙("`last_seen`에서 유예가 지난 뒤에
/// Worker를 삭제하라")이 성립하려면 이 감시가 필요하다. 그 규칙의 근거는 "유예를
/// 넘겼으면 프로세스는 이미 없다"인데, 죽은 루프는 그 추론의 반례다.
///
/// **덮지 못하는 것.** 루프가 `await`에서 영구히 막히거나(데드락), 런타임이
/// 블로킹 호출에 굶거나, 프로세스가 `SIGSTOP`으로 멈추면 `JoinHandle`은 끝나지
/// 않으므로 이 함수도 깨어나지 않는다. 그것까지 잡으려면 루프가 매 회차 올리는
/// 진행 카운터를 **OS 스레드**가 감시해야 하고(`spawn`한 태스크는 굶은 런타임에서
/// 같이 굶는다), 그 스레드가 할 수 있는 유일한 조치는 `.fleet-agent.json`에 적힌
/// pid를 직접 죽이는 것이다 — `process::abort`는 소멸자를 돌리지 않아
/// `kill_on_drop`이 자식을 죽이지 못하기 때문이다. 이 증분에서는 만들지 않았다.
async fn await_shutdown(
    hb_handle: Option<&mut tokio::task::JoinHandle<()>>,
) -> (String, Option<WorkerError>) {
    let Some(handle) = hb_handle else {
        // `on_demand`에는 루프가 없다 — 감시할 대상이 없으므로 신호만 기다린다.
        return (wait_for_signal().await, None);
    };
    tokio::select! {
        reason = wait_for_signal() => (reason, None),
        joined = handle => {
            // 루프는 shutdown 신호에만 끝나도록 쓰여 있고 그 신호는 아직 오지
            // 않았다. 그러므로 어떤 갈래든 결함이다.
            let reason = match joined {
                Err(e) if e.is_panic() => "heartbeat loop panicked",
                Err(_) => "heartbeat loop was cancelled",
                Ok(()) => "heartbeat loop returned without a shutdown signal",
            };
            (reason.to_string(), Some(WorkerError::Other(reason.to_string())))
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 죽은 루프가 **깨우는지**가 이 시험의 전부다. 이전에는 이 자리가
    /// `wait_for_signal().await` 하나여서 루프가 어떻게 끝나든 신호가 올
    /// 때까지 아무 일도 일어나지 않았다 — 그동안 Agent 프로세스는 계속 돌고
    /// 펜싱은 영영 오지 않는다. 시험이 걸리면 hang이 아니라 실패로 끝나야
    /// 하므로 타임아웃 안에서 돌린다.
    #[tokio::test]
    async fn a_panicking_heartbeat_loop_ends_the_wait_as_a_fault() {
        let mut handle = tokio::spawn(async { panic!("boom") });

        let (reason, fault) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            await_shutdown(Some(&mut handle)),
        )
        .await
        .expect("죽은 루프는 신호를 기다리지 않고 즉시 깨워야 한다");

        assert_eq!(reason, "heartbeat loop panicked");
        assert!(
            fault.is_some(),
            "결함으로 분류해야 한다 — 여기서 None이면 워커가 성공 종료 코드로 \
             끝나고 감시자가 재기동하지 않는다"
        );
    }

    /// 패닉이 아니라 **조용히 반환**하는 경우도 결함이다. 루프는 shutdown
    /// 신호에만 끝나도록 쓰여 있으므로, 신호 없이 돌아온 것은 그 계약이 깨진
    /// 것이며 결과는 패닉과 같다 — heartbeat도 펜싱도 없는 워커.
    #[tokio::test]
    async fn a_heartbeat_loop_that_returns_early_is_also_a_fault() {
        let mut handle = tokio::spawn(async {});

        let (reason, fault) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            await_shutdown(Some(&mut handle)),
        )
        .await
        .expect("조용한 반환도 즉시 깨워야 한다");

        assert_eq!(reason, "heartbeat loop returned without a shutdown signal");
        assert!(fault.is_some());
    }

    /// 살아 있는 루프는 이 함수를 깨우지 않는다. 이 단정이 없으면 위 둘은
    /// "무엇이든 결함으로 만든다"로도 통과한다 — 그 구현은 정상 워커를
    /// 기동 직후 죽인다.
    #[tokio::test]
    async fn a_running_heartbeat_loop_does_not_wake_the_wait() {
        let mut handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        });

        let outcome = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            await_shutdown(Some(&mut handle)),
        )
        .await;

        assert!(
            outcome.is_err(),
            "루프가 도는 동안에는 신호를 계속 기다려야 한다"
        );
        handle.abort();
    }
}
