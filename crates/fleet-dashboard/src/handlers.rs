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

use fleet_core::PermissionKind;
use fleet_core::{TaskFilter, TaskStatus, WorkerFilter};

use crate::app::DashboardState;
use crate::auth::{require_permission, AuthPrincipal};
use crate::schema::{
    OverviewResponse, TaskCounts, TaskSummary, TokenStats, WorkerCounts, WorkerSummary,
};

/// `/health` — 헬스체크.
pub async fn health() -> &'static str {
    "ok"
}

/// `/api/overview` — 요약 통계.
pub async fn overview(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<OverviewResponse>, StatusCode> {
    require_permission(&principal, PermissionKind::DashboardView)?;
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
    let mut tok_stats = TokenStats::default();
    for t in &tasks {
        task_counts.total += 1;
        match &t.status {
            TaskStatus::Pending => task_counts.pending += 1,
            TaskStatus::Dispatched { .. } => task_counts.dispatched += 1,
            TaskStatus::Completed(result) => {
                task_counts.completed += 1;
                if let Some(usage) = &result.token_usage {
                    tok_stats.input_tokens += usage.input_tokens;
                    tok_stats.output_tokens += usage.output_tokens;
                    tok_stats.cache_read_tokens += usage.cache_read_tokens;
                    tok_stats.total_tokens += usage.total();
                }
            }
            TaskStatus::Failed(_) => task_counts.failed += 1,
            TaskStatus::Cancelled { .. } => task_counts.cancelled += 1,
        }
    }

    let tokens = if tok_stats.total_tokens > 0 {
        Some(tok_stats)
    } else {
        None
    };

    Ok(Json(OverviewResponse {
        workers: counts,
        tasks: task_counts,
        tokens,
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
    Extension(principal): Extension<AuthPrincipal>,
    Query(q): Query<ListWorkersQuery>,
) -> Result<Json<Vec<WorkerSummary>>, StatusCode> {
    require_permission(&principal, PermissionKind::WorkerList)?;
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
    #[serde(default)]
    pub offset: usize,
}

/// `/api/tasks` — 작업 목록.
pub async fn list_tasks(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Query(q): Query<ListTasksQuery>,
) -> Result<Json<Vec<TaskSummary>>, StatusCode> {
    require_permission(&principal, PermissionKind::TaskList)?;
    let filter = TaskFilter {
        limit: q.limit,
        offset: q.offset,
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
    Extension(principal): Extension<AuthPrincipal>,
    Query(q): Query<ListEventsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_permission(&principal, PermissionKind::EventsList)?;
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
    let (phase, worker_id, exit_code, duration_secs, token_usage) = match &t.status {
        TaskStatus::Pending => ("pending", None, None, None, None),
        TaskStatus::Dispatched { worker_id, .. } => {
            ("dispatched", Some(worker_id.to_string()), None, None, None)
        }
        TaskStatus::Completed(r) => (
            "completed",
            Some(r.worker_id.to_string()),
            Some(r.exit_code),
            Some(r.duration_secs),
            r.token_usage.map(|u| TokenStats {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_read_tokens: u.cache_read_tokens,
                total_tokens: u.total(),
            }),
        ),
        TaskStatus::Failed(f) => (
            "failed",
            f.worker_id.map(|w| w.to_string()),
            None,
            None,
            None,
        ),
        TaskStatus::Cancelled { .. } => ("cancelled", None, None, None, None),
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
        model: t.model.clone(),
        token_usage,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  P1/P1.5/P2 페이지 + API 핸들러
// ═══════════════════════════════════════════════════════════════════════

use crate::schema::{
    HostDetail, HostEventSummary, HostMetricsSummary, HostSummary, OsInfoSummary, UserSummary,
    WorkerDetail,
};
use axum::extract::Path;

/// 임베드된 HTML 페이지를 반환하는 헬퍼.
pub fn serve_page(name: &str) -> Response {
    match crate::assets::Asset::get(name) {
        Some(file) => (
            [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
            file.data,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "page not built").into_response(),
    }
}

// ── P1: Task Queue ───────────────────────────────────────────────────

/// GET /tasks — 태스크 큐 HTML 페이지.
pub async fn task_queue_page() -> Response {
    serve_page("tasks.html")
}

/// GET /tasks/:id — 태스크 상세 HTML 페이지.
pub async fn task_detail_page(Path(_id): Path<String>) -> Response {
    serve_page("task-detail.html")
}

/// GET /api/tasks/:id — 태스크 상세 JSON API (출력 포함).
pub async fn get_task_detail_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_permission(&principal, PermissionKind::TaskRead)?;

    let task_id: fleet_core::TaskId = id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

    let task = state
        .store
        .get_task(task_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_task_detail: failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let summary = task_to_summary(&task);

    // stdout/stderr 출력 조회 (task:output 권한 필요).
    let output = if principal.has(PermissionKind::TaskOutput) {
        match state.store.get_output(task_id, 0).await {
            Ok(o) => Some(o),
            Err(_) => None,
        }
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "task": summary,
        "output": output,
    })))
}

// ── P1: Worker Detail ────────────────────────────────────────────────

/// GET /workers/:id — 워커 상세 HTML 페이지.
pub async fn worker_detail_page(Path(_id): Path<String>) -> Response {
    serve_page("worker-detail.html")
}

/// GET /api/workers/:id — 워커 상세 JSON API.
pub async fn get_worker_detail(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<WorkerDetail>, StatusCode> {
    require_permission(&principal, PermissionKind::WorkerList)?;
    let worker_id: fleet_core::WorkerId = id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

    let worker = state
        .store
        .get_worker(worker_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_worker_detail: failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let summary = worker_to_summary(&worker);

    // 최근 태스크 조회 (이 워커가 처리한 것 — SQL 레벨에서 worker_id 필터링).
    let tasks = state
        .store
        .list_tasks(&TaskFilter {
            limit: 20,
            worker_id: Some(worker_id),
            ..Default::default()
        })
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_worker_detail: list_tasks failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let recent_tasks: Vec<TaskSummary> = tasks.iter().map(task_to_summary).collect();

    Ok(Json(WorkerDetail {
        summary,
        worker_version: worker.worker_version.clone(),
        recent_tasks,
    }))
}

// ── P1: User Management ──────────────────────────────────────────────

/// GET /admin/users — 사용자 관리 HTML 페이지.
pub async fn admin_users_page() -> Response {
    serve_page("admin-users.html")
}

/// GET /api/users — 사용자 목록 JSON API.
pub async fn list_users_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<Vec<UserSummary>>, StatusCode> {
    require_permission(&principal, PermissionKind::UserRead)?;
    let users = state.store.list_users().await.map_err(|e| {
        tracing::error!(error = %e, "list_users failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut summaries = Vec::with_capacity(users.len());
    for u in &users {
        let roles = state.store.list_user_roles(u.id).await.unwrap_or_default();
        summaries.push(UserSummary {
            id: u.id.to_string(),
            username: u.username.clone(),
            email: u.email.clone(),
            enabled: u.enabled,
            roles: roles.iter().map(|r| r.name.clone()).collect(),
            created_at: u.created_at,
            last_login_at: u.last_login_at,
        });
    }
    Ok(Json(summaries))
}

/// POST /api/users — 사용자 생성 (admin only).
#[derive(Debug, serde::Deserialize)]
pub struct CreateUserForm {
    /// 로그인 식별자 (이메일).
    pub email: String,
    /// 표시용 이름 (옵션, 미지정 시 email prefix).
    #[serde(default)]
    pub username: Option<String>,
    pub password: String,
    #[serde(default)]
    pub csrf_token: String,
}

pub async fn create_user_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    Form(form): Form<CreateUserForm>,
) -> Result<(StatusCode, CookieJar), (StatusCode, String)> {
    require_permission(&principal, PermissionKind::UserCreate)
        .map_err(|_| (StatusCode::FORBIDDEN, "Permission denied".to_string()))?;

    // CSRF 검증.
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return Err((StatusCode::FORBIDDEN, "CSRF token invalid".to_string()));
    }

    // email 검증.
    if let Err(e) = fleet_core::User::validate_email(&form.email) {
        return Err((StatusCode::BAD_REQUEST, e.to_string()));
    }

    // 비밀번호 강도 검증.
    if fleet_core::auth::password::validate_password(&form.password, &[&form.email]).is_err() {
        return Err((StatusCode::BAD_REQUEST, "Password too weak".to_string()));
    }

    let hash = fleet_core::auth::password::hash_password(&form.password)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Hash failed".to_string()))?;

    // username: 명시값 또는 email prefix.
    let username = form
        .username
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| form.email.split('@').next().unwrap_or("user").to_string());

    let user = fleet_core::User {
        id: fleet_core::UserId::new(),
        username,
        email: Some(form.email.clone()),
        email_verified: false,
        password_hash: hash,
        enabled: true,
        created_at: Utc::now(),
        last_login_at: None,
    };

    state.store.create_user(&user).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("already exists") {
            (StatusCode::CONFLICT, "Username already exists".to_string())
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, msg)
        }
    })?;

    // viewer 역할 부여 (기본).
    if let Some(viewer_role) = state.store.get_role_by_name("viewer").await.ok().flatten() {
        let _ = state
            .store
            .assign_user_role(user.id, viewer_role.id, Some(principal.user.id))
            .await;
    }

    // 이메일 인증 토큰 생성 + 발송.
    let (raw_token, token_hash) = generate_session_token();
    let verification = fleet_core::EmailVerificationToken {
        id: uuid::Uuid::new_v4(),
        user_id: user.id,
        token_hash,
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(24),
        consumed_at: None,
    };
    let _ = state
        .store
        .create_email_verification_token(&verification)
        .await;
    let base_url =
        std::env::var("FLEET_BASE_URL").unwrap_or_else(|_| "http://localhost:8082".into());
    let verify_url = format!("{base_url}/verify-email?token={raw_token}");
    if let Err(e) =
        crate::email::send_verification_email(state.smtp_config.as_ref(), &form.email, &verify_url)
            .await
    {
        tracing::warn!(error = %e, "failed to send verification email — user created but email not sent");
    }

    tracing::info!(email = %form.email, created_by = %principal.user.username, "user created — verification email sent");
    Ok((StatusCode::CREATED, jar))
}

/// POST /api/users/:id/toggle — 활성/비활성 토글 (admin only).
#[derive(Debug, serde::Deserialize)]
pub struct ToggleUserForm {
    #[serde(default)]
    pub csrf_token: String,
}

pub async fn toggle_user_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    Path(id): Path<String>,
    Form(form): Form<ToggleUserForm>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_permission(&principal, PermissionKind::UserCreate)
        .map_err(|_| (StatusCode::FORBIDDEN, "Permission denied".to_string()))?;

    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return Err((StatusCode::FORBIDDEN, "CSRF token invalid".to_string()));
    }

    let user_id: fleet_core::UserId = uuid::Uuid::parse_str(&id)
        .map(fleet_core::UserId::from)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid user ID".to_string()))?;

    let user = state
        .store
        .get_user_by_id(user_id)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "DB error".to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "User not found".to_string()))?;

    let new_enabled = !user.enabled;
    state
        .store
        .set_user_enabled(user_id, new_enabled)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "DB error".to_string()))?;

    if !new_enabled {
        let _ = state.store.delete_user_sessions(user_id).await;
    }

    tracing::info!(username = %user.username, enabled = new_enabled, by = %principal.user.username, "user toggled");
    Ok(StatusCode::OK)
}

