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
    /// 하트비트 누락으로 오프라인 처리됨.
    #[default]
    Offline,
    /// CircuitBreaker가 열려 자동 차단됨.
    CircuitOpen,
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
}

impl Default for WorkerFilter {
    fn default() -> Self {
        Self {
            status: None,
            labels: HashMap::new(),
            limit: default_worker_limit(),
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
    #[serde(default = "default_true")]
    pub agent_healthy: bool,
}

fn default_true() -> bool {
    true
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
}
