//! MCP 도구 호출 핸들러.
//!
//! `tools/call` 요청의 `name`/`arguments`를 받아 해당 도구를 실행합니다.
//! 핸들러는 `Arc<FleetState>`(Store 접근)와 `Arc<Dispatcher>`(작업 제출)를 참조.
//!
//! ## 결과 형태
//!
//! 모든 핸들러는 `Result<Value, JsonRpcError>`를 반환합니다.
//! - `Ok(value)` — MCP 도구 결과 객체 (`{content, isError}` 형태).
//!   도구의 논리적 실패(예: 작업을 찾지 못함)도 `Ok(tool_error(...))`로 반환되며,
//!   이때 `isError: true` 플래그가 설정됩니다.
//! - `Err(json_rpc_error)` — JSON-RPC 레벨 에러 (잘못된 인자 등).

use std::sync::Arc;

use serde_json::{json, Value};

use fleet_core::{Task, TaskId, TaskRequest, WorkerFilter, WorkerStatus};
use fleet_scheduler::{Dispatcher, FleetState};
use tracing::debug;

use crate::schema::{
    self, JsonRpcError, TOOL_CANCEL_TASK, TOOL_DISPATCH_TASK, TOOL_GET_TASK_STATUS,
    TOOL_LIST_WORKERS, TOOL_WAIT_FOR_TASK,
};

/// 도구 호출 컨텍스트. 핸들러가 필요로 하는 모든 의존성을 캡슐화.
#[derive(Clone)]
pub struct ToolContext {
    pub state: Arc<FleetState>,
    pub dispatcher: Arc<Dispatcher>,
}

impl ToolContext {
    pub fn new(state: Arc<FleetState>, dispatcher: Arc<Dispatcher>) -> Self {
        Self { state, dispatcher }
    }
}

/// 도구 호출을 실행.
///
/// `name`은 `tools/list`에 정의된 도구 이름이어야 합니다.
/// `arguments`는 JSON 객체 (또는 null).
pub async fn dispatch_tool(
    ctx: &ToolContext,
    name: &str,
    arguments: &Value,
) -> Result<Value, JsonRpcError> {
    debug!(tool = name, "dispatching tool call");
    match name {
        TOOL_DISPATCH_TASK => handle_dispatch_task(ctx, arguments).await,
        TOOL_GET_TASK_STATUS => handle_get_task_status(ctx, arguments).await,
        TOOL_LIST_WORKERS => handle_list_workers(ctx, arguments).await,
        TOOL_CANCEL_TASK => handle_cancel_task(ctx, arguments).await,
        TOOL_WAIT_FOR_TASK => handle_wait_for_task(ctx, arguments).await,
        other => Err(JsonRpcError::method_not_found(other)),
    }
}

// ── fleet_dispatch_task ─────────────────────────────────────────────────

async fn handle_dispatch_task(
    ctx: &ToolContext,
    args: &Value,
) -> Result<Value, JsonRpcError> {
    let args = args.as_object().ok_or_else(|| {
        JsonRpcError::invalid_params("arguments must be a JSON object")
    })?;

    let prompt = args
        .get("prompt")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: prompt"))?
        .to_string();

    if prompt.trim().is_empty() {
        return Err(JsonRpcError::invalid_params("prompt must not be empty"));
    }

    let mut req = TaskRequest {
        prompt,
        ..Default::default()
    };
    req.cwd = args.get("cwd").and_then(|v| v.as_str()).map(String::from);
    req.model = args.get("model").and_then(|v| v.as_str()).map(String::from);
    req.server_hint = args
        .get("server_hint")
        .and_then(|v| v.as_str())
        .map(String::from);
    req.required_labels = args
        .get("required_labels")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    req.max_turns = args.get("max_turns").and_then(|v| v.as_u64()).map(|n| n as u32);
    req.timeout_secs = args.get("timeout_secs").and_then(|v| v.as_u64());
    req.created_by = "mcp".to_string();

    let task = Task::from_request(req);
    let task_id = task.id;

    match ctx.dispatcher.submit(task).await {
        Ok(returned_id) => {
            debug!(%returned_id, "dispatch_task succeeded");
            Ok(schema::tool_json(&json!({
                "task_id": returned_id.to_string(),
                "status": "dispatched",
                "hint": "Poll fleet_get_task_status with the task_id to observe completion."
            })))
        }
        Err(e) => {
            // 디스패치 실패 — 도구 호출 자체는 성공했지만 결과가 에러.
            // isError 플래그를 설정하여 클라이언트에게 알림.
            Ok(schema::tool_error(format!(
                "dispatch failed: {e} (task_id={task_id})"
            )))
        }
    }
}

