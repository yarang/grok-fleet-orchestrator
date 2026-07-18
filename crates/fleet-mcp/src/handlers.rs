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
    self, JsonRpcError, TOOL_CANCEL_TASK, TOOL_COLLECT_RESULTS, TOOL_DISPATCH_TASK,
    TOOL_GET_TASK_STATUS, TOOL_LIST_WORKERS, TOOL_STREAM_TASK_OUTPUT, TOOL_WAIT_FOR_TASK,
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
        TOOL_STREAM_TASK_OUTPUT => handle_stream_task_output(ctx, arguments).await,
        TOOL_COLLECT_RESULTS => handle_collect_results(ctx, arguments).await,
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

// ── fleet_stream_task_output ────────────────────────────────────────────

async fn handle_stream_task_output(
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

    let mut offset = args
        .get("from_offset")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let poll_interval_secs = args
        .get("poll_interval_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(1)
        .clamp(1, 30);
    let max_polls = args
        .get("max_polls")
        .and_then(|v| v.as_u64())
        .unwrap_or(60)
        .clamp(1, 600);

    // 존재 여부 확인 — 없으면 즉시 에러.
    let initial = ctx
        .state
        .store
        .get_task(task_id)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;
    let Some(initial_task) = initial else {
        return Ok(schema::tool_error(format!("task not found: {task_id}")));
    };

    let mut buffer = String::new();
    let mut chunks_seen = 0u64;
    let mut polls_used = 0u64;
    let mut stopped_reason = "max_polls_reached";

    // 이미 종료 상태라면 한 번의 출력 읽기로 끝.
    if initial_task.is_terminal() {
        let output = ctx
            .state
            .store
            .get_output(task_id, offset)
            .await
            .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;
        for chunk in &output.chunks {
            buffer.push_str(&chunk.chunk);
            chunks_seen += 1;
        }
        offset = output.next_offset;
        polls_used = 1;
        stopped_reason = "terminal";
    } else {
        let sleep = std::time::Duration::from_secs(poll_interval_secs);
        for poll_idx in 1..=max_polls {
            let output = ctx
                .state
                .store
                .get_output(task_id, offset)
                .await
                .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;
            for chunk in &output.chunks {
                buffer.push_str(&chunk.chunk);
                chunks_seen += 1;
            }
            offset = output.next_offset;
            polls_used = poll_idx;

            // 상태 확인 — 매 폴링마다.
            let task = ctx
                .state
                .store
                .get_task(task_id)
                .await
                .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;
            if task.as_ref().is_some_and(|t| t.is_terminal()) {
                stopped_reason = "terminal";
                break;
            }

            // 마지막 폴링이 아니면 대기.
            if poll_idx < max_polls {
                tokio::time::sleep(sleep).await;
            }
        }
    }

    // 최종 위상 조회.
    let final_task = ctx
        .state
        .store
        .get_task(task_id)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;
    let phase = final_task
        .as_ref()
        .map(|t| phase_str(&t.status))
        .unwrap_or("unknown");

    Ok(schema::tool_json(&json!({
        "task_id": task_id.to_string(),
        "phase": phase,
        "output": buffer,
        "chunks_seen": chunks_seen,
        "next_offset": offset,
        "polls_used": polls_used,
        "stopped_reason": stopped_reason,
    })))
}

// ── fleet_collect_results ───────────────────────────────────────────────

async fn handle_collect_results(
    ctx: &ToolContext,
    args: &Value,
) -> Result<Value, JsonRpcError> {
    let args = args.as_object().ok_or_else(|| {
        JsonRpcError::invalid_params("arguments must be a JSON object")
    })?;

    let ids_arr = args
        .get("task_ids")
        .and_then(|v| v.as_array())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: task_ids (array)"))?;

    if ids_arr.is_empty() {
        return Err(JsonRpcError::invalid_params("task_ids must not be empty"));
    }
    if ids_arr.len() > 200 {
        return Err(JsonRpcError::invalid_params(
            "task_ids length exceeds maximum of 200",
        ));
    }

    let include_output = args
        .get("include_output")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // task_id 문자열 파싱 — 하나라도 잘못되면 전체 실패.
    let mut task_ids: Vec<TaskId> = Vec::with_capacity(ids_arr.len());
    for (i, v) in ids_arr.iter().enumerate() {
        let s = v.as_str().ok_or_else(|| {
            JsonRpcError::invalid_params(format!("task_ids[{i}] must be a string"))
        })?;
        let id: TaskId = s.parse().map_err(|e| {
            JsonRpcError::invalid_params(format!("task_ids[{i}] invalid uuid: {e}"))
        })?;
        task_ids.push(id);
    }

    // 병렬 조회 — futures::future::join_all.
    let store = ctx.state.store.clone();
    let futures_iter = task_ids.iter().map(|&id| {
        let store = store.clone();
        async move {
            let result = store.get_task(id).await;
            (id, result)
        }
    });
    let results = futures::future::join_all(futures_iter).await;

    let mut entries = Vec::with_capacity(results.len());
    let mut not_found = 0u32;
    let mut terminal = 0u32;
    for (id, result) in results {
        match result {
            Ok(Some(task)) => {
                if task.is_terminal() {
                    terminal += 1;
                }
                entries.push(task_summary_compact(&task, include_output));
            }
            Ok(None) => {
                not_found += 1;
                entries.push(json!({
                    "task_id": id.to_string(),
                    "phase": "not_found",
                    "error": "task not found",
                }));
            }
            Err(e) => {
                entries.push(json!({
                    "task_id": id.to_string(),
                    "phase": "error",
                    "error": format!("store error: {e}"),
                }));
            }
        }
    }

    Ok(schema::tool_json(&json!({
        "results": entries,
        "count": entries.len(),
        "summary": {
            "terminal": terminal,
            "not_found": not_found,
            "total": entries.len(),
        },
    })))
}

/// 클라이언트에게 반환할 작업 요약. 전체 `Task`에서 핵심 필드만 발췌.
fn task_summary(task: &Task) -> Value {
    task_summary_with_options(task, true)
}

/// `include_output`으로 출력 포함 여부를 제어하는 작업 요약.
/// `fleet_collect_results`에서 대량 조회 시 출력을 생략하기 위해 사용.
fn task_summary_with_options(task: &Task, include_output: bool) -> Value {
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
            if include_output {
                summary["output"] = json!(result.output);
            } else {
                summary["output_bytes"] = json!(result.output.len());
            }
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

/// `fleet_collect_results`용 compact 요약. `task_summary_with_options`의 thin wrapper.
fn task_summary_compact(task: &Task, include_output: bool) -> Value {
    task_summary_with_options(task, include_output)
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

    #[test]
    fn task_summary_with_output_includes_output_field() {
        use fleet_core::{TaskResult, TaskStatus, WorkerId};
        let result = TaskResult {
            output: "build finished".into(),
            exit_code: 0,
            duration_secs: 12.5,
            token_usage: None,
            worker_id: WorkerId::new(),
            finished_at: chrono::Utc::now(),
        };
        let task = Task {
            id: TaskId::new(),
            prompt: "cargo build".into(),
            cwd: None,
            model: None,
            server_hint: None,
            required_labels: vec![],
            max_turns: None,
            timeout_secs: None,
            created_at: chrono::Utc::now(),
            created_by: "test".into(),
            priority: fleet_core::TaskPriority::Normal,
            status: TaskStatus::Completed(result),
        };
        let summary = task_summary_with_options(&task, true);
        assert_eq!(summary["phase"], "completed");
        assert_eq!(summary["output"], "build finished");
        assert!(summary.get("output_bytes").is_none());
    }

    #[test]
    fn task_summary_without_output_shows_byte_count() {
        use fleet_core::{TaskResult, TaskStatus, WorkerId};
        let result = TaskResult {
            output: "build finished".into(),
            exit_code: 0,
            duration_secs: 12.5,
            token_usage: None,
            worker_id: WorkerId::new(),
            finished_at: chrono::Utc::now(),
        };
        let task = Task {
            id: TaskId::new(),
            prompt: "cargo build".into(),
            cwd: None,
            model: None,
            server_hint: None,
            required_labels: vec![],
            max_turns: None,
            timeout_secs: None,
            created_at: chrono::Utc::now(),
            created_by: "test".into(),
            priority: fleet_core::TaskPriority::Normal,
            status: TaskStatus::Completed(result),
        };
        let summary = task_summary_with_options(&task, false);
        assert_eq!(summary["phase"], "completed");
        assert!(summary.get("output").is_none());
        // "build finished" = 14 bytes
        assert_eq!(summary["output_bytes"], 14);
    }
}
