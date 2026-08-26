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
    /// 클라이언트가 부여한 제출 멱등성 키 (로드맵 #62 2단계).
    ///
    /// `None`이면 멱등성 검사를 하지 않는다 — 매 제출이 새 Task다. `Some`이면
    /// `(created_by, idempotency_key)`가 저장소에서 유일하며, 같은 키의 재요청은
    /// 페이로드 해시가 같을 때 기존 Task를 그대로 돌려주고 다를 때 거절된다.
    /// 자세한 계약은 [`Self::payload_hash`] 참고.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// [`TaskRequest::payload_hash`]가 쓰는 인코딩 버전.
///
/// 해시에 포함되는 필드 집합이나 인코딩이 바뀌면 이 값을 올린다 — 그러면 이전
/// 버전으로 저장된 해시와 절대 같아질 수 없으므로, "포맷이 바뀌었는데 우연히
/// 같은 해시가 나와 다른 요청을 중복으로 판정"하는 경로가 사라진다. 대신 배포
/// 경계에서 같은 키의 재요청이 409로 거절될 수 있다(의도된 안전한 실패).
const PAYLOAD_HASH_VERSION: &str = "fleet.task.payload.v1";

impl TaskRequest {
    /// 제출 페이로드의 안정된 SHA-256 해시(hex).
    ///
    /// ## 왜 serde 직렬화를 쓰지 않는가
    ///
    /// `serde_json::to_vec(self)`는 구조체 필드 순서에 의존한다. 누군가 필드를
    /// 재배치하거나 `skip_serializing_if`를 바꾸면 **컴파일도 테스트도 통과한 채로**
    /// 기존 Task의 해시가 전부 달라진다 — 그 순간 모든 재제출이 409가 된다.
    /// 그래서 필드를 하나씩 명시적으로 먹인다: 해시에 무엇이 들어가는지가 코드에
    /// 드러나고, 필드를 추가하면 이 함수를 고칠 수밖에 없다.
    ///
    /// ## 왜 길이를 앞에 붙이는가
    ///
    /// 구분자만 쓰면 `prompt="ab", cwd=None`과 `prompt="a", cwd=Some("b")`가 같은
    /// 바이트열이 될 수 있다. 각 조각 앞에 길이(u64 LE)를 넣으면 경계가 모호해지지
    /// 않는다.
    ///
    /// ## 왜 목록을 정렬하는가
    ///
    /// `required_labels`/`dependency_ids`/`skills_required`는 모두 **집합**으로
    /// 소비된다 — 셀렉터는 "요구 라벨을 전부 가진 워커"를, DAG는 "선행 작업이 전부
    /// 완료"를 볼 뿐 순서를 보지 않는다. 정렬하지 않으면 `HashSet`을 순회해 요청을
    /// 만드는 클라이언트가 같은 논리적 재제출에서 다른 해시를 만들어내고, 중복
    /// Task가 조용히 생긴다.
    ///
    /// ## 무엇이 빠지는가
    ///
    /// `created_by`는 유일성 키의 다른 절반이므로 해시에 넣지 않는다(넣으면 같은
    /// 정보를 두 번 건다). `resolved_model`/`token_budget`은 라우터가 채우는
    /// 값이라 클라이언트 의도가 아니다 — 이 함수가 `Task`가 아니라 `TaskRequest`에
    /// 사는 이유이기도 하다. `Dispatcher::submit`의 라우팅 블록이 뒤에서 무엇을
    /// 채우든 해시에 닿을 수 없다.
    pub fn payload_hash(&self) -> String {
        use sha2::{Digest, Sha256};

        // 길이 접두사를 붙여 조각 경계를 명확히 한다. 클로저 대신 함수를 쓰는
        // 이유는 `hasher`를 마지막에 `finalize`로 소비해야 하기 때문 — 클로저가
        // 가변 차용을 붙잡고 있으면 그게 불가능하다.
        fn feed(hasher: &mut Sha256, bytes: &[u8]) {
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        }

        let mut hasher = Sha256::new();
        feed(&mut hasher, PAYLOAD_HASH_VERSION.as_bytes());
        feed(
            &mut hasher,
            self.project_id
                .map(|id| id.to_string())
                .unwrap_or_default()
                .as_bytes(),
        );
        feed(&mut hasher, self.prompt.as_bytes());
        feed(
            &mut hasher,
            self.cwd.as_deref().unwrap_or_default().as_bytes(),
        );
        feed(
            &mut hasher,
            self.model.as_deref().unwrap_or_default().as_bytes(),
        );
        feed(
            &mut hasher,
            self.server_hint.as_deref().unwrap_or_default().as_bytes(),
        );
        feed(
            &mut hasher,
            self.max_turns
                .map(|v| v.to_string())
                .unwrap_or_default()
                .as_bytes(),
        );
        feed(
            &mut hasher,
            self.timeout_secs
                .map(|v| v.to_string())
                .unwrap_or_default()
                .as_bytes(),
        );
        feed(
            &mut hasher,
            match self.priority {
                TaskPriority::Low => "low",
                TaskPriority::Normal => "normal",
                TaskPriority::High => "high",
            }
            .as_bytes(),
        );
        feed(
            &mut hasher,
            self.parent_task_id
                .map(|id| id.to_string())
                .unwrap_or_default()
                .as_bytes(),
        );
        feed(
            &mut hasher,
            self.routing_profile
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );

        let mut labels = self.required_labels.clone();
        labels.sort_unstable();
        labels.dedup();
        feed(&mut hasher, &(labels.len() as u64).to_le_bytes());
        for label in &labels {
            feed(&mut hasher, label.as_bytes());
        }

        let mut deps: Vec<String> = self
            .dependency_ids
            .iter()
            .map(|id| id.to_string())
            .collect();
        deps.sort_unstable();
        deps.dedup();
        feed(&mut hasher, &(deps.len() as u64).to_le_bytes());
        for dep in &deps {
            feed(&mut hasher, dep.as_bytes());
        }

        let mut skills = self.skills_required.clone();
        skills.sort_unstable();
        skills.dedup();
        feed(&mut hasher, &(skills.len() as u64).to_le_bytes());
        for skill in &skills {
            feed(&mut hasher, skill.as_bytes());
        }

        hex::encode(hasher.finalize())
    }
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
    /// 선택 Project 경계. `TaskRequest::project_id`에서 그대로 옮겨지거나
    /// `inherit_from_parent`로 이어받는다 — `None`이면 일반 풀 Task.
    /// (이전엔 "project 그룹화 도입 전까지 항상 None"이라 적혀 있었지만,
    /// #48 이후 실제 호출부(예: `submit_task_api`)가 채우므로 더 이상
    /// 사실이 아니었다. `Some`이면 이 Task는 `#58`의 Project 경계 불변식
    /// 대상이다 — 예: `link_issue_task`는 Issue와 다른 Project의 Task를
    /// 거부한다.)
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
    /// 제출 시 클라이언트가 부여한 멱등성 키 (로드맵 #62 2단계).
    /// `(created_by, idempotency_key)`가 저장소에서 유일하다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// 제출 시점 페이로드의 [`TaskRequest::payload_hash`] 값.
    ///
    /// `idempotency_key`와 항상 함께 채워지고 함께 비어 있다 — 키 없이 해시만
    /// 남는 행은 비교할 상대가 없어 아무 의미가 없다. 같은 키의 재요청이
    /// 도착했을 때 "같은 요청의 재시도"인지 "키를 재사용한 다른 요청"인지
    /// 가르는 값이다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_payload_hash: Option<String>,
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
        // 해시는 키가 있을 때만 계산해서 붙인다. 키 없는 제출에 해시만 채워
        // 두면 아무도 읽지 않는 컬럼이 생긴다 — "채울 방법이 없는 것은 미리
        // 만들지 않는다"에 어긋나고, 나중에 이 컬럼이 유일성 판정에 쓰인다고
        // 오해할 여지도 남는다.
        let idempotency_payload_hash = req.idempotency_key.as_ref().map(|_| req.payload_hash());
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
            idempotency_key: req.idempotency_key,
            idempotency_payload_hash,
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
        // 로드맵 #48 2단계 — 이어가기는 같은 작업 흐름의 연속이므로 Project
        // 경계도 물려받는다. 그러지 않으면 한 thread가 절반은 Project 안,
        // 절반은 일반 풀에 걸쳐 Project 경계의 의미가 사라진다.
        //
        // **호출부 주의**: 상속된 `project_id`도 명시 입력과 똑같이 검증
        // 대상이다 — 부모가 속한 Project가 그 사이 `Draining`/`Archived`가
        // 됐을 수 있고, 그 경우 이어가기는 거절돼야 한다(닫힌 Project는 새
        // Task를 받지 않는다). 이 메서드는 값을 채우기만 하고 검증하지
        // 않는다.
        if self.project_id.is_none() {
            self.project_id = parent.project_id;
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
/// - `Pending` → `Dispatched` | `Cancelled` | `Failed`
/// - `Dispatched` → `Completed` | `Failed` | `Cancelled`
/// - `Completed` / `Failed` / `Cancelled` → (종료, 전이 불가)
///
/// `Pending` → `Failed`은 워커를 못 잡았거나 CircuitBreaker가 열린 채로
/// dispatch가 끝난 경우다(`Dispatcher::mark_failed`, dispatcher.rs의
/// `NoWorker`/`CircuitOpen` 경로). 이 간선은 실제로 존재했지만 이 표에는
/// 빠져 있었다 — 표를 강제하는 코드가 없어 드리프트가 드러나지 않았다.
/// 지금은 [`TaskPhase::transition_allowed`]가 정본이며 이 주석은 그 사본이다.
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

impl TaskStatus {
    /// 이 상태의 phase 태그.
    ///
    /// DB의 `tasks.status_phase` 생성 칼럼(`status->>'phase'`,
    /// 001_init.sql)이 담고 있는 값과 정확히 같다.
    pub fn phase(&self) -> TaskPhase {
        match self {
            TaskStatus::Pending => TaskPhase::Pending,
            TaskStatus::Dispatched { .. } => TaskPhase::Dispatched,
            TaskStatus::Completed(_) => TaskPhase::Completed,
            TaskStatus::Failed(_) => TaskPhase::Failed,
            TaskStatus::Cancelled { .. } => TaskPhase::Cancelled,
        }
    }
}

/// [`TaskStatus`]에서 페이로드를 걷어낸 상태 태그.
///
/// compare-and-set의 "기대 상태" 집합을 표현하려고 존재한다. `TaskStatus`
/// 자체는 페이로드(worker_id, 실행 결과, 취소 사유)를 들고 있어서 "무엇이든
/// Dispatched이기만 하면 된다"를 표현할 수 없고, [`TaskStatusFilter`]는 조회
/// 편의를 위한 합성 변형(`Terminal`/`Active`)을 갖고 있어 상태 기계의
/// 정점(vertex) 집합이 아니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Pending,
    Dispatched,
    Completed,
    Failed,
    Cancelled,
}

impl TaskPhase {
    /// DB `status_phase` 칼럼에 실제로 저장되는 문자열.
    ///
    /// `TaskStatus`의 `#[serde(tag = "phase", rename_all = "snake_case")]`가
    /// 만들어내는 값과 반드시 일치해야 한다 — CAS의 SQL 조건절이 이 문자열로
    /// 비교하기 때문이다. 이 계약은 `phase_str_matches_serialized_status`가
    /// 지킨다(주석만으로 두었다가 어긋난 전례가 있다).
    pub fn as_str(self) -> &'static str {
        match self {
            TaskPhase::Pending => "pending",
            TaskPhase::Dispatched => "dispatched",
            TaskPhase::Completed => "completed",
            TaskPhase::Failed => "failed",
            TaskPhase::Cancelled => "cancelled",
        }
    }

