//! # RBAC 인증 도메인 모델 (Phase 9.1)
//!
//! 웹 대시보드의 사용자/역할/권한/세션 도메인 타입. 기존 bearer token 인증
//! (fleet-api)과 병행하며, 대시보드 경로에만 적용.
//!
//! ## 설계 결정
//!
//! 1. **Stateful Session (DB-backed)** — JWT가 아닌 불투명 쿠키 토큰. 즉시
//!    취소 가능, 감사 추적 완벽, fleet은 이미 Postgres 중심.
//! 2. **OTP 부트스트랩** — 환경변수 평문이 아닌 1회용 토큰으로 최초 관리자
//!    등록. 기존 `bootstrap_tokens` 패턴 재사용.
//! 3. **Rust enum으로 권한 카탈로그 관리** — SQL 하드코딩 회피, 단일 진실
//!    공급원, `seed_rbac_if_empty()`로 부트 시 자동 동기화.
//!
//! ## 도메인 관계
//!
//! ```text
//! User ─┬─< UserRole >─ Role ─< RolePermission >─ Permission
//!       │
//!       └─< Session (cookie token, hashed in DB)
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod password;

// ── Newtype IDs ─────────────────────────────────────────────────────────

/// 사용자 식별자.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(pub Uuid);

impl UserId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for UserId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for UserId {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 역할 식별자.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RoleId(pub Uuid);

impl RoleId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for RoleId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for RoleId {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}

/// 권한 식별자.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PermissionId(pub Uuid);

impl PermissionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for PermissionId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for PermissionId {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}

/// 세션 식별자.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for SessionId {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}

// ── Entities ────────────────────────────────────────────────────────────

/// 사용자 계정. 이메일/비밀번호 기반 로그인에 사용.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    /// 고유. `^[a-zA-Z][a-zA-Z0-9_-]{2,63}$`.
    pub username: String,
    /// 고유 로그인 식별자 (이메일).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// 이메일 인증 여부.
    #[serde(default)]
    pub email_verified: bool,
    /// Argon2id PHC 문자열.
    #[serde(skip_serializing)]
    pub password_hash: String,
    /// 비활성화 시 로그인 거부.
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_login_at: Option<DateTime<Utc>>,
}

impl User {
    /// username 형식 검증: `^[a-zA-Z][a-zA-Z0-9_-]{2,63}$`.
    pub fn validate_username(name: &str) -> Result<(), AuthError> {
        if name.is_empty() || name.len() > 64 {
            return Err(AuthError::InvalidUsername);
        }
        let mut chars = name.chars();
        let first = chars.next().ok_or(AuthError::InvalidUsername)?;
        if !first.is_ascii_alphabetic() {
            return Err(AuthError::InvalidUsername);
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(AuthError::InvalidUsername);
        }
        if name.len() < 3 {
            return Err(AuthError::InvalidUsername);
        }
        Ok(())
    }

    /// 이메일 형식 검증 (간단한 RFC 5322 축소판).
    pub fn validate_email(addr: &str) -> Result<(), AuthError> {
        if addr.is_empty() || addr.len() > 254 {
            return Err(AuthError::InvalidEmail);
        }
        // local@domain 형식 확인.
        let at = addr.find('@').ok_or(AuthError::InvalidEmail)?;
        let (local, domain) = addr.split_at(at);
        let domain = &domain[1..]; // '@' 제거
        if local.is_empty() || local.len() > 64 {
            return Err(AuthError::InvalidEmail);
        }
        if domain.is_empty() || !domain.contains('.') {
            return Err(AuthError::InvalidEmail);
        }
        // 로컬 파트: 알파벳, 숫자, 점, 하이픈, 언더스코어, 플러스 허용.
        if !local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' || c == '+')
        {
            return Err(AuthError::InvalidEmail);
        }
        Ok(())
    }
}

/// 역할. builtin(admin/operator/viewer) 또는 custom.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: RoleId,
    /// 고유 이름.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// admin/operator/viewer = true.
    pub builtin: bool,
    pub created_at: DateTime<Utc>,
}

/// 권한 카탈로그 엔트리.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub id: PermissionId,
    /// `task:create` 형식.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// 사용자-역할 매핑.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRole {
    pub user_id: UserId,
    pub role_id: RoleId,
    pub granted_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granted_by: Option<UserId>,
}

/// 역할-권한 매핑.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolePermission {
    pub role_id: RoleId,
    pub permission_id: PermissionId,
}

