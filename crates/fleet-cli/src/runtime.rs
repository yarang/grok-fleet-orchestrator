//! 명령어 실행 로직. Store + Transport + Dispatcher + MCP 서버를 조립.
//!
//! ## Phase 1 wiring
//!
//! ```text
//! DATABASE_URL ── PgStore::connect ── migrate
//!                       │
//!                       ▼
//!              MockTransport (event_rx)
//!                       │
//!                       ▼
//!              FleetState { store, transport, breakers, selector }
//!                       │
//!                       ▼
//!              Dispatcher { state, event_rx }
//!                       │
//!                       ▼
//!              tokio::spawn(dispatcher.run_event_loop())
//!                       │
//!                       ▼
//!              run_mcp_server(state, dispatcher)  ← stdio
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use fleet_api::{
    issue_admin_bootstrap_token_if_needed, run_http_server, sync_env_admin_tokens_to_store,
    ApiTokenCredential, AppState,
};
use fleet_core::{
    CircuitBreakerConfig, TaskFilter, TaskId, TaskStatus, TaskStatusFilter, WorkerFilter,
    WorkerStatus,
};
// CLI 하위 명령 enum (main.rs).
use crate::{AdminTokensAction, EventsAction, TasksAction, WorkerCredentialAction, WorkersAction};
use fleet_mcp::run_mcp_server;
use fleet_provisioner::{
    append_known_hosts_line, default_known_hosts_path, scan_host_key, HostKeyConfig, HostKeyPolicy,
    Inventory, InventoryWorker, MockExecutor, Playbook, PlaybookContext, PlaybookError,
    PlaybookReport, PrereqReport, ProvisionOptions, RemoteExecutor, SshClient, SshConnectInfo,
    StepContext,
};
use fleet_scheduler::{
    CleanupConfig, Dispatcher, FleetState, HealthChecker, HealthConfig, MultiAdminSync,
    ReconcileConfig, Reconciler, SessionCleanup,
};
use fleet_store::{PgStore, PoolConfig, Store};
use fleet_transport::{MockTransport, WorkerTransport};

#[cfg(feature = "acp")]
use fleet_transport::AcpTransport;
#[cfg(feature = "mtls")]
use fleet_transport::ClientTlsConfig;

/// Postgres 연결 URL 조회 (`DATABASE_URL` 필수).
fn database_url() -> Result<String> {
    std::env::var("DATABASE_URL")
        .context("DATABASE_URL is not set. Export DATABASE_URL=postgres://user@host/dbname")
}

/// `--mtls-ca/--mtls-cert/--mtls-key` CLI 플래그 묶음 (Phase 8.5).
///
/// 세 값이 모두 `Some` 이거나 모두 `None` 이어야 함 (`requires` 제약).
/// `Some` 인 경우 `AcpTransport` 가 `wss://` endpoint 에 mTLS 핸드셰이크를 수행.
//
// acp 와 mtls 가 모두 꺼진 최소 빌드에서는 어디서도 읽히지 않으므로 dead_code 허용.
#[cfg_attr(not(any(feature = "acp", feature = "mtls")), allow(dead_code))]
#[derive(Debug, Default, Clone, Copy)]
pub struct MtlsFlags<'a> {
    /// 사설 CA 인증서 PEM 경로.
    pub ca: Option<&'a str>,
    /// orchestrator 클라이언트 인증서 PEM 경로.
    pub cert: Option<&'a str>,
    /// orchestrator 클라이언트 비밀키 PEM 경로.
    pub key: Option<&'a str>,
}

#[cfg(feature = "mtls")]
impl<'a> MtlsFlags<'a> {
    /// 세 플래그가 모두 설정된 경우 `Some(ClientTlsConfig)` 반환.
    /// 하나라도 누락되면 `None`.
    fn to_tls_config(self) -> Option<ClientTlsConfig> {
        let ca = self.ca?;
        let cert = self.cert?;
        let key = self.key?;
        Some(ClientTlsConfig::from_paths(ca, cert, key))
    }
}

/// `--transport acp` 인 경우 `AcpTransport` 생성. mTLS 플래그가 모두
/// 설정된 경우 `with_client_tls` 로 클라이언트 인증서를 전달.
#[cfg(feature = "acp")]
fn build_acp_transport(mtls_flags: &MtlsFlags) -> Result<AcpTransport, anyhow::Error> {
    let transport = AcpTransport::new();
    #[cfg(feature = "mtls")]
    {
        if let Some(ca) = mtls_flags.ca {
            let tls = mtls_flags.to_tls_config().expect("checked ca above");
            tracing::info!(
                %ca,
                "enabling mTLS on AcpTransport (wss:// endpoints only)"
            );
            return Ok(transport.with_client_tls(tls));
        }
    }
    #[cfg(not(feature = "mtls"))]
    {
        // mtls 플래그가 일부라도 설정된 경우 명확한 에러 (run_serve 에서 사전 검증되지만
        // 방어적으로 한 번 더).
        if mtls_flags.ca.is_some() || mtls_flags.cert.is_some() || mtls_flags.key.is_some() {
            return Err(anyhow!(
                "--mtls-ca/--mtls-cert/--mtls-key require building with --features mtls"
            ));
        }
    }
    Ok(transport)
}

/// 기동 전 환경변수 검증. 문제가 있으면 전부 모아 한 번에 보고하고 중단한다.
///
/// 잘못된 설정으로 기동한 뒤 런타임에 조용히 실패하는 것(깨진 인증 메일 링크,
/// 반쪽만 설정된 SMTP 자격증명 등)을 막기 위함이다.
fn validate_env_or_bail() -> Result<()> {
    let issues = fleet_core::validate_orchestrator_env();
    if issues.is_empty() {
        return Ok(());
    }
    for issue in &issues {
        tracing::error!(key = issue.key, problem = %issue.problem, "invalid configuration");
    }
    let detail = issues
        .iter()
        .map(|i| format!("  - {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    Err(anyhow!(
        "환경변수 설정에 문제가 {}건 있습니다:\n{}",
        issues.len(),
        detail
    ))
}

/// PgStore 생성 + 마이그레이션 실행 (기본 풀 옵션, `max_connections`만 지정).
/// 짧게 실행되고 종료되는 CLI 하위 명령(`fleet tasks list` 등)이 사용.
async fn connect_and_migrate(max_conn: u32) -> Result<Arc<PgStore>> {
    connect_and_migrate_with_pool(PoolConfig {
        max_connections: max_conn,
        ..PoolConfig::default()
    })
    .await
}

/// PgStore 생성 + 마이그레이션 실행 (커넥션 풀 세부 튜닝까지 지정).
///
/// 로드맵 P2 #16 — `serve`처럼 장수명 프로세스는 `acquire_timeout`/
/// `max_lifetime`/`idle_timeout`을 명시적으로 튜닝해야 한다. 방화벽/로드밸런서의
/// idle connection kill, DB 재시작 후 stale connection, 풀 고갈 시 무한 대기
/// 같은 문제를 예방하기 위함. 짧게 실행되고 종료되는 일회성 CLI 명령에는
/// 중요도가 낮아 [`connect_and_migrate`]가 기본값을 대신 사용한다.
async fn connect_and_migrate_with_pool(pool_config: PoolConfig) -> Result<Arc<PgStore>> {
    validate_env_or_bail()?;
    let url = database_url()?;
    tracing::info!(
        url = %sanitize_url(&url),
        max_conn = pool_config.max_connections,
        acquire_timeout = ?pool_config.acquire_timeout,
        max_lifetime = ?pool_config.max_lifetime,
        idle_timeout = ?pool_config.idle_timeout,
        "connecting to Postgres"
    );
    let store = PgStore::connect_with_config(&url, pool_config)
        .await
        .context("failed to connect to Postgres")?;
    store.migrate().await.context("migration failed")?;
    tracing::info!("database migrations applied");
    Ok(Arc::new(store))
}

/// `0`은 "제한 없음"으로 해석해 `None`을 반환 (CLI 플래그에서 max_lifetime/
/// idle_timeout을 끄고 싶을 때 사용). 그 외 값은 초 단위 `Duration`으로 변환.
fn secs_to_optional_duration(secs: u64) -> Option<Duration> {
    if secs == 0 {
        None
    } else {
        Some(Duration::from_secs(secs))
    }
}

/// `postgres://user:PASSWORD@host/db`에서 비밀번호 부분 마스킹.
fn sanitize_url(url: &str) -> String {
    // 단순한 마스킹 — `://user:secret@` 형태 감지
    if let Some(idx) = url.find("://") {
        let scheme_end = idx + 3;
        if let Some(at) = url[scheme_end..].find('@') {
            let creds_end = scheme_end + at;
            let creds = &url[scheme_end..creds_end];
            if let Some(colon) = creds.find(':') {
                let user = &creds[..colon];
                return format!("{}{}:****{}", &url[..scheme_end], user, &url[creds_end..]);
            }
        }
    }
    url.to_string()
}