    /// 종료 상태(나가는 간선이 없는 상태)인지 여부.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskPhase::Completed | TaskPhase::Failed | TaskPhase::Cancelled
        )
    }

    /// 상태 기계의 허용 간선 판정 (순수 함수 — 테스트가 전이표 전체를 훑을
    /// 수 있게 공개한다).
    ///
    /// `Pending` → `Failed`은 워커를 배정하지 못했거나 CircuitBreaker가 열린
    /// 채로 dispatch가 끝난 경로다. 실제로 늘 일어나던 전이인데 [`TaskStatus`]
    /// 주석의 표에는 빠져 있었다.
    pub fn transition_allowed(from: TaskPhase, to: TaskPhase) -> bool {
        use TaskPhase::*;
        matches!(
            (from, to),
            (Pending, Dispatched)
                | (Pending, Failed)
                | (Pending, Cancelled)
                | (Dispatched, Completed)
                | (Dispatched, Failed)
                | (Dispatched, Cancelled)
        )
    }

    /// `to`로 전이할 수 있는 선행 상태 전체.
    ///
    /// compare-and-set 기대값의 **기본값**이다. 호출자가 더 좁은 집합을 아는
    /// 경우에는 이 값 대신 그 집합을 넘겨야 한다. 예를 들어 reconciler는
    /// 이미 dispatch된 작업만 orphan으로 실패 처리해야 하므로 `[Dispatched]`를
    /// 넘긴다 — 넓은 기본값 `[Pending, Dispatched]`을 그대로 쓰면 방금
    /// `Pending` → `Dispatched`로 넘어간 작업을 orphan으로 오인해 죽이는
    /// 경합이 그대로 남는다.
    pub fn allowed_predecessors(to: TaskPhase) -> &'static [TaskPhase] {
        use TaskPhase::*;
        match to {
            Pending => &[],
            Dispatched => &[Pending],
            Completed => &[Dispatched],
            Failed => &[Pending, Dispatched],
            Cancelled => &[Pending, Dispatched],
        }
    }
}

