//! `worker.toml` 파서.
//!
//! ## 파일 형식
//!
//! ```toml
//! [worker]
//! name = "build-farm-1"
//! orchestrator_url = "https://fleet.example.com"
//! heartbeat_interval_secs = 15
//! bootstrap_token = "fleet-xxx"        # bearer auth (옵션)
//! labels = { arch = "arm64", gpu = "false" }
//! existing_worker_id = "550e8400-..."  # 재등록 시 ID 유지 (옵션)
//!
//! [grok]
//! bin = "/usr/local/bin/grok"
//! bind_addr = "127.0.0.1:2419"
//! secret = "server-key-secret"
//! max_concurrent_tasks = 4
//! restart_delay_secs = 5
//! cwd = "/var/lib/fleet-worker"       # 옵션
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::WorkerError;

/// worker.toml 전체.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
    pub worker: WorkerSection,
    pub grok: GrokSection,
}

/// `[worker]` 섹션.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSection {
    /// 워커 이름 (DNS-safe). orchestrator 등록 시 사용.
    pub name: String,
    /// orchestrator의 base URL (예: `https://fleet.example.com`).
    /// trailing slash 없이 저장됨.
    pub orchestrator_url: String,
    /// 하트비트 주기 (초).
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u32,
    /// bearer auth 토큰 (orchestrator가 `--api-tokens`로 보호된 경우 필요).
    #[serde(default)]
    pub bootstrap_token: Option<String>,
    /// 워커 라벨 (필터링용).
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// 재등록 시 유지할 기존 worker_id (UUID 문자열). 신규 등록이면 None.
    #[serde(default)]
    pub existing_worker_id: Option<String>,
}

/// `[grok]` 섹션.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrokSection {
    /// grok 실행 파일 절대 경로.
    pub bin: String,
    /// `grok agent serve --bind <addr>`의 bind 주소.
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
    /// `grok agent serve --secret <secret>` 값. ACP server-key로도 사용.
    pub secret: String,
    /// 동시 처리 작업 수.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_tasks: u32,
    /// 비정상 종료 시 재시작 대기 시간 (초).
    #[serde(default = "default_restart_delay")]
    pub restart_delay_secs: u32,
    /// grok 서브프로세스의 작업 디렉토리 (옵션).
    #[serde(default)]
    pub cwd: Option<String>,
}

fn default_heartbeat_interval() -> u32 {
    15
}
fn default_bind_addr() -> String {
    "127.0.0.1:2419".to_string()
}
fn default_max_concurrent() -> u32 {
    4
}
fn default_restart_delay() -> u32 {
    5
}

impl WorkerConfig {
    /// 파일에서 설정 로드.
    pub fn from_file(path: &Path) -> Result<Self, WorkerError> {
        let contents = std::fs::read_to_string(path)?;
        contents.parse()
    }

    /// 정규화: trailing slash 제거, 공백 trim.
    fn normalize(&mut self) {
        self.worker.orchestrator_url = self.worker.orchestrator_url.trim().trim_end_matches('/').to_string();
        self.worker.name = self.worker.name.trim().to_string();
        self.grok.bin = self.grok.bin.trim().to_string();
        self.grok.bind_addr = self.grok.bind_addr.trim().to_string();
        self.grok.secret = self.grok.secret.trim().to_string();
    }