/// `serve` 명령 실행.
//
// acp feature 가 꺼진 최소 빌드에서는 mtls_flags 가 사용되지 않으므로 unused 허용.
#[cfg_attr(not(feature = "acp"), allow(unused_variables))]
#[allow(clippy::too_many_arguments)]
pub async fn run_serve(
    transport_kind: &str,
    db_max_conn: u32,
    db_acquire_timeout_secs: u64,
    db_max_lifetime_secs: u64,
    db_idle_timeout_secs: u64,
    no_health_check: bool,
    health_interval_secs: u64,
    health_missed: u32,
    no_cleanup: bool,
    cleanup_interval_secs: u64,
    cleanup_retention_days: i64,
    no_reconcile: bool,
    reconcile_interval_secs: u64,
    reconcile_stale_secs: u64,
    reconcile_dispatched_check_secs: u64,
    reconcile_offline_worker_grace_secs: u64,
    reconcile_max_dispatch_retries: u32,
    http_bind: Option<&str>,
    api_tokens: Option<&str>,
    cf_audience: Option<&str>,
    dashboard_bind: Option<&str>,
    no_circuit_sync: bool,
    mtls_flags: MtlsFlags<'_>,
) -> Result<()> {
    // 로드맵 P2 #16 — `serve`는 장수명 프로세스이므로 풀 세부 옵션을 명시적으로
    // 튜닝한다 (기본값은 `PoolConfig::default()`와 동일 — CLI에서 오버라이드하지
    // 않으면 이전과 동일하게 동작).
    let pool_config = PoolConfig {
        max_connections: db_max_conn,
        acquire_timeout: Duration::from_secs(db_acquire_timeout_secs.max(1)),
        max_lifetime: secs_to_optional_duration(db_max_lifetime_secs),
        idle_timeout: secs_to_optional_duration(db_idle_timeout_secs),
    };
    let store = connect_and_migrate_with_pool(pool_config).await?;

    // Transport 선택: `mock` (기본, 테스트/개발) 또는 `acp` (Phase 7 — 실제 grok agent).
    let (transport, event_rx): (
        Arc<dyn fleet_transport::WorkerTransport>,
        tokio::sync::mpsc::UnboundedReceiver<fleet_transport::WorkerEvent>,
    ) = match transport_kind {
        "mock" => {
            tracing::info!("using MockTransport (no real workers will be contacted)");
            let t = MockTransport::new();
            let rx = t.subscribe().await?;
            (Arc::new(t) as Arc<dyn fleet_transport::WorkerTransport>, rx)
        }
        "acp" => {
            #[cfg(feature = "acp")]
            {
                if mtls_flags.ca.is_some() && !cfg!(feature = "mtls") {
                    return Err(anyhow!("--mtls-ca requires building with --features mtls"));
                }
                tracing::info!("using AcpTransport (will connect to grok agent serve on each registered worker)");
                let t = build_acp_transport(&mtls_flags)?;
                let rx = t.subscribe().await?;
                (Arc::new(t) as Arc<dyn fleet_transport::WorkerTransport>, rx)
            }
            #[cfg(not(feature = "acp"))]
            {
                return Err(anyhow!(
                    "transport 'acp' requires building with --features fleet-transport/acp"
                ));
            }
        }
        other => {
            return Err(anyhow!(
                "unknown transport '{other}'. Supported: `mock`, `acp`."
            ));
        }
    };

    let transport_handle = transport.clone();

    let state = Arc::new(FleetState::new(
        store.clone(),
        transport,
        CircuitBreakerConfig::default(),
    ));

    // 로드맵 #38 — `Dispatcher`와 `Reconciler`가 같은 재시도 상한
    // (`reconcile_max_dispatch_retries`)을 공유해야 `submit()`이 남겨둔
    // `Pending` 작업을 Reconciler가 일관되게 재시도/소진 판단할 수 있다.
    let dispatcher = Arc::new(
        Dispatcher::new(state.clone()).with_max_dispatch_retries(reconcile_max_dispatch_retries),
    );
    dispatcher.attach_event_receiver(event_rx).await;

    // 백그라운드에서 워커 이벤트 소비 루프 시작.
    let dispatcher_loop = dispatcher.clone();
    tokio::spawn(async move {
        dispatcher_loop.run_event_loop().await;
    });

    // 헬스체크 루프 (옵션). missed heartbeat → offline 처리.
    let _health_handle = if !no_health_check {
        let cfg = HealthConfig {
            check_interval: Duration::from_secs(health_interval_secs),
            missed_heartbeat_threshold: health_missed,
        };
        tracing::info!(
            interval_secs = health_interval_secs,
            missed_threshold = health_missed,
            "health checker enabled"
        );
        let checker = HealthChecker::new(state.clone(), cfg);
        Some(checker.spawn())
    } else {
        tracing::info!("health checker disabled by --no-health-check");
        None
    };

    // 자율 자가치유 제어 엔진 (Autonomic Self-Healing Engine, MAPE-K)은
    // 2026-08-13에 삭제되었다 — 재연결에는 단순 타입 수정이 아니라 하드웨어
    // 메트릭 저장 위치부터 다시 설계해야 하는 별도 기능 개발이 필요했다.
    // 설계 의도는 `docs/architecture/overview.md`에 보존, 재구현 시
    // `docs/roadmap/roadmap.md` #43 참고.

    // 만료 세션/오래된 로그인 시도 정리 루프 (옵션). 로드맵 P1 #18 — 이전에는
    // `delete_expired_sessions`를 프로덕션 어디서도 호출하지 않아 `sessions`
    // 테이블이 무한정 쌓였다.
    let _cleanup_handle = if !no_cleanup {
        let cfg = CleanupConfig {
            interval: Duration::from_secs(cleanup_interval_secs.max(1)),
            login_attempt_retention: chrono::Duration::days(cleanup_retention_days.max(1)),
        };
        tracing::info!(
            interval_secs = cleanup_interval_secs,
            retention_days = cleanup_retention_days,
            "session/login-attempt cleanup task enabled"
        );
        let cleanup = SessionCleanup::new(store.clone(), cfg);
        Some(cleanup.spawn())
    } else {
        tracing::info!("cleanup task disabled by --no-cleanup");
        None
    };

    // stale `Pending`/`Dispatched` 작업 재조정 루프 (옵션). `Dispatcher::submit()`은
    // 제출 시점에 딱 한 번만 워커 선택/dispatch를 시도하므로, 그 시도가
    // 터미널 상태에 도달하기 전에 프로세스가 죽으면 작업이 영구히 `Pending`에
    // 고아로 남는다 — 이 루프가 주기적으로 그런 작업을 재시도한다. 워커가
    // 재시작해 새 worker_id로 재등록되어 `Dispatched` 작업이 고아가 되는
    // 케이스, 그리고 워커가 여전히 등록돼 있지만 `Offline`으로 장기간 남아있는
    // 케이스(HealthChecker는 Worker.status만 바꾸고 Task는 건드리지 않음)도
    // 같은 루프가 회수한다 (자세한 배경은 `reconcile.rs` 모듈 문서 참고).
    let _reconcile_handle = if !no_reconcile {
        let cfg = ReconcileConfig {
            interval: Duration::from_secs(reconcile_interval_secs.max(1)),
            stale_after: Duration::from_secs(reconcile_stale_secs.max(1)),
            dispatched_worker_check_after: Duration::from_secs(
                reconcile_dispatched_check_secs.max(1),
            ),
            offline_worker_grace: Duration::from_secs(reconcile_offline_worker_grace_secs.max(1)),
            max_dispatch_retries: reconcile_max_dispatch_retries,
        };
        tracing::info!(
            interval_secs = reconcile_interval_secs,
            stale_secs = reconcile_stale_secs,
            dispatched_check_secs = reconcile_dispatched_check_secs,
            offline_worker_grace_secs = reconcile_offline_worker_grace_secs,
            max_dispatch_retries = reconcile_max_dispatch_retries,
            "task reconciliation loop enabled (pending redispatch + orphaned/offline dispatched reap)"
        );
        let reconciler = Reconciler::new(state.clone(), dispatcher.clone(), cfg);
        Some(reconciler.spawn())
    } else {
        tracing::info!("pending-task reconciliation loop disabled by --no-reconcile");
        None
    };

    // 다중 오케스트레이터 인스턴스 간 CircuitBreaker 상태 동기화 (옵션,
    // 기본값: 활성). 로드맵 #25 — `MultiAdminSync`는 Postgres LISTEN/NOTIFY로
    // 다른 인스턴스가 발행한 `WorkerCircuitChanged`/`WorkerLeft` 이벤트를
    // 받아 로컬 `BreakerRegistry`에 즉시 반영한다. 구현·테스트는 오래전에
    // 끝났지만(`fleet-scheduler/tests/scaleout_sync.rs`) 이 기동 경로에는
    // 한 번도 연결되지 않아, 스케일아웃 배포에서 한 인스턴스가 워커를
    // CircuitOpen 시켜도 다른 인스턴스는 자신이 별도로 실패를 겪기 전까지
    // 이를 몰랐다.
    let _circuit_sync_handle = if !no_circuit_sync {
        tracing::info!("multi-admin CircuitBreaker sync enabled");
        let sync = MultiAdminSync::new(state.clone(), store.pool().clone());
        Some(tokio::spawn(async move {
            sync.run().await;
        }))
    } else {
        tracing::info!("multi-admin CircuitBreaker sync disabled by --no-circuit-sync");
        None
    };

    // HTTP API 서버 (옵션). --http-bind가 지정된 경우에만 실행.
    let _http_handle = if let Some(bind_str) = http_bind {
        let bind: SocketAddr = bind_str
            .parse()
            .with_context(|| format!("invalid --http-bind address: {bind_str}"))?;

        let scoped_tokens = api_tokens.map(parse_scoped_api_tokens).transpose()?;
        let has_bearer_tokens = scoped_tokens
            .as_ref()
            .is_some_and(|tokens| !tokens.is_empty());
        let has_cf_access = cf_audience.is_some_and(|aud| !aud.trim().is_empty());
        if !bind.ip().is_loopback() && !has_bearer_tokens && !has_cf_access {
            return Err(anyhow!(
                "refusing unauthenticated non-loopback HTTP bind {bind}; configure FLEET_API_TOKENS or FLEET_CF_AUDIENCE"
            ));
        }

        let mut app_state = AppState::new(store.clone())
            .with_heartbeat_interval(health_interval_secs as u32)
            .with_transport(transport_handle.clone());

        // join 응답의 worker.toml에 채워 넣을 이 orchestrator의 공개 URL.
        // 미설정이면 렌더링된 worker.toml의 orchestrator_url이 플레이스홀더로
        // 남아 운영자가 수동으로 채워야 한다 (기존 동작, 하위 호환).
        if let Ok(base_url) = std::env::var("FLEET_BASE_URL") {
            let base_url = base_url.trim();
            if !base_url.is_empty() {
                app_state = app_state.with_public_base_url(base_url);
            }
        }

        // Phase 8.6: master key 로드 (credentials 암호화용).
        // FLEET_MASTER_KEY env 또는 /etc/fleet/master.key 파일에서.
        // 로드 실패 시 credentials API 엔드포인트가 503 반환 (다른 API는 정상 동작).
        match fleet_credentials::MasterKey::load_with_paths(
            fleet_credentials::ENV_VAR,
            fleet_credentials::DEFAULT_KEY_FILE,
        ) {
            Ok(key) => {
                tracing::info!(
                    "master key loaded — worker credentials API enabled ({} env or {} file)",
                    fleet_credentials::ENV_VAR,
                    fleet_credentials::DEFAULT_KEY_FILE
                );
                app_state = app_state.with_master_key(key);
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "master key not available — worker credentials API will return 503 \
                     (set {} env or create {} to enable)",
                    fleet_credentials::ENV_VAR,
                    fleet_credentials::DEFAULT_KEY_FILE
                );
            }
        }

        // CORS allow-list (선택). FLEET_API_CORS_ORIGINS="https://a.example,https://b.example"
        // 미설정이면 CORS 비활성 — fleet-api의 정상 소비자는 비브라우저 클라이언트.
        if let Ok(raw) = std::env::var("FLEET_API_CORS_ORIGINS") {
            let origins: Vec<String> = raw
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !origins.is_empty() {
                tracing::info!(origins = ?origins, "HTTP API CORS allow-list configured");
                app_state = app_state.with_cors_origins(origins);
            }
        }

        if let Some(aud) = cf_audience.filter(|aud| !aud.trim().is_empty()) {
            app_state = app_state.with_cf_audience(aud);
            tracing::info!(bind = %bind, aud = %aud, "HTTP API server with Cloudflare Access auth");
        }
        if let Some(token_list) = scoped_tokens {
            if !token_list.is_empty() {
                // 로드맵 #72 — env FLEET_API_TOKENS를 DB(admin_api_tokens)로 1회
                // 자동 upsert(멱등, 원문 재노출 없음). 실패해도 기동은 계속한다 —
                // env 목록은 with_tokens()로 여전히 인증에 쓰인다.
                if let Err(e) = sync_env_admin_tokens_to_store(&*store, &token_list).await {
                    tracing::warn!(
                        error = %e,
                        "failed to sync FLEET_API_TOKENS into DB (로드맵 #72) — \
                         continuing with env-only bearer auth"
                    );
                }
                app_state = app_state.with_tokens(token_list);
                tracing::info!(bind = %bind, "HTTP API server with bearer auth");
            } else if !has_cf_access {
                tracing::warn!(bind = %bind, "HTTP API server in NO-AUTH mode (empty token list)");
            }
        } else if !has_cf_access {
            tracing::warn!(bind = %bind, "HTTP API server in NO-AUTH mode (dev only)");
        }

        // 로드맵 #80 — DB에 admin API 토큰이 하나도 없으면(위 sync가 방금 채운
        // 경우 제외) 최초 1개를 발급해 0600 파일로 남긴다. `fleet
        // admin-tokens create`/`fleet token issue`는 둘 다 기존 bearer를
        // 요구하므로, 이게 없으면 API로는 최초 admin 토큰을 만들 방법이
        // 없었다. 저널/표준출력에는 원문을 남기지 않는다 — 로그는 영구
        // 보존되는 경우가 많아 dashboard OTP(`tracing::info!`)와 달리 이
        // 값은 파일로만 1회 넘긴다.
        match issue_admin_bootstrap_token_if_needed(&*store).await {
            Ok(Some(token)) => match write_admin_bootstrap_token_file(&token) {
                Ok(path) => {
                    tracing::info!(
                        path = %path.display(),
                        "issued first-run admin bootstrap token (full capability); \
                         read it once from this file and rotate/revoke it afterward"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "issued admin bootstrap token but failed to write it to disk — \
                         it is now stranded in the DB with no way to retrieve the plaintext; \
                         revoke principal_id 'bootstrap' and re-issue via a working token file path"
                    );
                }
            },
            Ok(None) => {
                tracing::debug!(
                    "admin_api_tokens already has at least one entry — no bootstrap token issued"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "admin bootstrap token check failed (continuing)");
            }
        }

        let app_state = Arc::new(app_state);
        let http_join = tokio::spawn(async move {
            if let Err(e) = run_http_server(app_state, bind).await {
                tracing::error!(error = %e, "HTTP API server terminated with error");
            }
        });
        Some(http_join)
    } else {
        tracing::info!("HTTP API server disabled (pass --http-bind ADDR:PORT to enable)");
        None
    };

    // 웹 대시보드 서버 (옵션). --dashboard-bind가 지정된 경우에만 실행.
    // 부트 시 RBAC 시드 + admin bootstrap OTP 자동 발급.
    let _dashboard_handle = if let Some(bind_str) = dashboard_bind {
        let bind: SocketAddr = bind_str
            .parse()
            .with_context(|| format!("invalid --dashboard-bind address: {bind_str}"))?;

        // RBAC 시드 (idempotent) + 부트스트랩 OTP 자동 발급 (users 테이블 비어있을 때).
        match fleet_store::seed_rbac_and_maybe_issue_bootstrap(&*store).await {
            Ok(Some(token)) => {
                tracing::info!("═══════════════════════════════════════════════════════");
                tracing::info!(
                    "  ADMIN BOOTSTRAP TOKEN (use at https://fleet.agentthread.dev/bootstrap):"
                );
                tracing::info!("  {}", token);
                tracing::info!("═══════════════════════════════════════════════════════");
            }
            Ok(None) => {
                tracing::debug!("RBAC seeded; existing bootstrap token or users present");
            }
            Err(e) => {
                tracing::warn!(error = %e, "RBAC seed/bootstrap issue failed (continuing)");
            }
        }

        let dashboard_state = Arc::new(fleet_dashboard::DashboardState::new(
            store.clone(),
            store.pool().clone(),
            Some(dispatcher.clone()),
        ));
        tracing::info!(bind = %bind, "dashboard server starting");
        let dash_join = tokio::spawn(async move {
            if let Err(e) = fleet_dashboard::run_dashboard_server(dashboard_state, bind).await {
                tracing::error!(error = %e, "dashboard server terminated with error");
            }
        });
        Some(dash_join)
    } else {
        tracing::info!("dashboard disabled (pass --dashboard-bind ADDR:PORT to enable)");
        None
    };

    tracing::info!("starting MCP stdio server");
    run_mcp_server(state, dispatcher)
        .await
        .context("MCP server error")?;

    // MCP 서버 종료 시 백그라운드 태스크도 정리.
    if let Some(h) = _health_handle {
        h.abort().await;
    }
    if let Some(h) = _cleanup_handle {
        h.abort().await;
    }
    if let Some(h) = _reconcile_handle {
        h.abort().await;
    }
    if let Some(h) = _circuit_sync_handle {
        h.abort();
    }
    if let Some(h) = _http_handle {
        h.abort();
    }
    if let Some(h) = _dashboard_handle {
        h.abort();
    }
    Ok(())
}

