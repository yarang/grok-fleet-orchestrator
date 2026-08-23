//! 워커(Worker) 도메인 모델.
//!
//! 워커는 원격 Linux 서버에서 실행되는 `grok agent serve` 인스턴스를 추상화합니다.
//! 오케스트레이터는 워커마다 독립적인 연결 상태, 부하, CircuitBreaker 상태를 관리합니다.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::WorkerId;
use crate::task::Labels;

/// 워커 엔티티.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worker {
    pub id: WorkerId,
    /// 사람이 읽을 수 있는 고유 이름 (예: "build-farm-1").
    pub name: String,
    /// 워커의 접속 엔드포인트 (예: "wss://worker-a.fleet.example.com/ws").
    ///
    /// **secret을 담고 있다** — 대부분의 배포에서 `?server-key=<값>` 쿼리
    /// 파라미터가 붙어 있으며, 이는 워커의 grok 서브프로세스 ACP 인증
    /// 토큰 원문이다(로드맵 `#75`). 이 필드를 외부 응답·이벤트·로그로
    /// 그대로 내보내면 안 된다 — [`mask_server_key`]로 마스킹한 뒤에만
    /// 내보낸다. 원문이 필요한 유일한 정당한 소비자는 `fleet-transport`가
    /// 실제로 워커에 다이얼할 때뿐이다.
    pub endpoint: String,
    /// 라벨 맵 (예: {"arch":"arm64", "gpu":"true"}). 작업 라벨 필터에 사용.
    #[serde(default)]
    pub labels: Labels,
    pub status: WorkerStatus,
    /// 마지막 하트비트 수신 시각. `None`이면 한 번도 heartbeat를 받지 않음.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<DateTime<Utc>>,
    /// 현재 이 워커에서 실행 중인 작업 수.
    #[serde(default)]
    pub active_tasks: u32,
    /// 최대 동시 실행 작업 수.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
    /// CircuitBreaker 상태.
    #[serde(default)]
    pub circuit_state: CircuitState,
    /// 워커 사이드카 버전 (예: "0.1.0").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_version: Option<String>,
    /// Liveness 보고 방식 (로드맵 #61). 기본값 `Periodic` — 기존 배포와의
    /// 하위 호환을 위해 필드가 없는 구 페이로드/행은 이 값으로 취급한다.
    /// `OnDemand`는 스키마·모니터링 예외만 이 증분에서 구현되며, 실제 dispatch
    /// 경로(사전 ACP probe 등)는 별도 control-stream 인프라(로드맵 #67) 없이는
    /// 아직 안전하지 않다 — [`docs/architecture/worker-liveness-policy.md`] 참고.
    #[serde(default)]
    pub liveness_mode: WorkerLivenessMode,
    /// 등록 시각.
    pub registered_at: DateTime<Utc>,
}

fn default_max_concurrent() -> u32 {
    4
}

impl Worker {
    /// 새 워커 등록용 생성자.
    pub fn new(name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            id: WorkerId::new(),
            name: name.into(),
            endpoint: endpoint.into(),
            labels: HashMap::new(),
            status: WorkerStatus::Online,
            last_seen: Some(Utc::now()),
            active_tasks: 0,
            max_concurrent: 4,
            circuit_state: CircuitState::Closed,
            worker_version: None,
            liveness_mode: WorkerLivenessMode::default(),
            registered_at: Utc::now(),
        }
    }

    /// 추가 용량이 있는지 (활성 작업 < 최대 동시).
    pub fn has_capacity(&self) -> bool {
        self.active_tasks < self.max_concurrent
    }

    /// 요청된 라벨 집합을 모두 만족하는지.
    pub fn matches_labels(&self, required: &[String]) -> bool {
        required.iter().all(|lbl| self.labels.contains_key(lbl))
    }

    /// dispatch 가능 여부: online + 회로 닫힘 + 용량 있음.
    pub fn is_dispatchable(&self) -> bool {
        matches!(self.status, WorkerStatus::Online)
            && matches!(self.circuit_state, CircuitState::Closed)
            && self.has_capacity()
    }
}

/// 워커 가용성 상태.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    /// 정상 동작 중.
    Online,
    /// 응답 지연 또는 부하 과다 (완전 오프라인은 아님).
    Degraded,
    /// 노드 드레인 중 (이관 준비 및 신규 작업 수령 차단).
    Draining,
    /// 하트비트 누락으로 오프라인 처리됨.
    #[default]
    Offline,
    /// CircuitBreaker가 열려 자동 차단됨.
    CircuitOpen,
}

