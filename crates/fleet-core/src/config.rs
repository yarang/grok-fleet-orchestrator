//! 오케스트레이터 및 워커 설정 타입.
//!
//! 이 크레이트는 설정 파일 파싱(toml)을 직접 수행하지 않습니다 — `fleet-cli`가
//! 파싱한 결과를 이 타입들로 역직렬화합니다. 여기서는 타입 정의만 제공합니다.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::task::Labels;

/// 오케스트레이터 전역 설정 (`orchestrator.toml`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    #[serde(default)]
    pub store: StoreConfig,
    #[serde(default)]
    pub oidc: Option<OidcConfig>,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
    #[serde(default)]
    pub circuit_breaker: CircuitBreakerConfig,
    /// 정적 등록 워커 (선택). 동적 등록이 기본.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workers: Vec<StaticWorkerConfig>,
}

/// 영속 저장소 설정.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreConfig {
    /// PostgreSQL 연결 문자열.
    /// 환경변수 `${DATABASE_URL}` 치환은 `fleet-cli`가 수행.
    pub database_url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

fn default_max_connections() -> u32 {
    10
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            database_url: "postgres://fleet@localhost/fleet".into(),
            max_connections: 10,
        }
    }
}

/// OIDC 중앙 집중식 인증 설정.
///
/// `OidcAuthProvider`의 생성 인자와 1:1로 대응. `client_secret_env`는
/// 평문 비밀을 설정 파일에 두지 않기 위한 환경변수명.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    /// 평문이 아닌 환경변수명 (예: "FLEET_OIDC_SECRET").
    pub client_secret_env: String,
    /// 요청할 스코프 (예: ["openid", "profile", "fleet:admin"]).
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,
}

fn default_scopes() -> Vec<String> {
    vec!["openid".into(), "profile".into()]
}

/// HTTP API 서버 설정.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default)]
    pub dashboard_enabled: bool,
    /// 임베드된 대시보드 자산 경로 (사용되지 않을 수 있음).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard_path: Option<String>,
}

fn default_bind() -> String {
    "127.0.0.1:8080".into()
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8080".into(),
            dashboard_enabled: false,
            dashboard_path: None,
        }
    }
}

/// 스케줄러 설정.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    #[serde(default = "default_timeout")]
    pub default_timeout_secs: u64,
    #[serde(default = "default_max_per_worker")]
    pub max_concurrent_tasks_per_worker: u32,
    #[serde(default = "default_health_interval")]
    pub health_check_interval_secs: u64,
    /// 하트비트 누락 허용 횟수. 이 횟수를 넘기면 워커를 Offline 처리.
    #[serde(default = "default_missed_heartbeats")]
    pub missed_heartbeat_threshold: u32,
}

fn default_timeout() -> u64 {
    3600
}
fn default_max_per_worker() -> u32 {
    4
}
fn default_health_interval() -> u64 {
    15
}
fn default_missed_heartbeats() -> u32 {
    3
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: 3600,
            max_concurrent_tasks_per_worker: 4,
            health_check_interval_secs: 15,
            missed_heartbeat_threshold: 3,
        }
    }
}

/// CircuitBreaker 튜닝 파라미터.
///
/// grok-build의 `xai_circuit_breaker::BreakerConfig`와 필드가 1:1로 대응합니다.
/// 여기서는 직렬화 가능한 형태(`u64` 초, `Vec<u16>`)로 보관하고,
/// `fleet-scheduler`가 `BreakerConfig`로 변환하여 사용합니다.
///
/// ## 권장값
/// - **서버/오케스트레이터**: `BreakerConfig::server()` 프리셋 (min 10, 50%, 10s)
/// - **클라이언트/워커**: `BreakerConfig::client()` 프리셋 (min 5, 60s, 401만)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// CircuitBreaker 전체 활성화 여부. `false`면 모든 워커가 항상 통과.
    #[serde(default = "default_cb_enabled")]
    pub enabled: bool,
    /// 실패율 계산 윈도우 (초). 이 윈도우 내 샘플만 고려.
    #[serde(default = "default_cb_window")]
    pub window_duration_secs: u64,
    /// 판단을 위한 최소 샘플 수. 이보다 적으면 trip하지 않음.
    #[serde(default = "default_cb_min_samples")]
    pub min_samples: u32,
    /// 이 실패율 이상에서 회로를 엽니다 (0.0 ~ 1.0).
    #[serde(default = "default_cb_error_rate")]
    pub error_rate_threshold: f64,
    /// Open 상태 유지 시간 (초). 이후 HalfOpen으로 전이.
    #[serde(default = "default_cb_open_duration")]
    pub open_duration_secs: u64,
    /// HalfOpen 상태에서 허용할 프로브 요청 수.
    #[serde(default = "default_cb_half_open_probes")]
    pub half_open_max_probes: u32,
    /// 실패로 간주할 HTTP 상태 코드 목록 (예: 429, 500, 502, 503, 504).
    #[serde(default = "default_cb_failure_codes")]
    pub failure_codes: Vec<u16>,
}