/// POST /api/users/:id/delete — 사용자 삭제 (admin only).
pub async fn delete_user_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    Path(id): Path<String>,
    Form(form): Form<ToggleUserForm>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_permission(&principal, PermissionKind::UserDelete)
        .map_err(|_| (StatusCode::FORBIDDEN, "Permission denied".to_string()))?;

    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return Err((StatusCode::FORBIDDEN, "CSRF token invalid".to_string()));
    }

    let user_id: fleet_core::UserId = uuid::Uuid::parse_str(&id)
        .map(fleet_core::UserId::from)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid user ID".to_string()))?;

    // 자기 자신 삭제 방지.
    if user_id == principal.user.id {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cannot delete yourself".to_string(),
        ));
    }

    let user = state
        .store
        .get_user_by_id(user_id)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "DB error".to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "User not found".to_string()))?;

    state
        .store
        .delete_user(user_id)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "DB error".to_string()))?;

    tracing::info!(username = %user.username, by = %principal.user.username, "user deleted");
    Ok(StatusCode::OK)
}

// ── P1.5: Host Inventory ─────────────────────────────────────────────

/// GET /hosts — 호스트 인벤토리 HTML 페이지.
pub async fn host_inventory_page() -> Response {
    serve_page("hosts.html")
}

/// GET /hosts/:hostname — 호스트 상세 HTML 페이지.
pub async fn host_detail_page(Path(_hostname): Path<String>) -> Response {
    serve_page("host-detail.html")
}