/// JSON token manifest을 엄격하게 검증한다. 평면 쉼표 bearer 목록은 권한을 표현하지
/// 못하므로 더 이상 허용하지 않는다.
fn parse_scoped_api_tokens(raw: &str) -> Result<Vec<ApiTokenCredential>> {
    let tokens: Vec<ApiTokenCredential> = serde_json::from_str(raw)
        .context("FLEET_API_TOKENS must be a JSON array of {principal_id, token, capabilities}")?;
    if tokens.is_empty()
        || tokens.iter().any(|token| {
            token.principal_id.trim().is_empty()
                || token.token.trim().is_empty()
                || token.capabilities.is_empty()
        })
    {
        return Err(anyhow!(
            "each FLEET_API_TOKENS entry requires non-empty principal_id, token, and capabilities"
        ));
    }
    Ok(tokens)
}

/// `migrate` 명령.
pub async fn run_migrate() -> Result<()> {
    let _ = connect_and_migrate(1).await?;
    println!("migrations applied successfully");
    Ok(())
}

/// `workers` 명령 그룹 디스패치.
pub async fn run_workers(action: WorkersAction) -> Result<()> {
    match action {
        WorkersAction::List { status, json } => run_workers_list(status, json).await,
        WorkersAction::Show { name } => run_workers_show(&name).await,
        WorkersAction::Credential { action } => run_workers_credential(action).await,
    }
}

/// `workers credential` 명령 그룹 디스패치 (로드맵 #60 6단계).
async fn run_workers_credential(action: WorkerCredentialAction) -> Result<()> {
    match action {
        WorkerCredentialAction::Rotate {
            api_url,
            api_token,
            worker_id,
            expires_in_secs,
            json,
        } => {
            run_workers_credential_rotate(&api_url, &api_token, &worker_id, expires_in_secs, json)
                .await
        }
        WorkerCredentialAction::Revoke {
            api_url,
            api_token,
            worker_id,
        } => run_workers_credential_revoke(&api_url, &api_token, &worker_id).await,
    }
}

#[derive(Debug, serde::Serialize)]
struct RotateCredentialApiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_in_secs: Option<u64>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct RotateCredentialApiResponse {
    worker_id: String,
    operational_token: String,
    rotation_generation: i64,
    issued_at: String,
    expires_at: Option<String>,
}