/// 워커 liveness 보고 방식 (로드맵 #61,
/// `docs/architecture/worker-liveness-policy.md`).
///
/// - `Periodic`: 기본값. `heartbeat_interval_secs`마다 heartbeat 전송,
///   fleet-scheduler의 HealthChecker가 누락 시 Offline으로 전이.
/// - `OnDemand`: idle 시 트래픽 없음. 이 열거값은 스키마/모니터링 예외
///   (HealthChecker skip)만 이번 증분에서 지원한다 — dispatch 직전 ACP probe는
///   아직 구현되지 않았으므로(로드맵 #67 의존) `on_demand`로 설정된 워커에
///   실제로 task를 배정하는 로직은 이 증분의 범위 밖이다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerLivenessMode {
    #[default]
    Periodic,
    OnDemand,
}

/// CircuitBreaker 3상태.
///
/// - `Closed`: 정상. 요청이 통과함.
/// - `Open`: 실패 임계치 도달. 요청이 즉시 차단됨.
/// - `HalfOpen`: 쿨다운 후 1회 프로브 허용. 성공하면 Closed, 실패하면 Open으로 복귀.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    #[default]
    Closed,
    Open,
    HalfOpen,
}

/// 워커 목록 조회용 필터. Store::list_workers에 전달.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<WorkerStatus>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
    #[serde(default = "default_worker_limit")]
    pub limit: usize,
    /// 건너뛸 행 수 (페이지네이션). `TaskFilter`와 동일한 의미.
    #[serde(default)]
    pub offset: usize,
}

impl Default for WorkerFilter {
    fn default() -> Self {
        Self {
            status: None,
            labels: HashMap::new(),
            limit: default_worker_limit(),
            offset: 0,
        }
    }
}

fn default_worker_limit() -> usize {
    100
}

/// 하트비트로 워커가 전달하는 로컬 상태.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerHeartbeat {
    pub worker_id: WorkerId,
    #[serde(default)]
    pub active_tasks: u32,
    /// Unix load average (1, 5, 15분).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub load_avg: Vec<f32>,
    #[serde(default)]
    pub mem_available_mb: u64,
    #[serde(default)]
    pub disk_free_mb: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_usage: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ram_usage: Option<f32>,
    #[serde(default = "default_true")]
    pub agent_healthy: bool,
    /// grok CLI 버전 (예: "0.2.112").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grok_version: Option<String>,
    /// fleet-worker 바이너리 버전 (예: "0.1.0").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_worker_version: Option<String>,
    /// OS 정보 (kernel, distro, arch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_info: Option<OsInfo>,
}

/// 하트비트로 전송되는 OS 식별 정보.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OsInfo {
    /// OS 종류 (예: "linux", "macos").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub os_type: String,
    /// 배포판 또는 버전 (예: "Ubuntu 22.04").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub distro: String,
    /// 커널 버전 (예: "5.15.0-91-generic").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kernel: String,
    /// CPU 아키텍처 (예: "aarch64", "x86_64").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arch: String,
    /// 호스트명 (hostname -s).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hostname: String,
}

fn default_true() -> bool {
    true
}