/// GET /api/hosts — 호스트 목록 JSON API.
pub async fn list_hosts_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<Vec<HostSummary>>, StatusCode> {
    require_permission(&principal, PermissionKind::DashboardView)?;
    let hosts = state.store.list_hosts().await.map_err(|e| {
        tracing::error!(error = %e, "list_hosts failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // 워커 이름 해결을 위해 워커 목록 조회.
    let workers = state
        .store
        .list_workers(&WorkerFilter::default())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "list_hosts_api: list_workers failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let summaries = hosts
        .iter()
        .map(|h| {
            let worker_name = h
                .worker_id
                .and_then(|wid| workers.iter().find(|w| w.id == wid))
                .map(|w| w.name.clone());
            host_to_summary(h, worker_name)
        })
        .collect();
    Ok(Json(summaries))
}

/// GET /api/hosts/:hostname — 호스트 상세 JSON API.
pub async fn get_host_detail_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(hostname): Path<String>,
) -> Result<Json<HostDetail>, StatusCode> {
    require_permission(&principal, PermissionKind::DashboardView)?;
    let host = state
        .store
        .get_host_by_hostname(&hostname)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_host_detail: failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let events = state
        .store
        .list_host_events(host.id, 50)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_host_detail: list_events failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let worker_name = if let Some(wid) = host.worker_id {
        state
            .store
            .get_worker(wid)
            .await
            .ok()
            .flatten()
            .map(|w| w.name)
    } else {
        None
    };

    let summary = host_to_summary(&host, worker_name);
    let os_info = host.os_info.as_ref().map(|oi| OsInfoSummary {
        os_type: oi.os_type.clone(),
        distro: oi.distro.clone(),
        kernel: oi.kernel.clone(),
        arch: oi.arch.clone(),
        hostname: oi.hostname.clone(),
    });

    let event_summaries: Vec<HostEventSummary> = events
        .iter()
        .map(|e| HostEventSummary {
            id: e.id.to_string(),
            event_type: e.event_type.clone(),
            severity: e.severity.as_str().to_string(),
            message: e.message.clone(),
            created_at: e.created_at,
        })
        .collect();

    Ok(Json(HostDetail {
        summary,
        ssh_host: host.ssh_host.clone(),
        ssh_port: host.ssh_port,
        ssh_user: host.ssh_user.clone(),
        os_info,
        metrics: HostMetricsSummary {
            load_avg: host.metrics.load_avg.clone(),
            mem_available_mb: host.metrics.mem_available_mb,
            disk_free_mb: host.metrics.disk_free_mb,
        },
        events: event_summaries,
    }))
}

fn host_to_summary(h: &fleet_core::Host, worker_name: Option<String>) -> HostSummary {
    HostSummary {
        id: h.id.to_string(),
        hostname: h.hostname.clone(),
        status: h.status.as_str().to_string(),
        worker_id: h.worker_id.map(|w| w.to_string()),
        worker_name,
        grok_version: h.grok_version.clone(),
        fleet_worker_version: h.fleet_worker_version.clone(),
        os_type: h.os_info.as_ref().map(|oi| oi.os_type.clone()),
        arch: h.os_info.as_ref().map(|oi| oi.arch.clone()),
        last_heartbeat_at: h.last_heartbeat_at,
        provisioned_at: h.provisioned_at,
        created_at: h.created_at,
    }
}

// ── P2: Audit Log ────────────────────────────────────────────────────

/// GET /admin/audit — 감사 로그 HTML 페이지.
pub async fn admin_audit_page() -> Response {
    serve_page("admin-audit.html")
}

/// GET /api/audit — 감사 로그 JSON API.
pub async fn list_audit_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Query(q): Query<ListEventsQuery>,
) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    require_permission(&principal, PermissionKind::AuditRead)?;
    let events = state
        .store
        .list_events(q.after_seq, q.limit)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "list_audit failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // EventEntry { seq, event: FleetEvent }를 JSON으로 직렬화.
    // FleetEvent는 #[serde(tag = "type")]으로 태그되어 있어,
    // type 필드에서 이벤트 종류를 알 수 있다.
    let entries = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "seq": e.seq,
                "event": e.event,
            })
        })
        .collect();
    Ok(Json(entries))
}

