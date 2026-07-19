//! 작업(Task) 도메인 모델.
//!
//! 작업은 비동기 장기 실행 모델을 따릅니다:
//! 1. 클라이언트가 `Task`를 생성 → 상태 `Pending`
//! 2. 스케줄러가 워커를 선택 → `Dispatched { worker_id }`
//! 3. 워커가 완료 → `Completed(result)` 또는 `Failed(failure)`
//! 4. 도중 취소 → `Cancelled { reason }`

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{TaskId, WorkerId};

/// 작업 우선순위. 스케줄러 큐 정렬에 사용.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskPriority {
    Low,
    #[default]
    Normal,
    High,
}

/// 작업 생성 요청 (클라이언트 → 오케스트레이터).
///
/// `id`, `created_at`은 오케스트레이터가 채웁니다. `Task::from_request` 사용.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskRequest {
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub priority: TaskPriority,
    #[serde(default)]
    pub created_by: String,
}

/// 작업 엔티티 (Store에 영속화되는 형태).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub priority: TaskPriority,
    pub status: TaskStatus,
}

impl Task {
    /// `TaskRequest`에서 새 작업을 생성합니다. `id`와 `created_at`은 자동 발급.
    pub fn from_request(req: TaskRequest) -> Self {
        Self {
            id: TaskId::new(),
            prompt: req.prompt,
            cwd: req.cwd,
            model: req.model,
            server_hint: req.server_hint,
            required_labels: req.required_labels,
            max_turns: req.max_turns,
            timeout_secs: req.timeout_secs,
            created_at: Utc::now(),
            created_by: req.created_by,
            priority: req.priority,
            status: TaskStatus::Pending,
        }
    }

    /// 작업이 종료 상태(Completed/Failed/Cancelled)인지 여부.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            TaskStatus::Completed(_) | TaskStatus::Failed(_) | TaskStatus::Cancelled { .. }
        )
    }

    /// 작업이 현재 워커에서 실행 중인지 여부.
    pub fn is_running(&self) -> bool {
        matches!(self.status, TaskStatus::Dispatched { .. })
    }
}

/// 작업 상태 (상태머신).
///
/// 허용 전이:
/// - `Pending` → `Dispatched` | `Cancelled`
/// - `Dispatched` → `Completed` | `Failed` | `Cancelled`
/// - `Completed` / `Failed` / `Cancelled` → (종료, 전이 불가)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum TaskStatus {
    /// 워커 할당 대기 중.
    #[default]
    Pending,
    /// 워커에서 실행 중.
    Dispatched {
        worker_id: WorkerId,
        started_at: DateTime<Utc>,
    },
    /// 성공적으로 완료.
    Completed(TaskResult),
    /// 실패 (워커 에러, 타임아웃, 회로 차단 등).
    Failed(TaskFailure),
    /// 사용자 요청으로 취소.
    Cancelled {
        reason: String,
        cancelled_at: DateTime<Utc>,
    },
}

/// 작업 완료 결과.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskResult {
    pub output: String,
    pub exit_code: i32,
    pub duration_secs: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
    pub worker_id: WorkerId,
    pub finished_at: DateTime<Utc>,
}

/// 토큰 사용량 (선택적 메트릭).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// 작업 실패 정보.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskFailure {
    pub error: String,
    pub kind: FailureKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<WorkerId>,
    /// 이 작업이 거쳐온 재시도 횟수.
    #[serde(default)]
    pub attempts: u32,
}

/// 실패 원인 분류. 재시도 정책과 모니터링 대시보드에서 사용.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// 워커가 응답하지 않거나 등록 해제됨.
    WorkerUnavailable,
    /// 작업 시간 제한 초과.
    Timeout,
    /// 워커에서 실행 중 발생한 에러 (exit ≠ 0, panic 등).
    WorkerError,
    /// OIDC 토큰 검증 실패 또는 만료.
    AuthFailed,
    /// CircuitBreaker가 열려 있어 dispatch 자체가 차단됨.
    CircuitOpen,
    /// 클라이언트가 취소.
    Cancelled,
}

/// 작업 목록 조회용 필터. Store::list_tasks에 전달.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatusFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<WorkerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

impl Default for TaskFilter {
    fn default() -> Self {
        Self {
            status: None,
            worker_id: None,
            created_by: None,
            limit: default_limit(),
        }
    }
}

fn default_limit() -> usize {
    100
}

/// `TaskFilter`용 단순화된 상태 필터.
/// `TaskStatus` 전체를 비교하기엔 무거우므로 위상(phase)만 매칭.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatusFilter {
    Pending,
    Dispatched,
    Completed,
    Failed,
    Cancelled,
    /// 종료 상태 모두 (Completed | Failed | Cancelled).
    Terminal,
    /// 실행 중 (Pending | Dispatched).
    Active,
}

/// 작업 출력 청크 (스트리밍용). Store에 append-only로 저장.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutputChunk {
    pub task_id: TaskId,
    /// 단조 증가 시퀀스 번호. Store가 발급.
    pub seq: u64,
    /// stdout/stderr 텍스트 청크.
    pub chunk: String,
    /// 청크가 기록된 시각.
    pub written_at: DateTime<Utc>,
}

/// 작업 출력 버퍼에서 읽은 결과.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutput {
    pub task_id: TaskId,
    pub chunks: Vec<TaskOutputChunk>,
    /// 다음 읽기 시작 offset. `from_offset`으로 사용.
    pub next_offset: u64,
}

// HashMap alias for label maps (worker 쪽과 공유).
pub type Labels = HashMap<String, String>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_starts_pending() {
        let req = TaskRequest {
            prompt: "cargo build".into(),
            cwd: None,
            model: None,
            server_hint: None,
            required_labels: vec![],
            max_turns: None,
            timeout_secs: None,
            priority: TaskPriority::Normal,
            created_by: "admin@org".into(),
        };
        let task = Task::from_request(req);
        assert!(matches!(task.status, TaskStatus::Pending));
        assert!(!task.is_terminal());
        assert!(!task.is_running());
    }

    #[test]
    fn status_serializes_as_tagged_enum() {
        let status = TaskStatus::Dispatched {
            worker_id: WorkerId::new(),
            started_at: Utc::now(),
        };
        let json = serde_json::to_value(&status).unwrap();
        // 내부적으로 tag = "phase"
        assert_eq!(json["phase"], "dispatched");
        assert!(json.get("worker_id").is_some());

        let back: TaskStatus = serde_json::from_value(json).unwrap();
        assert_eq!(status, back);
    }

    #[test]
    fn terminal_states_are_detected() {
        let result = TaskResult {
            output: "done".into(),
            exit_code: 0,
            duration_secs: 1.0,
            token_usage: None,
            worker_id: WorkerId::new(),
            finished_at: Utc::now(),
        };
        let t = Task {
            id: TaskId::new(),
            prompt: "x".into(),
            cwd: None,
            model: None,
            server_hint: None,
            required_labels: vec![],
            max_turns: None,
            timeout_secs: None,
            created_at: Utc::now(),
            created_by: "x".into(),
            priority: TaskPriority::Normal,
            status: TaskStatus::Completed(result),
        };
        assert!(t.is_terminal());
        assert!(!t.is_running());
    }

    #[test]
    fn failure_kind_snake_case() {
        let json = serde_json::to_string(&FailureKind::CircuitOpen).unwrap();
        assert_eq!(json, "\"circuit_open\"");
    }

    #[test]
    fn token_usage_total() {
        let u = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 10,
        };
        assert_eq!(u.total(), 150);
    }
}
