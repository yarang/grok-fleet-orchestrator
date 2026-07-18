//! 인벤토리 YAML 파서. `workers.yaml` 형식.
//!
//! ```yaml
//! defaults:
//!   user: ubuntu
//!   ssh_key: ~/.ssh/fleet_workers_ed25519
//!   ssh_port: 22
//!
//! workers:
//!   - host: 203.0.113.10
//!     name: build-farm-1
//!     labels:
//!       arch: arm64
//!       gpu: "false"
//!     region: us-east-1
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
}

impl InventoryWorker {
    /// effective user — 개별값 우선, 그 다음 defaults.
    pub fn effective_user(&self, defaults: &InventoryDefaults) -> String {
        self.user
            .clone()
            .or_else(|| Some(defaults.user.clone()))
            .unwrap()
    }

    pub fn effective_ssh_key(&self, defaults: &InventoryDefaults) -> Result<String, InventoryError> {
        let key = self
            .ssh_key
            .clone()
            .or_else(|| defaults.ssh_key.clone());
        key.ok_or(InventoryError::MissingSshKey)
    }

    pub fn effective_ssh_port(&self, defaults: &InventoryDefaults) -> u16 {
        self.ssh_port.unwrap_or(defaults.ssh_port)
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
    }

    #[test]
    fn worker_specific_overrides_defaults() {
        let inv = Inventory::parse(SAMPLE_YAML).unwrap();
        let w = &inv.workers[1];
        assert_eq!(w.effective_user(&inv.defaults), "admin");
        assert_eq!(w.effective_ssh_port(&inv.defaults), 22);
    }
}