/// `workers credential rotate <worker_id>` — 새 operational credential 발급.
async fn run_workers_credential_rotate(
    api_url: &str,
    api_token: &str,
    worker_id: &str,
    expires_in_secs: Option<u64>,
    json: bool,
) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let url = format!(
        "{}/v1/workers/{}/credential/rotate",
        api_url.trim_end_matches('/'),
        worker_id
    );
    let resp = http
        .post(&url)
        .bearer_auth(api_token)
        .json(&RotateCredentialApiRequest { expires_in_secs })
        .send()
        .await
        .context("credential rotate request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("credential rotate failed: {status} — {text}"));
    }
    let parsed: RotateCredentialApiResponse =
        resp.json().await.context("parsing rotate response")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&parsed)?);
        return Ok(());
    }
    println!("worker_id:           {}", parsed.worker_id);
    println!("rotation_generation: {}", parsed.rotation_generation);
    println!("issued_at:           {}", parsed.issued_at);
    println!(
        "expires_at:          {}",
        parsed.expires_at.as_deref().unwrap_or("(none)")
    );
    println!("operational_token:   {}", parsed.operational_token);
    println!("(store this token in worker.toml now — it will not be shown again)");
    Ok(())
}

/// `workers credential revoke <worker_id>` — operational credential 즉시 회수.
async fn run_workers_credential_revoke(
    api_url: &str,
    api_token: &str,
    worker_id: &str,
) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let url = format!(
        "{}/v1/workers/{}/credential",
        api_url.trim_end_matches('/'),
        worker_id
    );
    let resp = http
        .delete(&url)
        .bearer_auth(api_token)
        .send()
        .await
        .context("credential revoke request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("credential revoke failed: {status} — {text}"));
    }
    println!("revoked: worker {worker_id}");
    Ok(())
}

/// `admin-tokens` 명령 그룹 디스패치 (로드맵 #72).
pub async fn run_admin_tokens(action: AdminTokensAction) -> Result<()> {
    match action {
        AdminTokensAction::Create {
            api_url,
            api_token,
            principal_id,
            capabilities,
            json,
        } => {
            run_admin_tokens_create(&api_url, &api_token, &principal_id, &capabilities, json).await
        }
        AdminTokensAction::Rotate {
            api_url,
            api_token,
            principal_id,
            json,
        } => run_admin_tokens_rotate(&api_url, &api_token, &principal_id, json).await,
        AdminTokensAction::Revoke {
            api_url,
            api_token,
            principal_id,
        } => run_admin_tokens_revoke(&api_url, &api_token, &principal_id).await,
        AdminTokensAction::List {
            api_url,
            api_token,
            json,
        } => run_admin_tokens_list(&api_url, &api_token, json).await,
    }
}

#[derive(Debug, serde::Serialize)]
struct CreateAdminTokenApiRequest {
    principal_id: String,
    capabilities: Vec<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct AdminTokenApiResponse {
    principal_id: String,
    #[serde(default)]
    token: Option<String>,
    capabilities: Vec<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    rotated_at: Option<String>,
    #[serde(default)]
    revoked: Option<bool>,
    rotation_generation: i64,
}

fn print_admin_token_response(parsed: &AdminTokenApiResponse, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(parsed)?);
        return Ok(());
    }
    println!("principal_id:        {}", parsed.principal_id);
    println!("capabilities:        {}", parsed.capabilities.join(","));
    println!("rotation_generation: {}", parsed.rotation_generation);
    if let Some(created_at) = &parsed.created_at {
        println!("created_at:          {created_at}");
    }
    if let Some(rotated_at) = &parsed.rotated_at {
        println!("rotated_at:          {rotated_at}");
    }
    if let Some(token) = &parsed.token {
        println!("token:               {token}");
        println!("(store this token now — it will not be shown again)");
    }
    Ok(())
}

/// `admin-tokens create <principal_id> --capabilities a,b,c`.
async fn run_admin_tokens_create(
    api_url: &str,
    api_token: &str,
    principal_id: &str,
    capabilities: &str,
    json: bool,
) -> Result<()> {
    let caps: Vec<String> = capabilities
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if caps.is_empty() {
        return Err(anyhow!("--capabilities must list at least one capability"));
    }
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let url = format!("{}/v1/admin/tokens", api_url.trim_end_matches('/'));
    let resp = http
        .post(&url)
        .bearer_auth(api_token)
        .json(&CreateAdminTokenApiRequest {
            principal_id: principal_id.to_string(),
            capabilities: caps,
        })
        .send()
        .await
        .context("admin token create request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("admin token create failed: {status} — {text}"));
    }
    let parsed: AdminTokenApiResponse = resp.json().await.context("parsing create response")?;
    print_admin_token_response(&parsed, json)
}

/// `admin-tokens rotate <principal_id>`.
async fn run_admin_tokens_rotate(
    api_url: &str,
    api_token: &str,
    principal_id: &str,
    json: bool,
) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let url = format!(
        "{}/v1/admin/tokens/{}/rotate",
        api_url.trim_end_matches('/'),
        principal_id
    );
    let resp = http
        .post(&url)
        .bearer_auth(api_token)
        .send()
        .await
        .context("admin token rotate request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("admin token rotate failed: {status} — {text}"));
    }
    let parsed: AdminTokenApiResponse = resp.json().await.context("parsing rotate response")?;
    print_admin_token_response(&parsed, json)
}

/// `admin-tokens revoke <principal_id>`.
async fn run_admin_tokens_revoke(api_url: &str, api_token: &str, principal_id: &str) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let url = format!(
        "{}/v1/admin/tokens/{}",
        api_url.trim_end_matches('/'),
        principal_id
    );
    let resp = http
        .delete(&url)
        .bearer_auth(api_token)
        .send()
        .await
        .context("admin token revoke request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("admin token revoke failed: {status} — {text}"));
    }
    println!("revoked: principal {principal_id}");
    Ok(())
}

/// `admin-tokens list`.
async fn run_admin_tokens_list(api_url: &str, api_token: &str, json: bool) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let url = format!("{}/v1/admin/tokens", api_url.trim_end_matches('/'));
    let resp = http
        .get(&url)
        .bearer_auth(api_token)
        .send()
        .await
        .context("admin token list request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("admin token list failed: {status} — {text}"));
    }
    let parsed: Vec<AdminTokenApiResponse> = resp.json().await.context("parsing list response")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&parsed)?);
        return Ok(());
    }
    if parsed.is_empty() {
        println!("(no admin tokens issued)");
        return Ok(());
    }
    println!(
        "{:<24} {:<10} {:<10} CAPABILITIES",
        "PRINCIPAL", "GEN", "REVOKED"
    );
    for t in &parsed {
        println!(
            "{:<24} {:<10} {:<10} {}",
            t.principal_id,
            t.rotation_generation,
            t.revoked.unwrap_or(false),
            t.capabilities.join(",")
        );
    }
    Ok(())
}

/// `tasks` 명령 그룹 디스패치.
pub async fn run_tasks(action: TasksAction) -> Result<()> {
    match action {
        TasksAction::List {
            status,
            limit,
            json,
        } => run_tasks_list(status, limit, json).await,
        TasksAction::Show { id } => run_tasks_show(&id).await,
        TasksAction::Cancel { id, reason } => run_tasks_cancel(&id, reason).await,
        TasksAction::Submit {
            prompt,
            skills,
            model,
            server_hint,
            priority,
            max_turns,
            timeout_secs,
            required_labels,
            cwd,
            created_by,
            project_id,
            json,
        } => {
            run_tasks_submit(
                prompt,
                skills,
                model,
                server_hint,
                priority,
                max_turns,
                timeout_secs,
                required_labels,
                cwd,
                created_by,
                project_id,
                json,
            )
            .await
        }
    }
}

/// `events` 명령 그룹 디스패치. 감사 로그 조회.
pub async fn run_events(action: EventsAction) -> Result<()> {
    match action {
        EventsAction::List {
            after_seq,
            limit,
            json,
        } => run_events_list(after_seq, limit, json).await,
    }
}

/// `events list` 명령.
async fn run_events_list(after_seq: u64, limit: u32, json: bool) -> Result<()> {
    let store = connect_and_migrate(2).await?;
    let events = store
        .list_events(after_seq, limit)
        .await
        .context("failed to list events")?;

    if json {
        let value = serde_json::to_string_pretty(&events)?;
        println!("{value}");
        return Ok(());
    }

    if events.is_empty() {
        println!("(no events in range)");
        return Ok(());
    }

    println!("{:<8} {:<24} {:<22} DETAIL", "SEQ", "TIMESTAMP", "TYPE");
    println!("{}", "-".repeat(100));
    for e in events {
        let type_str = event_type_str(&e.event);
        let detail = event_detail_str(&e.event);
        let ts = chrono::Utc::now(); // 이벤트 자체 timestamp가 없으면 현재 시간.
        println!(
            "{:<8} {:<24} {:<22} {}",
            e.seq,
            ts.to_rfc3339(),
            type_str,
            detail,
        );
    }
    Ok(())
}

fn event_type_str(e: &fleet_core::FleetEvent) -> &'static str {
    use fleet_core::FleetEvent;
    match e {
        FleetEvent::TaskCreated { .. } => "task_created",
        FleetEvent::TaskDispatched { .. } => "task_dispatched",
        FleetEvent::TaskProgress { .. } => "task_progress",
        FleetEvent::TaskCompleted { .. } => "task_completed",
        FleetEvent::TaskFailed { .. } => "task_failed",
        FleetEvent::TaskCancelled { .. } => "task_cancelled",
        FleetEvent::WorkerJoined { .. } => "worker_joined",
        FleetEvent::WorkerLeft { .. } => "worker_left",
        FleetEvent::WorkerCircuitChanged { .. } => "worker_circuit_changed",
        FleetEvent::WorkerHeartbeat { .. } => "worker_heartbeat",
    }
}

