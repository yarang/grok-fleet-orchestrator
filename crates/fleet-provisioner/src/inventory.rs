//! 인벤토리 YAML 파서. `workers.yaml` 형식.
//!
//! ```yaml
//! defaults:
//!   user: ubuntu
//!   ssh_key: ~/.ssh/fleet_workers_ed25519
//!   ssh_port: 22
//!   # mTLS(선택) — 여러 워커가 공유하는 값만 defaults에 둔다.
//!   # mtls_enabled: true
//!   # mtls_listen_addr: "0.0.0.0:2420"
//!   # mtls_client_ca: /etc/fleet/ca.pem
//!
//! workers:
//!   - host: 203.0.113.10
//!     name: build-farm-1
//!     labels:
//!       arch: arm64
//!       gpu: "false"
//!     region: us-east-1
//!     # mTLS 서버 인증서/키는 워커마다 고유하므로 defaults가 아닌 여기서만 지정.
//!     # mtls_server_cert: /etc/fleet/build-farm-1.pem
//!     # mtls_server_key: /etc/fleet/build-farm-1.key
//!
//! options:
//!   orchestrator_url: https://orch.fleet.example.com
//!   parallel: 3
//!   tags: [setup, tunnel]
//! ```

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::InventoryError;

/// 인벤토리 파일 전체.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    #[serde(default)]
    pub defaults: InventoryDefaults,
    #[serde(default)]
    pub workers: Vec<InventoryWorker>,
    #[serde(default)]
    pub options: ProvisionOptions,
}

/// 공통 기본값. 개별 워커가 오버라이드 가능.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryDefaults {
    #[serde(default = "default_user")]
    pub user: String,
    /// SSH 개인키 경로. defaults에서 필수.
    #[serde(default)]
    pub ssh_key: Option<String>,
    #[serde(default = "default_port")]
    pub ssh_port: u16,
    /// CF 토큰 (필요시 defaults에 지정).
    #[serde(default)]
    pub cf_token: Option<String>,
    /// SSH 서버 호스트 키 검증 정책 (`accept-all` | `tofu` | `strict`).
    /// 미지정 시 CLI 기본값(TOFU)을 따름.
    /// `StrictHostKeyChecking` OpenSSH 설정과 대응.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_key_policy: Option<String>,
    /// `known_hosts` 파일 경로. 미지정 시 `~/.ssh/known_hosts`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_hosts: Option<String>,
    // ── mTLS (로드맵 #37) — 여러 워커에 공유될 만한 값만 defaults에 둔다.
    // 서버 인증서/키(mtls_server_cert/mtls_server_key)는 워커마다 고유해야
    // 하므로 defaults가 아니라 InventoryWorker 전용 필드다.
    /// mTLS 종단 proxy 활성화 (기본값 false). 개별 워커가 `mtls_enabled`로 오버라이드 가능.
    #[serde(default)]
    pub mtls_enabled: bool,
    /// mTLS 리스닝 주소. 보통 모든 워커가 동일 값(예: `0.0.0.0:2420`)을 공유.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtls_listen_addr: Option<String>,
    /// 클라이언트 CA PEM 원격 경로. 보통 모든 워커가 동일 CA를 공유.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtls_client_ca: Option<String>,
    /// orchestrator 에 광고할 포트. 보통 모든 워커가 동일 값을 공유.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtls_advertised_port: Option<u16>,
}

fn default_user() -> String {
    "ubuntu".into()
}

fn default_port() -> u16 {
    22
}

impl Default for InventoryDefaults {
    fn default() -> Self {
        Self {
            user: default_user(),
            ssh_key: None,
            ssh_port: default_port(),
            cf_token: None,
            host_key_policy: None,
            known_hosts: None,
            mtls_enabled: false,
            mtls_listen_addr: None,
            mtls_client_ca: None,
            mtls_advertised_port: None,
        }
    }
}

