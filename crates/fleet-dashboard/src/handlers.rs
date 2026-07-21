//! 대시보드 API 핸들러.
//!
//! 모든 엔드포인트는 `Store`에서 데이터를 조회하여 JSON으로 반환합니다.
//! `/api/overview`는 집계 카운트를, `/api/workers`와 `/api/tasks`는 페이지네이션된
//! 목록을 제공합니다.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use tracing::debug;

use fleet_core::{TaskFilter, TaskStatus, WorkerFilter};

use crate::app::DashboardState;
use crate::schema::{OverviewResponse, TaskCounts, TaskSummary, WorkerCounts, WorkerSummary};

/// `/health` — 헬스체크.
pub async fn health() -> &'static str {
    "ok"
}

/// `/api/overview` — 요약 통계.
pub async fn overview(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<OverviewResponse>, StatusCode> {
    let workers = state
        .store
        .list_workers(&WorkerFilter::default())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "overview: list_workers failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let mut counts = WorkerCounts::default();
    for w in &workers {
        counts.total += 1;
        match w.status {
            fleet_core::WorkerStatus::Online => counts.online += 1,
            fleet_core::WorkerStatus::Degraded => counts.degraded += 1,
            fleet_core::WorkerStatus::Offline => counts.offline += 1,
            fleet_core::WorkerStatus::CircuitOpen => counts.circuit_open += 1,
        }
    }

    let tasks = state
        .store
        .list_tasks(&TaskFilter {
            limit: 1000,
            ..Default::default()
        })
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "overview: list_tasks failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let mut task_counts = TaskCounts::default();
    for t in &tasks {
        task_counts.total += 1;
        match &t.status {
            TaskStatus::Pending => task_counts.pending += 1,
            TaskStatus::Dispatched { .. } => task_counts.dispatched += 1,
            TaskStatus::Completed(_) => task_counts.completed += 1,
            TaskStatus::Failed(_) => task_counts.failed += 1,
            TaskStatus::Cancelled { .. } => task_counts.cancelled += 1,
        }
    }

    Ok(Json(OverviewResponse {
        workers: counts,
        tasks: task_counts,
        generated_at: Utc::now(),
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct ListWorkersQuery {
    pub status: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    100
}

/// `/api/workers` — 워커 목록.
pub async fn list_workers(
    State(state): State<Arc<DashboardState>>,
    Query(q): Query<ListWorkersQuery>,
) -> Result<Json<Vec<WorkerSummary>>, StatusCode> {
    let mut filter = WorkerFilter::default();
    if let Some(s) = &q.status {
        filter.status = parse_worker_status(s);
    }
    filter.limit = q.limit;

    let workers = state.store.list_workers(&filter).await.map_err(|e| {
        tracing::error!(error = %e, "list_workers failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let summaries = workers.iter().map(worker_to_summary).collect();
    Ok(Json(summaries))
}

#[derive(Debug, serde::Deserialize)]
pub struct ListTasksQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// `/api/tasks` — 작업 목록.
pub async fn list_tasks(
    State(state): State<Arc<DashboardState>>,
    Query(q): Query<ListTasksQuery>,
) -> Result<Json<Vec<TaskSummary>>, StatusCode> {
    let filter = TaskFilter {
        limit: q.limit,
        ..Default::default()
    };
    let tasks = state.store.list_tasks(&filter).await.map_err(|e| {
        tracing::error!(error = %e, "list_tasks failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let summaries: Vec<TaskSummary> = tasks.iter().map(task_to_summary).collect();
    debug!(count = summaries.len(), "list_tasks");
    Ok(Json(summaries))
}

#[derive(Debug, serde::Deserialize)]
pub struct ListEventsQuery {
    #[serde(default)]
    pub after_seq: u64,
    #[serde(default = "default_event_limit")]
    pub limit: u32,
}

fn default_event_limit() -> u32 {
    100
}

/// `/api/events` — 이벤트 로그 (페이지네이션).
pub async fn list_events(
    State(state): State<Arc<DashboardState>>,
    Query(q): Query<ListEventsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let events = state
        .store
        .list_events(q.after_seq, q.limit)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "list_events failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(serde_json::json!({
        "events": events,
        "count": events.len(),
    })))
}

/// `/` — 대시보드 HTML 페이지 (임베드된 자산).
pub async fn index() -> Response {
    match crate::assets::Asset::get("index.html") {
        Some(file) => {
            let body = file.data;
            (
                [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                body,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "dashboard not built").into_response(),
    }
}

/// `/static/*path` — 정적 자산.
pub async fn static_asset(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    let cleaned = path.trim_start_matches('/');
    let full = if cleaned.is_empty() {
        "index.html"
    } else {
        cleaned
    };
    match crate::assets::Asset::get(full) {
        Some(file) => {
            let mime = file.metadata.mimetype();
            ([(axum::http::header::CONTENT_TYPE, mime)], file.data).into_response()
        }
        None => (StatusCode::NOT_FOUND, "asset not found").into_response(),
    }
}

// ── 헬퍼 ────────────────────────────────────────────────────────────────

fn parse_worker_status(s: &str) -> Option<fleet_core::WorkerStatus> {
    match s {
        "online" => Some(fleet_core::WorkerStatus::Online),
        "degraded" => Some(fleet_core::WorkerStatus::Degraded),
        "offline" => Some(fleet_core::WorkerStatus::Offline),
        "circuit_open" => Some(fleet_core::WorkerStatus::CircuitOpen),
        _ => None,
    }
}

fn worker_to_summary(w: &fleet_core::Worker) -> WorkerSummary {
    WorkerSummary {
        id: w.id.to_string(),
        name: w.name.clone(),
        endpoint: w.endpoint.clone(),
        status: WorkerSummary::status_str(w.status).to_string(),
        labels: w.labels.clone(),
        active_tasks: w.active_tasks,
        max_concurrent: w.max_concurrent,
        circuit_state: format!("{:?}", w.circuit_state).to_lowercase(),
        last_seen: w.last_seen,
        registered_at: w.registered_at,
    }
}

fn task_to_summary(t: &fleet_core::Task) -> TaskSummary {
    let (phase, worker_id, exit_code, duration_secs) = match &t.status {
        TaskStatus::Pending => ("pending", None, None, None),
        TaskStatus::Dispatched { worker_id, .. } => {
            ("dispatched", Some(worker_id.to_string()), None, None)
        }
        TaskStatus::Completed(r) => (
            "completed",
            Some(r.worker_id.to_string()),
            Some(r.exit_code),
            Some(r.duration_secs),
        ),
        TaskStatus::Failed(f) => ("failed", f.worker_id.map(|w| w.to_string()), None, None),
        TaskStatus::Cancelled { .. } => ("cancelled", None, None, None),
    };
    TaskSummary {
        id: t.id.to_string(),
        phase: phase.into(),
        prompt: t.prompt.clone(),
        created_at: t.created_at,
        created_by: t.created_by.clone(),
        worker_id,
        exit_code,
        duration_secs,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  인증 핸들러 (Phase 9.1.2)
// ═══════════════════════════════════════════════════════════════════════

use axum::{
    extract::{ConnectInfo, Extension},
    response::Redirect,
    Form,
};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use chrono::Duration;
use fleet_core::auth::password::{generate_session_token, verify_password};
use fleet_core::{Session, SessionId};
use serde::Deserialize;
use std::net::SocketAddr;

use crate::assets::Asset;
use crate::auth::{
    check_rate_limit, record_login_failure, record_login_success, AuthPrincipal, SESSION_COOKIE,
    SESSION_DURATION_SECS,
};

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

/// 로그인 페이지 HTML.
pub async fn login_page(State(_state): State<Arc<DashboardState>>) -> Response {
    let asset = Asset::get("login.html")
        .map(|a| a.data.to_vec())
        .unwrap_or_else(|| include_bytes!("../assets/login.html").to_vec());
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        asset,
    )
        .into_response()
}

/// POST /login — 폼 제출 처리.
///
/// 성공: 쿠키 설정 + `/` 리다이렉트.
/// 실패: 401 + login.html 재렌더 (에러 메시지 포함).
pub async fn login(
    State(state): State<Arc<DashboardState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> Result<(CookieJar, Redirect), (CookieJar, Response)> {
    let ip = addr.ip().to_string();

    // rate limit 검사.
    let allowed = check_rate_limit(&state, &form.username, Some(&ip))
        .await
        .map_err(|_| (jar.clone(), internal_error_page()))?;
    if !allowed {
        record_login_failure(&state, &form.username, Some(&ip), "rate_limited")
            .await
            .ok();
        return Err((
            jar,
            login_failed_page("Too many attempts. Try again in 60s."),
        ));
    }

    // 사용자 조회 (타이밍 공격 방지: 사용자 없어도 동일한 시간 소모).
    let user = state
        .store
        .get_user_by_username(&form.username)
        .await
        .map_err(|_| (jar.clone(), internal_error_page()))?;

    let valid = match &user {
        Some(u) if u.enabled => verify_password(&form.password, &u.password_hash).unwrap_or(false),
        _ => {
            // 동일한 시간 소모를 위해 dummy 검증 수행.
            let _ = verify_password(&form.password, "$argon2id$invalid");
            false
        }
    };

    if !valid {
        record_login_failure(&state, &form.username, Some(&ip), "invalid_credentials")
            .await
            .ok();
        return Err((jar, login_failed_page("Invalid username or password")));
    }

    let user = user.expect("checked Some above");

    // 세션 생성.
    let (token, hash) = generate_session_token();
    let session = Session {
        id: SessionId::new(),
        user_id: user.id,
        token_hash: hash,
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::seconds(SESSION_DURATION_SECS),
        ip_address: Some(ip.clone()),
        user_agent: None,
    };
    state
        .store
        .create_session(&session)
        .await
        .map_err(|_| (jar.clone(), internal_error_page()))?;
    state
        .store
        .update_user_last_login(user.id, Utc::now())
        .await
        .ok();
    record_login_success(&state, &form.username, Some(&ip))
        .await
        .ok();

    tracing::info!(username = %user.username, "login success");

    // 쿠키 설정.
    let cookie = Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .secure(state.secure_cookies)
        .same_site(SameSite::Strict)
        .max_age(time::Duration::seconds(SESSION_DURATION_SECS))
        .build();
    let new_jar = jar.add(cookie);

    Ok((new_jar, Redirect::to("/")))
}

/// POST /logout — 세션 삭제 + 쿠키 제거.
pub async fn logout(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
) -> (CookieJar, Redirect) {
    state.store.delete_session(principal.session_id).await.ok();
    tracing::info!(username = %principal.user.username, "logout");
    let removed = Cookie::from(SESSION_COOKIE);
    let new_jar = jar.remove(removed);
    (new_jar, Redirect::to("/login"))
}

/// GET /api/me — 현재 사용자 정보 (프론트엔드 헤더 표시용).
pub async fn me(Extension(principal): Extension<AuthPrincipal>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "username": principal.user.username,
        "email": principal.user.email,
        "permissions": principal.permissions.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
    }))
}

// ── 에러 페이지 헬퍼 ────────────────────────────────────────────────────

fn internal_error_page() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [("content-type", "text/html; charset=utf-8")],
        "<html><body><h1>500 Internal Server Error</h1></body></html>",
    )
        .into_response()
}

fn login_failed_page(msg: &str) -> Response {
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Fleet Orchestrator — Login</title>
  <link rel="stylesheet" href="/static/login.css" />
</head>
<body class="auth-page">
  <div class="auth-card">
    <div class="auth-logo">F</div>
    <h1>Sign in to Fleet</h1>
    <p class="auth-subtitle">Use your administrator account</p>
    <div class="auth-error">{msg}</div>
    <form method="POST" action="/login" autocomplete="on">
      <label>
        <span>Username</span>
        <input type="text" name="username" required autofocus
               autocomplete="username" minlength="3" maxlength="64"
               pattern="[a-zA-Z][a-zA-Z0-9_-]{{2,63}}" />
      </label>
      <label>
        <span>Password</span>
        <input type="password" name="password" required
               autocomplete="current-password" minlength="8" />
      </label>
      <button type="submit" class="auth-button">Sign in</button>
    </form>
    <p class="auth-footer">Fleet Orchestrator • RBAC + cookie session</p>
  </div>
</body>
</html>"#
    );
    (
        StatusCode::UNAUTHORIZED,
        [("content-type", "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

// ═══════════════════════════════════════════════════════════════════════
//  부트스트랩 핸들러 (Phase 9.1.3)
// ═══════════════════════════════════════════════════════════════════════

/// OTP 폼 (6박스)에서 전송된 데이터. 박스 값을 하나로 합침.
#[derive(Debug, Deserialize)]
pub struct BootstrapForm {
    #[serde(rename = "otp_full")]
    pub otp_full: String,
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    pub password: String,
}

/// GET /bootstrap — 부트스트랩 페이지.
///
/// users 테이블이 비어있을 때만 접근 가능. 이미 활성 사용자가 있으면 /login으로.
pub async fn bootstrap_page(
    State(state): State<Arc<DashboardState>>,
) -> Result<Response, Result<Redirect, StatusCode>> {
    let count = state
        .store
        .count_users()
        .await
        .map_err(|_| Err(StatusCode::INTERNAL_SERVER_ERROR))?;
    if count > 0 {
        return Err(Ok(Redirect::to("/login")));
    }
    let asset = Asset::get("bootstrap.html")
        .map(|a| a.data.to_vec())
        .unwrap_or_else(|| include_bytes!("../assets/bootstrap.html").to_vec());
    Ok((
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        asset,
    )
        .into_response())
}

/// POST /bootstrap — OTP 검증 + 첫 관리자 생성 + 자동 로그인.
pub async fn bootstrap(
    State(state): State<Arc<DashboardState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Form(form): Form<BootstrapForm>,
) -> Result<(CookieJar, Redirect), (StatusCode, Response)> {
    use fleet_core::auth::password::hash_password;
    use fleet_store::consume_bootstrap_and_create_admin;

    // users 테이블이 비어있는지 재확인 (TOCTOU 방어).
    let count = state
        .store
        .count_users()
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, internal_error_page()))?;
    if count > 0 {
        return Err((
            StatusCode::CONFLICT,
            bootstrap_failed_page("System already activated."),
        ));
    }

    // OTP 검증 — prefix + _ + token 형식으로 변환.
    // form.otp_full은 순수 토큰(6자). 실제 토큰은 "fleet_boot_<rand>" 형식.
    // OTP는 token의 일부로 매칭.
    let otp_clean: String = form
        .otp_full
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if otp_clean.len() < 6 {
        return Err((
            StatusCode::BAD_REQUEST,
            bootstrap_failed_page("OTP must be at least 6 characters"),
        ));
    }

    // 활성 토큰 중에서 OTP 접두사와 매칭되는 것 찾기.
    let tokens = state
        .store
        .list_bootstrap_tokens()
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, internal_error_page()))?;

    let matching_token = tokens
        .iter()
        .find(|t| t.is_usable() && t.token.ends_with(&otp_clean))
        .cloned();

    let Some(token) = matching_token else {
        return Err((
            StatusCode::UNAUTHORIZED,
            bootstrap_failed_page("Invalid or expired bootstrap token. Check the CLI output."),
        ));
    };

    // username 검증.
    if let Err(e) = fleet_core::User::validate_username(&form.username) {
        return Err((
            StatusCode::BAD_REQUEST,
            bootstrap_failed_page(&format!("{e}")),
        ));
    }

    // 비밀번호 강도 검증 (zxcvbn은 Phase 9.1.6에서 추가. 일단 길이 검증).
    if form.password.len() < 12 {
        return Err((
            StatusCode::BAD_REQUEST,
            bootstrap_failed_page("Password must be at least 12 characters"),
        ));
    }

    // 해싱.
    let password_hash = hash_password(&form.password)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, internal_error_page()))?;

    // 도메인 사용자 생성.
    let user = fleet_core::User {
        id: fleet_core::UserId::new(),
        username: form.username.clone(),
        email: form.email.clone(),
        password_hash: password_hash.clone(),
        enabled: true,
        created_at: Utc::now(),
        last_login_at: None,
    };

    // 부트스트랩 처리 (토큰 소비 + 사용자 생성 + admin 역할 부여 + 첫 세션).
    let (_new_user, session_token, _session_id) =
        consume_bootstrap_and_create_admin(&*state.store, &token.token, user, password_hash)
            .await
            .map_err(|e| {
                let status = match &e {
                    fleet_store::BootstrapAdminError::InvalidToken(_) => StatusCode::UNAUTHORIZED,
                    fleet_store::BootstrapAdminError::CreateUser(_) => StatusCode::CONFLICT,
                    fleet_store::BootstrapAdminError::AdminRoleMissing => {
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                    fleet_store::BootstrapAdminError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, bootstrap_failed_page(&format!("{e}")))
            })?;

    let ip = addr.ip().to_string();
    record_login_success(&state, &form.username, Some(&ip))
        .await
        .ok();
    tracing::info!(username = %form.username, ip = %ip, "bootstrap completed");

    // 쿠키 설정.
    let cookie = Cookie::build((SESSION_COOKIE, session_token))
        .path("/")
        .http_only(true)
        .secure(state.secure_cookies)
        .same_site(SameSite::Strict)
        .max_age(time::Duration::seconds(SESSION_DURATION_SECS))
        .build();
    let new_jar = jar.add(cookie);

    Ok((new_jar, Redirect::to("/")))
}

fn bootstrap_failed_page(msg: &str) -> Response {
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Fleet Orchestrator — Setup</title>
  <link rel="stylesheet" href="/static/login.css" />
</head>
<body class="bootstrap-page">
  <section class="bootstrap-hero">
    <p class="bootstrap-eyebrow">// first run</p>
    <h1>FLEET</h1>
    <p class="tagline">Activate your control plane</p>
  </section>
  <main class="bootstrap-main">
    <div class="bootstrap-card">
      <div class="auth-error">{msg}</div>
      <p style="text-align:center; margin: 24px 0;">
        <a href="/bootstrap" style="color: var(--primary); font-weight: 500;">Try again</a>
      </p>
    </div>
  </main>
</body>
</html>"#
    );
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_known() {
        assert!(matches!(
            parse_worker_status("online"),
            Some(fleet_core::WorkerStatus::Online)
        ));
        assert!(parse_worker_status("unknown").is_none());
    }
}