/// `agent_endpoint`류 문자열에서 `server-key=` 값을 `<redacted>`로 마스킹한다
/// (로드맵 `#75`). `?server-key=<secret>&other=...`/`#fragment` 어느
/// 위치에 있든 다음 `&`/`#` 또는 문자열 끝까지만 값으로 보고 그 구간만
/// 치환한다 — 앞뒤(스킴·호스트·포트·경로·다른 쿼리 파라미터)는 그대로 둔다.
/// `server-key=`가 없으면 원문을 그대로 반환한다(이미 secret이 없는 값을
/// 실수로 바꾸지 않기 위해).
///
/// `Worker.endpoint`/`agent_endpoint`를 **어떤 외부 응답·이벤트·로그로든**
/// 내보내기 전에는 항상 이 함수를 거쳐야 한다 — 원문이 필요한 유일한 소비자는
/// `fleet-transport`가 워커에 실제로 다이얼할 때뿐이다.
pub fn mask_server_key(endpoint: &str) -> String {
    const MARKER: &str = "server-key=";
    let Some(idx) = endpoint.find(MARKER) else {
        return endpoint.to_string();
    };
    let start = idx + MARKER.len();
    let end = endpoint[start..]
        .find(['&', '#'])
        .map(|e| start + e)
        .unwrap_or(endpoint.len());
    format!("{}<redacted>{}", &endpoint[..start], &endpoint[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_worker_defaults() {
        let w = Worker::new("build-farm-1", "wss://localhost:2419/ws");
        assert!(matches!(w.status, WorkerStatus::Online));
        assert!(matches!(w.circuit_state, CircuitState::Closed));
        assert!(w.has_capacity());
        assert!(w.is_dispatchable());
        assert_eq!(w.max_concurrent, 4);
    }

    #[test]
    fn label_matching() {
        let mut w = Worker::new("gpu-1", "wss://gpu/ws");
        w.labels.insert("gpu".into(), "true".into());
        w.labels.insert("arch".into(), "x86_64".into());

        assert!(w.matches_labels(&["gpu".into()]));
        assert!(w.matches_labels(&["gpu".into(), "arch".into()]));
        assert!(!w.matches_labels(&["tpu".into()]));
        assert!(w.matches_labels(&[])); // 빈 라벨은 항상 매칭
    }

    #[test]
    fn capacity_check() {
        let mut w = Worker::new("c1", "wss://x");
        w.max_concurrent = 2;
        assert!(w.has_capacity());
        w.active_tasks = 2;
        assert!(!w.has_capacity());
        assert!(!w.is_dispatchable());
    }

    #[test]
    fn status_snake_case() {
        let s = serde_json::to_string(&WorkerStatus::CircuitOpen).unwrap();
        assert_eq!(s, "\"circuit_open\"");
    }

    #[test]
    fn new_worker_defaults_to_periodic_liveness() {
        let w = Worker::new("build-farm-1", "wss://localhost:2419/ws");
        assert_eq!(w.liveness_mode, WorkerLivenessMode::Periodic);
    }

    #[test]
    fn liveness_mode_snake_case_serialization() {
        assert_eq!(
            serde_json::to_string(&WorkerLivenessMode::Periodic).unwrap(),
            "\"periodic\""
        );
        assert_eq!(
            serde_json::to_string(&WorkerLivenessMode::OnDemand).unwrap(),
            "\"on_demand\""
        );
    }

    #[test]
    fn liveness_mode_deserializes_from_snake_case() {
        let periodic: WorkerLivenessMode = serde_json::from_str("\"periodic\"").unwrap();
        let on_demand: WorkerLivenessMode = serde_json::from_str("\"on_demand\"").unwrap();
        assert_eq!(periodic, WorkerLivenessMode::Periodic);
        assert_eq!(on_demand, WorkerLivenessMode::OnDemand);
    }

    #[test]
    fn worker_json_missing_liveness_mode_defaults_to_periodic() {
        // 하위 호환 회귀 테스트 — 로드맵 #61 이전에 저장/전송된 Worker JSON은
        // liveness_mode 필드가 없다. #[serde(default)]가 없으면 기존 데이터
        // 역직렬화가 깨진다.
        let json = serde_json::json!({
            "id": WorkerId::new(),
            "name": "legacy-worker",
            "endpoint": "wss://legacy/ws",
            "labels": {},
            "status": "online",
            "active_tasks": 0,
            "max_concurrent": 4,
            "circuit_state": "closed",
            "registered_at": Utc::now(),
        });
        let w: Worker = serde_json::from_value(json).unwrap();
        assert_eq!(w.liveness_mode, WorkerLivenessMode::Periodic);
    }

    // ── mask_server_key (로드맵 #75) ────────────────────────────────────

    #[test]
    fn mask_server_key_redacts_query_param_value() {
        let s = mask_server_key("wss://h:1/ws?server-key=topsecret");
        assert!(!s.contains("topsecret"));
        assert!(s.contains("<redacted>"));
    }

    #[test]
    fn mask_server_key_preserves_scheme_host_path() {
        let s = mask_server_key("wss://worker-1.fleet.internal:2420/ws?server-key=topsecret");
        assert!(s.starts_with("wss://worker-1.fleet.internal:2420/ws?server-key="));
    }

    #[test]
    fn mask_server_key_preserves_trailing_query_params() {
        let s = mask_server_key("ws://h/ws/name?server-key=topsecret&foo=bar");
        assert!(!s.contains("topsecret"));
        assert!(s.ends_with("&foo=bar"));
    }

    #[test]
    fn mask_server_key_preserves_fragment() {
        let s = mask_server_key("ws://h/ws?server-key=topsecret#frag");
        assert!(!s.contains("topsecret"));
        assert!(s.ends_with("#frag"));
    }

    #[test]
    fn mask_server_key_leaves_endpoint_without_secret_untouched() {
        let s = mask_server_key("wss://h:1/ws");
        assert_eq!(s, "wss://h:1/ws");
    }

    #[test]
    fn mask_server_key_leaves_empty_string_untouched() {
        assert_eq!(mask_server_key(""), "");
    }
}
