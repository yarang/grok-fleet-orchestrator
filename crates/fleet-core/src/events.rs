//! 상태 변화 이벤트.
//!
//! 모든 상태 변화는 `FleetEvent`로 append-only 이벤트 로그에 기록됩니다.
//! PostgreSQL의 LISTEN/NOTIFY로 다중 admin/대시보드에 실시간 전파됩니다.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{TaskId, WorkerId};
use crate::task::{TaskFailure, TaskResult};
use crate::worker::CircuitState;

/// Fleet 전역에서 발생하는 이벤트.
///
/// 직렬화 시 `#[serde(tag = "type")]`을 사용하여 JSON 객체의 `type` 필드로
/// 변별됩니다. 이벤트 로그 테이블의 `event_type` 칼럼과 일치.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FleetEvent {
    /// 작업이 생성되어 대기 큐에 진입.
    TaskCreated {
        task_id: TaskId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server_hint: Option<String>,
        created_by: String,
        at: DateTime<Utc>,
    },

    /// 작업이 특정 워커로 dispatch됨.
    TaskDispatched {
        task_id: TaskId,
        worker_id: WorkerId,
        at: DateTime<Utc>,
    },

    /// 워커로부터의 stdout/stderr 청크 (스트리밍).
    TaskProgress {
        task_id: TaskId,
        worker_id: WorkerId,
        /// 단조 증가 시퀀스 (출력 버퍼의 seq와 동일).
        seq: u64,
        chunk: String,
        at: DateTime<Utc>,
    },

    /// 작업이 성공적으로 완료.
    TaskCompleted {
        task_id: TaskId,
        worker_id: WorkerId,
        result: TaskResult,
        at: DateTime<Utc>,
    },

    /// 작업이 실패.
    TaskFailed {
        task_id: TaskId,
        failure: TaskFailure,
        at: DateTime<Utc>,
    },

    /// 작업이 취소됨.
    TaskCancelled {
        task_id: TaskId,
        reason: String,
        at: DateTime<Utc>,
    },

    /// 새 워커가 등록됨.
    WorkerJoined {
        worker_id: WorkerId,
        name: String,
        endpoint: String,
        at: DateTime<Utc>,
    },

    /// 워커가 등록 해제되었거나 하트비트 누락으로 오프라인 처리됨.
    WorkerLeft {
        worker_id: WorkerId,
        reason: String,
        at: DateTime<Utc>,
    },

    /// 워커의 CircuitBreaker 상태 변화.
    WorkerCircuitChanged {
        worker_id: WorkerId,
        from: CircuitState,
        to: CircuitState,
        at: DateTime<Utc>,
    },

    /// 하트비트 수신 (주기적, 상태 요약 포함).
    WorkerHeartbeat {
        worker_id: WorkerId,
        active_tasks: u32,
        agent_healthy: bool,
        at: DateTime<Utc>,
    },
}

