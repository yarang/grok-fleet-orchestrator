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

use crate::ids::{ProjectId, TaskId, WorkerId};

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
    /// 선택 Project 경계. `None`은 기존과 동일한 일반 풀 Task를 뜻한다.
    /// Project 정책 검증은 control plane 구현 단계에서 수행한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
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
    /// 이 태스크가 "이어가기(Reply)"라면, 직전 태스크의 id.
    /// `None`이면 새 스레드의 루트 태스크.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<TaskId>,
    /// DAG 체이닝용 선행 태스크 ID 목록.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependency_ids: Vec<TaskId>,
    /// 요구 에이전트 스킬 목록.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills_required: Vec<String>,
    /// 요구 논리적 프로파일 (`economy` | `balanced` | `complex` | `reasoning` | `auto`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_profile: Option<String>,
    /// 라우터가 최종 선정한 물리 모델명 (사전 지정 또는 라우팅 결과).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_model: Option<String>,
    /// 할당된 최대 토큰 예산 (Soft-Landing 기준).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatched_at: Option<DateTime<Utc>>,
    /// 스레드(연속 대화) 전체를 한 번에 조회하기 위한 평평한 키.
    /// 스레드 루트 태스크는 자기 자신의 `id`를 그대로 갖고, 이어지는 모든
    /// 자식 태스크는 부모의 `thread_id`를 그대로 물려받는다 — `parent_task_id`를
    /// 재귀로 거슬러 올라가지 않고 `WHERE thread_id = ?` 한 번으로 스레드 전체를
    /// 구하기 위함.
    pub thread_id: TaskId,
    /// 이 태스크가 "이어가기(Reply)"라면, 직전 태스크의 id. `None`이면 스레드 루트.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<TaskId>,
    /// 예약 필드 — project 그룹화 기능 도입 전까지는 항상 `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
    /// dispatch 재시도 횟수 (로드맵 #38). `submit()`의 최초 시도 또는
    /// `Reconciler`의 stale-Pending 재시도가 `WorkerUnavailable`/`CircuitOpen`으로
    /// 실패할 때마다 1씩 증가한다 — 이 값이 `max_dispatch_retries`에 도달하면
    /// 더 이상 재시도하지 않고 `Failed`(dead-letter)로 전이된다.
    #[serde(default)]
    pub retry_count: u32,
    /// DAG 체이닝용 선행 태스크 ID 목록.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependency_ids: Vec<TaskId>,
    /// 작업 마이그레이션 이관용 Git 임시 브랜치명.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_branch: Option<String>,
    /// 요구 에이전트 스킬 목록.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills_required: Vec<String>,
    /// 요구 논리적 프로파일 (`economy` | `balanced` | `complex` | `reasoning` | `auto`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_profile: Option<String>,
    /// 라우터가 최종 선정한 물리 모델명.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_model: Option<String>,
    /// 할당된 최대 토큰 예산.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u64>,
    /// 예산 초과(Hard Abort) 시 보존된 중간 git diff 또는 요약.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_output: Option<String>,
}

impl Task {
    /// `TaskRequest`에서 새 작업을 생성합니다. `id`와 `created_at`은 자동 발급.
    ///
    /// `thread_id`는 일단 자기 자신의 `id`로 채워진다 — 스레드 루트라고 가정한
    /// 기본값이다. `parent_task_id`가 채워진 "이어가기" 요청이라면, 호출자가
    /// 부모 태스크를 조회해 `thread_id`(부모의 것으로 교체)와 host/cwd/model
    /// 상속을 직접 반영해야 한다 — 이 함수는 DB 접근이 없는 순수 함수라서
    /// 부모 조회 자체는 할 수 없다.
    pub fn from_request(req: TaskRequest) -> Self {
        let id = TaskId::new();
        Self {
            id,
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
            dispatched_at: None,
            thread_id: id,
            parent_task_id: req.parent_task_id,
            project_id: req.project_id,
            retry_count: 0,
            dependency_ids: req.dependency_ids,
            checkpoint_branch: None,
            skills_required: req.skills_required,
            routing_profile: req.routing_profile,
            resolved_model: req.resolved_model,
            token_budget: req.token_budget,
            partial_output: None,
        }
    }

