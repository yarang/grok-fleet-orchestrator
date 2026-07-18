//! # fleet-cli
//!
//! Grok Fleet Orchestrator의 명령줄 인터페이스.
//!
//! ## 명령
//!
//! - `fleet serve` — MCP stdio 서버 실행 (AI 코딩 클라이언트에 도구 노출)
//! - `fleet migrate` — 데이터베이스 마이그레이션만 실행
//! - `fleet worker list` — 등록된 워커 조회 (사람용)
//!
//! ## 환경변수
//!
//! - `DATABASE_URL` — PostgreSQL 연결 문자열 (필수)
//! - `RUST_LOG` — 로깅 레벨 (예: `info,fleet=debug`)

#![forbid(unsafe_code)]
#![allow(missing_docs)]

mod logging;
mod runtime;

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
        /// `mock` (개발/테스트) 또는 `hub` (프로덕션, Phase 3).
        /// 현재는 `mock`만 구현됨.
        #[arg(long, env = "FLEET_TRANSPORT", default_value = "mock")]
        transport: String,

        /// Postgres 최대 연결 수.
        #[arg(long, env = "FLEET_DB_MAX_CONN", default_value_t = 10)]
        db_max_conn: u32,
    },

    /// 데이터베이스 마이그레이션만 실행하고 종료.
    Migrate,

    /// 등록된 워커 목록을 사람이 읽기 쉬운 형태로 출력.
    WorkerList {
        /// 상태 필터 (`online`, `offline`, `degraded`, `circuit_open`).
        #[arg(long)]
        status: Option<String>,
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
        } => runtime::run_serve(&transport, db_max_conn).await,
        Command::Migrate => runtime::run_migrate().await,
        Command::WorkerList { status } => runtime::run_worker_list(status).await,
    }
    .context("fleet command failed")?;

    Ok(())
}
