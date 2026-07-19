//! fleet-worker CLI 진입점.
//!
//! 두 가지 모드:
//! - `fleet-worker --config /etc/fleet/worker.toml` — 데몬 모드
//! - `fleet-worker join --url URL --token TOKEN --name NAME` — 부트스트랩 (Phase 8.3)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use fleet_worker::{JoinArgs, WorkerConfig, WorkerError, WorkerRunner};

/// fleet-worker CLI.
#[derive(Debug, Parser)]
#[command(name = "fleet-worker", version, about = "Fleet worker daemon")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// worker.toml 설정 파일 경로 (daemon 모드).
    #[arg(short, long, env = "FLEET_WORKER_CONFIG")]
    config: Option<PathBuf>,

    /// 설정 파일 검증만 하고 종료 (daemon 모드).
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 부트스트랩 토큰으로 orchestrator에 자동 등록하고 worker.toml을 생성 (Phase 8.3).
    /// 성공하면 config-out 경로에 worker.toml이 기록됨. `--start` 시 daemon으로 exec.
    Join {
        /// Orchestrator base URL.
        #[arg(long, env = "FLEET_ORCHESTRATOR_URL")]
        orchestrator_url: String,

        /// 어드민이 발급한 부트스트랩 토큰.
        #[arg(long, env = "FLEET_BOOTSTRAP_TOKEN")]
        token: String,

        /// 워커 이름 (DNS-safe).
        #[arg(long, env = "FLEET_WORKER_NAME")]
        name: String,

        /// 라벨 (key=value 반복). 쉼표 또는 반복 지정 가능.
        #[arg(long, value_delimiter = ',')]
        labels: Vec<String>,

        /// 워커의 외부 agent endpoint (ws:// 또는 wss://).
        /// 생략하면 orchestrator_url 호스트 기반으로 자동 생성.
        /// cloudflared가 orchestrator와 같은 호스트에서 터널링한다고 가정.
        #[arg(long)]
        agent_endpoint: Option<String>,

        /// grok 서브프로세스 시크릿. 생략하면 32바이트 무작위 생성.
        #[arg(long)]
        grok_secret: Option<String>,

        /// 출력할 worker.toml 경로.
        #[arg(long, default_value = "/etc/fleet/worker.toml")]
        config_out: PathBuf,

        /// max_concurrent_tasks 오버라이드.
        #[arg(long)]
        max_concurrent_tasks: Option<u32>,

        /// config 기록 후 daemon 모드로 exec.
        #[arg(long, default_value_t = false)]
        start: bool,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    fleet_worker::init_tracing();
    let cli = Cli::parse();

    let result: Result<()> = match cli.command {
        Some(Command::Join {
            orchestrator_url,
            token,
            name,
            labels,
            agent_endpoint,
            grok_secret,
            config_out,
            max_concurrent_tasks,
            start,
        }) => async {
            let label_map = parse_labels(&labels)?;
            let args = JoinArgs {
                orchestrator_url,
                token,
                name,
                labels: label_map,
                agent_endpoint,
                grok_secret,
                config_out,
                start,
                max_concurrent_tasks,
            };
            fleet_worker::join::run_join(args)
                .await
                .context("fleet-worker join failed")
        }
        .await,
        None => async {
            // daemon 모드 — config 필수.
            let config_path = cli.config.as_deref().ok_or_else(|| {
                anyhow!("no --config provided. Use `fleet-worker join` for bootstrap or pass --config PATH")
            })?;
            let config = load_config(config_path).context("loading config")?;
            info!(path = %config_path.display(), name = %config.worker.name, "config loaded");

            if cli.check {
                println!(
                    "config OK: worker '{}' → {}",
                    config.worker.name, config.worker.orchestrator_url
                );
                return Ok(());
            }

            let runner = WorkerRunner::new(config);
            runner.run().await.context("fleet-worker daemon")
        }
        .await,
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `--labels key=value,key2=value2` 파싱.
fn parse_labels(raw: &[String]) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for item in raw {
        let (k, v) = item
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid label '{item}' — expected key=value"))?;
        if k.is_empty() {
            return Err(anyhow!("invalid label '{item}' — empty key"));
        }
        out.insert(k.to_string(), v.to_string());
    }
    Ok(out)
}

/// 설정 파일 로드.
fn load_config(path: &Path) -> Result<WorkerConfig, anyhow::Error> {
    WorkerConfig::from_file(path)
        .map_err(|e: WorkerError| anyhow!(e))
        .with_context(|| format!("loading config from {}", path.display()))
}
