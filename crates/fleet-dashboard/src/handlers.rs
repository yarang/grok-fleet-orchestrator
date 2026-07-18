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

    let workers = state
        .store
        .list_workers(&filter)
        .await
        .map_err(|e| {
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
    let tasks = state
        .store
        .list_tasks(&filter)
        .await
        .map_err(|e| {
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
pub async fn static_asset(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    let cleaned = path.trim_start_matches('/');
    let full = if cleaned.is_empty() {
        "index.html"
    } else {
        cleaned
    };
    match crate::assets::Asset::get(full) {
        Some(file) => {
            let mime = file.metadata.mimetype();
            (
                [(axum::http::header::CONTENT_TYPE, mime)],
                file.data,
            )
                .into_response()
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
