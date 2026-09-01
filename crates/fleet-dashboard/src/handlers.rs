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
use crate::error::ApiError;
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
) -> Result<Json<OverviewResponse>, ApiError> {
    require_permission(&principal, PermissionKind::DashboardView)?;
    let workers = state
        .store
        .list_workers(&WorkerFilter::default())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "overview: list_workers failed");
            ApiError::Store(e.to_string())
        })?;

    let mut counts = WorkerCounts::default();
    for w in &workers {
        counts.total += 1;
        match w.status {
            fleet_core::WorkerStatus::Online => counts.online += 1,
            fleet_core::WorkerStatus::Degraded => counts.degraded += 1,
            fleet_core::WorkerStatus::Draining => counts.draining += 1,
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
            ApiError::Store(e.to_string())
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
    /// 건너뛸 행 수 (페이지네이션). 태스크 목록과 동일한 계약.
    #[serde(default)]
    pub offset: usize,
    /// `label_` 접두사를 기반으로 하는 동적 라벨 필터 수집
    #[serde(flatten)]
    pub label_filters: std::collections::HashMap<String, String>,
}

fn default_limit() -> usize {
    100
}

/// `/api/workers` — 워커 목록.
pub async fn list_workers(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Query(q): Query<ListWorkersQuery>,
) -> Result<Json<Vec<WorkerSummary>>, ApiError> {
    require_permission(&principal, PermissionKind::WorkerList)?;
    let mut filter = WorkerFilter::default();
    if let Some(s) = &q.status {
        filter.status = parse_worker_status(s);
    }
    filter.limit = q.limit;
    filter.offset = q.offset;

    let mut labels = std::collections::HashMap::new();
    for (k, v) in q.label_filters {
        if let Some(clean_key) = k.strip_prefix("label_") {
            if !clean_key.is_empty() {
                labels.insert(clean_key.to_string(), v);
            }
        }
    }
    if !labels.is_empty() {
        filter.labels = labels;
    }

    let workers = state.store.list_workers(&filter).await.map_err(|e| {
        tracing::error!(error = %e, "list_workers failed");
        ApiError::Store(e.to_string())
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
) -> Result<Json<Vec<TaskSummary>>, ApiError> {
    require_permission(&principal, PermissionKind::TaskList)?;
    let filter = TaskFilter {
        limit: q.limit,
        offset: q.offset,
        ..Default::default()
    };
    let tasks = state.store.list_tasks(&filter).await.map_err(|e| {
        tracing::error!(error = %e, "list_tasks failed");
        ApiError::Store(e.to_string())
    })?;

    let summaries: Vec<TaskSummary> = tasks.iter().map(task_to_summary).collect();
    debug!(count = summaries.len(), "list_tasks");
    Ok(Json(summaries))
}

/// `/api/task-threads` — 스레드 단위 페이지(`#96`).
///
/// `docs/ui-dashboard/ui-design.md` §3.3: 페이지의 단위는 Task가 아니라
/// 스레드다. `Store::list_task_threads`로 이번 페이지의 `thread_id`들을
/// 활동순으로 고른 뒤, 각 스레드의 구성원 전체를 `list_thread_tasks`로
/// 채운다 — 설계 문서가 명시한 두 질의 구조. 루트(`id == thread_id`)가
/// 삭제됐으면 `root`는 `null`이고 `members`만 남는다 — 그룹을 "루트 Task의
/// 행"이 아니라 "`thread_id` 값 자체"로 정의하기로 한 설계 결정 그대로다.
pub async fn list_task_threads_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Query(q): Query<ListTasksQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_permission(&principal, PermissionKind::TaskList)?;

    let thread_ids = state
        .store
        .list_task_threads(q.limit, q.offset)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "list_task_threads failed");
            ApiError::Store(e.to_string())
        })?;

    let mut threads = Vec::with_capacity(thread_ids.len());
    for thread_id in thread_ids {
        let members = state
            .store
            .list_thread_tasks(thread_id)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, thread_id = %thread_id, "list_thread_tasks failed");
                ApiError::Store(e.to_string())
            })?;

        let root = members
            .iter()
            .find(|t| t.id == thread_id)
            .map(task_to_summary);
        let member_summaries: Vec<TaskSummary> = members.iter().map(task_to_summary).collect();

        threads.push(serde_json::json!({
            "thread_id": thread_id,
            "root": root,
            "members": member_summaries,
        }));
    }

    debug!(count = threads.len(), "list_task_threads");
    Ok(Json(serde_json::json!({ "threads": threads })))
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
) -> Result<Json<serde_json::Value>, ApiError> {
    require_permission(&principal, PermissionKind::EventsList)?;
    let events = state
        .store
        .list_events(q.after_seq, q.limit)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "list_events failed");
            ApiError::Store(e.to_string())
        })?;

    // 작업 stdout/stderr는 `task:output` 권한이 있어야 볼 수 있다.
    // 스트리밍 경로(sse.rs)와 반드시 동일한 필터를 통과시킨다 — 한쪽만 막으면
    // 다른 쪽으로 그대로 새어나간다.
    let events = crate::event_view::filter_events(
        events,
        crate::event_view::may_see_task_output(&principal),
    );

    Ok(Json(serde_json::json!({
        "count": events.len(),
        "events": events,
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
        // 로드맵 #75 — endpoint의 `server-key=` 값은 워커의 grok ACP 인증
        // 토큰 원문이다. 대시보드 뷰어 중 그 값을 봐야 하는 사람은 없다.
        endpoint: fleet_core::mask_server_key(&w.endpoint),
        status: WorkerSummary::status_str(w.status).to_string(),
        labels: w.labels.clone(),
        active_tasks: w.active_tasks,
        max_concurrent: w.max_concurrent,
        max_agent_processes: w.max_agent_processes,
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
        thread_id: t.thread_id.to_string(),
        parent_task_id: t.parent_task_id.map(|id| id.to_string()),
        project_id: t.project_id.map(|id| id.to_string()),
        agent_id: t.agent_id.map(|id| id.to_string()),
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

/// 권한 부족 시 403 HTML 응답.
fn forbidden_page() -> Response {
    (
        StatusCode::FORBIDDEN,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>403</title></head>\
         <body><h1>403 Forbidden</h1>\
         <p>You do not have permission to view this page.</p>\
         <p><a href=\"/\">Back to dashboard</a></p></body></html>",
    )
        .into_response()
}

/// 권한을 검사한 뒤 HTML 페이지를 반환하는 헬퍼.
///
/// **왜 페이지에도 게이트가 필요한가**: 각 페이지 HTML은 데이터를 담지 않는
/// 셸이고 실제 데이터는 `/api/*`가 내려주므로 "API만 막으면 된다"고 보기 쉽다.
/// 그러나 (1) 권한 없는 사용자에게 관리 UI를 노출하면 어떤 관리 기능이
/// 존재하는지·어떤 필드를 받는지가 드러나고, (2) API 게이트 하나만 실수로
/// 빠지면 남는 방어선이 없다. 기능 수준 접근 제어는 계층마다 적용해야 한다
/// (OWASP A01 Broken Access Control / CWE-862 Missing Authorization).
///
/// 각 페이지의 권한은 그 페이지가 호출하는 API의 권한과 일치시킨다 — 어긋나면
/// 페이지는 열리는데 내용은 비어 있는 혼란스러운 상태가 된다.
pub fn serve_page_if_permitted(
    principal: &AuthPrincipal,
    perm: PermissionKind,
    name: &str,
) -> Response {
    if !principal.has(perm) {
        tracing::warn!(
            user = %principal.user.username,
            page = name,
            required = perm.as_str(),
            "page access denied"
        );
        return forbidden_page();
    }
    serve_page(name)
}

// ── P1: Task Queue ───────────────────────────────────────────────────

/// GET /tasks — 태스크 큐 HTML 페이지.
pub async fn task_queue_page() -> Response {
    serve_page("tasks.html")
}

/// GET /tasks/new — 태스크 제출 HTML 페이지.
pub async fn task_new_page() -> Response {
    serve_page("task-new.html")
}

/// `POST /api/tasks` 폼 본문.
#[derive(Debug, serde::Deserialize)]
pub struct SubmitTaskForm {
    pub prompt: String,
    /// 워커 라벨 `model`과 정확히 일치해야 라우팅됨(비우면 스케줄러가 아무 워커나 선택).
    #[serde(default)]
    pub model: Option<String>,
    /// 특정 워커 이름으로 하드 핀. 해당 워커가 오프라인/circuit-open이면
    /// 폴백 없이 실패한다(`WorkerSelector` 동작).
    #[serde(default)]
    pub server_hint: Option<String>,
    /// ACP 프로토콜에는 별도 시스템 프롬프트 필드가 없어, 있으면 실제 프롬프트
    /// 앞에 구분선과 함께 병합해 전송한다.
    #[serde(default)]
    pub custom_instructions: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// `"low"` | `"normal"` | `"high"`. 그 외 값은 `normal`로 취급.
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub max_turns: Option<u32>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// 이 태스크가 "이어가기(Reply)"라면, 직전 태스크의 id (문자열 UUID).
    /// 부모 태스크가 있으면 thread_id를 물려받고, host/cwd/model 중 이 폼에서
    /// 명시하지 않은 값은 부모 값을 상속한다 — 우선순위:
    /// 사용자가 명시한 값 > 부모 태스크 값 > None.
    #[serde(default)]
    pub parent_task_id: Option<String>,
    /// 이 태스크를 묶을 Project (문자열 UUID). 로드맵 #48 2단계 — MCP
    /// `fleet_dispatch_task`와 동일하게 존재·상태를 검증한다. 비우면
    /// 일반 풀 Task이며, 이어가기(`parent_task_id`)인 경우 부모의 Project를
    /// 상속한다.
    #[serde(default)]
    pub project_id: Option<String>,
    /// 이 태스크를 실행할 Agent (문자열 UUID). 로드맵 #49 2단계 — MCP
    /// `fleet_dispatch_task`의 `agent_id`와 동일한 규칙을 쓴다
    /// (`fleet_store::apply_agent_pin`): 존재하지 않는 Agent, `server_hint`와의
    /// 동시 지정, `project_id` 불일치는 제출 시점에 400으로 거절하고,
    /// `project_id`를 비우면 Agent의 Project를 상속한다. 가용성(정지·미배치·
    /// 워커 포화)은 여기서 보지 않고 dispatch 시점에 판정한다.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// 제출 멱등성 키 (로드맵 #62 2단계). 같은 사용자가 같은 키로 같은
    /// 페이로드를 다시 보내면 새 Task를 만들지 않고 기존 Task를 돌려준다.
    /// 같은 키에 다른 페이로드가 오면 409로 거절한다. 비우면 멱등성 검사
    /// 없음(기존 동작 그대로).
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub csrf_token: String,
}

/// `POST /api/tasks` — 대시보드에서 태스크를 제출한다.
///
/// MCP `submit_task` 도구와 동일하게 `Dispatcher::submit`을 직접 호출한다(재조정
/// 루프가 아니라 즉시 디스패치 시도). 워커 선택/CircuitBreaker/transport 실패로
/// `submit()`이 `Err`를 반환해도 **태스크 행 자체는 이미 Store에 생성돼 있다**
/// (실패 사유와 함께 `Failed`로 마킹됨) — 그래서 이 경우도 HTTP 200으로 응답하고
/// `dispatched: false` + `warning`으로 알린다. 진짜 4xx/5xx는 태스크가 아예
/// 생성되기 전(권한 없음/빈 프롬프트/CSRF 무효/Dispatcher 미구성)에만 낸다.
///
/// 로드맵 #38: dispatch 재시도가 활성화된 배포(`--reconcile-max-dispatch-retries`가
/// 0보다 큼, 기본값)에서는 `submit()`이 `Ok`를 반환해도 실제 태스크가 아직
/// `Pending`(재조정 루프의 백그라운드 재시도 대기)일 수 있다 — 이 경우
/// `dispatched: false`이지만 `warning` 필드는 없다(진짜 에러가 아니라 정상적인
/// "재시도 예약됨" 상태이기 때문). `warning` 필드 유무로 "실패"와 "재시도 중"을
/// 구분해야 한다.
///
/// 로드맵 #62 2단계: 응답에 `deduplicated: bool`이 항상 포함된다. `true`면
/// `idempotency_key`가 기존 제출과 일치해 **새 실행이 일어나지 않았고**,
/// `task_id`는 최초 작업의 id다. 이때 그 작업은 이미 `Completed`/`Failed`일 수
/// 있으므로 `dispatched: false` + `warning` 없음 조합이 나오는데, 위 문단의
/// "재시도 예약됨"과 겉모습이 같다 — **`deduplicated`를 먼저 확인해야** 두
/// 상태를 구분할 수 있다. 같은 키에 다른 페이로드가 오면 409 Conflict다.
pub async fn submit_task_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    Form(form): Form<SubmitTaskForm>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_permission(&principal, PermissionKind::TaskCreate)?;

    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return Err(ApiError::Forbidden("CSRF token invalid".into()));
    }

    let prompt = form.prompt.trim();
    if prompt.is_empty() {
        return Err(ApiError::BadRequest("prompt must not be empty".into()));
    }

    let full_prompt = match form.custom_instructions.as_deref().map(str::trim) {
        Some(instr) if !instr.is_empty() => format!("{instr}\n\n---\n\n{prompt}"),
        _ => prompt.to_string(),
    };

    let priority = match form.priority.as_deref() {
        Some("low") => fleet_core::TaskPriority::Low,
        Some("high") => fleet_core::TaskPriority::High,
        _ => fleet_core::TaskPriority::Normal,
    };

    let dispatcher = state
        .dispatcher
        .clone()
        .ok_or_else(|| ApiError::Unavailable("task submission is not configured".into()))?;

    // parent_task_id가 있으면 먼저 부모를 조회 — 없거나 파싱 실패 시 태스크
    // 자체를 생성하지 않고 4xx로 거절한다("이어가기"가 실제로 이어지지 않는
    // 상태로 조용히 새 스레드를 시작하면 사용자가 눈치채기 어렵다).
    let parent_task = match form.parent_task_id.as_deref().filter(|s| !s.is_empty()) {
        Some(raw) => {
            let parent_id: fleet_core::TaskId = raw
                .parse()
                .map_err(|_| ApiError::BadRequest("invalid parent_task_id".into()))?;
            let parent = state
                .store
                .get_task(parent_id)
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?
                .ok_or_else(|| ApiError::BadRequest("parent task not found".into()))?;
            Some(parent)
        }
        None => None,
    };

    // 로드맵 #48 2단계 — 폼에서 명시한 project_id를 파싱한다. 실제 존재·상태
    // 검증은 부모 상속까지 끝난 뒤에 한다(아래) — 이어가기로 물려받은
    // project_id도 명시 입력과 똑같이 검증 대상이기 때문이다.
    let explicit_project_id = match form.project_id.as_deref().filter(|s| !s.is_empty()) {
        Some(raw) => Some(
            raw.parse::<fleet_core::ProjectId>()
                .map_err(|_| ApiError::BadRequest("invalid project_id".into()))?,
        ),
        None => None,
    };

    // 로드맵 #49 2단계 — Agent 지목. 파싱만 여기서 하고, 존재·핀 충돌·Project
    // 경계 검증은 MCP와 공유하는 `apply_agent_pin`이 아래에서 한다.
    let explicit_agent_id = match form.agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(raw) => Some(
            raw.parse::<fleet_core::AgentId>()
                .map_err(|_| ApiError::BadRequest("invalid agent_id".into()))?,
        ),
        None => None,
    };

    let mut req = fleet_core::TaskRequest {
        prompt: full_prompt,
        cwd: form.cwd.filter(|s| !s.is_empty()),
        model: form.model.filter(|s| !s.is_empty()),
        server_hint: form.server_hint.filter(|s| !s.is_empty()),
        required_labels: vec![],
        max_turns: form.max_turns,
        timeout_secs: form.timeout_secs,
        priority,
        created_by: principal.user.username.clone(),
        parent_task_id: None, // inherit_from_parent가 아래서 채운다.
        project_id: explicit_project_id,
        agent_id: explicit_agent_id,
        // 빈 문자열은 키가 아니다 — HTML 폼은 비어 있는 입력도 `""`로
        // 보내므로, 접지 않으면 키를 쓰지 않는 모든 제출이 `""` 하나를
        // 공유해 서로를 중복으로 판정한다.
        idempotency_key: form
            .idempotency_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        ..Default::default()
    };

    // Agent 지목의 검증은 `Task`를 만들기 **전에** 한다 — 거절될 제출이
    // Store에 행을 남기지 않게 하려는 것이고, `project_id` 상속 결과가
    // 페이로드 해시에 반영되게 하려는 것이기도 하다(같은 폼은 같은 Agent를
    // 거쳐 같은 Project로 귀결되므로 해시는 여전히 결정적이다).
    fleet_store::apply_agent_pin(state.store.as_ref(), &mut req)
        .await
        .map_err(|e| match e {
            fleet_store::TaskPinError::Store(inner) => ApiError::Store(inner.to_string()),
            fleet_store::TaskPinError::Project(fleet_store::ProjectAdmissionError::Store(
                inner,
            )) => ApiError::Store(inner.to_string()),
            other => ApiError::BadRequest(other.to_string()),
        })?;

    let mut task = fleet_core::Task::from_request(req);
    // 주의: 페이로드 해시는 `from_request` 시점, 즉 아래 `inherit_from_parent`
    // **이전**의 요청 값으로 계산된다. 이것이 옳다 — 해시가 식별하는 것은
    // "클라이언트가 보낸 제출"이고, 상속은 그 제출과 부모로부터 결정되는
    // 파생값이다. 같은 폼을 두 번 보내면 상속 결과도 같으므로 해시도 같다.
    if let Some(parent) = &parent_task {
        task.inherit_from_parent(parent);
    }

    // 명시 입력이든 부모에서 상속했든, 최종 project_id를 검증한다 —
    // 부모가 속한 Project가 그 사이 닫혔으면(Draining/Archived) 이어가기도
    // 거절돼야 한다. 검증 규칙은 MCP `fleet_dispatch_task`와 공유한다
    // (`fleet_store::ensure_project_accepts_new_tasks`).
    if let Some(project_id) = task.project_id {
        fleet_store::ensure_project_accepts_new_tasks(state.store.as_ref(), project_id)
            .await
            .map_err(|e| match e {
                fleet_store::ProjectAdmissionError::NotFound(_)
                | fleet_store::ProjectAdmissionError::NotAccepting { .. } => {
                    ApiError::BadRequest(e.to_string())
                }
                fleet_store::ProjectAdmissionError::Store(inner) => {
                    ApiError::Store(inner.to_string())
                }
            })?;
    }

    // 로드맵 #69 — 같은 이유로 상속 **뒤**에 검증한다. 이어가기 작업은
    // 부모의 `cwd`를 물려받으므로, 폼이 비어 있어도 최종 값이 존재할 수 있고
    // 반대로 부모에도 없으면 여기서 걸린다. `submit()` 안에도 같은 게이트가
    // 있지만, 여기서 판정해야 `dispatch failed`가 아니라 400으로 돌아간다.
    fleet_core::validate_workspace_cwd(task.cwd.as_deref())
        .map_err(|e| ApiError::BadRequest(format!("cwd: {e}")))?;

    let task_id = task.id;

    match dispatcher.submit(task).await {
        Ok(id) => {
            // 로드맵 #62 2단계: `submit()`이 멱등성 키로 중복을 흡수하면 **최초**
            // 작업 id를 돌려준다 — 방금 만든 `task_id`가 아니라. 그 차이가
            // 곧 "중복이었다"는 신호이므로 Dispatcher API를 넓히지 않고 여기서
            // 판정한다.
            //
            // 이 플래그가 없으면 신호가 거짓이 된다: 이미 `Completed`인 작업을
            // 되돌려준 경우 아래 `actually_dispatched`는 false가 되고 `warning`
            // 필드는 없는데, 위 문서가 그 조합을 "재시도 예약됨"으로 정의한다.
            // 끝난 작업을 "재시도 대기 중"으로 보고하는 셈이다.
            let deduplicated = id != task_id;
            // 로드맵 #38: 재시도가 활성화된 배포에서는 submit()이 워커 선택
            // 실패/CircuitOpen에서도 Ok를 반환할 수 있다 — 이 경우 작업은
            // 아직 Pending(백그라운드 재시도 대기)이므로 `dispatched: true`를
            // 무조건 단정하지 않고 실제 상태를 조회해 정확히 보고한다.
            let actually_dispatched = state
                .store
                .get_task(id)
                .await
                .ok()
                .flatten()
                .map(|t| matches!(t.status, fleet_core::TaskStatus::Dispatched { .. }))
                .unwrap_or(true);
            Ok(Json(serde_json::json!({
                "task_id": id,
                "dispatched": actually_dispatched,
                "deduplicated": deduplicated,
            })))
        }
        // 멱등성 충돌만 4xx다. 이 경로에서는 Task 행이 **생성되지 않았고**,
        // 클라이언트가 고쳐야 할 것이 분명하다(키를 바꾸거나 원래 페이로드로
        // 보낸다) — 아래 일반 분기의 `200 + warning`은 "Task는 만들어졌지만
        // 디스패치가 미뤄졌다"는 뜻이라 여기에 쓰면 거짓말이 된다.
        Err(fleet_scheduler::DispatchError::IdempotencyConflict {
            key,
            existing_task_id,
        }) => Err(ApiError::Conflict(format!(
            "idempotency key '{key}' was already used with a different payload \
             (existing task {existing_task_id})"
        ))),
        Err(e) => {
            debug!(%task_id, error = %e, "dashboard task submission did not dispatch immediately");
            Ok(Json(serde_json::json!({
                "task_id": task_id,
                "dispatched": false,
                "warning": e.to_string(),
            })))
        }
    }
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
) -> Result<Json<serde_json::Value>, ApiError> {
    require_permission(&principal, PermissionKind::TaskRead)?;

    let task_id: fleet_core::TaskId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid task id: {id}")))?;

    let task = state
        .store
        .get_task(task_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_task_detail: failed");
            ApiError::Store(e.to_string())
        })?
        .ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;

    let summary = task_to_summary(&task);

    // stdout/stderr 출력 조회 (task:output 권한 필요).
    let output = if principal.has(PermissionKind::TaskOutput) {
        state.store.get_output(task_id, 0).await.ok()
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "task": summary,
        "output": output,
    })))
}

