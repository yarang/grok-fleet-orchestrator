//! # fleet-store
//!
//! 영속 저장소 추상화 계층. `Store` trait은 백엔드에 독립적인 인터페이스를
//! 정의하고, `PgStore`가 PostgreSQL 구현을 제공합니다.
//!
//! ## 설계 원칙
//!
//! 1. **Trait 기반**: 상위 크레이트(`fleet-scheduler`, `fleet-mcp`)는 `Store`
//!    trait에만 의존하므로, 테스트 시 mock 구현으로 대체 가능.
//! 2. **도메인 타입 직접 사용**: DB 행 ↔ `fleet_core::Task`/`Worker` 변환은
//!    Store 내부에서 처리. 호출자는 SQL을 몰라도 됨.
//! 3. **JSONB 활용**: `TaskStatus`, `FleetEvent` 등 가변 구조는 JSONB로 저장.
//!    `status_phase` 생성 칼럼으로 빠른 필터링.
//! 4. **Append-only 이벤트 로그**: 모든 상태 변화는 `events` 테이블에 기록.
//!    LISTEN/NOTIFY로 다중 admin/대시보드에 실시간 전파.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

pub mod error;
pub mod listener;
#[cfg(feature = "test-support")]
pub mod mem;
pub mod postgres;
pub mod project_rules;
pub mod rbac;
pub mod task_pins;

pub use error::StoreError;
pub use listener::listen_events;
pub use postgres::{PgStore, PoolConfig};
pub use project_rules::{
    advance_project_archive, ensure_project_accepts_new_agents, ensure_project_accepts_new_tasks,
    task_project_matches_issue_project, ArchiveBlockers, ArchiveProgress, ProjectAdmissionError,
};
pub use rbac::{
    consume_bootstrap_and_create_admin, issue_admin_bootstrap_token, seed_builtin_roles,
    seed_permissions, seed_rbac_and_maybe_issue_bootstrap, BootstrapAdminError,
};
pub use task_pins::{apply_agent_pin, TaskPinError};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fleet_core::{
    Agent, AgentAck, AgentCommand, AgentDesiredStatus, AgentFilter, AgentId, AgentObservation,
    AgentStatus, AgentTemplate, AgentTemplateBody, AgentTemplateFilter, AgentTemplateId,
    AgentTemplateRevision, AgentTemplateRevisionId, AgentTemplateStatus, AuditEvent, AuditFilter,
    BootstrapToken, CloseReason, EventEntry, FleetEvent, Issue, IssueComment, IssueFilter, IssueId,
    IssueStatus, IssueTaskLink, LoginAttempt, Permission, PermissionId, PermissionKind, Project,
    ProjectFilter, ProjectId, ProjectStatus, Role, RoleId, Session, SessionId, Task,
    TaskDeleteOutcome, TaskFilter, TaskId, TaskOutput, TaskPhase, TaskStatus, TransitionOrigin,
    TransitionOutcome, User, UserId, Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
};
use uuid::Uuid;

/// Worker operational credential의 저장 형태. 원문 credential은 이 타입에 포함하지 않는다.
#[derive(Debug, Clone)]
pub struct WorkerOperationalCredential {
    pub worker_id: WorkerId,
    pub credential_digest: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub rotation_generation: i64,
}

/// Admin API bearer token의 저장 형태 (로드맵 #72). 원문 토큰은 이 타입에
/// 포함하지 않는다 — `token_digest`만 보관하며, 원문은 생성/rotate 응답에서만
/// 1회 노출된다.
#[derive(Debug, Clone)]
pub struct AdminApiToken {
    pub principal_id: String,
    pub token_digest: String,
    pub capabilities: Vec<PermissionKind>,
    pub created_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub rotation_generation: i64,
}

/// Control plane 권한 lease의 저장 형태 (로드맵 #63, 1단계).
///
/// Fleet는 유효한 dispatch lease를 가진 Orchestrator가 최대 하나여야 한다
/// ([Control Plane 권한과 장애 전환](../../../docs/architecture/control-plane-authority-and-failover.md)).
/// `epoch`는 획득마다 단조 증가하며, 이전 epoch에서 시작된 in-flight 요청이
/// 새 epoch 획득 이후 상태를 바꾸는 걸 막는 근거가 된다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlLease {
    pub cluster_id: String,
    pub active_instance_id: String,
    pub epoch: i64,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_renewed_at: DateTime<Utc>,
}

/// Task 상태 쓰기에 함께 거는 control-plane epoch 술어 (로드맵 #62 3단계).
///
/// [`ControlLease`]의 `epoch`는 획득마다 단조 증가한다. 그 값을 **읽어서 분기**
/// 하는 것과 **쓰기에 술어로 거는** 것은 다르다 — 전자는 관측과 쓰기 사이에
/// 창이 남고(관측 직후 fenced되면 그 쓰기는 그대로 DB에 도착한다), 후자는
/// 그 창을 저장소의 한 문장 안으로 밀어 넣는다.
///
/// `cluster_id`까지 담는 이유: epoch는 클러스터마다 독립적으로 증가하므로
/// 숫자만으로는 어느 lease의 epoch인지 결정되지 않는다. 하나의 DB가 여러
/// 클러스터의 lease를 담을 수 있다는 `control_plane_lease`의 PK 설계
/// (마이그레이션 021)를 그대로 따른다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFence {
    pub cluster_id: String,
    pub epoch: i64,
}

/// 슬롯 선점의 결과 (로드맵 `#67` 구현 게이트 ①-A-2).
///
/// `bool`이 아닌 이유는 실패가 **두 가지 다른 사실**이기 때문이다. 상한에
/// 걸린 것은 지금의 fleet 상태이고(409, 나중에 다시 하면 된다), 존재하지
/// 않는 대상을 지목한 것은 요청의 결함이다(404/400, 다시 해도 같다).
/// `bool`로 뭉개면 호출부가 그 둘을 구분할 방법이 없어 한쪽으로 오분류한다.
///
/// **`NoSuchAgent`가 `NoSuchWorker`보다 우선한다.** PgStore의 UPDATE는
/// Agent가 없으면 0행을 갱신하므로 Worker의 존재 여부에 닿지도 않으며,
/// MemStore도 그 순서를 흉내 내고 있다(`mem::MemStore::assign_agent_worker`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotClaim {
    /// 배정이 기록됐다.
    Claimed,
    /// Worker가 이미 `max_agent_processes`만큼 들고 있다. 재배정에서
    /// 자기 자신은 세지 않으므로, 이미 그 Worker에 있는 Agent를 같은
    /// Worker로 다시 배정하는 것은 이 값을 내지 않는다.
    CapReached,
    /// 그런 Agent가 없다.
    NoSuchAgent,
    /// 그런 Worker가 없다.
    NoSuchWorker,
}

/// 영속 저장소 trait. 모든 상태 조회/변경은 이 인터페이스를 경유합니다.
///
/// 구현체:
/// - [`PgStore`] — PostgreSQL (프로덕션)
/// - [`mem::MemStore`] — 완전 동작하는 인메모리 구현 (`test-support` 피처,
///   테스트 전용)
#[async_trait]
pub trait Store: Send + Sync {
    // ── Task ───────────────────────────────────────────────────────

    /// 작업을 저장소에 삽입. ID 충돌 시 에러.
    ///
    /// **멱등성 키를 존중하지 않는다.** 클라이언트가 `idempotency_key`를 붙였더라도
    /// 이 메서드는 그대로 삽입을 시도하며, 키가 이미 쓰였다면 유일 인덱스에
    /// 걸려 [`StoreError::Conflict`]가 난다. 제출 경로는
    /// [`Store::insert_task_idempotent`]를 쓴다.
    async fn insert_task(&self, task: &Task) -> Result<(), StoreError>;

    /// 제출 멱등성을 존중하며 작업을 삽입한다 (로드맵 #62 2단계).
    ///
    /// `task.idempotency_key`가 `None`이면 [`Store::insert_task`]와 같고 항상
    /// [`IdempotentInsert::Inserted`]다. `Some`이면 `(created_by, idempotency_key)`가
    /// 이미 존재하는지 보고, 존재하면 저장된 페이로드 해시를 비교해
    /// [`IdempotentInsert::Duplicate`](같은 요청의 재시도)와
    /// [`IdempotentInsert::Conflict`](키 재사용)를 가른다.
    ///
    /// ## 왜 호출부가 아니라 저장소에서 강제하는가
    ///
    /// 프로덕션의 `insert_task` 호출 경로는 세 갈래이고 하나의 깔때기를 지나지
    /// 않는다 — `Dispatcher::submit`, `fleet-cli`의 직접 삽입, 그리고 MCP
    /// `handle_dispatch_task`(이건 dispatcher를 경유한다). 호출부에서 "먼저
    /// 조회하고 없으면 삽입"을 하면 (a) 세 곳이 드리프트하고 (b) 조회와 삽입
    /// 사이에 다른 프로세스가 끼어드는 창이 그대로 남는다. 유일 인덱스가 그
    /// 창을 닫는 유일한 지점이므로, 판정도 같은 자리에서 한다.
    ///
    /// ## 유일성 스코프의 한계
    ///
    /// 설계 정본(`docs/architecture/tasks/execution-consistency.md`)은 "동일
    /// principal, 동일 key"라고 쓰지만, 오늘 코드에 principal이 없다. MCP 제출은
    /// 전부 `created_by = "mcp"` 한 버킷을 공유하므로, 이 스코프는 MCP 클라이언트
    /// 단위가 아니라 오케스트레이터 단위다. 마이그레이션 024 주석과 로드맵 #62
    /// 행에 같은 내용이 적혀 있다.
    async fn insert_task_idempotent(
        &self,
        task: &Task,
    ) -> Result<fleet_core::IdempotentInsert, StoreError>;

