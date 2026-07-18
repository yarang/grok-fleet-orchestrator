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
use fleet_core::{CircuitBreakerConfig, WorkerFilter, WorkerStatus};
use fleet_mcp::run_mcp_server;
use fleet_provisioner::{
    Inventory, InventoryWorker, MockExecutor, Playbook, PlaybookContext, PlaybookReport,
    PrereqReport, ProvisionOptions, RemoteExecutor, SshClient, SshConnectInfo, StepContext,
};
use fleet_scheduler::{Dispatcher, FleetState, HealthChecker, HealthConfig};
use fleet_store::{PgStore, Store};
use fleet_transport::MockTransport;

/// Postgres 연결 URL 조회 (`DATABASE_URL` 필수).
fn database_url() -> Result<String> {
    std::env::var("DATABASE_URL").context(
        "DATABASE_URL is not set. Export DATABASE_URL=postgres://user@host/dbname",
    )
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
) -> Result<()> {
    let store = connect_and_migrate(db_max_conn).await?;

    // Phase 1: MockTransport만 지원. Phase 3에서 HubTransport feature 추가.
    if transport_kind != "mock" {
        return Err(anyhow!(
            "transport '{transport_kind}' is not supported in Phase 1. \
             Use `--transport mock` (Phase 3 will add `hub`)."
        ));
    }

    let (transport, event_rx) = MockTransport::new();
    let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(transport);

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
            .with_heartbeat_interval(health_interval_secs as u32);
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
    Ok(())
}

/// `migrate` 명령.
pub async fn run_migrate() -> Result<()> {
    let _ = connect_and_migrate(1).await?;
    println!("migrations applied successfully");
    Ok(())
}

/// `worker list` 명령.
pub async fn run_worker_list(status_filter: Option<String>) -> Result<()> {
    let store = connect_and_migrate(2).await?;

    let mut filter = WorkerFilter::default();
    if let Some(s) = status_filter {
        filter.status = Some(parse_status(&s)?);
    }

    let workers = store
        .list_workers(&filter)
        .await
        .context("failed to list workers")?;

    if workers.is_empty() {
        println!("(no workers registered)");
        return Ok(());
    }

    // 사람용 텍스트 포맷
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
    pub inventory: Option<String>,
    pub parallel: usize,
    pub tags: Vec<String>,
    pub only: Vec<String>,
    pub dry_run: bool,
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
        args.dry_run,
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

fn build_step_context(
    name: &str,
    labels: std::collections::HashMap<String, String>,
    orchestrator_url: Option<&str>,
    cf_token: Option<&str>,
    fleet_worker_bin: Option<&str>,
    dry_run: bool,
) -> PlaybookContext {
    let base = StepContext {
        worker_name: name.to_string(),
        labels,
        orchestrator_url: orchestrator_url.unwrap_or("").to_string(),
        cf_token: cf_token.map(String::from),
        fleet_worker_bin: fleet_worker_bin.map(String::from),
        dry_run,
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
        dry_run: options.dry_run,
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