/// 세션. 쿠키 토큰은 SHA-256 해시만 저장 (재현 불가).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub user_id: UserId,
    /// SHA-256 hex of cookie value.
    #[serde(skip_serializing)]
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
}

impl Session {
    /// 현재 시각 기준 만료 여부.
    pub fn is_expired(&self) -> bool {
        self.expires_at <= Utc::now()
    }
}

/// 이메일 인증 토큰.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailVerificationToken {
    pub id: Uuid,
    pub user_id: UserId,
    /// SHA-256 hex of the raw token.
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
}

impl EmailVerificationToken {
    pub fn is_expired(&self) -> bool {
        self.expires_at <= Utc::now()
    }
    pub fn is_consumed(&self) -> bool {
        self.consumed_at.is_some()
    }
}

/// 비밀번호 재설정 토큰.
pub type PasswordResetToken = EmailVerificationToken;

/// 로그인 시도 추적 (rate limiting + 감사). 5회 실패 시 60초 잠금.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginAttempt {
    pub id: Uuid,
    /// username 또는 IP (username 미확인 시).
    pub identifier: String,
    pub ip_address: Option<String>,
    pub success: bool,
    /// 실패 사유 (success=false일 때).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    pub attempted_at: DateTime<Utc>,
}

// ── Permission Catalog (Rust enum = single source of truth) ─────────────

/// fleet의 모든 권한. 새 권한 추가 시 이 enum에만 추가하면
/// `seed_rbac_if_empty()`가 자동으로 DB에 동기화.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    // 대시보드
    #[serde(rename = "dashboard:view")]
    DashboardView,
    // 작업
    #[serde(rename = "task:list")]
    TaskList,
    #[serde(rename = "task:create")]
    TaskCreate,
    #[serde(rename = "task:cancel")]
    TaskCancel,
    #[serde(rename = "task:read")]
    TaskRead,
    #[serde(rename = "task:output")]
    TaskOutput,
    // 워커
    #[serde(rename = "worker:list")]
    WorkerList,
    #[serde(rename = "worker:register")]
    WorkerRegister,
    #[serde(rename = "worker:delete")]
    WorkerDelete,
    /// worker operational credential rotate/revoke (로드맵 #60 6단계).
    /// admin 전용 — Operator 기본 역할에는 부여하지 않는다.
    ///
    /// **주의**: 이것은 worker가 스스로를 인증할 때 쓰는 `fwo_` 토큰(단수형
    /// `/credential` 경로)에 대한 권한이다. 워커가 모델 호출에 쓰는 LLM
    /// 프로바이더 API 키(복수형 `/credentials` 경로)는 아래 `WorkerLlmCredential*`
    /// 권한이 따로 통제한다. 두 개념을 하나의 capability로 묶지 않는다.
    #[serde(rename = "worker:credential:manage")]
    WorkerCredentialManage,
    /// worker LLM credential 메타데이터 조회(`GET /v1/workers/:name/credentials`).
    /// 응답에 api_key는 포함되지 않는다 — 어떤 모델이 설정돼 있는지만 노출.
    #[serde(rename = "worker:llm_credential:read")]
    WorkerLlmCredentialRead,
    /// worker LLM credential **평문 export**
    /// (`GET /v1/workers/:name/credentials/:model_id/export`).
    ///
    /// 이 권한을 가진 주체는 해당 orchestrator가 보관한 모든 워커의 LLM
    /// 프로바이더 API 키 원문을 획득할 수 있다. 프로비저너처럼 실제로 키를
    /// 배포해야 하는 주체에게만 부여한다. 저장·삭제 권한
    /// ([`PermissionKind::WorkerLlmCredentialManage`])과 분리해 둔 이유는,
    /// 프로비저너 토큰이 유출되더라도 credential을 덮어쓰거나 지울 수는 없게
    /// 하기 위함이다.
    #[serde(rename = "worker:llm_credential:export")]
    WorkerLlmCredentialExport,
    /// worker LLM credential 저장/회전/삭제
    /// (`PUT`/`DELETE /v1/workers/:name/credentials...`).
    ///
    /// export를 허용하지 않는다 — 평문 열람은
    /// [`PermissionKind::WorkerLlmCredentialExport`]가 따로 통제한다.
    #[serde(rename = "worker:llm_credential:manage")]
    WorkerLlmCredentialManage,
    // 토큰
    #[serde(rename = "token:issue")]
    TokenIssue,
    #[serde(rename = "token:list")]
    TokenList,
    #[serde(rename = "token:revoke")]
    TokenRevoke,
    /// admin API bearer token(`admin_api_tokens` 테이블) 생성/rotate/revoke
    /// (로드맵 #72).
    ///
    /// **주의**: 위 `Token*`(`TokenIssue`/`TokenList`/`TokenRevoke`)은 worker
    /// join 전용 bootstrap token에 대한 권한이다. 이것은 admin이 스스로를
    /// 인증하는 bearer credential(`admin_api_tokens`)에 대한 별개 권한이며,
    /// `#66`에서 겪은 capability 이름 재사용으로 인한 권한 혼선을 반복하지
    /// 않기 위해 의도적으로 분리했다. admin 전용 — Operator/Viewer 기본
    /// 역할에는 부여하지 않는다.
    #[serde(rename = "admin_token:manage")]
    AdminTokenManage,
    /// admin API bearer token 메타데이터 목록 조회(`GET /v1/admin/tokens`).
    /// 응답에 digest는 포함되지 않는다 — 어떤 principal이 발급돼 있는지만
    /// 노출한다. 생성/rotate/revoke는 [`PermissionKind::AdminTokenManage`]가
    /// 따로 통제한다 (worker LLM credential의 read/manage 분리와 동일한 이유).
    #[serde(rename = "admin_token:list")]
    AdminTokenList,
    // 사용자/역할 (admin 전용)
    #[serde(rename = "user:create")]
    UserCreate,
    #[serde(rename = "user:delete")]
    UserDelete,
    #[serde(rename = "user:read")]
    UserRead,
    #[serde(rename = "user:role:assign")]
    UserRoleAssign,
    #[serde(rename = "user:role:revoke")]
    UserRoleRevoke,
    #[serde(rename = "role:create")]
    RoleCreate,
    #[serde(rename = "role:delete")]
    RoleDelete,
    #[serde(rename = "audit:read")]
    AuditRead,
    #[serde(rename = "host:provision")]
    HostProvision,
    // Project (로드맵 #48 1단계). `project:policy_manage`/`project:assign`은
    // 목표 계약에서 명시적으로 승인 전 차단 대상이라 아직 만들지 않는다
    // (docs/architecture/project-feature-design.md "권한과 구현 차단 조건").
    #[serde(rename = "project:read")]
    ProjectRead,
    #[serde(rename = "project:create")]
    ProjectCreate,
    /// Project를 `Draining`으로 전이(archive 요청)한다. 영구 삭제가 아니다
    /// — [`crate::project::ProjectStatus`] 참고.
    #[serde(rename = "project:delete")]
    ProjectDelete,
    // 시스템
    #[serde(rename = "events:list")]
    EventsList,
    #[serde(rename = "metrics:view")]
    MetricsView,
}