// ── P2: MCP Tools Explorer ───────────────────────────────────────────

/// GET /admin/tools — MCP 도구 탐색기 HTML 페이지.
pub async fn admin_tools_page() -> Response {
    serve_page("admin-tools.html")
}

/// GET /api/tools — MCP 도구 목록 JSON API.
pub async fn list_tools_api(
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_permission(&principal, PermissionKind::DashboardView)?;
    // MCP 도구 카탈로그 — fleet-mcp schema.rs의 실제 도구명과 일치.
    Ok(Json(serde_json::json!({
        "tools": [
            {"name": "fleet_dispatch_task", "description": "Submit a new task to the fleet"},
            {"name": "fleet_get_task_status", "description": "Check task status by ID"},
            {"name": "fleet_wait_for_task", "description": "Wait for task completion"},
            {"name": "fleet_cancel_task", "description": "Cancel a running task"},
            {"name": "fleet_list_workers", "description": "List all registered workers"},
            {"name": "fleet_list_tasks", "description": "List tasks with optional status filtering and pagination"},
            {"name": "fleet_stream_task_output", "description": "Stream task output in real-time"},
            {"name": "fleet_collect_results", "description": "Collect completed task results"},
        ]
    })))
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
    check_rate_limit, record_login_failure, record_login_success, CSRF_COOKIE, SESSION_COOKIE,
    SESSION_DURATION_SECS,
};
use crate::auth_util::{csrf_tokens_match, generate_csrf_token};

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub email: String,
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
    let html = String::from_utf8_lossy(&asset).replace("{{csrf_token}}", &csrf_token);
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
    let allowed = check_rate_limit(&state, &form.email, Some(&ip))
        .await
        .map_err(|_| (jar.clone(), internal_error_page()))?;
    if !allowed {
        record_login_failure(&state, &form.email, Some(&ip), "rate_limited")
            .await
            .ok();
        return Err((
            jar,
            login_failed_page("Too many attempts. Try again in 60s."),
        ));
    }

    // 사용자 조회 — email 기반 (타이밍 공격 방지: 사용자 없어도 동일한 시간 소모).
    let user = state
        .store
        .get_user_by_email(&form.email)
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
        record_login_failure(&state, &form.email, Some(&ip), "invalid_credentials")
            .await
            .ok();
        return Err((
            jar,
            login_failed_page_csrf(
                "Invalid email or password",
                cookie_csrf.as_deref().unwrap_or(""),
            ),
        ));
    }

    let user = user.expect("checked Some above");

    // 이메일 인증 확인.
    if !user.email_verified {
        record_login_failure(&state, &form.email, Some(&ip), "email_not_verified")
            .await
            .ok();
        return Err((
            jar,
            login_failed_page_csrf(
                "Email not verified. Please check your inbox for the verification link.",
                cookie_csrf.as_deref().unwrap_or(""),
            ),
        ));
    }

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
    record_login_success(&state, &form.email, Some(&ip))
        .await
        .ok();

    tracing::info!(email = %form.email, username = %user.username, "login success");

    // 쿠키 설정.
    let cookie = Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .secure(state.secure_cookies)
        .same_site(SameSite::Strict)
        .max_age(time::Duration::seconds(SESSION_DURATION_SECS))
        .build();
    let new_jar = jar.add(cookie);

    // CSRF 토큰 회전 — 로그인 성공 후 새 토큰 발급.
    // 인증 전 CSRF 쿠키를 재사용하지 않아 세션 고정 공격을 방어.
    let rotated_csrf = generate_csrf_token();
    let csrf_cookie = Cookie::build((CSRF_COOKIE, rotated_csrf))
        .path("/")
        .http_only(false)
        .secure(state.secure_cookies)
        .same_site(SameSite::Strict)
        .max_age(time::Duration::seconds(3600))
        .build();
    let new_jar = new_jar.add(csrf_cookie);

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
        "email_verified": principal.user.email_verified,
        "permissions": principal.permissions.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
    }))
}

