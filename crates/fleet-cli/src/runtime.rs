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

use fleet_api::{run_http_server, AppState};
use fleet_core::{
    CircuitBreakerConfig, TaskFilter, TaskId, TaskStatus, TaskStatusFilter, WorkerFilter,
    WorkerStatus,
};
// CLI 하위 명령 enum (main.rs).
use crate::{EventsAction, TasksAction, WorkersAction};
use fleet_mcp::run_mcp_server;
use fleet_provisioner::{
    HostKeyConfig, HostKeyPolicy, Inventory, InventoryWorker, MockExecutor, Playbook,
    PlaybookContext, PlaybookReport, PrereqReport, ProvisionOptions, RemoteExecutor, SshClient,
    SshConnectInfo, StepContext,
};
use fleet_scheduler::{
    CleanupConfig, Dispatcher, FleetState, HealthChecker, HealthConfig, ReconcileConfig,
    Reconciler, SessionCleanup,
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
    http_bind: Option<&str>,
    api_tokens: Option<&str>,
    cf_audience: Option<&str>,
    dashboard_bind: Option<&str>,
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

    let dispatcher = Arc::new(Dispatcher::new(state.clone()));
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

    // stale `Pending` 작업 재조정 루프 (옵션). `Dispatcher::submit()`은
    // 제출 시점에 딱 한 번만 워커 선택/dispatch를 시도하므로, 그 시도가
    // 터미널 상태에 도달하기 전에 프로세스가 죽으면 작업이 영구히 `Pending`에
    // 고아로 남는다 — 이 루프가 주기적으로 그런 작업을 재시도한다.
    let _reconcile_handle = if !no_reconcile {
        let cfg = ReconcileConfig {
            interval: Duration::from_secs(reconcile_interval_secs.max(1)),
            stale_after: Duration::from_secs(reconcile_stale_secs.max(1)),
        };
        tracing::info!(
            interval_secs = reconcile_interval_secs,
            stale_secs = reconcile_stale_secs,
            "pending-task reconciliation loop enabled"
        );
        let reconciler = Reconciler::new(state.clone(), dispatcher.clone(), cfg);
        Some(reconciler.spawn())
    } else {
        tracing::info!("pending-task reconciliation loop disabled by --no-reconcile");
        None
    };

    // HTTP API 서버 (옵션). --http-bind가 지정된 경우에만 실행.
    let _http_handle = if let Some(bind_str) = http_bind {
        let bind: SocketAddr = bind_str
            .parse()
            .with_context(|| format!("invalid --http-bind address: {bind_str}"))?;

        let mut app_state = AppState::new(store.clone())
            .with_heartbeat_interval(health_interval_secs as u32)
            .with_transport(transport_handle.clone());

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

        if let Some(aud) = cf_audience {
            app_state = app_state.with_cf_audience(aud);
            tracing::info!(bind = %bind, aud = %aud, "HTTP API server with Cloudflare Access auth");
        }
        if let Some(tokens) = api_tokens {
            let token_list: Vec<String> = tokens
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !token_list.is_empty() {
                app_state = app_state.with_tokens(token_list);
                tracing::info!(bind = %bind, "HTTP API server with bearer auth");
            } else if cf_audience.is_none() {
                tracing::warn!(bind = %bind, "HTTP API server in NO-AUTH mode (empty token list)");
            }
        } else if cf_audience.is_none() {
            tracing::warn!(bind = %bind, "HTTP API server in NO-AUTH mode (dev only)");
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
    if let Some(h) = _http_handle {
        h.abort();
    }
    if let Some(h) = _dashboard_handle {
        h.abort();
    }
    Ok(())
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
    }
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
                        steps: vec![],
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
                                steps: vec![],
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
        ..Default::default()
    };
    PlaybookContext::new(base)
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