/// GET /api/tasks/:id/thread — 이 태스크가 속한 스레드 전체를 시간순으로 조회.
///
/// "이어가기(Reply)" UI가 이전 turn들을 보여주기 위해 사용한다. 단일 태스크
/// (스레드 루트뿐이고 아직 이어간 적 없음)여도 배열엔 그 태스크 하나가
/// 담겨서 온다 — 프런트가 "히스토리 없음"과 "아직 안 불러옴"을 구분할 필요가
/// 없게.
pub async fn get_task_thread_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_permission(&principal, PermissionKind::TaskRead)?;

    let task_id: fleet_core::TaskId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid task id: {id}")))?;

    let task = state
        .store
        .get_task(task_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;

    let thread = state
        .store
        .list_thread_tasks(task.thread_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_task_thread: failed");
            ApiError::Store(e.to_string())
        })?;

    let summaries: Vec<TaskSummary> = thread.iter().map(task_to_summary).collect();
    Ok(Json(serde_json::json!({ "thread": summaries })))
}

/// DELETE /api/tasks/:id — terminal Task 영구 삭제 (`#96`).
///
/// 204는 성공, 404/409/403은 각각 부재·거절·권한없음을 뜻한다 —
/// `docs/contracts/dashboard-api.md`의 응답 코드 표가 정본이다. 세 경우
/// 모두 감사 이벤트를 남긴다: 실패한 삭제 시도도 "누가 무엇을 지우려
/// 했는가"라는 점에서 감사 가치가 있다(`audit.rs`의 기록 원칙).
pub async fn delete_task_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_permission(&principal, PermissionKind::TaskDelete)?;
    verify_csrf_header(&jar, &headers)?;

    let task_id: fleet_core::TaskId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid task id: {id}")))?;

    let outcome = state.store.delete_task(task_id).await.map_err(|e| {
        tracing::error!(error = %e, "delete_task: failed");
        ApiError::from(e)
    })?;

    let result = match &outcome {
        fleet_core::TaskDeleteOutcome::Deleted => Ok(StatusCode::NO_CONTENT),
        fleet_core::TaskDeleteOutcome::NotTerminal { current } => Err(ApiError::Conflict(format!(
            "task {id} is not terminal (current: {current:?})"
        ))),
        fleet_core::TaskDeleteOutcome::BlockedByDependents { dependent_ids } => {
            let ids = dependent_ids
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Err(ApiError::Conflict(format!(
                "task {id} is blocked by pending dependents: {ids}"
            )))
        }
    };

    let audit = match &result {
        Ok(_) => fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::TASK_DELETE,
        ),
        Err(e) => fleet_core::AuditEvent::failure(
            &principal.user.username,
            fleet_core::audit::action::TASK_DELETE,
        )
        .detail(serde_json::json!({ "reason": e.to_string() })),
    }
    .actor(principal.user.id)
    .target("task", task_id.to_string());
    crate::audit::record(&state, audit).await;

    result
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
) -> Result<Json<WorkerDetail>, ApiError> {
    require_permission(&principal, PermissionKind::WorkerList)?;
    let worker_id: fleet_core::WorkerId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid worker id: {id}")))?;

    let worker = state
        .store
        .get_worker(worker_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_worker_detail: failed");
            ApiError::Store(e.to_string())
        })?
        .ok_or_else(|| ApiError::NotFound(format!("worker {id}")))?;

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
            ApiError::Store(e.to_string())
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
///
/// `/api/users`(목록)와 동일하게 `user:read` 권한 필요.
pub async fn admin_users_page(Extension(principal): Extension<AuthPrincipal>) -> Response {
    serve_page_if_permitted(&principal, PermissionKind::UserRead, "admin-users.html")
}

/// GET /api/users — 사용자 목록 JSON API.
pub async fn list_users_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<Vec<UserSummary>>, ApiError> {
    require_permission(&principal, PermissionKind::UserRead)?;
    let users = state.store.list_users().await.map_err(|e| {
        tracing::error!(error = %e, "list_users failed");
        ApiError::Store(e.to_string())
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
) -> Result<(StatusCode, CookieJar), ApiError> {
    require_permission(&principal, PermissionKind::UserCreate)
        .map_err(|_| ApiError::Forbidden("Permission denied".into()))?;

    // CSRF 검증.
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return Err(ApiError::Forbidden("CSRF token invalid".into()));
    }

    // email 검증.
    if let Err(e) = fleet_core::User::validate_email(&form.email) {
        return Err(ApiError::BadRequest(e.to_string()));
    }

    // 비밀번호 강도 검증.
    if fleet_core::auth::password::validate_password(&form.password, &[&form.email]).is_err() {
        return Err(ApiError::BadRequest("Password too weak".into()));
    }

    let hash = fleet_core::auth::password::hash_password(&form.password)
        .map_err(|_| ApiError::Internal("Hash failed".into()))?;

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
            ApiError::Conflict("Username already exists".into())
        } else {
            ApiError::Store(msg)
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
    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::USER_CREATE,
        )
        .actor(principal.user.id)
        .target("user", user.id.as_uuid().to_string())
        .detail(serde_json::json!({ "email": form.email })),
    )
    .await;
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
) -> Result<StatusCode, ApiError> {
    require_permission(&principal, PermissionKind::UserCreate)
        .map_err(|_| ApiError::Forbidden("Permission denied".into()))?;

    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return Err(ApiError::Forbidden("CSRF token invalid".into()));
    }

    let user_id: fleet_core::UserId = uuid::Uuid::parse_str(&id)
        .map(fleet_core::UserId::from)
        .map_err(|_| ApiError::BadRequest("Invalid user ID".into()))?;

    let user = state
        .store
        .get_user_by_id(user_id)
        .await
        .map_err(|_| ApiError::Internal("DB error".into()))?
        .ok_or(ApiError::NotFound("User not found".into()))?;

    let new_enabled = !user.enabled;
    state
        .store
        .set_user_enabled(user_id, new_enabled)
        .await
        .map_err(|_| ApiError::Internal("DB error".into()))?;

    if !new_enabled {
        let _ = state.store.delete_user_sessions(user_id).await;
    }

    tracing::info!(username = %user.username, enabled = new_enabled, by = %principal.user.username, "user toggled");
    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::USER_TOGGLE,
        )
        .actor(principal.user.id)
        .target("user", user_id.as_uuid().to_string())
        .detail(serde_json::json!({
            "target_username": user.username,
            "enabled": new_enabled,
        })),
    )
    .await;
    Ok(StatusCode::OK)
}