// ── 이메일 인증 ──────────────────────────────────────────────────────────

/// GET /verify-email?token=... — 이메일 인증 링크 처리.
///
/// 토큰 검증 후 `email_verified = true` 설정.
pub async fn verify_email_page(
    State(state): State<Arc<DashboardState>>,
    Query(params): Query<VerifyEmailParams>,
) -> Response {
    let token = params.token.unwrap_or_default();
    if token.is_empty() {
        return verification_result_page(false, "Missing verification token.");
    }

    // 토큰 해시.
    let hash = crate::auth_util::sha256_hex(token.as_bytes());

    // DB 조회.
    let verification = match state.store.get_email_verification_token(&hash).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return verification_result_page(false, "Invalid or unknown verification token.");
        }
        Err(e) => {
            tracing::error!(error = %e, "get_email_verification_token failed");
            return verification_result_page(false, "Server error. Please try again later.");
        }
    };

    // 만료 확인.
    if verification.is_expired() {
        return verification_result_page(
            false,
            "Verification token has expired. Please request a new one.",
        );
    }

    // 이미 사용됨.
    if verification.is_consumed() {
        return verification_result_page(false, "This verification link has already been used.");
    }

    // 토큰 소비 + 사용자 email_verified 설정.
    let now = Utc::now();
    if let Err(e) = state
        .store
        .consume_email_verification_token(verification.id, now)
        .await
    {
        tracing::error!(error = %e, "consume_email_verification_token failed");
        return verification_result_page(false, "Server error. Please try again later.");
    }
    if let Err(e) = state
        .store
        .set_user_email_verified(verification.user_id, true)
        .await
    {
        tracing::error!(error = %e, "set_user_email_verified failed");
        return verification_result_page(false, "Server error. Please try again later.");
    }

    tracing::info!(user_id = %verification.user_id, "email verified successfully");
    verification_result_page(true, "Your email has been verified. You can now sign in.")
}

#[derive(Debug, serde::Deserialize)]
pub struct VerifyEmailParams {
    pub token: Option<String>,
}

/// POST /api/users/resend-verification — 인증 이메일 재발송.
pub async fn resend_verification_api(
    State(state): State<Arc<DashboardState>>,
    Json(req): Json<ResendVerificationRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let user = state
        .store
        .get_user_by_email(&req.email)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "DB error".into()))?
        .ok_or((StatusCode::NOT_FOUND, "User not found".into()))?;

    if user.email_verified {
        return Err((StatusCode::CONFLICT, "Email already verified".into()));
    }

    // 새 인증 토큰 생성.
    let (raw_token, token_hash) = generate_session_token();
    let verification = fleet_core::EmailVerificationToken {
        id: uuid::Uuid::new_v4(),
        user_id: user.id,
        token_hash,
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(24),
        consumed_at: None,
    };
    state
        .store
        .create_email_verification_token(&verification)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "DB error".into()))?;

    // 이메일 발송 (SMTP 미설정 시 로그 출력).
    let base_url =
        std::env::var("FLEET_BASE_URL").unwrap_or_else(|_| "http://localhost:8082".into());
    let verify_url = format!("{base_url}/verify-email?token={raw_token}");

    if let Err(e) = crate::email::send_verification_email(
        state.smtp_config.as_ref(),
        req.email.as_str(),
        &verify_url,
    )
    .await
    {
        tracing::error!(error = %e, "failed to send verification email");
    }

    Ok(StatusCode::OK)
}

#[derive(Debug, serde::Deserialize)]
pub struct ResendVerificationRequest {
    pub email: String,
}