    /// 의존성 검증.
    fn validate(&self) -> Result<(), WorkerError> {
        if self.worker.name.is_empty() {
            return Err(WorkerError::Config("worker.name must not be empty".into()));
        }
        if !self
            .worker
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(WorkerError::Config(
                "worker.name must be alphanumeric, '-', '_', or '.' only".into(),
            ));
        }
        if !self.worker.orchestrator_url.starts_with("http://")
            && !self.worker.orchestrator_url.starts_with("https://")
        {
            return Err(WorkerError::Config(
                "worker.orchestrator_url must start with http:// or https://".into(),
            ));
        }
        if self.grok.bin.is_empty() {
            return Err(WorkerError::Config("grok.bin must not be empty".into()));
        }
        if self.grok.secret.is_empty() {
            return Err(WorkerError::Config("grok.secret must not be empty".into()));
        }
        if self.grok.max_concurrent_tasks == 0 {
            return Err(WorkerError::Config(
                "grok.max_concurrent_tasks must be >= 1".into(),
            ));
        }
        if self.grok.restart_delay_secs > 300 {
            return Err(WorkerError::Config(
                "grok.restart_delay_secs must be <= 300".into(),
            ));
        }
        Ok(())
    }

    /// 등록 시 grok agent의 WebSocket endpoint로 노출될 URL.
    /// orchestrator는 이 값을 transport.register()에 전달.
    pub fn agent_endpoint(&self) -> String {
        // bind_addr이 127.0.0.1인 경우 orchestrator와 같은 호스트로 간주.
        // 그러나 일반적으로 cloudflared 터널이 bind_addr을 외부 호스트명으로 노출.
        // → worker.toml의 orchestrator_url과 같은 호스트를 사용하되,
        //   cloudflared가 localhost:2419를 터널링한다고 가정.
        // MVP: orchestrator의 host + /ws 경로 사용.
        let host = self.orchestrator_url_host();
        format!("ws://{host}/ws?server-key={}", self.grok.secret)
    }

    /// orchestrator_url에서 host:port 추출.
    pub(crate) fn orchestrator_url_host(&self) -> &str {
        let url = &self.worker.orchestrator_url;
        let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
        let host_end = after_scheme
            .find(['/', '?', '#'])
            .unwrap_or(after_scheme.len());
        &after_scheme[..host_end]
    }
}

/// TOML 문자열에서 WorkerConfig로 파싱. normalize + validate 수행.
impl std::str::FromStr for WorkerConfig {
    type Err = WorkerError;
    fn from_str(contents: &str) -> Result<Self, Self::Err> {
        let mut config: WorkerConfig = toml::from_str(contents)?;
        config.normalize();
        config.validate()?;
        Ok(config)
    }
}

impl WorkerConfig {
    /// 테스트 편의용 빌더 시작점.
    pub fn for_test() -> WorkerConfigBuilder {
        WorkerConfigBuilder::default()
    }
}

/// 테스트용 config 빌더.
#[derive(Default)]
pub struct WorkerConfigBuilder {
    name: Option<String>,
    orchestrator_url: Option<String>,
    bootstrap_token: Option<String>,
    grok_bin: Option<String>,
    grok_secret: Option<String>,
    bind_addr: Option<String>,
    max_concurrent: Option<u32>,
    labels: HashMap<String, String>,
}

impl WorkerConfigBuilder {
    pub fn name(mut self, n: impl Into<String>) -> Self {
        self.name = Some(n.into());
        self
    }
    pub fn orchestrator_url(mut self, u: impl Into<String>) -> Self {
        self.orchestrator_url = Some(u.into());
        self
    }
    pub fn grok_bin(mut self, b: impl Into<String>) -> Self {
        self.grok_bin = Some(b.into());
        self
    }
    pub fn grok_secret(mut self, s: impl Into<String>) -> Self {
        self.grok_secret = Some(s.into());
        self
    }
    pub fn bind_addr(mut self, a: impl Into<String>) -> Self {
        self.bind_addr = Some(a.into());
        self
    }
    pub fn max_concurrent(mut self, m: u32) -> Self {
        self.max_concurrent = Some(m);
        self
    }
    /// 라벨 한 쌍 추가. 여러 번 호출하여 누적.
    pub fn label(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.labels.insert(k.into(), v.into());
        self
    }
    pub fn build(self) -> WorkerConfig {
        WorkerConfig {
            worker: WorkerSection {
                name: self.name.unwrap_or_else(|| "test-worker".into()),
                orchestrator_url: self
                    .orchestrator_url
                    .unwrap_or_else(|| "http://127.0.0.1:8080".into()),
                heartbeat_interval_secs: 1,
                bootstrap_token: self.bootstrap_token,
                labels: self.labels,
                existing_worker_id: None,
            },
            grok: GrokSection {
                bin: self.grok_bin.unwrap_or_else(|| "/bin/true".into()),
                bind_addr: self
                    .bind_addr
                    .unwrap_or_else(|| "127.0.0.1:2419".into()),
                secret: self.grok_secret.unwrap_or_else(|| "test-secret".into()),
                max_concurrent_tasks: self.max_concurrent.unwrap_or(2),
                restart_delay_secs: 1,
                cwd: None,
            },
        }
    }
}

