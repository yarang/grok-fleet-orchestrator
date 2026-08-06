//! 오케스트레이터 설정 타입 + 기동 시 환경변수 검증.
//!
//! ## 설정 모델
//!
//! 오케스트레이터는 **환경변수로만** 구성합니다 (`DATABASE_URL`, `FLEET_BASE_URL`,
//! `FLEET_GMAIL_*` 등). `orchestrator.toml`을 역직렬화하던 타입들
//! (`OrchestratorConfig`, `StoreConfig`, `OidcConfig`, `ApiConfig`,
//! `SchedulerConfig`, `StaticWorkerConfig`, `WorkerSidecarConfig` 등)은
//! 워크스페이스 어디에서도 역직렬화되지 않는 죽은 코드였으므로 제거했습니다.
//! 설정 파일을 다시 도입한다면 실제 로딩 경로와 함께 추가해야 합니다.
//!
//! 워커 사이드카는 예외로 TOML을 사용하며, `fleet-worker` 크레이트가 자체
//! `WorkerConfig`로 파싱합니다 (이 모듈과 무관).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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

// ═══════════════════════════════════════════════════════════════════════
//  기동 시 환경변수 검증
// ═══════════════════════════════════════════════════════════════════════

/// 설정 항목 하나에 대한 문제.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigIssue {
    /// 문제가 있는 환경변수 이름.
    pub key: &'static str,
    /// 사람이 읽을 수 있는 문제 설명 + 조치 방법.
    pub problem: String,
}

impl std::fmt::Display for ConfigIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.key, self.problem)
    }
}

/// 오케스트레이터 기동에 필요한 환경변수를 검증한다.
///
/// 잘못된 설정으로 기동한 뒤 런타임에 조용히 실패하는 것(예: 이메일 링크가
/// 잘못된 호스트를 가리키거나, SMTP 자격증명이 반쪽만 설정되어 메일이
/// 발송되지 않는 상황)을 막기 위해 **기동 시점에** 한 번에 보고한다.
///
/// 반환된 `Vec`가 비어 있지 않으면 기동을 중단해야 한다.
pub fn validate_orchestrator_env() -> Vec<ConfigIssue> {
    validate_env_with(|k| std::env::var(k).ok())
}

