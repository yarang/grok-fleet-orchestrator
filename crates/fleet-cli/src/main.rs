//! # fleet-cli
//!
//! Grok Fleet Orchestrator의 명령줄 인터페이스.
//!
//! ## 명령
//!
//! - `fleet serve` — MCP stdio + HTTP API + (옵션) 대시보드 실행
//! - `fleet migrate` — 데이터베이스 마이그레이션만 실행
//! - `fleet workers list` / `workers show <name>` — 워커 조회
//! - `fleet tasks list` / `tasks show <id>` / `tasks cancel <id>` — 작업 관리
//! - `fleet token new` — 부트스트랩 토큰 생성
//! - `fleet doctor` — 인프라 진단 (DB 연결, 마이그레이션, 워커 상태)
//! - `fleet provision` — SSH 자동 프로비저닝
//!
//! ## 환경변수
//!
//! - `DATABASE_URL` — PostgreSQL 연결 문자열 (필수)
//! - `RUST_LOG` — 로깅 레벨 (예: `info,fleet=debug`)

#![forbid(unsafe_code)]
#![allow(missing_docs)]

mod doctor;
mod logging;
mod runtime;
mod token;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

/// Grok Fleet Orchestrator CLI.
#[derive(Debug, Parser)]
#[command(name = "fleet", version, about, propagate_version = true)]
struct Cli {
    /// 로깅 레벨 (`RUST_LOG` 형식). 예: `info`, `debug,fleet=trace`.
    #[arg(long, env = "FLEET_LOG", default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// MCP stdio 서버 실행. AI 클라이언트(grok build, Claude Code 등)에
    /// Fleet 도구를 노출합니다.
    Serve {
        /// `mock` (개발/테스트, 가상 워커) 또는 `acp` (실제 grok agent serve와 WebSocket 통신).
        /// `acp`는 빌드 시 `--features acp` 필요 (기본 활성화).
        #[arg(long, env = "FLEET_TRANSPORT", default_value = "mock")]
        transport: String,

        /// Postgres 최대 연결 수.
        #[arg(long, env = "FLEET_DB_MAX_CONN", default_value_t = 10)]
        db_max_conn: u32,

        /// 헬스체크 비활성화 (기본값: 활성).
        #[arg(long, default_value_t = false)]
        no_health_check: bool,

        /// 헬스체크 폴링 주기 (초).
        #[arg(long, env = "FLEET_HEALTH_INTERVAL", default_value_t = 15)]
        health_interval_secs: u64,

        /// 하트비트 누락 허용 횟수. 이 횟수 × 주기를 초과하면 offline 처리.
        #[arg(long, env = "FLEET_HEALTH_MISSED", default_value_t = 3)]
        health_missed: u32,

        /// HTTP API 바인드 주소 (예: `127.0.0.1:8081`).
        /// 생략하면 HTTP API를 실행하지 않고 MCP stdio만 서비스.
        /// 지정하면 워커 등록/하트비트 엔드포인트가 병렬로 serve됩니다.
        #[arg(long, env = "FLEET_HTTP_BIND")]
        http_bind: Option<String>,

        /// HTTP API 인증용 bearer 토큰 (쉼표 구분).
        /// 생략하면 no-auth 모드 (개발용). Phase 4에서 OIDC로 대체.
        #[arg(long, env = "FLEET_API_TOKENS")]
        api_tokens: Option<String>,

        /// Cloudflare Access Application AUD. 설정된 경우
        /// CF-Access-Jwt-Assertion 헤더 검증 활성화.
        #[arg(long, env = "FLEET_CF_AUDIENCE")]
        cf_audience: Option<String>,

        /// 웹 대시보드 바인드 주소 (예: `127.0.0.1:8082`).
        /// 생략하면 대시보드 서버를 실행하지 않습니다.
        /// 지정하면 `/api/overview`, `/api/workers`, `/api/tasks`,
        /// `/api/events/stream` (SSE) 엔드포인트가 제공됩니다.
        #[arg(long, env = "FLEET_DASHBOARD_BIND")]
        dashboard_bind: Option<String>,
    },

    /// 데이터베이스 마이그레이션만 실행하고 종료.
    Migrate,

    /// 워커 관련 조회 명령 그룹.
    Workers {
        #[command(subcommand)]
        action: WorkersAction,
    },

    /// 작업 관련 조회/제어 명령 그룹.
    Tasks {
        #[command(subcommand)]
        action: TasksAction,
    },

