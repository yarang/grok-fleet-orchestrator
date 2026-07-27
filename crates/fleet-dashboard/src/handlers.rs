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
use fleet_core::auth::password::{generate_session_token, verify_password, verify_password_dummy};
use fleet_core::{Session, SessionId};
use serde::Deserialize;
use std::net::SocketAddr;

use crate::assets::Asset;
use crate::auth::{
    check_rate_limit, record_login_failure, record_login_success, AuthPrincipal, CSRF_COOKIE,
    SESSION_COOKIE, SESSION_DURATION_SECS,
};
use crate::auth_util::{csrf_tokens_match, generate_csrf_token};

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub csrf_token: String,
}

/// 로그인 페이지 HTML. CSRF 토큰 쿠키 설정 + 폼에 토큰 주입.
pub async fn login_page(
    State(state): State<Arc<DashboardState>>,
    jar: CookieJar,
) -> (CookieJar, Response) {
    // CSRF 토큰 — 더블 서밋 쿠키. 기존 토큰이 있으면 재사용, 없으면 생성.
    let csrf_token = jar
        .get(CSRF_COOKIE)
        .map(|c| c.value().to_string())
        .unwrap_or_else(generate_csrf_token);
    let csrf_cookie = Cookie::build((CSRF_COOKIE, csrf_token.clone()))
        .path("/")
        .http_only(false) // JS에서 읽을 수 있어야 함 (더블 서밋 패턴)
        .secure(state.secure_cookies)
        .same_site(SameSite::Strict)
        .max_age(time::Duration::seconds(3600))
        .build();
    let jar = jar.add(csrf_cookie);

    let asset = Asset::get("login.html")
        .map(|a| a.data.to_vec())
        .unwrap_or_else(|| include_bytes!("../assets/login.html").to_vec());
    // 정적 HTML에 CSRF 토큰 주입.
    let html = String::from_utf8_lossy(&asset)
        .replace("{{csrf_token}}", &csrf_token);
    (
        jar,
        (
            StatusCode::OK,
            [("content-type", "text/html; charset=utf-8")],
            html.into_bytes(),
        )
            .into_response(),
    )
}

/// POST /login — 폼 제출 처리.
///
/// 성공: 쿠키 설정 + `/` 리다이렉트.
/// 실패: 401 + login.html 재렌더 (에러 메시지 포함).
pub async fn login(
    State(state): State<Arc<DashboardState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Form(form): Form<LoginForm>,
) -> Result<(CookieJar, Redirect), (CookieJar, Response)> {
    let ip = addr.ip().to_string();

    // CSRF 검증 — 더블 서밋 쿠키 패턴.
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return Err((
            jar,
            login_failed_page("Security token expired. Please reload the page."),
        ));
    }

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
            // 타이밍 공격 방지: 사용자가 없어도 실제 검증과 동일한 시간 소모.
            // 유효한 Argon2id PHC에 대해 전체 해싱 연산(m=19456, t=2)을 수행.
            verify_password_dummy(&form.password);
            false
        }
    };

    if !valid {
        record_login_failure(&state, &form.username, Some(&ip), "invalid_credentials")
            .await
            .ok();
        return Err((
            jar,
            login_failed_page_csrf(
                "Invalid username or password",
                cookie_csrf.as_deref().unwrap_or(""),
            ),
        ));
    }

    let user = user.expect("checked Some above");

    // 세션 생성.
    let (token, hash) = generate_session_token();
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let session = Session {
        id: SessionId::new(),
        user_id: user.id,
        token_hash: hash,
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::seconds(SESSION_DURATION_SECS),
        ip_address: Some(ip.clone()),
        user_agent,
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
///
/// CSRF 보호: JS에서 `X-CSRF-Token` 헤더로 CSRF 토큰을 전송해야 함.
/// (세션 쿠키는 SameSite::Strict이지만 defense-in-depth로 이중 검증.)
pub async fn logout(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
) -> Result<(CookieJar, Redirect), (StatusCode, CookieJar, Response)> {
    // CSRF 검증 — 더블 서밋 패턴 (헤더 variant).
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    let header_csrf = headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !csrf_valid(cookie_csrf.as_deref(), header_csrf) {
        return Err((
            StatusCode::FORBIDDEN,
            jar,
            (
                [("content-type", "text/html; charset=utf-8")],
                "CSRF token invalid. Please reload the page.",
            )
                .into_response(),
        ));
    }
    state.store.delete_session(principal.session_id).await.ok();
    tracing::info!(username = %principal.user.username, "logout");
    let removed = Cookie::from(SESSION_COOKIE);
    let new_jar = jar.remove(removed);
    Ok((new_jar, Redirect::to("/login")))
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

/// CSRF 토큰 검증 (더블 서밋 패턴).
/// 쿠키 값과 폼/헤더 값이 상수시간으로 일치하는지 확인.
fn csrf_valid(cookie_token: Option<&str>, submitted_token: &str) -> bool {
    match cookie_token {
        Some(cookie) if !cookie.is_empty() && !submitted_token.is_empty() => {
            csrf_tokens_match(cookie, submitted_token)
        }
        _ => false,
    }
}

fn internal_error_page() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [("content-type", "text/html; charset=utf-8")],
        "<html><body><h1>500 Internal Server Error</h1></body></html>",
    )
        .into_response()
}

