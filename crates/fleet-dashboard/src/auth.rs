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
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use chrono::Utc;
use fleet_core::{PermissionKind, Session, User};

use crate::DashboardState;

/// 쿠키 이름.
pub const SESSION_COOKIE: &str = "fleet_session";

/// CSRF 토큰 쿠키 이름 (더블 서밋 패턴).
pub const CSRF_COOKIE: &str = "fleet_csrf";

/// CSRF 토큰 폼 필드 / 헤더 이름.
pub const CSRF_FIELD: &str = "csrf_token";

/// 세션 기본 만료 (8시간).
pub const SESSION_DURATION_SECS: i64 = 8 * 3600;

/// 세션 토큰 로테이션 주기 (초). 이 시간이 지난 세션은 다음 요청에서
/// 새 토큰으로 교체된다.
///
/// 고정 토큰은 8시간 내내 동일한 값이 유지되므로, 한 번 유출되면 만료까지
/// 그대로 사용 가능하다. 주기적으로 교체하면 유출된 토큰의 유효 창이 줄어들고,
/// 구 토큰이 계속 쓰이는 정황을 탐지할 여지도 생긴다.
///
/// **절대 수명은 연장하지 않는다** — 새 세션은 기존 `expires_at`을 그대로
/// 물려받으므로, 로그인 후 8시간이 지나면 로테이션 여부와 무관하게 만료된다.
pub const SESSION_ROTATE_AFTER_SECS: i64 = 30 * 60;

/// 로테이션 시 구 세션에 남겨두는 유예 시간 (초).
///
/// 구 세션을 즉시 삭제하면, 브라우저가 이미 병렬로 보낸 요청들(대시보드는
/// 페이지당 여러 API를 동시에 호출한다)이 전부 401을 맞고 로그아웃된다.
pub const SESSION_ROTATION_GRACE_SECS: i64 = 30;

/// 로그인 시도 실패 허용 한계 — identifier 단독 (사용자명당 5회).
pub const MAX_FAILED_ATTEMPTS: u64 = 5;

/// 로그인 시도 실패 허용 한계 — IP 단독 (IP당 20회, 다중 사용자 공유 고려).
pub const MAX_IP_FAILED_ATTEMPTS: u64 = 20;

/// 실패 잠금 윈도우 (최근 60초).
pub const FAILED_ATTEMPT_WINDOW_SECS: i64 = 60;

/// 이메일 발송 시도 실패 허용 한계 — identifier 단독 (사용자명당 3회).
pub const MAX_EMAIL_SEND_ATTEMPTS: u64 = 3;

/// 이메일 발송 시도 실패 허용 한계 — IP 단독 (IP당 10회).
pub const MAX_IP_EMAIL_SEND_ATTEMPTS: u64 = 10;