impl PermissionKind {
    /// DB permissions.name 컬럼 값.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DashboardView => "dashboard:view",
            Self::TaskList => "task:list",
            Self::TaskCreate => "task:create",
            Self::TaskCancel => "task:cancel",
            Self::TaskRead => "task:read",
            Self::TaskOutput => "task:output",
            Self::WorkerList => "worker:list",
            Self::WorkerRegister => "worker:register",
            Self::WorkerDelete => "worker:delete",
            Self::WorkerCredentialManage => "worker:credential:manage",
            Self::WorkerLlmCredentialRead => "worker:llm_credential:read",
            Self::WorkerLlmCredentialExport => "worker:llm_credential:export",
            Self::WorkerLlmCredentialManage => "worker:llm_credential:manage",
            Self::TokenIssue => "token:issue",
            Self::TokenList => "token:list",
            Self::TokenRevoke => "token:revoke",
            Self::AdminTokenManage => "admin_token:manage",
            Self::AdminTokenList => "admin_token:list",
            Self::UserCreate => "user:create",
            Self::UserDelete => "user:delete",
            Self::UserRead => "user:read",
            Self::UserRoleAssign => "user:role:assign",
            Self::UserRoleRevoke => "user:role:revoke",
            Self::RoleCreate => "role:create",
            Self::RoleDelete => "role:delete",
            Self::AuditRead => "audit:read",
            Self::HostProvision => "host:provision",
            Self::ProjectRead => "project:read",
            Self::ProjectCreate => "project:create",
            Self::ProjectDelete => "project:delete",
            Self::EventsList => "events:list",
            Self::MetricsView => "metrics:view",
        }
    }

    /// 전체 카탈로그 (알파벳순).
    pub fn all() -> &'static [PermissionKind] {
        &[
            Self::DashboardView,
            Self::TaskList,
            Self::TaskCreate,
            Self::TaskCancel,
            Self::TaskRead,
            Self::TaskOutput,
            Self::WorkerList,
            Self::WorkerRegister,
            Self::WorkerDelete,
            Self::WorkerCredentialManage,
            Self::WorkerLlmCredentialRead,
            Self::WorkerLlmCredentialExport,
            Self::WorkerLlmCredentialManage,
            Self::TokenIssue,
            Self::TokenList,
            Self::TokenRevoke,
            Self::AdminTokenManage,
            Self::AdminTokenList,
            Self::UserCreate,
            Self::UserDelete,
            Self::UserRead,
            Self::UserRoleAssign,
            Self::UserRoleRevoke,
            Self::RoleCreate,
            Self::RoleDelete,
            Self::AuditRead,
            Self::HostProvision,
            Self::ProjectRead,
            Self::ProjectCreate,
            Self::ProjectDelete,
            Self::EventsList,
            Self::MetricsView,
        ]
    }

    /// 이름 문자열에서 역직렬화.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::all().iter().copied().find(|p| p.as_str() == name)
    }
}