/// 파일 경로가 비어있는지 확인 (CLI에서 사용).
pub fn config_path_or_error(path: &Option<PathBuf>) -> Result<&Path, WorkerError> {
    path.as_deref().ok_or_else(|| WorkerError::Config("no --config provided".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let toml = r#"
[worker]
name = "build-farm-1"
orchestrator_url = "https://fleet.example.com/"
heartbeat_interval_secs = 30
labels = { arch = "arm64" }

[grok]
bin = "/usr/local/bin/grok"
bind_addr = "127.0.0.1:2419"
secret = "topsecret"
max_concurrent_tasks = 8
"#;
        let config = toml.parse::<WorkerConfig>().unwrap();
        assert_eq!(config.worker.name, "build-farm-1");
        // trailing slash 제거 확인.
        assert_eq!(config.worker.orchestrator_url, "https://fleet.example.com");
        assert_eq!(config.worker.heartbeat_interval_secs, 30);
        assert_eq!(config.worker.labels.get("arch").unwrap(), "arm64");
        assert_eq!(config.grok.bin, "/usr/local/bin/grok");
        assert_eq!(config.grok.bind_addr, "127.0.0.1:2419");
        assert_eq!(config.grok.max_concurrent_tasks, 8);
    }

    #[test]
    fn defaults_applied_when_omitted() {
        let toml = r#"
[worker]
name = "minimal"
orchestrator_url = "http://localhost:8080"

[grok]
bin = "/usr/local/bin/grok"
secret = "x"
"#;
        let config = toml.parse::<WorkerConfig>().unwrap();
        assert_eq!(config.worker.heartbeat_interval_secs, 15);
        assert_eq!(config.grok.bind_addr, "127.0.0.1:2419");
        assert_eq!(config.grok.max_concurrent_tasks, 4);
        assert_eq!(config.grok.restart_delay_secs, 5);
    }

    #[test]
    fn rejects_empty_name() {
        let toml = r#"
[worker]
name = ""
orchestrator_url = "http://localhost"

[grok]
bin = "/x"
secret = "x"
"#;
        let err = toml.parse::<WorkerConfig>().unwrap_err();
        assert!(matches!(err, WorkerError::Config(_)));
    }

    #[test]
    fn rejects_invalid_url_scheme() {
        let toml = r#"
[worker]
name = "x"
orchestrator_url = "ftp://nope"

[grok]
bin = "/x"
secret = "x"
"#;
        let err = toml.parse::<WorkerConfig>().unwrap_err();
        assert!(matches!(err, WorkerError::Config(_)));
    }

    #[test]
    fn rejects_invalid_name_chars() {
        let toml = r#"
[worker]
name = "bad name with space"
orchestrator_url = "http://localhost"

[grok]
bin = "/x"
secret = "x"
"#;
        let err = toml.parse::<WorkerConfig>().unwrap_err();
        assert!(matches!(err, WorkerError::Config(_)));
    }

    #[test]
    fn agent_endpoint_includes_secret() {
        let config = WorkerConfig::for_test()
            .orchestrator_url("https://fleet.example.com")
            .grok_secret("topsecret")
            .build();
        let endpoint = config.agent_endpoint();
        assert!(endpoint.starts_with("ws://"));
        assert!(endpoint.contains("server-key=topsecret"));
    }

    #[test]
    fn for_test_builder_works() {
        let config = WorkerConfig::for_test()
            .name("custom")
            .grok_bin("/bin/echo")
            .build();
        assert_eq!(config.worker.name, "custom");
        assert_eq!(config.grok.bin, "/bin/echo");
    }
}
