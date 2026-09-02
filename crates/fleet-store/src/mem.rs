//! 테스트 전용 완전 인메모리 [`Store`] 구현.
//!
//! `test-support` 피처 뒤에 게이트되어 있어 프로덕션 빌드에는 절대 포함되지
//! 않는다. `fleet-api`/`fleet-dashboard`/`fleet-scheduler`가 각자
//! `#[cfg(test)] struct MemStore { ... }`를 따로 구현하고 있던 것(10개 파일,
//! 로드맵 #45)을 이 하나로 통합한다 — 한 곳을 고쳐도 나머지가 갈라질 위험을
//! 없애고, [`PgStore`](crate::PgStore)의 실제 SQL 동작(정렬 순서, `NotFound`
//! 반환 조건, `dispatched_at` 자동 갱신 등)과 일치하도록 맞췄다.
//!
//! ## 실패 주입
//!
//! 소수의 회복성 테스트(HealthChecker/Reconciler/SessionCleanup)는 특정
//! 메서드가 항상 에러를 반환해야 한다. [`MemStore::with_failing`]으로 메서드
//! 이름을 등록하면 해당 메서드가 [`StoreError::Unsupported`]를 반환한다.
//!
//! ## 직접 데이터 주입
//!
//! 실제 API 흐름(예: `create_user` → `assign_user_role` → `grant_role_permission`)을
//! 거치지 않고 테스트 셋업을 단순화하고 싶을 때는 [`MemStore::with_worker`],
//! [`MemStore::with_task`], [`MemStore::with_host`], [`MemStore::seed_permissions`]
//! 등 동기 헬퍼로 데이터를 직접 주입할 수 있다.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use fleet_core::{
    Agent, AgentAck, AgentCommand, AgentDesiredStatus, AgentFilter, AgentId, AgentObservation,
    AgentObservedStatus, AgentStatus, AgentTemplate, AgentTemplateBody, AgentTemplateFilter,
    AgentTemplateId, AgentTemplateRevision, AgentTemplateRevisionId, AgentTemplateStatus,
    AuditEvent, AuditFilter, BootstrapToken, CloseReason, EmailVerificationToken, EventEntry,
    FleetEvent, Host, HostEvent, IdempotentInsert, Issue, IssueComment, IssueFilter, IssueId,
    IssueStatus, IssueTaskLink, LoginAttempt, Permission, PermissionId, Project, ProjectFilter,
    ProjectId, ProjectStatus, Role, RoleId, Session, SessionId, SshKey, Task, TaskDeleteOutcome,
    TaskFilter, TaskId, TaskOutput, TaskOutputChunk, TaskPhase, TaskStatus, TaskStatusFilter,
    TransitionOrigin, TransitionOutcome, User, UserId, Worker, WorkerFilter, WorkerHeartbeat,
    WorkerId,
};

use crate::{
    AdminApiToken, CommandIssue, ControlFence, ControlLease, SlotClaim, Store, StoreError,
    StoredCredential, WorkerOperationalCredential,
};

/// 모든 메서드가 실제로 동작하는 인메모리 [`Store`] — 테스트 전용 단일 구현.
#[derive(Default)]
pub struct MemStore {
    tasks: Mutex<HashMap<TaskId, Task>>,
    workers: Mutex<HashMap<WorkerId, Worker>>,
    events: Mutex<Vec<EventEntry>>,
    outputs: Mutex<HashMap<TaskId, Vec<String>>>,
    bootstrap_tokens: Mutex<HashMap<String, BootstrapToken>>,
    worker_operational_credentials: Mutex<HashMap<String, WorkerOperationalCredential>>,
    /// principal_id → admin API token (로드맵 #72). PG의 `principal_id` PK와
    /// 동일하게 키를 잡아 "1 principal = 1 활성 토큰"을 mem 구현에서도 강제.
    admin_api_tokens: Mutex<HashMap<String, AdminApiToken>>,
    credentials: Mutex<HashMap<(String, String), StoredCredential>>,
    users: Mutex<HashMap<UserId, User>>,
    roles: Mutex<HashMap<RoleId, Role>>,
    permissions: Mutex<HashMap<PermissionId, Permission>>,
    user_roles: Mutex<HashMap<UserId, Vec<RoleId>>>,
    role_permissions: Mutex<HashMap<RoleId, Vec<PermissionId>>>,
    /// 역할 경유 계산과 별개로, 테스트가 사용자에게 직접 주입한 권한
    /// (실제 role/permission 시딩 없이 "이 사용자는 이 권한들을 가진다"를
    /// 바로 표현하고 싶을 때 사용 — `seed_permissions` 참고).
    direct_user_permissions: Mutex<HashMap<UserId, Vec<Permission>>>,
    sessions: Mutex<HashMap<String, Session>>,
    email_verification_tokens: Mutex<HashMap<Uuid, EmailVerificationToken>>,
    password_reset_tokens: Mutex<HashMap<Uuid, EmailVerificationToken>>,
    login_attempts: Mutex<Vec<LoginAttempt>>,
    audit_events: Mutex<Vec<AuditEvent>>,
    hosts: Mutex<Vec<Host>>,
    host_events: Mutex<Vec<HostEvent>>,
    ssh_keys: Mutex<HashMap<String, SshKey>>,
    control_leases: Mutex<HashMap<String, ControlLease>>,
    projects: Mutex<HashMap<ProjectId, Project>>,
    agents: Mutex<HashMap<AgentId, Agent>>,
    agent_templates: Mutex<HashMap<AgentTemplateId, AgentTemplate>>,
    agent_template_revisions: Mutex<HashMap<AgentTemplateRevisionId, AgentTemplateRevision>>,
    issues: Mutex<HashMap<IssueId, Issue>>,
    issue_comments: Mutex<Vec<IssueComment>>,
    issue_task_links: Mutex<Vec<IssueTaskLink>>,
    /// 실패 주입 대상 메서드 이름 집합 — `check`/`record` 자체가 아니라
    /// 테스트 셋업 편의를 위한 것이므로 트레이트 밖 필드.
    failing: Mutex<HashSet<&'static str>>,
    /// `create_agent`가 후보 Worker를 받아도 **배정 없이** 저장하게 만드는
    /// 주입 스위치. 상한 잠금이 선점에 실패한 결과를 결정적으로 세운다
    /// (`dropping_placements` 참고).
    drop_placements: Mutex<bool>,
    last_delete_old_login_attempts_cutoff: Mutex<Option<DateTime<Utc>>>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Arc<dyn Store>`으로 바로 래핑.
    pub fn new_arc() -> std::sync::Arc<dyn Store> {
        std::sync::Arc::new(Self::new())
    }

    /// 지정한 메서드 이름들이 항상 `Err(StoreError::Unsupported(name))`을
    /// 반환하도록 만든다. 회복성(에러 처리) 테스트 전용.
    ///
    /// 지원 대상: `"list_tasks"`, `"delete_expired_sessions"`,
    /// `"delete_old_login_attempts"`, `"record_audit_event"`(로드맵 `#76` —
    /// 감사 기록 실패 시 발급된 secret을 즉시 회수하는 fail-closed 경로를
    /// 시험하기 위함).
    pub fn with_failing(self, methods: &[&'static str]) -> Self {
        self.failing.lock().unwrap().extend(methods.iter().copied());
        self
    }

    /// `create_agent`가 후보 Worker를 실어 보내도 배정 없이 저장하도록
    /// 만든다 — 상한 잠금이 선점에 실패한 결과(`Ok(None)`)를 결정적으로
    /// 재현한다. `with_failing`과 달리 **오류가 아니다**: 선점 실패는
    /// 생성을 막지 않고 배정만 떨어뜨리는 정상 경로이기 때문이다.
    ///
    /// 이 스위치가 필요한 이유는 경합으로 그 상태를 만들 수 없기
    /// 때문이다. MemStore의 연산 사이에는 `.await` 양보점이 없어
    /// `place_on_create`와 `create_agent`가 한 태스크 안에서 이어 붙고,
    /// 8-way 배리어로 동시 생성을 던져도 두 요청이 겹치지 않았다(12회
    /// 중 0건). 상위 계층(Dashboard·MCP 생성 핸들러)이 저장된 사실에
    /// 되맞추는지는 결과를 직접 세워야 검증된다.
    pub fn dropping_placements(self) -> Self {
        *self.drop_placements.lock().unwrap() = true;
        self
    }

    fn is_failing(&self, method: &'static str) -> bool {
        self.failing.lock().unwrap().contains(method)
    }

    /// 호출자가 건 control-plane 술어가 지금도 성립하는가
    /// (로드맵 `#67` 구현 게이트 ①-B).
    ///
    /// `control_leases` 락만 잡고 값을 복사해 나온다 — 호출부가 자기 락을
    /// 잡기 **전에** 부르기 위해서다. 이유는
    /// [`compare_and_set_task_status`](Store::compare_and_set_task_status)의
    /// 주석과 같다: 락을 중첩시키지 않고, fenced를 다른 어떤 판정보다 먼저
    /// 낸다.
    fn control_fence_holds(&self, fence: Option<&ControlFence>) -> bool {
        let Some(f) = fence else {
            return true;
        };
        let held = self
            .control_leases
            .lock()
            .unwrap()
            .get(&f.cluster_id)
            .map(|l| l.epoch);
        held == Some(f.epoch)
    }

    /// 워커를 직접 주입 (빌더 스타일).
    pub fn with_worker(self, w: Worker) -> Self {
        self.workers.lock().unwrap().insert(w.id, w);
        self
    }

    /// 작업을 직접 주입 (빌더 스타일).
    pub fn with_task(self, t: Task) -> Self {
        self.tasks.lock().unwrap().insert(t.id, t);
        self
    }

    /// 호스트를 직접 주입 (빌더 스타일).
    pub fn with_host(self, h: Host) -> Self {
        self.hosts.lock().unwrap().push(h);
        self
    }

    /// 사용자에게 권한을 직접 부여 (role/permission 테이블을 거치지 않는
    /// 지름길). `list_user_permissions`가 역할 경유 계산과 합쳐서 반환한다.
    pub fn seed_permissions(&self, user_id: UserId, perms: Vec<Permission>) {
        self.direct_user_permissions
            .lock()
            .unwrap()
            .insert(user_id, perms);
    }

    /// `delete_old_login_attempts`에 마지막으로 전달된 cutoff 시각 (테스트
    /// 검증용 — 호출 여부/인자 확인).
    pub fn last_delete_old_login_attempts_cutoff(&self) -> Option<DateTime<Utc>> {
        *self.last_delete_old_login_attempts_cutoff.lock().unwrap()
    }
}

#[async_trait]
impl Store for MemStore {
    // ── Task ───────────────────────────────────────────────────────────

    async fn insert_task(&self, t: &Task) -> Result<(), StoreError> {
        let mut tasks = self.tasks.lock().unwrap();
        if tasks.contains_key(&t.id) {
            return Err(StoreError::Conflict(format!(
                "task already exists: {}",
                t.id
            )));
        }
        tasks.insert(t.id, t.clone());
        Ok(())
    }

    async fn insert_task_idempotent(&self, t: &Task) -> Result<IdempotentInsert, StoreError> {
        let mut tasks = self.tasks.lock().unwrap();
        if tasks.contains_key(&t.id) {
            return Err(StoreError::Conflict(format!(
                "task already exists: {}",
                t.id
            )));
        }

        let Some(key) = t.idempotency_key.as_deref() else {
            // 키가 없으면 유일성 대상이 아니다. PG의 부분 유일 인덱스가
            // `WHERE idempotency_key IS NOT NULL`인 것과 같은 규칙 — NULL 두 개를
            // 같은 값으로 보는 구현을 만들면 여기서만 통과하는 코드가 생긴다.
            tasks.insert(t.id, t.clone());
            return Ok(IdempotentInsert::Inserted);
        };

        // PG는 유일 인덱스가, 여기서는 하나의 락이 검사와 쓰기를 원자적으로
        // 묶는다. 선형 탐색은 O(n)이지만 이 구현은 테스트 전용이고, 별도 인덱스를
        // 두면 `with_task` 같은 직접 주입 헬퍼가 인덱스를 우회해 두 자료구조가
        // 갈라진다 — 그쪽이 훨씬 비싼 버그다.
        let existing = tasks
            .values()
            .find(|task| {
                task.created_by == t.created_by && task.idempotency_key.as_deref() == Some(key)
            })
            .cloned();

        match existing {
            None => {
                tasks.insert(t.id, t.clone());
                Ok(IdempotentInsert::Inserted)
            }
            Some(existing) if existing.idempotency_payload_hash == t.idempotency_payload_hash => {
                Ok(IdempotentInsert::Duplicate(Box::new(existing)))
            }
            Some(existing) => Ok(IdempotentInsert::Conflict {
                existing_task_id: existing.id,
            }),
        }
    }

    async fn get_task(&self, id: TaskId) -> Result<Option<Task>, StoreError> {
        Ok(self.tasks.lock().unwrap().get(&id).cloned())
    }

    async fn update_task_status(&self, id: TaskId, status: &TaskStatus) -> Result<(), StoreError> {
        let mut tasks = self.tasks.lock().unwrap();
        let Some(task) = tasks.get_mut(&id) else {
            return Err(StoreError::NotFound);
        };
        task.status = status.clone();
        if matches!(status, TaskStatus::Dispatched { .. }) {
            task.dispatched_at = Some(Utc::now());
        }
        Ok(())
    }