/// 이메일 발송 잠금 윈도우 (1시간 = 3600초).
pub const EMAIL_SEND_WINDOW_SECS: i64 = 3600;

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
) -> Result<Response, Response> {
    // 요청이 브라우저 페이지 네비게이션인지 API/fetch인지 판별.
    let is_api = is_api_request(&req);

    // 1. 쿠키 추출.
    let token = cookies
        .get(SESSION_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(|| auth_redirect(is_api, &state.base_path))?;

    // 2. 해시 + DB 조회.
    let hash = crate::auth_util::sha256_hex(token.as_bytes());
    let session = state
        .store
        .get_session_by_token_hash(&hash)
        .await
        .map_err(|_| internal_server_error(is_api))?
        .ok_or_else(|| auth_redirect(is_api, &state.base_path))?;

    // 3. 만료 확인.
    if session.is_expired() {
        state.store.delete_session(session.id).await.ok();
        return Err(auth_redirect(is_api, &state.base_path));
    }

    // 4. 사용자 로드.
    let user = state
        .store
        .get_user_by_id(session.user_id)
        .await
        .map_err(|_| internal_server_error(is_api))?
        .ok_or_else(|| auth_redirect(is_api, &state.base_path))?;

    // 5. 활성화 확인.
    if !user.enabled {
        return Err(forbidden_response(is_api, &state.base_path));
    }

    // 6. 권한 로드.
    let perm_rows = state
        .store
        .list_user_permissions(user.id)
        .await
        .map_err(|_| internal_server_error(is_api))?;
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

    // 8. 세션 IP 검증 (감사 목적 — 차단하지 않음, 경고만 로깅).
    //    정상적인 IP 변경(VPN, 모바일 네트워크 전환 등)을 차단하지 않지만,
    //    세션 공유/도용 탐지를 위한 감사 증거를 남김.
    if let Some(ref session_ip) = session.ip_address {
        if let Some(axum::extract::ConnectInfo(addr)) = req
            .extensions()
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        {
            let current_ip = extract_client_ip(req.headers(), *addr);
            if session_ip != &current_ip {
                tracing::warn!(
                    user_id = %principal.user.id,
                    username = %principal.user.username,
                    session_ip = %session_ip,
                    current_ip = %current_ip,
                    "session IP mismatch (possible session sharing)"
                );
            }
        }
    }

    // 9. CSRF 쿠키 갱신 — 기존 값이 있으면 재사용(그대로 유지), 없으면 새로
    //    발급한다. 응답을 만들기 전에 값을 정해둬야 next.run() 안의 핸들러가
    //    (드물게) 직접 쿠키를 참조하더라도 일관된다.
    //
    // FLEET FIX (2026-08-12): task_new_page를 비롯한 인증된 페이지 핸들러들은
    // 원래 fleet_csrf 쿠키를 전혀 재발급하지 않았다 — 로그인 시 한 번 발급된
    // 뒤로는 순수히 브라우저의 Max-Age에만 의존했다. 세션이 살아있는 한
    // fleet_csrf도 항상 존재하도록, 인증된 모든 요청에서 여기 한 곳에서
    // 갱신한다(개별 핸들러마다 반복하다 하나를 빠뜨리는 이전과 같은 실수를
    // 구조적으로 방지).
    let csrf_token = cookies
        .get(CSRF_COOKIE)
        .map(|c| c.value().to_string())
        .unwrap_or_else(crate::auth_util::generate_csrf_token);

    req.extensions_mut().insert(principal);

    // 10. 토큰 로테이션 판단은 응답 생성 **전에** 끝낸다 (session은 여기서 소비).
    let rotation = maybe_rotate_session(&state, &session).await;

    let mut response = next.run(req).await;

    // 11. CSRF 쿠키를 응답에 심는다 (매 요청 슬라이딩 갱신 — 세션과 수명 동기화).
    match Cookie::build((CSRF_COOKIE, csrf_token))
        .path("/")
        .http_only(false) // JS에서 읽어야 함 (더블 서밋 패턴)
        .secure(state.secure_cookies)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(SESSION_DURATION_SECS))
        .build()
        .to_string()
        .parse()
    {
        Ok(value) => {
            response
                .headers_mut()
                .append(axum::http::header::SET_COOKIE, value);
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to build refreshed CSRF cookie");
        }
    }

    // 12. 새 세션 토큰이 발급되었으면 응답에 쿠키를 심는다.
    if let Some(new_token) = rotation {
        match session_cookie_header(&state, &new_token) {
            Ok(value) => {
                response
                    .headers_mut()
                    .append(axum::http::header::SET_COOKIE, value);
            }
            Err(e) => {
                // 쿠키 헤더 생성 실패는 치명적이지 않다 — 구 토큰이 유예 기간
                // 동안 유효하므로 사용자는 로그아웃되지 않는다.
                tracing::error!(error = %e, "failed to build rotated session cookie");
            }
        }
    }

    Ok(response)
}