/// compare-and-set 상태 전이의 결과.
///
/// `Rejected`는 실패가 아니라 **정상적인 관측 결과**다 — 다른 writer가 먼저
/// 상태를 옮겼다는 뜻이며, 호출자는 대개 자기 쓰기를 포기하는 것이 옳다.
/// 그래서 `Result`의 `Err`이 아니라 `Ok`의 값으로 표현한다. `Err`은 DB 장애나
/// 역직렬화 실패처럼 관측 자체가 불가능했던 경우로 남긴다.
///
/// `current`는 **거절 사유를 설명하기 위한 값**이며, 관측 시점 이후에도
/// 그 상태가 유지된다는 보장은 없다 — Postgres 구현은 UPDATE가 0행을 돌려준
/// 뒤 별도 SELECT로 읽으므로 그 사이에 또 다른 writer가 상태를 바꿀 수 있다.
/// 로깅·에러 메시지용으로만 쓰고, 제어 흐름의 근거로 삼지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionOutcome {
    /// 기대한 선행 상태와 일치해 새 상태가 기록됐다.
    Applied,
    /// 기대한 선행 상태와 달라 아무것도 쓰지 않았다.
    Rejected { current: TaskPhase },
    /// 호출자가 건 control-plane epoch 술어가 더 이상 성립하지 않아 아무것도
    /// 쓰지 않았다 (로드맵 #62 3단계).
    ///
    /// `Rejected`와 나누는 이유는 **후속 동작이 다르기** 때문이다. `Rejected`는
    /// "다른 writer가 이 Task를 먼저 옮겼다"이므로 이 Task 하나를 포기하면
    /// 되지만, `Fenced`는 "이 인스턴스가 더 이상 제어 기관이 아니다"이므로
    /// 같은 이유로 **이후의 모든 쓰기도 실패한다**. 둘을 뭉개면 fenced
    /// 인스턴스가 Task마다 정상 경합인 줄 알고 재시도를 반복하게 된다.
    ///
    /// 어느 epoch가 현재값인지는 담지 않는다 — 그 값을 알려면 별도 조회가
    /// 필요하고, 읽는 순간 또 바뀔 수 있어 `Rejected { current }`와 같은
    /// 이유로 제어 흐름의 근거가 되지 못한다. 호출자가 아는 것은 "내가 건
    /// 술어가 깨졌다"뿐이고, 그것으로 충분하다.
    Fenced,
}