    async fn compare_and_set_task_status(
        &self,
        id: TaskId,
        expected: &[TaskPhase],
        new: &TaskStatus,
        fence: Option<&ControlFence>,
        origin: TransitionOrigin,
    ) -> Result<TransitionOutcome, StoreError> {
        debug_assert!(
            !expected.is_empty(),
            "compare_and_set_task_status: expected가 비어 있으면 어떤 전이도 성립하지 않는다"
        );

        // fence 검사를 tasks 락 **밖에서** 먼저 끝내고 값만 복사해 나온다.
        // 두 락을 중첩시키지 않기 위해서다 — 이 파일의 다른 메서드가 반대
        // 순서로 잡는 날 교착이 생기고, 그건 재현이 어려운 종류의 버그다.
        // Postgres 쪽과 마찬가지로 fenced를 위상 검사보다 먼저 판정한다.
        if let Some(f) = fence {
            let held = self
                .control_leases
                .lock()
                .unwrap()
                .get(&f.cluster_id)
                .map(|l| l.epoch);
            if held != Some(f.epoch) {
                return Ok(TransitionOutcome::Fenced);
            }
        }

        // Postgres 구현과 달리 검사와 쓰기가 같은 락 안에서 끝나므로,
        // `Rejected`가 보고하는 `current`는 거절 시점의 값과 정확히 일치한다.
        // 그럼에도 호출자는 이 값을 제어 흐름에 쓰지 않아야 한다 — 두 백엔드가
        // 다른 보장을 주면 MemStore에서만 통과하는 코드가 생긴다.
        //
        // fence 판정은 이 락 밖에서 이뤄지므로 Postgres의 단일 문장보다 창이
        // 넓다. MemStore는 단일 프로세스 테스트용이라 그 창을 실제로 통과하는
        // 시나리오가 없고, 좁히려면 두 락을 중첩해야 해서 대가가 더 크다.
        let mut tasks = self.tasks.lock().unwrap();
        let Some(task) = tasks.get_mut(&id) else {
            return Err(StoreError::NotFound);
        };

        // dispatch 세대 술어 — Postgres의 `AND (dispatch_control_epoch IS NULL
        // OR dispatch_control_epoch = $5)`에 대응한다. 위상 검사보다 **먼저**
        // 두는 것이 그쪽 구현과 같은 순서다: Postgres는 두 술어가 한 문장에
        // 있어 0행만 보고, 진단은 lease → dispatch epoch → 위상 순으로 한다.
        // 여기서 순서를 뒤집으면 둘 다 어긋난 경우에 두 백엔드가 다른 값을
        // 돌려주고, `both_backends!` 테스트가 그걸 잡아낸다.
        if let (Some(f), TransitionOrigin::WorkerOutcome) = (fence, origin) {
            if let Some(under) = task.dispatch_control_epoch {
                if under != f.epoch {
                    return Ok(TransitionOutcome::StaleDispatchEpoch {
                        dispatched_under: under,
                    });
                }
            }
        }

        let current = task.status.phase();
        if !expected.contains(&current) {
            return Ok(TransitionOutcome::Rejected { current });
        }

        task.status = new.clone();
        if matches!(new, TaskStatus::Dispatched { .. }) {
            task.dispatched_at = Some(Utc::now());
            // Postgres 쪽 SET 절과 같은 조건이다 — Dispatched 전이 x fence 존재.
            // fence가 없을 때 `None`을 대입하지 않고 아예 건드리지 않는 것도
            // 의도적이다. 두 백엔드가 문장 단위로 대응해야 `both_backends!`
            // 테스트가 백엔드 차이를 잡아낼 수 있다.
            if let Some(f) = fence {
                task.dispatch_control_epoch = Some(f.epoch);
            }
        }
        Ok(TransitionOutcome::Applied)
    }

    async fn increment_task_retry_count(&self, id: TaskId) -> Result<u32, StoreError> {
        let mut tasks = self.tasks.lock().unwrap();
        let Some(task) = tasks.get_mut(&id) else {
            return Err(StoreError::NotFound);
        };
        task.retry_count += 1;
        Ok(task.retry_count)
    }

    async fn count_dispatched_tasks_by_worker(&self) -> Result<HashMap<WorkerId, u32>, StoreError> {
        if self.is_failing("count_dispatched_tasks_by_worker") {
            return Err(StoreError::Unsupported("count_dispatched_tasks_by_worker"));
        }
        let tasks = self.tasks.lock().unwrap();
        let mut out: HashMap<WorkerId, u32> = HashMap::new();
        for t in tasks.values() {
            // `Completed`/`Failed`도 `worker_id`를 갖지만 이미 끝난 작업이므로
            // 용량을 차지하지 않는다 — `Dispatched`만 센다. PgStore 쪽의
            // `status_phase = 'dispatched'` 술어와 같은 의미다.
            if let TaskStatus::Dispatched { worker_id, .. } = &t.status {
                *out.entry(*worker_id).or_insert(0) += 1;
            }
        }
        Ok(out)
    }

    async fn update_task_checkpoint(
        &self,
        id: TaskId,
        checkpoint_branch: Option<&str>,
    ) -> Result<(), StoreError> {
        let mut tasks = self.tasks.lock().unwrap();
        let Some(task) = tasks.get_mut(&id) else {
            return Err(StoreError::NotFound);
        };
        task.checkpoint_branch = checkpoint_branch.map(|s| s.to_string());
        Ok(())
    }

    async fn delete_task(&self, id: TaskId) -> Result<TaskDeleteOutcome, StoreError> {
        // 1. 의존자 선검사. PgStore와 동일한 순서로 판정하지만, 이 락과 아래
        //    terminal 판정 락은 별개다 — PgStore의 두 SQL 왕복과 마찬가지로
        //    그 사이에 새 Pending 의존자가 끼어드는 창이 여기도 남아 있다
        //    (`TaskDeleteOutcome` 문서 참고). PgStore와 다른 보장을 주면
        //    MemStore에서만 통과하는 테스트가 생기므로 일부러 좁히지 않는다.
        {
            let tasks = self.tasks.lock().unwrap();
            let dependent_ids: Vec<TaskId> = tasks
                .values()
                .filter(|t| {
                    t.status.phase() == TaskPhase::Pending && t.dependency_ids.contains(&id)
                })
                .map(|t| t.id)
                .collect();
            if !dependent_ids.is_empty() {
                return Ok(TaskDeleteOutcome::BlockedByDependents { dependent_ids });
            }
        }

        // 2. terminal 판정과 제거는 같은 락 안에서 끝낸다 — Postgres와 달리
        //    여기엔 그 둘 사이의 TOCTOU가 없다(단일 Mutex).
        {
            let mut tasks = self.tasks.lock().unwrap();
            let Some(task) = tasks.get(&id) else {
                return Err(StoreError::NotFound);
            };
            let current = task.status.phase();
            if !current.is_terminal() {
                return Ok(TaskDeleteOutcome::NotTerminal { current });
            }
            tasks.remove(&id);
        }

        // task_outputs(001)의 ON DELETE CASCADE 대응.
        self.outputs.lock().unwrap().remove(&id);

        // tasks.parent_task_id(013)의 ON DELETE SET NULL 대응 — thread_id는
        // 건드리지 않는다: 루트가 삭제된 스레드는 도달 가능한 정상 상태다.
        {
            let mut tasks = self.tasks.lock().unwrap();
            for task in tasks.values_mut() {
                if task.parent_task_id == Some(id) {
                    task.parent_task_id = None;
                }
            }
        }

        // issue_task_links.task_id(023)의 ON DELETE SET NULL 대응 —
        // task_label은 이미 별도로 저장돼 있어 여기서 건드릴 것이 없다.
        {
            let mut links = self.issue_task_links.lock().unwrap();
            for link in links.iter_mut() {
                if link.task_id == Some(id) {
                    link.task_id = None;
                }
            }
        }

        // events(001)의 ON DELETE SET NULL 대응은 하지 않는다 — MemStore의
        // `EventEntry`는 `FleetEvent`를 그대로 감쌀 뿐 PgStore의 `events.task_id`
        // 컬럼에 대응하는 별도 필드가 없다. PgStore에서도 그 컬럼은 어떤
        // 조회 경로에서도 읽히지 않으므로(`list_events`는 `payload`만 읽는다),
        // 지울 것이 애초에 없다는 점에서 두 구현은 일치한다.
        // task_telemetry(016)에 대응하는 저장소도 MemStore에 없다 — Store
        // 트레이트에 그 테이블을 다루는 메서드 자체가 없다.
        Ok(TaskDeleteOutcome::Deleted)
    }

    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        if self.is_failing("list_tasks") {
            return Err(StoreError::Unsupported("list_tasks"));
        }
        let tasks = self.tasks.lock().unwrap();
        let mut out: Vec<Task> = tasks
            .values()
            .filter(|t| match &filter.created_by {
                Some(created_by) => &t.created_by == created_by,
                None => true,
            })
            .filter(|t| match filter.worker_id {
                Some(wid) => {
                    matches!(
                        &t.status,
                        TaskStatus::Dispatched { worker_id, .. } if *worker_id == wid
                    ) || matches!(
                        &t.status,
                        TaskStatus::Completed(r) if r.worker_id == wid
                    ) || matches!(
                        &t.status,
                        TaskStatus::Failed(f) if f.worker_id == Some(wid)
                    )
                }
                None => true,
            })
            .filter(|t| match &filter.status {
                Some(TaskStatusFilter::Pending) => matches!(t.status, TaskStatus::Pending),
                Some(TaskStatusFilter::Dispatched) => {
                    matches!(t.status, TaskStatus::Dispatched { .. })
                }
                Some(TaskStatusFilter::Completed) => matches!(t.status, TaskStatus::Completed(_)),
                Some(TaskStatusFilter::Failed) => matches!(t.status, TaskStatus::Failed(_)),
                Some(TaskStatusFilter::Cancelled) => {
                    matches!(t.status, TaskStatus::Cancelled { .. })
                }
                Some(TaskStatusFilter::Terminal) => t.is_terminal(),
                Some(TaskStatusFilter::Active) => !t.is_terminal(),
                None => true,
            })
            .cloned()
            .collect();
        // 실제 PgStore와 동일하게 최신순(내림차순)으로 정렬 후 offset/limit 적용.
        out.sort_by_key(|t| std::cmp::Reverse(t.created_at));
        let start = filter.offset.min(out.len());
        let end = (start + filter.limit).min(out.len());
        Ok(out[start..end].to_vec())
    }

    // ── Worker ─────────────────────────────────────────────────────────

    async fn upsert_worker(&self, w: &Worker) -> Result<(), StoreError> {
        let mut workers = self.workers.lock().unwrap();
        let mut next = w.clone();
        if let Some(prev) = workers.get(&w.id) {
            // PgStore의 `ON CONFLICT DO UPDATE`가 이 두 컬럼을 갱신 목록에서
            // 제외하므로 여기서도 기존 값을 보존한다. 통짜 insert로 두면
            // heartbeat 한 번에 `incarnation_started_at`이 흔들려 진행 중인
            // 작업이 전부 고아로 판정되고, 그 차이가 MemStore를 쓰는
            // 테스트에서만 보이지 않는다.
            next.registered_at = prev.registered_at;
            next.incarnation_started_at = prev.incarnation_started_at;
        }
        workers.insert(w.id, next);
        Ok(())
    }

