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
pub mod postgres;
pub mod rbac;

pub use error::StoreError;
pub use listener::listen_events;
pub use postgres::PgStore;
pub use rbac::{
    consume_bootstrap_and_create_admin, issue_admin_bootstrap_token, seed_builtin_roles,
    seed_permissions, seed_rbac_and_maybe_issue_bootstrap, BootstrapAdminError,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fleet_core::{
    BootstrapToken, EventEntry, FleetEvent, LoginAttempt, Permission, PermissionId, Role, RoleId,
    Session, SessionId, Task, TaskFilter, TaskId, TaskOutput, TaskStatus, User, UserId, Worker,
    WorkerFilter, WorkerHeartbeat, WorkerId,
};

/// 영속 저장소 trait. 모든 상태 조회/변경은 이 인터페이스를 경유합니다.
///
/// 구현체:
/// - [`PgStore`] — PostgreSQL (프로덕션)
/// - (테스트용 mock은 `fleet-scheduler` 테스트에서 정의)
#[async_trait]
pub trait Store: Send + Sync {
    // ── Task ───────────────────────────────────────────────────────

    /// 작업을 저장소에 삽입. ID 충돌 시 에러.
    async fn insert_task(&self, task: &Task) -> Result<(), StoreError>;

    /// 작업 ID로 조회. 없으면 `None`.
    async fn get_task(&self, id: TaskId) -> Result<Option<Task>, StoreError>;

    /// 작업 상태 업데이트. `status_phase` 생성 칼럼도 자동 갱신.
    async fn update_task_status(&self, id: TaskId, status: &TaskStatus) -> Result<(), StoreError>;

    /// 필터 조건으로 작업 목록 조회 (생성일 역순).
    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, StoreError>;

    // ── Worker ──────────────────────────────────────────────────────

    /// 워커를 upsert (id 기준). 같은 name의 기존 워커는 덮어씀.
    async fn upsert_worker(&self, worker: &Worker) -> Result<(), StoreError>;

    /// 워커 ID로 조회.
    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError>;

    /// 워커 이름으로 조회 (MCP `server_hint` 해석용).
    async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError>;

    /// 필터 조건으로 워커 목록 조회.
    async fn list_workers(&self, filter: &WorkerFilter) -> Result<Vec<Worker>, StoreError>;

    /// 워커 삭제 (등록 해제).
    async fn delete_worker(&self, id: WorkerId) -> Result<(), StoreError>;

    /// 하트비트 수신 시 워커 상태 갱신 (active_tasks, last_seen, agent_healthy).
    async fn update_worker_heartbeat(
        &self,
        id: WorkerId,
        heartbeat: &WorkerHeartbeat,
    ) -> Result<(), StoreError>;

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

    /// 만료된 세션 일괄 삭제 (정기 정리용).
    async fn delete_expired_sessions(&self) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("delete_expired_sessions"))
    }

    /// 사용자의 모든 세션 삭제 (비활성화/패스워드 변경 시).
    async fn delete_user_sessions(&self, _user_id: UserId) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("delete_user_sessions"))
    }

    // ── Login attempts (rate limiting + 감사) ────────────────────

    /// 로그인 시도 기록.
    async fn record_login_attempt(&self, _attempt: &LoginAttempt) -> Result<(), StoreError> {
        Err(StoreError::Unsupported("record_login_attempt"))
    }

    /// `(identifier, ip)` 기준 최근 `window_secs`초 내 실패 횟수.
    async fn count_recent_failed_attempts(
        &self,
        _identifier: &str,
        _ip: Option<&str>,
        _window_secs: i64,
    ) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("count_recent_failed_attempts"))
    }

    /// identifier의 과거 시도 기록 삭제 (성공 시 초기화).
    async fn clear_login_attempts(
        &self,
        _identifier: &str,
        _ip: Option<&str>,
    ) -> Result<u64, StoreError> {
        Err(StoreError::Unsupported("clear_login_attempts"))
    }
}