/// 로테이션 주기가 지난 세션이면 새 토큰을 발급하고 raw 토큰을 반환한다.
///
/// 실패해도 `None`을 반환할 뿐 요청을 막지 않는다 — 로테이션은 보안 강화
/// 조치이지 인증 자체가 아니므로, 실패로 사용자를 로그아웃시키면 안 된다.
async fn maybe_rotate_session(state: &DashboardState, session: &Session) -> Option<String> {
    let now = Utc::now();
    let grace_deadline = now + chrono::Duration::seconds(SESSION_ROTATION_GRACE_SECS);

    if !should_rotate(session.created_at, session.expires_at, now) {
        return None;
    }

    let (raw_token, token_hash) = fleet_core::auth::password::generate_session_token();
    let rotated = Session {
        id: fleet_core::SessionId::new(),
        user_id: session.user_id,
        token_hash,
        created_at: now,
        // 절대 수명 유지 — 로테이션이 세션을 무한 연장하면 안 된다.
        expires_at: session.expires_at,
        ip_address: session.ip_address.clone(),
        user_agent: session.user_agent.clone(),
    };

    if let Err(e) = state.store.create_session(&rotated).await {
        tracing::error!(error = %e, "session rotation: create_session failed");
        return None;
    }

    // 구 세션은 삭제하지 않고 짧은 유예만 남긴다 (병렬 요청 보호).
    if let Err(e) = state
        .store
        .update_session_expiry(session.id, grace_deadline)
        .await
    {
        tracing::warn!(error = %e, "session rotation: failed to shorten old session");
    }

    tracing::debug!(user_id = %session.user_id, "session token rotated");
    Some(raw_token)
}

/// 로테이션 여부 판단 (순수 함수 — 시간 주입으로 테스트 가능).
///
/// 두 조건을 모두 만족해야 로테이션한다:
/// 1. 세션 생성 후 [`SESSION_ROTATE_AFTER_SECS`]가 지났다.
/// 2. 남은 수명이 유예 기간보다 길다 — 이미 유예 상태로 들어간 세션은 다른
///    요청이 방금 로테이션한 구 세션이므로 다시 돌리지 않는다. 만료 직전
///    세션을 로테이션해봤자 새 토큰도 곧바로 만료되므로 무의미하기도 하다.
fn should_rotate(
    created_at: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
    now: chrono::DateTime<Utc>,
) -> bool {
    if now - created_at < chrono::Duration::seconds(SESSION_ROTATE_AFTER_SECS) {
        return false;
    }
    expires_at > now + chrono::Duration::seconds(SESSION_ROTATION_GRACE_SECS)
}

/// 로테이션된 세션 쿠키의 `Set-Cookie` 헤더 값 생성.
fn session_cookie_header(
    state: &DashboardState,
    token: &str,
) -> Result<axum::http::HeaderValue, axum::http::header::InvalidHeaderValue> {
    // 로그인 시 발급하는 쿠키와 동일한 속성이어야 한다.
    // FLEET FIX (2026-08-12): 9c7560e에서 로그인 쿠키만 SameSite=Lax로 바꾸고
    // 이 로테이션 쿠키는 Strict로 남아 있던 누락분을 맞춤.
    let secure = if state.secure_cookies { "; Secure" } else { "" };
    let value =
        format!("{SESSION_COOKIE}={token}; Path=/; HttpOnly{secure}; SameSite=Lax; Max-Age={SESSION_DURATION_SECS}");
    axum::http::HeaderValue::from_str(&value)
}

// ── 인증 에러 응답 헬퍼 ──────────────────────────────────────────────────

/// 요청이 API/fetch인지 브라우저 네비게이션인지 판별.
fn is_api_request(req: &Request) -> bool {
    // Accept 헤더가 JSON을 요구하거나 X-Requested-With 헤더가 있으면 API.
    if let Some(accept) = req.headers().get("accept") {
        if let Ok(v) = accept.to_str() {
            if v.contains("application/json") {
                return true;
            }
        }
    }
    req.headers().contains_key("x-requested-with")
}

