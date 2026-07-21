//! 대시보드 인증 미들웨어 (Phase 9.1.2).
//!
//! `require_session` — 쿠키 기반 세션 검증 + AuthPrincipal 주입.
//! `require_permission` — 권한 검사 헬퍼.
//!
//! ## 보안 속성
//!
//! - 쿠키 토큰은 SHA-256 해시로 DB 조회 (재현 불가).
//! - 만료된 세션은 자동 삭제.
//! - 비활성 사용자는 401 (UNAUTHORIZED)이 아닌 403 (FORBIDDEN)으로 차단 — UI 차별화.
//! - `AuthPrincipal`을 `Extension`에 주입하여 handler에서 권한 검사 가능.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use axum_extra::extract::CookieJar;
use chrono::Utc;
use fleet_core::{PermissionKind, User};

use crate::DashboardState;

/// 쿠키 이름.
pub const SESSION_COOKIE: &str = "fleet_session";

/// 세션 기본 만료 (8시간).
pub const SESSION_DURATION_SECS: i64 = 8 * 3600;

/// 로그인 시도 실패 허용 한계 (5회).
pub const MAX_FAILED_ATTEMPTS: u64 = 5;

/// 실패 잠금 윈도우 (최근 60초).
pub const FAILED_ATTEMPT_WINDOW_SECS: i64 = 60;

/// 인증된 사용자 컨텍스트 (handler에서 권한 검사에 사용).
#[derive(Debug, Clone)]
pub struct AuthPrincipal {
    pub user: User,
    /// 권한 이름 집합 (빠른 membership 검사).
    pub permissions: Vec<PermissionKind>,
    pub session_id: fleet_core::SessionId,
}

impl AuthPrincipal {
    /// 권한 보유 여부.
    pub fn has(&self, perm: PermissionKind) -> bool {
        self.permissions.contains(&perm)
    }
}

/// 보호된 경로에 적용할 미들웨어.
///
/// 순서:
/// 1. 쿠키에서 세션 토큰 추출
/// 2. SHA-256 해시 → DB 조회
/// 3. 만료 확인 (지난 세션은 삭제)
/// 4. 사용자 + 권한 로드
/// 5. 활성화 여부 확인
/// 6. AuthPrincipal Extension 주입
pub async fn require_session(
    State(state): State<Arc<DashboardState>>,
    cookies: CookieJar,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // 1. 쿠키 추출.
    let token = cookies
        .get(SESSION_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // 2. 해시 + DB 조회.
    let hash = crate::auth_util::sha256_hex(token.as_bytes());
    let session = state
        .store
        .get_session_by_token_hash(&hash)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // 3. 만료 확인.
    if session.is_expired() {
        state.store.delete_session(session.id).await.ok();
        return Err(StatusCode::UNAUTHORIZED);
    }

    // 4. 사용자 로드.
    let user = state
        .store
        .get_user_by_id(session.user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // 5. 활성화 확인.
    if !user.enabled {
        return Err(StatusCode::FORBIDDEN);
    }

    // 6. 권한 로드.
    let perm_rows = state
        .store
        .list_user_permissions(user.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let permissions: Vec<PermissionKind> = perm_rows
        .iter()
        .filter_map(|p| PermissionKind::from_name(&p.name))
        .collect();

    // 7. AuthPrincipal 주입.
    let principal = AuthPrincipal {
        user,
        permissions,
        session_id: session.id,
    };
    req.extensions_mut().insert(principal);

    Ok(next.run(req).await)
}

/// 권한 검사 헬퍼. handler에서 사용.
///
/// ```ignore
/// pub async fn delete_worker(
///     Extension(principal): Extension<AuthPrincipal>,
///     // ...
/// ) -> Result<..., StatusCode> {
///     require_permission(&principal, PermissionKind::WorkerDelete)?;
///     // ...
/// }
/// ```
pub fn require_permission(
    principal: &AuthPrincipal,
    perm: PermissionKind,
) -> Result<(), StatusCode> {
    if principal.has(perm) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// 로그인 시도 가능 여부 (rate limit 판정).
///
/// 최근 `FAILED_ATTEMPT_WINDOW_SECS`초 내 실패가 `MAX_FAILED_ATTEMPTS` 이상이면 false.
pub async fn check_rate_limit(
    state: &DashboardState,
    identifier: &str,
    ip: Option<&str>,
) -> Result<bool, StatusCode> {
    let count = state
        .store
        .count_recent_failed_attempts(identifier, ip, FAILED_ATTEMPT_WINDOW_SECS)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(count < MAX_FAILED_ATTEMPTS)
}

/// 로그인 실패 시 기록 + rate limit 도달 여부 반환.
pub async fn record_login_failure(
    state: &DashboardState,
    identifier: &str,
    ip: Option<&str>,
    reason: &str,
) -> Result<(), StatusCode> {
    use uuid::Uuid;
    let attempt = fleet_core::LoginAttempt {
        id: Uuid::new_v4(),
        identifier: identifier.to_string(),
        ip_address: ip.map(|s| s.to_string()),
        success: false,
        failure_reason: Some(reason.to_string()),
        attempted_at: Utc::now(),
    };
    state
        .store
        .record_login_attempt(&attempt)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

/// 로그인 성공 시 기존 실패 기록 초기화.
pub async fn record_login_success(
    state: &DashboardState,
    identifier: &str,
    ip: Option<&str>,
) -> Result<(), StatusCode> {
    use uuid::Uuid;
    let attempt = fleet_core::LoginAttempt {
        id: Uuid::new_v4(),
        identifier: identifier.to_string(),
        ip_address: ip.map(|s| s.to_string()),
        success: true,
        failure_reason: None,
        attempted_at: Utc::now(),
    };
    state
        .store
        .record_login_attempt(&attempt)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    state
        .store
        .clear_login_attempts(identifier, ip)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}