/// POST /api/users/:id/delete — 사용자 삭제 (admin only).
pub async fn delete_user_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    Path(id): Path<String>,
    Form(form): Form<ToggleUserForm>,
) -> Result<StatusCode, ApiError> {
    require_permission(&principal, PermissionKind::UserDelete)
        .map_err(|_| ApiError::Forbidden("Permission denied".into()))?;

    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return Err(ApiError::Forbidden("CSRF token invalid".into()));
    }

    let user_id: fleet_core::UserId = uuid::Uuid::parse_str(&id)
        .map(fleet_core::UserId::from)
        .map_err(|_| ApiError::BadRequest("Invalid user ID".into()))?;

    // 자기 자신 삭제 방지.
    if user_id == principal.user.id {
        return Err(ApiError::BadRequest("Cannot delete yourself".into()));
    }

    let user = state
        .store
        .get_user_by_id(user_id)
        .await
        .map_err(|_| ApiError::Internal("DB error".into()))?
        .ok_or(ApiError::NotFound("User not found".into()))?;

    state
        .store
        .delete_user(user_id)
        .await
        .map_err(|_| ApiError::Internal("DB error".into()))?;

    tracing::info!(username = %user.username, by = %principal.user.username, "user deleted");
    // 감사: 삭제된 사용자의 actor_user_id는 FK ON DELETE SET NULL로 NULL이
    // 되지만, target_username이 남아 누구를 지웠는지 추적할 수 있다.
    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::USER_DELETE,
        )
        .actor(principal.user.id)
        .target("user", user_id.as_uuid().to_string())
        .detail(serde_json::json!({ "target_username": user.username })),
    )
    .await;
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

// ── Project 화면 (로드맵 #48 / UI 설계 §3.9·§3.10) ─────────────────────
//
// **UI 설계 문서와의 차이**: `ui-design.md` §3.9는 목록에 "Host/Worker 배정
// 수"를 요구하지만, 모델 정본(`project-feature-design.md`)의 공유 실행 풀
// 불변식은 "Host와 Worker에는 `project_id`를 두지 않는다"고 못 박는다 —
// UI 문서가 그 결정보다 앞서 작성돼 갱신되지 않은 것이다. 화면은 모델
// 정본을 따르고, UI 문서는 이 커밋에서 함께 정정했다.

/// GET /projects — 프로젝트 목록 HTML 페이지.
pub async fn projects_page(Extension(principal): Extension<AuthPrincipal>) -> Response {
    serve_page_if_permitted(&principal, PermissionKind::ProjectRead, "projects.html")
}

/// GET /projects/new — 프로젝트 생성 폼. `ProjectCreate`가 없으면 폼을
/// 아예 보여주지 않는다 — 제출 시점에야 403을 받는 것보다 낫다.
pub async fn project_new_page(Extension(principal): Extension<AuthPrincipal>) -> Response {
    serve_page_if_permitted(
        &principal,
        PermissionKind::ProjectCreate,
        "project-new.html",
    )
}

/// GET /projects/:id — 프로젝트 상세 HTML 페이지.
pub async fn project_detail_page(
    Extension(principal): Extension<AuthPrincipal>,
    Path(_id): Path<String>,
) -> Response {
    serve_page_if_permitted(
        &principal,
        PermissionKind::ProjectRead,
        "project-detail.html",
    )
}

// ── AgentTemplate 관리 화면 (로드맵 #92) ────────────────────────────────
//
// Project 화면과 같은 관례다 — 페이지 자체를 권한으로 가리고(서버측),
// 화면 안의 개별 조작은 JS가 `/api/me`로 한 번 더 숨긴다. 앞의 것이 진짜
// 게이트이고 뒤의 것은 "눌러 본 뒤에야 403을 받는" 경험을 없애는 장치다.

/// GET /agent-templates — 템플릿 목록 HTML 페이지.
pub async fn agent_templates_page(Extension(principal): Extension<AuthPrincipal>) -> Response {
    serve_page_if_permitted(
        &principal,
        PermissionKind::AgentTemplateRead,
        "agent-templates.html",
    )
}

/// GET /agent-templates/new — 템플릿 생성 폼.
///
/// 여기서 만드는 것은 **정체성뿐**이며 항상 `Draft`로 시작한다. 본문은
/// 상세 화면의 revision 폼이 담당한다(`#86`이 두 계층을 나눈 이유).
pub async fn agent_template_new_page(Extension(principal): Extension<AuthPrincipal>) -> Response {
    serve_page_if_permitted(
        &principal,
        PermissionKind::AgentTemplateCreate,
        "agent-template-new.html",
    )
}

/// GET /agent-templates/:id — 템플릿 상세 HTML 페이지.
pub async fn agent_template_detail_page(
    Extension(principal): Extension<AuthPrincipal>,
    Path(_id): Path<String>,
) -> Response {
    serve_page_if_permitted(
        &principal,
        PermissionKind::AgentTemplateRead,
        "agent-template-detail.html",
    )
}

/// GET /api/hosts — 호스트 목록 JSON API.
pub async fn list_hosts_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<Vec<HostSummary>>, ApiError> {
    require_permission(&principal, PermissionKind::DashboardView)?;
    let hosts = state.store.list_hosts().await.map_err(|e| {
        tracing::error!(error = %e, "list_hosts failed");
        ApiError::Store(e.to_string())
    })?;

    // 워커 이름 해결을 위해 워커 목록 조회.
    let workers = state
        .store
        .list_workers(&WorkerFilter::default())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "list_hosts_api: list_workers failed");
            ApiError::Store(e.to_string())
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

// ── Project (로드맵 #48, 1단계) ────────────────────────────────────────
//
// `docs/contracts/project-management.md`의 "차단 중인 표면"에 해당하는
// `PATCH /api/projects/{id}`는 여기 없다. 2026-08-27 보안 모델 승인으로
// 차단 사유가 바뀌었다 — 메타데이터 편집은 Agent를 만들지 않아 권한 문제가
// 아니며, 남은 것은 그 문서가 구현 전에 확정하라고 정한 동시 편집 의미
// (revision 또는 `If-Match`, `request_id`)다. host/worker 배정 endpoint는
// 공유 실행 풀 불변식이 배제해 애초에 생기지 않는다.

/// GET /api/projects — Project 목록 JSON API.
pub async fn list_projects_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<Vec<crate::schema::ProjectSummary>>, ApiError> {
    require_permission(&principal, PermissionKind::ProjectRead)?;
    let projects = state
        .store
        .list_projects(&fleet_core::ProjectFilter {
            status: None,
            limit: 1000,
            offset: 0,
        })
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "list_projects failed");
            ApiError::Store(e.to_string())
        })?;
    Ok(Json(
        projects
            .iter()
            .map(crate::schema::ProjectSummary::from)
            .collect(),
    ))
}

/// POST /api/projects — Project 생성 JSON API.
///
/// CSRF: JS에서 `X-CSRF-Token` 헤더로 전송(`logout`과 동일한 헤더 variant —
/// 이 endpoint는 HTML form이 아니라 JSON body를 받으므로 form 필드에 심는
/// 방식을 쓸 수 없다).
pub async fn create_project_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Json(body): Json<crate::schema::CreateProjectRequest>,
) -> Result<Json<crate::schema::ProjectSummary>, ApiError> {
    require_permission(&principal, PermissionKind::ProjectCreate)?;
    verify_csrf_header(&jar, &headers)?;

    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("name must not be empty".into()));
    }

    let mut project = fleet_core::Project::new(name);
    if let Some(description) = body.description.as_deref().map(str::trim) {
        if !description.is_empty() {
            project = project.with_description(description);
        }
    }
    project = project.with_created_by(principal.user.username.clone());

    state.store.create_project(&project).await.map_err(|e| {
        if let fleet_store::StoreError::Conflict(msg) = &e {
            return ApiError::Conflict(msg.clone());
        }
        tracing::error!(error = %e, "create_project failed");
        ApiError::Store(e.to_string())
    })?;

    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::PROJECT_CREATE,
        )
        .actor(principal.user.id)
        .target("project", project.id.to_string())
        .detail(serde_json::json!({ "name": project.name })),
    )
    .await;

    Ok(Json(crate::schema::ProjectSummary::from(&project)))
}

/// GET /api/projects/:id — Project 상세 JSON API.
pub async fn get_project_detail_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<crate::schema::ProjectSummary>, ApiError> {
    require_permission(&principal, PermissionKind::ProjectRead)?;
    let project_id = parse_project_id(&id)?;
    let project = state
        .store
        .get_project(project_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("project {id}")))?;
    Ok(Json(crate::schema::ProjectSummary::from(&project)))
}

/// DELETE /api/projects/:id — archive 요청(즉시 영구 삭제 아님).
///
/// `docs/contracts/project-management.md`: "`Active → Draining` idempotent
/// archive 요청". 재호출은 안전하다 — 이미 `Draining`/`Archived`인
/// Project에 다시 호출해도 현재 상태를 그대로 반환한다. `Active`면
/// `Draining`으로 전이하고, archive 게이트(비종료 Task 없음 **그리고** 살아
/// 있는 Agent 없음)를 통과하면 같은 요청 안에서 곧바로 `Archived`까지
/// 진행한다 — effect ledger가 없는 지금은 그 이상 기다릴 대상이 없다.
///
/// 게이트가 막으면 `draining` 상태에 더해 **무엇이 막았는지**를
/// `archive_blocked_by`로 함께 돌려준다. 두 조건은 해소 방법이 다르다 —
/// Task는 끝나기를 기다리면 되지만 `Ready` Agent는 저절로 끝나지 않고 사람이
/// 회수해야 한다. 사유 없이 상태만 주면 호출자가 안내할 수 있는 것은 추측뿐이고,
/// 실제로 그렇게 틀린 안내가 나갔다.
pub async fn delete_project_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::schema::ProjectArchiveResponse>, ApiError> {
    require_permission(&principal, PermissionKind::ProjectDelete)?;
    verify_csrf_header(&jar, &headers)?;

    let project_id = parse_project_id(&id)?;
    let mut project = state
        .store
        .get_project(project_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("project {id}")))?;

    // archive 절차는 MCP `fleet_delete_project`와 공유한다
    // (`fleet_store::advance_project_archive`) — 계약 문서가 두 표면의 동일
    // 동작을 요구하므로 규칙을 각자 구현하지 않는다. 상태 전이는 콜백으로
    // 받아 이 표면의 감사 파이프라인에 기록한다.
    let mut transitions = Vec::new();
    let progress =
        fleet_store::advance_project_archive(state.store.as_ref(), &mut project, |status| {
            transitions.push(status)
        })
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;

    for status in transitions {
        let action = match status {
            fleet_core::ProjectStatus::Draining => {
                fleet_core::audit::action::PROJECT_ARCHIVE_REQUESTED
            }
            fleet_core::ProjectStatus::Archived => fleet_core::audit::action::PROJECT_ARCHIVED,
            // advance_project_archive는 Active로 되돌리지 않는다.
            fleet_core::ProjectStatus::Active => continue,
        };
        crate::audit::record(
            &state,
            fleet_core::AuditEvent::success(&principal.user.username, action)
                .actor(principal.user.id)
                .target("project", project.id.to_string()),
        )
        .await;
    }

    let archive_blocked_by = match progress {
        fleet_store::ArchiveProgress::Draining(blockers) => {
            blockers.labels().into_iter().map(str::to_string).collect()
        }
        fleet_store::ArchiveProgress::Archived => Vec::new(),
    };

    Ok(Json(crate::schema::ProjectArchiveResponse {
        project: crate::schema::ProjectSummary::from(&project),
        archive_blocked_by,
    }))
}

fn parse_worker_id(raw: &str) -> Result<fleet_core::WorkerId, ApiError> {
    raw.parse::<fleet_core::WorkerId>()
        .map_err(|_| ApiError::BadRequest("invalid worker id".into()))
}

fn parse_project_id(raw: &str) -> Result<fleet_core::ProjectId, ApiError> {
    raw.parse::<fleet_core::ProjectId>()
        .map_err(|_| ApiError::BadRequest("invalid project id".into()))
}

// ── Agent (로드맵 #49, 1단계) ──────────────────────────────────────────
//
// `PATCH /api/agents/{id}`가 없는 것은 미구현이 아니라 규칙이다 — Agent의
// `project_id`는 불변이고, 이름/설명만 고칠 수 있는 endpoint를 지금 만들면
// 나중에 "무엇은 고칠 수 있고 무엇은 못 고치는가"를 필드별로 방어해야
// 한다. 옮기고 싶으면 대상 Project에 새 Agent를 만든다.

/// `GET /api/agents` 쿼리 파라미터.
#[derive(Debug, Clone, Deserialize)]
pub struct ListAgentsQuery {
    #[serde(default)]
    pub project_id: Option<String>,
    /// 배정된 Worker로 거른다 (로드맵 #67 4a). "어느 Worker도 배정되지
    /// 않은 것만"을 뽑는 값은 없다 — 그 질문의 소비자가 아직 없고, 없는
    /// 소비자를 위한 필터는 항상 비어 있는 컬럼과 같은 종류의 부채다.
    #[serde(default)]
    pub worker_id: Option<String>,
}

/// GET /api/agents — Agent 목록 JSON API.
///
/// `project_id` 쿼리 파라미터로 필터링한다. Project 상세 화면이 유일한
/// 소비자라 Task 목록처럼 클라이언트에서 거를 수도 있었지만, Agent는
/// **항상** 하나의 Project에 속하므로 서버측 필터가 자연스럽고
/// `idx_agents_project_status`가 그대로 쓰인다.
pub async fn list_agents_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Query(query): Query<ListAgentsQuery>,
) -> Result<Json<Vec<crate::schema::AgentSummary>>, ApiError> {
    require_permission(&principal, PermissionKind::AgentRead)?;
    let project_id = match query.project_id.as_deref() {
        Some(raw) => Some(parse_project_id(raw)?),
        None => None,
    };
    let worker_id = match query.worker_id.as_deref() {
        Some(raw) => Some(parse_worker_id(raw)?),
        None => None,
    };
    let agents = state
        .store
        .list_agents(&fleet_core::AgentFilter {
            project_id,
            status: None,
            worker_id,
            limit: 1000,
            offset: 0,
        })
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "list_agents failed");
            ApiError::Store(e.to_string())
        })?;
    Ok(Json(
        agents
            .iter()
            .map(crate::schema::AgentSummary::from)
            .collect(),
    ))
}