impl TransitionOutcome {
    /// 전이가 실제로 적용됐는가.
    ///
    /// 이벤트 발행처럼 "상태가 바뀌었다"를 전제로 하는 후속 동작을 이 값으로
    /// 가드한다. 거절된 전이에 이벤트를 붙이면 이벤트 로그가 일어나지 않은
    /// 일을 주장하게 된다.
    pub fn applied(self) -> bool {
        matches!(self, TransitionOutcome::Applied)
    }

    /// control-plane epoch 술어가 깨져 거절됐는가.
    ///
    /// 호출자가 "이 Task를 포기"와 "제어권을 잃었으니 루프 자체를 중단"을
    /// 갈라야 할 때 쓴다.
    pub fn fenced(self) -> bool {
        matches!(self, TransitionOutcome::Fenced)
    }
}

/// 멱등성 키가 붙은 Task 삽입의 결과 (로드맵 #62 2단계).
///
/// `TransitionOutcome`과 같은 이유로 `Duplicate`/`Conflict`를 `Err`이 아니라
/// `Ok`의 값으로 표현한다 — 둘 다 저장소가 정상적으로 관측한 결과이지 장애가
/// 아니다. 다만 호출자가 갈라야 하는 후속 동작이 셋 다 다르다:
/// `Inserted`만 dispatch와 `TaskCreated` 이벤트로 이어지고, `Duplicate`는
/// 아무것도 하지 않은 채 기존 Task를 돌려주며, `Conflict`는 클라이언트에게
/// 409로 알린다.
///
/// `PartialEq`를 파생하지 않는다 — `Task`가 `PartialEq`가 아니고, 이 enum 하나를
/// 위해 Task 전체에 구조적 동등성을 부여하면 "두 Task가 같다"가 무엇을 뜻하는지에
/// 대한 답을 코드 전체에 강요하게 된다. 비교가 필요한 자리에서는 `matches!`나
/// 아래 접근자를 쓴다.
#[derive(Debug, Clone)]
pub enum IdempotentInsert {
    /// 새 Task가 저장됐다. 멱등성 키가 없는 제출도 항상 이 결과다.
    Inserted,
    /// 같은 `(created_by, idempotency_key)`에 **같은 페이로드 해시**를 가진
    /// Task가 이미 있다 — 같은 요청의 재시도다. 아무것도 쓰지 않았고, 담긴
    /// Task는 기존 행이다.
    ///
    /// 이 Task의 상태는 `Pending`이 아닐 수 있다. 최초 제출이 이미 완료됐거나
    /// 실패한 뒤 클라이언트가 재시도하는 것이 정확히 이 게이트가 막으려는
    /// 시나리오(timeout 후 재호출)이기 때문이다. 응답을 만드는 쪽은 "방금
    /// 만들어진 Pending Task"를 가정하면 안 된다.
    ///
    /// `Box`로 감싼 이유는 `Task`가 큰 구조체라 그대로 넣으면 `Inserted`
    /// 하나를 돌려줄 때도 enum 전체 크기를 지불하기 때문이다
    /// (`clippy::large_enum_variant`).
    Duplicate(Box<Task>),
    /// 같은 키에 **다른 페이로드 해시**를 가진 Task가 이미 있다 — 키가
    /// 재사용됐다. 아무것도 쓰지 않았다.
    ///
    /// 기존 Task 전체가 아니라 id만 돌려준다: 키를 재사용한 클라이언트가 그
    /// Task의 소유자라는 보장이 없으므로(같은 `created_by` 버킷을 여러 호출자가
    /// 공유할 수 있다 — 마이그레이션 024 주석 참고), 프롬프트나 결과를 그대로
    /// 흘리면 안 된다.
    Conflict { existing_task_id: TaskId },
}