    /// 부모 태스크로부터 스레드 정보(`thread_id`)와 host/cwd/model 기본값을
    /// 상속시킨다. 우선순위(높은 것부터): 사용자가 명시적으로 지정한 값,
    /// 부모 태스크 값, `None`. `submit_task_api`(대시보드) 등 "이어가기"
    /// 제출 경로에서 `Task::from_request` 직후 호출한다.
    pub fn inherit_from_parent(&mut self, parent: &Task) {
        self.parent_task_id = Some(parent.id);
        self.thread_id = parent.thread_id;
        if self.server_hint.is_none() {
            self.server_hint = parent.server_hint.clone();
        }
        if self.cwd.is_none() {
            self.cwd = parent.cwd.clone();
        }
        if self.model.is_none() {
            self.model = parent.model.clone();
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
///
/// 여기 있는 variant는 전부 저장소 어딘가에서 실제로 생성된다 — 아무도
/// 만들지 않는 variant는 재시도 정책·metric 분해를 조용히 잘못 이해하게
/// 만드는 죽은 코드다(로드맵 `#70`, `#90` 흡수 조사에서 `Timeout`·
/// `AuthFailed`·`Cancelled` 세 variant가 이 기준을 어기고 있음을 확인해
/// 제거했다). 새 variant를 추가할 때는 실제 생성 지점을 함께 만든다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// 워커가 응답하지 않거나 등록 해제됨.
    WorkerUnavailable,
    /// 워커에서 실행 중 발생한 에러 (exit ≠ 0, panic 등).
    WorkerError,
    /// CircuitBreaker가 열려 있어 dispatch 자체가 차단됨.
    CircuitOpen,
    /// 요청된 model의 credential을 보유한 워커가 하나도 없어 dispatch 후보가
    /// 전부 걸러짐 (로드맵 #71). `WorkerUnavailable`과 달리 원인이 명확히
    /// "credential 미프로비저닝"이므로, 재시도로는 해소되지 않고
    /// `fleet provision`의 `PushCredentials` 스텝으로 해당 워커에 credential을
    /// 배포해야 해소된다.
    CredentialMissing,
}

impl FailureKind {
    /// 모든 variant를 순서대로 나열 — metric label 등 전량 순회가 필요한
    /// 곳에서 사용(새 variant 추가를 컴파일러가 강제하도록 이 배열도 함께
    /// 갱신해야 한다).
    pub const ALL: [FailureKind; 4] = [
        FailureKind::WorkerUnavailable,
        FailureKind::WorkerError,
        FailureKind::CircuitOpen,
        FailureKind::CredentialMissing,
    ];

    /// Prometheus label 등 안정적인 텍스트 표현이 필요한 곳에서 사용.
    /// `#[serde(rename_all = "snake_case")]`가 만드는 JSON 표현과 동일한
    /// 문자열이다.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkerUnavailable => "worker_unavailable",
            Self::WorkerError => "worker_error",
            Self::CircuitOpen => "circuit_open",
            Self::CredentialMissing => "credential_missing",
        }
    }
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
    #[serde(default)]
    pub offset: usize,
}

impl Default for TaskFilter {
    fn default() -> Self {
        Self {
            status: None,
            worker_id: None,
            created_by: None,
            limit: default_limit(),
            offset: 0,
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
            created_by: "admin@org".into(),
            ..Default::default()
        };
        let task = Task::from_request(req);
        assert!(matches!(task.status, TaskStatus::Pending));
        assert!(!task.is_terminal());
        assert!(!task.is_running());
    }

    #[test]
    fn request_project_id_is_retained() {
        let project_id = ProjectId::new();
        let task = Task::from_request(TaskRequest {
            prompt: "project task".into(),
            project_id: Some(project_id),
            ..Default::default()
        });
        assert_eq!(task.project_id, Some(project_id));
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
        let mut t = Task::from_request(TaskRequest {
            prompt: "x".into(),
            created_by: "x".into(),
            ..Default::default()
        });
        t.status = TaskStatus::Completed(result);
        assert!(t.is_terminal());
        assert!(!t.is_running());
    }

    #[test]
    fn fresh_task_is_root_of_its_own_thread() {
        let task = Task::from_request(TaskRequest {
            prompt: "start".into(),
            created_by: "admin@org".into(),
            ..Default::default()
        });
        assert_eq!(task.thread_id, task.id);
        assert!(task.parent_task_id.is_none());
    }

    #[test]
    fn inherit_from_parent_adopts_thread_id_and_unset_fields() {
        let mut parent = Task::from_request(TaskRequest {
            prompt: "parent prompt".into(),
            created_by: "admin@org".into(),
            server_hint: Some("worker-arm1".into()),
            cwd: Some("/home/worker/repo".into()),
            model: Some("gemini".into()),
            ..Default::default()
        });
        // 부모 자신도 스레드 루트이므로 thread_id == parent.id.
        parent.thread_id = parent.id;

        let mut reply = Task::from_request(TaskRequest {
            prompt: "이어서 해줘".into(),
            created_by: "admin@org".into(),
            ..Default::default()
        });
        reply.inherit_from_parent(&parent);

        assert_eq!(reply.parent_task_id, Some(parent.id));
        assert_eq!(reply.thread_id, parent.thread_id);
        assert_eq!(reply.server_hint, parent.server_hint);
        assert_eq!(reply.cwd, parent.cwd);
        assert_eq!(reply.model, parent.model);
    }

    #[test]
    fn inherit_from_parent_preserves_explicit_override() {
        let parent = Task::from_request(TaskRequest {
            prompt: "parent prompt".into(),
            created_by: "admin@org".into(),
            server_hint: Some("worker-arm1".into()),
            cwd: Some("/home/worker/repo".into()),
            model: Some("gemini".into()),
            ..Default::default()
        });

        // 사용자가 폼에서 명시적으로 다른 host를 지정한 경우 — 부모 값으로
        // 덮어써지면 안 된다.
        let mut reply = Task::from_request(TaskRequest {
            prompt: "다른 워커로 이어가줘".into(),
            created_by: "admin@org".into(),
            server_hint: Some("worker-ec1".into()),
            ..Default::default()
        });
        reply.inherit_from_parent(&parent);

        assert_eq!(reply.server_hint, Some("worker-ec1".into()));
        // cwd/model은 명시하지 않았으므로 부모에서 상속.
        assert_eq!(reply.cwd, parent.cwd);
        assert_eq!(reply.model, parent.model);
    }

    #[test]
    fn failure_kind_snake_case() {
        let json = serde_json::to_string(&FailureKind::CircuitOpen).unwrap();
        assert_eq!(json, "\"circuit_open\"");
    }

    #[test]
    fn failure_kind_credential_missing_snake_case() {
        let json = serde_json::to_string(&FailureKind::CredentialMissing).unwrap();
        assert_eq!(json, "\"credential_missing\"");
    }

    #[test]
    fn failure_kind_as_str_matches_serde_snake_case_for_every_variant() {
        // as_str()이 직렬화 표현과 갈라지면 metric label과 API 응답의 kind
        // 문자열이 서로 달라진다 — ALL을 순회해 모든 variant를 강제한다.
        for kind in FailureKind::ALL {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
        }
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