    /// 부트스트랩 토큰 관리 (워커 등록용).
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },

    /// 감사 로그 (이벤트 히스토리) 조회.
    /// 모든 상태 변화는 `fleet_events` 테이블에 append-only로 기록됩니다.
    Events {
        #[command(subcommand)]
        action: EventsAction,
    },

    /// 인프라 진단. DB 연결, 마이그레이션 상태, 워커 가용성을 점검하고
    /// 보고서를 출력합니다.
    Doctor {
        /// HTTP API URL (선택). 지정된 경우 /v1/health 를 호출해 응답을 점검.
        #[arg(long, env = "FLEET_API_URL")]
        api_url: Option<String>,

        /// 대시보드 URL (선택). 지정된 경우 /health 호출.
        #[arg(long, env = "FLEET_DASHBOARD_URL")]
        dashboard_url: Option<String>,

        /// Postgres 최대 연결 수 (진단용).
        #[arg(long, default_value_t = 2)]
        db_max_conn: u32,
    },

    /// 원격 서버에 SSH로 접속해 워커 스택을 자동 프로비저닝.
    ///
    /// 단일 호스트 또는 inventory YAML 파일로 일괄 처리.
    Provision {
        /// 단일 호스트 (IP 또는 호스트명). --inventory와 배타.
        #[arg(long, conflicts_with = "inventory")]
        host: Option<String>,

        /// SSH 사용자. --host 모드에서 사용.
        #[arg(long, default_value = "ubuntu")]
        user: String,

        /// SSH 포트.
        #[arg(long, default_value_t = 22)]
        ssh_port: u16,

        /// SSH 개인키 경로.
        #[arg(long)]
        ssh_key: Option<String>,

        /// 워커 이름 (오케스트레이터에 등록될 식별자).
        #[arg(long)]
        name: Option<String>,

        /// 라벨 (key=value 반복). 예: --labels arch=arm64,gpu=false
        #[arg(long, value_delimiter = ',')]
        labels: Vec<String>,

        /// Cloudflare 토큰 (터널 생성용).
        #[arg(long, env = "FLEET_CF_TOKEN")]
        cf_token: Option<String>,

        /// 오케스트레이터 URL.
        #[arg(long, env = "FLEET_ORCHESTRATOR_URL")]
        orchestrator_url: Option<String>,

        /// 로컬 빌드한 fleet-worker 바이너리 경로.
        #[arg(long)]
        fleet_worker_bin: Option<String>,

        /// 인벤토리 YAML 파일 경로. --host 대신 사용.
        #[arg(long, conflicts_with = "host")]
        inventory: Option<String>,

        /// 병렬 처리 수 (인벤토리 모드).
        #[arg(long, default_value_t = 1)]
        parallel: usize,

        /// 특정 태그만 실행 (예: tunnel, setup).
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,

        /// 인벤토리 내에서 특정 워커만 실행 (쉼표 구분 이름).
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,

        /// Dry-run — 실제 변경 없이 무엇을 할지 로깅.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
enum WorkersAction {
    /// 등록된 워커 목록을 테이블 형태로 출력.
    List {
        /// 상태 필터 (`online`, `offline`, `degraded`, `circuit_open`).
        #[arg(long)]
        status: Option<String>,

        /// JSON 형식 출력 (스크립트용).
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// 이름으로 단일 워커 상세 조회.
    Show {
        /// 워커 이름.
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum TasksAction {
    /// 작업 목록을 최신순으로 출력.
    List {
        /// 위상 필터 (`pending`, `dispatched`, `completed`, `failed`, `cancelled`,
        /// `terminal`, `active`).
        #[arg(long)]
        status: Option<String>,

        /// 최대 출력 수.
        #[arg(long, default_value_t = 50)]
        limit: usize,

        /// JSON 형식 출력.
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// 작업 ID로 단일 작업 상세 조회.
    Show {
        /// 작업 ID (UUID).
        id: String,
    },

    /// 실행 중인 작업을 취소 요청.
    Cancel {
        /// 작업 ID (UUID).
        id: String,

        /// 취소 사유 (기본값: "manual cancel").
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum TokenAction {
    /// 무작위 부트스트랩 토큰을 생성해 stdout에 출력.
    /// 생성된 토큰은 `--api-tokens` (또는 `FLEET_API_TOKENS`)에 추가하여
    /// 워커 등록 인증에 사용합니다.
    New {
        /// 토큰 접두어.
        #[arg(long, default_value = "fleet")]
        prefix: String,

        /// 무작위 바이트 길이 (16~64 권장).
        #[arg(long, default_value_t = 32)]
        bytes: usize,
    },
}

#[derive(Debug, Subcommand)]
enum EventsAction {
    /// 최근 이벤트를 시간 역순으로 출력.
    List {
        /// 이 seq 이후의 이벤트만 조회 (기본값: 0 = 처음부터).
        #[arg(long, default_value_t = 0)]
        after_seq: u64,

        /// 최대 출력 수.
        #[arg(long, default_value_t = 50)]
        limit: u32,

        /// JSON 형식 출력.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    logging::init(&cli.log_level);

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        command = ?cli.command,
        "fleet CLI starting"
    );

    match cli.command {
        Command::Serve {
            transport,
            db_max_conn,
            no_health_check,
            health_interval_secs,
            health_missed,
            http_bind,
            api_tokens,
            cf_audience,
            dashboard_bind,
        } => {
            runtime::run_serve(
                &transport,
                db_max_conn,
                no_health_check,
                health_interval_secs,
                health_missed,
                http_bind.as_deref(),
                api_tokens.as_deref(),
                cf_audience.as_deref(),
                dashboard_bind.as_deref(),
            )
            .await
        }
        Command::Migrate => runtime::run_migrate().await,
        Command::Workers { action } => runtime::run_workers(action).await,
        Command::Tasks { action } => runtime::run_tasks(action).await,
        Command::Token { action } => token::run_token(action).await,
        Command::Events { action } => runtime::run_events(action).await,
        Command::Doctor {
            api_url,
            dashboard_url,
            db_max_conn,
        } => doctor::run_doctor(api_url, dashboard_url, db_max_conn).await,
        Command::Provision {
            host,
            user,
            ssh_port,
            ssh_key,
            name,
            labels,
            cf_token,
            orchestrator_url,
            fleet_worker_bin,
            inventory,
            parallel,
            tags,
            only,
            dry_run,
        } => {
            runtime::run_provision(runtime::ProvisionArgs {
                host,
                user,
                ssh_port,
                ssh_key,
                name,
                labels,
                cf_token,
                orchestrator_url,
                fleet_worker_bin,
                inventory,
                parallel,
                tags,
                only,
                dry_run,
            })
            .await
        }
    }
    .context("fleet command failed")?;

    Ok(())
}