fn event_detail_str(e: &fleet_core::FleetEvent) -> String {
    let json = serde_json::to_value(e).unwrap_or_default();
    let obj = json.as_object().cloned().unwrap_or_default();
    // type 필드는 이미 TYPE 컬럼에 표시되므로 detail에서는 제외.
    let mut parts = Vec::new();
    for (k, v) in &obj {
        if k == "type" {
            continue;
        }
        parts.push(format!("{k}={v}"));
    }
    parts.join(" ")
}

/// `workers list` 명령.
async fn run_workers_list(status_filter: Option<String>, json: bool) -> Result<()> {
    let store = connect_and_migrate(2).await?;

    let mut filter = WorkerFilter::default();
    if let Some(s) = status_filter {
        filter.status = Some(parse_status(&s)?);
    }

    let workers = store
        .list_workers(&filter)
        .await
        .context("failed to list workers")?;

    if json {
        let value = serde_json::to_string_pretty(&workers)?;
        println!("{value}");
        return Ok(());
    }

    if workers.is_empty() {
        println!("(no workers registered)");
        return Ok(());
    }

    println!(
        "{:<36} {:<20} {:<14} {:<10} {:<10}",
        "ID", "NAME", "STATUS", "ACTIVE", "CIRCUIT"
    );
    println!("{}", "-".repeat(96));
    for w in workers {
        println!(
            "{:<36} {:<20} {:<14} {:<10} {:<10}",
            w.id.to_string(),
            truncate(&w.name, 20),
            format!("{:?}", w.status).to_lowercase(),
            format!("{}/{}", w.active_tasks, w.max_concurrent),
            format!("{:?}", w.circuit_state).to_lowercase(),
        );
    }
    Ok(())
}

/// `workers show <name>` 명령.
async fn run_workers_show(name: &str) -> Result<()> {
    let store = connect_and_migrate(2).await?;
    let w = store
        .get_worker_by_name(name)
        .await
        .with_context(|| format!("failed to look up worker {name}"))?
        .ok_or_else(|| anyhow!("no worker named '{name}'"))?;

    println!("{:<20} {}", "ID:", w.id);
    println!("{:<20} {}", "NAME:", w.name);
    println!("{:<20} {}", "ENDPOINT:", w.endpoint);
    println!(
        "{:<20} {}",
        "STATUS:",
        format!("{:?}", w.status).to_lowercase()
    );
    println!("{:<20} {}/{}", "ACTIVE:", w.active_tasks, w.max_concurrent);
    println!(
        "{:<20} {}",
        "CIRCUIT:",
        format!("{:?}", w.circuit_state).to_lowercase()
    );
    println!(
        "{:<20} {}",
        "LAST_SEEN:",
        w.last_seen
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| "(never)".into())
    );
    println!("{:<20} {}", "REGISTERED_AT:", w.registered_at.to_rfc3339());
    println!("{:<20} {:?}", "LABELS:", w.labels);
    Ok(())
}

/// `tasks list` 명령.
async fn run_tasks_list(status_filter: Option<String>, limit: usize, json: bool) -> Result<()> {
    let store = connect_and_migrate(2).await?;
    let mut filter = TaskFilter {
        limit,
        ..Default::default()
    };
    if let Some(s) = status_filter {
        filter.status = Some(parse_task_status_filter(&s)?);
    }

    let tasks = store
        .list_tasks(&filter)
        .await
        .context("failed to list tasks")?;

    if json {
        let value = serde_json::to_string_pretty(&tasks)?;
        println!("{value}");
        return Ok(());
    }

    if tasks.is_empty() {
        println!("(no tasks match)");
        return Ok(());
    }

    println!(
        "{:<38} {:<12} {:<24} {:<20} {:<8}",
        "ID", "PHASE", "CREATED_AT", "CREATED_BY", "PROMPT"
    );
    println!("{}", "-".repeat(110));
    for t in tasks {
        let phase = phase_str(&t.status);
        let prompt = truncate(&t.prompt, 20);
        println!(
            "{:<38} {:<12} {:<24} {:<20} {:<8}",
            t.id.to_string(),
            phase,
            t.created_at.to_rfc3339(),
            truncate(&t.created_by, 20),
            prompt,
        );
    }
    Ok(())
}

/// `tasks show <id>` 명령.
async fn run_tasks_show(id_str: &str) -> Result<()> {
    let store = connect_and_migrate(2).await?;
    let id: TaskId = id_str
        .parse()
        .with_context(|| format!("invalid task id '{id_str}' (expected UUID)"))?;
    let t = store
        .get_task(id)
        .await
        .with_context(|| format!("failed to look up task {id}"))?
        .ok_or_else(|| anyhow!("no task with id {id}"))?;

    let phase = phase_str(&t.status);
    println!("{:<20} {}", "ID:", t.id);
    println!("{:<20} {}", "PHASE:", phase);
    println!("{:<20} {}", "PROMPT:", truncate(&t.prompt, 60));
    println!("{:<20} {}", "CREATED_BY:", t.created_by);
    println!("{:<20} {}", "CREATED_AT:", t.created_at.to_rfc3339());
    if let Some(hint) = &t.server_hint {
        println!("{:<20} {hint}", "SERVER_HINT:");
    }
    match &t.status {
        TaskStatus::Dispatched {
            worker_id,
            started_at,
        } => {
            println!("{:<20} {worker_id}", "WORKER_ID:");
            println!("{:<20} {}", "STARTED_AT:", started_at.to_rfc3339());
        }
        TaskStatus::Completed(r) => {
            println!("{:<20} {}", "WORKER_ID:", r.worker_id);
            println!("{:<20} {}", "EXIT_CODE:", r.exit_code);
            println!("{:<20} {:.2}s", "DURATION:", r.duration_secs);
            if let Some(usage) = &r.token_usage {
                println!(
                    "{:<20} in={} out={}",
                    "TOKENS:", usage.input_tokens, usage.output_tokens
                );
            }
            // 2026-08-11: 예전엔 output 텍스트를 아예 출력하지 않아, CLI로는
            // 태스크가 "completed"라는 것 외에 실제 응답 내용을 확인할 방법이
            // 없었다. r.output이 비어 있으면(구버전 데이터, 또는 극히 드문 케이스)
            // task_outputs 테이블(스트리밍 청크)을 폴백으로 조회한다.
            let output_text = if !r.output.is_empty() {
                Some(r.output.clone())
            } else {
                store
                    .get_output(id, 0)
                    .await
                    .ok()
                    .filter(|o| !o.chunks.is_empty())
                    .map(|o| o.chunks.into_iter().map(|c| c.chunk).collect::<String>())
            };
            println!("{:-<20}", "OUTPUT:");
            match output_text {
                Some(text) if !text.is_empty() => println!("{text}"),
                _ => println!("(no output captured)"),
            }
        }
        TaskStatus::Failed(f) => {
            if let Some(w) = &f.worker_id {
                println!("{:<20} {w}", "WORKER_ID:");
            }
            println!("{:<20} {}", "ERROR:", f.error);
            println!("{:<20} {:?}", "KIND:", f.kind);
        }
        TaskStatus::Cancelled {
            reason,
            cancelled_at,
        } => {
            println!("{:<20} {reason}", "REASON:");
            println!("{:<20} {}", "CANCELLED_AT:", cancelled_at.to_rfc3339());
        }
        TaskStatus::Pending => {}
    }
    Ok(())
}

/// `tasks cancel <id>` 명령.
async fn run_tasks_cancel(id_str: &str, reason: Option<String>) -> Result<()> {
    let store = connect_and_migrate(2).await?;
    let id: TaskId = id_str
        .parse()
        .with_context(|| format!("invalid task id '{id_str}'"))?;
    let reason = reason.unwrap_or_else(|| "manual cancel".into());
    let cancelled_at = chrono::Utc::now();
    let t = store.get_task(id).await?;
    let task = t.ok_or_else(|| anyhow!("no task with id {id}"))?;
    if task.is_terminal() {
        return Err(anyhow!(
            "task {id} is already in terminal state ({})",
            phase_str(&task.status)
        ));
    }
    let new_status = TaskStatus::Cancelled {
        reason: reason.clone(),
        cancelled_at,
    };
    store
        .update_task_status(id, &new_status)
        .await
        .context("failed to update task status")?;
    println!("task {id} cancelled (reason: {reason})");
    Ok(())
}

/// `tasks submit <prompt>` 명령.
///
/// DB에 직접 Pending 태스크를 생성한다. 실행 중인 `fleet serve`의 Reconciler/
/// Dispatcher가 `reconcile-interval`마다 Pending 작업을 스캔해 자동으로 워커에
/// 디스패치한다 (`--reconcile-max-dispatch-retries > 0`인 기본 배포에서는 즉시
/// 재시도가 활성화되어 있으므로 수 초 이내에 Dispatched 상태로 전환된다).
///
/// 스킬 이름이 지정된 경우 `FLEET_SKILLS_DIR` 또는 `~/.config/grok-fleet/skills/`에서
/// `<name>.md` 파일을 읽어 프롬프트 앞에 XML 블록으로 주입한 뒤 저장한다.
#[allow(clippy::too_many_arguments)]
async fn run_tasks_submit(
    prompt: String,
    skills: Vec<String>,
    model: Option<String>,
    server_hint: Option<String>,
    priority: String,
    max_turns: Option<u32>,
    timeout_secs: Option<u64>,
    required_labels: Vec<String>,
    cwd: Option<String>,
    created_by: String,
    project_id: Option<String>,
    json_output: bool,
) -> Result<()> {
    let store = connect_and_migrate(2).await?;

    // 스킬 주입: skills 목록이 비어 있으면 prompt를 그대로 사용.
    let final_prompt = if skills.is_empty() {
        prompt.clone()
    } else {
        fleet_scheduler::skill_loader::inject_skills(&prompt, &skills)
    };

    let task_priority = match priority.to_lowercase().as_str() {
        "low" => fleet_core::TaskPriority::Low,
        "high" => fleet_core::TaskPriority::High,
        _ => fleet_core::TaskPriority::Normal,
    };
    let project_id = project_id
        .filter(|value| !value.is_empty())
        .map(|value| value.parse::<fleet_core::ProjectId>())
        .transpose()
        .context("project_id must be a UUID")?;

    let task = fleet_core::Task::from_request(fleet_core::TaskRequest {
        prompt: final_prompt,
        cwd: cwd.filter(|s| !s.is_empty()),
        model: model.filter(|s| !s.is_empty()),
        server_hint: server_hint.filter(|s| !s.is_empty()),
        required_labels,
        max_turns,
        timeout_secs,
        priority: task_priority,
        created_by,
        project_id,
        skills_required: skills.clone(),
        ..Default::default()
    });
    let task_id = task.id;

    store
        .insert_task(&task)
        .await
        .context("failed to insert task")?;

    // 스킬 주입 결과 요약 출력.
    if json_output {
        println!(
            "{}",
            serde_json::json!({
                "task_id": task_id,
                "status": "pending",
                "skills_injected": skills,
            })
        );
    } else {
        println!("task {task_id} submitted (status: pending)");
        if !skills.is_empty() {
            println!("  skills injected: {}", skills.join(", "));
        }
        println!("  tip: use `fleet tasks show {task_id}` to track status");
    }

    Ok(())
}