/// POST /api/agents — Agent 생성 JSON API.
pub async fn create_agent_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Json(body): Json<crate::schema::CreateAgentRequest>,
) -> Result<Json<crate::schema::AgentSummary>, ApiError> {
    require_permission(&principal, PermissionKind::AgentManage)?;
    verify_csrf_header(&jar, &headers)?;

    let project_id = parse_project_id(&body.project_id)?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("name must not be empty".into()));
    }

    // 소속 Project 검증은 MCP `fleet_create_agent`와 같은 함수를 쓴다 —
    // 두 표면이 같은 규칙을 집행해야 하므로 술어를 복제하지 않는다.
    // 이 검사는 되돌릴 수 없다: 통과하면 그 `project_id`가 확정된다.
    fleet_store::ensure_project_accepts_new_agents(state.store.as_ref(), project_id)
        .await
        .map_err(|e| match e {
            fleet_store::ProjectAdmissionError::NotFound(_)
            | fleet_store::ProjectAdmissionError::NotAccepting { .. } => {
                ApiError::BadRequest(e.to_string())
            }
            fleet_store::ProjectAdmissionError::Store(inner) => ApiError::Store(inner.to_string()),
        })?;

    let mut agent = fleet_core::Agent::new(project_id, name);
    if let Some(description) = body.description.as_deref().map(str::trim) {
        if !description.is_empty() {
            agent = agent.with_description(description);
        }
    }
    agent = agent.with_created_by(principal.user.username.clone());

    // 템플릿 pin (로드맵 #86). 한쪽만 준 요청은 여기서 400으로 끊는다 —
    // 코어의 `AgentTemplatePin`이 절반만 채워진 상태를 표현할 수 없으므로
    // 이 자리에서 거르지 않으면 조용히 무시되어 pin 없는 Agent가 만들어진다.
    match (
        body.agent_template_id.as_deref(),
        body.agent_template_revision_id.as_deref(),
    ) {
        (Some(t), Some(r)) => {
            let template_id = parse_agent_template_id(t)?;
            let revision_id = r
                .parse::<fleet_core::AgentTemplateRevisionId>()
                .map_err(|_| ApiError::BadRequest("invalid revision id".into()))?;
            agent = agent.with_template_pin(fleet_core::AgentTemplatePin {
                template_id,
                revision_id,
            });
        }
        (None, None) => {}
        _ => {
            return Err(ApiError::BadRequest(
                "agent_template_id and agent_template_revision_id must be given together".into(),
            ))
        }
    }

    // Worker 배정 (로드맵 #67 4a). 후보가 없어도 생성은 계속된다 —
    // Agent 정의가 Worker 가용성에 인질로 잡히면 안 되기 때문이며, 그때
    // 남는 `worker_id = NULL`은 `POST /api/agents/{id}/place`로 회복
    // 가능하다. 별도 UPDATE가 아니라 **같은 INSERT**에 실어 보내므로,
    // 중간에 실패해서 "아무도 고치지 않을 미배정 행"이 남는 경우가 없다.
    if let Some((worker_id, assigned_at)) =
        fleet_scheduler::placement::place_on_create(state.store.as_ref()).await
    {
        agent = agent.with_placement(worker_id, assigned_at);
    }

    // pin이 지금도 유효한지(revoke·retire 여부)는 Store가 트랜잭션 안에서
    // 본다. 여기서 미리 읽어 검사하면 그 사이에 revoke가 끼어들 수 있다.
    let placed = state.store.create_agent(&agent).await.map_err(|e| {
        if let fleet_store::StoreError::Conflict(msg) = &e {
            return ApiError::Conflict(msg.clone());
        }
        tracing::error!(error = %e, "create_agent failed");
        ApiError::Store(e.to_string())
    })?;

    // 저장된 사실에 로컬 구조체를 되맞춘다 (로드맵 `#67` 구현 게이트 ①-A-2).
    // Store가 상한 잠금 아래에서 선점에 실패하면 배정 없이 생성하는데,
    // 아래 감사 로그와 응답이 모두 이 구조체에서 나오므로 되맞추지 않으면
    // **일어나지 않은 배정을 기록한다**. 응답의 거짓은 다시 조회하면
    // 드러나지만 감사 로그의 거짓은 남는다.
    if placed != agent.worker_id {
        tracing::info!(
            target: "fleet::placement",
            agent_id = %agent.id,
            attempted_worker = ?agent.worker_id.map(|w| w.to_string()),
            "placement dropped at slot claim; agent created unplaced"
        );
        agent = agent.without_placement();
    }

    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::AGENT_CREATE,
        )
        .actor(principal.user.id)
        .target("agent", agent.id.to_string())
        .detail(serde_json::json!({
            "name": agent.name,
            "project_id": agent.project_id.to_string(),
            // 생성 시점 배정은 `agent.assign`을 따로 내지 않는다. 여기에
            // 실어 두면 "이 Agent가 몇 번 옮겨졌는가"가 `agent.assign`
            // 건수로 그대로 세어진다 — 생성이 그 수를 1 부풀리지 않는다.
            "worker_id": agent.worker_id.map(|w| w.to_string()),
        })),
    )
    .await;

    Ok(Json(crate::schema::AgentSummary::from(&agent)))
}

/// POST /api/agents/:id/place — Agent를 Worker에 (재)배정한다 (로드맵 #67 4a).
///
/// 생성 시점 배정이 자동이므로 이 경로는 **예외 처리용**이다. 존재해야
/// 하는 이유는 하나뿐이다: 생성은 배정 실패로 막히지 않으므로 `worker_id`가
/// `NULL`인 Agent가 정상적으로 생길 수 있고, 이 경로가 없으면 그 상태가
/// 영구히 고착된다. Worker 등록 해제로 배정이 풀린 Agent도 같은 자리로
/// 돌아온다.
///
/// `DELETE`(회수)와 달리 idempotent하지 않다 — 같은 Worker로 두 번 불러도
/// `assigned_at`이 갱신된다. 회수는 "언제 회수됐는가"가 기록이라 밀리면
/// 안 되지만, 배정은 "언제 이 자리를 다시 확인했는가"가 기록이기 때문이다.
pub async fn place_agent_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<crate::schema::PlaceAgentRequest>,
) -> Result<Json<crate::schema::AgentSummary>, ApiError> {
    require_permission(&principal, PermissionKind::AgentManage)?;
    verify_csrf_header(&jar, &headers)?;

    let agent_id = id
        .parse::<fleet_core::AgentId>()
        .map_err(|_| ApiError::BadRequest("invalid agent id".into()))?;
    let agent = state
        .store
        .get_agent(agent_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id}")))?;

    // 회수된 Agent는 배정하지 않는다. 원장(`count_agents_by_worker`)이
    // `stopped`를 세지 않으므로 배정해도 부하에 잡히지 않고, 4b의 desired
    // state도 실릴 일이 없다 — 아무도 읽지 않는 값을 쓰는 것이 된다.
    if agent.status == fleet_core::AgentStatus::Stopped {
        return Err(ApiError::BadRequest(format!(
            "agent {agent_id} is stopped and cannot be placed"
        )));
    }

    let worker_id = match body.worker_id.as_deref() {
        Some(raw) => parse_worker_id(raw)?,
        None => fleet_scheduler::placement::choose_worker(state.store.as_ref())
            .await
            .map_err(|e| match e {
                fleet_scheduler::placement::PlacementError::Store(inner) => {
                    ApiError::Store(inner.to_string())
                }
                // 후보 없음은 서버 오류가 아니라 지금의 fleet 상태다 —
                // 409로 돌려 운영자가 Worker를 올린 뒤 재시도하게 한다.
                other => ApiError::Conflict(other.to_string()),
            })?,
    };

    let previous_worker_id = agent.worker_id;
    let claim = state
        .store
        .assign_agent_worker(agent_id, worker_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;
    match claim {
        fleet_store::SlotClaim::Claimed => {}
        // 지금의 fleet 상태이지 요청의 결함이 아니다 — 위의 후보 없음과
        // 같은 이유로 409다. 운영자는 다른 Worker를 지목하거나 그 Worker에서
        // Agent를 회수한 뒤 그대로 다시 시도하면 된다.
        fleet_store::SlotClaim::CapReached => {
            return Err(ApiError::Conflict(format!(
                "worker {worker_id} is at its agent process cap"
            )))
        }
        fleet_store::SlotClaim::NoSuchAgent => {
            return Err(ApiError::NotFound(format!("agent {id}")))
        }
        // 등록 해제가 선택과 선점 사이에 끼어든 경우. 예전에는 FK 위반이
        // `Conflict`로 올라와 400이 됐고, 여기서도 400을 유지한다 —
        // 다시 시도해도 같은 답이 나오는 종류의 실패다.
        fleet_store::SlotClaim::NoSuchWorker => {
            return Err(ApiError::BadRequest(format!(
                "no such worker for agent placement: {worker_id}"
            )))
        }
    }

    // `assigned_at`은 DB의 `NOW()`가 정하므로 로컬 구조체를 고쳐 응답하면
    // 저장된 값과 다른 시각을 싣게 된다.
    let placed = state
        .store
        .get_agent(agent_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .ok_or_else(|| ApiError::Store("agent disappeared during placement".into()))?;

    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::AGENT_ASSIGN,
        )
        .actor(principal.user.id)
        .target("agent", agent_id.to_string())
        .detail(serde_json::json!({
            "worker_id": worker_id.to_string(),
            // 이전 배정을 함께 남긴다 — 이것이 없으면 감사 로그가 "어디로
            // 갔는가"만 말하고 "어디에서 왔는가"는 앞 이벤트를 거슬러
            // 올라가야 알 수 있다. 최초 배정이면 `null`이다.
            "previous_worker_id": previous_worker_id.map(|w| w.to_string()),
        })),
    )
    .await;

    Ok(Json(crate::schema::AgentSummary::from(&placed)))
}

/// POST /api/agents/:id/start — desired state를 `running`으로 (로드맵 #67 4b).
///
/// **생성도 배정도 이 자리를 대신하지 않는다.** 4a가 생성 시점에 자동으로
/// 배정하므로 "배정 ⇒ running"은 곧 "생성 ⇒ running"이고, 그러면
/// `AgentStatus::Ready`의 정의("정의는 끝났고 시작 명령을 **받을 수 있다**")가
/// 빈다. 회수가 이미 `DELETE /api/agents/{id}`라는 명시적 표면인 것과 대칭이다.
///
/// 4c 전에는 이 호출이 프로세스를 띄우지 않는다 — 의도를 기록하고 heartbeat에
/// 실을 뿐이다.
pub async fn start_agent_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::schema::AgentSummary>, ApiError> {
    require_permission(&principal, PermissionKind::AgentManage)?;
    verify_csrf_header(&jar, &headers)?;

    let agent_id = id
        .parse::<fleet_core::AgentId>()
        .map_err(|_| ApiError::BadRequest("invalid agent id".into()))?;
    let mut agent = state
        .store
        .get_agent(agent_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id}")))?;

    // 회수는 종단이다 — `place_agent_api`가 같은 기준으로 거절한다. 이 판정을
    // `set_agent_desired_status`의 UPDATE 술어(`status <> 'stopped'`)로 내리지
    // 않는 이유는, 그러면 "바뀐 것이 없음"과 "그런 Agent가 없음"이 같은
    // 0-row로 뭉개져 404와 400을 구분할 수 없게 되기 때문이다.
    if agent.status == fleet_core::AgentStatus::Stopped {
        return Err(ApiError::BadRequest(format!(
            "agent {agent_id} is stopped and cannot be started"
        )));
    }

    // **미배정(`worker_id` 없음)은 거부하지 않는다.** 명령은 갈 곳이 없을 뿐
    // 잃어버리지 않으며, 다음 배정이 세대를 올릴 때 실려 간다. 여기서 거부하면
    // `NULL`에서의 회복이 "먼저 배정하고 그다음 start"라는 순서 제약을 갖는데,
    // 그 순서를 강제할 이유가 없다.
    if agent.desired_status != fleet_core::AgentDesiredStatus::Running {
        state
            .store
            .set_agent_desired_status(agent_id, fleet_core::AgentDesiredStatus::Running)
            .await
            .map_err(|e| ApiError::Store(e.to_string()))?;
        // 세대는 저장소가 정하므로 로컬 필드를 고쳐 응답하면 실제 발행된
        // 세대와 다른 값을 싣게 된다(`place_agent_api`의 `assigned_at`과 같은
        // 이유).
        agent = state
            .store
            .get_agent(agent_id)
            .await
            .map_err(|e| ApiError::Store(e.to_string()))?
            .ok_or_else(|| ApiError::Store("agent disappeared during start".into()))?;

        crate::audit::record(
            &state,
            fleet_core::AuditEvent::success(
                &principal.user.username,
                fleet_core::audit::action::AGENT_START,
            )
            .actor(principal.user.id)
            .target("agent", agent.id.to_string())
            .detail(serde_json::json!({
                "project_id": agent.project_id.to_string(),
                "generation": agent.command_generation,
                // 미배정이면 `null`이다 — 명령이 어디로도 가지 않은 채
                // 발행됐다는 사실 자체가 기록될 값이다.
                "worker_id": agent.worker_id.map(|w| w.to_string()),
            })),
        )
        .await;
    }

    Ok(Json(crate::schema::AgentSummary::from(&agent)))
}

/// DELETE /api/agents/:id — Agent 회수(`Ready → Stopped`).
///
/// idempotent다 — 이미 `Stopped`면 아무것도 쓰지 않고 현재 상태를
/// 반환한다. 재호출마다 `updated_at`을 갱신하면 "언제 회수됐는가"라는
/// 기록이 계속 밀리기 때문이다. 행을 지우지 않는 이유는 감사 대상이기
/// 때문이며, `Stopped`가 된 Agent는 소속 Project의 archive를 더는 막지
/// 않는다([`fleet_store::advance_project_archive`]).
pub async fn stop_agent_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::schema::AgentSummary>, ApiError> {
    require_permission(&principal, PermissionKind::AgentManage)?;
    verify_csrf_header(&jar, &headers)?;

    let agent_id = id
        .parse::<fleet_core::AgentId>()
        .map_err(|_| ApiError::BadRequest("invalid agent id".into()))?;
    let mut agent = state
        .store
        .get_agent(agent_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id}")))?;

    if agent.status != fleet_core::AgentStatus::Stopped {
        state
            .store
            .update_agent_status(agent.id, fleet_core::AgentStatus::Stopped)
            .await
            .map_err(|e| ApiError::Store(e.to_string()))?;
        // 로컬 필드만 고치면 응답의 `updated_at`이 회수 이전 값이 된다 —
        // idempotent 재호출이 다른 값을 돌려주지 않도록 저장된 행을 다시
        // 읽는다(MCP `fleet_stop_agent`와 동일).
        agent = state
            .store
            .get_agent(agent.id)
            .await
            .map_err(|e| ApiError::Store(e.to_string()))?
            .ok_or_else(|| ApiError::Store("agent disappeared during stop".into()))?;

        crate::audit::record(
            &state,
            fleet_core::AuditEvent::success(
                &principal.user.username,
                fleet_core::audit::action::AGENT_STOP,
            )
            .actor(principal.user.id)
            .target("agent", agent.id.to_string())
            .detail(serde_json::json!({ "project_id": agent.project_id.to_string() })),
        )
        .await;
    }

    Ok(Json(crate::schema::AgentSummary::from(&agent)))
}