/// 인증 결과 HTML 페이지.
fn verification_result_page(success: bool, message: &str) -> Response {
    let (title, color) = if success {
        ("✓ Verified", "#1a7d31")
    } else {
        ("✗ Verification Failed", "#c61e00")
    };
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>Email Verification — Fleet</title>
<link rel="stylesheet" href="/static/login.css"></head>
<body class="auth-page">
  <div class="auth-card">
    <h1 style="color:{color};margin:0 0 12px;">{title}</h1>
    <p style="font-size:15px;line-height:1.6;">{message}</p>
    <a href="/login" class="auth-button" style="display:inline-block;text-decoration:none;margin-top:16px;">Go to Sign In</a>
  </div>
</body>
</html>"#
    );
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        html.into_bytes(),
    )
        .into_response()
}

// ── 비밀번호 재설정 ──────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct ResendVerificationForm {
    pub email: String,
    #[allow(dead_code)]
    pub csrf_token: String,
}

/// GET /resend-verification — 인증 이메일 재발송 요청 페이지.
pub async fn resend_verification_page(
    State(state): State<Arc<DashboardState>>,
    jar: CookieJar,
) -> (CookieJar, Response) {
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

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>Resend Verification — Fleet</title>
<link rel="stylesheet" href="/static/login.css"></head>
<body class="auth-page">
  <div class="auth-card">
    <div class="auth-logo">F</div>
    <h1>Resend Verification</h1>
    <p class="auth-subtitle">Enter your email to receive a new verification link</p>
    <form method="POST" action="/resend-verification">
      <input type="hidden" name="csrf_token" value="{csrf_token}" />
      <label>
        <span>Email</span>
        <input type="email" name="email" required autofocus autocomplete="email" />
      </label>
      <button type="submit" class="auth-button">Send Verification Link</button>
    </form>
    <a href="/login" style="display:inline-block;margin-top:12px;color:#5b7fef;text-decoration:none;">← Back to Sign In</a>
  </div>
</body>
</html>"#
    );

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

/// POST /resend-verification — 폼 기반 인증 이메일 재발송.
pub async fn resend_verification_form(
    State(state): State<Arc<DashboardState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Form(form): Form<ResendVerificationForm>,
) -> Response {
    let ip = addr.ip().to_string();

    // CSRF 검증.
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return info_page("error", "Security token expired. Please reload the page.");
    }

    // IP 기반 rate limit — 이메일 폭탄 방지.
    let allowed = check_rate_limit(&state, &form.email, Some(&ip))
        .await
        .unwrap_or(false);
    if !allowed {
        record_login_failure(&state, &form.email, Some(&ip), "resend_rate_limited")
            .await
            .ok();
        return info_page("error", "Too many requests. Please try again later.");
    }

    match state.store.get_user_by_email(&form.email).await {
        Ok(Some(user)) if !user.email_verified => {
            let (raw_token, token_hash) = generate_session_token();
            let verification = fleet_core::EmailVerificationToken {
                id: uuid::Uuid::new_v4(),
                user_id: user.id,
                token_hash,
                created_at: Utc::now(),
                expires_at: Utc::now() + Duration::hours(24),
                consumed_at: None,
            };
            if let Err(e) = state
                .store
                .create_email_verification_token(&verification)
                .await
            {
                tracing::error!(error = %e, "create_email_verification_token failed");
            }

            let base_url =
                std::env::var("FLEET_BASE_URL").unwrap_or_else(|_| "http://localhost:8082".into());
            let verify_url = format!("{base_url}/verify-email?token={raw_token}");

            if let Err(e) = crate::email::send_verification_email(
                state.smtp_config.as_ref(),
                &form.email,
                &verify_url,
            )
            .await
            {
                tracing::error!(error = %e, "failed to send verification email");
            }
        }
        Ok(Some(_)) => { /* already verified — silently succeed */ }
        Ok(None) => { /* user not found — silently succeed (anti-enumeration) */ }
        Err(e) => tracing::error!(error = %e, "get_user_by_email failed"),
    }

    info_page(
        "success",
        "If the email exists and is unverified, a verification link has been sent.",
    )
}

// ── 비밀번호 재설정 ──────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct ForgotPasswordForm {
    pub email: String,
    #[allow(dead_code)]
    pub csrf_token: String,
}

/// GET /forgot-password — 비밀번호 재설정 요청 페이지.
pub async fn forgot_password_page(
    State(state): State<Arc<DashboardState>>,
    jar: CookieJar,
) -> (CookieJar, Response) {
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

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>Forgot Password — Fleet</title>
<link rel="stylesheet" href="/static/login.css"></head>
<body class="auth-page">
  <div class="auth-card">
    <div class="auth-logo">F</div>
    <h1>Reset Password</h1>
    <p class="auth-subtitle">Enter your email to receive a reset link</p>
    <form method="POST" action="/forgot-password">
      <input type="hidden" name="csrf_token" value="{csrf_token}" />
      <label>
        <span>Email</span>
        <input type="email" name="email" required autofocus autocomplete="email" />
      </label>
      <button type="submit" class="auth-button">Send Reset Link</button>
    </form>
    <a href="/login" style="display:inline-block;margin-top:12px;color:#5b7fef;text-decoration:none;">← Back to Sign In</a>
  </div>
</body>
</html>"#
    );

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