impl IdempotentInsert {
    /// 이 삽입으로 실제 새 Task가 저장됐는가.
    ///
    /// `TransitionOutcome::applied`와 같은 역할이다 — dispatch나 `TaskCreated`
    /// 이벤트처럼 "새 작업이 생겼다"를 전제로 하는 후속 동작을 이 값으로 가드한다.
    pub fn inserted(&self) -> bool {
        matches!(self, IdempotentInsert::Inserted)
    }
}

/// Task 영구 삭제 시도의 결과 (로드맵 #96).
///
/// `TransitionOutcome`과 같은 이유로 거절을 `Err`이 아니라 `Ok`의 값으로
/// 표현한다 — "terminal이 아니다"와 "Pending 의존자가 있다"는 둘 다 저장소가
/// 정상적으로 판정한 결과이지 장애가 아니며, 호출자(핸들러)가 이 값을 보고
/// 서로 다른 HTTP 상태 코드로 응답을 나눠야 한다(계약: `docs/contracts/dashboard-api.md`
/// "계획된 표면" 절). `Err`은 행이 아예 없거나(`StoreError::NotFound`) DB
/// 접근 자체가 실패한 경우로 남긴다.
///
/// `NotTerminal`의 원자성은 `TransitionOutcome`과 동일하다 — `DELETE ... WHERE
/// id = $1 AND status_phase = ANY($2)`처럼 삭제 대상 행 자체를 조건절에 걸므로
/// TOCTOU가 없다. 반면 `BlockedByDependents`는 **다른 행들**을 대상으로 한
/// 별도 조회이므로 같은 보장이 없다 — 이 조회와 뒤따르는 `DELETE` 사이에 새
/// `Pending` 의존자가 삽입되면 그 창을 통과한다. `SERIALIZABLE` 격리나 아직
/// 존재하지 않는 행에 대한 predicate lock 없이는 닫을 수 없는 창이라 지금은
/// 닫지 않는다 — 제출과 삭제 모두 사람이 직접 트리거하는 저빈도 경로이고,
/// 결과가 나타나도 "영원히 Pending인 Task"라는 관측 가능하고 되돌릴 수 있는
/// 상태이지 데이터 손상이 아니다. `docs/architecture/tasks/management.md`
/// "삭제 계약"에 이 한계를 명시적으로 남긴다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskDeleteOutcome {
    /// 삭제됐다.
    Deleted,
    /// 대상 Task가 terminal(`is_terminal()`)이 아니라 삭제를 거절했다.
    NotTerminal { current: TaskPhase },
    /// 이 Task를 `dependency_ids`에 담은 `Pending` Task가 하나 이상 있어
    /// 거절했다 — 그 Task들이 먼저 정리되거나 자기 자신이 삭제돼야 한다.
    BlockedByDependents { dependent_ids: Vec<TaskId> },
}

