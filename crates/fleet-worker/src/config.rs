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
//!
//! # Phase 8.5: mTLS proxy (worker 앞단). 활성화 시 grok agent serve는
//! # loopback에서만 듣고, 외부 연결은 wss:// 로 mTLS 프록시가 종단한다.
//! [mtls]
//! enabled = true
//! listen_addr = "0.0.0.0:2420"
//! server_cert_path = "/etc/fleet/worker/server.pem"
//! server_key_path = "/etc/fleet/worker/server.key"
//! client_ca_path = "/etc/fleet/ca.pem"
//! advertised_scheme = "wss"            # 기본값 wss
//! advertised_host = "worker-1.fleet"   # orchestrator에게 노출할 호스트명
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
    /// mTLS proxy 설정 (옵션). 누락 시 mTLS 비활성.
    #[serde(default)]
    pub mtls: Option<MtlsSection>,
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

/// `[mtls]` 섹션 (Phase 8.5).
///
/// 활성화 시 `grok agent serve` 앞에 mTLS 종단 TCP proxy가 배치된다.
/// orchestrator는 사설 CA로 서명된 클라이언트 인증서로 자신을 증명해야 한다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtlsSection {
    /// mTLS proxy 활성화 여부. false인 경우 다른 필드는 무시.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// proxy가 청취할 주소. 일반적으로 `0.0.0.0:2420`.
    pub listen_addr: String,
    /// 서버 인증서 PEM 경로 (사설 CA로 서명됨).
    pub server_cert_path: PathBuf,
    /// 서버 비밀키 PEM 경로.
    pub server_key_path: PathBuf,
    /// 클라이언트 인증서 검증용 CA PEM 경로 (orchestrator의 클라이언트 인증서 검증).
    pub client_ca_path: PathBuf,
    /// orchestrator에게 노출할 host (엔드포인트 URL의 호스트 부분).
    /// 미지정 시 worker.orchestrator_url의 호스트 사용.
    #[serde(default)]
    pub advertised_host: Option<String>,
    /// orchestrator에게 노출할 포트. 미지정 시 listen_addr의 포트 사용.
    #[serde(default)]
    pub advertised_port: Option<u16>,
}