/// POST /forgot-password — 재설정 링크 이메일 발송.
pub async fn forgot_password(
    State(state): State<Arc<DashboardState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Form(form): Form<ForgotPasswordForm>,
) -> Response {
    let ip = addr.ip().to_string();

    // CSRF 검증.
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return info_page("error", "Security token expired. Please reload the page.");
    }

    // IP 기반 rate limit — 이메일 폭탄 방지.
    let allowed = check_rate_limit(&state, &form.email, Some(&ip))
        .await
        .unwrap_or(false);
    if !allowed {
        record_login_failure(&state, &form.email, Some(&ip), "forgot_rate_limited")
            .await
            .ok();
        return info_page("error", "Too many requests. Please try again later.");
    }

    // 사용자 조회 — 사용자가 없어도 성공 응답 (계정 열거 방지).
    if let Ok(Some(user)) = state.store.get_user_by_email(&form.email).await {
        let (raw_token, token_hash) = generate_session_token();
        let reset_token = fleet_core::PasswordResetToken {
            id: uuid::Uuid::new_v4(),
            user_id: user.id,
            token_hash,
            created_at: Utc::now(),
            expires_at: Utc::now() + Duration::hours(1),
            consumed_at: None,
        };
        if let Err(e) = state.store.create_password_reset_token(&reset_token).await {
            tracing::error!(error = %e, "create_password_reset_token failed");
        }

        let base_url =
            std::env::var("FLEET_BASE_URL").unwrap_or_else(|_| "http://localhost:8082".into());
        let reset_url = format!("{base_url}/reset-password?token={raw_token}");

        if let Err(e) = crate::email::send_password_reset_email(
            state.smtp_config.as_ref(),
            &form.email,
            &reset_url,
        )
        .await
        {
            tracing::error!(error = %e, "failed to send password reset email");
        }
    }

    info_page(
        "success",
        "If the email exists in our system, a reset link has been sent. Please check your inbox.",
    )
}

#[derive(Debug, serde::Deserialize)]
pub struct ResetPasswordParams {
    pub token: Option<String>,
}

/// GET /reset-password?token=... — 비밀번호 재설정 폼.
pub async fn reset_password_page(Query(params): Query<ResetPasswordParams>) -> Response {
    let token = params.token.unwrap_or_default();

    if token.is_empty() {
        return info_page("error", "Missing reset token.");
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>Reset Password — Fleet</title>
<link rel="stylesheet" href="/static/login.css"></head>
<body class="auth-page">
  <div class="auth-card">
    <div class="auth-logo">F</div>
    <h1>Set New Password</h1>
    <p class="auth-subtitle">Choose a strong password (min 8 characters)</p>
    <form method="POST" action="/reset-password">
      <input type="hidden" name="token" value="{token}" />
      <label>
        <span>New Password</span>
        <input type="password" name="password" required autocomplete="new-password" minlength="8" />
      </label>
      <label>
        <span>Confirm Password</span>
        <input type="password" name="password_confirm" required autocomplete="new-password" minlength="8" />
      </label>
      <button type="submit" class="auth-button">Reset Password</button>
    </form>
    <a href="/login" style="display:inline-block;margin-top:12px;color:#5b7fef;text-decoration:none;">← Back to Sign In</a>
  </div>
</body>
</html>"#
    );

    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        html.into_bytes(),
    )
        .into_response()
}

#[derive(Debug, serde::Deserialize)]
pub struct ResetPasswordForm {
    pub token: String,
    pub password: String,
    pub password_confirm: String,
    #[allow(dead_code)]
    pub csrf_token: String,
}