impl TaskDeleteOutcome {
    /// 이 시도로 실제 행이 삭제됐는가.
    pub fn deleted(&self) -> bool {
        matches!(self, TaskDeleteOutcome::Deleted)
    }
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

    // 로드맵 #48 2단계 — 이어가기는 Project 경계도 물려받는다.
    #[test]
    fn inherit_from_parent_adopts_project_id() {
        let project_id = ProjectId::new();
        let parent = Task::from_request(TaskRequest {
            prompt: "parent".into(),
            created_by: "admin@org".into(),
            project_id: Some(project_id),
            ..Default::default()
        });

        let mut reply = Task::from_request(TaskRequest {
            prompt: "이어서".into(),
            created_by: "admin@org".into(),
            ..Default::default()
        });
        reply.inherit_from_parent(&parent);

        assert_eq!(reply.project_id, Some(project_id));
    }

    #[test]
    fn inherit_from_parent_does_not_override_explicit_project_id() {
        let parent_project = ProjectId::new();
        let explicit_project = ProjectId::new();
        let parent = Task::from_request(TaskRequest {
            prompt: "parent".into(),
            created_by: "admin@org".into(),
            project_id: Some(parent_project),
            ..Default::default()
        });

        let mut reply = Task::from_request(TaskRequest {
            prompt: "다른 project로".into(),
            created_by: "admin@org".into(),
            project_id: Some(explicit_project),
            ..Default::default()
        });
        reply.inherit_from_parent(&parent);

        assert_eq!(
            reply.project_id,
            Some(explicit_project),
            "an explicitly supplied project_id must win over the parent's"
        );
    }

