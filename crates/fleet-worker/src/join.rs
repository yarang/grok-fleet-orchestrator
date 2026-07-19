//! `fleet-worker join` 서브커맨드 — 부트스트랩 토큰으로 자동 등록 (Phase 8.3).
//!
//! 흐름:
//! 1. 어드민이 `fleet token issue` 로 부트스트랩 토큰을 발급하여 워커 운영자에게 전달.
//! 2. 워커 머신에서 `fleet-worker join --token TOKEN --url URL --name foo` 실행.
//! 3. 이 모듈이 orchestrator의 `POST /v1/workers/join` 을 호출하여
//!    토큰을 검증 + worker_id를 발급받고, worker.toml을 디스크에 기록.
//! 4. (옵션) `--start` 플래그가 있으면 `fleet-worker --config <path>` 로 exec.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::error::WorkerError;

/// `fleet-worker join` 인자.
#[derive(Debug, Clone)]
pub struct JoinArgs {
    /// Orchestrator base URL (예: `https://fleet.example.com`).
    pub orchestrator_url: String,
    /// 어드민이 발급한 부트스트랩 토큰.
    pub token: String,
    /// 워커 이름 (DNS-safe).
    pub name: String,
    /// 라벨 (key=value 쌍).
    pub labels: HashMap<String, String>,
    /// 워커의 agent endpoint. None이면 orchestrator 호스트 기반으로 자동 생성.
    pub agent_endpoint: Option<String>,
    /// grok 서브프로세스 시크릿. None이면 무작위 생성.
    pub grok_secret: Option<String>,
    /// 출력할 worker.toml 경로. 기본 `/etc/fleet/worker.toml`.
    pub config_out: PathBuf,
    /// config를 쓴 후 daemon으로 exec할지 여부.
    pub start: bool,
    /// max_concurrent_tasks 오버라이드.
    pub max_concurrent_tasks: Option<u32>,
}

/// `POST /v1/workers/join` 요청 바디.
#[derive(Debug, Serialize)]
struct JoinApiRequest {
    token: String,
    name: String,
    agent_endpoint: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    labels: HashMap<String, String>,
    max_concurrent_tasks: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_version: Option<String>,
}

/// `POST /v1/workers/join` 응답.
#[derive(Debug, Deserialize)]
struct JoinApiResponse {
    #[allow(dead_code)]
    worker_id: String,
    #[allow(dead_code)]
    heartbeat_interval_secs: u32,
    #[allow(dead_code)]
    config_revision: u32,
    #[allow(dead_code)]
    orchestrator_version: String,
    #[allow(dead_code)]
    status: String,
    /// 워커가 디스크에 기록할 worker.toml 내용.
    worker_config_toml: String,
}

/// join 흐름 실행. 성공하면 config_out에 worker.toml이 기록됨.
pub async fn run_join(args: JoinArgs) -> Result<()> {
    // 1. 워커 이름 검증.
    validate_worker_name(&args.name)?;

    // 2. grok_secret 결정 (명시적 값 or 무작위 생성).
    let grok_secret = match args.grok_secret.as_deref() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => generate_random_secret(32).context("generating grok secret")?,
    };

    // 3. agent_endpoint 결정. 명시적 값이 없으면 orchestrator 호스트 기반.
    let agent_endpoint = match args.agent_endpoint.as_deref() {
        Some(e) if !e.is_empty() => e.to_string(),
        _ => derive_agent_endpoint(&args.orchestrator_url, &grok_secret)?,
    };

    // 4. orchestrator에 join 요청.
    let max_concurrent = args.max_concurrent_tasks.unwrap_or(4);
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(WorkerError::Http)?;

    let url = format!(
        "{}/v1/workers/join",
        args.orchestrator_url.trim_end_matches('/')
    );
    let body = JoinApiRequest {
        token: args.token.clone(),
        name: args.name.clone(),
        agent_endpoint: agent_endpoint.clone(),
        labels: args.labels.clone(),
        max_concurrent_tasks: max_concurrent,
        worker_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    tracing::info!(url = %url, name = %args.name, "joining fleet");
    let resp = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("join request failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("join failed: {status} — {text}"));
    }
    let parsed: JoinApiResponse = resp
        .json()
        .await
        .map_err(|e| anyhow!("parsing join response: {e}"))?;

    // 5. worker.toml을 atomic하게 디스크에 기록.
    write_config_atomic(&args.config_out, &parsed.worker_config_toml)
        .with_context(|| format!("writing config to {}", args.config_out.display()))?;
    println!(
        "worker '{}' joined — config written to {}",
        args.name,
        args.config_out.display()
    );

    // 6. (옵션) daemon으로 exec.
    if args.start {
        exec_daemon(&args.config_out)?;
    }
    Ok(())
}

/// 워커 이름 DNS-safe 검증.
fn validate_worker_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("worker name must not be empty"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(anyhow!(
            "worker name must be alphanumeric, '-', '_', or '.' only (got '{name}')"
        ));
    }
    Ok(())
}

/// orchestrator URL에서 호스트 추출.
fn extract_host(url: &str) -> Result<&str> {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    Ok(&after_scheme[..host_end])
}