fn login_failed_page(msg: &str) -> Response {
    login_failed_page_csrf(msg, "")
}

fn login_failed_page_csrf(msg: &str, csrf_token: &str) -> Response {
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
      <input type="hidden" name="csrf_token" value="{csrf_token}" />
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

/// OTP/토큰 폼 데이터.
///
/// 보안: 이전 6자 접미사 매칭(`ends_with`)은 브루트포스에 취약했음.
/// Phase 9.1.7 보안 패치에서 전체 토큰(`fleet_boot_<43 chars>`) 정확 매칭으로 변경.
#[derive(Debug, Deserialize)]
pub struct BootstrapForm {
    /// 전체 부트스트랩 토큰 (`fleet_boot_...` 형식, 로그/CLI 출력에서 복사).
    #[serde(rename = "otp_full")]
    pub otp_full: String,
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    pub password: String,
    #[serde(default)]
    pub csrf_token: String,
}

/// GET /bootstrap — 부트스트랩 페이지. CSRF 쿠키 설정 + 토큰 주입.
pub async fn bootstrap_page(
    State(state): State<Arc<DashboardState>>,
    jar: CookieJar,
) -> Result<(CookieJar, Response), Result<Redirect, StatusCode>> {
    let count = state
        .store
        .count_users()
        .await
        .map_err(|_| Err(StatusCode::INTERNAL_SERVER_ERROR))?;
    if count > 0 {
        return Err(Ok(Redirect::to("/login")));
    }

    // CSRF 토큰 설정.
    let csrf_token = jar
        .get(CSRF_COOKIE)
        .map(|c| c.value().to_string())
        .unwrap_or_else(generate_csrf_token);
    let csrf_cookie = Cookie::build((CSRF_COOKIE, csrf_token.clone()))
        .path("/")
        .http_only(false)
        .secure(state.secure_cookies)
        .same_site(SameSite::Strict)
        .max_age(time::Duration::seconds(3600))
        .build();
    let jar = jar.add(csrf_cookie);

    let asset = Asset::get("bootstrap.html")
        .map(|a| a.data.to_vec())
        .unwrap_or_else(|| include_bytes!("../assets/bootstrap.html").to_vec());
    let html = String::from_utf8_lossy(&asset)
        .replace("{{csrf_token}}", &csrf_token);
    Ok((
        jar,
        (
            StatusCode::OK,
            [("content-type", "text/html; charset=utf-8")],
            html.into_bytes(),
        )
            .into_response(),
    ))
}

/// POST /bootstrap — OTP 검증 + 첫 관리자 생성 + 자동 로그인.
pub async fn bootstrap(
    State(state): State<Arc<DashboardState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Form(form): Form<BootstrapForm>,
) -> Result<(CookieJar, Redirect), (StatusCode, CookieJar, Response)> {
    use fleet_core::auth::password::hash_password;
    use fleet_store::consume_bootstrap_and_create_admin;

    // CSRF 검증.
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return Err((
            StatusCode::FORBIDDEN,
            jar,
            bootstrap_failed_page("Security token expired. Please reload the page."),
        ));
    }

    // users 테이블이 비어있는지 재확인 (TOCTOU 방어).
    let count = state
        .store
        .count_users()
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, jar.clone(), internal_error_page()))?;
    if count > 0 {
        return Err((
            StatusCode::CONFLICT,
            jar,
            bootstrap_failed_page("System already activated."),
        ));
    }

    let ip = addr.ip().to_string();

    // Rate limit — bootstrap 엔드포인트도 무차별 대입 공격에서 보호.
    // 식별자는 "bootstrap:<ip>" (아직 사용자가 없으므로 IP 기반).
    let bootstrap_id = format!("bootstrap:{ip}");
    let allowed = crate::auth::check_rate_limit(&state, &bootstrap_id, Some(&ip))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                jar.clone(),
                internal_error_page(),
            )
        })?;
    if !allowed {
        crate::auth::record_login_failure(&state, &bootstrap_id, Some(&ip), "bootstrap_rate_limited")
            .await
            .ok();
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            jar,
            bootstrap_failed_page("Too many bootstrap attempts. Wait 60s and try again."),
        ));
    }

    // 전체 토큰 검증 — 접미사 매칭이 아닌 정확 일치.
    // 입력은 `fleet_boot_<43 base64url chars>` 형식이어야 함.
    let token_input = form.otp_full.trim();
    if !token_input.starts_with("fleet_boot_") || token_input.len() < 20 {
        crate::auth::record_login_failure(&state, &bootstrap_id, Some(&ip), "bootstrap_bad_format")
            .await
            .ok();
        return Err((
            StatusCode::BAD_REQUEST,
            jar,
            bootstrap_failed_page(
                "Invalid token format. Copy the full token from the CLI output.",
            ),
        ));
    }

    // 활성 토큰 중에서 정확히 일치하는 것을 상수시간으로 검색.
    let tokens = state
        .store
        .list_bootstrap_tokens()
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, jar.clone(), internal_error_page()))?;

    let matching_token = tokens
        .iter()
        .find(|t| t.is_usable() && fleet_core::auth::password::constant_time_eq(&t.token, token_input))
        .cloned();

    let Some(token) = matching_token else {
        crate::auth::record_login_failure(&state, &bootstrap_id, Some(&ip), "bootstrap_invalid_token")
            .await
            .ok();
        return Err((
            StatusCode::UNAUTHORIZED,
            jar,
            bootstrap_failed_page("Invalid or expired bootstrap token. Check the CLI output."),
        ));
    };

    // username 검증.
    if let Err(e) = fleet_core::User::validate_username(&form.username) {
        return Err((
            StatusCode::BAD_REQUEST,
            jar,
            bootstrap_failed_page(&format!("{e}")),
        ));
    }

    // 비밀번호 강도 검증 (zxcvbn + 길이 정책 중앙화).
    if fleet_core::auth::password::validate_password(&form.password, &[&form.username]).is_err() {
        return Err((
            StatusCode::BAD_REQUEST,
            jar,
            bootstrap_failed_page("Password is too weak. Use at least 12 characters with a mix of letters, numbers, and symbols."),
        ));
    }

    // 해싱.
    let password_hash = hash_password(&form.password).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            jar.clone(),
            internal_error_page(),
        )
    })?;

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
                let (status, msg) = match &e {
                    fleet_store::BootstrapAdminError::InvalidToken(_) => (
                        StatusCode::UNAUTHORIZED,
                        "Invalid or expired bootstrap token.",
                    ),
                    fleet_store::BootstrapAdminError::CreateUser(_) => {
                        (StatusCode::CONFLICT, "Unable to create account. Please try again.")
                    }
                    fleet_store::BootstrapAdminError::AdminRoleMissing => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Setup incomplete. Contact your administrator.",
                    ),
                    fleet_store::BootstrapAdminError::Store(_) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "A system error occurred. Please try again.",
                    ),
                };
                tracing::error!(error = %e, "bootstrap admin creation failed");
                (status, jar.clone(), bootstrap_failed_page(msg))
            })?;

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