    #[test]
    fn inherit_from_parent_leaves_project_id_none_when_parent_has_none() {
        let parent = Task::from_request(TaskRequest {
            prompt: "일반 풀 parent".into(),
            created_by: "admin@org".into(),
            ..Default::default()
        });
        let mut reply = Task::from_request(TaskRequest {
            prompt: "이어서".into(),
            created_by: "admin@org".into(),
            ..Default::default()
        });
        reply.inherit_from_parent(&parent);

        assert_eq!(reply.project_id, None);
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

    /// 상태 기계의 정점 전체. 새 변형이 생기면 이 배열도 같이 늘려야
    /// 아래 전수 테스트들이 계속 전수로 남는다.
    const ALL_PHASES: [TaskPhase; 5] = [
        TaskPhase::Pending,
        TaskPhase::Dispatched,
        TaskPhase::Completed,
        TaskPhase::Failed,
        TaskPhase::Cancelled,
    ];

    fn sample_status(phase: TaskPhase) -> TaskStatus {
        let now = Utc::now();
        match phase {
            TaskPhase::Pending => TaskStatus::Pending,
            TaskPhase::Dispatched => TaskStatus::Dispatched {
                worker_id: WorkerId::nil(),
                started_at: now,
            },
            TaskPhase::Completed => TaskStatus::Completed(TaskResult {
                output: "ok".into(),
                exit_code: 0,
                duration_secs: 1.0,
                token_usage: None,
                worker_id: WorkerId::nil(),
                finished_at: now,
            }),
            TaskPhase::Failed => TaskStatus::Failed(TaskFailure {
                error: "boom".into(),
                kind: FailureKind::WorkerError,
                worker_id: None,
                attempts: 1,
            }),
            TaskPhase::Cancelled => TaskStatus::Cancelled {
                reason: "user".into(),
                cancelled_at: now,
            },
        }
    }

    /// [`TaskPhase::as_str`]이 DB `status_phase`에 실제로 들어가는 문자열과
    /// 같은지 전수 확인한다.
    ///
    /// CAS는 `WHERE status_phase = ANY($3)`로 비교하므로 이 둘이 어긋나면
    /// 조건절이 조용히 0행을 매칭하고, 모든 전이가 "거부됨"으로 보인다.
    /// 같은 종류의 드리프트(주석은 맞다고 하는데 실제 직렬화는 다른)가
    /// PgStore의 worker_id 필터에서 이미 한 번 일어났다.
    #[test]
    fn phase_str_matches_serialized_status() {
        for phase in ALL_PHASES {
            let json = serde_json::to_value(sample_status(phase)).unwrap();
            let serialized = json
                .get("phase")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("{phase:?} 직렬화에 phase 태그가 없다: {json}"));
            assert_eq!(
                phase.as_str(),
                serialized,
                "{phase:?}: as_str()와 직렬화된 phase 태그가 다르다"
            );
        }
    }

    /// `TaskStatus::phase()`가 각 변형을 올바른 태그로 보낸다.
    #[test]
    fn status_maps_to_its_phase() {
        for phase in ALL_PHASES {
            assert_eq!(sample_status(phase).phase(), phase);
        }
    }

    /// 허용 간선이 [`TaskStatus`] 문서의 표와 정확히 일치한다.
    #[test]
    fn allowed_transitions_match_the_contract_diagram() {
        use TaskPhase::*;
        let expected = [
            (Pending, Dispatched),
            (Pending, Failed),
            (Pending, Cancelled),
            (Dispatched, Completed),
            (Dispatched, Failed),
            (Dispatched, Cancelled),
        ];
        for from in ALL_PHASES {
            for to in ALL_PHASES {
                let allowed = TaskPhase::transition_allowed(from, to);
                let want = expected.contains(&(from, to));
                assert_eq!(allowed, want, "전이 {from:?} → {to:?} 판정이 표와 다르다");
            }
        }
    }

    /// 종료 상태에서 나가는 간선은 하나도 없다.
    #[test]
    fn terminal_phases_have_no_outgoing_transitions() {
        for from in ALL_PHASES.into_iter().filter(|p| p.is_terminal()) {
            for to in ALL_PHASES {
                assert!(
                    !TaskPhase::transition_allowed(from, to),
                    "종료 상태 {from:?}에서 {to:?}로 나가는 간선이 있다"
                );
            }
        }
    }

