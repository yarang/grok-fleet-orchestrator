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
    Inventory, InventoryWorker, MockExecutor, Playbook, PlaybookContext, PlaybookReport,
    PrereqReport, ProvisionOptions, RemoteExecutor, SshClient, SshConnectInfo, StepContext,
};
use fleet_scheduler::{Dispatcher, FleetState, HealthChecker, HealthConfig};
use fleet_store::{PgStore, Store};
use fleet_transport::{MockTransport, WorkerTransport};

/// Postgres 연결 URL 조회 (`DATABASE_URL` 필수).
fn database_url() -> Result<String> {
    std::env::var("DATABASE_URL").context(
        "DATABASE_URL is not set. Export DATABASE_URL=postgres://user@host/dbname",
    )
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
    fn to_tls_config(self) -> Option<fleet_transport::ClientTlsConfig> {
        let ca = self.ca?;
        let cert = self.cert?;
        let key = self.key?;
        Some(fleet_transport::ClientTlsConfig::from_paths(
            ca, cert, key,
        ))
    }
}

/// `--transport acp` 인 경우 `AcpTransport` 생성. mTLS 플래그가 모두
/// 설정된 경우 `with_client_tls` 로 클라이언트 인증서를 전달.
#[cfg(feature = "acp")]
fn build_acp_transport(
    mtls_flags: &MtlsFlags,
) -> Result<fleet_transport::AcpTransport, anyhow::Error> {
    let transport = fleet_transport::AcpTransport::new();
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

/// PgStore 생성 + 마이그레이션 실행.
async fn connect_and_migrate(max_conn: u32) -> Result<Arc<PgStore>> {
    let url = database_url()?;
    tracing::info!(url = %sanitize_url(&url), max_conn, "connecting to Postgres");
    let store = PgStore::connect(&url, max_conn)
        .await
        .context("failed to connect to Postgres")?;
    store.migrate().await.context("migration failed")?;
    tracing::info!("database migrations applied");
    Ok(Arc::new(store))
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
                return format!(
                    "{}{}:****{}",
                    &url[..scheme_end],
                    user,
                    &url[creds_end..]
                );
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
    no_health_check: bool,
    health_interval_secs: u64,
    health_missed: u32,
    http_bind: Option<&str>,
    api_tokens: Option<&str>,
    cf_audience: Option<&str>,
    dashboard_bind: Option<&str>,
    mtls_flags: MtlsFlags<'_>,
) -> Result<()> {
    let store = connect_and_migrate(db_max_conn).await?;

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
                    return Err(anyhow!(
                        "--mtls-ca requires building with --features mtls"
                    ));
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

    // HTTP API 서버 (옵션). --http-bind가 지정된 경우에만 실행.
    let _http_handle = if let Some(bind_str) = http_bind {
        let bind: SocketAddr = bind_str
            .parse()
            .with_context(|| format!("invalid --http-bind address: {bind_str}"))?;

        let mut app_state = AppState::new(store.clone())
            .with_heartbeat_interval(health_interval_secs as u32)
            .with_transport(transport_handle.clone());
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
    let _dashboard_handle = if let Some(bind_str) = dashboard_bind {
        let bind: SocketAddr = bind_str
            .parse()
            .with_context(|| format!("invalid --dashboard-bind address: {bind_str}"))?;
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
        TasksAction::List { status, limit, json } => run_tasks_list(status, limit, json).await,
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

    println!(
        "{:<8} {:<24} {:<22} DETAIL",
        "SEQ", "TIMESTAMP", "TYPE"
    );
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
    println!("{:<20} {}", "STATUS:", format!("{:?}", w.status).to_lowercase());
    println!(
        "{:<20} {}/{}",
        "ACTIVE:", w.active_tasks, w.max_concurrent
    );
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
    println!(
        "{:<20} {}",
        "REGISTERED_AT:",
        w.registered_at.to_rfc3339()
    );
    println!("{:<20} {:?}", "LABELS:", w.labels);
    Ok(())
}

/// `tasks list` 명령.
async fn run_tasks_list(
    status_filter: Option<String>,
    limit: usize,
    json: bool,
) -> Result<()> {
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
        TaskStatus::Dispatched { worker_id, started_at } => {
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
        TaskStatus::Cancelled { reason, cancelled_at } => {
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
    pub inventory: Option<String>,
    pub parallel: usize,
    pub tags: Vec<String>,
    pub only: Vec<String>,
    pub dry_run: bool,
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
        let connect_info = SshConnectInfo::new(host, &args.user, PathBuf::from(&ssh_key))
            .with_port(args.ssh_port);
        let ssh = SshClient::connect(connect_info)
            .await
            .context("SSH connection failed")?;
        run_playbook(&ssh, &ctx, &args.tags).await?
    };

    print_report(&report);
    Ok(())
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

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore closed");
                tracing::info!(%worker_name, %host, "starting provisioning");
                let connect_info = SshConnectInfo::new(&host, &user, PathBuf::from(&ssh_key))
                    .with_port(port);
                match SshClient::connect(connect_info).await {
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
    println!("Provisioning summary: {} succeeded, {} failed", succeeded, failed);
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
        orchestrator_url: options
            .orchestrator_url
            .clone()
            .unwrap_or_default(),
        cf_token,
        fleet_worker_bin: None,
        grok_secret: w.grok_secret.clone(),
        bootstrap_token: options.bootstrap_token.clone(),
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
    println!("Status: {}", if report.succeeded { "✓ success" } else { "✗ failed" });
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