// ── fleet_get_task_status ───────────────────────────────────────────────

async fn handle_get_task_status(
    ctx: &ToolContext,
    args: &Value,
) -> Result<Value, JsonRpcError> {
    let args = args.as_object().ok_or_else(|| {
        JsonRpcError::invalid_params("arguments must be a JSON object")
    })?;

    let id_str = args
        .get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: task_id"))?;

    let task_id: TaskId = id_str
        .parse()
        .map_err(|e| JsonRpcError::invalid_params(format!("invalid task_id: {e}")))?;

    let task = ctx
        .state
        .store
        .get_task(task_id)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    let Some(task) = task else {
        return Ok(schema::tool_error(format!(
            "task not found: {task_id}"
        )));
    };

    Ok(schema::tool_json(&task_summary(&task)))
}

// ── fleet_cancel_task ───────────────────────────────────────────────────

async fn handle_cancel_task(
    ctx: &ToolContext,
    args: &Value,
) -> Result<Value, JsonRpcError> {
    let args = args.as_object().ok_or_else(|| {
        JsonRpcError::invalid_params("arguments must be a JSON object")
    })?;

    let id_str = args
        .get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: task_id"))?;

    let task_id: TaskId = id_str
        .parse()
        .map_err(|e| JsonRpcError::invalid_params(format!("invalid task_id: {e}")))?;

    let reason = args
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("cancelled by user")
        .to_string();

    match ctx.dispatcher.cancel(task_id, reason).await {
        Ok(()) => Ok(schema::tool_json(&json!({
            "task_id": task_id.to_string(),
            "status": "cancelled",
            "hint": "Cancellation has been recorded; the worker has been notified (best-effort)."
        }))),
        Err(e) => Ok(schema::tool_error(format!("cancel failed: {e}"))),
    }
}

// ── fleet_wait_for_task ─────────────────────────────────────────────────

async fn handle_wait_for_task(
    ctx: &ToolContext,
    args: &Value,
) -> Result<Value, JsonRpcError> {
    let args = args.as_object().ok_or_else(|| {
        JsonRpcError::invalid_params("arguments must be a JSON object")
    })?;

    let id_str = args
        .get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: task_id"))?;

    let task_id: TaskId = id_str
        .parse()
        .map_err(|e| JsonRpcError::invalid_params(format!("invalid task_id: {e}")))?;

    let timeout_secs = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(300);
    // 스키마가 최대 3600을 선언하지만 서버 측에서도 clamp.
    let timeout_secs = timeout_secs.clamp(1, 3600);
    let timeout = std::time::Duration::from_secs(timeout_secs);

    match ctx.dispatcher.wait_for_task(task_id, timeout).await {
        Ok(task) => Ok(schema::tool_json(&task_summary(&task))),
        Err(e) => Ok(schema::tool_error(format!("wait failed: {e}"))),
    }
}

/// 클라이언트에게 반환할 작업 요약. 전체 `Task`에서 핵심 필드만 발췌.
fn task_summary(task: &Task) -> Value {
    let phase = phase_str(&task.status);
    let mut summary = json!({
        "task_id": task.id.to_string(),
        "phase": phase,
        "prompt": task.prompt,
        "created_at": task.created_at.to_rfc3339(),
        "created_by": task.created_by,
    });

    if let Some(hint) = &task.server_hint {
        summary["server_hint"] = json!(hint);
    }
    if !task.required_labels.is_empty() {
        summary["required_labels"] = json!(task.required_labels);
    }

    match &task.status {
        fleet_core::TaskStatus::Dispatched { worker_id, started_at } => {
            summary["worker_id"] = json!(worker_id.to_string());
            summary["started_at"] = json!(started_at.to_rfc3339());
        }
        fleet_core::TaskStatus::Completed(result) => {
            summary["worker_id"] = json!(result.worker_id.to_string());
            summary["output"] = json!(result.output);
            summary["exit_code"] = json!(result.exit_code);
            summary["duration_secs"] = json!(result.duration_secs);
            summary["finished_at"] = json!(result.finished_at.to_rfc3339());
        }
        fleet_core::TaskStatus::Failed(failure) => {
            summary["error"] = json!(failure.error);
            summary["failure_kind"] = json!(format!("{:?}", failure.kind));
            if let Some(wid) = failure.worker_id {
                summary["worker_id"] = json!(wid.to_string());
            }
        }
        fleet_core::TaskStatus::Cancelled { reason, cancelled_at } => {
            summary["reason"] = json!(reason);
            summary["cancelled_at"] = json!(cancelled_at.to_rfc3339());
        }
        fleet_core::TaskStatus::Pending => {}
    }

    summary
}