fn parse_task_status_filter(s: &str) -> Result<TaskStatusFilter> {
    match s.to_lowercase().as_str() {
        "pending" => Ok(TaskStatusFilter::Pending),
        "dispatched" => Ok(TaskStatusFilter::Dispatched),
        "completed" => Ok(TaskStatusFilter::Completed),
        "failed" => Ok(TaskStatusFilter::Failed),
        "cancelled" => Ok(TaskStatusFilter::Cancelled),
        "terminal" => Ok(TaskStatusFilter::Terminal),
        "active" => Ok(TaskStatusFilter::Active),
        other => Err(anyhow!(
            "invalid status '{other}': expected pending, dispatched, completed, failed, cancelled, terminal, or active"
        )),
    }
}

fn phase_str(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Pending => "pending",
        TaskStatus::Dispatched { .. } => "dispatched",
        TaskStatus::Completed(_) => "completed",
        TaskStatus::Failed(_) => "failed",
        TaskStatus::Cancelled { .. } => "cancelled",
    }
}

fn parse_status(s: &str) -> Result<WorkerStatus> {
    match s.to_lowercase().as_str() {
        "online" => Ok(WorkerStatus::Online),
        "degraded" => Ok(WorkerStatus::Degraded),
        "offline" => Ok(WorkerStatus::Offline),
        "circuit_open" => Ok(WorkerStatus::CircuitOpen),
        other => Err(anyhow!(
            "invalid status '{other}': expected online, degraded, offline, or circuit_open"
        )),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n.saturating_sub(1)])
    }
}

// (Worker type reserved for future --register-worker flag in Phase 2.)

// ═══════════════════════════════════════════════════════════════════════
//  provision 명령 (Phase 4)
// ═══════════════════════════════════════════════════════════════════════

/// CLI에서 전달된 provision 인자.
pub struct ProvisionArgs {
    pub host: Option<String>,
    pub user: String,
    pub ssh_port: u16,
    pub ssh_key: Option<String>,
    pub name: Option<String>,
    pub labels: Vec<String>,
    pub cf_token: Option<String>,
    pub orchestrator_url: Option<String>,
    pub fleet_worker_bin: Option<String>,
    pub grok_secret: Option<String>,
    pub bootstrap_token: Option<String>,
    pub api_token: Option<String>,
    pub inventory: Option<String>,
    pub parallel: usize,
    pub tags: Vec<String>,
    pub only: Vec<String>,
    pub dry_run: bool,
    // ── SSH 호스트 키 검증 ──────────────────────────────────────────────
    /// 서버 호스트 키 검증 정책 (CLI 명시값). None 이면 inventory defaults,
    /// 그것도 없으면 기본값(TOFU) 사용. OpenSSH accept-new 에 대응.
    pub host_key_policy: Option<HostKeyPolicy>,
    /// known_hosts 파일 경로 (CLI 명시값). None 이면 inventory defaults,
    /// 그것도 없으면 `~/.ssh/known_hosts`.
    pub known_hosts: Option<PathBuf>,
    // ── mTLS (Phase 8.5) ────────────────────────────────────────────────
    /// mTLS 종단 proxy 활성화. 다른 mtls_* 필드는 이 값이 true 인 경우에만 사용됨.
    pub mtls_enabled: bool,
    pub mtls_listen_addr: Option<String>,
    pub mtls_server_cert_path: Option<String>,
    pub mtls_server_key_path: Option<String>,
    pub mtls_client_ca_path: Option<String>,
    pub mtls_advertised_host: Option<String>,
    pub mtls_advertised_port: Option<u16>,
}

/// `provision` 명령 실행.
pub async fn run_provision(args: ProvisionArgs) -> Result<()> {
    // 단일 호스트 모드 vs 인벤토리 모드 분기.
    if let Some(inv_path) = &args.inventory {
        run_provision_inventory(inv_path, &args).await
    } else if let Some(host) = &args.host {
        run_provision_single(host, &args).await
    } else {
        Err(anyhow!(
            "either --host or --inventory must be specified. \
             Run `fleet provision --help` for usage."
        ))
    }
}

/// CLI / inventory defaults 에서 최종 호스트 키 검증 구성을 결정.
///
/// 우선순위 (정책과 경로 각각 독립 적용):
/// 1. CLI 명시값 (`--host-key-policy`, `--known-hosts`)
/// 2. inventory defaults (`host_key_policy`, `known_hosts`)
/// 3. 기본값 — 정책 TOFU, 경로 `~/.ssh/known_hosts`
fn resolve_host_key_config(
    args: &ProvisionArgs,
    defaults: Option<&fleet_provisioner::InventoryDefaults>,
) -> HostKeyConfig {
    // 정책: CLI > inventory > 기본(Tofu)
    let policy = args.host_key_policy.or_else(|| {
        defaults
            .and_then(|d| d.host_key_policy.as_deref())
            .and_then(|s| match HostKeyPolicy::parse(s) {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "invalid host_key_policy in inventory defaults; using default"
                    );
                    None
                }
            })
    });

    // 경로: CLI > inventory > 기본(HostKeyConfig::effective_known_hosts 가 처리)
    let explicit_path = args.known_hosts.clone().or_else(|| {
        defaults
            .and_then(|d| d.known_hosts.clone())
            .map(PathBuf::from)
    });

    match (policy, explicit_path) {
        (Some(p), Some(path)) => HostKeyConfig::new(p).with_known_hosts(path),
        (Some(p), None) => HostKeyConfig::new(p),
        (None, Some(path)) => HostKeyConfig::default().with_known_hosts(path),
        (None, None) => HostKeyConfig::default(),
    }
}

/// 단일 호스트 프로비저닝.
async fn run_provision_single(host: &str, args: &ProvisionArgs) -> Result<()> {
    let name = args
        .name
        .clone()
        .ok_or_else(|| anyhow!("--name is required in single-host mode"))?;
    let ssh_key = args
        .ssh_key
        .clone()
        .ok_or_else(|| anyhow!("--ssh-key is required in single-host mode"))?;

    tracing::info!(%host, %name, %args.user, "single-host provisioning");

    let labels = parse_labels(&args.labels)?;
    let ctx = build_step_context(
        &name,
        labels,
        args.orchestrator_url.as_deref(),
        args.cf_token.as_deref(),
        args.fleet_worker_bin.as_deref(),
        args.grok_secret.as_deref(),
        args.bootstrap_token.as_deref(),
        args.dry_run,
        args,
    );

    let report = if args.dry_run {
        tracing::info!("dry-run mode: no SSH connection, simulating");
        let mock = MockExecutor::new();
        run_playbook(&mock, &ctx, &args.tags).await?
    } else {
        let connect_info =
            SshConnectInfo::new(host, &args.user, PathBuf::from(&ssh_key)).with_port(args.ssh_port);
        let host_key = resolve_host_key_config(args, None);
        let ssh = SshClient::connect(connect_info, host_key)
            .await
            .context("SSH connection failed")?;
        run_playbook(&ssh, &ctx, &args.tags).await?
    };

    print_report(&report);

    // 프로비저닝 결과를 오케스트레이터에 등록 (호스트 인벤토리).
    if !args.dry_run {
        if let Some(url) = &args.orchestrator_url {
            register_host_with_orchestrator(
                url,
                host,
                args.ssh_port.into(),
                &args.user,
                report.succeeded,
                args.api_token.as_deref(),
            )
            .await;
        }
    }

    Ok(())
}

/// 프로비저닝 완료 후 오케스트레이터에 호스트를 등록 (best-effort).
///
/// `hostname`은 `--name`(워커 논리 이름 — provision 실행마다, 또는
/// `--tags credentials`처럼 워커 재사용 목적으로 다르게 줄 수 있음)이 아니라
/// `ssh_host`(접속 대상)에서 도메인 접미사를 제거해 도출한다. 과거엔 `--name`을
/// 그대로 hostname으로 보내서, 최초 프로비저닝(`--name oci-yarangdev-arm1`)과
/// 이후 재실행(예: `--name worker-arm1`으로 자격증명만 재적용)이 서로 다른
/// hostname으로 `hosts` 테이블에 등록되어 `ON CONFLICT (hostname)` upsert가
/// 매칭되지 못하고 동일 머신이 중복 행으로 쌓였다.
async fn register_host_with_orchestrator(
    orchestrator_url: &str,
    ssh_host: &str,
    ssh_port: i32,
    ssh_user: &str,
    succeeded: bool,
    api_token: Option<&str>,
) {
    let hostname = ssh_host.split('.').next().unwrap_or(ssh_host);
    let url = format!(
        "{}/v1/hosts/register",
        orchestrator_url.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "hostname": hostname,
        "ssh_host": ssh_host,
        "ssh_port": ssh_port,
        "ssh_user": ssh_user,
        "succeeded": succeeded,
        "message": if succeeded { "provisioning completed" } else { "provisioning failed" },
    });

    let client = reqwest::Client::new();
    let mut req = client.post(&url).json(&body);
    if let Some(token) = api_token {
        req = req.bearer_auth(token);
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!(%hostname, "host registered with orchestrator");
        }
        Ok(resp) => {
            tracing::warn!(
                status = %resp.status(),
                %hostname,
                "host registration returned non-OK (best-effort)"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, %hostname, "host registration failed (best-effort)");
        }
    }
}