impl FleetEvent {
    /// 이벤트 종류 문자열 (이벤트 로그의 `event_type` 칼럼용).
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::TaskCreated { .. } => "task_created",
            Self::TaskDispatched { .. } => "task_dispatched",
            Self::TaskProgress { .. } => "task_progress",
            Self::TaskCompleted { .. } => "task_completed",
            Self::TaskFailed { .. } => "task_failed",
            Self::TaskCancelled { .. } => "task_cancelled",
            Self::WorkerJoined { .. } => "worker_joined",
            Self::WorkerLeft { .. } => "worker_left",
            Self::WorkerCircuitChanged { .. } => "worker_circuit_changed",
            Self::WorkerHeartbeat { .. } => "worker_heartbeat",
        }
    }

    /// 이벤트 발생 시각.
    pub fn at(&self) -> DateTime<Utc> {
        match self {
            Self::TaskCreated { at, .. }
            | Self::TaskDispatched { at, .. }
            | Self::TaskProgress { at, .. }
            | Self::TaskCompleted { at, .. }
            | Self::TaskFailed { at, .. }
            | Self::TaskCancelled { at, .. }
            | Self::WorkerJoined { at, .. }
            | Self::WorkerLeft { at, .. }
            | Self::WorkerCircuitChanged { at, .. }
            | Self::WorkerHeartbeat { at, .. } => *at,
        }
    }

    /// 관련 작업 ID (작업 이벤트인 경우).
    pub fn task_id(&self) -> Option<TaskId> {
        match self {
            Self::TaskCreated { task_id, .. }
            | Self::TaskDispatched { task_id, .. }
            | Self::TaskProgress { task_id, .. }
            | Self::TaskCompleted { task_id, .. }
            | Self::TaskFailed { task_id, .. }
            | Self::TaskCancelled { task_id, .. } => Some(*task_id),
            _ => None,
        }
    }

    /// 관련 워커 ID (있는 경우).
    pub fn worker_id(&self) -> Option<WorkerId> {
        match self {
            Self::TaskDispatched { worker_id, .. }
            | Self::TaskProgress { worker_id, .. }
            | Self::TaskCompleted { worker_id, .. }
            | Self::WorkerJoined { worker_id, .. }
            | Self::WorkerLeft { worker_id, .. }
            | Self::WorkerCircuitChanged { worker_id, .. }
            | Self::WorkerHeartbeat { worker_id, .. } => Some(*worker_id),
            Self::TaskFailed {
                failure:
                    TaskFailure {
                        worker_id: Some(wid),
                        ..
                    },
                ..
            } => Some(*wid),
            _ => None,
        }
    }

    /// 현재 시각로 이벤트를 생성하는 편의 메서드들.
    pub fn task_created(
        task_id: TaskId,
        server_hint: Option<String>,
        created_by: impl Into<String>,
    ) -> Self {
        Self::TaskCreated {
            task_id,
            server_hint,
            created_by: created_by.into(),
            at: Utc::now(),
        }
    }

    pub fn task_dispatched(task_id: TaskId, worker_id: WorkerId) -> Self {
        Self::TaskDispatched {
            task_id,
            worker_id,
            at: Utc::now(),
        }
    }

    pub fn task_completed(task_id: TaskId, worker_id: WorkerId, result: TaskResult) -> Self {
        Self::TaskCompleted {
            task_id,
            worker_id,
            result,
            at: Utc::now(),
        }
    }

    pub fn task_failed(task_id: TaskId, failure: TaskFailure) -> Self {
        Self::TaskFailed {
            task_id,
            failure,
            at: Utc::now(),
        }
    }

    pub fn task_cancelled(task_id: TaskId, reason: impl Into<String>) -> Self {
        Self::TaskCancelled {
            task_id,
            reason: reason.into(),
            at: Utc::now(),
        }
    }

    pub fn worker_joined(
        worker_id: WorkerId,
        name: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self::WorkerJoined {
            worker_id,
            name: name.into(),
            endpoint: endpoint.into(),
            at: Utc::now(),
        }
    }

    pub fn worker_left(worker_id: WorkerId, reason: impl Into<String>) -> Self {
        Self::WorkerLeft {
            worker_id,
            reason: reason.into(),
            at: Utc::now(),
        }
    }

    pub fn worker_circuit_changed(
        worker_id: WorkerId,
        from: CircuitState,
        to: CircuitState,
    ) -> Self {
        Self::WorkerCircuitChanged {
            worker_id,
            from,
            to,
            at: Utc::now(),
        }
    }
}

/// 이벤트 로그에서 (seq, event) 쌍으로 읽어온 형태.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEntry {
    /// 이벤트 로그의 단조 증가 시퀀스.
    pub seq: u64,
    pub event: FleetEvent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_type_tag_roundtrip() {
        let e = FleetEvent::task_created(TaskId::new(), None, "admin@org");
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["type"], "task_created");
        assert!(json.get("task_id").is_some());

        let back: FleetEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back.event_type(), "task_created");
    }

    #[test]
    fn task_progress_carries_seq() {
        let e = FleetEvent::TaskProgress {
            task_id: TaskId::new(),
            worker_id: WorkerId::new(),
            seq: 42,
            chunk: "Compiling...".into(),
            at: Utc::now(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"seq\":42"));
    }

    #[test]
    fn worker_id_extraction() {
        let wid = WorkerId::new();
        let e = FleetEvent::TaskDispatched {
            task_id: TaskId::new(),
            worker_id: wid,
            at: Utc::now(),
        };
        assert_eq!(e.worker_id(), Some(wid));

        let e2 = FleetEvent::task_created(TaskId::new(), None, "x");
        assert_eq!(e2.worker_id(), None);
    }
}