/// `TaskStatus`에서 위상(phase) 문자열 추출 (클라이언트 친화적).
fn phase_str(status: &fleet_core::TaskStatus) -> &'static str {
    use fleet_core::TaskStatus::*;
    match status {
        Pending => "pending",
        Dispatched { .. } => "dispatched",
        Completed(_) => "completed",
        Failed(_) => "failed",
        Cancelled { .. } => "cancelled",
    }
}

// ── fleet_list_workers ──────────────────────────────────────────────────

async fn handle_list_workers(
    ctx: &ToolContext,
    args: &Value,
) -> Result<Value, JsonRpcError> {
    let mut filter = WorkerFilter::default();

    if let Some(obj) = args.as_object() {
        if let Some(status_str) = obj.get("status").and_then(|v| v.as_str()) {
            filter.status = Some(parse_worker_status(status_str)?);
        }
        if let Some(labels) = obj.get("labels").and_then(|v| v.as_object()) {
            for (k, v) in labels {
                let val = v.as_str().ok_or_else(|| {
                    JsonRpcError::invalid_params(format!(
                        "label '{k}' value must be a string"
                    ))
                })?;
                filter.labels.insert(k.clone(), val.to_string());
            }
        }
        if let Some(limit) = obj.get("limit").and_then(|v| v.as_u64()) {
            filter.limit = limit as usize;
        }
    }

    let workers = ctx
        .state
        .store
        .list_workers(&filter)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    let summary: Vec<Value> = workers.iter().map(worker_summary).collect();

    Ok(schema::tool_json(&json!({
        "workers": summary,
        "count": summary.len(),
    })))
}

/// `WorkerStatus` 문자열 → enum. snake_case 매칭.
fn parse_worker_status(s: &str) -> Result<WorkerStatus, JsonRpcError> {
    match s {
        "online" => Ok(WorkerStatus::Online),
        "degraded" => Ok(WorkerStatus::Degraded),
        "offline" => Ok(WorkerStatus::Offline),
        "circuit_open" => Ok(WorkerStatus::CircuitOpen),
        other => Err(JsonRpcError::invalid_params(format!(
            "invalid status '{other}': expected one of online, degraded, offline, circuit_open"
        ))),
    }
}

/// 클라이언트에게 반환할 워커 요약.
fn worker_summary(w: &fleet_core::Worker) -> Value {
    json!({
        "id": w.id.to_string(),
        "name": w.name,
        "endpoint": w.endpoint,
        "status": format!("{:?}", w.status).to_lowercase(),
        "labels": w.labels,
        "active_tasks": w.active_tasks,
        "max_concurrent": w.max_concurrent,
        "circuit_state": format!("{:?}", w.circuit_state).to_lowercase(),
        "last_seen": w.last_seen.map(|t| t.to_rfc3339()),
        "registered_at": w.registered_at.to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_string_pending() {
        assert_eq!(phase_str(&fleet_core::TaskStatus::Pending), "pending");
    }

    #[test]
    fn phase_string_failed_and_cancelled() {
        use fleet_core::{TaskFailure, FailureKind};
        let failed = fleet_core::TaskStatus::Failed(TaskFailure {
            error: "boom".into(),
            kind: FailureKind::WorkerError,
            worker_id: None,
            attempts: 1,
        });
        assert_eq!(phase_str(&failed), "failed");

        let cancelled = fleet_core::TaskStatus::Cancelled {
            reason: "user".into(),
            cancelled_at: chrono::Utc::now(),
        };
        assert_eq!(phase_str(&cancelled), "cancelled");
    }

    #[test]
    fn parse_status_accepts_known_values() {
        assert!(parse_worker_status("online").is_ok());
        assert!(parse_worker_status("circuit_open").is_ok());
        assert!(parse_worker_status("bogus").is_err());
    }
}
