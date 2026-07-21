//! RBAC 시드 + 부트스트랩 OTP 자동 발급 (Phase 9.1.1).
//!
//! 부트 시 자동으로 호출되어 다음을 수행:
//!
//! 1. **권한 카탈로그 동기화** — `PermissionKind::all()`의 각 항목을
//!    `permissions` 테이블에 upsert (idempotent).
//! 2. **builtin 역할 동기화** — admin/operator/viewer를 `roles`에 upsert.
//! 3. **역할-권한 매핑** — 각 builtin 역할에 매핑된 권한을 `role_permissions`에 삽입.
//! 4. **부트스트랩 OTP 발급** — `users` 테이블이 비어 있고 활성 admin_bootstrap
//!    토큰이 없으면 1회용 토큰을 자동 발급.
//!
//! 모든 작업은 idempotent하므로, fleet serve가 여러 번 재시작되어도 안전.

use chrono::{Duration, Utc};
use tracing::info;
use uuid::Uuid;

use fleet_core::auth::password::generate_session_token;
use fleet_core::{
    BootstrapPurpose, BootstrapToken, BuiltinRole, Permission, PermissionId, PermissionKind, Role,
    RoleId, User,
};

use crate::{Store, StoreError};

/// 시드 + 부트스트랩 토큰 발급을 모두 수행.
///
/// 반환: 발급된 admin_bootstrap 토큰 (있으면). 없거나 이미 활성 사용자가 있으면 None.
pub async fn seed_rbac_and_maybe_issue_bootstrap(
    store: &dyn Store,
) -> Result<Option<String>, StoreError> {
    seed_permissions(store).await?;
    seed_builtin_roles(store).await?;

    // users 테이블이 비어있는 경우에만 부트스트랩 토큰 발급 고려.
    let user_count = store.count_users().await?;
    if user_count > 0 {
        // 이미 활성 사용자가 있으면 발급 불필요.
        return Ok(None);
    }

    issue_admin_bootstrap_if_needed(store).await
}

/// `PermissionKind::all()`를 permissions 테이블에 upsert.
pub async fn seed_permissions(store: &dyn Store) -> Result<(), StoreError> {
    for &kind in PermissionKind::all() {
        // 이미 존재하면 skip하기 위해 name으로 조회.
        if store.get_permission_by_name(kind.as_str()).await?.is_some() {
            continue;
        }
        store
            .create_permission(&Permission {
                id: PermissionId::new(),
                name: kind.as_str().to_string(),
                description: Some(permission_description(kind)),
            })
            .await?;
    }
    Ok(())
}

fn permission_description(kind: PermissionKind) -> String {
    match kind {
        PermissionKind::DashboardView => "View the dashboard",
        PermissionKind::TaskList => "List tasks",
        PermissionKind::TaskCreate => "Dispatch new tasks",
        PermissionKind::TaskCancel => "Cancel running tasks",
        PermissionKind::TaskRead => "Read task details",
        PermissionKind::TaskOutput => "Read task output streams",
        PermissionKind::WorkerList => "List registered workers",
        PermissionKind::WorkerRegister => "Register new workers",
        PermissionKind::WorkerDelete => "Delete/deregister workers",
        PermissionKind::TokenIssue => "Issue bootstrap tokens",
        PermissionKind::TokenList => "List bootstrap tokens",
        PermissionKind::TokenRevoke => "Revoke bootstrap tokens",
        PermissionKind::UserCreate => "Create new users",
        PermissionKind::UserDelete => "Delete users",
        PermissionKind::UserRead => "List/read users",
        PermissionKind::UserRoleAssign => "Assign roles to users",
        PermissionKind::UserRoleRevoke => "Revoke roles from users",
        PermissionKind::RoleCreate => "Create custom roles",
        PermissionKind::RoleDelete => "Delete roles",
        PermissionKind::AuditRead => "Read audit log",
        PermissionKind::EventsList => "List events",
        PermissionKind::MetricsView => "View metrics",
    }
    .to_string()
}

/// admin/operator/viewer 역할과 매핑을 동기화.
pub async fn seed_builtin_roles(store: &dyn Store) -> Result<(), StoreError> {
    for &builtin in BuiltinRole::all() {
        let role = if let Some(existing) = store.get_role_by_name(builtin.name()).await? {
            existing
        } else {
            let role = Role {
                id: RoleId::new(),
                name: builtin.name().to_string(),
                description: Some(builtin.description().to_string()),
                builtin: true,
                created_at: Utc::now(),
            };
            store.create_role(&role).await?;
            // 재조회로 안정적인 id 획득 (create에서 충돌 시).
            store
                .get_role_by_name(builtin.name())
                .await?
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "builtin role {} not found after insert",
                        builtin.name()
                    ))
                })?
        };

        // 역할-권한 매핑 동기화.
        for kind in builtin.permissions() {
            let perm = store
                .get_permission_by_name(kind.as_str())
                .await?
                .ok_or_else(|| {
                    StoreError::Decode(format!(
                        "permission {} not seeded before role mapping",
                        kind.as_str()
                    ))
                })?;
            store.grant_role_permission(role.id, perm.id).await?;
        }
    }
    Ok(())
}