// ── AgentTemplate (로드맵 #86, 1단계) ──────────────────────────────────
//
// capability 여섯 종이 검사받는 자리다. 표면 없이 권한만 추가하면
// `auth.rs`가 네 번 명시적으로 거절해 온 "죽은 권한"이 되므로 같은
// 커밋에 들어간다.
//
// **MCP 표면은 의도적으로 없다.** LLM이 직접 부르는 표면에 템플릿 편집
// 권한을 주면 Agent가 자기 role prompt와 도구 목록을 스스로 고칠 수 있고,
// 그것이 정확히 이 도메인이 막으려는 권한 상승 경로다.
//
// 편집(`PATCH`)이 없는 것도 규칙이다 — 본문은 revision으로만 바뀐다.
// 정체성 행(`name`/`description`)의 수정은 revision 이력에 남지 않아
// 감사에서 본문 변경과 구분이 안 되므로, 필요해질 때 별도로 설계한다.

/// 전역 템플릿(`project_id IS NULL`)을 건드리려면 추가 capability를 요구한다.
///
/// 전역 템플릿은 모든 Project가 볼 수 있으므로, Project 하나에 대한 권한으로
/// 전체에 영향을 주는 편집을 허용하면 범위가 조용히 새어 나간다. Project
/// 범위 템플릿은 이 검사를 통과시키고, Project별 세분화는 `#48`의 정책
/// 컬럼이 생긴 뒤에 붙인다(그 전에는 검사할 대상이 없다).
fn authorize_template_scope(
    principal: &AuthPrincipal,
    project_id: Option<fleet_core::ProjectId>,
) -> Result<(), ApiError> {
    if project_id.is_none() {
        require_permission(principal, PermissionKind::AgentTemplateManageGlobal)?;
    }
    Ok(())
}

fn parse_agent_template_id(raw: &str) -> Result<fleet_core::AgentTemplateId, ApiError> {
    raw.parse::<fleet_core::AgentTemplateId>()
        .map_err(|_| ApiError::BadRequest("invalid agent template id".into()))
}

async fn load_agent_template(
    state: &DashboardState,
    id: fleet_core::AgentTemplateId,
) -> Result<fleet_core::AgentTemplate, ApiError> {
    state
        .store
        .get_agent_template(id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_agent_template failed");
            ApiError::Store(e.to_string())
        })?
        .ok_or_else(|| ApiError::NotFound("agent template not found".into()))
}

/// `GET /api/agent-templates` 쿼리 파라미터.
#[derive(Debug, Clone, Deserialize)]
pub struct ListAgentTemplatesQuery {
    /// 주면 그 Project의 템플릿만, `global=true`면 전역만, 둘 다 없으면 전부.
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub global: Option<bool>,
    #[serde(default)]
    pub status: Option<String>,
}

/// GET /api/agent-templates — 템플릿 목록.
pub async fn list_agent_templates_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Query(query): Query<ListAgentTemplatesQuery>,
) -> Result<Json<Vec<crate::schema::AgentTemplateSummary>>, ApiError> {
    require_permission(&principal, PermissionKind::AgentTemplateRead)?;

    // `project_scope`는 3상태다: `None`=전부, `Some(None)`=전역만,
    // `Some(Some(p))`=그 Project만. 쿼리 문자열은 그것을 직접 표현할 수
    // 없으므로 두 파라미터로 편다.
    let project_scope = match (query.project_id.as_deref(), query.global) {
        (Some(_), Some(true)) => {
            return Err(ApiError::BadRequest(
                "project_id and global=true are mutually exclusive".into(),
            ))
        }
        (Some(raw), _) => Some(Some(parse_project_id(raw)?)),
        (None, Some(true)) => Some(None),
        (None, _) => None,
    };
    let status = match query.status.as_deref() {
        Some(raw) => Some(
            fleet_core::AgentTemplateStatus::parse_str(raw)
                .ok_or_else(|| ApiError::BadRequest(format!("unknown status: {raw}")))?,
        ),
        None => None,
    };

    let templates = state
        .store
        .list_agent_templates(&fleet_core::AgentTemplateFilter {
            project_scope,
            status,
            limit: 1000,
            offset: 0,
        })
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "list_agent_templates failed");
            ApiError::Store(e.to_string())
        })?;

    Ok(Json(
        templates
            .iter()
            .map(crate::schema::AgentTemplateSummary::from)
            .collect(),
    ))
}

/// POST /api/agent-templates — 템플릿 정체성 생성 (항상 `Draft`).
///
/// 본문은 여기서 만들지 않는다. 정체성과 본문을 한 번에 만들면 "본문 없는
/// 템플릿"이라는 상태가 없어져 편하지만, 그 대신 첫 본문만 다른 경로로
/// 저장되어 revision 이력의 시작점이 두 종류가 된다.
pub async fn create_agent_template_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Json(body): Json<crate::schema::CreateAgentTemplateRequest>,
) -> Result<Json<crate::schema::AgentTemplateSummary>, ApiError> {
    require_permission(&principal, PermissionKind::AgentTemplateCreate)?;
    verify_csrf_header(&jar, &headers)?;

    let project_id = match body.project_id.as_deref() {
        Some(raw) => Some(parse_project_id(raw)?),
        None => None,
    };
    authorize_template_scope(&principal, project_id)?;

    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("name must not be empty".into()));
    }

    let mut template = fleet_core::AgentTemplate::new(project_id, name);
    if let Some(description) = body.description.as_deref().map(str::trim) {
        if !description.is_empty() {
            template = template.with_description(description);
        }
    }
    template = template.with_created_by(principal.user.username.clone());

    state
        .store
        .create_agent_template(&template)
        .await
        .map_err(|e| {
            if let fleet_store::StoreError::Conflict(msg) = &e {
                return ApiError::Conflict(msg.clone());
            }
            tracing::error!(error = %e, "create_agent_template failed");
            ApiError::Store(e.to_string())
        })?;

    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::AGENT_TEMPLATE_CREATE,
        )
        .actor(principal.user.id)
        .target("agent_template", template.id.to_string())
        .detail(serde_json::json!({
            "name": template.name,
            "project_id": template.project_id.map(|p| p.to_string()),
        })),
    )
    .await;

    Ok(Json(crate::schema::AgentTemplateSummary::from(&template)))
}

/// GET /api/agent-templates/:id — 템플릿 한 건.
///
/// 목록을 받아 클라이언트에서 거를 수도 있지만, 그러면 "없는 템플릿"과
/// "id를 잘못 친 템플릿"이 모두 빈 결과가 되어 상세 화면이 404를 표시할
/// 방법이 없다. `load_agent_template`이 그 구분을 이미 갖고 있으므로
/// 표면에서 다시 만들지 않고 노출만 한다.
pub async fn get_agent_template_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<crate::schema::AgentTemplateSummary>, ApiError> {
    require_permission(&principal, PermissionKind::AgentTemplateRead)?;
    let template_id = parse_agent_template_id(&id)?;
    let template = load_agent_template(&state, template_id).await?;
    Ok(Json(crate::schema::AgentTemplateSummary::from(&template)))
}

/// GET /api/agent-templates/:id/revisions — revision 이력.
pub async fn list_agent_template_revisions_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<Vec<crate::schema::AgentTemplateRevisionSummary>>, ApiError> {
    require_permission(&principal, PermissionKind::AgentTemplateRead)?;
    let template_id = parse_agent_template_id(&id)?;
    // 없는 템플릿과 revision이 0건인 템플릿을 구분한다 — 전자는 404여야
    // 하고, 빈 배열은 후자만을 뜻해야 한다.
    load_agent_template(&state, template_id).await?;

    let revisions = state
        .store
        .list_agent_template_revisions(template_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "list_agent_template_revisions failed");
            ApiError::Store(e.to_string())
        })?;

    Ok(Json(
        revisions
            .iter()
            .map(crate::schema::AgentTemplateRevisionSummary::from)
            .collect(),
    ))
}

/// POST /api/agent-templates/:id/revisions — 새 revision 발행.
///
/// 요구 capability는 **무엇이 바뀌었는지에 따라 달라진다**
/// ([`fleet_core::AgentTemplateBody::required_permissions_for_change`]).
/// role prompt만 고치면 `agent_template:update`로 충분하지만, 도구/스킬
/// 목록이 바뀌면 `agent:manage`를 추가로 요구한다 — 그쪽은 Agent가 무엇을
/// 실행할 수 있는지를 넓히는 변경이라 문구 교정과 같은 등급일 수 없다.
///
/// 첫 revision은 빈 본문에서의 변경으로 취급한다. 따라서 도구를 하나라도
/// 실은 첫 revision은 처음부터 `agent:manage`를 요구한다.
pub async fn create_agent_template_revision_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<crate::schema::CreateAgentTemplateRevisionRequest>,
) -> Result<Json<crate::schema::AgentTemplateRevisionSummary>, ApiError> {
    require_permission(&principal, PermissionKind::AgentTemplateUpdate)?;
    verify_csrf_header(&jar, &headers)?;

    let template_id = parse_agent_template_id(&id)?;
    let template = load_agent_template(&state, template_id).await?;
    authorize_template_scope(&principal, template.project_id)?;

    let next = fleet_core::AgentTemplateBody::new(body.role_prompt.clone())
        .with_tools(body.tools.clone())
        .with_skills(body.skills.clone());

    let revisions = state
        .store
        .list_agent_template_revisions(template_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "list_agent_template_revisions failed");
            ApiError::Store(e.to_string())
        })?;
    let current = revisions
        .first()
        .map(|r| r.body.clone())
        .unwrap_or_else(|| fleet_core::AgentTemplateBody::new(""));

    for needed in current.required_permissions_for_change(&next) {
        require_permission(&principal, needed)?;
    }

    let revision = state
        .store
        .create_agent_template_revision(template_id, &next, Some(principal.user.username.as_str()))
        .await
        .map_err(|e| {
            if let fleet_store::StoreError::Conflict(msg) = &e {
                return ApiError::Conflict(msg.clone());
            }
            tracing::error!(error = %e, "create_agent_template_revision failed");
            ApiError::Store(e.to_string())
        })?;

    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::AGENT_TEMPLATE_REVISION_CREATE,
        )
        .actor(principal.user.id)
        .target("agent_template", template_id.to_string())
        .detail(serde_json::json!({
            "revision_id": revision.id.to_string(),
            "content_revision": revision.content_revision,
            "content_hash": revision.content_hash,
        })),
    )
    .await;

    Ok(Json(crate::schema::AgentTemplateRevisionSummary::from(
        &revision,
    )))
}

/// POST /api/agent-templates/:id/revisions/:revision_id/revoke — 새 pin 차단.
///
/// idempotent다 — 이미 revoke된 revision이면 아무것도 쓰지 않고 현재
/// 상태를 돌려준다. 이미 이 revision을 pin한 Agent는 영향받지 않는다.
pub async fn revoke_agent_template_revision_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path((id, revision_id)): Path<(String, String)>,
) -> Result<Json<crate::schema::AgentTemplateRevisionSummary>, ApiError> {
    require_permission(&principal, PermissionKind::AgentTemplateRevisionRevoke)?;
    verify_csrf_header(&jar, &headers)?;

    let template_id = parse_agent_template_id(&id)?;
    let template = load_agent_template(&state, template_id).await?;
    authorize_template_scope(&principal, template.project_id)?;

    let revision_id = revision_id
        .parse::<fleet_core::AgentTemplateRevisionId>()
        .map_err(|_| ApiError::BadRequest("invalid revision id".into()))?;

    let revision = state
        .store
        .get_agent_template_revision(revision_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_agent_template_revision failed");
            ApiError::Store(e.to_string())
        })?
        .ok_or_else(|| ApiError::NotFound("revision not found".into()))?;
    // 경로의 템플릿에 속하지 않는 revision을 그 템플릿의 권한으로 revoke하면
    // 범위 검사를 우회하게 된다.
    if revision.template_id != template_id {
        return Err(ApiError::NotFound("revision not found".into()));
    }

    let changed = state
        .store
        .revoke_agent_template_revision(revision_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "revoke_agent_template_revision failed");
            ApiError::Store(e.to_string())
        })?;

    if !changed {
        return Ok(Json(crate::schema::AgentTemplateRevisionSummary::from(
            &revision,
        )));
    }

    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::AGENT_TEMPLATE_REVISION_REVOKE,
        )
        .actor(principal.user.id)
        .target("agent_template", template_id.to_string())
        .detail(serde_json::json!({
            "revision_id": revision_id.to_string(),
            "content_revision": revision.content_revision,
        })),
    )
    .await;

    // 저장된 값을 다시 읽어 `revoked_at`을 응답에 담는다 — 메모리의 사본은
    // 그 시각을 모른다.
    let stored = state
        .store
        .get_agent_template_revision(revision_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_agent_template_revision failed");
            ApiError::Store(e.to_string())
        })?
        .ok_or_else(|| ApiError::NotFound("revision not found".into()))?;
    Ok(Json(crate::schema::AgentTemplateRevisionSummary::from(
        &stored,
    )))
}

/// GET /api/agent-templates/:id/dependents — 이 템플릿에 pin한 Agent 목록.
///
/// retire 확인 화면이 쓰는 값이며, 함께 돌려주는 `dependent_set_hash`를
/// 그대로 retire 요청에 실어야 한다.
pub async fn agent_template_dependents_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<crate::schema::AgentTemplateDependents>, ApiError> {
    require_permission(&principal, PermissionKind::AgentTemplateRead)?;
    let template_id = parse_agent_template_id(&id)?;
    load_agent_template(&state, template_id).await?;

    let dependents = state
        .store
        .agent_template_dependents(template_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "agent_template_dependents failed");
            ApiError::Store(e.to_string())
        })?;

    Ok(Json(crate::schema::AgentTemplateDependents {
        template_id: template_id.to_string(),
        dependent_set_hash: fleet_core::dependent_set_hash(&dependents),
        agent_ids: dependents.iter().map(|a| a.to_string()).collect(),
    }))
}