/// 인벤토리 파일 기반 일괄 프로비저닝.
async fn run_provision_inventory(inv_path: &str, args: &ProvisionArgs) -> Result<()> {
    let inv = Inventory::from_file(inv_path)
        .with_context(|| format!("failed to load inventory from {inv_path}"))?;
    tracing::info!(
        workers = inv.workers.len(),
        parallel = args.parallel,
        dry_run = args.dry_run,
        "inventory loaded"
    );

    // CLI의 --only, --tags, --dry-run이 인벤토리 options를 오버라이드.
    let mut options = inv.options.clone();
    if !args.only.is_empty() {
        options.only = args.only.clone();
    }
    if !args.tags.is_empty() {
        options.tags = args.tags.clone();
    }
    if args.dry_run {
        options.dry_run = true;
    }
    if let Some(p) = Some(args.parallel) {
        if p > 0 {
            options.parallel = p;
        }
    }
    // CLI --api-token 이 있으면 inventory options 보다 우선.
    if let Some(tok) = args.api_token.as_deref() {
        if !tok.is_empty() {
            options.api_token = Some(tok.to_string());
        }
    }

    let workers: Vec<InventoryWorker> = filter_workers(&inv, &options);
    if workers.is_empty() {
        tracing::warn!("no workers matched filters");
        return Ok(());
    }

    tracing::info!(matched = workers.len(), "workers to provision");

    let mut reports = Vec::new();
    if options.dry_run {
        // dry-run은 MockExecutor로 모든 워커 순차 시뮬레이션.
        for w in &workers {
            let ctx = build_inventory_step_context(w, &inv.defaults, &options);
            let mock = MockExecutor::new();
            match run_playbook(&mock, &ctx, &options.tags).await {
                Ok(r) => reports.push(r),
                Err(e) => {
                    tracing::error!(worker = %w.name, error = %e, "playbook failed");
                    reports.push(PlaybookReport {
                        worker_name: w.name.clone(),
                        steps: recover_completed_steps(&e),
                        succeeded: false,
                    });
                }
            }
        }
    } else {
        // 실제 SSH 병렬 실행.
        let parallel = options.parallel.max(1);
        let sem = Arc::new(tokio::sync::Semaphore::new(parallel));
        let mut handles = Vec::new();
        for w in workers {
            let sem = sem.clone();
            let ctx = build_inventory_step_context(&w, &inv.defaults, &options);
            let tags = options.tags.clone();
            let worker_name = w.name.clone();
            let ssh_key = w.effective_ssh_key(&inv.defaults)?.clone();
            let user = w.effective_user(&inv.defaults);
            let port = w.effective_ssh_port(&inv.defaults);
            let host = w.host.clone();
            let host_key = resolve_host_key_config(args, Some(&inv.defaults));

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore closed");
                tracing::info!(%worker_name, %host, "starting provisioning");
                let connect_info =
                    SshConnectInfo::new(&host, &user, PathBuf::from(&ssh_key)).with_port(port);
                match SshClient::connect(connect_info, host_key).await {
                    Ok(ssh) => match run_playbook(&ssh, &ctx, &tags).await {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::error!(worker = %worker_name, error = %e, "playbook failed");
                            PlaybookReport {
                                worker_name: worker_name.clone(),
                                steps: recover_completed_steps(&e),
                                succeeded: false,
                            }
                        }
                    },
                    Err(e) => {
                        tracing::error!(worker = %worker_name, error = %e, "SSH connection failed");
                        PlaybookReport {
                            worker_name: worker_name.clone(),
                            steps: vec![],
                            succeeded: false,
                        }
                    }
                }
            });
            handles.push(handle);
        }
        for h in handles {
            match h.await {
                Ok(r) => reports.push(r),
                Err(e) => tracing::error!(error = %e, "task panicked"),
            }
        }
    }

    // 요약 출력
    let succeeded = reports.iter().filter(|r| r.succeeded).count();
    let failed = reports.len() - succeeded;
    println!("\n{}", "=".repeat(60));
    println!(
        "Provisioning summary: {} succeeded, {} failed",
        succeeded, failed
    );
    for r in &reports {
        let mark = if r.succeeded { "✓" } else { "✗" };
        println!("  {mark} {}", r.worker_name);
    }
    println!("{}", "=".repeat(60));

    if failed > 0 {
        return Err(anyhow!("{failed} of {} workers failed", reports.len()));
    }
    Ok(())
}

/// Playbook을 실행하고 결과 반환. prereq를 추정(단순화 — ubuntu/x86_64 가정).
/// `run_playbook`이 실패했을 때 (`anyhow::Error`로 소거된) 부분 실행 이력을
/// 복원한다 (로드맵 #79). 실패가 `PlaybookError::StepFailed`가 아니면(예:
/// `run_playbook` 자체의 다른 오류) 빈 벡터로 폴백한다 — 이 경우는 애초에
/// 어떤 스텝도 실행되지 않았을 가능성이 높다.
fn recover_completed_steps(err: &anyhow::Error) -> Vec<fleet_provisioner::StepReport> {
    match err.downcast_ref::<PlaybookError>() {
        Some(PlaybookError::StepFailed {
            completed_steps, ..
        }) => completed_steps.clone(),
        _ => Vec::new(),
    }
}

/// admin bootstrap 토큰 파일의 기본 경로. `FLEET_ADMIN_BOOTSTRAP_TOKEN_FILE`로
/// 오버라이드 가능 (로드맵 #80).
const DEFAULT_ADMIN_BOOTSTRAP_TOKEN_FILE: &str = "/etc/fleet/bootstrap-admin-token";