    /// 작업 ID로 조회. 없으면 `None`.
    async fn get_task(&self, id: TaskId) -> Result<Option<Task>, StoreError>;

    /// 작업 상태 업데이트. `status_phase` 생성 칼럼도 자동 갱신.
    ///
    /// **무조건 덮어쓴다.** 동시에 다른 writer가 같은 작업을 옮기고 있을 수
    /// 있는 경로에서는 [`Store::compare_and_set_task_status`]를 쓴다.
    async fn update_task_status(&self, id: TaskId, status: &TaskStatus) -> Result<(), StoreError>;

    /// 현재 상태가 `expected` 중 하나일 때만 `new`로 전이한다 (낙관적 잠금).
    ///
    /// 여러 writer가 같은 작업의 상태를 옮기는 경로 — 워커 이벤트, reconciler의
    /// 스윕, 사용자 취소 — 는 서로를 알지 못한 채 동시에 도착한다. 조건 없는
    /// `update_task_status`는 마지막에 도착한 쓰기가 이기므로, 이미 `Failed`로
    /// 확정된 작업에 늦은 `Completed`가 덮어써지는 일이 실제로 가능하다.
    /// 이 메서드는 `WHERE` 절에 현재 위상을 걸어 그 창을 닫는다.
    ///
    /// `expected`는 **호출 지점이 실제로 뒤따를 수 있는 상태만** 넘긴다.
    /// [`TaskPhase::allowed_predecessors`]의 넓은 기본값을 그대로 쓰면
    /// reconciler가 방금 `Pending` → `Dispatched`로 넘어간 작업을 orphan으로
    /// 오인해 죽이는 경합이 그대로 남는다 — 그 함수의 주석에 적힌 그대로다.
    ///
    /// 전이가 거절된 것은 에러가 아니라 관측 결과이므로
    /// [`TransitionOutcome::Rejected`]로 돌려준다. `Err`은 작업이 아예 없거나
    /// (`StoreError::NotFound`) DB 접근 자체가 실패한 경우다.
    ///
    /// `fence`를 주면 위상 조건에 **control-plane epoch 술어가 AND로 더해진다**
    /// (로드맵 #62 3단계). 그 술어가 깨져 있으면 아무것도 쓰지 않고
    /// [`TransitionOutcome::Fenced`]를 돌려준다. `None`은 "이 호출자에게는
    /// 걸 fence가 없다"는 뜻이며, 그 자체가 기록으로 남아야 하는 사실이다 —
    /// 아래 구현 주석과 각 호출 지점을 참고한다.
    ///
    /// **fence는 `expected`를 대체하지 않는다.** 둘은 서로 다른 경합을 막는다:
    /// `expected`는 같은 Task를 두고 경쟁하는 writer들을, `fence`는 제어권을
    /// 잃은 인스턴스 전체를 막는다.
    ///
    /// `origin`이 세 번째 경합을 막는다 (로드맵 #67 1단계, 불변식 ②).
    /// [`TransitionOrigin::WorkerOutcome`]이면 `fence`가 있을 때 dispatch 세대
    /// 술어가 하나 더 AND로 더해진다 — 작업을 디스패치한 세대
    /// (`tasks.dispatch_control_epoch`)와 `fence.epoch`가 같아야 한다.
    /// 이것이 막는 것은 **한 프로세스 안에서** 리스를 잃었다가 되찾는 창이다:
    /// epoch 5로 디스패치 → 리스를 다른 인스턴스에 뺏김(epoch 6, 그쪽이 작업을
    /// 재디스패치) → 다시 획득(epoch 7). 이때 epoch 5에 보낸 dispatch의 완료가
    /// 도착하면 위상은 `Dispatched`로 맞고 fence 술어도 epoch 7로 성립하므로,
    /// 이 술어가 없으면 **epoch 6이 만든 진행을 epoch 5의 결과가 덮어쓴다**.
    ///
    /// [`TransitionOrigin::ControlDecision`]에는 걸지 않는다. 그쪽은 현재
    /// 보유자가 지금 내리는 결정이고, 걸면 낡은 세대가 디스패치한 고아를
    /// 회수할 수 없게 된다 — 이유는 `TransitionOrigin`의 주석에 있다.
    ///
    /// `dispatch_control_epoch`가 NULL이면 술어는 통과시킨다. 026 마이그레이션이
    /// 규정한 대로 NULL은 "값을 못 구했다"가 아니라 **"제어 세대라는 개념이 없는
    /// 배포"**(HA 리스를 쓰지 않는 단일 인스턴스, 또는 026 이전 행)이므로,
    /// 물어볼 세대 자체가 없다. 이 값을 거절로 읽으면 HA를 나중에 켠 배포에서
    /// 전환 이전에 디스패치된 작업이 전부 종료 불가가 된다.
    ///
    /// 여전히 남는 것은 **같은 세대 안의 재디스패치**를 가르는 일이다 — 그건
    /// attempt/generation의 몫이고 이 저장소에는 없다. `#62` 4단계가 `tasks`에
    /// 컬럼 하나만 남긴 근거(재시도 없음 → Task당 시도 최대 하나)가 지금도
    /// 유효하므로, 그 창은 재시도 정책이 바뀌기 전에는 열리지 않는다.
    async fn compare_and_set_task_status(
        &self,
        id: TaskId,
        expected: &[TaskPhase],
        new: &TaskStatus,
        fence: Option<&ControlFence>,
        origin: TransitionOrigin,
    ) -> Result<TransitionOutcome, StoreError>;

    /// 필터 조건으로 작업 목록 조회 (생성일 역순).
    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, StoreError>;

    /// 워커별 `Dispatched` 작업 수 — 오케스트레이터 원장이 관측한 실제 부하 (로드맵 #67 3단계).
    ///
    /// 스케줄러의 용량 판단은 예전에 `Worker::active_tasks`(워커 자기보고)를 읽었다.
    /// 그 값은 하트비트로만 갱신되므로 (a) 최대 health interval(기본 15초)만큼 낡고,
    /// (b) 워커가 위조할 수 있으며, (c) 0을 신고하면 필터를 통과할 뿐 아니라
    /// least-loaded 정렬에서 **우대**받는다. 이 메서드는 그 판단을 오케스트레이터
    /// 자신이 기록한 `Dispatched` 행으로 옮긴다.
    ///
    /// 반환 맵에 **없는 워커는 0건**을 뜻한다 (희소 맵). 호출자는
    /// `counts.get(&id).copied().unwrap_or(0)`으로 읽는다.
    ///
    /// `#67` 2단계(worker incarnation)가 선행 조건이었다. 그 전에는 같은 `--name`으로
    /// 재시작한 워커가 `worker_id`를 재사용해서 이전 화신의 in-flight 작업이 영원히
    /// `Dispatched`로 남았고, 그러면 이 카운트가 영구히 과대계상됐다.
    async fn count_dispatched_tasks_by_worker(
        &self,
    ) -> Result<std::collections::HashMap<WorkerId, u32>, StoreError>;

    /// `tasks.retry_count`를 원자적으로 1 증가시키고 새 값을 반환 (로드맵 #38).
    /// dispatch 재시도 상한(`max_dispatch_retries`) 판단에 사용 — `submit()`
    /// 최초 시도 또는 `Reconciler`의 stale-Pending 재시도가 `WorkerUnavailable`/
    /// `CircuitOpen`으로 실패할 때마다 호출된다.
    async fn increment_task_retry_count(&self, id: TaskId) -> Result<u32, StoreError>;

    /// 작업 마이그레이션 이관용 Git 임시 브랜치명을 업데이트합니다.
    async fn update_task_checkpoint(
        &self,
        id: TaskId,
        checkpoint_branch: Option<&str>,
    ) -> Result<(), StoreError>;

    /// 스레드(연속 대화) 전체를 시간순(오름차순)으로 조회 — 대화를 읽는 순서.
    ///
    /// 기본 구현은 `list_tasks`를 넉넉한 limit으로 호출한 뒤 클라이언트 측에서
    /// `thread_id`로 필터링/정렬한다 — 목 스토어(테스트)는 재정의 없이도
    /// 정확하게 동작한다. [`PgStore`]는 인덱스(`idx_tasks_thread_id`)를 타는
    /// SQL로 재정의한다.
    async fn list_thread_tasks(&self, thread_id: TaskId) -> Result<Vec<Task>, StoreError> {
        let mut tasks = self
            .list_tasks(&TaskFilter {
                limit: 10_000,
                ..Default::default()
            })
            .await?;
        tasks.retain(|t| t.thread_id == thread_id);
        tasks.sort_by_key(|t| t.created_at);
        Ok(tasks)
    }