/// POST /api/agent-templates/:id/status — 수명 주기 전이.
///
/// 전이 유효성은 코어의 표(`can_transition_to`)가 정한다. 표면이 자기
/// 판단을 갖지 않아야 MCP나 CLI가 나중에 붙어도 같은 규칙이 걸린다.
///
/// `retired`만 `dependent_set_hash`를 요구한다. 나머지 전이는 이미 만들어진
/// Agent를 못 쓰게 만들지 않으므로 확인시킬 대상이 없다.
pub async fn change_agent_template_status_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<crate::schema::AgentTemplateStatusRequest>,
) -> Result<Json<crate::schema::AgentTemplateSummary>, ApiError> {
    require_permission(&principal, PermissionKind::AgentTemplateLifecycle)?;
    verify_csrf_header(&jar, &headers)?;

    let template_id = parse_agent_template_id(&id)?;
    let template = load_agent_template(&state, template_id).await?;
    authorize_template_scope(&principal, template.project_id)?;

    let next = fleet_core::AgentTemplateStatus::parse_str(&body.status)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown status: {}", body.status)))?;
    if next == template.status {
        // idempotent — 같은 상태로의 전이는 표에 없지만 오류도 아니다.
        return Ok(Json(crate::schema::AgentTemplateSummary::from(&template)));
    }
    if !template.status.can_transition_to(next) {
        return Err(ApiError::Conflict(format!(
            "cannot transition from {} to {}",
            template.status.as_str(),
            next.as_str()
        )));
    }

    let changed = if next == fleet_core::AgentTemplateStatus::Retired {
        let expected = body.dependent_set_hash.as_deref().ok_or_else(|| {
            ApiError::BadRequest("dependent_set_hash is required to retire a template".into())
        })?;
        state
            .store
            .retire_agent_template(template_id, expected)
            .await
            .map_err(|e| {
                if let fleet_store::StoreError::Conflict(msg) = &e {
                    return ApiError::Conflict(msg.clone());
                }
                tracing::error!(error = %e, "retire_agent_template failed");
                ApiError::Store(e.to_string())
            })?
    } else {
        state
            .store
            .update_agent_template_status(template_id, next)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "update_agent_template_status failed");
                ApiError::Store(e.to_string())
            })?
    };
    if !changed {
        return Err(ApiError::NotFound("agent template not found".into()));
    }

    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::AGENT_TEMPLATE_STATUS_CHANGE,
        )
        .actor(principal.user.id)
        .target("agent_template", template_id.to_string())
        .detail(serde_json::json!({
            "from": template.status.as_str(),
            "to": next.as_str(),
            "dependent_set_hash": body.dependent_set_hash,
        })),
    )
    .await;

    let stored = load_agent_template(&state, template_id).await?;
    Ok(Json(crate::schema::AgentTemplateSummary::from(&stored)))
}

// ── Issue (로드맵 #92, Issue 표면) ──────────────────────────────────────
//
// `#92`는 AgentTemplate 표면도 함께 소유한다. 그쪽은 `#86`(AgentTemplate
// 엔티티)이 들어오면서 바로 위 절에 함께 났다 — 권한만 추가하고 검사받는
// 자리를 안 만들면 죽은 권한이 되기 때문이다. 두 도메인은 "관리 표면"이라는
// 주제만 공유할 뿐 서로 독립이다.
//
// 상태 전이는 `PATCH`(필드 수정)와 **분리된** endpoint다 — 계약이
// `issue:update`(오탈자 수정)와 `issue:close`(문제 종결)를 다른 capability로
// 나눈 것을 표면에서도 유지하기 위함이며, 저장소 계층에서 이미
// `update_issue_fields`/`transition_issue`로 갈라 둔 것과 같은 이유다.

/// 목표 상태별로 요구되는 capability.
///
/// 실제 규칙은 [`fleet_core::required_capability_for_transition`]이 소유한다 —
/// MCP 표면도 같은 함수를 쓴다(계약의 "두 표면 동일 동작" 요구). 여기서는
/// 이름만 다시 내보내, 이 모듈을 읽는 사람이 규칙의 위치를 찾을 수 있게 한다.
pub use fleet_core::required_capability_for_transition;

fn parse_issue_id(raw: &str) -> Result<fleet_core::IssueId, ApiError> {
    raw.parse::<fleet_core::IssueId>()
        .map_err(|_| ApiError::BadRequest("invalid issue id".into()))
}

fn parse_severity(raw: &str) -> Result<fleet_core::IssueSeverity, ApiError> {
    fleet_core::IssueSeverity::parse_str(raw)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown severity: {raw}")))
}

/// Issue를 조회하고 파생 `has_active_tasks`까지 채워 응답 형태로 만든다.
async fn issue_summary(
    state: &DashboardState,
    issue: &fleet_core::Issue,
) -> Result<crate::schema::IssueSummary, ApiError> {
    let has_active_tasks = state
        .store
        .issue_has_active_tasks(issue.id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;
    Ok(crate::schema::IssueSummary::from_issue(
        issue,
        has_active_tasks,
    ))
}

async fn load_issue(
    state: &DashboardState,
    id: fleet_core::IssueId,
) -> Result<fleet_core::Issue, ApiError> {
    state
        .store
        .get_issue(id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("issue {id}")))
}

/// `GET /api/issues` — Project 범위 Issue 목록.
#[derive(Debug, serde::Deserialize)]
pub struct ListIssuesQuery {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub open_only: Option<bool>,
}

pub async fn list_issues_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Query(query): Query<ListIssuesQuery>,
) -> Result<Json<Vec<crate::schema::IssueSummary>>, ApiError> {
    require_permission(&principal, PermissionKind::IssueRead)?;

    let project_id = match query.project_id.as_deref().filter(|s| !s.is_empty()) {
        Some(raw) => Some(parse_project_id(raw)?),
        None => None,
    };
    let status = match query.status.as_deref().filter(|s| !s.is_empty()) {
        Some(raw) => Some(
            fleet_core::IssueStatus::parse_str(raw)
                .ok_or_else(|| ApiError::BadRequest(format!("unknown issue status: {raw}")))?,
        ),
        None => None,
    };

    let issues = state
        .store
        .list_issues(&fleet_core::IssueFilter {
            project_id,
            status,
            open_only: query.open_only.unwrap_or(false),
            limit: 1000,
            offset: 0,
        })
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;

    let mut out = Vec::with_capacity(issues.len());
    for issue in &issues {
        out.push(issue_summary(&state, issue).await?);
    }
    Ok(Json(out))
}

/// `POST /api/issues` — Issue 생성. 항상 `Open`으로 시작한다.
pub async fn create_issue_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Json(body): Json<crate::schema::CreateIssueRequest>,
) -> Result<Json<crate::schema::IssueSummary>, ApiError> {
    require_permission(&principal, PermissionKind::IssueCreate)?;
    verify_csrf_header(&jar, &headers)?;

    let title = body.title.trim();
    if title.is_empty() {
        return Err(ApiError::BadRequest("title must not be empty".into()));
    }
    let project_id = parse_project_id(&body.project_id)?;

    // Issue는 항상 Project 경계 안에 있다 — 존재하지 않는 Project를 가리키는
    // Issue를 만들 수 없다. (Project 상태는 보지 않는다: 계약이 "`Draining`
    // 중에도 Issue 쓰기는 허용하고 claim과 Issue→Task 생성만 막는다"고
    // 명시했다.)
    if state
        .store
        .get_project(project_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .is_none()
    {
        return Err(ApiError::BadRequest(format!(
            "no such project: {project_id}"
        )));
    }

    let mut issue = fleet_core::Issue::new(project_id, title, &principal.user.username);
    if let Some(b) = body.body.as_deref() {
        issue.body = b.to_string();
    }
    if let Some(sev) = body.severity.as_deref().filter(|s| !s.is_empty()) {
        issue.severity = parse_severity(sev)?;
    }
    if let Some(labels) = body.labels {
        issue.labels = labels;
    }

    state
        .store
        .create_issue(&issue)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;

    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::ISSUE_CREATE,
        )
        .actor(principal.user.id)
        .target("issue", issue.id.to_string())
        .detail(serde_json::json!({ "project_id": project_id.to_string() })),
    )
    .await;

    Ok(Json(issue_summary(&state, &issue).await?))
}

/// `GET /api/issues/:id`.
pub async fn get_issue_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<crate::schema::IssueSummary>, ApiError> {
    require_permission(&principal, PermissionKind::IssueRead)?;
    let issue = load_issue(&state, parse_issue_id(&id)?).await?;
    Ok(Json(issue_summary(&state, &issue).await?))
}

/// `PATCH /api/issues/:id` — title·body·severity·labels·assignee 수정.
///
/// **상태는 바꾸지 못한다** — `POST .../transition`이 담당한다.
/// `assignee` 변경은 `issue:assign`을 추가로 요구한다(계약이 별도
/// capability로 분리했다).
pub async fn update_issue_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<crate::schema::UpdateIssueRequest>,
) -> Result<Json<crate::schema::IssueSummary>, ApiError> {
    require_permission(&principal, PermissionKind::IssueUpdate)?;
    verify_csrf_header(&jar, &headers)?;

    // assignee를 건드리는 요청만 추가 권한을 요구한다 — 나머지 필드 수정에
    // assign 권한을 함께 요구하면 계약의 capability 분리가 무의미해진다.
    if body.assignee.is_some() {
        require_permission(&principal, PermissionKind::IssueAssign)?;
    }

    let mut issue = load_issue(&state, parse_issue_id(&id)?).await?;

    if let Some(title) = body.title.as_deref() {
        let title = title.trim();
        if title.is_empty() {
            return Err(ApiError::BadRequest("title must not be empty".into()));
        }
        issue.title = title.to_string();
    }
    if let Some(b) = body.body {
        issue.body = b;
    }
    if let Some(sev) = body.severity.as_deref().filter(|s| !s.is_empty()) {
        issue.severity = parse_severity(sev)?;
    }
    if let Some(labels) = body.labels {
        issue.labels = labels;
    }
    if let Some(assignee) = body.assignee {
        issue.assignee = assignee.filter(|s| !s.trim().is_empty());
    }

    state
        .store
        .update_issue_fields(&issue)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;

    let issue = load_issue(&state, issue.id).await?;
    Ok(Json(issue_summary(&state, &issue).await?))
}

/// `POST /api/issues/:id/transition` — 상태 전이.
///
/// 요구 capability는 **목표 상태에 따라 다르다** —
/// [`required_capability_for_transition`] 참고.
pub async fn transition_issue_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<crate::schema::TransitionIssueRequest>,
) -> Result<Json<crate::schema::IssueSummary>, ApiError> {
    verify_csrf_header(&jar, &headers)?;

    let to = fleet_core::IssueStatus::parse_str(&body.status)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown issue status: {}", body.status)))?;
    require_permission(&principal, required_capability_for_transition(to))?;

    let close_reason = match body.close_reason.as_deref().filter(|s| !s.is_empty()) {
        Some(raw) => Some(
            fleet_core::CloseReason::parse_str(raw)
                .ok_or_else(|| ApiError::BadRequest(format!("unknown close_reason: {raw}")))?,
        ),
        None => None,
    };

    let mut issue = load_issue(&state, parse_issue_id(&id)?).await?;

    // 상태 기계 검증은 도메인 타입이 소유한다 — 허용되지 않는 간선과
    // close_reason 정합성 위반은 409(lifecycle 위반)로 매핑한다.
    issue
        .transition_to(to, close_reason)
        .map_err(|e| ApiError::Conflict(e.to_string()))?;

    state
        .store
        .transition_issue(issue.id, issue.status, issue.close_reason)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;

    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::ISSUE_TRANSITION,
        )
        .actor(principal.user.id)
        .target("issue", issue.id.to_string())
        .detail(serde_json::json!({
            "to": to.as_str(),
            "close_reason": close_reason.map(|r| r.as_str()),
        })),
    )
    .await;

    let issue = load_issue(&state, issue.id).await?;
    Ok(Json(issue_summary(&state, &issue).await?))
}

/// `GET /api/issues/:id/comments`.
pub async fn list_issue_comments_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<Vec<crate::schema::IssueCommentSummary>>, ApiError> {
    require_permission(&principal, PermissionKind::IssueRead)?;
    let issue_id = parse_issue_id(&id)?;
    let comments = state
        .store
        .list_issue_comments(issue_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;
    Ok(Json(
        comments
            .into_iter()
            .map(|c| crate::schema::IssueCommentSummary {
                id: c.id.to_string(),
                author: c.author,
                body: c.body,
                created_at: c.created_at,
            })
            .collect(),
    ))
}

/// `POST /api/issues/:id/comments`.
pub async fn add_issue_comment_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<crate::schema::AddIssueCommentRequest>,
) -> Result<Json<crate::schema::IssueCommentSummary>, ApiError> {
    require_permission(&principal, PermissionKind::IssueComment)?;
    verify_csrf_header(&jar, &headers)?;

    let text = body.body.trim();
    if text.is_empty() {
        return Err(ApiError::BadRequest(
            "comment body must not be empty".into(),
        ));
    }

    // 존재하지 않는 Issue에 코멘트를 남길 수 없다.
    let issue = load_issue(&state, parse_issue_id(&id)?).await?;

    let comment = fleet_core::IssueComment::new(issue.id, &principal.user.username, text);
    state
        .store
        .add_issue_comment(&comment)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;

    Ok(Json(crate::schema::IssueCommentSummary {
        id: comment.id.to_string(),
        author: comment.author,
        body: comment.body,
        created_at: comment.created_at,
    }))
}

/// `GET /api/issues/:id/links` — 연관된 Task 목록.
pub async fn list_issue_links_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<Vec<crate::schema::IssueTaskLinkSummary>>, ApiError> {
    require_permission(&principal, PermissionKind::IssueRead)?;
    let links = state
        .store
        .list_issue_task_links(parse_issue_id(&id)?)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;
    Ok(Json(
        links
            .into_iter()
            .map(|l| crate::schema::IssueTaskLinkSummary {
                task_id: l.task_id.map(|t| t.to_string()),
                task_label: l.task_label,
                linked_by: l.linked_by,
                linked_at: l.linked_at,
            })
            .collect(),
    ))
}