/// 개별 워커 정의.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryWorker {
    /// 호스트명 또는 IP.
    pub host: String,
    /// 워커 이름 (오케스트레이터에 등록될 이름).
    pub name: String,
    /// user 오버라이드.
    #[serde(default)]
    pub user: Option<String>,
    /// ssh_key 오버라이드.
    #[serde(default)]
    pub ssh_key: Option<String>,
    /// ssh_port 오버라이드.
    #[serde(default)]
    pub ssh_port: Option<u16>,
    /// 라벨.
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// 리전 (메타데이터).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// 이 워커 전용 grok 서버 키 시크릿. worker.toml `[grok] secret`에 기록.
    /// 미설정 시 프로비저닝 단계에서 실패함 — caller가 반드시 채워야 함.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grok_secret: Option<String>,
    // ── mTLS (로드맵 #37) ────────────────────────────────────────────────
    /// `defaults.mtls_enabled` 오버라이드. 특정 워커만 mTLS를 끄거나 켤 때 사용.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtls_enabled: Option<bool>,
    /// `defaults.mtls_listen_addr` 오버라이드.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtls_listen_addr: Option<String>,
    /// 이 워커의 서버 인증서 PEM 원격 절대경로. 워커마다 고유해야 함
    /// (`fleet mtls issue-server`로 사전 발급 — 인증서 파일 자체는 프로비저너가
    /// 업로드하지 않고 worker.toml의 경로만 채운다).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtls_server_cert: Option<String>,
    /// 이 워커의 서버 비밀키 PEM 원격 절대경로.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtls_server_key: Option<String>,
    /// `defaults.mtls_client_ca` 오버라이드.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtls_client_ca: Option<String>,
    /// orchestrator 에 광고할 호스트명. 미지정 시 `name` 필드를 그대로 사용
    /// (대부분의 경우 워커 이름 = mTLS 광고 호스트명이므로).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtls_advertised_host: Option<String>,
    /// `defaults.mtls_advertised_port` 오버라이드.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtls_advertised_port: Option<u16>,
}

impl InventoryWorker {
    /// effective user — 개별값 우선, 그 다음 defaults.
    pub fn effective_user(&self, defaults: &InventoryDefaults) -> String {
        self.user
            .clone()
            .or_else(|| Some(defaults.user.clone()))
            .unwrap()
    }

    pub fn effective_ssh_key(
        &self,
        defaults: &InventoryDefaults,
    ) -> Result<String, InventoryError> {
        let key = self.ssh_key.clone().or_else(|| defaults.ssh_key.clone());
        key.ok_or(InventoryError::MissingSshKey)
    }

    pub fn effective_ssh_port(&self, defaults: &InventoryDefaults) -> u16 {
        self.ssh_port.unwrap_or(defaults.ssh_port)
    }

    /// mTLS 활성화 여부 — 개별값 우선, 그 다음 defaults.
    pub fn effective_mtls_enabled(&self, defaults: &InventoryDefaults) -> bool {
        self.mtls_enabled.unwrap_or(defaults.mtls_enabled)
    }

    /// mTLS 리스닝 주소 — 개별값 우선, 그 다음 defaults.
    pub fn effective_mtls_listen_addr(&self, defaults: &InventoryDefaults) -> Option<String> {
        self.mtls_listen_addr
            .clone()
            .or_else(|| defaults.mtls_listen_addr.clone())
    }

    /// 클라이언트 CA 경로 — 개별값 우선, 그 다음 defaults.
    pub fn effective_mtls_client_ca(&self, defaults: &InventoryDefaults) -> Option<String> {
        self.mtls_client_ca
            .clone()
            .or_else(|| defaults.mtls_client_ca.clone())
    }

    /// 광고 포트 — 개별값 우선, 그 다음 defaults.
    pub fn effective_mtls_advertised_port(&self, defaults: &InventoryDefaults) -> Option<u16> {
        self.mtls_advertised_port.or(defaults.mtls_advertised_port)
    }

    /// 광고 호스트명 — 개별값 우선, 미지정 시 워커 `name`으로 폴백
    /// (server_cert/server_key와 달리 defaults 공유값이 의미 없으므로
    /// defaults를 거치지 않고 곧바로 name으로 대체한다).
    pub fn effective_mtls_advertised_host(&self) -> String {
        self.mtls_advertised_host
            .clone()
            .unwrap_or_else(|| self.name.clone())
    }
}

/// 프로비저닝 옵션.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionOptions {
    /// 오케스트레이터 URL.
    #[serde(default)]
    pub orchestrator_url: Option<String>,
    /// 동시 프로비저닝 수 (기본 1).
    #[serde(default = "default_parallel")]
    pub parallel: usize,
    /// 실패 시 재시도 여부.
    #[serde(default = "default_true")]
    pub retry_failed: bool,
    /// 특정 태그만 실행.
    #[serde(default)]
    pub tags: Vec<String>,
    /// 특정 워커만 실행.
    #[serde(default)]
    pub only: Vec<String>,
    /// Dry-run 모드.
    #[serde(default)]
    pub dry_run: bool,
    /// 오케스트레이터 등록용 bootstrap bearer 토큰. 모든 워커에 동일 적용.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap_token: Option<String>,
    /// 오케스트레이터 관리 API 용 bearer 토큰 (PushCredentials 스텝이 사용).
    /// bootstrap_token 과 별개 — `/v1/workers/:name/credentials` 권한 필요.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token: Option<String>,
}

fn default_parallel() -> usize {
    1
}

fn default_true() -> bool {
    true
}

impl Default for ProvisionOptions {
    fn default() -> Self {
        Self {
            orchestrator_url: None,
            parallel: default_parallel(),
            retry_failed: true,
            tags: vec![],
            only: vec![],
            dry_run: false,
            bootstrap_token: None,
            api_token: None,
        }
    }
}