    /// terminal Task를 영구 삭제한다 (로드맵 #96).
    ///
    /// **기본 구현이 없다** — `list_thread_tasks`와 달리 삭제는 `list_tasks`로
    /// 유도할 수 없다(목록에서 없는 행을 만들어낼 수는 있어도, 실제로 행을
    /// 지우는 동작은 저장소마다 직접 구현해야 한다). 그래서 테스트 전용 mock
    /// 4종을 포함한 이 트레이트의 모든 구현체가 이 메서드에 명시적으로
    /// 답해야 한다 — 실제로 지원하지 않는 mock은 `unimplemented!()`로 그
    /// 사실을 드러낸다.
    ///
    /// 판정 순서(둘 다 [`TaskDeleteOutcome`]의 값이지 `Err`이 아니다 —
    /// `TransitionOutcome`과 같은 이유):
    /// 1. terminal이 아니면 [`TaskDeleteOutcome::NotTerminal`].
    /// 2. 이 Task를 `dependency_ids`에 담은 `Pending` Task가 있으면
    ///    [`TaskDeleteOutcome::BlockedByDependents`].
    /// 3. 그 외엔 삭제하고 [`TaskDeleteOutcome::Deleted`].
    ///
    /// `Err(StoreError::NotFound)`는 행 자체가 없을 때만 쓴다. 두 조건 모두
    /// 통과하지 못한 실패가 아니라 **관측 결과**이므로 앞의 두 판정과 섞지
    /// 않는다.
    ///
    /// 원자성 한계는 [`TaskDeleteOutcome::BlockedByDependents`] 문서 참고 —
    /// terminal 판정은 대상 행 자체를 조건절에 걸어 TOCTOU가 없지만, 의존자
    /// 판정은 다른 행을 대상으로 한 별도 조회라 같은 보장이 없다.
    async fn delete_task(&self, id: TaskId) -> Result<TaskDeleteOutcome, StoreError>;

    /// 스레드 단위 페이지의 `thread_id` 목록 (`#96`, `GET /api/task-threads`).
    ///
    /// `docs/ui-dashboard/ui-design.md` §3.3 "페이지네이션": 페이지의 단위는
    /// Task가 아니라 스레드다. 이 메서드는 그 "스레드 선정" 질의만 맡는다 —
    /// 값은 활동순(스레드 구성원 `created_at`의 최댓값 내림차순)으로 정렬된
    /// `thread_id` 페이지 하나다. "구성원 적재"는 호출부가 이 결과의 각
    /// `thread_id`에 대해 [`Store::list_thread_tasks`]를 따로 불러 채운다 —
    /// 설계 문서가 명시한 두 질의 구조를 트레이트 경계에도 그대로 옮긴다.
    ///
    /// 기본 구현은 [`Store::list_thread_tasks`]와 같은 방식으로
    /// [`Store::list_tasks`] 위에서 유도한다: 전체를 끌어와 Rust에서
    /// `thread_id`별 최신 활동을 계산하고 정렬·페이지네이션한다. 스레드
    /// 수가 커지면 비효율적이다 — [`PgStore`](crate::PgStore)는
    /// `GROUP BY thread_id ORDER BY MAX(created_at) DESC`로 재정의해
    /// `idx_tasks_thread_id (thread_id, created_at)`를 타는 집계 질의를
    /// 쓴다.
    async fn list_task_threads(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<TaskId>, StoreError> {
        let tasks = self
            .list_tasks(&TaskFilter {
                limit: 10_000,
                ..Default::default()
            })
            .await?;

        let mut last_activity: std::collections::HashMap<TaskId, chrono::DateTime<chrono::Utc>> =
            std::collections::HashMap::new();
        for task in &tasks {
            last_activity
                .entry(task.thread_id)
                .and_modify(|max| {
                    if task.created_at > *max {
                        *max = task.created_at;
                    }
                })
                .or_insert(task.created_at);
        }

        let mut threads: Vec<(TaskId, chrono::DateTime<chrono::Utc>)> =
            last_activity.into_iter().collect();
        threads.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));