/// `POST /api/issues/:id/links` — Task 연관 추가 (멱등).
pub async fn link_issue_task_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<crate::schema::LinkIssueTaskRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_permission(&principal, PermissionKind::IssueLink)?;
    verify_csrf_header(&jar, &headers)?;

    let issue = load_issue(&state, parse_issue_id(&id)?).await?;
    let task_id: fleet_core::TaskId = body
        .task_id
        .parse()
        .map_err(|_| ApiError::BadRequest("invalid task id".into()))?;
    let task = state
        .store
        .get_task(task_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .ok_or_else(|| ApiError::BadRequest(format!("no such task: {task_id}")))?;

    // Project 경계 검사 (#58): `issue:link`는 호출자의 Project 범위를 검사하지
    // 않으므로, project_id가 다른 Task를 그대로 받아들이면 다른 Project의
    // Task 존재·label을 이 Issue를 통해 열람할 수 있는 confused-deputy가
    // 열린다. 판정 규칙과 그 배치 이유는
    // `fleet_store::project_rules::task_project_matches_issue_project` 문서 참고
    // (일반 풀 Task는 계속 허용 — 기존 동작·시험과 일치). 존재하지 않는 Task와
    // **동일한 오류**로 거절해 "다른 Project 소속"이라는 사실 자체를 노출하지
    // 않는다.
    if !fleet_store::task_project_matches_issue_project(task.project_id, issue.project_id) {
        return Err(ApiError::BadRequest(format!("no such task: {task_id}")));
    }

    // task_label은 Task가 삭제된 뒤에도 남는 표시 문자열이다 — prompt 앞부분을
    // 쓰되 길이를 제한한다.
    let label: String = task.prompt.chars().take(80).collect();

    let created = state
        .store
        .link_issue_task(&fleet_core::IssueTaskLink {
            issue_id: issue.id,
            task_id: Some(task_id),
            task_label: label,
            linked_by: principal.user.username.clone(),
            linked_at: Utc::now(),
        })
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "issue_id": issue.id.to_string(),
        "task_id": task_id.to_string(),
        "created": created,
    })))
}

/// `DELETE /api/issues/:id/links/:task_id` — Task 연관 해제.
pub async fn unlink_issue_task_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path((id, task_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_permission(&principal, PermissionKind::IssueLink)?;
    verify_csrf_header(&jar, &headers)?;

    let issue_id = parse_issue_id(&id)?;
    let task_id: fleet_core::TaskId = task_id
        .parse()
        .map_err(|_| ApiError::BadRequest("invalid task id".into()))?;

    let removed = state
        .store
        .unlink_issue_task(issue_id, task_id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?;
    Ok(Json(serde_json::json!({ "removed": removed })))
}

/// 더블 서밋 CSRF 검증 (헤더 variant) — JSON body를 받는 API 전용.
/// `logout`(form 없는 endpoint)과 동일한 패턴.
fn verify_csrf_header(jar: &CookieJar, headers: &axum::http::HeaderMap) -> Result<(), ApiError> {
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    let header_csrf = headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !csrf_valid(cookie_csrf.as_deref(), header_csrf) {
        return Err(ApiError::Forbidden("CSRF token invalid".into()));
    }
    Ok(())
}

/// GET /api/hosts/:hostname — 호스트 상세 JSON API.
pub async fn get_host_detail_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(hostname): Path<String>,
) -> Result<Json<HostDetail>, ApiError> {
    require_permission(&principal, PermissionKind::DashboardView)?;
    let host = state
        .store
        .get_host_by_hostname(&hostname)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_host_detail: failed");
            ApiError::Store(e.to_string())
        })?
        .ok_or_else(|| ApiError::NotFound(format!("host {hostname}")))?;

    let events = state
        .store
        .list_host_events(host.id, 50)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_host_detail: list_events failed");
            ApiError::Store(e.to_string())
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

/// GET /admin/activity — 활동 로그 HTML 페이지 (작업·워커 생명주기 이벤트).
///
/// 표시 데이터가 `/api/events`(`events:list`)이므로 페이지 게이트도 같은
/// 권한으로 맞춘다 — 이전에는 `audit:read`(admin 전용)로 잠겨 있었는데,
/// 정작 내용은 전 역할이 `/api/events`로 볼 수 있는 이벤트라 의미가 어긋났다.
///
/// 인증/권한 감사 로그는 이 페이지가 아니라 `/api/audit`가 담당한다
/// (전용 화면은 아직 없음).
pub async fn admin_activity_page(Extension(principal): Extension<AuthPrincipal>) -> Response {
    serve_page_if_permitted(
        &principal,
        PermissionKind::EventsList,
        "admin-activity.html",
    )
}

/// `GET /api/audit` 쿼리 파라미터.
#[derive(Debug, serde::Deserialize)]
pub struct ListAuditQuery {
    /// 액션명으로 필터 (예: `auth.login`).
    pub action: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

/// GET /api/audit — 인증/권한 감사 로그 JSON API.
///
/// `audit_log` 테이블의 인증/권한 이벤트를 반환한다. 작업·워커 생명주기
/// 이벤트는 별개로 `/api/events`가 담당한다.
pub async fn list_auth_audit_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Query(q): Query<ListAuditQuery>,
) -> Result<Json<Vec<fleet_core::AuditEvent>>, ApiError> {
    require_permission(&principal, PermissionKind::AuditRead)?;

    let filter = fleet_core::AuditFilter {
        actor_user_id: None,
        action: q.action,
        limit: q.limit,
        offset: q.offset,
    };

    let events = state.store.list_audit_events(&filter).await.map_err(|e| {
        tracing::error!(error = %e, "list_audit_events failed");
        ApiError::Store(e.to_string())
    })?;

    Ok(Json(events))
}

// ── P2: MCP Tools Explorer ───────────────────────────────────────────

/// GET /admin/tools — MCP 도구 탐색기 HTML 페이지.
///
/// `/api/tools`와 동일하게 `dashboard:view` 권한 필요.
pub async fn admin_tools_page(Extension(principal): Extension<AuthPrincipal>) -> Response {
    serve_page_if_permitted(
        &principal,
        PermissionKind::DashboardView,
        "admin-tools.html",
    )
}

/// GET /api/tools — MCP 도구 목록 JSON API.
pub async fn list_tools_api(
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_permission(&principal, PermissionKind::DashboardView)?;
    // 단일 출처: fleet-mcp의 실제 도구 카탈로그를 그대로 노출한다.
    // 하드코딩 목록을 두면 MCP에 도구가 추가/삭제될 때 조용히 어긋난다.
    let tools: Vec<serde_json::Value> = fleet_mcp::schema::all_tools()
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "tools": tools })))
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
    check_rate_limit, check_rate_limit_custom, extract_client_ip, record_login_failure,
    record_login_success, record_rate_limited_request, CSRF_COOKIE, EMAIL_SEND_WINDOW_SECS,
    MAX_EMAIL_SEND_ATTEMPTS, MAX_IP_EMAIL_SEND_ATTEMPTS, SESSION_COOKIE, SESSION_DURATION_SECS,
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
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(SESSION_DURATION_SECS)) // FLEET FIX (2026-08-12): match session lifetime, not a fixed 1h — the fixed value expired while the 8h session cookie was still valid, causing "CSRF token invalid" on task submission after ~1h
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
#[tracing::instrument(skip(state, jar, headers, form), fields(email = %form.email))]
#[allow(
    clippy::result_large_err,
    reason = "axum 핸들러·미들웨어의 반환 타입은 `IntoResponse` 바운드에 묶여 있고, \
axum-core에는 `impl IntoResponse for Box<T>` 제네릭 구현이 없어 Err를 Box로 감쌀 수 없다. \
게다가 이 Result는 요청당 최대 한 번 구성되어 곧바로 IntoResponse로 소비되므로, 이 lint가 \
겨냥하는 비용(큰 Err를 여러 스택 프레임에 걸쳐 이동시키는 것) 자체가 발생하지 않는다."
)]
pub async fn login(
    State(state): State<Arc<DashboardState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Form(form): Form<LoginForm>,
) -> Result<(CookieJar, Redirect), (CookieJar, Response)> {
    let ip = extract_client_ip(&headers, addr);

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
        // 감사: 차단까지 도달했다는 것은 브루트포스 정황이므로 반드시 남긴다.
        crate::audit::record(
            &state,
            fleet_core::AuditEvent::failure(&form.email, fleet_core::audit::action::AUTH_LOGIN)
                .ip(&ip)
                .detail(serde_json::json!({ "reason": "rate_limited" })),
        )
        .await;
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
        // 감사: 실패한 시도가 성공한 로그인보다 중요한 신호인 경우가 많다.
        // 계정 열거를 돕지 않도록 사용자 존재 여부는 detail에 남기지 않는다.
        crate::audit::record(
            &state,
            fleet_core::AuditEvent::failure(&form.email, fleet_core::audit::action::AUTH_LOGIN)
                .ip(&ip)
                .detail(serde_json::json!({ "reason": "invalid_credentials" })),
        )
        .await;
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
        crate::audit::record(
            &state,
            fleet_core::AuditEvent::failure(&user.username, fleet_core::audit::action::AUTH_LOGIN)
                .actor(user.id)
                .ip(&ip)
                .detail(serde_json::json!({ "reason": "email_not_verified" })),
        )
        .await;
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

    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(&user.username, fleet_core::audit::action::AUTH_LOGIN)
            .actor(user.id)
            .ip(&ip),
    )
    .await;

    // 쿠키 설정.
    let cookie = Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .secure(state.secure_cookies)
        .same_site(SameSite::Lax)
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
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(SESSION_DURATION_SECS)) // FLEET FIX (2026-08-12): match session lifetime, not a fixed 1h — the fixed value expired while the 8h session cookie was still valid, causing "CSRF token invalid" on task submission after ~1h
        .build();
    let new_jar = new_jar.add(csrf_cookie);

    Ok((new_jar, Redirect::to(&state.abs("/"))))
}