/// [`validate_orchestrator_env`]의 테스트 가능한 형태.
///
/// 실제 프로세스 환경을 건드리지 않고 검증 규칙만 시험할 수 있도록
/// 조회 함수를 주입받는다 (`std::env::set_var`는 테스트 병렬 실행 시 경쟁 발생).
pub fn validate_env_with(get: impl Fn(&str) -> Option<String>) -> Vec<ConfigIssue> {
    let mut issues = Vec::new();

    // ── DATABASE_URL — 필수 ──
    match get("DATABASE_URL") {
        None => issues.push(ConfigIssue {
            key: "DATABASE_URL",
            problem: "설정되지 않았습니다. 예: postgres://user@host/dbname".into(),
        }),
        Some(url) if url.trim().is_empty() => issues.push(ConfigIssue {
            key: "DATABASE_URL",
            problem: "값이 비어 있습니다.".into(),
        }),
        Some(url) if !(url.starts_with("postgres://") || url.starts_with("postgresql://")) => {
            issues.push(ConfigIssue {
                key: "DATABASE_URL",
                problem: "postgres:// 또는 postgresql:// 로 시작해야 합니다.".into(),
            });
        }
        Some(_) => {}
    }

    // ── FLEET_BASE_URL — 선택이지만, 설정했다면 형식이 맞아야 함 ──
    // 이메일 인증/비밀번호 재설정 링크의 기준 주소. 잘못되면 사용자가 받는
    // 링크가 통째로 깨지는데 서버 로그에는 아무 흔적이 남지 않는다.
    if let Some(base) = get("FLEET_BASE_URL") {
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            issues.push(ConfigIssue {
                key: "FLEET_BASE_URL",
                problem: "http:// 또는 https:// 로 시작해야 합니다.".into(),
            });
        } else if base.ends_with('/') {
            issues.push(ConfigIssue {
                key: "FLEET_BASE_URL",
                problem: "끝의 '/'를 제거하세요. 링크가 '//path' 형태로 생성됩니다.".into(),
            });
        }
    }

    // ── Gmail SMTP — 짝이 맞아야 함 ──
    // 한쪽만 설정하면 메일 발송이 조용히 비활성화되어, 가입/재설정 플로우가
    // 오류 없이 중단된다.
    let gmail_user = get("FLEET_GMAIL_USER").filter(|v| !v.trim().is_empty());
    let gmail_pass = get("FLEET_GMAIL_APP_PASS").filter(|v| !v.trim().is_empty());
    match (&gmail_user, &gmail_pass) {
        (Some(_), None) => issues.push(ConfigIssue {
            key: "FLEET_GMAIL_APP_PASS",
            problem: "FLEET_GMAIL_USER만 설정되어 있습니다. 앱 비밀번호도 함께 설정하세요.".into(),
        }),
        (None, Some(_)) => issues.push(ConfigIssue {
            key: "FLEET_GMAIL_USER",
            problem: "FLEET_GMAIL_APP_PASS만 설정되어 있습니다. 계정도 함께 설정하세요.".into(),
        }),
        _ => {}
    }

    // ── FLEET_TRUSTED_PROXIES — 선택이지만, 설정했다면 형식이 맞아야 함 ──
    if let Some(proxies_str) = get("FLEET_TRUSTED_PROXIES") {
        let proxies_str = proxies_str.trim();
        if !proxies_str.is_empty() {
            for part in proxies_str.split(',') {
                let part = part.trim();
                if !part.is_empty() && part.parse::<std::net::IpAddr>().is_err() {
                    issues.push(ConfigIssue {
                        key: "FLEET_TRUSTED_PROXIES",
                        problem: format!("유효하지 않은 IP 주소입니다: '{part}'"),
                    });
                }
            }
        }
    }

    issues
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

    /// 검증 규칙 시험용 환경 조회 함수 (실제 프로세스 환경 미사용).
    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn circuit_breaker_config_defaults() {
        let c = CircuitBreakerConfig::default();
        assert_eq!(c.min_samples, 10);
        assert_eq!(c.window_duration_secs, 60);
        assert_eq!(c.half_open_max_probes, 1);
        assert!(c.enabled);
        assert!(c.failure_codes.contains(&503));
    }

    #[test]
    fn env_validation_accepts_minimal_valid_config() {
        let issues = validate_env_with(env_of(&[(
            "DATABASE_URL",
            "postgres://fleet@localhost/fleet",
        )]));
        assert!(issues.is_empty(), "issues: {issues:?}");
    }

    #[test]
    fn env_validation_requires_database_url() {
        let issues = validate_env_with(env_of(&[]));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "DATABASE_URL");
    }

    #[test]
    fn env_validation_rejects_non_postgres_database_url() {
        let issues = validate_env_with(env_of(&[("DATABASE_URL", "mysql://host/db")]));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "DATABASE_URL");
    }

    #[test]
    fn env_validation_rejects_base_url_without_scheme() {
        let issues = validate_env_with(env_of(&[
            ("DATABASE_URL", "postgres://fleet@localhost/fleet"),
            ("FLEET_BASE_URL", "fleet.example.com"),
        ]));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "FLEET_BASE_URL");
    }

    #[test]
    fn env_validation_rejects_base_url_trailing_slash() {
        let issues = validate_env_with(env_of(&[
            ("DATABASE_URL", "postgres://fleet@localhost/fleet"),
            ("FLEET_BASE_URL", "https://fleet.example.com/"),
        ]));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "FLEET_BASE_URL");
    }

    #[test]
    fn env_validation_rejects_half_configured_smtp() {
        let issues = validate_env_with(env_of(&[
            ("DATABASE_URL", "postgres://fleet@localhost/fleet"),
            ("FLEET_GMAIL_USER", "ops@example.com"),
        ]));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "FLEET_GMAIL_APP_PASS");

        let issues = validate_env_with(env_of(&[
            ("DATABASE_URL", "postgres://fleet@localhost/fleet"),
            ("FLEET_GMAIL_APP_PASS", "secret"),
        ]));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "FLEET_GMAIL_USER");
    }

    #[test]
    fn env_validation_accepts_fully_configured_smtp() {
        let issues = validate_env_with(env_of(&[
            ("DATABASE_URL", "postgres://fleet@localhost/fleet"),
            ("FLEET_BASE_URL", "https://fleet.example.com"),
            ("FLEET_GMAIL_USER", "ops@example.com"),
            ("FLEET_GMAIL_APP_PASS", "secret"),
        ]));
        assert!(issues.is_empty(), "issues: {issues:?}");
    }

    #[test]
    fn env_validation_rejects_invalid_trusted_proxies() {
        let issues = validate_env_with(env_of(&[
            ("DATABASE_URL", "postgres://fleet@localhost/fleet"),
            ("FLEET_TRUSTED_PROXIES", "127.0.0.1,invalid-ip"),
        ]));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "FLEET_TRUSTED_PROXIES");
    }

    #[test]
    fn env_validation_accepts_valid_trusted_proxies() {
        let issues = validate_env_with(env_of(&[
            ("DATABASE_URL", "postgres://fleet@localhost/fleet"),
            ("FLEET_TRUSTED_PROXIES", "127.0.0.1, ::1, 10.0.0.1"),
        ]));
        assert!(issues.is_empty(), "issues: {issues:?}");
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
}