// ── Builtin Roles ───────────────────────────────────────────────────────

/// 미리 정의된 역할. `seed_rbac_if_empty()`가 부트 시 DB에 삽입.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinRole {
    /// 모든 권한.
    Admin,
    /// task/worker 관리 + 대시보드 조회 (사용자 관리 불가).
    Operator,
    /// 읽기 전용.
    Viewer,
}

impl BuiltinRole {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Operator => "operator",
            Self::Viewer => "viewer",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Admin => "Full access to all features including user management",
            Self::Operator => "Manage tasks and workers, view dashboard",
            Self::Viewer => "Read-only access to dashboard",
        }
    }

    /// 역할이 가지는 권한 목록.
    pub fn permissions(&self) -> Vec<PermissionKind> {
        match self {
            Self::Admin => PermissionKind::all().to_vec(),
            Self::Operator => vec![
                PermissionKind::DashboardView,
                PermissionKind::TaskList,
                PermissionKind::TaskCreate,
                PermissionKind::TaskCancel,
                PermissionKind::TaskRead,
                PermissionKind::TaskOutput,
                PermissionKind::WorkerList,
                PermissionKind::ProjectRead,
                PermissionKind::EventsList,
                PermissionKind::MetricsView,
            ],
            Self::Viewer => vec![
                PermissionKind::DashboardView,
                PermissionKind::TaskList,
                PermissionKind::TaskRead,
                PermissionKind::WorkerList,
                PermissionKind::ProjectRead,
                PermissionKind::EventsList,
            ],
        }
    }

    pub fn all() -> &'static [BuiltinRole] {
        &[Self::Admin, Self::Operator, Self::Viewer]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::all().iter().copied().find(|r| r.name() == name)
    }
}

// ── Bootstrap Token Purpose ─────────────────────────────────────────────

/// 부트스트랩 토큰 용도. 기존 `worker_join` 외에 admin 등록용 `admin_bootstrap`
/// 추가. 기존 `bootstrap_tokens` 테이블의 `purpose` 컬럼에 저장.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapPurpose {
    /// 기존 — `fleet worker join` 워커 가입.
    #[default]
    #[serde(rename = "worker_join")]
    WorkerJoin,
    /// 신규 — 웹 `/bootstrap` 첫 관리자 등록.
    #[serde(rename = "admin_bootstrap")]
    AdminBootstrap,
}

impl BootstrapPurpose {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WorkerJoin => "worker_join",
            Self::AdminBootstrap => "admin_bootstrap",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "worker_join" => Some(Self::WorkerJoin),
            "admin_bootstrap" => Some(Self::AdminBootstrap),
            _ => None,
        }
    }
}

// ── Errors ──────────────────────────────────────────────────────────────