/// 인증 실패: 브라우저면 /login 리다이렉트, API면 401 JSON.
///
/// `base_path`는 리버스 프록시 마운트 prefix(`DashboardState::base_path`) — 리다이렉트
/// 목적지와 JSON 바디 안의 `redirect` 필드 둘 다에 반영해야, prefix 뒤에 마운트된
/// 배포에서도 프론트엔드가 올바른 절대경로로 다시 이동한다.
fn auth_redirect(is_api: bool, base_path: &str) -> Response {
    if is_api {
        (
            StatusCode::UNAUTHORIZED,
            [("content-type", "application/json")],
            format!(r#"{{"error":"unauthorized","redirect":"{base_path}/login"}}"#),
        )
            .into_response()
    } else {
        Redirect::to(&format!("{base_path}/login")).into_response()
    }
}

/// 403: 브라우저면 /login 리다이렉트 (메시지 포함), API면 403 JSON.
fn forbidden_response(is_api: bool, base_path: &str) -> Response {
    if is_api {
        (
            StatusCode::FORBIDDEN,
            [("content-type", "application/json")],
            r#"{"error":"forbidden","reason":"account disabled"}"#,
        )
            .into_response()
    } else {
        Redirect::to(&format!("{base_path}/login?reason=disabled")).into_response()
    }
}

/// 500: 내부 서버 오류.
fn internal_server_error(is_api: bool) -> Response {
    if is_api {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "application/json")],
            r#"{"error":"internal_server_error"}"#,
        )
            .into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "text/html; charset=utf-8")],
            "<html><body><h1>500 Internal Server Error</h1><p>Please try again later.</p></body></html>",
        )
            .into_response()
    }
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
/// **이중 제한 (Phase 9.1.7 보안 패치):**
/// - **identifier 단독**: 같은 사용자명에 대해 최근 N회 실패 (IP 무관) → credential stuffing 방어
/// - **IP 단독**: 같은 IP에서 최근 M회 실패 (사용자 무관) → IP 회전 브루트포스 방어
///
/// 어느 하나라도 초과하면 차단.
///
/// **호출자 계약:** 이 함수는 카운터를 *읽기만* 한다. 호출하는 핸들러는
/// 반드시 실제 실패 지점(또는 열거 방지 엔드포인트라면 모든 요청)에서
/// [`record_login_failure`] / [`record_rate_limited_request`]로 카운터를
/// 증가시켜야 한다. 기록 지점이 이미 차단된 `if !allowed` 블록 안에만 있으면
/// 카운터가 영원히 0이라 차단 분기에 도달할 수 없다.
pub async fn check_rate_limit(
    state: &DashboardState,
    identifier: &str,
    ip: Option<&str>,
) -> Result<bool, StatusCode> {
    check_rate_limit_custom(
        state,
        identifier,
        ip,
        MAX_FAILED_ATTEMPTS,
        MAX_IP_FAILED_ATTEMPTS,
        FAILED_ATTEMPT_WINDOW_SECS,
    )
    .await
}