/// admin bootstrap 토큰 파일 경로. `FLEET_ADMIN_BOOTSTRAP_TOKEN_FILE`로
/// 오버라이드 가능하다.
fn resolve_admin_bootstrap_token_path() -> PathBuf {
    std::env::var("FLEET_ADMIN_BOOTSTRAP_TOKEN_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_ADMIN_BOOTSTRAP_TOKEN_FILE))
}

/// admin bootstrap 토큰 원문을 `0600` 파일에 1회 쓴다.
fn write_admin_bootstrap_token_file(token: &str) -> std::io::Result<PathBuf> {
    let path = resolve_admin_bootstrap_token_path();
    write_admin_bootstrap_token_to_path(&path, token)?;
    Ok(path)
}

/// 경로를 명시적으로 받는 실제 쓰기 로직 — env var를 건드리지 않고
/// 단위 테스트할 수 있도록 [`write_admin_bootstrap_token_file`]에서 분리했다.
///
/// 이미 파일이 존재하면 **덮어쓰지 않고 에러를 반환한다** — 이 함수는
/// `issue_admin_bootstrap_token_if_needed`가 DB에 새로 발급한 토큰에 대해서만
/// 호출되므로, 이 시점에 파일이 이미 있다는 것은 이전 실행의 잔재이거나
/// 예상치 못한 충돌이다. 조용히 덮어쓰면 이전에 발급된(운영자가 이미
/// 회수했을 수도 있는) 값을 새 값으로 가장하게 된다.
fn write_admin_bootstrap_token_to_path(path: &std::path::Path, token: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    use std::io::Write;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;

    file.write_all(token.as_bytes())
}

async fn run_playbook(
    exec: &dyn RemoteExecutor,
    ctx: &PlaybookContext,
    tags: &[String],
) -> Result<PlaybookReport> {
    // 단순화: 실제 환경에서는 check_prereqs 결과를 받아 다음 스텝에 전달.
    // 여기서는 기본값(ubuntu, x86_64, 충분한 자원)을 가정.
    let assumed_prereq = PrereqReport {
        os: "ubuntu".into(),
        arch: "x86_64".into(),
        mem_mb: 16384,
        disk_gb: 100,
        has_rust: false,
        has_systemd: true,
    };
    let playbook = Playbook::standard(&assumed_prereq);
    let mut pb_ctx = ctx.clone();
    if !tags.is_empty() {
        pb_ctx = pb_ctx.with_tags(tags.to_vec());
    }
    Ok(playbook.run(exec, &pb_ctx).await?)
}

#[allow(clippy::too_many_arguments)]
fn build_step_context(
    name: &str,
    labels: std::collections::HashMap<String, String>,
    orchestrator_url: Option<&str>,
    cf_token: Option<&str>,
    fleet_worker_bin: Option<&str>,
    grok_secret: Option<&str>,
    bootstrap_token: Option<&str>,
    dry_run: bool,
    args: &ProvisionArgs,
) -> PlaybookContext {
    let base = StepContext {
        worker_name: name.to_string(),
        labels,
        orchestrator_url: orchestrator_url.unwrap_or("").to_string(),
        cf_token: cf_token.map(String::from),
        fleet_worker_bin: fleet_worker_bin.map(String::from),
        grok_secret: grok_secret.map(String::from),
        bootstrap_token: bootstrap_token.map(String::from),
        orchestrator_api_token: args.api_token.clone(),
        dry_run,
        mtls_enabled: args.mtls_enabled,
        mtls_listen_addr: args.mtls_listen_addr.clone(),
        mtls_server_cert_path: args.mtls_server_cert_path.clone(),
        mtls_server_key_path: args.mtls_server_key_path.clone(),
        mtls_client_ca_path: args.mtls_client_ca_path.clone(),
        mtls_advertised_host: args.mtls_advertised_host.clone(),
        mtls_advertised_port: args.mtls_advertised_port,
        ..Default::default()
    };
    PlaybookContext::new(base)
}

fn build_inventory_step_context(
    w: &InventoryWorker,
    defaults: &fleet_provisioner::InventoryDefaults,
    options: &ProvisionOptions,
) -> PlaybookContext {
    let cf_token = defaults.cf_token.clone();
    let mtls_enabled = w.effective_mtls_enabled(defaults);
    let base = StepContext {
        worker_name: w.name.clone(),
        labels: w.labels.clone(),
        orchestrator_url: options.orchestrator_url.clone().unwrap_or_default(),
        cf_token,
        fleet_worker_bin: None,
        grok_secret: w.grok_secret.clone(),
        bootstrap_token: options.bootstrap_token.clone(),
        orchestrator_api_token: options.api_token.clone(),
        dry_run: options.dry_run,
        // 로드맵 #37 — 인벤토리 모드 mTLS 설정 주입. cert/key는 워커별 필드에서만
        // 오고(defaults에는 없음), listen_addr/client_ca/advertised_port는
        // defaults와 워커별 오버라이드를 함께 본다. 실제로 이 필드가 비어있는데
        // mtls_enabled=true인 경우의 검증은 `templates.rs`의 렌더링 단계
        // (`StepError::Template("mtls_enabled=true requires ...")`)가 그대로
        // 담당한다 — 여기서 별도 검증을 중복하지 않는다.
        mtls_enabled,
        mtls_listen_addr: if mtls_enabled {
            w.effective_mtls_listen_addr(defaults)
        } else {
            None
        },
        mtls_server_cert_path: if mtls_enabled {
            w.mtls_server_cert.clone()
        } else {
            None
        },
        mtls_server_key_path: if mtls_enabled {
            w.mtls_server_key.clone()
        } else {
            None
        },
        mtls_client_ca_path: if mtls_enabled {
            w.effective_mtls_client_ca(defaults)
        } else {
            None
        },
        mtls_advertised_host: if mtls_enabled {
            Some(w.effective_mtls_advertised_host())
        } else {
            None
        },
        mtls_advertised_port: if mtls_enabled {
            w.effective_mtls_advertised_port(defaults)
        } else {
            None
        },
        ..Default::default()
    };
    PlaybookContext::new(base)
}

/// `fleet scan-host-keys` 인자.
pub struct ScanHostKeysArgs {
    pub host: Option<String>,
    pub ssh_port: u16,
    pub inventory: Option<String>,
    pub known_hosts: Option<PathBuf>,
    pub write: bool,
}

/// SSH 호스트 공개키 사전 수집 (`ssh-keyscan`과 동일한 목적, 로드맵 #39).
///
/// `--write` 없이는 지문(fingerprint)만 출력한다 — 운영자가 대역 밖 채널로
/// 검증한 뒤 다시 `--write`로 실행해야 known_hosts에 반영된다. 여러 호스트
/// 중 일부가 실패해도 나머지는 계속 스캔하고, 마지막에 실패 건수를 집계해
/// 0이 아니면 에러로 종료한다.
pub async fn run_scan_host_keys(args: ScanHostKeysArgs) -> Result<()> {
    let targets: Vec<(String, u16)> = if let Some(host) = &args.host {
        vec![(host.clone(), args.ssh_port)]
    } else if let Some(inv_path) = &args.inventory {
        let inv = Inventory::from_file(inv_path)
            .with_context(|| format!("failed to load inventory from {inv_path}"))?;
        inv.workers
            .iter()
            .map(|w| (w.host.clone(), w.effective_ssh_port(&inv.defaults)))
            .collect()
    } else {
        return Err(anyhow!("either --host or --inventory is required"));
    };

    if targets.is_empty() {
        println!("(no hosts to scan)");
        return Ok(());
    }

    let known_hosts_path = args.known_hosts.clone().or_else(default_known_hosts_path);
    if args.write && known_hosts_path.is_none() {
        return Err(anyhow!(
            "--write requires a known_hosts path — pass --known-hosts explicitly or set HOME"
        ));
    }

    let mut failed = 0usize;
    for (host, port) in &targets {
        match scan_host_key(host, *port).await {
            Ok(scanned) => {
                println!(
                    "{}:{}  {}  {}",
                    scanned.host, scanned.port, scanned.algorithm, scanned.fingerprint
                );
                if args.write {
                    let path = known_hosts_path.as_ref().expect("checked above");
                    match append_known_hosts_line(&scanned.known_hosts_line, path) {
                        Ok(()) => println!("  -> appended to {}", path.display()),
                        Err(e) => {
                            tracing::error!(%host, error = %e, "failed to write known_hosts entry");
                            eprintln!("  -> FAILED to write known_hosts entry: {e}");
                            failed += 1;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(%host, port, error = %e, "host key scan failed");
                eprintln!("{host}:{port}  SCAN FAILED: {e}");
                failed += 1;
            }
        }
    }

    if !args.write {
        println!();
        println!(
            "⚠️  위 지문을 대역 밖(클라우드 콘솔, 프로비저닝 로그 등) 채널로 반드시 검증한 뒤 사용하세요."
        );
        println!("검증 후 --write 플래그로 다시 실행하면 known_hosts에 기록됩니다.");
    }

    if failed > 0 {
        return Err(anyhow!(
            "{failed}/{} host(s) failed to scan or write",
            targets.len()
        ));
    }
    Ok(())
}

fn filter_workers(inv: &Inventory, options: &ProvisionOptions) -> Vec<InventoryWorker> {
    if options.only.is_empty() {
        inv.workers.clone()
    } else {
        inv.workers
            .iter()
            .filter(|w| options.only.iter().any(|n| n == &w.name))
            .cloned()
            .collect()
    }
}

fn parse_labels(labels: &[String]) -> Result<std::collections::HashMap<String, String>> {
    let mut map = std::collections::HashMap::new();
    for l in labels {
        let (k, v) = l
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid label '{l}': expected key=value"))?;
        map.insert(k.to_string(), v.to_string());
    }
    Ok(map)
}

fn print_report(report: &PlaybookReport) {
    println!("\n{}", "=".repeat(60));
    println!("Worker: {}", report.worker_name);
    println!(
        "Status: {}",
        if report.succeeded {
            "✓ success"
        } else {
            "✗ failed"
        }
    );
    for step in &report.steps {
        let mark = match &step.status {
            fleet_provisioner::StepStatus::Skipped => "→".to_string(),
            fleet_provisioner::StepStatus::Applied { message } => format!("✓ {message}"),
            fleet_provisioner::StepStatus::Failed { error } => format!("✗ {error}"),
        };
        println!("  {:<25} {mark}", step.name);
    }
    println!("{}", "=".repeat(60));
}

#[cfg(test)]
mod recover_completed_steps_tests {
    use super::*;
    use fleet_provisioner::{StepReport, StepStatus};

    #[test]
    fn extracts_completed_steps_from_step_failed_error() {
        let err: anyhow::Error = PlaybookError::StepFailed {
            step: "install_grok".into(),
            host: "worker-7".into(),
            source: Box::new(fleet_provisioner::StepError::UnsupportedOs("bsd".into())),
            completed_steps: vec![
                StepReport {
                    name: "check_prereqs".into(),
                    status: StepStatus::Skipped,
                },
                StepReport {
                    name: "install_deps".into(),
                    status: StepStatus::Applied {
                        message: "ok".into(),
                    },
                },
                StepReport {
                    name: "install_grok".into(),
                    status: StepStatus::Failed {
                        error: "unsupported OS".into(),
                    },
                },
            ],
        }
        .into();

        // 로드맵 #79 — 실패한 20대 인벤토리 중 7번째 워커가 어느 스텝에서
        // 멈췄는지, 그 전에 무엇까지 성공했는지를 이 벡터만으로 알 수 있어야
        // 한다. 빈 벡터로 돌아가면 그 회귀다.
        let steps = recover_completed_steps(&err);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[2].name, "install_grok");
        assert!(matches!(steps[2].status, StepStatus::Failed { .. }));
    }

    #[test]
    fn falls_back_to_empty_for_non_playbook_errors() {
        // playbook 자체에 도달하지 못한 실패(예: SSH 연결 실패)는 실행된
        // 스텝이 없으므로 빈 벡터가 정확한 답이다.
        let err = anyhow::anyhow!("ssh connection refused");
        assert!(recover_completed_steps(&err).is_empty());
    }
}

#[cfg(test)]
mod admin_bootstrap_token_file_tests {
    use super::*;

    #[test]
    fn writes_token_with_0600_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bootstrap-admin-token");

        write_admin_bootstrap_token_to_path(&path, "fat_secret-value").unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "fat_secret-value");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "token file must be 0600, got {mode:o}");
        }
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("etc").join("fleet").join("bootstrap-admin-token");

        write_admin_bootstrap_token_to_path(&path, "fat_x").unwrap();
        assert!(path.exists());
    }

    #[test]
    fn refuses_to_overwrite_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bootstrap-admin-token");
        write_admin_bootstrap_token_to_path(&path, "fat_first").unwrap();

        // 두 번째 발급을 같은 경로에 쓰려는 시도 — 조용히 값을 갈아치우면
        // 안 된다. `issue_admin_bootstrap_token_if_needed`가 정상 동작하는
        // 한 이 경로에는 절대 도달하지 않아야 하지만, 파일 시스템 잔재가
        // 남아 있는 경우를 위한 안전장치다.
        let result = write_admin_bootstrap_token_to_path(&path, "fat_second");
        assert!(result.is_err());

        // 원래 값이 그대로 남아 있어야 한다.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fat_first");
    }
}