fn default_true() -> bool {
    true
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
        if let Some(mtls) = &self.mtls {
            if mtls.enabled {
                if mtls.listen_addr.trim().is_empty() {
                    return Err(WorkerError::Config(
                        "mtls.listen_addr must not be empty when mtls.enabled".into(),
                    ));
                }
                if mtls.server_cert_path.as_os_str().is_empty() {
                    return Err(WorkerError::Config(
                        "mtls.server_cert_path must not be empty when mtls.enabled".into(),
                    ));
                }
                if mtls.server_key_path.as_os_str().is_empty() {
                    return Err(WorkerError::Config(
                        "mtls.server_key_path must not be empty when mtls.enabled".into(),
                    ));
                }
                if mtls.client_ca_path.as_os_str().is_empty() {
                    return Err(WorkerError::Config(
                        "mtls.client_ca_path must not be empty when mtls.enabled".into(),
                    ));
                }
                // listen_addr 이 "host:port" 형태인지 확인.
                if mtls.listen_addr.parse::<std::net::SocketAddr>().is_err() {
                    return Err(WorkerError::Config(format!(
                        "mtls.listen_addr must be host:port — got: {}",
                        mtls.listen_addr
                    )));
                }
            }
        }
        Ok(())
    }

    /// 등록 시 grok agent의 WebSocket endpoint로 노출될 URL.
    /// orchestrator는 이 값을 transport.register()에 전달.
    ///
    /// - mTLS 비활성: `ws://<orchestrator-host>/ws?server-key=...`
    ///   (Phase 7 모델 — cloudflared가 localhost:2419를 터널링한다고 가정)
    /// - mTLS 활성 (Phase 8.5): `wss://<advertised_host>:<advertised_port>/ws?server-key=...`
    pub fn agent_endpoint(&self) -> String {
        let secret = &self.grok.secret;
        match &self.mtls {
            Some(mtls) if mtls.enabled => {
                let host = mtls
                    .advertised_host
                    .as_deref()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| self.orchestrator_url_host().to_string());
                let port = mtls.advertised_port.unwrap_or_else(|| {
                    mtls.listen_addr
                        .parse::<std::net::SocketAddr>()
                        .map(|a| a.port())
                        .unwrap_or(2420)
                });
                format!("wss://{host}:{port}/ws?server-key={secret}")
            }
            _ => {
                let host = self.orchestrator_url_host();
                format!("ws://{host}/ws?server-key={secret}")
            }
        }
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
    mtls: Option<MtlsSection>,
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
    /// mTLS 섹션 오버라이드 (테스트용).
    pub fn mtls(mut self, mtls: MtlsSection) -> Self {
        self.mtls = Some(mtls);
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
            mtls: self.mtls,
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
    fn mtls_disabled_by_default() {
        let config = WorkerConfig::for_test().build();
        assert!(config.mtls.is_none(), "mtls must default to None");
    }

    #[test]
    fn mtls_endpoint_uses_wss_scheme() {
        let mtls = MtlsSection {
            enabled: true,
            listen_addr: "0.0.0.0:2420".into(),
            server_cert_path: "/etc/server.pem".into(),
            server_key_path: "/etc/server.key".into(),
            client_ca_path: "/etc/ca.pem".into(),
            advertised_host: Some("worker-1.fleet".into()),
            advertised_port: Some(2420),
        };
        let config = WorkerConfig::for_test()
            .orchestrator_url("https://fleet.example.com")
            .grok_secret("topsecret")
            .mtls(mtls)
            .build();
        let endpoint = config.agent_endpoint();
        assert!(endpoint.starts_with("wss://"), "got: {endpoint}");
        assert!(endpoint.contains("worker-1.fleet:2420"));
        assert!(endpoint.contains("server-key=topsecret"));
    }

    #[test]
    fn mtls_disabled_does_not_affect_endpoint() {
        let mtls = MtlsSection {
            enabled: false,
            listen_addr: "0.0.0.0:2420".into(),
            server_cert_path: "/etc/server.pem".into(),
            server_key_path: "/etc/server.key".into(),
            client_ca_path: "/etc/ca.pem".into(),
            advertised_host: Some("ignored".into()),
            advertised_port: Some(9999),
        };
        let config = WorkerConfig::for_test()
            .orchestrator_url("https://fleet.example.com")
            .grok_secret("topsecret")
            .mtls(mtls)
            .build();
        let endpoint = config.agent_endpoint();
        assert!(endpoint.starts_with("ws://"), "disabled mtls must keep ws://");
        assert!(!endpoint.contains("ignored"));
    }

    #[test]
    fn mtls_enabled_rejects_invalid_listen_addr() {
        let mtls = MtlsSection {
            enabled: true,
            listen_addr: "not-an-addr".into(),
            server_cert_path: "/x".into(),
            server_key_path: "/x".into(),
            client_ca_path: "/x".into(),
            advertised_host: None,
            advertised_port: None,
        };
        let config = WorkerConfig::for_test().mtls(mtls).build();
        let err = config.validate().unwrap_err();
        assert!(matches!(err, WorkerError::Config(_)));
    }

    #[test]
    fn mtls_enabled_rejects_empty_paths() {
        let mtls = MtlsSection {
            enabled: true,
            listen_addr: "0.0.0.0:2420".into(),
            server_cert_path: "".into(),
            server_key_path: "".into(),
            client_ca_path: "".into(),
            advertised_host: None,
            advertised_port: None,
        };
        let config = WorkerConfig::for_test().mtls(mtls).build();
        let err = config.validate().unwrap_err();
        assert!(matches!(err, WorkerError::Config(_)));
    }

    #[test]
    fn mtls_section_parses_from_toml() {
        let toml = r#"
[worker]
name = "w"
orchestrator_url = "http://localhost:8080"

[grok]
bin = "/x"
secret = "x"

[mtls]
enabled = true
listen_addr = "0.0.0.0:2420"
server_cert_path = "/etc/server.pem"
server_key_path = "/etc/server.key"
client_ca_path = "/etc/ca.pem"
advertised_host = "worker-1.fleet"
"#;
        let config: WorkerConfig = toml.parse().unwrap();
        let mtls = config.mtls.as_ref().expect("mtls section");
        assert!(mtls.enabled);
        assert_eq!(mtls.listen_addr, "0.0.0.0:2420");
        assert_eq!(mtls.server_cert_path, std::path::Path::new("/etc/server.pem"));
        assert_eq!(mtls.advertised_host.as_deref(), Some("worker-1.fleet"));
        assert!(config.agent_endpoint().starts_with("wss://"));
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