    /// `allowed_predecessors`는 `transition_allowed`를 뒤집은 것과 같아야
    /// 한다. 두 표현이 갈라지면 CAS가 상태 기계와 다른 규칙을 강제하게 된다.
    #[test]
    fn allowed_predecessors_agree_with_transition_table() {
        for to in ALL_PHASES {
            let derived: Vec<TaskPhase> = ALL_PHASES
                .into_iter()
                .filter(|&from| TaskPhase::transition_allowed(from, to))
                .collect();
            assert_eq!(
                TaskPhase::allowed_predecessors(to),
                derived.as_slice(),
                "{to:?}의 선행 상태 집합이 전이표와 다르다"
            );
        }
    }

    /// `Pending`은 초기 상태이므로 선행 상태가 없다 — CAS로 진입할 수 없다.
    #[test]
    fn pending_has_no_predecessors() {
        assert!(TaskPhase::allowed_predecessors(TaskPhase::Pending).is_empty());
    }

    // ── payload hash (로드맵 #62 stage 2, 게이트 3) ──────────────────────

    fn hashed(f: impl FnOnce(&mut TaskRequest)) -> String {
        let mut req = TaskRequest {
            prompt: "build".into(),
            created_by: "alice".into(),
            ..Default::default()
        };
        f(&mut req);
        req.payload_hash()
    }

    #[test]
    fn payload_hash_is_stable_for_identical_requests() {
        assert_eq!(hashed(|_| {}), hashed(|_| {}));
    }

    /// 두 필드가 붙어 있으면 `prompt="ab", cwd=None`과 `prompt="a",
    /// cwd=Some("b")`가 같은 바이트열이 된다. 길이 접두사가 그걸 막는다.
    #[test]
    fn payload_hash_separates_adjacent_fields() {
        let merged = hashed(|r| r.prompt = "ab".into());
        let split = hashed(|r| {
            r.prompt = "a".into();
            r.cwd = Some("b".into());
        });
        assert_ne!(merged, split, "필드 경계가 해시에 반영돼야 한다");
    }

    /// 라벨·의존성·스킬은 집합으로 소비된다. HashSet을 순회해서 넘기는
    /// 클라이언트가 같은 제출을 매번 다르게 해시하면, 재전송이 중복으로 통과해
    /// 버린다 — 정렬이 그 구멍을 막는다.
    #[test]
    fn payload_hash_ignores_set_ordering() {
        let forward = hashed(|r| r.required_labels = vec!["gpu".into(), "linux".into()]);
        let reverse = hashed(|r| r.required_labels = vec!["linux".into(), "gpu".into()]);
        assert_eq!(forward, reverse);
    }

    #[test]
    fn payload_hash_reflects_set_contents() {
        let one = hashed(|r| r.required_labels = vec!["gpu".into()]);
        let two = hashed(|r| r.required_labels = vec!["gpu".into(), "linux".into()]);
        assert_ne!(one, two);
    }

    /// 라우터가 채우는 필드는 클라이언트 의도가 아니다. 여기에 반응하면 같은
    /// 재전송이 라우팅 결과에 따라 409로 튕긴다.
    #[test]
    fn payload_hash_ignores_router_filled_fields() {
        let bare = hashed(|_| {});
        let routed = hashed(|r| {
            r.resolved_model = Some("grok-4".into());
            r.token_budget = Some(100_000);
        });
        assert_eq!(bare, routed);
    }

    /// 해시는 키가 있을 때만 붙는다 — 아무도 읽지 않는 컬럼을 만들지 않는다.
    #[test]
    fn hash_is_attached_only_with_a_key() {
        let without = Task::from_request(TaskRequest {
            prompt: "build".into(),
            ..Default::default()
        });
        assert!(without.idempotency_key.is_none());
        assert!(without.idempotency_payload_hash.is_none());

        let with = Task::from_request(TaskRequest {
            prompt: "build".into(),
            idempotency_key: Some("k".into()),
            ..Default::default()
        });
        assert_eq!(with.idempotency_key.as_deref(), Some("k"));
        assert!(with.idempotency_payload_hash.is_some());
    }
}