pub async fn check_rate_limit_custom(
    state: &DashboardState,
    identifier: &str,
    ip: Option<&str>,
    max_id_attempts: u64,
    max_ip_attempts: u64,
    window_secs: i64,
) -> Result<bool, StatusCode> {
    // 1. identifier 단독 카운트
    let id_count = state
        .store
        .count_recent_failed_attempts(identifier, None, window_secs)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if id_count >= max_id_attempts {
        return Ok(false);
    }
    // 2. IP 단독 카운트
    if let Some(ip) = ip {
        let ip_count = state
            .store
            .count_recent_ip_failures(ip, window_secs)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if ip_count >= max_ip_attempts {
            return Ok(false);
        }
    }
    Ok(true)
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

/// 남용 방지 카운터에 시도 1건 기록 (로그인 이외 엔드포인트용).
///
/// `/forgot-password`, `/resend-verification` 처럼 **성공/실패를 응답으로
/// 구분하지 않는**(계정 열거 방지) 엔드포인트는 요청 자체가 남용 단위다.
/// 따라서 실패 경로가 아니라 **모든 요청**에서 호출해야 카운터가 증가하고
/// [`check_rate_limit`]의 차단 분기가 도달 가능해진다.
///
/// 내부적으로는 [`record_login_failure`]와 동일하게 `login_attempts`에
/// `success = FALSE` 행을 남긴다 — `count_recent_failed_attempts`가 세는 대상.
pub async fn record_rate_limited_request(
    state: &DashboardState,
    identifier: &str,
    ip: Option<&str>,
    reason: &str,
) -> Result<(), StatusCode> {
    record_login_failure(state, identifier, ip, reason).await
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
        .clear_login_attempts(identifier, None)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 기회적 정리 — 7일 이상 된 로그인 시도 기록 삭제 (테이블 무한 증가 방지).
    let cutoff = Utc::now() - chrono::Duration::days(7);
    state.store.delete_old_login_attempts(cutoff).await.ok();

    Ok(())
}

pub fn extract_client_ip(
    headers: &axum::http::HeaderMap,
    peer_addr: std::net::SocketAddr,
) -> String {
    let trusted_proxies: Vec<std::net::IpAddr> = std::env::var("FLEET_TRUSTED_PROXIES")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim())
        .filter_map(|s| s.parse::<std::net::IpAddr>().ok())
        .collect();

    let peer_ip = peer_addr.ip();

    if trusted_proxies.contains(&peer_ip) {
        if let Some(cf_ip) = headers
            .get("cf-connecting-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<std::net::IpAddr>().ok())
        {
            return cf_ip.to_string();
        }

        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            let parts: Vec<&str> = xff.split(',').map(|s| s.trim()).collect();
            for part in parts.iter().rev() {
                if let Ok(ip) = part.parse::<std::net::IpAddr>() {
                    if !trusted_proxies.contains(&ip) {
                        return ip.to_string();
                    }
                }
            }
            if let Some(first) = parts.first() {
                if let Ok(ip) = first.parse::<std::net::IpAddr>() {
                    return ip.to_string();
                }
            }
        }
    }

    peer_ip.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: i64) -> chrono::DateTime<Utc> {
        chrono::DateTime::from_timestamp(1_800_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn fresh_session_is_not_rotated() {
        let created = t(0);
        let expires = created + chrono::Duration::seconds(SESSION_DURATION_SECS);
        // 로테이션 주기 직전.
        let now = created + chrono::Duration::seconds(SESSION_ROTATE_AFTER_SECS - 1);
        assert!(!should_rotate(created, expires, now));
    }

    #[test]
    fn session_older_than_rotation_interval_is_rotated() {
        let created = t(0);
        let expires = created + chrono::Duration::seconds(SESSION_DURATION_SECS);
        let now = created + chrono::Duration::seconds(SESSION_ROTATE_AFTER_SECS);
        assert!(should_rotate(created, expires, now));
    }

    /// 이미 유예 기간으로 단축된 구 세션은 다시 로테이션하지 않는다
    /// (병렬 요청이 세션 행을 계속 불려나가는 것을 방지).
    #[test]
    fn session_already_in_grace_window_is_not_rotated() {
        let created = t(0);
        let now = created + chrono::Duration::seconds(SESSION_ROTATE_AFTER_SECS + 10);
        let expires = now + chrono::Duration::seconds(SESSION_ROTATION_GRACE_SECS);
        assert!(!should_rotate(created, expires, now));
    }

    /// 만료가 임박한 세션은 로테이션해도 새 토큰이 곧 죽으므로 건너뛴다.
    #[test]
    fn nearly_expired_session_is_not_rotated() {
        let created = t(0);
        let now = created + chrono::Duration::seconds(SESSION_DURATION_SECS - 5);
        let expires = created + chrono::Duration::seconds(SESSION_DURATION_SECS);
        assert!(!should_rotate(created, expires, now));
    }

    /// 로테이션은 절대 수명을 연장하지 않는다 — 새 세션은 기존 만료 시각을
    /// 그대로 물려받으므로, 반복 로테이션해도 최초 로그인 기준 8시간에 끝난다.
    #[test]
    fn rotation_preserves_absolute_expiry() {
        let login = t(0);
        let absolute_expiry = login + chrono::Duration::seconds(SESSION_DURATION_SECS);

        // 30분 간격으로 계속 로테이션되는 상황을 모사.
        let mut created = login;
        let mut rotations = 0;
        loop {
            let now = created + chrono::Duration::seconds(SESSION_ROTATE_AFTER_SECS);
            if !should_rotate(created, absolute_expiry, now) {
                break;
            }
            // 새 세션의 created_at만 갱신되고 expires_at은 유지된다.
            created = now;
            rotations += 1;
            assert!(rotations < 100, "무한 로테이션 — 절대 수명이 연장되고 있다");
        }

        assert!(rotations > 0, "적어도 한 번은 로테이션되어야 한다");
        // 마지막 세션도 최초 로그인 기준 만료 시각을 넘기지 못한다.
        assert!(created < absolute_expiry);
    }
}