        Ok(threads
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(thread_id, _)| thread_id)
            .collect())
    }

    // ── Worker ──────────────────────────────────────────────────────

    /// 워커를 upsert (id 기준). 같은 name의 기존 워커는 덮어씀.
    async fn upsert_worker(&self, worker: &Worker) -> Result<(), StoreError>;

    /// 워커 ID로 조회.
    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError>;

    /// 워커 이름으로 조회 (MCP `server_hint` 해석용).
    async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError>;

    /// 필터 조건으로 워커 목록 조회.
    async fn list_workers(&self, filter: &WorkerFilter) -> Result<Vec<Worker>, StoreError>;

    /// 워커의 incarnation 시작 시각을 **지금**으로 올린다 (migration 028).
    ///
    /// 재등록을 감지한 쪽만 호출한다. `upsert_worker`가 이 컬럼을 건드리지 않는
    /// 이유와 짝을 이룬다 — heartbeat도 upsert를 타므로 거기서 값이 움직이면
    /// 하트비트마다 진행 중인 작업이 전부 고아로 판정된다.
    ///
    /// 시각은 호출자의 시계가 아니라 **Store의 시계**로 찍는다. 이 값은
    /// `tasks.dispatched_at`(역시 Store가 `NOW()`로 찍는다)과 대소를 비교하므로,
    /// 두 값이 같은 시계에서 나와야 오케스트레이터가 여러 대인 배포에서도
    /// 호스트 간 시계 오차가 회수 판정에 들어오지 않는다.
    ///
    /// 대상 워커가 없으면 `None`.
    async fn bump_worker_incarnation(
        &self,
        id: WorkerId,
    ) -> Result<Option<DateTime<Utc>>, StoreError>;

    /// 워커 삭제 (등록 해제).
    async fn delete_worker(&self, id: WorkerId) -> Result<(), StoreError>;

    /// 하트비트 수신 시 워커 상태 갱신 (active_tasks, last_seen, agent_healthy).
    async fn update_worker_heartbeat(
        &self,
        id: WorkerId,
        heartbeat: &WorkerHeartbeat,
    ) -> Result<(), StoreError>;

    /// 워커의 CircuitBreaker 상태 강제 갱신.
    async fn update_worker_circuit_state(
        &self,
        _id: WorkerId,
        _state: fleet_core::worker::CircuitState,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    // ── Event log (append-only) ────────────────────────────────────

    /// 이벤트를 로그에 추가. 발급된 시퀀스 번호 반환.
    /// LISTEN/NOTIFY 트리거가 모든 리스너에게 통지.
    async fn append_event(&self, event: &FleetEvent) -> Result<u64, StoreError>;

    /// `after_seq` 이후의 이벤트를 최대 `limit`개 조회 (페이지네이션용).
    async fn list_events(&self, after_seq: u64, limit: u32) -> Result<Vec<EventEntry>, StoreError>;

    // ── Output buffer (스트리밍 stdout) ─────────────────────────────

    /// 작업 출력 청크를 append. 발급된 시퀀스 번호 반환.
    async fn append_output(&self, task_id: TaskId, chunk: &str) -> Result<u64, StoreError>;

    /// `after_seq` 이후의 출력 청크를 조회 (폴링 기반 스트리밍).
    async fn get_output(&self, task_id: TaskId, after_seq: u64) -> Result<TaskOutput, StoreError>;

    // ── Migration ──────────────────────────────────────────────────

    /// 보류 중인 마이그레이션을 모두 적용.
    async fn migrate(&self) -> Result<(), StoreError>;

    // ── Bootstrap tokens (Phase 8.3) ───────────────────────────────

    /// 부트스트랩 토큰을 저장. 동일 token이 이미 존재하면 에러.
    async fn create_bootstrap_token(&self, token: &BootstrapToken) -> Result<(), StoreError>;

    /// 부트스트랩 토큰을 atomic하게 소비.
    /// - 토큰이 존재하고 사용 가능 (use_count < max_uses, 만료 안 됨) 하면
    ///   use_count를 1 증가시키고 last_used_by/at을 갱신한 뒤 Ok 반환.
    /// - 존재하지 않거나 소진/만료된 경우 `StoreError::BootstrapTokenInvalid` 반환.
    ///
    /// 구현은 단일 UPDATE ... RETURNING 문으로 race condition을 방지해야 함.
    async fn consume_bootstrap_token(&self, token: &str, used_by: &str) -> Result<(), StoreError>;

    /// 모든 부트스트랩 토큰을 생성일 역순으로 조회.
    async fn list_bootstrap_tokens(&self) -> Result<Vec<BootstrapToken>, StoreError>;

    /// 부트스트랩 토큰 삭제 (revocation). 존재하지 않으면 false 반환.
    async fn revoke_bootstrap_token(&self, token: &str) -> Result<bool, StoreError>;

    // ── Worker operational credentials ─────────────────────────────

    async fn upsert_worker_operational_credential(
        &self,
        credential: &WorkerOperationalCredential,
    ) -> Result<(), StoreError> {
        let _ = credential;
        Err(StoreError::Unsupported("worker operational credentials"))
    }

    async fn find_active_worker_operational_credential(
        &self,
        credential_digest: &str,
    ) -> Result<Option<WorkerOperationalCredential>, StoreError> {
        let _ = credential_digest;
        Err(StoreError::Unsupported("worker operational credentials"))
    }

    /// worker의 operational credential을 즉시 회수한다 (로드맵 #60 6단계).
    ///
    /// `revoked_at`을 설정할 뿐 row는 삭제하지 않는다 — 회수 이력을 보존하고
    /// `find_active_worker_operational_credential`의 필터가 이후 인증을 거부한다.
    /// 대상 credential이 없거나 이미 회수된 경우 `false`를 반환한다.
    async fn revoke_worker_operational_credential(
        &self,
        worker_id: WorkerId,
    ) -> Result<bool, StoreError> {
        let _ = worker_id;
        Err(StoreError::Unsupported(
            "revoke_worker_operational_credential",
        ))
    }

    /// worker의 operational credential을 새 digest로 회전한다 (로드맵 #60 6단계).
    ///
    /// 기존 row를 새 digest로 in-place 갱신하며 `rotation_generation`을 1
    /// 증가시킨다 — PK가 `worker_id` 단일 row이므로 과거 세대 이력은 별도로
    /// 남기지 않는다. 이전 digest는 이 호출 즉시 무효화된다(자동 fallback
    /// 없음). 대상 worker에 credential이 없으면 `StoreError::NotFound`.
    async fn rotate_worker_operational_credential(
        &self,
        worker_id: WorkerId,
        new_credential_digest: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<WorkerOperationalCredential, StoreError> {
        let _ = (worker_id, new_credential_digest, expires_at);
        Err(StoreError::Unsupported(
            "rotate_worker_operational_credential",
        ))
    }

    /// bootstrap 토큰 소비, Worker 생성, operational credential 저장을 하나의 단위로
    /// 실행한다 (로드맵 #60, worker 등록 원자성).
    ///
    /// 셋 중 하나라도 실패하면 아무 것도 반영되지 않는다 — 토큰은 소비되지 않고
    /// Worker도 생성되지 않는다. `join_worker` 핸들러가 사용하는 authoritative
    /// entry point이며, 이름/digest 충돌은 `StoreError::Conflict`로 반환된다.
    ///
    /// 기본 구현은 순차 호출 fallback — 진짜 atomic을 지원하지 않는 Store(테스트
    /// mock 등)용이며, [`crate::postgres::PgStore`]/[`crate::mem::MemStore`]는
    /// 각자의 저장 구조에 맞춰 진짜 원자적으로 재정의한다.
    async fn enroll_worker(
        &self,
        bootstrap_token: &str,
        used_by: &str,
        worker: &fleet_core::Worker,
        credential: &WorkerOperationalCredential,
    ) -> Result<(), StoreError> {
        self.consume_bootstrap_token(bootstrap_token, used_by)
            .await?;
        self.upsert_worker(worker).await?;
        self.upsert_worker_operational_credential(credential).await
    }

    // ── Admin API tokens (로드맵 #72) ────────────────────────────────
    //
    // 기본 구현은 `Unsupported` — mock store(테스트용)는 재정의하지 않아도
    // 됨. `PgStore`/`mem::MemStore`만 실제 구현.

    /// 신규 principal의 admin API 토큰을 생성한다. 동일 `principal_id`가
    /// 이미 존재하면 `StoreError::Conflict`.
    async fn create_admin_token(&self, token: &AdminApiToken) -> Result<(), StoreError> {
        let _ = token;
        Err(StoreError::Unsupported("create_admin_token"))
    }

    /// digest로 활성(`revoked_at IS NULL`) admin 토큰을 조회한다.
    /// `auth_middleware`의 인증 경로에서 사용 — env `valid_tokens`와 함께
    /// 검사되는 두 번째 소스다.
    async fn find_active_admin_token_by_digest(
        &self,
        token_digest: &str,
    ) -> Result<Option<AdminApiToken>, StoreError> {
        let _ = token_digest;
        Err(StoreError::Unsupported("find_active_admin_token_by_digest"))
    }

    /// 모든 admin 토큰의 메타데이터를 생성일 역순으로 조회한다 (회수된 것
    /// 포함). `token_digest`가 포함되어 있으므로 API 응답으로 그대로
    /// 내보내면 안 된다 — 호출부(`GET /v1/admin/tokens` 핸들러)가 digest를
    /// 제외한 요약 타입으로 변환해야 한다.
    async fn list_admin_tokens(&self) -> Result<Vec<AdminApiToken>, StoreError> {
        Err(StoreError::Unsupported("list_admin_tokens"))
    }

    /// principal의 admin 토큰을 새 digest로 회전한다. 이전 digest는 이 호출
    /// 즉시 무효화되며(자동 fallback 없음) `rotation_generation`이 1 증가한다.
    /// 대상 principal에 토큰이 없으면 `StoreError::NotFound`.
    async fn rotate_admin_token(
        &self,
        principal_id: &str,
        new_token_digest: &str,
    ) -> Result<AdminApiToken, StoreError> {
        let _ = (principal_id, new_token_digest);
        Err(StoreError::Unsupported("rotate_admin_token"))
    }

    /// principal의 admin 토큰을 즉시 회수한다. `revoked_at`을 설정할 뿐 row는
    /// 삭제하지 않는다. 대상이 없거나 이미 회수된 경우 `false`.
    async fn revoke_admin_token(&self, principal_id: &str) -> Result<bool, StoreError> {
        let _ = principal_id;
        Err(StoreError::Unsupported("revoke_admin_token"))
    }

    // ── RBAC: Users (Phase 9.1) ───────────────────────────────────
    //
    // 기본 구현은 `Unsupported` — mock store (테스트용)는 RBAC가 필요 없으므로
    // trait impl 시 이 메서드들을 재정의하지 않아도 됨. PgStore만 실제 구현.

    /// 신규 사용자 생성. username 충돌 시 `StoreError::Conflict`.
    async fn create_user(&self, _user: &User) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("create_user"))
    }

    /// ID로 사용자 조회.
    async fn get_user_by_id(&self, _id: UserId) -> Result<Option<User>, StoreError> {
        Err(StoreError::Unsupported("get_user_by_id"))
    }

    /// username으로 사용자 조회 (로그인 경로).
    async fn get_user_by_username(&self, _username: &str) -> Result<Option<User>, StoreError> {
        Err(StoreError::Unsupported("get_user_by_username"))
    }

    /// email로 사용자 조회 (이메일 기반 로그인).
    async fn get_user_by_email(&self, _email: &str) -> Result<Option<User>, StoreError> {
        Err(StoreError::Unsupported("get_user_by_email"))
    }

    /// 모든 사용자 조회 (사용자 관리 페이지용).
    async fn list_users(&self) -> Result<Vec<User>, StoreError> {
        Err(StoreError::Unsupported("list_users"))
    }

    /// 사용자 수 반환 (bootstrap 필요 여부 판정용).
    async fn count_users(&self) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("count_users"))
    }

    /// 비밀번호 해시 업데이트 (재설정).
    async fn update_user_password(&self, _id: UserId, _hash: &str) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("update_user_password"))
    }

    /// 마지막 로그인 시각 갱신.
    async fn update_user_last_login(
        &self,
        _id: UserId,
        _at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("update_user_last_login"))
    }

    /// 활성/비활성 토글. 비활성화 시 기존 세션도 별도 삭제 필요.
    async fn set_user_enabled(&self, _id: UserId, _enabled: bool) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("set_user_enabled"))
    }

    /// 사용자 삭제. user_roles / sessions는 CASCADE로 함께 삭제됨.
    async fn delete_user(&self, _id: UserId) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("delete_user"))
    }

    // ── RBAC: Roles & Permissions ─────────────────────────────────

    /// 역할 생성. name 충돌 시 에러.
    async fn create_role(&self, _role: &Role) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("create_role"))
    }

    /// 역할을 name으로 조회 (builtin 시드용).
    async fn get_role_by_name(&self, _name: &str) -> Result<Option<Role>, StoreError> {
        Err(StoreError::Unsupported("get_role_by_name"))
    }

    /// 모든 역할 조회.
    async fn list_roles(&self) -> Result<Vec<Role>, StoreError> {
        Err(StoreError::Unsupported("list_roles"))
    }

    /// 권한 생성 (idempotent — name 충돌 시 무시).
    async fn create_permission(&self, _perm: &Permission) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("create_permission"))
    }

    /// 권한을 name으로 조회.
    async fn get_permission_by_name(&self, _name: &str) -> Result<Option<Permission>, StoreError> {
        Err(StoreError::Unsupported("get_permission_by_name"))
    }

    /// 모든 권한 조회.
    async fn list_permissions(&self) -> Result<Vec<Permission>, StoreError> {
        Err(StoreError::Unsupported("list_permissions"))
    }

    /// 사용자에게 역할 부여.
    async fn assign_user_role(
        &self,
        _user_id: UserId,
        _role_id: RoleId,
        _granted_by: Option<UserId>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("assign_user_role"))
    }

    /// 사용자의 역할 회수.
    async fn revoke_user_role(&self, _user_id: UserId, _role_id: RoleId) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("revoke_user_role"))
    }

    /// 사용자의 모든 역할 조회.
    async fn list_user_roles(&self, _user_id: UserId) -> Result<Vec<Role>, StoreError> {
        Err(StoreError::Unsupported("list_user_roles"))
    }

    /// 사용자의 유효 권한 조회 (역할 → 권한 조인).
    async fn list_user_permissions(&self, _user_id: UserId) -> Result<Vec<Permission>, StoreError> {
        Err(StoreError::Unsupported("list_user_permissions"))
    }

    /// 역할에 권한 부여 (idempotent).
    async fn grant_role_permission(
        &self,
        _role_id: RoleId,
        _permission_id: PermissionId,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("grant_role_permission"))
    }

    // ── Sessions (쿠키 기반 로그인) ──────────────────────────────

    /// 세션 생성 (token_hash는 SHA-256 hex).
    async fn create_session(&self, _session: &Session) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("create_session"))
    }

    /// token_hash로 세션 조회 (만료된 세션도 반환 — 호출자가 만료 판정).
    async fn get_session_by_token_hash(&self, _hash: &str) -> Result<Option<Session>, StoreError> {
        Err(StoreError::Unsupported("get_session_by_token_hash"))
    }

    /// 세션 삭제 (로그아웃).
    async fn delete_session(&self, _id: SessionId) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("delete_session"))
    }

    /// 세션 만료 시각 갱신.
    ///
    /// 토큰 로테이션 시 구 세션을 즉시 삭제하지 않고 짧은 유예 기간만 남기는 데
    /// 사용한다. 즉시 삭제하면 이미 전송 중이던 병렬 요청들이 한꺼번에 401을
    /// 맞고 로그아웃되기 때문이다.
    async fn update_session_expiry(
        &self,
        _id: SessionId,
        _expires_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("update_session_expiry"))
    }

    /// 만료된 세션 일괄 삭제 (정기 정리용).
    async fn delete_expired_sessions(&self) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("delete_expired_sessions"))
    }

    /// 사용자의 모든 세션 삭제 (비활성화/패스워드 변경 시).
    async fn delete_user_sessions(&self, _user_id: UserId) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("delete_user_sessions"))
    }

    // ── Email verification ───────────────────────────────────────

    /// 이메일 인증 토큰 생성.
    async fn create_email_verification_token(
        &self,
        _token: &fleet_core::EmailVerificationToken,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("create_email_verification_token"))
    }

    /// 토큰 해시로 인증 토큰 조회.
    async fn get_email_verification_token(
        &self,
        _token_hash: &str,
    ) -> Result<Option<fleet_core::EmailVerificationToken>, StoreError> {
        Err(StoreError::Unsupported("get_email_verification_token"))
    }

    /// 인증 토큰 소비 (consumed_at 설정).
    async fn consume_email_verification_token(
        &self,
        _token_id: Uuid,
        _at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("consume_email_verification_token"))
    }

    /// 사용자의 email_verified 플래그 설정.
    async fn set_user_email_verified(
        &self,
        _user_id: UserId,
        _verified: bool,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("set_user_email_verified"))
    }

    // ── Password reset ─────────────────────────────────────────────

    /// 비밀번호 재설정 토큰 생성.
    async fn create_password_reset_token(
        &self,
        _token: &fleet_core::PasswordResetToken,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("create_password_reset_token"))
    }

    /// 토큰 해시로 재설정 토큰 조회.
    async fn get_password_reset_token(
        &self,
        _token_hash: &str,
    ) -> Result<Option<fleet_core::PasswordResetToken>, StoreError> {
        Err(StoreError::Unsupported("get_password_reset_token"))
    }

    /// 재설정 토큰 소비.
    async fn consume_password_reset_token(
        &self,
        _token_id: Uuid,
        _at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("consume_password_reset_token"))
    }

    // ── Login attempts (rate limiting + 감사) ────────────────────

    /// 로그인 시도 기록.
    async fn record_login_attempt(&self, _attempt: &LoginAttempt) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("record_login_attempt"))
    }

    /// `(identifier, ip)` 기준 최근 `window_secs`초 내 실패 횟수.
    ///
    /// `ip`가 `None`이면 IP 필터를 적용하지 않고 해당 identifier의 **모든 IP**
    /// 실패를 합산한다 (`ip_address IS NULL`인 행만 세지 않는다).
    async fn count_recent_failed_attempts(
        &self,
        _identifier: &str,
        _ip: Option<&str>,
        _window_secs: i64,
    ) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("count_recent_failed_attempts"))
    }

    /// IP 단독 기준 최근 `window_secs`초 내 실패 횟수 (모든 identifier 합산).
    ///
    /// IP 회전을 통한 rate limit 우회(credential stuffing)를 방지하기 위해
    /// `count_recent_failed_attempts`와 별도로 IP-only 카운트를 시행.
    async fn count_recent_ip_failures(
        &self,
        _ip: &str,
        _window_secs: i64,
    ) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("count_recent_ip_failures"))
    }

    // ── Audit log (구조화된 감사 로그) ──────────────────────────

    /// 감사 이벤트 1건 기록.
    ///
    /// 감사 기록 실패가 원래 작업을 되돌려서는 안 된다 — 호출부는 오류를
    /// 로깅만 하고 진행하는 것이 보통이다.
    async fn record_audit_event(&self, _event: &AuditEvent) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("record_audit_event"))
    }

    /// 감사 이벤트 조회 (최신순).
    async fn list_audit_events(
        &self,
        _filter: &AuditFilter,
    ) -> Result<Vec<AuditEvent>, StoreError> {
        Err(StoreError::Unsupported("list_audit_events"))
    }

    /// identifier의 과거 시도 기록 삭제 (성공 시 초기화).
    async fn clear_login_attempts(
        &self,
        _identifier: &str,
        _ip: Option<&str>,
    ) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("clear_login_attempts"))
    }

    /// 지정 시각 이전의 모든 로그인 시도 기록 삭제 (테이블 무한 증가 방지).
    ///
    /// 정기적 또는 기회적(login 성공 시)으로 호출하여 login_attempts 테이블이
    /// 제한 없이 커지는 것을 방지.
    async fn delete_old_login_attempts(
        &self,
        _before: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("delete_old_login_attempts"))
    }

    // ── Worker credentials (Phase 8.6) ─────────────────────────────
    //
    // 기본 구현은 `Unsupported` — mock store (테스트용)는 credentials가 필요
    // 없으므로 trait impl 시 재정의하지 않아도 됨. PgStore만 실제 구현.

    /// 워커의 자격 증명(API 키 등)을 암호화하여 저장하거나 갱신.
    ///
    /// 동일한 (worker_name, model_id) 조합이 이미 존재하면 덮어씀(upsert).
    /// `encrypted_blob`은 이미 암호화된 상태로 전달되어야 함 — 이 trait은
    /// 암호화를 수행하지 않음. (암호화는 상위 크레이트 `fleet-credentials`와
    /// `fleet-api`/`fleet-cli`에서 처리.)
    #[allow(clippy::too_many_arguments)]
    async fn upsert_worker_credential(
        &self,
        _worker_name: &str,
        _model_id: &str,
        _encrypted_blob: &str,
        _base_url: &str,
        _api_backend: &str,
        _context_window: u32,
        _model_name: Option<&str>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("upsert_worker_credential"))
    }

    /// 특정 (worker_name, model_id) 자격 증명 조회.
    /// 암호화된 blob을 그대로 반환 (복호화는 호출자 책임).
    async fn get_worker_credential(
        &self,
        _worker_name: &str,
        _model_id: &str,
    ) -> Result<Option<StoredCredential>, StoreError> {
        Err(StoreError::Unsupported("get_worker_credential"))
    }

    /// 워커의 모든 자격 증명 조회 (모델별 여러 개일 수 있음).
    async fn list_worker_credentials(
        &self,
        _worker_name: &str,
    ) -> Result<Vec<StoredCredential>, StoreError> {
        Err(StoreError::Unsupported("list_worker_credentials"))
    }

    /// 자격 증명 삭제. 존재하지 않으면 false 반환.
    async fn delete_worker_credential(
        &self,
        _worker_name: &str,
        _model_id: &str,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("delete_worker_credential"))
    }

    // ── Host inventory (Phase P1.5) ───────────────────────────────
    //
    // 기본 구현은 `Unsupported` — mock store (테스트용)는 host 기능이 필요 없음.
    // PgStore만 실제 구현.

    /// 호스트를 upsert (hostname 기준). 동일 hostname이 존재하면 갱신.
    async fn upsert_host(&self, _host: &fleet_core::Host) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("upsert_host"))
    }

    /// hostname으로 호스트 조회.
    async fn get_host_by_hostname(
        &self,
        _hostname: &str,
    ) -> Result<Option<fleet_core::Host>, StoreError> {
        Err(StoreError::Unsupported("get_host_by_hostname"))
    }

    /// worker_id로 호스트 조회.
    async fn get_host_by_worker(
        &self,
        _worker_id: WorkerId,
    ) -> Result<Option<fleet_core::Host>, StoreError> {
        Err(StoreError::Unsupported("get_host_by_worker"))
    }

    /// 모든 호스트 목록 조회 (생성일순).
    async fn list_hosts(&self) -> Result<Vec<fleet_core::Host>, StoreError> {
        Err(StoreError::Unsupported("list_hosts"))
    }

    /// 호스트 이벤트 추가 (타임라인).
    async fn append_host_event(&self, _event: &fleet_core::HostEvent) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("append_host_event"))
    }

    /// 특정 호스트의 이벤트 목록 (최신순, limit 제한).
    async fn list_host_events(
        &self,
        _host_id: uuid::Uuid,
        _limit: u32,
    ) -> Result<Vec<fleet_core::HostEvent>, StoreError> {
        Err(StoreError::Unsupported("list_host_events"))
    }

    // ── SSH 키 금고 ───────────────────────────────────────────────

    /// SSH 비밀키 저장 (이미 암호화됨).
    async fn create_ssh_key(&self, _key: &fleet_core::SshKey) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("create_ssh_key"))
    }

    /// 이름으로 SSH 키 조회.
    async fn get_ssh_key(&self, _name: &str) -> Result<Option<fleet_core::SshKey>, StoreError> {
        Err(StoreError::Unsupported("get_ssh_key"))
    }

    /// 전체 SSH 키 목록.
    async fn list_ssh_keys(&self) -> Result<Vec<fleet_core::SshKey>, StoreError> {
        Err(StoreError::Unsupported("list_ssh_keys"))
    }

    /// SSH 키 삭제.
    async fn delete_ssh_key(&self, _name: &str) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("delete_ssh_key"))
    }

    // ── Control plane lease (로드맵 #63, 1단계) ──────────────────────
    //
    // 기본 구현은 `Unsupported` — lease 없이도 동작하던 기존 minimal Store
    // 테스트 double(통합 테스트가 직접 구현하는 fixture)을 깨지 않기 위함.
    // PgStore/MemStore만 실제로 CAS 의미론을 구현한다.

    /// lease를 획득한다(최초 획득 포함). 유효한(만료되지 않은) lease를 다른
    /// instance가 쥐고 있으면 `StoreError::Conflict`를 반환한다 — 상태
    /// 다이어그램의 `Refused` 전이. 기존 lease가 이미 만료됐으면 그대로
    /// 가로채(`epoch`를 올려) 새로 획득한다 — Cold Standby가 이전 Primary의
    /// TTL 만료를 기다렸다가 자동으로 승격하는 경로다.
    async fn acquire_control_lease(
        &self,
        _cluster_id: &str,
        _instance_id: &str,
        _ttl: std::time::Duration,
    ) -> Result<ControlLease, StoreError> {
        Err(StoreError::Unsupported("acquire_control_lease"))
    }

    /// 현재 보유 중인 lease를 갱신한다. `(instance_id, epoch)`가 저장된
    /// 값과 정확히 일치하고 아직 만료되지 않은 경우에만 성공한다 — 실패는
    /// 이 instance가 더 이상 유효한 제어권이 없다는 뜻이므로(다른
    /// instance가 가로챘거나 이미 만료됨) 신규 제어 동작을 즉시 멈춰야
    /// 한다(상태 다이어그램의 `Active → Fenced`).
    async fn renew_control_lease(
        &self,
        _cluster_id: &str,
        _instance_id: &str,
        _epoch: i64,
        _ttl: std::time::Duration,
    ) -> Result<ControlLease, StoreError> {
        Err(StoreError::Unsupported("renew_control_lease"))
    }

    /// lease를 명시적으로 반납한다(정상 종료). `expires_at`을 즉시 과거로
    /// 당겨 standby가 TTL을 기다리지 않고 곧바로 획득할 수 있게 한다.
    /// `(instance_id, epoch)`가 일치하지 않으면(이미 다른 instance가
    /// 가로챈 경우 등) `false`를 반환하고 아무것도 바꾸지 않는다.
    async fn release_control_lease(
        &self,
        _cluster_id: &str,
        _instance_id: &str,
        _epoch: i64,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("release_control_lease"))
    }

    /// 현재 lease 상태를 읽기 전용으로 조회한다(관측용). 한 번도 획득된
    /// 적 없으면 `None`.
    async fn get_control_lease(
        &self,
        _cluster_id: &str,
    ) -> Result<Option<ControlLease>, StoreError> {
        Err(StoreError::Unsupported("get_control_lease"))
    }

    // ── Project (로드맵 #48, 1단계) ───────────────────────────────────
    //
    // 기본 구현은 `Unsupported` — 다른 신규 trait method들과 같은 관례로,
    // 기존 minimal Store 테스트 double을 깨지 않기 위함이다.

    /// Project를 생성한다. `name`이 이미 존재하면 `StoreError::Conflict`.
    async fn create_project(&self, _project: &Project) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("create_project"))
    }

    /// id로 단건 조회.
    async fn get_project(&self, _id: ProjectId) -> Result<Option<Project>, StoreError> {
        Err(StoreError::Unsupported("get_project"))
    }

    /// 고유 이름으로 단건 조회 (생성 시 중복 확인, Task 제출 시 이름으로
    /// project를 지정하는 경로 등에 사용).
    async fn get_project_by_name(&self, _name: &str) -> Result<Option<Project>, StoreError> {
        Err(StoreError::Unsupported("get_project_by_name"))
    }

    /// 목록 조회 (최신순).
    async fn list_projects(&self, _filter: &ProjectFilter) -> Result<Vec<Project>, StoreError> {
        Err(StoreError::Unsupported("list_projects"))
    }

    /// 상태를 전이하고 `updated_at`을 갱신한다. 전이 유효성(예: `Archived`에서
    /// 곧바로 `Draining`으로 못 감)은 이 메서드가 아니라 호출부(`fleet-api`
    /// 핸들러)가 검사한다 — Store는 CAS 없는 단순 쓰기다. 존재하지 않는
    /// id면 `false`.
    async fn update_project_status(
        &self,
        _id: ProjectId,
        _status: ProjectStatus,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("update_project_status"))
    }

    /// 이 Project를 참조하는 비종료(`Pending`/`Dispatched`) Task가 하나라도
    /// 있는지. `Draining → Archived` 전이의 유일한 게이트(1단계) — 목표
    /// 계약의 나머지 archive 게이트(Agent process/lease/credential grant
    /// cleanup 증거)는 그 하부 구조가 생기기 전까지 없다.
    async fn project_has_active_tasks(&self, _project_id: ProjectId) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("project_has_active_tasks"))
    }

    // ── Agent (로드맵 #49, 1단계) ─────────────────────────────────────
    //
    // 기본 구현은 `Unsupported` — Project와 같은 관례.
    //
    // **여기에 Agent를 다른 Project로 옮기는 메서드는 없다.** `project_id`는
    // 생성 시점에 고정되며 정본은 이동 대신 새 Agent 생성을 규정한다
    // (docs/architecture/entity-placement-and-context.md). 갱신 경로를 두지
    // 않는 것이 그 불변식의 집행 방법이다.

    /// Agent를 생성한다. 같은 Project에 같은 `name`이 이미 있으면
    /// `StoreError::Conflict`. `project_id`가 실재하는지는 호출부가
    /// `project_rules::ensure_project_accepts_new_agents`로 먼저 검사한다 —
    /// FK 위반은 그 검사를 통과한 뒤의 경합에서만 나타나는 2차 방어선이다.
    ///
    /// **반환값은 실제로 기록된 배정이다** (로드맵 `#67` 구현 게이트 ①-A-2).
    /// `agent.worker_id`가 `Some`이어도 그 Worker가 상한에 걸려 있으면
    /// 배정 없이 생성되고 `None`이 돌아온다 — 배정 실패가 생성을 되돌리지
    /// 않는다는 4a의 결정을 그대로 지킨다. 호출부는 이 값으로 로컬
    /// 구조체를 **되맞춰야** 한다. 그러지 않으면 응답과 감사 로그가
    /// 일어나지 않은 배정을 기록한다.
    async fn create_agent(&self, _agent: &Agent) -> Result<Option<WorkerId>, StoreError> {
        Err(StoreError::Unsupported("create_agent"))
    }

    /// id로 단건 조회.
    async fn get_agent(&self, _id: AgentId) -> Result<Option<Agent>, StoreError> {
        Err(StoreError::Unsupported("get_agent"))
    }

    /// Project 안에서 이름으로 단건 조회. 이름은 Project 범위에서만 유일하므로
    /// `project_id` 없이는 조회할 수 없다.
    async fn get_agent_by_name(
        &self,
        _project_id: ProjectId,
        _name: &str,
    ) -> Result<Option<Agent>, StoreError> {
        Err(StoreError::Unsupported("get_agent_by_name"))
    }

    /// 목록 조회 (최신순).
    async fn list_agents(&self, _filter: &AgentFilter) -> Result<Vec<Agent>, StoreError> {
        Err(StoreError::Unsupported("list_agents"))
    }

    /// 상태를 전이하고 `updated_at`을 갱신한다. Project와 같이 Store는 CAS
    /// 없는 단순 쓰기이며 전이 유효성은 호출부가 검사한다. 존재하지 않는
    /// id면 `false`.
    ///
    /// `Stopped`로 갈 때는 `desired_status`도 같은 쓰기에서 `stopped`로
    /// 내린다(로드맵 `#67` 4b). 회수를 두 번의 쓰기로 나누면 그 사이에
    /// heartbeat이 끼어 이미 회수된 Agent에 `running` 명령이 나간다.
    async fn update_agent_status(
        &self,
        _id: AgentId,
        _status: AgentStatus,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("update_agent_status"))
    }

    /// 이 Project에 아직 회수되지 않은(`Ready`) Agent가 하나라도 있는지.
    /// `Draining → Archived` 전이의 두 번째 게이트 — 정본의 `ArchiveBlocked`
    /// 조건 중 "Agent cleanup 증거"의, 1단계에서 실제로 확인 가능한 부분이다.
    async fn project_has_live_agents(&self, _project_id: ProjectId) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("project_has_live_agents"))
    }

    /// Agent를 Worker에 (재)배정하고 `assigned_at`/`updated_at`을 갱신한다
    /// (로드맵 `#67` 4a). 존재하지 않는 id면 [`SlotClaim::NoSuchAgent`].
    ///
    /// **`command_generation`도 함께 올린다**(로드맵 `#67` 4b). 새 Worker는
    /// 이전 Worker가 받은 명령을 본 적이 없으므로, 올리지 않으면
    /// `last_acked_generation == command_generation`이 "새 Worker가
    /// 확인했다"는 거짓을 말한다.
    ///
    /// 생성 시점의 배정은 이 메서드를 쓰지 않는다 — `create_agent`의 INSERT가
    /// 두 컬럼을 함께 넣는다. 이 메서드의 호출자는 운영자의 명시적 재배정
    /// 하나뿐이며, 그래서 배정 **해제** 메서드는 만들지 않았다: 해제할 이유
    /// 두 가지가 모두 다른 곳에서 처리된다. Worker 등록 해제는 `030`의
    /// `ON DELETE SET NULL`이, Agent 회수는 아래 원장이 `stopped`를 세지
    /// 않는 것이 처리한다.
    ///
    /// **상한을 `workers` 행 잠금 아래에서 집행한다**
    /// (로드맵 `#67` 구현 게이트 ①-A-2). 세는 대상은 그 Worker에 배정된
    /// `agents` 행이고, 상한을 소유한 것은 `workers` 행이므로 잠금은
    /// 후자에 건다 — 아직 존재하지 않는 행은 잠글 수 없으니
    /// `INSERT ... WHERE (SELECT COUNT(*)) < cap`은 READ COMMITTED에서
    /// phantom을 막지 못한다. 다른 Worker로의 배정은 서로 잠그지 않는다.
    async fn assign_agent_worker(
        &self,
        _id: AgentId,
        _worker_id: WorkerId,
    ) -> Result<SlotClaim, StoreError> {
        Err(StoreError::Unsupported("assign_agent_worker"))
    }

    /// 배정 원장 — Worker별로 배정된 채 아직 회수되지 않은 Agent 수
    /// (로드맵 `#67` 4a).
    ///
    /// `count_dispatched_tasks_by_worker`와 같은 역할을 Agent에 대해 한다:
    /// least-loaded 배정의 부하 출처이며, 출처는 오케스트레이터 자신의 원장
    /// 이지 Worker 자기보고가 아니다(그 판단의 근거는
    /// `fleet-scheduler/src/selector.rs`의 `#67` 3단계 논거와 같다).
    ///
    /// `stopped` Agent는 세지 않는다 — 회수된 Agent는 실행될 일이 없으므로
    /// 슬롯을 잡지 않는다. 이것이 별도의 배정 해제 경로를 불필요하게 만든다.
    async fn count_agents_by_worker(
        &self,
    ) -> Result<std::collections::HashMap<WorkerId, u32>, StoreError> {
        Err(StoreError::Unsupported("count_agents_by_worker"))
    }

    // ── 수렴 프로토콜 (로드맵 #67 4b) ──────────────────────────────────
    //
    // 명령 큐 테이블이 아니라 `agents` 행의 desired state로 구현한다. 아래
    // 세 메서드가 각각 명령 발행·전달·수신 확인이며, "지연된 ACK가 새 상태를
    // 덮어쓰지 못한다"는 세 번째 메서드의 CAS가 그대로 만든다.

    /// 이 Agent에 바라는 상태를 정한다 (로드맵 `#67` 4b).
    ///
    /// 값이 실제로 바뀔 때만 `command_generation`을 올린다 — 매 호출마다
    /// 올리면 같은 의도를 반복해 눌렀다는 이유로 이미 확인된 명령이
    /// 미확인으로 되돌아간다. 존재하지 않는 id면 `false`.
    async fn set_agent_desired_status(
        &self,
        _id: AgentId,
        _desired: AgentDesiredStatus,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("set_agent_desired_status"))
    }

    /// 이 Worker에 배정된, 회수되지 않은 Agent들의 현재 명령
    /// (로드맵 `#67` 4b). heartbeat 응답이 매 beat 이것을 통째로 싣는다.
    ///
    /// 매번 전부 다시 보내기 때문에 `expires_at`이 필요 없다 — 큐 모델에서
    /// "오래된 명령이 뒤늦게 실행되는" 창이 여기서는 열리지 않고, 신선도
    /// 판정은 generation이 한다.
    ///
    /// **`AgentFilter`를 쓰지 않는 이유**: 그 구조체의 `limit`은 파생
    /// `Default`가 0을 주고 두 Store가 조용히 1로 올린다(`mem.rs`의
    /// `max(1)`, `postgres.rs`의 `clamp(1, 1000)`). 그 함정에 걸리면 모든
    /// Worker가 정확히 한 개의 명령만 받고, 증상은 어느 크레이트에서든
    /// 똑같은 `left: 1`이라 원인을 가리키지 않는다.
    async fn list_agent_commands(
        &self,
        _worker_id: WorkerId,
    ) -> Result<Vec<AgentCommand>, StoreError> {
        Err(StoreError::Unsupported("list_agent_commands"))
    }

    /// Worker의 명령 수신 확인을 반영하고 반영된 건수를 돌려준다
    /// (로드맵 `#67` 4b).
    ///
    /// CAS 조건이 셋이다: `command_generation = $ack`(지연된 ACK가 새 상태를
    /// 덮어쓰지 못한다), `worker_id = $worker`(4a로 재배정이 가능해졌으므로,
    /// 더는 자기 것이 아닌 Agent를 확인해 주지 못하게 한다),
    /// `last_acked_generation < $ack`(같은 세대의 중복 ACK를 no-op으로 만들어
    /// 반환 건수가 "실제로 새로 확인된 수"가 되게 한다).
    async fn ack_agent_commands(
        &self,
        _worker_id: WorkerId,
        _acks: &[AgentAck],
    ) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("ack_agent_commands"))
    }

    /// Worker가 이번 beat에 **본 것**을 반영하고 실제로 바뀐 행 수를 돌려준다
    /// (로드맵 `#67` 4c-B).
    ///
    /// **목록은 그 Worker에 대한 권위 있는 전체 집합이다.** heartbeat 응답의
    /// 명령 목록을 워커가 그렇게 읽는 것과 대칭이며, 그래서 목록에 없는 Agent의
    /// 관측은 **지운다**. 지우지 않으면 회수돼 프로세스가 사라진 Agent에
    /// `observed_status = running`이 영원히 남고, 그것을 지울 주체가 어디에도
    /// 없다. 그래서 `Some(&[])`은 "이 Worker에는 관측할 것이 하나도 없다"라는
    /// 뜻이 되며, "말해 줄 것이 없다"는 애초에 이 함수를 부르지 않는 것으로
    /// 표현한다(호출부의 `Option`).
    ///
    /// `worker_id`로 잠근다 — 재배정된 Agent에 대해 이전 Worker의 지연된 beat이
    /// 관측을 적지 못하게 한다. `ack_agent_commands`의 세 CAS 조건 중 이것 하나만
    /// 가져오는 이유는 나머지 둘이 세대에 대한 것이고 관측에는 세대가 없기
    /// 때문이다: 관측은 명령 없이도 바뀐다(프로세스가 스스로 죽는다).
    ///
    /// **`status`는 건드리지 않는다.** 그 컬럼은 운영자의 회수가 쓰는 자리이고,
    /// 여기서 함께 쓰면 회수 직후 도착한 beat이 회수를 덮는다
    /// (`032_agent_observed_state.sql`).
    ///
    /// `updated_at`도 밀지 않는다 — `ack_agent_commands`와 같은 이유다. 관측은
    /// 프로토콜 부기이지 운영자의 변경이 아니며, 매 beat 밀면 "언제 회수됐는가"가
    /// heartbeat 주기로 덧칠된다.
    async fn apply_agent_observations(
        &self,
        _worker_id: WorkerId,
        _observations: &[AgentObservation],
    ) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("apply_agent_observations"))
    }

    // ── AgentTemplate (로드맵 #86, 1단계) ─────────────────────────────
    //
    // 기본 구현은 `Unsupported` — 다른 신규 도메인과 같은 관례.
    //
    // **여기에 revision 본문을 고치는 메서드는 없다.** revision immutability는
    // DB 트리거가 아니라 "그런 함수가 존재하지 않음"으로 집행한다. `Agent`의
    // `project_id`가 불변인 것과 같은 방식이며, 두 Store 구현 중 하나가
    // 실수로 UPDATE를 넣는 사고를 트레이트 차원에서 막는다.

    /// 템플릿 정체성을 생성한다(항상 `Draft`). 같은 범위에 같은 `name`이 있으면
    /// `Conflict` — 범위는 `project_id`가 있으면 그 Project, 없으면 전역이다.
    async fn create_agent_template(&self, _template: &AgentTemplate) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("create_agent_template"))
    }

    async fn get_agent_template(
        &self,
        _id: AgentTemplateId,
    ) -> Result<Option<AgentTemplate>, StoreError> {
        Err(StoreError::Unsupported("get_agent_template"))
    }

    /// 목록 조회 (최신순).
    async fn list_agent_templates(
        &self,
        _filter: &AgentTemplateFilter,
    ) -> Result<Vec<AgentTemplate>, StoreError> {
        Err(StoreError::Unsupported("list_agent_templates"))
    }

    /// 수명 주기 전이. 전이 유효성(`can_transition_to`)은 호출부가 검사하며,
    /// Store는 `retire`만 특별 취급한다(아래 [`Store::retire_agent_template`]).
    /// 존재하지 않는 id면 `false`.
    async fn update_agent_template_status(
        &self,
        _id: AgentTemplateId,
        _status: AgentTemplateStatus,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("update_agent_template_status"))
    }

    /// 이 템플릿에 pin한 Agent id 목록 (정렬됨).
    ///
    /// 1단계의 의존 집합은 이것이 전부다. `#87`의 Attempt pin이 들어오면 그
    /// 종류가 추가된다.
    async fn agent_template_dependents(
        &self,
        _id: AgentTemplateId,
    ) -> Result<Vec<AgentId>, StoreError> {
        Err(StoreError::Unsupported("agent_template_dependents"))
    }

    /// 의존 집합 해시를 제시하고 retire한다.
    ///
    /// 트랜잭션 안에서 의존 집합을 **다시** 계산해 `expected_dependent_hash`와
    /// 대조하고, 다르면 `Conflict`를 낸다. 확인 화면이 보여준 목록과 실제로
    /// 못 쓰게 되는 목록이 다를 수 있기 때문이다 — 그 사이에 누군가 이 템플릿을
    /// pin한 Agent를 새로 만들었다면, 조작자는 자기가 승인하지 않은 회수를
    /// 집행하게 된다.
    ///
    /// 존재하지 않는 id면 `false`.
    async fn retire_agent_template(
        &self,
        _id: AgentTemplateId,
        _expected_dependent_hash: &str,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("retire_agent_template"))
    }

    /// 새 revision을 발행한다. `content_revision`은 **Store가** 트랜잭션 안에서
    /// 할당한다 — 호출부가 정하면 경합에서 같은 번호 두 개가 생긴다.
    ///
    /// `body`는 저장 전에 정규화된다. 정규화 전 형태를 저장하면 저장된 값에서
    /// `content_hash`를 재계산할 수 없어 감사 대조가 불가능해진다.
    ///
    /// 템플릿이 `Retired`/`Discarded`면 `Conflict`.
    async fn create_agent_template_revision(
        &self,
        _template_id: AgentTemplateId,
        _body: &AgentTemplateBody,
        _created_by: Option<&str>,
    ) -> Result<AgentTemplateRevision, StoreError> {
        Err(StoreError::Unsupported("create_agent_template_revision"))
    }

    /// 템플릿의 revision 목록 (`content_revision` 내림차순).
    async fn list_agent_template_revisions(
        &self,
        _template_id: AgentTemplateId,
    ) -> Result<Vec<AgentTemplateRevision>, StoreError> {
        Err(StoreError::Unsupported("list_agent_template_revisions"))
    }

    async fn get_agent_template_revision(
        &self,
        _id: AgentTemplateRevisionId,
    ) -> Result<Option<AgentTemplateRevision>, StoreError> {
        Err(StoreError::Unsupported("get_agent_template_revision"))
    }

    /// revision에 `revoked_at`을 찍어 **새 pin을 막는다**. 이미 pin한 Agent는
    /// 영향받지 않는다 — 과거 실행의 재현성을 사후에 깨뜨리지 않는 것이
    /// revision immutability의 요지다. 이미 revoke됐거나 없는 id면 `false`.
    async fn revoke_agent_template_revision(
        &self,
        _id: AgentTemplateRevisionId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("revoke_agent_template_revision"))
    }

    // ── Issue (로드맵 #88) ────────────────────────────────────────────
    //
    // 기본 구현은 `Unsupported` — 다른 신규 trait method들과 같은 관례.
    //
    // **여기에 "Task 상태를 보는" 메서드는 없다**(불변식 I2) — Issue의
    // close에는 Task 상태에 대한 선행 조건이 없다. 반대로 Task 쪽 메서드도
    // Issue를 읽지 않는다(I1). 두 방향 모두 비어 있어야 교착이 없다.

    async fn create_issue(&self, _issue: &Issue) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("create_issue"))
    }

    async fn get_issue(&self, _id: IssueId) -> Result<Option<Issue>, StoreError> {
        Err(StoreError::Unsupported("get_issue"))
    }

    async fn list_issues(&self, _filter: &IssueFilter) -> Result<Vec<Issue>, StoreError> {
        Err(StoreError::Unsupported("list_issues"))
    }

    /// 상태 외 필드(title·body·labels·severity·assignee) 갱신.
    ///
    /// 상태 전이는 [`Store::transition_issue`]가 따로 담당한다 — `issue:update`
    /// (오탈자 수정)와 `issue:close`(문제 종결)를 다른 capability로 분리한
    /// 계약을 저장소 API 수준에서도 갈라 둬, 호출부가 실수로 한 메서드에
    /// 둘 다 태우지 못하게 한다.
    async fn update_issue_fields(&self, _issue: &Issue) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("update_issue_fields"))
    }

    /// 검증된 상태 전이를 영속화한다. 전이 유효성은 호출부가
    /// [`fleet_core::Issue::transition_to`]로 이미 확인한 상태로 들어온다 —
    /// 이 메서드는 `(status, close_reason)`을 함께 쓰는 것만 보장한다(DB의
    /// CHECK 제약이 둘의 정합성을 다시 강제한다).
    async fn transition_issue(
        &self,
        _id: IssueId,
        _status: IssueStatus,
        _close_reason: Option<CloseReason>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("transition_issue"))
    }

    async fn add_issue_comment(&self, _comment: &IssueComment) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("add_issue_comment"))
    }

    async fn list_issue_comments(
        &self,
        _issue_id: IssueId,
    ) -> Result<Vec<IssueComment>, StoreError> {
        Err(StoreError::Unsupported("list_issue_comments"))
    }

    /// Issue와 Task를 연관짓는다. 이미 연관돼 있으면 `false`(멱등).
    async fn link_issue_task(&self, _link: &IssueTaskLink) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("link_issue_task"))
    }

    async fn unlink_issue_task(
        &self,
        _issue_id: IssueId,
        _task_id: TaskId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("unlink_issue_task"))
    }

    async fn list_issue_task_links(
        &self,
        _issue_id: IssueId,
    ) -> Result<Vec<IssueTaskLink>, StoreError> {
        Err(StoreError::Unsupported("list_issue_task_links"))
    }

    /// 이 Issue에 연관된 비터미널 Task가 있는지 — UI의 "진행 중" **파생**
    /// 배지용이다(로드맵 `#88`).
    ///
    /// **이 값을 Issue 상태로 저장하지 않는다.** `InProgress` 상태를 두지
    /// 않은 이유가 정확히 이것이다 — 저장하는 순간 Task 상태의 복제본이
    /// 생기고 두 상태 머신이 경쟁한다. 읽기 전용 유도 값으로만 쓴다.
    ///
    /// I2를 깨지 않는다: 이 메서드는 close 경로가 호출하지 않으며, Issue
    /// 전이의 어떤 선행 조건도 아니다.
    async fn issue_has_active_tasks(&self, _issue_id: IssueId) -> Result<bool, StoreError> {
        Err(StoreError::Unsupported("issue_has_active_tasks"))
    }
}

/// DB에 저장된 자격 증명 행. api_key는 암호화된 상태로 반환됨.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredCredential {
    pub worker_name: String,
    pub model_id: String,
    /// AES-256-GCM으로 암호화된 blob (base64).
    pub encrypted_blob: String,
    pub base_url: String,
    pub api_backend: String,
    pub context_window: u32,
    pub model_name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub rotated_at: chrono::DateTime<chrono::Utc>,
}