impl Inventory {
    /// 파일에서 로드.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, InventoryError> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content)
    }

    /// YAML 문자열에서 로드.
    pub fn parse(content: &str) -> Result<Self, InventoryError> {
        let inv: Inventory = serde_yaml::from_str(content)?;
        inv.validate()?;
        Ok(inv)
    }

    /// 유효성 검사.
    pub fn validate(&self) -> Result<(), InventoryError> {
        if self.workers.is_empty() {
            return Err(InventoryError::Empty);
        }
        for w in &self.workers {
            if w.host.is_empty() {
                return Err(InventoryError::MissingHost(w.name.clone()));
            }
            if w.name.is_empty() {
                return Err(InventoryError::MissingName(w.host.clone()));
            }
            // ssh_key는 defaults 또는 개별에 있어야 함.
            let _ = w.effective_ssh_key(&self.defaults)?;
        }
        Ok(())
    }

    /// `options.only`로 필터링된 워커 반환.
    pub fn filtered_workers(&self) -> Vec<&InventoryWorker> {
        if self.options.only.is_empty() {
            self.workers.iter().collect()
        } else {
            self.workers
                .iter()
                .filter(|w| self.options.only.iter().any(|n| n == &w.name))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_YAML: &str = r#"
defaults:
  user: ubuntu
  ssh_key: ~/.ssh/fleet_workers_ed25519
  ssh_port: 22

workers:
  - host: 203.0.113.10
    name: build-farm-1
    labels:
      arch: arm64
      gpu: "false"
    region: us-east-1

  - host: 203.0.113.20
    name: gpu-runner-1
    user: admin
    ssh_key: ~/.ssh/admin_key
    labels:
      arch: x86_64
      gpu: "true"

options:
  orchestrator_url: https://orch.fleet.example.com
  parallel: 3
  tags: [setup, tunnel]
"#;

    #[test]
    fn parses_sample_inventory() {
        let inv = Inventory::parse(SAMPLE_YAML).unwrap();
        assert_eq!(inv.workers.len(), 2);
        assert_eq!(inv.workers[0].name, "build-farm-1");
        assert_eq!(inv.workers[0].host, "203.0.113.10");
        assert_eq!(inv.workers[1].effective_user(&inv.defaults), "admin");
        assert_eq!(
            inv.workers[1].effective_ssh_key(&inv.defaults).unwrap(),
            "~/.ssh/admin_key"
        );
        assert_eq!(inv.options.parallel, 3);
        assert_eq!(inv.options.tags, vec!["setup", "tunnel"]);
    }

    #[test]
    fn validate_rejects_empty_inventory() {
        let yaml = "defaults:\n  ssh_key: /x\nworkers: []\n";
        let result = Inventory::parse(yaml);
        assert!(matches!(result, Err(InventoryError::Empty)));
    }

    #[test]
    fn validate_rejects_missing_ssh_key() {
        let yaml = r#"
workers:
  - host: 1.2.3.4
    name: foo
defaults:
  user: ubuntu
"#;
        let result = Inventory::parse(yaml);
        assert!(matches!(result, Err(InventoryError::MissingSshKey)));
    }

    #[test]
    fn validate_rejects_missing_host() {
        let yaml = r#"
workers:
  - host: ""
    name: foo
defaults:
  ssh_key: /x
"#;
        let result = Inventory::parse(yaml);
        assert!(matches!(result, Err(InventoryError::MissingHost(_))));
    }

    #[test]
    fn filtered_workers_returns_all_when_only_empty() {
        let inv = Inventory::parse(SAMPLE_YAML).unwrap();
        assert_eq!(inv.filtered_workers().len(), 2);
    }

    #[test]
    fn filtered_workers_respects_only() {
        let mut inv = Inventory::parse(SAMPLE_YAML).unwrap();
        inv.options.only = vec!["build-farm-1".into()];
        let filtered = inv.filtered_workers();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "build-farm-1");
    }

    #[test]
    fn defaults_provide_sensible_defaults() {
        let d = InventoryDefaults::default();
        assert_eq!(d.user, "ubuntu");
        assert_eq!(d.ssh_port, 22);
        assert!(d.ssh_key.is_none());
        assert!(d.host_key_policy.is_none());
        assert!(d.known_hosts.is_none());
    }

    #[test]
    fn parses_host_key_policy_and_known_hosts_in_defaults() {
        let yaml = r#"
defaults:
  user: ubuntu
  ssh_key: ~/.ssh/fleet_workers_ed25519
  host_key_policy: strict
  known_hosts: /etc/fleet/known_hosts
workers:
  - host: 10.0.0.1
    name: w1
"#;
        let inv = Inventory::parse(yaml).unwrap();
        assert_eq!(inv.defaults.host_key_policy.as_deref(), Some("strict"));
        assert_eq!(
            inv.defaults.known_hosts.as_deref(),
            Some("/etc/fleet/known_hosts")
        );
    }

    #[test]
    fn host_key_policy_optional_and_omittable() {
        // 생략해도 파싱 성공 (기본 None).
        let yaml = r#"
defaults:
  user: ubuntu
  ssh_key: /x
workers:
  - host: 10.0.0.1
    name: w1
"#;
        let inv = Inventory::parse(yaml).unwrap();
        assert!(inv.defaults.host_key_policy.is_none());
        assert!(inv.defaults.known_hosts.is_none());
    }

    #[test]
    fn worker_specific_overrides_defaults() {
        let inv = Inventory::parse(SAMPLE_YAML).unwrap();
        let w = &inv.workers[1];
        assert_eq!(w.effective_user(&inv.defaults), "admin");
        assert_eq!(w.effective_ssh_port(&inv.defaults), 22);
    }

    // ── mTLS (로드맵 #37) ────────────────────────────────────────────────

    #[test]
    fn mtls_disabled_by_default() {
        let inv = Inventory::parse(SAMPLE_YAML).unwrap();
        assert!(!inv.defaults.mtls_enabled);
        assert!(!inv.workers[0].effective_mtls_enabled(&inv.defaults));
    }

    #[test]
    fn parses_shared_mtls_defaults_and_per_worker_cert_paths() {
        let yaml = r#"
defaults:
  ssh_key: ~/.ssh/fleet_workers_ed25519
  mtls_enabled: true
  mtls_listen_addr: "0.0.0.0:2420"
  mtls_client_ca: /etc/fleet/ca.pem
  mtls_advertised_port: 2420
workers:
  - host: 10.0.0.1
    name: worker-1
    mtls_server_cert: /etc/fleet/worker-1.pem
    mtls_server_key: /etc/fleet/worker-1.key
  - host: 10.0.0.2
    name: worker-2
    mtls_server_cert: /etc/fleet/worker-2.pem
    mtls_server_key: /etc/fleet/worker-2.key
"#;
        let inv = Inventory::parse(yaml).unwrap();
        let w1 = &inv.workers[0];

        assert!(w1.effective_mtls_enabled(&inv.defaults));
        assert_eq!(
            w1.effective_mtls_listen_addr(&inv.defaults).as_deref(),
            Some("0.0.0.0:2420")
        );
        assert_eq!(
            w1.effective_mtls_client_ca(&inv.defaults).as_deref(),
            Some("/etc/fleet/ca.pem")
        );
        assert_eq!(w1.effective_mtls_advertised_port(&inv.defaults), Some(2420));
        assert_eq!(
            w1.mtls_server_cert.as_deref(),
            Some("/etc/fleet/worker-1.pem")
        );
        assert_eq!(
            w1.mtls_server_key.as_deref(),
            Some("/etc/fleet/worker-1.key")
        );

        // 워커별 cert/key는 공유되지 않는다 — 각자 고유.
        let w2 = &inv.workers[1];
        assert_eq!(
            w2.mtls_server_cert.as_deref(),
            Some("/etc/fleet/worker-2.pem")
        );
        assert_ne!(w1.mtls_server_cert, w2.mtls_server_cert);
    }

    #[test]
    fn mtls_advertised_host_falls_back_to_worker_name() {
        let yaml = r#"
defaults:
  ssh_key: /x
workers:
  - host: 10.0.0.1
    name: worker-1
"#;
        let inv = Inventory::parse(yaml).unwrap();
        assert_eq!(inv.workers[0].effective_mtls_advertised_host(), "worker-1");
    }

    #[test]
    fn mtls_advertised_host_explicit_override_wins() {
        let yaml = r#"
defaults:
  ssh_key: /x
workers:
  - host: 10.0.0.1
    name: worker-1
    mtls_advertised_host: worker-1.fleet.internal
"#;
        let inv = Inventory::parse(yaml).unwrap();
        assert_eq!(
            inv.workers[0].effective_mtls_advertised_host(),
            "worker-1.fleet.internal"
        );
    }

    #[test]
    fn per_worker_mtls_enabled_overrides_defaults() {
        // defaults에서 mTLS를 켜뒀어도 특정 워커만 끌 수 있다.
        let yaml = r#"
defaults:
  ssh_key: /x
  mtls_enabled: true
workers:
  - host: 10.0.0.1
    name: no-mtls-worker
    mtls_enabled: false
  - host: 10.0.0.2
    name: mtls-worker
"#;
        let inv = Inventory::parse(yaml).unwrap();
        assert!(!inv.workers[0].effective_mtls_enabled(&inv.defaults));
        assert!(inv.workers[1].effective_mtls_enabled(&inv.defaults));
    }
}
