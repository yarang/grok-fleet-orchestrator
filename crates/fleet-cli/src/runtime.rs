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
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use fleet_api::{run_http_server, AppState};
use fleet_core::{CircuitBreakerConfig, WorkerFilter, WorkerStatus};
use fleet_mcp::run_mcp_server;
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
        if let Some(tokens) = api_tokens {
            let token_list: Vec<String> = tokens
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !token_list.is_empty() {
                app_state = app_state.with_tokens(token_list);
                tracing::info!(bind = %bind, "HTTP API server with bearer auth");
            } else {
                tracing::warn!(bind = %bind, "HTTP API server in NO-AUTH mode (empty token list)");
            }
        } else {
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