/// 활성 admin_bootstrap 토큰이 없으면 1회용 토큰 발급.
///
/// 반환: 발급된 토큰 문자열 (이미 활성 토큰이 있으면 None).
pub async fn issue_admin_bootstrap_if_needed(
    store: &dyn Store,
) -> Result<Option<String>, StoreError> {
    // 활성 admin_bootstrap 토큰이 이미 있으면 중복 발급 방지.
    let existing = store.list_bootstrap_tokens().await?;
    let has_active = existing.iter().any(|t| {
        // purpose 컬럼이 추가되었지만, 기존 BootstrapToken 도메인에는 아직 없음.
        // 일단 사용 가능 여부만 판단. (purpose 확장은 Phase 9.1.3에서.)
        t.is_usable()
    });
    if has_active {
        return Ok(None);
    }

    // 32바이트 난수 토큰 (session token과 동일한 엔트로피).
    let (token_value, _hash) = generate_session_token();
    let prefixed = format!("fleet_boot_{}", token_value);

    let token = BootstrapToken {
        token: prefixed.clone(),
        created_at: Utc::now(),
        created_by: Some("system".to_string()),
        expires_at: Some(Utc::now() + Duration::hours(24)),
        max_uses: 1,
        use_count: 0,
        notes: Some("Auto-issued admin bootstrap token. Use at /bootstrap.".to_string()),
        last_used_by: None,
        last_used_at: None,
    };
    store.create_bootstrap_token(&token).await?;

    info!(
        token_prefix = %&prefixed[..12],
        "auto-issued admin bootstrap token (use at /bootstrap)"
    );
    Ok(Some(prefixed))
}

/// 웹 `/bootstrap` 페이지에서 사용자가 OTP + 신규 관리자 정보 제출 시 호출.
///
/// 1. 토큰 검증 + 소비 (atomic)
/// 2. 사용자 생성
/// 3. admin 역할 부여
/// 4. 첫 세션 발급 (자동 로그인)
///
/// 반환: (user, session_token). session_token은 쿠키로 설정.
pub async fn consume_bootstrap_and_create_admin(
    store: &dyn Store,
    otp_token: &str,
    user: User,
    password_hash: String,
) -> Result<(fleet_core::User, String, String), BootstrapAdminError> {
    use fleet_core::auth::password::generate_session_token;

    // 1. 토큰 소비 (atomic).
    store
        .consume_bootstrap_token(otp_token, &user.username)
        .await
        .map_err(BootstrapAdminError::InvalidToken)?;

    // 2. 사용자 생성 (비밀번호 해시 적용).
    let mut new_user = user;
    new_user.password_hash = password_hash;
    store
        .create_user(&new_user)
        .await
        .map_err(BootstrapAdminError::CreateUser)?;

    // 3. admin 역할 부여.
    let admin_role = store
        .get_role_by_name(BuiltinRole::Admin.name())
        .await
        .map_err(BootstrapAdminError::Store)?
        .ok_or(BootstrapAdminError::AdminRoleMissing)?;
    store
        .assign_user_role(new_user.id, admin_role.id, None)
        .await
        .map_err(BootstrapAdminError::Store)?;

    // 4. 첫 세션 발급.
    let (session_token, token_hash) = generate_session_token();
    let session = fleet_core::Session {
        id: fleet_core::SessionId::new(),
        user_id: new_user.id,
        token_hash,
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(8),
        ip_address: None,
        user_agent: None,
    };
    store
        .create_session(&session)
        .await
        .map_err(BootstrapAdminError::Store)?;
    store
        .update_user_last_login(new_user.id, Utc::now())
        .await
        .ok();

    info!(
        user_id = %new_user.id,
        username = %new_user.username,
        "admin bootstrap completed"
    );
    Ok((new_user, session_token, session.id.as_uuid().to_string()))
}

/// 부트스트랩 관리자 등록 에러.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapAdminError {
    #[error("invalid or expired bootstrap token")]
    InvalidToken(#[source] StoreError),
    #[error("failed to create user (username may be taken)")]
    CreateUser(#[source] StoreError),
    #[error("admin role not found — run seed_rbac first")]
    AdminRoleMissing,
    #[error("store error: {0}")]
    Store(#[source] StoreError),
}

/// 감사 로그용 — 임의 UUID 기반 identifier (사용자에게 노출되지 않는 추적 ID).
#[allow(dead_code)]
pub fn new_audit_id() -> Uuid {
    Uuid::new_v4()
}

/// bootstrap_tokens 테이블의 purpose 컬럼 활용을 위한 임시 헬퍼.
///
/// TODO(9.1.3): BootstrapToken 도메인에 purpose 필드를 추가하면 제거.
#[allow(dead_code)]
pub fn classify_bootstrap_token(token: &BootstrapToken) -> BootstrapPurpose {
    // notes에 admin bootstrap 마커가 있으면 admin_bootstrap으로 추정.
    // 마이그레이션 004 이후 신규 발급분은 purpose 컬럼으로 구분.
    match token.notes.as_deref() {
        Some(n) if n.contains("admin bootstrap") => BootstrapPurpose::AdminBootstrap,
        _ => BootstrapPurpose::WorkerJoin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_description_covers_all() {
        // 모든 PermissionKind가 설명을 가져야 함 (panic 방지).
        for &kind in PermissionKind::all() {
            let desc = permission_description(kind);
            assert!(!desc.is_empty(), "{:?} has empty description", kind);
        }
    }

    #[test]
    fn builtin_roles_cover_all_permissions() {
        // admin은 모든 권한을 가져야 함.
        assert_eq!(
            BuiltinRole::Admin.permissions().len(),
            PermissionKind::all().len()
        );
    }
}