    async fn bump_worker_incarnation(
        &self,
        id: WorkerId,
    ) -> Result<Option<DateTime<Utc>>, StoreError> {
        let mut workers = self.workers.lock().unwrap();
        let Some(w) = workers.get_mut(&id) else {
            return Ok(None);
        };
        let now = Utc::now();
        w.incarnation_started_at = now;
        Ok(Some(now))
    }

    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
        Ok(self.workers.lock().unwrap().get(&id).cloned())
    }

    async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError> {
        Ok(self
            .workers
            .lock()
            .unwrap()
            .values()
            .find(|w| w.name == name)
            .cloned())
    }

    async fn list_workers(&self, filter: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
        let workers = self.workers.lock().unwrap();
        let mut out: Vec<Worker> = workers
            .values()
            .filter(|w| filter.status.is_none_or(|s| w.status == s))
            .filter(|w| {
                filter
                    .labels
                    .iter()
                    .all(|(k, v)| w.labels.get(k) == Some(v))
            })
            .cloned()
            .collect();
        // 실제 PgStore와 동일하게 최신 등록순(내림차순)으로 정렬 후 offset/limit 적용.
        out.sort_by_key(|w| std::cmp::Reverse(w.registered_at));
        let start = filter.offset.min(out.len());
        let end = (start + filter.limit).min(out.len());
        Ok(out[start..end].to_vec())
    }

    /// PgStore와 동작을 일치시킨다 (로드맵 #78).
    ///
    /// PostgreSQL에서는 `DELETE FROM workers`가 두 개의 `ON DELETE CASCADE`를
    /// 함께 발동시킨다 — `worker_operational_credentials`(018, `worker_id` 기준)와
    /// `worker_credentials`(005, `worker_name` 기준, 암호화된 LLM 키). MemStore가
    /// worker row만 지우면 두 저장소의 관측 가능한 동작이 갈라지고, 실제로 그
    /// 차이 때문에 "정상 종료가 워커 신원과 LLM credential을 파괴한다"는 결함이
    /// 인메모리 테스트를 전부 통과했다. 존재하지 않는 id에 대한 `NotFound`도
    /// PgStore(`rows_affected() == 0`)와 맞춘다.
    async fn delete_worker(&self, id: WorkerId) -> Result<(), StoreError> {
        let removed = self.workers.lock().unwrap().remove(&id);
        let Some(worker) = removed else {
            return Err(StoreError::NotFound);
        };

        // CASCADE 1: worker_operational_credentials (worker_id 기준).
        self.worker_operational_credentials
            .lock()
            .unwrap()
            .retain(|_digest, cred| cred.worker_id != id);

        // CASCADE 2: worker_credentials (worker_name 기준).
        self.credentials
            .lock()
            .unwrap()
            .retain(|(name, _model), _| name != &worker.name);

        // CASCADE 3: agents의 배치와 관측 (030의 FK `ON DELETE SET NULL` +
        // 036의 트리거). 이것이 없으면 `agents.worker_id`가 지워진 Worker를
        // 계속 가리키고, 게이트 ②의 술어가 stale한 `running`에 막혀 그
        // Agent를 **영구히 옮길 수 없게** 된다 — 036이 없애려는 바로 그
        // 상태이며, PgStore에서는 일어나지 않는다.
        //
        // 다섯 필드를 함께 비우는 이유는 짝 맞춤 CHECK 둘 때문이다:
        // 030의 `agents_placement_complete`(worker_id/assigned_at)와 032의
        // `agents_observation_complete`(관측 세 컬럼). 절반만 지우면 PgStore
        // 쪽에서는 애초에 쓸 수 없는 행이 된다.
        for agent in self.agents.lock().unwrap().values_mut() {
            if agent.worker_id == Some(id) {
                agent.worker_id = None;
                agent.assigned_at = None;
                agent.observed_status = None;
                agent.observed_at = None;
                agent.observed_reason = None;
            }
        }

        Ok(())
    }

    async fn update_worker_heartbeat(
        &self,
        id: WorkerId,
        heartbeat: &WorkerHeartbeat,
    ) -> Result<(), StoreError> {
        let mut workers = self.workers.lock().unwrap();
        let Some(w) = workers.get_mut(&id) else {
            return Err(StoreError::NotFound);
        };
        w.active_tasks = heartbeat.active_tasks;
        w.last_seen = Some(Utc::now());
        Ok(())
    }

    // ── Event log ──────────────────────────────────────────────────────

    async fn append_event(&self, e: &FleetEvent) -> Result<u64, StoreError> {
        let mut events = self.events.lock().unwrap();
        let seq = (events.len() + 1) as u64;
        events.push(EventEntry {
            seq,
            event: e.clone(),
        });
        Ok(seq)
    }

    async fn list_events(&self, after_seq: u64, limit: u32) -> Result<Vec<EventEntry>, StoreError> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.seq > after_seq)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    // ── Output buffer ──────────────────────────────────────────────────

    async fn append_output(&self, task_id: TaskId, chunk: &str) -> Result<u64, StoreError> {
        let mut outputs = self.outputs.lock().unwrap();
        let entry = outputs.entry(task_id).or_default();
        entry.push(chunk.to_string());
        Ok(entry.len() as u64)
    }

    async fn get_output(&self, task_id: TaskId, after_seq: u64) -> Result<TaskOutput, StoreError> {
        let outputs = self.outputs.lock().unwrap();
        let chunks: Vec<_> = outputs
            .get(&task_id)
            .map(|v| {
                v.iter()
                    .skip(after_seq as usize)
                    .enumerate()
                    .map(|(i, chunk)| TaskOutputChunk {
                        task_id,
                        seq: after_seq + i as u64,
                        chunk: chunk.clone(),
                        written_at: Utc::now(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let next_offset = after_seq + chunks.len() as u64;
        Ok(TaskOutput {
            task_id,
            chunks,
            next_offset,
        })
    }

    // ── Migration ──────────────────────────────────────────────────────

    async fn migrate(&self) -> Result<(), StoreError> {
        Ok(())
    }

    // ── Bootstrap tokens ───────────────────────────────────────────────

    async fn create_bootstrap_token(&self, token: &BootstrapToken) -> Result<(), StoreError> {
        let mut tokens = self.bootstrap_tokens.lock().unwrap();
        if tokens.contains_key(&token.token_digest) {
            return Err(StoreError::Conflict(format!(
                "bootstrap token already exists: {}",
                token.public_id()
            )));
        }
        tokens.insert(token.token_digest.clone(), token.clone());
        Ok(())
    }

    async fn consume_bootstrap_token(&self, token: &str, used_by: &str) -> Result<(), StoreError> {
        let mut tokens = self.bootstrap_tokens.lock().unwrap();
        let digest = BootstrapToken::digest_for(token);
        let entry = tokens
            .get_mut(&digest)
            .ok_or_else(|| StoreError::BootstrapTokenInvalid("token not found".into()))?;
        if !entry.is_usable() {
            let reason = if entry.use_count >= entry.max_uses {
                "exhausted"
            } else {
                "expired"
            };
            return Err(StoreError::BootstrapTokenInvalid(format!("token {reason}")));
        }
        entry.use_count += 1;
        entry.last_used_by = Some(used_by.to_string());
        entry.last_used_at = Some(Utc::now());
        Ok(())
    }

    async fn list_bootstrap_tokens(&self) -> Result<Vec<BootstrapToken>, StoreError> {
        let mut all: Vec<BootstrapToken> = self
            .bootstrap_tokens
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect();
        all.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        Ok(all)
    }

    async fn revoke_bootstrap_token(&self, token_digest: &str) -> Result<bool, StoreError> {
        Ok(self
            .bootstrap_tokens
            .lock()
            .unwrap()
            .remove(token_digest)
            .is_some())
    }

    // ── Admin API tokens (로드맵 #72) ────────────────────────────────

    async fn create_admin_token(&self, token: &AdminApiToken) -> Result<(), StoreError> {
        let mut tokens = self.admin_api_tokens.lock().unwrap();
        if tokens.contains_key(&token.principal_id) {
            return Err(StoreError::Conflict(format!(
                "admin token principal already exists: {}",
                token.principal_id
            )));
        }
        if tokens
            .values()
            .any(|t| t.token_digest == token.token_digest)
        {
            return Err(StoreError::Conflict(
                "admin token digest already exists".into(),
            ));
        }
        tokens.insert(token.principal_id.clone(), token.clone());
        Ok(())
    }

    async fn find_active_admin_token_by_digest(
        &self,
        token_digest: &str,
    ) -> Result<Option<AdminApiToken>, StoreError> {
        Ok(self
            .admin_api_tokens
            .lock()
            .unwrap()
            .values()
            .find(|t| t.token_digest == token_digest && t.revoked_at.is_none())
            .cloned())
    }

    async fn list_admin_tokens(&self) -> Result<Vec<AdminApiToken>, StoreError> {
        let mut all: Vec<AdminApiToken> = self
            .admin_api_tokens
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect();
        all.sort_by_key(|t| std::cmp::Reverse(t.created_at));
        Ok(all)
    }

    async fn rotate_admin_token(
        &self,
        principal_id: &str,
        new_token_digest: &str,
    ) -> Result<AdminApiToken, StoreError> {
        let mut tokens = self.admin_api_tokens.lock().unwrap();
        let token = tokens.get_mut(principal_id).ok_or(StoreError::NotFound)?;
        token.token_digest = new_token_digest.to_string();
        token.rotated_at = Some(Utc::now());
        token.revoked_at = None;
        token.rotation_generation += 1;
        Ok(token.clone())
    }

    async fn revoke_admin_token(&self, principal_id: &str) -> Result<bool, StoreError> {
        let mut tokens = self.admin_api_tokens.lock().unwrap();
        match tokens.get_mut(principal_id) {
            Some(token) if token.revoked_at.is_none() => {
                token.revoked_at = Some(Utc::now());
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    async fn upsert_worker_operational_credential(
        &self,
        credential: &WorkerOperationalCredential,
    ) -> Result<(), StoreError> {
        self.worker_operational_credentials
            .lock()
            .unwrap()
            .insert(credential.credential_digest.clone(), credential.clone());
        Ok(())
    }

    async fn find_active_worker_operational_credential(
        &self,
        credential_digest: &str,
    ) -> Result<Option<WorkerOperationalCredential>, StoreError> {
        Ok(self
            .worker_operational_credentials
            .lock()
            .unwrap()
            .get(credential_digest)
            .filter(|credential| {
                credential.revoked_at.is_none()
                    && credential
                        .expires_at
                        .is_none_or(|expires_at| expires_at > Utc::now())
            })
            .cloned())
    }

    async fn revoke_worker_operational_credential(
        &self,
        worker_id: WorkerId,
    ) -> Result<bool, StoreError> {
        let mut credentials = self.worker_operational_credentials.lock().unwrap();
        let entry = credentials
            .values_mut()
            .find(|c| c.worker_id == worker_id && c.revoked_at.is_none());
        match entry {
            Some(credential) => {
                credential.revoked_at = Some(Utc::now());
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn rotate_worker_operational_credential(
        &self,
        worker_id: WorkerId,
        new_credential_digest: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<WorkerOperationalCredential, StoreError> {
        let mut credentials = self.worker_operational_credentials.lock().unwrap();
        let old_digest = credentials
            .values()
            .find(|c| c.worker_id == worker_id)
            .map(|c| c.credential_digest.clone())
            .ok_or(StoreError::NotFound)?;
        let mut credential = credentials.remove(&old_digest).expect("checked above");
        credential.credential_digest = new_credential_digest.to_string();
        credential.issued_at = Utc::now();
        credential.expires_at = expires_at;
        credential.revoked_at = None;
        credential.rotation_generation += 1;
        credentials.insert(new_credential_digest.to_string(), credential.clone());
        Ok(credential)
    }

    /// bootstrap 토큰 소비 + worker 생성 + operational credential 저장을 하나의
    /// 임계 구역으로 묶는다. `bootstrap_tokens`/`workers`/`worker_operational_credentials`
    /// 세 `Mutex`를 모두 이 스코프 동안 보유해, 검사와 반영 사이에 다른 호출
    /// (`get_worker_by_name` 등)이 중간 상태를 관찰하지 못하게 한다 — 실패 시
    /// 어떤 락도 mutate되지 않은 채로 반환되므로 all-or-nothing이 성립한다.
    async fn enroll_worker(
        &self,
        bootstrap_token: &str,
        used_by: &str,
        worker: &Worker,
        credential: &WorkerOperationalCredential,
    ) -> Result<(), StoreError> {
        let mut tokens = self.bootstrap_tokens.lock().unwrap();
        let mut workers = self.workers.lock().unwrap();
        let mut credentials = self.worker_operational_credentials.lock().unwrap();

        // 1. bootstrap 토큰 검증 (아직 반영하지 않음 — 뒤 단계가 실패하면 그대로 둔다).
        let digest = BootstrapToken::digest_for(bootstrap_token);
        let entry = tokens
            .get(&digest)
            .ok_or_else(|| StoreError::BootstrapTokenInvalid("token not found".into()))?;
        if !entry.is_usable() {
            let reason = if entry.use_count >= entry.max_uses {
                "exhausted"
            } else {
                "expired"
            };
            return Err(StoreError::BootstrapTokenInvalid(format!("token {reason}")));
        }

        // 2. worker 이름 충돌 검사 (upsert_worker는 id 기준이라 이 검사가 없으면
        //    동일 이름의 워커가 여러 id로 중복 생성될 수 있다).
        if workers.values().any(|w| w.name == worker.name) {
            return Err(StoreError::Conflict(format!(
                "worker name already exists: {}",
                worker.name
            )));
        }

        // 3. credential digest 충돌 검사 (postgres의 UNIQUE(credential_digest)와 동등).
        if credentials
            .values()
            .any(|c| c.credential_digest == credential.credential_digest)
        {
            return Err(StoreError::Conflict(
                "credential digest already exists".into(),
            ));
        }

        // 모든 검사를 통과했을 때만 세 상태를 함께 반영한다.
        let token_entry = tokens.get_mut(&digest).expect("checked usable above");
        token_entry.use_count += 1;
        token_entry.last_used_by = Some(used_by.to_string());
        token_entry.last_used_at = Some(Utc::now());

        workers.insert(worker.id, worker.clone());
        credentials.insert(credential.credential_digest.clone(), credential.clone());

        Ok(())
    }

    // ── RBAC: Users ────────────────────────────────────────────────────

    async fn create_user(&self, user: &User) -> Result<(), StoreError> {
        let mut users = self.users.lock().unwrap();
        if users.values().any(|u| u.username == user.username) {
            return Err(StoreError::Conflict(format!(
                "username already exists: {}",
                user.username
            )));
        }
        users.insert(user.id, user.clone());
        Ok(())
    }

    async fn get_user_by_id(&self, id: UserId) -> Result<Option<User>, StoreError> {
        Ok(self.users.lock().unwrap().get(&id).cloned())
    }

    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>, StoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .values()
            .find(|u| u.username == username)
            .cloned())
    }

    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, StoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .values()
            .find(|u| u.email.as_deref() == Some(email))
            .cloned())
    }

    async fn list_users(&self) -> Result<Vec<User>, StoreError> {
        let mut all: Vec<User> = self.users.lock().unwrap().values().cloned().collect();
        all.sort_by_key(|u| u.created_at);
        Ok(all)
    }

    async fn count_users(&self) -> Result<u64, StoreError> {
        Ok(self.users.lock().unwrap().len() as u64)
    }

    async fn update_user_password(&self, id: UserId, hash: &str) -> Result<(), StoreError> {
        let mut users = self.users.lock().unwrap();
        let Some(u) = users.get_mut(&id) else {
            return Err(StoreError::NotFound);
        };
        u.password_hash = hash.to_string();
        Ok(())
    }

    async fn update_user_last_login(
        &self,
        id: UserId,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut users = self.users.lock().unwrap();
        let Some(u) = users.get_mut(&id) else {
            return Err(StoreError::NotFound);
        };
        u.last_login_at = Some(at);
        Ok(())
    }

    async fn set_user_enabled(&self, id: UserId, enabled: bool) -> Result<(), StoreError> {
        let mut users = self.users.lock().unwrap();
        let Some(u) = users.get_mut(&id) else {
            return Err(StoreError::NotFound);
        };
        u.enabled = enabled;
        Ok(())
    }

    async fn delete_user(&self, id: UserId) -> Result<(), StoreError> {
        self.users.lock().unwrap().remove(&id);
        self.user_roles.lock().unwrap().remove(&id);
        self.direct_user_permissions.lock().unwrap().remove(&id);
        self.sessions.lock().unwrap().retain(|_, s| s.user_id != id);
        Ok(())
    }

    // ── RBAC: Roles & Permissions ──────────────────────────────────────

    async fn create_role(&self, role: &Role) -> Result<(), StoreError> {
        let mut roles = self.roles.lock().unwrap();
        if roles.values().any(|r| r.name == role.name) {
            return Err(StoreError::Conflict(format!(
                "role already exists: {}",
                role.name
            )));
        }
        roles.insert(role.id, role.clone());
        Ok(())
    }

    async fn get_role_by_name(&self, name: &str) -> Result<Option<Role>, StoreError> {
        Ok(self
            .roles
            .lock()
            .unwrap()
            .values()
            .find(|r| r.name == name)
            .cloned())
    }

    async fn list_roles(&self) -> Result<Vec<Role>, StoreError> {
        Ok(self.roles.lock().unwrap().values().cloned().collect())
    }

    async fn create_permission(&self, perm: &Permission) -> Result<(), StoreError> {
        let mut perms = self.permissions.lock().unwrap();
        if perms.values().any(|p| p.name == perm.name) {
            return Ok(()); // idempotent
        }
        perms.insert(perm.id, perm.clone());
        Ok(())
    }

    async fn get_permission_by_name(&self, name: &str) -> Result<Option<Permission>, StoreError> {
        Ok(self
            .permissions
            .lock()
            .unwrap()
            .values()
            .find(|p| p.name == name)
            .cloned())
    }

    async fn list_permissions(&self) -> Result<Vec<Permission>, StoreError> {
        Ok(self.permissions.lock().unwrap().values().cloned().collect())
    }

    async fn assign_user_role(
        &self,
        user_id: UserId,
        role_id: RoleId,
        _granted_by: Option<UserId>,
    ) -> Result<(), StoreError> {
        let mut ur = self.user_roles.lock().unwrap();
        let entry = ur.entry(user_id).or_default();
        if !entry.contains(&role_id) {
            entry.push(role_id);
        }
        Ok(())
    }

    async fn revoke_user_role(&self, user_id: UserId, role_id: RoleId) -> Result<(), StoreError> {
        if let Some(entry) = self.user_roles.lock().unwrap().get_mut(&user_id) {
            entry.retain(|r| *r != role_id);
        }
        Ok(())
    }

    async fn list_user_roles(&self, user_id: UserId) -> Result<Vec<Role>, StoreError> {
        let role_ids = self
            .user_roles
            .lock()
            .unwrap()
            .get(&user_id)
            .cloned()
            .unwrap_or_default();
        let roles = self.roles.lock().unwrap();
        Ok(role_ids
            .iter()
            .filter_map(|id| roles.get(id).cloned())
            .collect())
    }

    async fn list_user_permissions(&self, user_id: UserId) -> Result<Vec<Permission>, StoreError> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();

        // 직접 주입된 권한 (테스트 편의 — seed_permissions).
        if let Some(direct) = self.direct_user_permissions.lock().unwrap().get(&user_id) {
            for p in direct {
                if seen.insert(p.id) {
                    result.push(p.clone());
                }
            }
        }

        // 역할 경유 계산 (실제 create_role/assign_user_role/grant_role_permission 경로).
        let role_ids = self
            .user_roles
            .lock()
            .unwrap()
            .get(&user_id)
            .cloned()
            .unwrap_or_default();
        let role_permissions = self.role_permissions.lock().unwrap();
        let permissions = self.permissions.lock().unwrap();
        for rid in role_ids {
            if let Some(pids) = role_permissions.get(&rid) {
                for pid in pids {
                    if let Some(p) = permissions.get(pid) {
                        if seen.insert(p.id) {
                            result.push(p.clone());
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    async fn grant_role_permission(
        &self,
        role_id: RoleId,
        permission_id: PermissionId,
    ) -> Result<(), StoreError> {
        let mut rp = self.role_permissions.lock().unwrap();
        let entry = rp.entry(role_id).or_default();
        if !entry.contains(&permission_id) {
            entry.push(permission_id);
        }
        Ok(())
    }

    // ── Sessions ───────────────────────────────────────────────────────

    async fn create_session(&self, session: &Session) -> Result<(), StoreError> {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.token_hash.clone(), session.clone());
        Ok(())
    }

    async fn get_session_by_token_hash(&self, hash: &str) -> Result<Option<Session>, StoreError> {
        Ok(self.sessions.lock().unwrap().get(hash).cloned())
    }

    async fn delete_session(&self, id: SessionId) -> Result<(), StoreError> {
        self.sessions.lock().unwrap().retain(|_, s| s.id != id);
        Ok(())
    }

    async fn update_session_expiry(
        &self,
        id: SessionId,
        expires_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(s) = sessions.values_mut().find(|s| s.id == id) {
            s.expires_at = expires_at;
        }
        Ok(())
    }

    async fn delete_expired_sessions(&self) -> Result<u64, StoreError> {
        if self.is_failing("delete_expired_sessions") {
            return Err(StoreError::Unsupported("delete_expired_sessions"));
        }
        let mut sessions = self.sessions.lock().unwrap();
        let now = Utc::now();
        let before = sessions.len();
        sessions.retain(|_, s| s.expires_at > now);
        Ok((before - sessions.len()) as u64)
    }

    async fn delete_user_sessions(&self, user_id: UserId) -> Result<u64, StoreError> {
        let mut sessions = self.sessions.lock().unwrap();
        let before = sessions.len();
        sessions.retain(|_, s| s.user_id != user_id);
        Ok((before - sessions.len()) as u64)
    }

    // ── Email verification ─────────────────────────────────────────────

    async fn create_email_verification_token(
        &self,
        token: &EmailVerificationToken,
    ) -> Result<(), StoreError> {
        self.email_verification_tokens
            .lock()
            .unwrap()
            .insert(token.id, token.clone());
        Ok(())
    }

    async fn get_email_verification_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<EmailVerificationToken>, StoreError> {
        Ok(self
            .email_verification_tokens
            .lock()
            .unwrap()
            .values()
            .find(|t| t.token_hash == token_hash)
            .cloned())
    }

    async fn consume_email_verification_token(
        &self,
        token_id: Uuid,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut tokens = self.email_verification_tokens.lock().unwrap();
        if let Some(t) = tokens.get_mut(&token_id) {
            t.consumed_at = Some(at);
        }
        Ok(())
    }

    async fn set_user_email_verified(
        &self,
        user_id: UserId,
        verified: bool,
    ) -> Result<(), StoreError> {
        let mut users = self.users.lock().unwrap();
        let Some(u) = users.get_mut(&user_id) else {
            return Err(StoreError::NotFound);
        };
        u.email_verified = verified;
        Ok(())
    }

    // ── Password reset ─────────────────────────────────────────────────

    async fn create_password_reset_token(
        &self,
        token: &EmailVerificationToken,
    ) -> Result<(), StoreError> {
        self.password_reset_tokens
            .lock()
            .unwrap()
            .insert(token.id, token.clone());
        Ok(())
    }

    async fn get_password_reset_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<EmailVerificationToken>, StoreError> {
        Ok(self
            .password_reset_tokens
            .lock()
            .unwrap()
            .values()
            .find(|t| t.token_hash == token_hash)
            .cloned())
    }

    async fn consume_password_reset_token(
        &self,
        token_id: Uuid,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut tokens = self.password_reset_tokens.lock().unwrap();
        if let Some(t) = tokens.get_mut(&token_id) {
            t.consumed_at = Some(at);
        }
        Ok(())
    }

    // ── Login attempts ─────────────────────────────────────────────────

    async fn record_login_attempt(&self, attempt: &LoginAttempt) -> Result<(), StoreError> {
        self.login_attempts.lock().unwrap().push(attempt.clone());
        Ok(())
    }

    async fn count_recent_failed_attempts(
        &self,
        identifier: &str,
        ip: Option<&str>,
        window_secs: i64,
    ) -> Result<u64, StoreError> {
        let cutoff = Utc::now() - chrono::Duration::seconds(window_secs);
        let attempts = self.login_attempts.lock().unwrap();
        Ok(attempts
            .iter()
            .filter(|a| a.identifier == identifier)
            .filter(|a| !a.success)
            .filter(|a| a.attempted_at >= cutoff)
            .filter(|a| match ip {
                Some(ip) => a.ip_address.as_deref() == Some(ip),
                None => true,
            })
            .count() as u64)
    }

    async fn count_recent_ip_failures(
        &self,
        ip: &str,
        window_secs: i64,
    ) -> Result<u64, StoreError> {
        let cutoff = Utc::now() - chrono::Duration::seconds(window_secs);
        let attempts = self.login_attempts.lock().unwrap();
        Ok(attempts
            .iter()
            .filter(|a| a.ip_address.as_deref() == Some(ip))
            .filter(|a| !a.success)
            .filter(|a| a.attempted_at >= cutoff)
            .count() as u64)
    }

    async fn clear_login_attempts(
        &self,
        identifier: &str,
        ip: Option<&str>,
    ) -> Result<u64, StoreError> {
        let mut attempts = self.login_attempts.lock().unwrap();
        let before = attempts.len();
        attempts.retain(|a| {
            !(a.identifier == identifier
                && match ip {
                    Some(ip) => a.ip_address.as_deref() == Some(ip),
                    None => true,
                })
        });
        Ok((before - attempts.len()) as u64)
    }

    async fn delete_old_login_attempts(&self, before: DateTime<Utc>) -> Result<u64, StoreError> {
        *self.last_delete_old_login_attempts_cutoff.lock().unwrap() = Some(before);
        if self.is_failing("delete_old_login_attempts") {
            return Err(StoreError::Unsupported("delete_old_login_attempts"));
        }
        let mut attempts = self.login_attempts.lock().unwrap();
        let count_before = attempts.len();
        attempts.retain(|a| a.attempted_at >= before);
        Ok((count_before - attempts.len()) as u64)
    }

    // ── Audit log ──────────────────────────────────────────────────────

    async fn record_audit_event(&self, event: &AuditEvent) -> Result<(), StoreError> {
        if self.is_failing("record_audit_event") {
            return Err(StoreError::Unsupported("record_audit_event"));
        }
        self.audit_events.lock().unwrap().push(event.clone());
        Ok(())
    }

    async fn list_audit_events(&self, filter: &AuditFilter) -> Result<Vec<AuditEvent>, StoreError> {
        let events = self.audit_events.lock().unwrap();
        let mut out: Vec<AuditEvent> = events
            .iter()
            .filter(|e| match filter.actor_user_id {
                Some(uid) => e.actor_user_id == Some(uid),
                None => true,
            })
            .filter(|e| match &filter.action {
                Some(action) => &e.action == action,
                None => true,
            })
            .filter(|e| match filter.project_id {
                Some(pid) => e.project_id == Some(pid),
                None => true,
            })
            .cloned()
            .collect();
        out.sort_by_key(|t| std::cmp::Reverse(t.created_at));
        let start = filter.offset.min(out.len());
        let end = (start + filter.limit).min(out.len());
        Ok(out[start..end].to_vec())
    }

    // ── Worker credentials ─────────────────────────────────────────────

    async fn upsert_worker_credential(
        &self,
        worker_name: &str,
        model_id: &str,
        encrypted_blob: &str,
        base_url: &str,
        api_backend: &str,
        context_window: u32,
        model_name: Option<&str>,
    ) -> Result<(), StoreError> {
        let mut creds = self.credentials.lock().unwrap();
        let key = (worker_name.to_string(), model_id.to_string());
        let now = Utc::now();
        let entry = creds.entry(key).or_insert_with(|| StoredCredential {
            worker_name: worker_name.to_string(),
            model_id: model_id.to_string(),
            encrypted_blob: encrypted_blob.to_string(),
            base_url: base_url.to_string(),
            api_backend: api_backend.to_string(),
            context_window,
            model_name: model_name.map(|s| s.to_string()),
            created_at: now,
            rotated_at: now,
        });
        entry.encrypted_blob = encrypted_blob.to_string();
        entry.base_url = base_url.to_string();
        entry.api_backend = api_backend.to_string();
        entry.context_window = context_window;
        entry.model_name = model_name.map(|s| s.to_string());
        entry.rotated_at = now;
        Ok(())
    }

    async fn get_worker_credential(
        &self,
        worker_name: &str,
        model_id: &str,
    ) -> Result<Option<StoredCredential>, StoreError> {
        Ok(self
            .credentials
            .lock()
            .unwrap()
            .get(&(worker_name.to_string(), model_id.to_string()))
            .cloned())
    }

    async fn list_worker_credentials(
        &self,
        worker_name: &str,
    ) -> Result<Vec<StoredCredential>, StoreError> {
        Ok(self
            .credentials
            .lock()
            .unwrap()
            .iter()
            .filter(|((w, _), _)| w == worker_name)
            .map(|(_, v)| v.clone())
            .collect())
    }

    async fn delete_worker_credential(
        &self,
        worker_name: &str,
        model_id: &str,
    ) -> Result<bool, StoreError> {
        Ok(self
            .credentials
            .lock()
            .unwrap()
            .remove(&(worker_name.to_string(), model_id.to_string()))
            .is_some())
    }

    // ── Host inventory ─────────────────────────────────────────────────

    async fn upsert_host(&self, host: &Host) -> Result<(), StoreError> {
        let mut hosts = self.hosts.lock().unwrap();
        if let Some(existing) = hosts.iter_mut().find(|h| h.hostname == host.hostname) {
            *existing = host.clone();
        } else {
            hosts.push(host.clone());
        }
        Ok(())
    }

    async fn get_host_by_hostname(&self, hostname: &str) -> Result<Option<Host>, StoreError> {
        Ok(self
            .hosts
            .lock()
            .unwrap()
            .iter()
            .find(|h| h.hostname == hostname)
            .cloned())
    }

    async fn get_host_by_worker(&self, worker_id: WorkerId) -> Result<Option<Host>, StoreError> {
        Ok(self
            .hosts
            .lock()
            .unwrap()
            .iter()
            .find(|h| h.worker_id == Some(worker_id))
            .cloned())
    }

    async fn list_hosts(&self) -> Result<Vec<Host>, StoreError> {
        let mut all: Vec<Host> = self.hosts.lock().unwrap().clone();
        all.sort_by_key(|h| h.created_at);
        Ok(all)
    }

    async fn append_host_event(&self, event: &HostEvent) -> Result<(), StoreError> {
        self.host_events.lock().unwrap().push(event.clone());
        Ok(())
    }

    async fn list_host_events(
        &self,
        host_id: Uuid,
        limit: u32,
    ) -> Result<Vec<HostEvent>, StoreError> {
        let events = self.host_events.lock().unwrap();
        let mut out: Vec<HostEvent> = events
            .iter()
            .filter(|e| e.host_id == host_id)
            .cloned()
            .collect();
        out.sort_by_key(|t| std::cmp::Reverse(t.created_at));
        out.truncate(limit as usize);
        Ok(out)
    }

    // ── SSH 키 금고 ────────────────────────────────────────────────────

    async fn create_ssh_key(&self, key: &SshKey) -> Result<(), StoreError> {
        let mut keys = self.ssh_keys.lock().unwrap();
        if keys.contains_key(&key.name) {
            return Err(StoreError::Conflict(format!(
                "ssh key already exists: {}",
                key.name
            )));
        }
        keys.insert(key.name.clone(), key.clone());
        Ok(())
    }

    async fn get_ssh_key(&self, name: &str) -> Result<Option<SshKey>, StoreError> {
        Ok(self.ssh_keys.lock().unwrap().get(name).cloned())
    }

    async fn list_ssh_keys(&self) -> Result<Vec<SshKey>, StoreError> {
        let mut all: Vec<SshKey> = self.ssh_keys.lock().unwrap().values().cloned().collect();
        all.sort_by_key(|k| k.created_at);
        Ok(all)
    }

    async fn delete_ssh_key(&self, name: &str) -> Result<bool, StoreError> {
        Ok(self.ssh_keys.lock().unwrap().remove(name).is_some())
    }

    // ── Control plane lease (로드맵 #63, 1단계) ──────────────────────
    //
    // PgStore의 `NOW()` 기반 CAS와 동일한 의미론을 단일 프로세스 안에서
    // `Utc::now()` + `Mutex` lock으로 재현한다 — MemStore는 테스트 전용이라
    // 여러 프로세스 사이의 클럭 스큐 문제가 없다.

    async fn acquire_control_lease(
        &self,
        cluster_id: &str,
        instance_id: &str,
        ttl: std::time::Duration,
    ) -> Result<ControlLease, StoreError> {
        let now = Utc::now();
        let ttl = chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::seconds(30));
        let mut leases = self.control_leases.lock().unwrap();
        let next_epoch = match leases.get(cluster_id) {
            Some(existing) if existing.expires_at > now => {
                return Err(StoreError::Conflict(format!(
                    "control plane lease for cluster '{cluster_id}' is held by another instance"
                )));
            }
            Some(existing) => existing.epoch + 1,
            None => 1,
        };
        let lease = ControlLease {
            cluster_id: cluster_id.to_string(),
            active_instance_id: instance_id.to_string(),
            epoch: next_epoch,
            acquired_at: now,
            expires_at: now + ttl,
            last_renewed_at: now,
        };
        leases.insert(cluster_id.to_string(), lease.clone());
        Ok(lease)
    }

    async fn renew_control_lease(
        &self,
        cluster_id: &str,
        instance_id: &str,
        epoch: i64,
        ttl: std::time::Duration,
    ) -> Result<ControlLease, StoreError> {
        let now = Utc::now();
        let ttl = chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::seconds(30));
        let mut leases = self.control_leases.lock().unwrap();
        let lease = leases
            .get_mut(cluster_id)
            .filter(|l| {
                l.active_instance_id == instance_id && l.epoch == epoch && l.expires_at > now
            })
            .ok_or(StoreError::NotFound)?;
        lease.expires_at = now + ttl;
        lease.last_renewed_at = now;
        Ok(lease.clone())
    }

    async fn release_control_lease(
        &self,
        cluster_id: &str,
        instance_id: &str,
        epoch: i64,
    ) -> Result<bool, StoreError> {
        let now = Utc::now();
        let mut leases = self.control_leases.lock().unwrap();
        match leases.get_mut(cluster_id) {
            Some(l) if l.active_instance_id == instance_id && l.epoch == epoch => {
                l.expires_at = now;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    async fn get_control_lease(
        &self,
        cluster_id: &str,
    ) -> Result<Option<ControlLease>, StoreError> {
        Ok(self.control_leases.lock().unwrap().get(cluster_id).cloned())
    }

    // ── Project (로드맵 #48, 1단계) ───────────────────────────────────

    async fn create_project(&self, project: &Project) -> Result<(), StoreError> {
        let mut projects = self.projects.lock().unwrap();
        if projects.values().any(|p| p.name == project.name) {
            return Err(StoreError::Conflict(format!(
                "project name already exists: {}",
                project.name
            )));
        }
        projects.insert(project.id, project.clone());
        Ok(())
    }

    async fn get_project(&self, id: ProjectId) -> Result<Option<Project>, StoreError> {
        Ok(self.projects.lock().unwrap().get(&id).cloned())
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Option<Project>, StoreError> {
        Ok(self
            .projects
            .lock()
            .unwrap()
            .values()
            .find(|p| p.name == name)
            .cloned())
    }

    async fn list_projects(&self, filter: &ProjectFilter) -> Result<Vec<Project>, StoreError> {
        let projects = self.projects.lock().unwrap();
        let mut out: Vec<Project> = projects
            .values()
            .filter(|p| match filter.status {
                Some(status) => p.status == status,
                None => true,
            })
            .cloned()
            .collect();
        out.sort_by_key(|p| std::cmp::Reverse(p.created_at));
        let limit = filter.limit.max(1);
        Ok(out.into_iter().skip(filter.offset).take(limit).collect())
    }

    async fn update_project_status(
        &self,
        id: ProjectId,
        status: ProjectStatus,
    ) -> Result<bool, StoreError> {
        let mut projects = self.projects.lock().unwrap();
        match projects.get_mut(&id) {
            Some(p) => {
                p.status = status;
                p.updated_at = Utc::now();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn project_has_active_tasks(&self, project_id: ProjectId) -> Result<bool, StoreError> {
        let tasks = self.tasks.lock().unwrap();
        Ok(tasks.values().any(|t| {
            t.project_id == Some(project_id)
                && matches!(
                    t.status,
                    TaskStatus::Pending | TaskStatus::Dispatched { .. }
                )
        }))
    }

    // ── Agent (로드맵 #49, 1단계) ─────────────────────────────────────

    async fn create_agent(&self, agent: &Agent) -> Result<Option<WorkerId>, StoreError> {
        // 상한을 `agents` 잠금 **앞에서** 읽는다. `assign_agent_worker`가
        // `workers → agents` 순서로 잡으므로, 여기서 반대로 잡으면 두
        // 경로가 서로를 기다린다. PgStore에서 잠금 순서를 하나로 유지한
        // 것과 같은 이유이고, 여기서는 Mutex라 위반이 곧 데드락이다.
        let worker_cap: Option<(bool, Option<u32>)> = agent.worker_id.map(|w| {
            let workers = self.workers.lock().unwrap();
            match workers.get(&w) {
                Some(worker) => (true, worker.max_agent_processes),
                None => (false, None),
            }
        });

        let mut agents = self.agents.lock().unwrap();
        // 유일성은 이름 전역이 아니라 `(project_id, name)` — Postgres의
        // UNIQUE 제약과 같은 범위여야 두 Store가 같은 입력에 같은 답을 낸다.
        if agents
            .values()
            .any(|a| a.project_id == agent.project_id && a.name == agent.name)
        {
            return Err(StoreError::Conflict(format!(
                "agent name already exists in this project: {}",
                agent.name
            )));
        }
        // pin 검증은 PgStore와 **같은 판정**이어야 한다 — 두 Store가 같은
        // 입력에 다른 답을 내면 MemStore로 통과한 테스트가 아무것도 증명하지
        // 못한다. 검사 항목: revision이 그 템플릿의 것인지, revoke되지
        // 않았는지, 템플릿이 새 pin을 받는 상태인지.
        if let Some(pin) = agent.template_pin {
            let revisions = self.agent_template_revisions.lock().unwrap();
            let rev = revisions
                .get(&pin.revision_id)
                .filter(|r| r.template_id == pin.template_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "no such template revision for agent pin: template={} revision={}",
                        pin.template_id, pin.revision_id
                    ))
                })?;
            if rev.is_revoked() {
                return Err(StoreError::Conflict(format!(
                    "template revision {} is revoked and cannot be pinned",
                    pin.revision_id
                )));
            }
            let templates = self.agent_templates.lock().unwrap();
            let template = templates.get(&pin.template_id).ok_or_else(|| {
                StoreError::Conflict(format!("no such agent template: {}", pin.template_id))
            })?;
            if !template.status.accepts_new_pins() {
                return Err(StoreError::Conflict(format!(
                    "template {} is {} and does not accept new pins",
                    pin.template_id,
                    template.status.as_str()
                )));
            }
        }

        // 슬롯 선점 (로드맵 `#67` 구현 게이트 ①-A-2). PgStore와 **같은
        // 판정**이어야 한다 — 상한을 PgStore에만 넣으면 MemStore 위의 상위
        // 계층 테스트가 상한 경로를 한 번도 밟지 않은 채 성공 경로로
        // 통과한다. `assign_agent_worker`가 FK에 대해 적어 둔 것과 같은
        // 함정이다.
        let mut placed = agent.worker_id;
        if let Some((found, cap)) = worker_cap {
            if !found {
                placed = None;
            } else if let Some(cap) = cap {
                // 세는 술어는 `count_agents_by_worker`와 같다. 자기 자신은
                // 아직 삽입되지 않았으므로 뺄 것이 없다.
                let n = agents
                    .values()
                    .filter(|a| a.worker_id == agent.worker_id && a.status != AgentStatus::Stopped)
                    .count();
                if n >= cap as usize {
                    placed = None;
                }
            }
        }
        // 테스트 주입(`dropping_placements`)은 실제 판정 **뒤**에 둔다.
        // 앞에 두면 진짜 상한 경로가 이 스위치에 가려져, 상한을 지워도
        // 테스트가 초록으로 통과한다.
        if *self.drop_placements.lock().unwrap() {
            placed = None;
        }

        let mut stored = agent.clone();
        if placed.is_none() {
            // 둘을 함께 떨어뜨려야 `030`의 `agents_placement_complete`와
            // 같은 불변식이 MemStore에서도 성립한다.
            stored.worker_id = None;
            stored.assigned_at = None;
        }
        agents.insert(agent.id, stored);
        Ok(placed)
    }

    async fn get_agent(&self, id: AgentId) -> Result<Option<Agent>, StoreError> {
        Ok(self.agents.lock().unwrap().get(&id).cloned())
    }

    async fn get_agent_by_name(
        &self,
        project_id: ProjectId,
        name: &str,
    ) -> Result<Option<Agent>, StoreError> {
        Ok(self
            .agents
            .lock()
            .unwrap()
            .values()
            .find(|a| a.project_id == project_id && a.name == name)
            .cloned())
    }

    async fn list_agents(&self, filter: &AgentFilter) -> Result<Vec<Agent>, StoreError> {
        let agents = self.agents.lock().unwrap();
        let mut out: Vec<Agent> = agents
            .values()
            .filter(|a| match filter.project_id {
                Some(project_id) => a.project_id == project_id,
                None => true,
            })
            .filter(|a| match filter.status {
                Some(status) => a.status == status,
                None => true,
            })
            .filter(|a| match filter.worker_id {
                Some(worker_id) => a.worker_id == Some(worker_id),
                None => true,
            })
            .cloned()
            .collect();
        out.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        let limit = filter.limit.max(1);
        Ok(out.into_iter().skip(filter.offset).take(limit).collect())
    }

    async fn update_agent_status(
        &self,
        id: AgentId,
        status: AgentStatus,
    ) -> Result<bool, StoreError> {
        let mut agents = self.agents.lock().unwrap();
        match agents.get_mut(&id) {
            Some(a) => {
                a.status = status;
                // PgStore의 CASE 두 개와 같은 의미다(로드맵 #67 4b).
                if status == AgentStatus::Stopped && a.desired_status != AgentDesiredStatus::Stopped
                {
                    a.desired_status = AgentDesiredStatus::Stopped;
                    a.command_generation += 1;
                }
                a.updated_at = Utc::now();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn project_has_live_agents(&self, project_id: ProjectId) -> Result<bool, StoreError> {
        let agents = self.agents.lock().unwrap();
        Ok(agents
            .values()
            .any(|a| a.project_id == project_id && a.status.blocks_project_archive()))
    }

    async fn assign_agent_worker(
        &self,
        id: AgentId,
        worker_id: WorkerId,
        fence: Option<&ControlFence>,
    ) -> Result<SlotClaim, StoreError> {
        // fenced를 존재·상한 판정보다 **먼저** 낸다. PgStore와 같은 순서이고
        // 이유도 같다 — fenced 인스턴스가 읽은 상태에는 권위가 없으므로,
        // 그 값을 근거로 `NoSuchAgent`/`NoSuchWorker`/`CapReached`를
        // 돌려주면 호출자가 "나는 제어 기관이었고 이 요청만 문제였다"고
        // 읽는다.
        if !self.control_fence_holds(fence) {
            return Ok(SlotClaim::Fenced);
        }

        // PgStore에서는 잠금 SELECT가 하는 일이다(예전에는 `agents.worker_id`
        // FK였다). 여기서 흉내 내지 않으면 "존재하지 않는 Worker 지목"을
        // 다루는 상위 계층 테스트가 MemStore 위에서는 성공 경로를 밟아 버려,
        // 검증하려던 400 경로를 **한 번도 실행하지 않은 채** 통과한다.
        //
        // 판정 순서까지 맞춘다. PgStore는 Agent의 존재를 먼저 확인하므로
        // 둘 다 없을 때의 답은 `NoSuchAgent`(→ 404)이지 `NoSuchWorker`가
        // 아니다. Worker 조회는 미리 해 두고 판정만 Agent를 찾은 뒤로
        // 미루면, 잠금을 중첩하지 않고도 그 순서가 된다 — 그리고 그
        // 순서(`workers → agents`)는 `create_agent`와도 같아야 한다.
        let worker_cap = self
            .workers
            .lock()
            .unwrap()
            .get(&worker_id)
            .map(|w| w.max_agent_processes);
        let mut agents = self.agents.lock().unwrap();
        if !agents.contains_key(&id) {
            return Ok(SlotClaim::NoSuchAgent);
        }
        let Some(cap) = worker_cap else {
            return Ok(SlotClaim::NoSuchWorker);
        };
        if let Some(cap) = cap {
            // 자기 자신을 뺀다 — 이미 그 Worker에 있는 Agent를 같은
            // Worker로 다시 배정하는 것은 슬롯을 더 쓰지 않는다.
            let n = agents
                .values()
                .filter(|a| {
                    a.worker_id == Some(worker_id) && a.status != AgentStatus::Stopped && a.id != id
                })
                .count();
            if n >= cap as usize {
                return Ok(SlotClaim::CapReached);
            }
        }
        // 게이트 ②: **다른** Worker가 돌리고 있다고 보고한 Agent는 옮기지
        // 않는다 (로드맵 #67).
        //
        // **cap 판정보다 뒤에 둔다.** PgStore에서는 cap이 사전 검사이고 관측은
        // UPDATE 안의 술어라, 둘 다 걸릴 상황에서 나오는 값이 `CapReached`다.
        // 여기서 순서를 뒤집으면 같은 입력에 두 저장소가 다른 답을 내고, 그
        // 차이는 저장소를 바꿔 끼우는 호출부 테스트가 어느 쪽을 골랐느냐에
        // 따라서만 드러난다.
        {
            let a = agents.get(&id).expect("checked above");
            if a.observed_status == Some(AgentObservedStatus::Running)
                && a.worker_id != Some(worker_id)
            {
                return Ok(SlotClaim::ObservedRunning);
            }
        }
        // 위에서 존재를 확인했고 그동안 잠금을 놓지 않았다.
        let a = agents.get_mut(&id).expect("checked above");
        let now = Utc::now();
        let moved = a.worker_id != Some(worker_id);
        a.worker_id = Some(worker_id);
        a.assigned_at = Some(now);
        // 배치가 **실제로 옮겨갔을 때만** 관측을 비운다. 관측은 "어떤 Worker가
        // 이 프로세스에 대해 한 말"이라, 다른 Worker로 옮기는 순간 새 자리에
        // 대해서는 거짓이 된다 — 옛 Worker의 `failed`가 따라오면 새 Worker가
        // 시도조차 하지 않았는데 실패한 것으로 보인다.
        //
        // 같은 Worker로의 재배정에서는 **남긴다.** 프로세스가 움직이지 않았으니
        // 그 말은 여전히 참이고, 여기서 지우면 "같은 Worker로 한 번 재배정해
        // 관측을 없앤 뒤 아무 데로나 옮긴다"는 2단계로 게이트 ②가 뚫린다.
        //
        // 마이그레이션 036의 `agents_clear_placement_and_observation` 트리거와
        // 같은 규칙이다(그쪽은 `NEW.worker_id IS DISTINCT FROM OLD.worker_id`).
        if moved {
            a.observed_status = None;
            a.observed_at = None;
            a.observed_reason = None;
        }
        // 새 Worker는 이전 Worker가 받은 명령을 본 적이 없다
        // (로드맵 #67 4b).
        a.command_generation += 1;
        // 세대를 올린 그 자리에서만 epoch을 찍는다 — 조건이 갈리면
        // "이 명령을 발행한 세대"가 "이 행을 마지막으로 손댄 세대"로
        // 뜻이 바뀐다. PgStore의 CASE와 같은 조건이다.
        a.command_control_epoch = fence.map(|f| f.epoch);
        a.updated_at = now;
        Ok(SlotClaim::Claimed)
    }

    async fn count_agents_by_worker(&self) -> Result<HashMap<WorkerId, u32>, StoreError> {
        let agents = self.agents.lock().unwrap();
        let mut out: HashMap<WorkerId, u32> = HashMap::new();
        for a in agents.values() {
            // PgStore의 `status <> 'stopped'` 술어와 같은 의미다.
            if a.status == AgentStatus::Stopped {
                continue;
            }
            if let Some(worker_id) = a.worker_id {
                *out.entry(worker_id).or_insert(0) += 1;
            }
        }
        Ok(out)
    }

    // ── 수렴 프로토콜 (로드맵 #67 4b) ──────────────────────────────────

    async fn set_agent_desired_status(
        &self,
        id: AgentId,
        desired: AgentDesiredStatus,
        fence: Option<&ControlFence>,
    ) -> Result<CommandIssue, StoreError> {
        // 판정 순서의 근거는 `assign_agent_worker`와 같다.
        if !self.control_fence_holds(fence) {
            return Ok(CommandIssue::Fenced);
        }

        let mut agents = self.agents.lock().unwrap();
        match agents.get_mut(&id) {
            Some(a) => {
                if a.desired_status != desired {
                    a.desired_status = desired;
                    a.command_generation += 1;
                    a.command_control_epoch = fence.map(|f| f.epoch);
                }
                a.updated_at = Utc::now();
                Ok(CommandIssue::Issued)
            }
            None => Ok(CommandIssue::NoSuchAgent),
        }
    }

    async fn list_agent_commands(
        &self,
        worker_id: WorkerId,
    ) -> Result<Vec<AgentCommand>, StoreError> {
        let agents = self.agents.lock().unwrap();
        let mut out: Vec<_> = agents
            .values()
            // PgStore의 술어와 같다 — 회수된 Agent도 그 회수 명령이 확인되기
            // 전까지는 남는다. 자세한 이유는 postgres.rs의 같은 함수 주석.
            .filter(|a| {
                a.worker_id == Some(worker_id)
                    && (a.status != AgentStatus::Stopped
                        || a.last_acked_generation < a.command_generation)
            })
            .collect();
        // PgStore의 `ORDER BY created_at`과 맞춘다. HashMap 순회 순서를 그대로
        // 내보내면 같은 입력에 다른 순서가 나와, 두 Store를 같은 단정으로
        // 검사하는 테스트가 MemStore 위에서만 간헐 실패한다.
        out.sort_by_key(|a| a.created_at);
        Ok(out
            .into_iter()
            .map(|a| AgentCommand {
                agent_id: a.id,
                desired_status: a.desired_status,
                generation: a.command_generation,
            })
            .collect())
    }

    async fn ack_agent_commands(
        &self,
        worker_id: WorkerId,
        acks: &[AgentAck],
    ) -> Result<u64, StoreError> {
        let mut agents = self.agents.lock().unwrap();
        let mut applied = 0u64;
        for ack in acks {
            let Some(a) = agents.get_mut(&ack.agent_id) else {
                continue;
            };
            // PgStore의 세 CAS 조건과 같다.
            if a.worker_id == Some(worker_id)
                && a.command_generation == ack.generation
                && a.last_acked_generation < ack.generation
            {
                a.last_acked_generation = ack.generation;
                // PgStore와 같이 `updated_at`은 건드리지 않는다 — 이유는
                // postgres.rs의 `ack_agent_commands` 주석.
                applied += 1;
            }
        }
        Ok(applied)
    }

    async fn apply_agent_observations(
        &self,
        worker_id: WorkerId,
        observations: &[AgentObservation],
    ) -> Result<u64, StoreError> {
        let spoken: std::collections::HashSet<AgentId> =
            observations.iter().map(|o| o.agent_id()).collect();
        let mut agents = self.agents.lock().unwrap();
        let mut changed = 0u64;

        // PgStore의 1단계 — 이번에 말하지 않은 것은 지운다.
        for a in agents.values_mut() {
            if a.worker_id == Some(worker_id)
                && a.observed_status.is_some()
                && !spoken.contains(&a.id)
            {
                a.observed_status = None;
                a.observed_at = None;
                a.observed_reason = None;
                changed += 1;
            }
        }

        // PgStore의 2단계 — 말한 것을 적는다. `worker_id` 조건도 같다.
        let now = Utc::now();
        for obs in observations {
            let Some(a) = agents.get_mut(&obs.agent_id()) else {
                continue;
            };
            if a.worker_id != Some(worker_id) {
                continue;
            }
            a.observed_status = Some(obs.status());
            a.observed_at = Some(now);
            a.observed_reason = obs.reason();
            // PgStore와 같이 `updated_at`은 건드리지 않는다.
            changed += 1;
        }
        Ok(changed)
    }

    // MemStore는 Worker 삭제 시 배정을 비우지 않는다 — PgStore에서는 `030`의
    // `ON DELETE SET NULL`이 하는 일이라 애플리케이션 코드가 없다. 두 Store의
    // 동작이 여기서 갈리며, 그래서 Worker 삭제 후의 배정 회수는 실제
    // Postgres 통합 테스트로만 증명된다.

    // ── AgentTemplate (로드맵 #86, 1단계) ─────────────────────────────

    async fn create_agent_template(&self, template: &AgentTemplate) -> Result<(), StoreError> {
        let mut templates = self.agent_templates.lock().unwrap();
        // 범위는 `project_id`가 있으면 그 Project, 없으면 전역 — 029의 부분
        // 유니크 인덱스 두 장과 같은 범위여야 두 Store가 같은 답을 낸다.
        // `Option == Option`이 NULL을 같은 값으로 보므로 전역끼리도 걸린다.
        if templates
            .values()
            .any(|t| t.project_id == template.project_id && t.name == template.name)
        {
            return Err(StoreError::Conflict(format!(
                "agent template name already exists in this scope: {}",
                template.name
            )));
        }
        templates.insert(template.id, template.clone());
        Ok(())
    }

    async fn get_agent_template(
        &self,
        id: AgentTemplateId,
    ) -> Result<Option<AgentTemplate>, StoreError> {
        Ok(self.agent_templates.lock().unwrap().get(&id).cloned())
    }

    async fn list_agent_templates(
        &self,
        filter: &AgentTemplateFilter,
    ) -> Result<Vec<AgentTemplate>, StoreError> {
        let templates = self.agent_templates.lock().unwrap();
        let mut out: Vec<AgentTemplate> = templates
            .values()
            .filter(|t| match filter.project_scope {
                Some(scope) => t.project_id == scope,
                None => true,
            })
            .filter(|t| match filter.status {
                Some(status) => t.status == status,
                None => true,
            })
            .cloned()
            .collect();
        out.sort_by_key(|t| std::cmp::Reverse(t.created_at));
        let limit = filter.limit.max(1);
        Ok(out.into_iter().skip(filter.offset).take(limit).collect())
    }

    async fn update_agent_template_status(
        &self,
        id: AgentTemplateId,
        status: AgentTemplateStatus,
    ) -> Result<bool, StoreError> {
        let mut templates = self.agent_templates.lock().unwrap();
        match templates.get_mut(&id) {
            Some(t) => {
                t.status = status;
                t.updated_at = Utc::now();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn agent_template_dependents(
        &self,
        id: AgentTemplateId,
    ) -> Result<Vec<AgentId>, StoreError> {
        let agents = self.agents.lock().unwrap();
        let mut out: Vec<AgentId> = agents
            .values()
            .filter(|a| a.template_pin.map(|p| p.template_id) == Some(id))
            .map(|a| a.id)
            .collect();
        out.sort();
        Ok(out)
    }

    async fn retire_agent_template(
        &self,
        id: AgentTemplateId,
        expected_dependent_hash: &str,
    ) -> Result<bool, StoreError> {
        let mut templates = self.agent_templates.lock().unwrap();
        if !templates.contains_key(&id) {
            return Ok(false);
        }
        let dependents = {
            let agents = self.agents.lock().unwrap();
            let mut out: Vec<AgentId> = agents
                .values()
                .filter(|a| a.template_pin.map(|p| p.template_id) == Some(id))
                .map(|a| a.id)
                .collect();
            out.sort();
            out
        };
        if fleet_core::dependent_set_hash(&dependents) != expected_dependent_hash {
            return Err(StoreError::Conflict(format!(
                "agent template dependents changed since it was shown: {} agent(s) now depend on {}",
                dependents.len(),
                id
            )));
        }
        let t = templates.get_mut(&id).expect("checked above");
        t.status = AgentTemplateStatus::Retired;
        t.updated_at = Utc::now();
        Ok(true)
    }

    async fn create_agent_template_revision(
        &self,
        template_id: AgentTemplateId,
        body: &AgentTemplateBody,
        created_by: Option<&str>,
    ) -> Result<AgentTemplateRevision, StoreError> {
        let status = {
            let templates = self.agent_templates.lock().unwrap();
            templates
                .get(&template_id)
                .map(|t| t.status)
                .ok_or_else(|| {
                    StoreError::Conflict(format!("no such agent template: {template_id}"))
                })?
        };
        if !status.accepts_new_revisions() {
            return Err(StoreError::Conflict(format!(
                "agent template {template_id} is {} and accepts no new revisions",
                status.as_str()
            )));
        }
        let mut revisions = self.agent_template_revisions.lock().unwrap();
        let next = revisions
            .values()
            .filter(|r| r.template_id == template_id)
            .map(|r| r.content_revision)
            .max()
            .unwrap_or(0)
            + 1;
        let normalized = body.normalized();
        let revision = AgentTemplateRevision {
            id: AgentTemplateRevisionId::new(),
            template_id,
            content_revision: next,
            content_hash: normalized.content_hash(),
            body: normalized,
            revoked_at: None,
            created_by: created_by.map(|s| s.to_string()),
            created_at: Utc::now(),
        };
        revisions.insert(revision.id, revision.clone());
        Ok(revision)
    }

    async fn list_agent_template_revisions(
        &self,
        template_id: AgentTemplateId,
    ) -> Result<Vec<AgentTemplateRevision>, StoreError> {
        let revisions = self.agent_template_revisions.lock().unwrap();
        let mut out: Vec<AgentTemplateRevision> = revisions
            .values()
            .filter(|r| r.template_id == template_id)
            .cloned()
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.content_revision));
        Ok(out)
    }

    async fn get_agent_template_revision(
        &self,
        id: AgentTemplateRevisionId,
    ) -> Result<Option<AgentTemplateRevision>, StoreError> {
        Ok(self
            .agent_template_revisions
            .lock()
            .unwrap()
            .get(&id)
            .cloned())
    }

    async fn revoke_agent_template_revision(
        &self,
        id: AgentTemplateRevisionId,
    ) -> Result<bool, StoreError> {
        let mut revisions = self.agent_template_revisions.lock().unwrap();
        match revisions.get_mut(&id) {
            // 이미 revoke된 것은 `false` — PgStore의 `revoked_at IS NULL`과 같다.
            Some(r) if r.revoked_at.is_none() => {
                r.revoked_at = Some(Utc::now());
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    // ── Issue (로드맵 #88) ────────────────────────────────────────────

    async fn create_issue(&self, issue: &Issue) -> Result<(), StoreError> {
        self.issues.lock().unwrap().insert(issue.id, issue.clone());
        Ok(())
    }

    async fn get_issue(&self, id: IssueId) -> Result<Option<Issue>, StoreError> {
        Ok(self.issues.lock().unwrap().get(&id).cloned())
    }

    async fn list_issues(&self, filter: &IssueFilter) -> Result<Vec<Issue>, StoreError> {
        let issues = self.issues.lock().unwrap();
        let mut out: Vec<Issue> = issues
            .values()
            .filter(|i| match filter.project_id {
                Some(p) => i.project_id == p,
                None => true,
            })
            .filter(|i| match filter.status {
                Some(s) => i.status == s,
                None => true,
            })
            .filter(|i| !filter.open_only || i.status.is_open())
            .cloned()
            .collect();
        out.sort_by_key(|i| std::cmp::Reverse(i.created_at));
        let limit = filter.limit.max(1);
        Ok(out.into_iter().skip(filter.offset).take(limit).collect())
    }

    async fn update_issue_fields(&self, issue: &Issue) -> Result<bool, StoreError> {
        let mut issues = self.issues.lock().unwrap();
        match issues.get_mut(&issue.id) {
            Some(stored) => {
                // PgStore와 동일하게 status/close_reason은 건드리지 않는다.
                stored.title = issue.title.clone();
                stored.body = issue.body.clone();
                stored.severity = issue.severity;
                stored.labels = issue.labels.clone();
                stored.assignee = issue.assignee.clone();
                stored.updated_at = Utc::now();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn transition_issue(
        &self,
        id: IssueId,
        status: IssueStatus,
        close_reason: Option<CloseReason>,
    ) -> Result<bool, StoreError> {
        let mut issues = self.issues.lock().unwrap();
        match issues.get_mut(&id) {
            Some(issue) => {
                issue.status = status;
                issue.close_reason = close_reason;
                issue.updated_at = Utc::now();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn add_issue_comment(&self, comment: &IssueComment) -> Result<(), StoreError> {
        self.issue_comments.lock().unwrap().push(comment.clone());
        Ok(())
    }

    async fn list_issue_comments(
        &self,
        issue_id: IssueId,
    ) -> Result<Vec<IssueComment>, StoreError> {
        let comments = self.issue_comments.lock().unwrap();
        let mut out: Vec<IssueComment> = comments
            .iter()
            .filter(|c| c.issue_id == issue_id)
            .cloned()
            .collect();
        out.sort_by_key(|c| c.created_at);
        Ok(out)
    }

    async fn link_issue_task(&self, link: &IssueTaskLink) -> Result<bool, StoreError> {
        let mut links = self.issue_task_links.lock().unwrap();
        // PgStore의 `(issue_id, task_id)` 유니크 인덱스와 같은 멱등성.
        if links.iter().any(|l| {
            l.issue_id == link.issue_id && l.task_id.is_some() && l.task_id == link.task_id
        }) {
            return Ok(false);
        }
        links.push(link.clone());
        Ok(true)
    }

    async fn unlink_issue_task(
        &self,
        issue_id: IssueId,
        task_id: TaskId,
    ) -> Result<bool, StoreError> {
        let mut links = self.issue_task_links.lock().unwrap();
        let before = links.len();
        links.retain(|l| !(l.issue_id == issue_id && l.task_id == Some(task_id)));
        Ok(links.len() != before)
    }

    async fn list_issue_task_links(
        &self,
        issue_id: IssueId,
    ) -> Result<Vec<IssueTaskLink>, StoreError> {
        let links = self.issue_task_links.lock().unwrap();
        let mut out: Vec<IssueTaskLink> = links
            .iter()
            .filter(|l| l.issue_id == issue_id)
            .cloned()
            .collect();
        out.sort_by_key(|l| l.linked_at);
        Ok(out)
    }

    async fn issue_has_active_tasks(&self, issue_id: IssueId) -> Result<bool, StoreError> {
        let links = self.issue_task_links.lock().unwrap();
        let tasks = self.tasks.lock().unwrap();
        Ok(links
            .iter()
            .filter(|l| l.issue_id == issue_id)
            .filter_map(|l| l.task_id)
            .filter_map(|tid| tasks.get(&tid))
            .any(|t| {
                matches!(
                    t.status,
                    TaskStatus::Pending | TaskStatus::Dispatched { .. }
                )
            }))
    }
}

#[cfg(test)]
mod enroll_worker_tests {
    use super::*;

    fn token(raw: &str, max_uses: u32) -> BootstrapToken {
        BootstrapToken {
            token_digest: BootstrapToken::digest_for(raw),
            created_at: Utc::now(),
            created_by: None,
            expires_at: None,
            max_uses,
            use_count: 0,
            notes: None,
            last_used_by: None,
            last_used_at: None,
        }
    }

    fn worker(name: &str) -> Worker {
        Worker::new(name, format!("wss://{name}.local/ws"))
    }

    /// 로드맵 #60 완료 게이트 — 중간 실패 rollback test.
    ///
    /// credential digest 충돌로 3단계 중 마지막(credential insert) 단계가
    /// 실패하면, bootstrap token도 소비되지 않고 worker도 생성되지 않아야 한다.
    #[tokio::test]
    async fn enroll_worker_rolls_back_on_credential_digest_conflict() {
        let store = MemStore::new();

        // 기존 워커 + credential을 미리 심어 digest 충돌을 유도.
        let existing_worker = worker("existing-worker");
        store.upsert_worker(&existing_worker).await.unwrap();
        store
            .upsert_worker_operational_credential(&WorkerOperationalCredential {
                worker_id: existing_worker.id,
                credential_digest: "dup-digest".to_string(),
                issued_at: Utc::now(),
                expires_at: None,
                revoked_at: None,
                rotation_generation: 1,
            })
            .await
            .unwrap();

        store
            .create_bootstrap_token(&token("join-token", 1))
            .await
            .unwrap();

        let new_worker = worker("new-worker");
        let new_credential = WorkerOperationalCredential {
            worker_id: new_worker.id,
            credential_digest: "dup-digest".to_string(), // 기존 credential과 충돌
            issued_at: Utc::now(),
            expires_at: None,
            revoked_at: None,
            rotation_generation: 1,
        };

        let result = store
            .enroll_worker("join-token", "new-worker", &new_worker, &new_credential)
            .await;

        assert!(
            matches!(result, Err(StoreError::Conflict(_))),
            "expected Conflict on digest collision, got {result:?}"
        );

        // (a) 호출은 에러를 반환했고 — 위에서 확인.
        // (b) bootstrap token은 여전히 미소비 상태.
        let tokens = store.list_bootstrap_tokens().await.unwrap();
        let stored = tokens
            .iter()
            .find(|t| t.token_digest == BootstrapToken::digest_for("join-token"))
            .expect("token still exists");
        assert_eq!(
            stored.use_count, 0,
            "token must remain unconsumed after rollback"
        );
        assert!(stored.last_used_by.is_none());

        // (c) worker는 생성되지 않음.
        assert!(store
            .get_worker_by_name("new-worker")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn enroll_worker_rolls_back_on_name_conflict() {
        let store = MemStore::new();

        let existing_worker = worker("taken-name");
        store.upsert_worker(&existing_worker).await.unwrap();
        store
            .create_bootstrap_token(&token("join-token-2", 1))
            .await
            .unwrap();

        // 동일 이름이지만 다른 id를 가진 worker로 enroll을 시도.
        let mut colliding_worker = worker("taken-name");
        colliding_worker.id = WorkerId::new();
        let credential = WorkerOperationalCredential {
            worker_id: colliding_worker.id,
            credential_digest: "unique-digest".to_string(),
            issued_at: Utc::now(),
            expires_at: None,
            revoked_at: None,
            rotation_generation: 1,
        };

        let result = store
            .enroll_worker("join-token-2", "taken-name", &colliding_worker, &credential)
            .await;
        assert!(
            matches!(result, Err(StoreError::Conflict(_))),
            "expected Conflict on name collision, got {result:?}"
        );

        let tokens = store.list_bootstrap_tokens().await.unwrap();
        let stored = tokens
            .iter()
            .find(|t| t.token_digest == BootstrapToken::digest_for("join-token-2"))
            .unwrap();
        assert_eq!(
            stored.use_count, 0,
            "token must remain unconsumed after rollback"
        );

        // credential도 저장되지 않아야 한다.
        assert!(store
            .find_active_worker_operational_credential("unique-digest")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn enroll_worker_commits_all_three_on_success() {
        let store = MemStore::new();
        store
            .create_bootstrap_token(&token("join-token-3", 1))
            .await
            .unwrap();

        let new_worker = worker("success-worker");
        let credential = WorkerOperationalCredential {
            worker_id: new_worker.id,
            credential_digest: "success-digest".to_string(),
            issued_at: Utc::now(),
            expires_at: None,
            revoked_at: None,
            rotation_generation: 1,
        };

        store
            .enroll_worker("join-token-3", "success-worker", &new_worker, &credential)
            .await
            .expect("enroll should succeed");

        let tokens = store.list_bootstrap_tokens().await.unwrap();
        let stored = tokens
            .iter()
            .find(|t| t.token_digest == BootstrapToken::digest_for("join-token-3"))
            .unwrap();
        assert_eq!(stored.use_count, 1);
        assert_eq!(stored.last_used_by.as_deref(), Some("success-worker"));

        assert!(store
            .get_worker_by_name("success-worker")
            .await
            .unwrap()
            .is_some());
        assert!(store
            .find_active_worker_operational_credential("success-digest")
            .await
            .unwrap()
            .is_some());
    }
}

#[cfg(test)]
mod delete_worker_cascade_tests {
    use super::*;

    fn worker(name: &str) -> Worker {
        Worker::new(name, format!("wss://{name}.local/ws"))
    }

    async fn put_credential(store: &MemStore, worker_name: &str, model_id: &str) {
        store
            .upsert_worker_credential(
                worker_name,
                model_id,
                "encrypted",
                "https://api.example.test",
                "chat_completions",
                200_000,
                None,
            )
            .await
            .unwrap();
    }

    /// 로드맵 #78 — MemStore가 PgStore의 두 CASCADE를 재현하는지 확인한다.
    ///
    /// PostgreSQL에서 `DELETE FROM workers`는
    /// `worker_operational_credentials`(018, worker_id)와
    /// `worker_credentials`(005, worker_name)를 함께 지운다. MemStore가 worker
    /// row만 지우면 이 계열 결함이 인메모리 테스트를 그대로 통과한다.
    #[tokio::test]
    async fn delete_worker_cascades_operational_and_llm_credentials() {
        let store = MemStore::new();
        let target = worker("doomed-worker");
        let bystander = worker("other-worker");
        store.upsert_worker(&target).await.unwrap();
        store.upsert_worker(&bystander).await.unwrap();

        store
            .upsert_worker_operational_credential(&WorkerOperationalCredential {
                worker_id: target.id,
                credential_digest: "target-digest".to_string(),
                issued_at: Utc::now(),
                expires_at: None,
                revoked_at: None,
                rotation_generation: 1,
            })
            .await
            .unwrap();
        store
            .upsert_worker_operational_credential(&WorkerOperationalCredential {
                worker_id: bystander.id,
                credential_digest: "bystander-digest".to_string(),
                issued_at: Utc::now(),
                expires_at: None,
                revoked_at: None,
                rotation_generation: 1,
            })
            .await
            .unwrap();

        put_credential(&store, "doomed-worker", "grok-4").await;
        put_credential(&store, "doomed-worker", "claude-opus").await;
        put_credential(&store, "other-worker", "grok-4").await;

        store.delete_worker(target.id).await.unwrap();

        // CASCADE 1 — operational credential이 사라져 더 이상 인증되지 않는다.
        assert!(
            store
                .find_active_worker_operational_credential("target-digest")
                .await
                .unwrap()
                .is_none(),
            "deleted worker's operational credential must not authenticate"
        );

        // CASCADE 2 — 암호화된 LLM credential도 함께 사라진다.
        assert!(
            store
                .list_worker_credentials("doomed-worker")
                .await
                .unwrap()
                .is_empty(),
            "deleted worker's LLM credentials must be removed"
        );

        // 다른 워커의 자산은 건드리지 않는다.
        assert!(
            store
                .find_active_worker_operational_credential("bystander-digest")
                .await
                .unwrap()
                .is_some(),
            "unrelated worker's operational credential must survive"
        );
        assert_eq!(
            store
                .list_worker_credentials("other-worker")
                .await
                .unwrap()
                .len(),
            1,
            "unrelated worker's LLM credentials must survive"
        );
    }

    /// PgStore는 `rows_affected() == 0`일 때 `NotFound`를 반환한다. MemStore도 같아야
    /// 두 저장소를 바꿔 끼워도 호출부 동작이 달라지지 않는다.
    #[tokio::test]
    async fn delete_worker_returns_not_found_for_unknown_id() {
        let store = MemStore::new();
        let result = store.delete_worker(WorkerId::new()).await;
        assert!(matches!(result, Err(StoreError::NotFound)));
    }
}

#[cfg(test)]
mod agent_command_fence_tests {
    //! Agent 명령 발행에 걸리는 control-plane 술어 (로드맵 `#67` 게이트 ①-B).
    //!
    //! PgStore 쪽 동치는 `tests/agents.rs`에 있다. 두 백엔드를 모두 도는
    //! 이유는 fence 판정이 **구조적으로 다른 자리**에 있기 때문이다:
    //! Postgres는 UPDATE 문 안의 `EXISTS` 술어가, MemStore는 락 밖의 선행
    //! 검사가 그 일을 한다. 한쪽만 시험하면 다른 쪽이 조용히 술어를 잃어도
    //! 알 수 없다.
    use super::*;

    /// lease를 한 번 만료시키고 다른 instance가 가져가게 해서, 첫 보유자의
    /// fence를 **낡은 것**으로 만든다. `(살아 있는 fence, 낡은 fence)`.
    async fn stale_and_current(store: &MemStore, cluster: &str) -> (ControlFence, ControlFence) {
        let first = store
            .acquire_control_lease(cluster, "instance-a", std::time::Duration::from_millis(1))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let second = store
            .acquire_control_lease(cluster, "instance-b", std::time::Duration::from_secs(30))
            .await
            .unwrap();
        assert!(second.epoch > first.epoch, "가로채면 epoch이 오른다");
        (
            ControlFence {
                cluster_id: cluster.to_string(),
                epoch: second.epoch,
            },
            ControlFence {
                cluster_id: cluster.to_string(),
                epoch: first.epoch,
            },
        )
    }

    async fn seeded() -> (MemStore, Agent, Worker) {
        let store = MemStore::new();
        let project = Project::new("fence");
        store.create_project(&project).await.unwrap();
        let agent = Agent::new(project.id, "fenced");
        store.create_agent(&agent).await.unwrap();
        let worker = Worker::new("fence-w", "wss://fence-w.local/ws");
        store.upsert_worker(&worker).await.unwrap();
        (store, agent, worker)
    }

    #[tokio::test]
    async fn a_stale_holder_cannot_place_an_agent() {
        let (store, agent, worker) = seeded().await;
        let (_current, stale) = stale_and_current(&store, "c-place").await;

        assert_eq!(
            store
                .assign_agent_worker(agent.id, worker.id, Some(&stale))
                .await
                .unwrap(),
            SlotClaim::Fenced
        );
        let after = store.get_agent(agent.id).await.unwrap().unwrap();
        assert_eq!(after.worker_id, None, "거절된 명령은 행을 바꾸지 않는다");
        assert_eq!(after.command_generation, 0);
    }

    #[tokio::test]
    async fn a_stale_holder_cannot_issue_a_desired_status() {
        let (store, agent, _worker) = seeded().await;
        let (_current, stale) = stale_and_current(&store, "c-desired").await;

        assert_eq!(
            store
                .set_agent_desired_status(agent.id, AgentDesiredStatus::Running, Some(&stale))
                .await
                .unwrap(),
            CommandIssue::Fenced
        );
        let after = store.get_agent(agent.id).await.unwrap().unwrap();
        assert_eq!(after.desired_status, AgentDesiredStatus::Stopped);
        assert_eq!(after.command_generation, 0);
    }

    /// fenced가 **존재 판정보다 먼저** 나와야 한다.
    ///
    /// 순서가 반대면 호출자는 "나는 제어 기관이었는데 이 Agent만 없더라"로
    /// 읽고 없는 대상을 쫓는다. PgStore가 같은 이유로 같은 순서를 지킨다.
    #[tokio::test]
    async fn fenced_outranks_not_found() {
        let (store, _agent, worker) = seeded().await;
        let (_current, stale) = stale_and_current(&store, "c-order").await;
        let missing = AgentId::new();

        assert_eq!(
            store
                .assign_agent_worker(missing, worker.id, Some(&stale))
                .await
                .unwrap(),
            SlotClaim::Fenced
        );
        assert_eq!(
            store
                .set_agent_desired_status(missing, AgentDesiredStatus::Running, Some(&stale))
                .await
                .unwrap(),
            CommandIssue::Fenced
        );
    }

    /// epoch은 **세대가 오를 때만** 찍힌다.
    ///
    /// 두 번째 호출은 같은 값을 다시 넣으므로 명령이 새로 발행되지 않는다.
    /// 그때도 epoch을 덮으면 그 컬럼의 뜻이 "이 명령을 발행한 세대"에서
    /// "이 행을 마지막으로 손댄 세대"로 바뀌고, 명령의 출처를 사후에
    /// 복원할 수 없게 된다.
    #[tokio::test]
    async fn the_epoch_is_stamped_only_when_a_command_is_actually_issued() {
        let (store, agent, _worker) = seeded().await;
        let first = store
            .acquire_control_lease("c-stamp", "instance-a", std::time::Duration::from_millis(1))
            .await
            .unwrap();
        let f1 = ControlFence {
            cluster_id: "c-stamp".into(),
            epoch: first.epoch,
        };
        store
            .set_agent_desired_status(agent.id, AgentDesiredStatus::Running, Some(&f1))
            .await
            .unwrap();
        let issued = store.get_agent(agent.id).await.unwrap().unwrap();
        assert_eq!(issued.command_generation, 1);
        assert_eq!(issued.command_control_epoch, Some(first.epoch));

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let second = store
            .acquire_control_lease("c-stamp", "instance-b", std::time::Duration::from_secs(30))
            .await
            .unwrap();
        let f2 = ControlFence {
            cluster_id: "c-stamp".into(),
            epoch: second.epoch,
        };
        assert_eq!(
            store
                .set_agent_desired_status(agent.id, AgentDesiredStatus::Running, Some(&f2))
                .await
                .unwrap(),
            CommandIssue::Issued,
            "살아 있는 보유자의 쓰기는 값이 같아도 거절이 아니다"
        );
        let again = store.get_agent(agent.id).await.unwrap().unwrap();
        assert_eq!(again.command_generation, 1, "값이 같으면 세대는 그대로다");
        assert_eq!(
            again.command_control_epoch,
            Some(first.epoch),
            "발행한 세대는 1이다 — 2가 되면 컬럼의 뜻이 바뀐다"
        );
    }

    /// fence가 없는 배포(HA lease 미사용)는 이 변경 이전과 동작이 같다.
    #[tokio::test]
    async fn no_fence_means_no_predicate() {
        let (store, agent, worker) = seeded().await;
        assert_eq!(
            store
                .assign_agent_worker(agent.id, worker.id, None)
                .await
                .unwrap(),
            SlotClaim::Claimed
        );
        let after = store.get_agent(agent.id).await.unwrap().unwrap();
        assert_eq!(after.command_control_epoch, None);
    }
}

#[cfg(test)]
mod agent_observation_gate_tests {
    //! 게이트 ② — 다른 Worker가 돌리고 있다고 보고한 Agent는 옮기지 않는다
    //! (로드맵 `#67`).
    //!
    //! PgStore 쪽 동치는 `tests/agents.rs`에 있다. 두 백엔드를 모두 도는
    //! 이유는 `agent_command_fence_tests`와 같다 — 판정이 구조적으로 다른
    //! 자리에 있다. Postgres는 UPDATE 문 안의 술어가, 여기서는 락 안의 선행
    //! 검사가 그 일을 한다.
    use super::*;
    // 관측 **사유**는 술어가 보지 않으므로(보는 것은 `observed_status`뿐)
    // 생산 코드에는 등장하지 않는다. 테스트만 값을 심는다.
    use fleet_core::AgentObservationReason;

    async fn seeded(cap: Option<u32>) -> (MemStore, Project, Worker, Worker) {
        let store = MemStore::new();
        let project = Project::new("gate2");
        store.create_project(&project).await.unwrap();
        let mut owner = Worker::new("gate2-owner", "wss://owner.local/ws");
        let mut other = Worker::new("gate2-other", "wss://other.local/ws");
        owner.max_agent_processes = cap;
        other.max_agent_processes = cap;
        store.upsert_worker(&owner).await.unwrap();
        store.upsert_worker(&other).await.unwrap();
        (store, project, owner, other)
    }

    async fn placed_and_running(
        store: &MemStore,
        project: &Project,
        owner: &Worker,
        name: &str,
    ) -> Agent {
        let agent = Agent::new(project.id, name).with_placement(owner.id, Utc::now());
        store.create_agent(&agent).await.unwrap();
        let changed = store
            .apply_agent_observations(
                owner.id,
                &[AgentObservation::Running { agent_id: agent.id }],
            )
            .await
            .unwrap();
        assert_eq!(changed, 1);
        agent
    }

    #[tokio::test]
    async fn running_observation_blocks_moving_to_another_worker() {
        let (store, project, owner, other) = seeded(None).await;
        let agent = placed_and_running(&store, &project, &owner, "busy").await;

        assert_eq!(
            store
                .assign_agent_worker(agent.id, other.id, None)
                .await
                .unwrap(),
            SlotClaim::ObservedRunning
        );
        let after = store.get_agent(agent.id).await.unwrap().unwrap();
        assert_eq!(after.worker_id, Some(owner.id), "옮기지 않았다");
        assert_eq!(after.command_generation, agent.command_generation);
    }

    #[tokio::test]
    async fn same_worker_reassignment_is_allowed_while_running() {
        let (store, project, owner, other) = seeded(None).await;
        let agent = placed_and_running(&store, &project, &owner, "busy-same").await;

        assert_eq!(
            store
                .assign_agent_worker(agent.id, owner.id, None)
                .await
                .unwrap(),
            SlotClaim::Claimed
        );

        // **관측이 살아남아야 한다.** 프로세스가 움직이지 않았으니 그 말은
        // 여전히 참이다. 여기서 지워지면 게이트 ②가 2단계로 뚫린다 — 같은
        // Worker로 한 번 재배정해(술어의 `worker_id = $2` 갈래가 허용한다)
        // 관측을 없앤 뒤, 그다음에 아무 데로나 옮기면 된다.
        let after = store.get_agent(agent.id).await.unwrap().unwrap();
        assert_eq!(after.observed_status, Some(AgentObservedStatus::Running));
        assert!(after.observed_at.is_some());
        assert_eq!(
            store
                .assign_agent_worker(agent.id, other.id, None)
                .await
                .unwrap(),
            SlotClaim::ObservedRunning,
            "재배정을 거쳐도 다른 Worker로는 여전히 못 간다"
        );
    }

    #[tokio::test]
    async fn failed_and_absent_observations_do_not_block() {
        let (store, project, owner, other) = seeded(None).await;

        let never_seen = Agent::new(project.id, "unseen").with_placement(owner.id, Utc::now());
        store.create_agent(&never_seen).await.unwrap();
        assert_eq!(
            store
                .assign_agent_worker(never_seen.id, other.id, None)
                .await
                .unwrap(),
            SlotClaim::Claimed
        );

        let rejected = Agent::new(project.id, "rejected").with_placement(owner.id, Utc::now());
        store.create_agent(&rejected).await.unwrap();
        store
            .apply_agent_observations(
                owner.id,
                &[AgentObservation::Failed {
                    agent_id: rejected.id,
                    reason: AgentObservationReason::SpawnFailed,
                }],
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .assign_agent_worker(rejected.id, other.id, None)
                .await
                .unwrap(),
            SlotClaim::Claimed
        );
        // 옛 Worker의 `failed`가 따라오면 안 된다. 새 Worker는 아직 시도조차
        // 하지 않았는데 "시도했고 실패했다"가 되고, 그 값은 다음 beat까지
        // 지워지지 않는다. PgStore에서는 036의 트리거가 같은 일을 한다.
        let after = store.get_agent(rejected.id).await.unwrap().unwrap();
        assert_eq!(after.observed_status, None);
        assert_eq!(after.observed_at, None);
        assert_eq!(after.observed_reason, None);
    }

    /// **cap이 관측보다 먼저 판정된다.** PgStore에서는 cap이 사전 검사이고
    /// 관측은 UPDATE 안의 술어라, 둘 다 걸리는 입력의 답이 `CapReached`다.
    /// 이 테스트가 없으면 두 저장소가 조용히 갈라지고, 그 차이는 호출부
    /// 테스트가 어느 저장소를 골랐느냐에 따라서만 드러난다.
    #[tokio::test]
    async fn cap_is_decided_before_the_observation() {
        let (store, project, owner, other) = seeded(Some(1)).await;
        // 목적지를 가득 채운다.
        let squatter = Agent::new(project.id, "squatter").with_placement(other.id, Utc::now());
        store.create_agent(&squatter).await.unwrap();
        // 그리고 옮기려는 Agent는 관측까지 걸려 있다.
        let agent = placed_and_running(&store, &project, &owner, "both").await;

        assert_eq!(
            store
                .assign_agent_worker(agent.id, other.id, None)
                .await
                .unwrap(),
            SlotClaim::CapReached
        );
    }

    #[tokio::test]
    async fn clearing_the_observation_reopens_the_move() {
        let (store, project, owner, other) = seeded(None).await;
        let agent = placed_and_running(&store, &project, &owner, "recalled").await;
        assert_eq!(
            store
                .assign_agent_worker(agent.id, other.id, None)
                .await
                .unwrap(),
            SlotClaim::ObservedRunning
        );

        // 회수하면 다음 beat의 목록에서 빠지고, 말해지지 않은 것이 지워진다.
        store.apply_agent_observations(owner.id, &[]).await.unwrap();

        assert_eq!(
            store
                .assign_agent_worker(agent.id, other.id, None)
                .await
                .unwrap(),
            SlotClaim::Claimed
        );
    }

    /// CASCADE 3 — `delete_worker`가 배치와 관측을 함께 지운다.
    ///
    /// PgStore에서는 030의 FK `ON DELETE SET NULL`과 036의 트리거가 한다.
    /// MemStore가 worker row만 지우면 `agents.worker_id`가 지워진 Worker를
    /// 계속 가리키고, stale한 `running`이 그 Agent를 영구히 묶는다.
    #[tokio::test]
    async fn deleting_the_worker_clears_placement_and_observation() {
        let (store, project, owner, other) = seeded(None).await;
        let agent = placed_and_running(&store, &project, &owner, "stranded").await;

        store.delete_worker(owner.id).await.unwrap();

        let after = store.get_agent(agent.id).await.unwrap().unwrap();
        assert_eq!(after.worker_id, None, "FK의 SET NULL 몫");
        assert_eq!(after.assigned_at, None, "030 트리거의 몫");
        assert_eq!(after.observed_status, None, "036 트리거의 몫");
        assert_eq!(after.observed_at, None);
        assert_eq!(after.observed_reason, None);
        // Agent 정의 자체는 살아 있다.
        assert_eq!(after.status, AgentStatus::Ready);

        assert_eq!(
            store
                .assign_agent_worker(agent.id, other.id, None)
                .await
                .unwrap(),
            SlotClaim::Claimed
        );
    }
}
