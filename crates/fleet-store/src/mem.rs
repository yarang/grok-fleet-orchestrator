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
    AuditEvent, AuditFilter, BootstrapToken, EmailVerificationToken, EventEntry, FleetEvent, Host,
    HostEvent, LoginAttempt, Permission, PermissionId, Role, RoleId, Session, SessionId, SshKey,
    Task, TaskFilter, TaskId, TaskOutput, TaskOutputChunk, TaskStatus, TaskStatusFilter, User,
    UserId, Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
};

use crate::{AdminApiToken, Store, StoreError, StoredCredential, WorkerOperationalCredential};

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
    /// 실패 주입 대상 메서드 이름 집합 — `check`/`record` 자체가 아니라
    /// 테스트 셋업 편의를 위한 것이므로 트레이트 밖 필드.
    failing: Mutex<HashSet<&'static str>>,
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
    /// `"delete_old_login_attempts"`.
    pub fn with_failing(self, methods: &[&'static str]) -> Self {
        self.failing.lock().unwrap().extend(methods.iter().copied());
        self
    }

    fn is_failing(&self, method: &'static str) -> bool {
        self.failing.lock().unwrap().contains(method)
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

    async fn increment_task_retry_count(&self, id: TaskId) -> Result<u32, StoreError> {
        let mut tasks = self.tasks.lock().unwrap();
        let Some(task) = tasks.get_mut(&id) else {
            return Err(StoreError::NotFound);
        };
        task.retry_count += 1;
        Ok(task.retry_count)
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
        self.workers.lock().unwrap().insert(w.id, w.clone());
        Ok(())
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