/// POST /logout — 세션 삭제 + 쿠키 제거.
///
/// CSRF 보호: JS에서 `X-CSRF-Token` 헤더로 CSRF 토큰을 전송해야 함.
/// (세션 쿠키는 SameSite::Lax이지만 defense-in-depth로 이중 검증.)
#[allow(
    clippy::result_large_err,
    reason = "axum 핸들러·미들웨어의 반환 타입은 `IntoResponse` 바운드에 묶여 있고, \
axum-core에는 `impl IntoResponse for Box<T>` 제네릭 구현이 없어 Err를 Box로 감쌀 수 없다. \
게다가 이 Result는 요청당 최대 한 번 구성되어 곧바로 IntoResponse로 소비되므로, 이 lint가 \
겨냥하는 비용(큰 Err를 여러 스택 프레임에 걸쳐 이동시키는 것) 자체가 발생하지 않는다."
)]
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
    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            &principal.user.username,
            fleet_core::audit::action::AUTH_LOGOUT,
        )
        .actor(principal.user.id),
    )
    .await;
    let removed = Cookie::from(SESSION_COOKIE);
    let new_jar = jar.remove(removed);
    Ok((new_jar, Redirect::to(&state.abs("/login"))))
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
    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            verification.user_id.as_uuid().to_string(),
            fleet_core::audit::action::AUTH_EMAIL_VERIFIED,
        )
        .actor(verification.user_id)
        .target("user", verification.user_id.as_uuid().to_string()),
    )
    .await;
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
) -> Result<StatusCode, ApiError> {
    let user = state
        .store
        .get_user_by_email(&req.email)
        .await
        .map_err(|_| ApiError::Internal("DB error".into()))?
        .ok_or(ApiError::NotFound("User not found".into()))?;

    if user.email_verified {
        return Err(ApiError::Conflict("Email already verified".into()));
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
        .map_err(|_| ApiError::Internal("DB error".into()))?;

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
<link rel="stylesheet" href="static/login.css"></head>
<body class="auth-page">
  <div class="auth-card">
    <h1 style="color:{color};margin:0 0 12px;">{title}</h1>
    <p style="font-size:15px;line-height:1.6;">{message}</p>
    <a href="login" class="auth-button" style="display:inline-block;text-decoration:none;margin-top:16px;">Go to Sign In</a>
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
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(SESSION_DURATION_SECS)) // FLEET FIX (2026-08-12): match session lifetime, not a fixed 1h — the fixed value expired while the 8h session cookie was still valid, causing "CSRF token invalid" on task submission after ~1h
        .build();
    let jar = jar.add(csrf_cookie);

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>Resend Verification — Fleet</title>
<link rel="stylesheet" href="static/login.css"></head>
<body class="auth-page">
  <div class="auth-card">
    <div class="auth-logo">F</div>
    <h1>Resend Verification</h1>
    <p class="auth-subtitle">Enter your email to receive a new verification link</p>
    <form method="POST" action="resend-verification">
      <input type="hidden" name="csrf_token" value="{csrf_token}" />
      <label>
        <span>Email</span>
        <input type="email" name="email" required autofocus autocomplete="email" />
      </label>
      <button type="submit" class="auth-button">Send Verification Link</button>
    </form>
    <a href="login" style="display:inline-block;margin-top:12px;color:#5b7fef;text-decoration:none;">← Back to Sign In</a>
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
    headers: axum::http::HeaderMap,
    Form(form): Form<ResendVerificationForm>,
) -> Response {
    let ip = extract_client_ip(&headers, addr);

    // CSRF 검증.
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return info_page("error", "Security token expired. Please reload the page.");
    }

    // rate limit — 이메일 폭탄 방지. (email, IP) 양쪽 카운터.
    //
    // 식별자에 엔드포인트 네임스페이스를 붙인다. 순수 email을 쓰면 이 엔드포인트
    // 스팸이 `/login`의 동일 email 카운터를 소모해 피해자를 로그인 불가로
    // 만들 수 있다(교차 엔드포인트 락아웃).
    let rl_identifier = format!("resend:{}", form.email);
    let allowed = check_rate_limit_custom(
        &state,
        &rl_identifier,
        Some(&ip),
        MAX_EMAIL_SEND_ATTEMPTS,
        MAX_IP_EMAIL_SEND_ATTEMPTS,
        EMAIL_SEND_WINDOW_SECS,
    )
    .await
    .unwrap_or(false);
    if !allowed {
        // 차단된 요청은 기록하지 않는다 — 기록하면 공격자가 요청을 계속
        // 퍼부어 잠금을 무한 연장할 수 있다(락아웃 증폭).
        return info_page("error", "Too many requests. Please try again later.");
    }

    // 카운터 증가 — 응답이 항상 동일(계정 열거 방지)하므로 실패 경로가 아니라
    // 통과한 모든 요청을 1건으로 센다. 이 호출이 없으면 카운터가 0에 머물러
    // 위 차단 분기에 영원히 도달하지 못한다.
    record_rate_limited_request(&state, &rl_identifier, Some(&ip), "resend_verification")
        .await
        .ok();

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
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(SESSION_DURATION_SECS)) // FLEET FIX (2026-08-12): match session lifetime, not a fixed 1h — the fixed value expired while the 8h session cookie was still valid, causing "CSRF token invalid" on task submission after ~1h
        .build();
    let jar = jar.add(csrf_cookie);

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>Forgot Password — Fleet</title>
<link rel="stylesheet" href="static/login.css"></head>
<body class="auth-page">
  <div class="auth-card">
    <div class="auth-logo">F</div>
    <h1>Reset Password</h1>
    <p class="auth-subtitle">Enter your email to receive a reset link</p>
    <form method="POST" action="forgot-password">
      <input type="hidden" name="csrf_token" value="{csrf_token}" />
      <label>
        <span>Email</span>
        <input type="email" name="email" required autofocus autocomplete="email" />
      </label>
      <button type="submit" class="auth-button">Send Reset Link</button>
    </form>
    <a href="login" style="display:inline-block;margin-top:12px;color:#5b7fef;text-decoration:none;">← Back to Sign In</a>
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
    headers: axum::http::HeaderMap,
    Form(form): Form<ForgotPasswordForm>,
) -> Response {
    let ip = extract_client_ip(&headers, addr);

    // CSRF 검증.
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return info_page("error", "Security token expired. Please reload the page.");
    }

    // rate limit — 이메일 폭탄 방지. (email, IP) 양쪽 카운터.
    //
    // 식별자에 엔드포인트 네임스페이스를 붙인다 — 순수 email을 쓰면 이 엔드포인트
    // 스팸이 `/login`의 동일 email 카운터를 소모해 피해자를 로그인 불가로
    // 만들 수 있다(교차 엔드포인트 락아웃).
    let rl_identifier = format!("forgot:{}", form.email);
    let allowed = check_rate_limit_custom(
        &state,
        &rl_identifier,
        Some(&ip),
        MAX_EMAIL_SEND_ATTEMPTS,
        MAX_IP_EMAIL_SEND_ATTEMPTS,
        EMAIL_SEND_WINDOW_SECS,
    )
    .await
    .unwrap_or(false);
    if !allowed {
        // 차단된 요청은 기록하지 않는다 (락아웃 증폭 방지).
        return info_page("error", "Too many requests. Please try again later.");
    }

    // 카운터 증가 — 응답이 항상 동일(계정 열거 방지)하므로 통과한 모든 요청을
    // 1건으로 센다. 유효한 이메일에 대한 반복 요청이 곧 이메일 폭탄이므로
    // "실패 경로에서만 기록"하면 방어가 성립하지 않는다.
    record_rate_limited_request(&state, &rl_identifier, Some(&ip), "forgot_password")
        .await
        .ok();

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
<link rel="stylesheet" href="static/login.css"></head>
<body class="auth-page">
  <div class="auth-card">
    <div class="auth-logo">F</div>
    <h1>Set New Password</h1>
    <p class="auth-subtitle">Choose a strong password (min 8 characters)</p>
    <form method="POST" action="reset-password">
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
    <a href="login" style="display:inline-block;margin-top:12px;color:#5b7fef;text-decoration:none;">← Back to Sign In</a>
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
    headers: axum::http::HeaderMap,
    Form(form): Form<ResetPasswordForm>,
) -> Response {
    let ip = extract_client_ip(&headers, addr);

    // CSRF 검증.
    let cookie_csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_string());
    if !csrf_valid(cookie_csrf.as_deref(), &form.csrf_token) {
        return info_page("error", "Security token expired. Please reload the page.");
    }

    // rate limit — 토큰 열거 공격 방지.
    //
    // 식별자는 반드시 **안정적인 값**이어야 한다. 이전 구현은 `form.token`을
    // 썼는데, 토큰은 시도마다 달라지므로 identifier별 카운터가 구조적으로
    // 항상 0이었다(= 차단 불가). 이 엔드포인트의 폼에는 이메일이 없으므로
    // 요청 출처 IP를 식별자로 사용한다.
    let rl_identifier = format!("reset:{ip}");
    let allowed = check_rate_limit(&state, &rl_identifier, Some(&ip))
        .await
        .unwrap_or(false);
    if !allowed {
        // 차단된 요청은 기록하지 않는다 (락아웃 증폭 방지).
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
        Ok(None) => {
            // 실제 실패 지점에서 카운터 증가 — 토큰 브루트포스가 여기로 모인다.
            record_login_failure(&state, &rl_identifier, Some(&ip), "reset_token_invalid")
                .await
                .ok();
            return info_page("error", "Invalid or unknown reset token.");
        }
        Err(e) => {
            tracing::error!(error = %e, "get_password_reset_token failed");
            return info_page("error", "Server error. Please try again later.");
        }
    };

    if reset_token.is_consumed() {
        record_login_failure(&state, &rl_identifier, Some(&ip), "reset_token_consumed")
            .await
            .ok();
        return info_page("error", "This reset link has already been used.");
    }
    if reset_token.is_expired() {
        record_login_failure(&state, &rl_identifier, Some(&ip), "reset_token_expired")
            .await
            .ok();
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

    // 정상 재설정 성공 — 해당 출처의 실패 카운터 초기화 (/login과 동일 패턴).
    record_login_success(&state, &rl_identifier, Some(&ip))
        .await
        .ok();

    // 감사: 비밀번호 변경은 계정 탈취의 핵심 지표다. 토큰 원문은 남기지 않는다.
    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(
            reset_token.user_id.as_uuid().to_string(),
            fleet_core::audit::action::AUTH_PASSWORD_RESET,
        )
        .actor(reset_token.user_id)
        .target("user", reset_token.user_id.as_uuid().to_string())
        .ip(&ip),
    )
    .await;

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
<link rel="stylesheet" href="static/login.css"></head>
<body class="auth-page">
  <div class="auth-card">
    <h1 style="color:{color};margin:0 0 12px;">{title}</h1>
    <p style="font-size:15px;line-height:1.6;">{message}</p>
    <a href="login" class="auth-button" style="display:inline-block;text-decoration:none;margin-top:16px;">Go to Sign In</a>
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
            let matched = csrf_tokens_match(cookie, submitted_token);
            if !matched {
                tracing::warn!(
                    cookie_len = cookie.len(),
                    submitted_len = submitted_token.len(),
                    "CSRF token mismatch — cookies and form fields do not match"
                );
            }
            matched
        }
        None => {
            tracing::warn!(
                "CSRF token validation failed — fleet_csrf cookie is missing from request"
            );
            false
        }
        Some(_) => {
            tracing::warn!("CSRF token validation failed — empty cookie or empty form token");
            false
        }
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
  <link rel="stylesheet" href="static/login.css" />
</head>
<body class="auth-page">
  <div class="auth-card">
    <div class="auth-logo">F</div>
    <h1>Sign in to Fleet</h1>
    <p class="auth-subtitle">Use your administrator account</p>
    <div class="auth-error">{msg}</div>
    <form method="POST" action="login" autocomplete="on">
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
        return Err(Ok(Redirect::to(&state.abs("/login"))));
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
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(SESSION_DURATION_SECS)) // FLEET FIX (2026-08-12): match session lifetime, not a fixed 1h — the fixed value expired while the 8h session cookie was still valid, causing "CSRF token invalid" on task submission after ~1h
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
#[tracing::instrument(skip(state, jar, headers, form), fields(email = %form.email))]
#[allow(
    clippy::result_large_err,
    reason = "axum 핸들러·미들웨어의 반환 타입은 `IntoResponse` 바운드에 묶여 있고, \
axum-core에는 `impl IntoResponse for Box<T>` 제네릭 구현이 없어 Err를 Box로 감쌀 수 없다. \
게다가 이 Result는 요청당 최대 한 번 구성되어 곧바로 IntoResponse로 소비되므로, 이 lint가 \
겨냥하는 비용(큰 Err를 여러 스택 프레임에 걸쳐 이동시키는 것) 자체가 발생하지 않는다."
)]
pub async fn bootstrap(
    State(state): State<Arc<DashboardState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
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

    let ip = extract_client_ip(&headers, addr);

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

    // DB에는 원문이 없으므로 입력 원문의 digest로 활성 토큰을 찾는다. 실제 소비는
    // 원문을 Store의 atomic consume 경로에 전달해 사용자 생성과 함께 처리한다.
    let tokens = state.store.list_bootstrap_tokens().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            jar.clone(),
            internal_error_page(),
        )
    })?;

    let token_digest = fleet_core::BootstrapToken::digest_for(token_input);
    let matching_token = tokens
        .iter()
        .any(|t| t.is_usable() && t.token_digest == token_digest);

    if !matching_token {
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
    }

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
        consume_bootstrap_and_create_admin(&*state.store, token_input, user, password_hash)
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
    // 감사: 관리자 부트스트랩은 가장 강한 권한이 생성되는 지점이다.
    crate::audit::record(
        &state,
        fleet_core::AuditEvent::success(&form.email, fleet_core::audit::action::AUTH_BOOTSTRAP)
            .ip(&ip)
            .detail(serde_json::json!({ "email": form.email })),
    )
    .await;

    // 쿠키 설정.
    let cookie = Cookie::build((SESSION_COOKIE, session_token))
        .path("/")
        .http_only(true)
        .secure(state.secure_cookies)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(SESSION_DURATION_SECS))
        .build();
    let new_jar = jar.add(cookie);

    Ok((new_jar, Redirect::to(&state.abs("/"))))
}

fn bootstrap_failed_page(msg: &str) -> Response {
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Fleet Orchestrator — Setup</title>
  <link rel="stylesheet" href="static/login.css" />
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
        <a href="bootstrap" style="color: var(--primary); font-weight: 500;">Try again</a>
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

    // ── 페이지 수준 인가 ──────────────────────────────────────────────

    /// 지정한 내장 역할의 권한을 가진 principal 생성.
    fn principal_for(role: fleet_core::BuiltinRole) -> AuthPrincipal {
        let user = fleet_core::User {
            id: fleet_core::UserId::new(),
            username: "tester".into(),
            email: Some("tester@example.com".into()),
            email_verified: true,
            password_hash: String::new(),
            enabled: true,
            created_at: chrono::Utc::now(),
            last_login_at: None,
        };
        AuthPrincipal {
            user,
            permissions: role.permissions(),
            session_id: fleet_core::SessionId::new(),
        }
    }

    #[test]
    fn viewer_is_denied_admin_pages() {
        let viewer = principal_for(fleet_core::BuiltinRole::Viewer);
        // Viewer는 user:read / host:provision 이 없다.
        // (활동 로그는 events:list라 Viewer도 접근 가능 — 아래 별도 테스트 참조.)
        for (perm, page) in [
            (PermissionKind::UserRead, "admin-users.html"),
            (PermissionKind::HostProvision, "admin-ssh-keys.html"),
        ] {
            let resp = serve_page_if_permitted(&viewer, perm, page);
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "viewer should be denied {page}"
            );
        }
    }

    #[test]
    fn operator_is_denied_user_pages() {
        let operator = principal_for(fleet_core::BuiltinRole::Operator);
        let resp = serve_page_if_permitted(&operator, PermissionKind::UserRead, "admin-users.html");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "operator should be denied the user management page"
        );
    }

    /// 활동 로그는 `/api/events`(events:list)를 보여주므로 전 역할이 접근 가능해야 한다.
    ///
    /// 이전에는 `audit:read`(admin 전용)로 잠겨 있어, Viewer가 페이지를 열면
    /// 데이터 fetch가 403이 나 빈 표만 보였다. 게이트를 데이터 권한에 맞춘 뒤의
    /// 동작을 고정한다.
    #[test]
    fn all_roles_can_view_activity_page() {
        for role in [
            fleet_core::BuiltinRole::Viewer,
            fleet_core::BuiltinRole::Operator,
            fleet_core::BuiltinRole::Admin,
        ] {
            let principal = principal_for(role);
            let resp = serve_page_if_permitted(
                &principal,
                PermissionKind::EventsList,
                "admin-activity.html",
            );
            assert_ne!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "{role:?} should be allowed the activity page"
            );
        }
    }

    #[test]
    fn admin_is_allowed_admin_pages() {
        let admin = principal_for(fleet_core::BuiltinRole::Admin);
        for (perm, page) in [
            (PermissionKind::UserRead, "admin-users.html"),
            (PermissionKind::EventsList, "admin-activity.html"),
            (PermissionKind::HostProvision, "admin-ssh-keys.html"),
        ] {
            let resp = serve_page_if_permitted(&admin, perm, page);
            assert_ne!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "admin should be allowed {page}"
            );
        }
    }

    #[test]
    fn viewer_lacks_task_output_permission() {
        // sse.rs의 redaction 전제 — Viewer는 task:output이 없다.
        // 이 전제가 깨지면 SSE redaction 분기가 의미를 잃으므로 여기서 고정한다.
        let viewer = principal_for(fleet_core::BuiltinRole::Viewer);
        assert!(viewer.has(PermissionKind::EventsList));
        assert!(!viewer.has(PermissionKind::TaskOutput));
    }
}

#[cfg(test)]
mod asset_embed_tests {
    /// 페이지 핸들러가 참조하는 자산이 실제로 임베드되어 있어야 한다.
    ///
    /// 자산 파일명을 바꾸면서 핸들러 문자열을 안 고치면(또는 그 반대) 컴파일은
    /// 통과하고 런타임에만 "page not built" 404가 난다. 이름을 바꾼 적이 있어
    /// (`admin-audit.html` → `admin-activity.html`) 그 조합을 고정해 둔다.
    #[test]
    fn page_assets_referenced_by_handlers_exist() {
        for name in [
            "index.html",
            "tasks.html",
            "admin-activity.html",
            "admin-tools.html",
            "admin-users.html",
            "hosts.html",
        ] {
            assert!(
                crate::assets::Asset::get(name).is_some(),
                "임베드된 자산에 {name}이 없다"
            );
        }
    }

    /// 이름을 바꾼 옛 자산이 남아 있으면 안 된다 (중복 방치 방지).
    #[test]
    fn renamed_asset_is_gone() {
        assert!(crate::assets::Asset::get("admin-audit.html").is_none());
    }
}