fn default_cb_enabled() -> bool {
    true
}
fn default_cb_window() -> u64 {
    60
}
fn default_cb_min_samples() -> u32 {
    10
}
fn default_cb_error_rate() -> f64 {
    0.5
}
fn default_cb_open_duration() -> u64 {
    10
}
fn default_cb_half_open_probes() -> u32 {
    1
}
fn default_cb_failure_codes() -> Vec<u16> {
    vec![429, 500, 502, 503, 504]
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window_duration_secs: 60,
            min_samples: 10,
            error_rate_threshold: 0.5,
            open_duration_secs: 10,
            half_open_max_probes: 1,
            failure_codes: default_cb_failure_codes(),
        }
    }
}

/// 정적 등록 워커 (config.toml의 `[[workers]]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticWorkerConfig {
    pub name: String,
    pub endpoint: String,
    #[serde(default)]
    pub labels: Labels,
}

/// 워커 사이드카 설정 (`/etc/fleet/worker.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSidecarConfig {
    pub orchestrator: WorkerOrchestratorConfig,
    pub worker: WorkerIdentityConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub auth: Option<OidcConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerOrchestratorConfig {
    /// 오케스트레이터 엔드포인트 (예: "wss://orch.fleet.example.com:8443").
    pub url: String,
    /// 디스커버리 메커니즘 (예: "dns:_fleet._tcp.internal.example.com").
    /// `Some`이면 `url` 대신 DNS SRV 조회 결과 사용.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerIdentityConfig {
    pub name: String,
    #[serde(default)]
    pub labels: Labels,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default = "default_max_per_worker")]
    pub max_concurrent_tasks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// `managed`면 사이드카가 `grok agent serve`를 관리.
    /// `external`이면 외부에서 관리되는 에이전트를 참조만.
    #[serde(default = "default_agent_mode")]
    pub mode: String,
    /// 오케스트레이터가 접근할 로컬 에이전트 엔드포인트.
    #[serde(default = "default_agent_endpoint")]
    pub endpoint: String,
    /// 에이전트 인증 비밀을 담은 환경변수명.
    #[serde(default)]
    pub secret_env: Option<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            mode: default_agent_mode(),
            endpoint: default_agent_endpoint(),
            secret_env: None,
        }
    }
}

fn default_agent_mode() -> String {
    "managed".into()
}
fn default_agent_endpoint() -> String {
    "wss://127.0.0.1:2419/ws".into()
}

/// 라벨 문자열 (`"k1=v1,k2=v2"`)을 `Labels`로 파싱. CLI 인자 처리용 편의 함수.
pub fn parse_labels(s: &str) -> Result<HashMap<String, String>, String> {
    let mut out = HashMap::new();
    for pair in s.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| format!("invalid label '{pair}', expected key=value"))?;
        out.insert(k.trim().to_string(), v.trim().to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orchestrator_config_defaults() {
        let c = OrchestratorConfig::default();
        assert_eq!(c.scheduler.default_timeout_secs, 3600);
        assert_eq!(c.scheduler.max_concurrent_tasks_per_worker, 4);
        assert_eq!(c.circuit_breaker.min_samples, 10);
        assert_eq!(c.circuit_breaker.window_duration_secs, 60);
        assert_eq!(c.circuit_breaker.half_open_max_probes, 1);
        assert!(c.circuit_breaker.enabled);
        assert!(c.circuit_breaker.failure_codes.contains(&503));
        assert!(!c.api.dashboard_enabled);
    }

    #[test]
    fn parse_labels_kv() {
        let m = parse_labels("arch=arm64,gpu=true,role=build").unwrap();
        assert_eq!(m.get("arch").unwrap(), "arm64");
        assert_eq!(m.get("gpu").unwrap(), "true");
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn parse_labels_empty_is_empty() {
        let m = parse_labels("").unwrap();
        assert!(m.is_empty());
    }

    #[test]
    fn parse_labels_rejects_bad_input() {
        assert!(parse_labels("no_equals").is_err());
    }

    #[test]
    fn worker_sidecar_config_roundtrip() {
        let toml_str = r#"
[orchestrator]
url = "wss://orch.fleet.example.com:8443"

[worker]
name = "build-farm-1"
labels = { arch = "arm64" }

[agent]
mode = "managed"
endpoint = "wss://127.0.0.1:2419/ws"
"#;
        // toml 파싱 자체는 fleet-cli에서 담당하지만, 역직렬화 결과가
        // 올바른지 검증.
        let cfg: WorkerSidecarConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.worker.name, "build-farm-1");
        assert_eq!(cfg.worker.labels.get("arch").unwrap(), "arm64");
        assert_eq!(cfg.agent.mode, "managed");
    }
}