/// orchestrator URL 기반으로 agent_endpoint 자동 생성.
/// cloudflared가 orchestrator와 같은 호스트에서 localhost:2419를 터널링한다고 가정.
/// ws://<orchestrator-host>/ws?server-key=<secret>
fn derive_agent_endpoint(orchestrator_url: &str, grok_secret: &str) -> Result<String> {
    let host = extract_host(orchestrator_url)?;
    Ok(format!("ws://{host}/ws?server-key={grok_secret}"))
}

/// 무작위 grok 시크릿 생성 (base64url, 32바이트).
fn generate_random_secret(bytes: usize) -> Result<String> {
    use std::io::Read;
    let mut buf = vec![0u8; bytes];
    #[cfg(unix)]
    {
        let mut f = std::fs::File::open("/dev/urandom").context("opening /dev/urandom")?;
        f.read_exact(&mut buf).context("reading /dev/urandom")?;
    }
    #[cfg(not(unix))]
    {
        let mut filled = 0;
        while filled < bytes {
            let id = uuid::Uuid::new_v4();
            let b = id.as_bytes();
            let take = (bytes - filled).min(b.len());
            buf[filled..filled + take].copy_from_slice(&b[..take]);
            filled += take;
        }
    }
    Ok(base64url(&buf))
}

/// base64url-no-pad 인코딩.
fn base64url(input: &[u8]) -> String {
    const ALPHA: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((input.len() * 4).div_ceil(3));
    let mut chunks = input.chunks_exact(3);
    for c in &mut chunks {
        let n = ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | (c[2] as u32);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(n & 0x3F) as usize] as char);
    }
    let rem = chunks.remainder();
    match rem.len() {
        1 => {
            let n = (rem[0] as u32) << 16;
            out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        }
        2 => {
            let n = ((rem[0] as u32) << 16) | ((rem[1] as u32) << 8);
            out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        }
        _ => {}
    }
    out
}

/// atomic 파일 쓰기: 임시 파일에 쓰고 rename.
fn write_config_atomic(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent dir {}", parent.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, content).with_context(|| format!("writing tmp file {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} → {}", tmp.display(), path.display()))?;
    Ok(())
}

/// 현재 프로세스를 daemon으로 교체 (exec).
#[cfg(unix)]
fn exec_daemon(config_path: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let bin = std::env::current_exe()
        .context("locating current_exe")?;
    tracing::info!(bin = %bin.display(), config = %config_path.display(), "exec-ing into daemon");
    let err = std::process::Command::new(&bin)
        .arg("--config")
        .arg(config_path)
        .exec();
    // exec가 성공하면 여기에 도달하지 않음.
    Err(anyhow!("exec failed: {err}"))
}

#[cfg(not(unix))]
fn exec_daemon(config_path: &Path) -> Result<()> {
    // Windows에서는 exec 미지원 — spawn + wait 로 폴백.
    let bin = std::env::current_exe().context("locating current_exe")?;
    tracing::info!(bin = %bin.display(), config = %config_path.display(), "spawning daemon");
    let status = std::process::Command::new(&bin)
        .arg("--config")
        .arg(config_path)
        .status()
        .context("spawning daemon")?;
    if status.success() {
        // 프로세스 종료 — daemon이 끝났다는 뜻.
        std::process::exit(status.code().unwrap_or(0));
    } else {
        Err(anyhow!("daemon exited with status {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_accepts_valid() {
        assert!(validate_worker_name("build-farm-1").is_ok());
        assert!(validate_worker_name("worker_a.b").is_ok());
        assert!(validate_worker_name("abc123").is_ok());
    }

    #[test]
    fn validate_name_rejects_invalid() {
        assert!(validate_worker_name("").is_err());
        assert!(validate_worker_name("name with space").is_err());
        assert!(validate_worker_name("café").is_err());
        assert!(validate_worker_name("a/b").is_err());
    }

    #[test]
    fn extract_host_works() {
        assert_eq!(extract_host("https://fleet.example.com").unwrap(), "fleet.example.com");
        assert_eq!(
            extract_host("http://localhost:8080/foo").unwrap(),
            "localhost:8080"
        );
        assert_eq!(extract_host("fleet.example.com").unwrap(), "fleet.example.com");
    }

    #[test]
    fn derive_endpoint_includes_secret() {
        let endpoint =
            derive_agent_endpoint("https://fleet.example.com", "topsecret").unwrap();
        assert_eq!(endpoint, "ws://fleet.example.com/ws?server-key=topsecret");
    }

    #[test]
    fn derive_endpoint_with_port() {
        let endpoint = derive_agent_endpoint("http://localhost:8080", "s").unwrap();
        assert_eq!(endpoint, "ws://localhost:8080/ws?server-key=s");
    }

    #[test]
    fn random_secret_is_base64url() {
        let s = generate_random_secret(32).unwrap();
        assert_eq!(s.len(), 43); // 32 bytes → 43 base64url chars (no padding)
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn base64url_known_vectors() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn write_atomic_creates_parent_dirs() {
        let dir = std::env::temp_dir().join(format!(
            "fleet-worker-test-{}",
            std::process::id()
        ));
        let path = dir.join("sub/dir/worker.toml");
        write_config_atomic(&path, "# test\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# test\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_replaces_existing() {
        let dir = std::env::temp_dir().join(format!(
            "fleet-worker-replace-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("worker.toml");
        std::fs::write(&path, "OLD\n").unwrap();
        write_config_atomic(&path, "NEW\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "NEW\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