/// 인증 도메인 에러.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid username format")]
    InvalidUsername,
    #[error("invalid email format")]
    InvalidEmail,
    #[error("password does not meet policy (min 8 chars, zxcvbn score >= 3)")]
    WeakPassword,
    #[error("argon2 hashing failed: {0}")]
    HashFailed(String),
    #[error("password hash parse failed: {0}")]
    HashParseFailed(String),
    #[error("session token mismatch")]
    InvalidSession,
    #[error("user not found")]
    UserNotFound,
    #[error("user disabled")]
    UserDisabled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_validation() {
        assert!(User::validate_username("admin").is_ok());
        assert!(User::validate_username("alice_operator-1").is_ok());
        assert!(User::validate_username("ab").is_err()); // too short
        assert!(User::validate_username("1abc").is_err()); // starts with digit
        assert!(User::validate_username("_abc").is_err()); // starts with underscore
        assert!(User::validate_username("a.b").is_err()); // dot not allowed
        assert!(User::validate_username(&"a".repeat(65)).is_err()); // too long
    }

    #[test]
    fn email_validation() {
        assert!(User::validate_email("admin@example.com").is_ok());
        assert!(User::validate_email("user.name+tag@domain.co.kr").is_ok());
        assert!(User::validate_email("a@b.c").is_ok());
        assert!(User::validate_email("").is_err()); // empty
        assert!(User::validate_email("noatsign.com").is_err()); // no @
        assert!(User::validate_email("@domain.com").is_err()); // empty local
        assert!(User::validate_email("user@").is_err()); // empty domain
        assert!(User::validate_email("user@domain").is_err()); // no dot in domain
        assert!(User::validate_email("user space@domain.com").is_err()); // space in local
    }

    #[test]
    fn permission_kind_roundtrip() {
        for &p in PermissionKind::all() {
            let s = p.as_str();
            assert_eq!(PermissionKind::from_name(s), Some(p));
        }
    }

    #[test]
    fn llm_credential_permissions_are_distinct_from_operational_credential() {
        // 로드맵 #66: worker operational identity(`fwo_` 토큰) 권한과 LLM
        // 프로바이더 API 키 권한이 같은 capability로 뭉개지면, 워커 등록용
        // 토큰을 가진 주체가 모든 워커의 API 키를 꺼낼 수 있게 된다.
        let names = [
            PermissionKind::WorkerCredentialManage.as_str(),
            PermissionKind::WorkerLlmCredentialRead.as_str(),
            PermissionKind::WorkerLlmCredentialExport.as_str(),
            PermissionKind::WorkerLlmCredentialManage.as_str(),
        ];
        let mut unique = names.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            names.len(),
            "capability 이름이 중복되면 안 된다"
        );

        // 기본 역할 중 admin만 평문 export 권한을 갖는다.
        for role in [BuiltinRole::Operator, BuiltinRole::Viewer] {
            let perms = role.permissions();
            assert!(
                !perms.contains(&PermissionKind::WorkerLlmCredentialExport),
                "{role:?}에게 LLM credential 평문 export 권한을 주면 안 된다"
            );
            assert!(!perms.contains(&PermissionKind::WorkerLlmCredentialManage));
        }
        assert!(BuiltinRole::Admin
            .permissions()
            .contains(&PermissionKind::WorkerLlmCredentialExport));
    }

    #[test]
    fn builtin_admin_has_all_permissions() {
        let admin_perms = BuiltinRole::Admin.permissions();
        assert_eq!(admin_perms.len(), PermissionKind::all().len());
    }

    #[test]
    fn builtin_viewer_is_read_only() {
        let viewer_perms = BuiltinRole::Viewer.permissions();
        assert!(viewer_perms.contains(&PermissionKind::TaskRead));
        assert!(!viewer_perms.contains(&PermissionKind::TaskCreate));
        assert!(!viewer_perms.contains(&PermissionKind::UserCreate));
    }

    #[test]
    fn builtin_operator_no_user_management() {
        let op_perms = BuiltinRole::Operator.permissions();
        assert!(op_perms.contains(&PermissionKind::TaskCreate));
        assert!(!op_perms.contains(&PermissionKind::UserCreate));
        assert!(!op_perms.contains(&PermissionKind::RoleCreate));
    }

    #[test]
    fn bootstrap_purpose_default_is_worker_join() {
        // 하위 호환성: 기존 행은 purpose가 없었으므로 worker_join.
        assert_eq!(BootstrapPurpose::default(), BootstrapPurpose::WorkerJoin);
    }

    #[test]
    fn bootstrap_purpose_roundtrip() {
        assert_eq!(
            BootstrapPurpose::parse_str("admin_bootstrap"),
            Some(BootstrapPurpose::AdminBootstrap)
        );
        assert_eq!(
            BootstrapPurpose::parse_str("worker_join"),
            Some(BootstrapPurpose::WorkerJoin)
        );
        assert_eq!(BootstrapPurpose::parse_str("unknown"), None);
    }
}
