//! fleet-worker CLI 진입점.
//!
//! 사용법: `fleet-worker --config /etc/fleet/worker.toml`

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use tracing::info;

use fleet_worker::{WorkerConfig, WorkerError, WorkerRunner};

/// fleet-worker CLI.
#[derive(Debug, Parser)]
#[command(name = "fleet-worker", version, about = "Fleet worker daemon")]
struct Cli {
    /// worker.toml 설정 파일 경로.
    #[arg(short, long, env = "FLEET_WORKER_CONFIG")]
    config: PathBuf,

    /// 설정 파일 검증만 하고 종료.
    #[arg(long)]
    check: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    fleet_worker::init_tracing();
    let cli = Cli::parse();

    let config = match load_config(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: failed to load config from {}: {e}", cli.config.display());
            return ExitCode::FAILURE;
        }
    };
    info!(path = %cli.config.display(), name = %config.worker.name, "config loaded");

    if cli.check {
        println!("config OK: worker '{}' → {}", config.worker.name, config.worker.orchestrator_url);
        return ExitCode::SUCCESS;
    }

    let runner = WorkerRunner::new(config);
    match runner.run().await {
        Ok(()) => {
            info!("fleet-worker exited cleanly");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: fleet-worker terminated: {e}");
            ExitCode::FAILURE
        }
    }
}

/// 설정 파일 로드. 단순히 `WorkerConfig::from_file` 위임 + 에러 래핑.
fn load_config(path: &Path) -> Result<WorkerConfig, anyhow::Error> {
    WorkerConfig::from_file(path)
        .map_err(|e: WorkerError| anyhow!(e))
        .with_context(|| format!("loading config from {}", path.display()))
}