/// POST /reset-password — 비밀번호 재설정 처리.
pub async fn reset_password(
    State(state): State<Arc<DashboardState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Form(form): Form<ResetPasswordForm>,
) -> Response {
    let ip = addr.ip().to_string();

    // CSRF 검증.
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return info_page("error", "Security token expired. Please reload the page.");
    }

    // IP 기반 rate limit — 토큰 열거 공격 방지.
    let allowed = check_rate_limit(&state, &form.token, Some(&ip))
        .await
        .unwrap_or(false);
    if !allowed {
        record_login_failure(&state, &form.token, Some(&ip), "reset_rate_limited")
            .await
            .ok();
        return info_page("error", "Too many requests. Please try again later.");
    }

    // 비밀번호 일치 확인.
    if form.password != form.password_confirm {
        return info_page("error", "Passwords do not match.");
    }

    // 비밀번호 강도 검증.
    if fleet_core::auth::password::validate_password(&form.password, &[]).is_err() {
        return info_page(
            "error",
            "Password is too weak. Use at least 8 characters with a mix of letters, numbers, and symbols.",
        );
    }

    // 토큰 검증.
    let hash = crate::auth_util::sha256_hex(form.token.as_bytes());
    let reset_token = match state.store.get_password_reset_token(&hash).await {
        Ok(Some(t)) => t,
        Ok(None) => return info_page("error", "Invalid or unknown reset token."),
        Err(e) => {
            tracing::error!(error = %e, "get_password_reset_token failed");
            return info_page("error", "Server error. Please try again later.");
        }
    };

    if reset_token.is_consumed() {
        return info_page("error", "This reset link has already been used.");
    }
    if reset_token.is_expired() {
        return info_page(
            "error",
            "This reset link has expired. Please request a new one.",
        );
    }

    // 비밀번호 해싱 + 업데이트.
    let password_hash = match fleet_core::auth::password::hash_password(&form.password) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(error = %e, "hash_password failed");
            return info_page("error", "Server error. Please try again later.");
        }
    };

    if let Err(e) = state
        .store
        .update_user_password(reset_token.user_id, &password_hash)
        .await
    {
        tracing::error!(error = %e, "update_user_password failed");
        return info_page("error", "Server error. Please try again later.");
    }

    // 토큰 소비.
    let _ = state
        .store
        .consume_password_reset_token(reset_token.id, Utc::now())
        .await;

    // 기존 세션 무효화 (보안: 비밀번호 변경 후 모든 세션 로그아웃).
    let _ = state.store.delete_user_sessions(reset_token.user_id).await;

    info_page(
        "success",
        "Your password has been reset. You can now sign in with your new password.",
    )
}

/// 정보/에러 페이지 헬퍼.
fn info_page(kind: &str, message: &str) -> Response {
    let (title, color) = match kind {
        "success" => ("✓ Done", "#1a7d31"),
        _ => ("⚠ Notice", "#c61e00"),
    };
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>Password Reset — Fleet</title>
<link rel="stylesheet" href="/static/login.css"></head>
<body class="auth-page">
  <div class="auth-card">
    <h1 style="color:{color};margin:0 0 12px;">{title}</h1>
    <p style="font-size:15px;line-height:1.6;">{message}</p>
    <a href="/login" class="auth-button" style="display:inline-block;text-decoration:none;margin-top:16px;">Go to Sign In</a>
  </div>
</body>
</html>"#
    );
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        html.into_bytes(),
    )
        .into_response()
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
        <span>Email</span>
        <input type="email" name="email" required autofocus
               autocomplete="email" />
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
    /// 로그인 식별자 (이메일).
    pub email: String,
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
    let html = String::from_utf8_lossy(&asset).replace("{{csrf_token}}", &csrf_token);
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
    let count = state.store.count_users().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            jar.clone(),
            internal_error_page(),
        )
    })?;
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
        crate::auth::record_login_failure(
            &state,
            &bootstrap_id,
            Some(&ip),
            "bootstrap_rate_limited",
        )
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
            bootstrap_failed_page("Invalid token format. Copy the full token from the CLI output."),
        ));
    }

    // 활성 토큰 중에서 정확히 일치하는 것을 상수시간으로 검색.
    let tokens = state.store.list_bootstrap_tokens().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            jar.clone(),
            internal_error_page(),
        )
    })?;

    let matching_token = tokens
        .iter()
        .find(|t| {
            t.is_usable() && fleet_core::auth::password::constant_time_eq(&t.token, token_input)
        })
        .cloned();

    let Some(token) = matching_token else {
        crate::auth::record_login_failure(
            &state,
            &bootstrap_id,
            Some(&ip),
            "bootstrap_invalid_token",
        )
        .await
        .ok();
        return Err((
            StatusCode::UNAUTHORIZED,
            jar,
            bootstrap_failed_page("Invalid or expired bootstrap token. Check the CLI output."),
        ));
    };

    // username 검증.
    // email 형식 검증.
    if let Err(e) = fleet_core::User::validate_email(&form.email) {
        return Err((
            StatusCode::BAD_REQUEST,
            jar,
            bootstrap_failed_page(&format!("{e}")),
        ));
    }

    // 비밀번호 강도 검증 (zxcvbn + 길이 정책 중앙화).
    if fleet_core::auth::password::validate_password(&form.password, &[&form.email]).is_err() {
        return Err((
            StatusCode::BAD_REQUEST,
            jar,
            bootstrap_failed_page("Password is too weak. Use at least 8 characters with a mix of letters, numbers, and symbols."),
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
        username: form.email.split('@').next().unwrap_or("admin").to_string(),
        email: Some(form.email.clone()),
        email_verified: true, // 부트스트랩 admin은 자동 인증.
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
                    fleet_store::BootstrapAdminError::CreateUser(_) => (
                        StatusCode::CONFLICT,
                        "Unable to create account. Please try again.",
                    ),
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

    record_login_success(&state, &form.email, Some(&ip))
        .await
        .ok();
    tracing::info!(email = %form.email, ip = %ip, "bootstrap completed");

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
