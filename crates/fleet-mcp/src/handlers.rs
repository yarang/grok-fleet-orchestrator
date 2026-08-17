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

use fleet_core::{
    CircuitState, FleetEvent, Host, Task, TaskFilter, TaskId, TaskRequest, TaskStatusFilter,
    WorkerFilter, WorkerId, WorkerStatus,
};
use fleet_scheduler::{BreakerState, Dispatcher, FleetState};
use tracing::debug;

use crate::schema::{
    self, JsonRpcError, TOOL_CANCEL_TASK, TOOL_COLLECT_RESULTS, TOOL_DISPATCH_TASK,
    TOOL_GET_TASK_STATUS, TOOL_LIST_BOOTSTRAP_TOKENS, TOOL_LIST_HOSTS, TOOL_LIST_TASKS,
    TOOL_LIST_WORKERS, TOOL_RESET_WORKER_BREAKER, TOOL_REVOKE_BOOTSTRAP_TOKEN,
    TOOL_STREAM_TASK_OUTPUT, TOOL_WAIT_FOR_TASK,
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
        TOOL_LIST_TASKS => handle_list_tasks(ctx, arguments).await,
        TOOL_CANCEL_TASK => handle_cancel_task(ctx, arguments).await,
        TOOL_WAIT_FOR_TASK => handle_wait_for_task(ctx, arguments).await,
        TOOL_STREAM_TASK_OUTPUT => handle_stream_task_output(ctx, arguments).await,
        TOOL_COLLECT_RESULTS => handle_collect_results(ctx, arguments).await,
        TOOL_LIST_HOSTS => handle_list_hosts(ctx, arguments).await,
        TOOL_RESET_WORKER_BREAKER => handle_reset_worker_breaker(ctx, arguments).await,
        TOOL_LIST_BOOTSTRAP_TOKENS => handle_list_bootstrap_tokens(ctx, arguments).await,
        TOOL_REVOKE_BOOTSTRAP_TOKEN => handle_revoke_bootstrap_token(ctx, arguments).await,
        other => Err(JsonRpcError::method_not_found(other)),
    }
}

// ── fleet_dispatch_task ─────────────────────────────────────────────────

async fn handle_dispatch_task(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

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
    req.max_turns = args
        .get("max_turns")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    req.timeout_secs = args.get("timeout_secs").and_then(|v| v.as_u64());
    req.skills_required = args
        .get("skills_required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    req.created_by = "mcp".to_string();

    let task = Task::from_request(req);
    let task_id = task.id;

    match ctx.dispatcher.submit(task).await {
        Ok(returned_id) => {
            debug!(%returned_id, "dispatch_task succeeded");
            // 로드맵 #38: 재시도가 활성화된 배포에서는 submit()이 워커 선택
            // 실패/CircuitOpen에서도 Ok(task_id)를 반환할 수 있다 — 그 경우
            // 작업은 아직 Pending(Reconciler의 백그라운드 재시도 대기)이므로
            // "dispatched"라고 단정하지 않고 실제 상태를 조회해 보고한다.
            let status = ctx
                .state
                .store
                .get_task(returned_id)
                .await
                .ok()
                .flatten()
                .map(|t| phase_str(&t.status))
                .unwrap_or("dispatched");
            Ok(schema::tool_json(&json!({
                "task_id": returned_id.to_string(),
                "status": status,
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

async fn handle_get_task_status(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

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
        return Ok(schema::tool_error(format!("task not found: {task_id}")));
    };

    Ok(schema::tool_json(&task_summary(&task)))
}

// ── fleet_cancel_task ───────────────────────────────────────────────────

async fn handle_cancel_task(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

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

async fn handle_wait_for_task(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

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

async fn handle_stream_task_output(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

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

async fn handle_collect_results(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

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
        fleet_core::TaskStatus::Dispatched {
            worker_id,
            started_at,
        } => {
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
            if let Some(ref usage) = result.token_usage {
                summary["token_usage"] = json!({
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                    "cache_read_tokens": usage.cache_read_tokens,
                    "total_tokens": usage.total(),
                });
            }
        }
        fleet_core::TaskStatus::Failed(failure) => {
            summary["error"] = json!(failure.error);
            summary["failure_kind"] = json!(format!("{:?}", failure.kind));
            if let Some(wid) = failure.worker_id {
                summary["worker_id"] = json!(wid.to_string());
            }
        }
        fleet_core::TaskStatus::Cancelled {
            reason,
            cancelled_at,
        } => {
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

async fn handle_list_workers(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let mut filter = WorkerFilter::default();

    if let Some(obj) = args.as_object() {
        if let Some(status_str) = obj.get("status").and_then(|v| v.as_str()) {
            filter.status = Some(parse_worker_status(status_str)?);
        }
        if let Some(labels) = obj.get("labels").and_then(|v| v.as_object()) {
            for (k, v) in labels {
                let val = v.as_str().ok_or_else(|| {
                    JsonRpcError::invalid_params(format!("label '{k}' value must be a string"))
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

// ── fleet_list_tasks ────────────────────────────────────────────────────

async fn handle_list_tasks(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let mut filter = TaskFilter::default();

    if let Some(obj) = args.as_object() {
        if let Some(status_str) = obj.get("status").and_then(|v| v.as_str()) {
            filter.status = Some(parse_task_status_filter(status_str)?);
        }
        if let Some(limit) = obj.get("limit").and_then(|v| v.as_u64()) {
            filter.limit = limit as usize;
        }
        if let Some(offset) = obj.get("offset").and_then(|v| v.as_u64()) {
            filter.offset = offset as usize;
        }
    }

    let tasks = ctx
        .state
        .store
        .list_tasks(&filter)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    let summary: Vec<Value> = tasks
        .iter()
        .map(|t| task_summary_with_options(t, false))
        .collect();

    Ok(schema::tool_json(&json!({
        "tasks": summary,
        "count": summary.len(),
    })))
}

/// `TaskStatusFilter` 문자열 → enum. snake_case 매칭.
fn parse_task_status_filter(s: &str) -> Result<TaskStatusFilter, JsonRpcError> {
    match s {
        "pending" => Ok(TaskStatusFilter::Pending),
        "dispatched" => Ok(TaskStatusFilter::Dispatched),
        "completed" => Ok(TaskStatusFilter::Completed),
        "failed" => Ok(TaskStatusFilter::Failed),
        "cancelled" => Ok(TaskStatusFilter::Cancelled),
        "terminal" => Ok(TaskStatusFilter::Terminal),
        "active" => Ok(TaskStatusFilter::Active),
        other => Err(JsonRpcError::invalid_params(format!(
            "invalid status '{other}': expected one of pending, dispatched, completed, failed, cancelled, terminal, active"
        ))),
    }
}

// ── fleet_list_hosts ─────────────────────────────────────────────────────

async fn handle_list_hosts(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let status_filter = args
        .as_object()
        .and_then(|obj| obj.get("status"))
        .and_then(|v| v.as_str())
        .map(parse_host_status)
        .transpose()?;

    let mut hosts = ctx
        .state
        .store
        .list_hosts()
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    if let Some(status) = status_filter {
        hosts.retain(|h| h.status == status);
    }

    let summary: Vec<Value> = hosts.iter().map(host_summary).collect();

    Ok(schema::tool_json(&json!({
        "hosts": summary,
        "count": summary.len(),
    })))
}

/// `HostStatus` 문자열 → enum. snake_case 매칭.
fn parse_host_status(s: &str) -> Result<fleet_core::HostStatus, JsonRpcError> {
    fleet_core::HostStatus::parse(s).ok_or_else(|| {
        JsonRpcError::invalid_params(format!(
            "invalid status '{s}': expected one of provisioned, online, offline, failed"
        ))
    })
}

/// 클라이언트에게 반환할 호스트 요약.
fn host_summary(h: &Host) -> Value {
    json!({
        "id": h.id.to_string(),
        "hostname": h.hostname,
        "worker_id": h.worker_id.map(|w| w.to_string()),
        "status": h.status.as_str(),
        "ssh_host": h.ssh_host,
        "ssh_port": h.ssh_port,
        "grok_version": h.grok_version,
        "fleet_worker_version": h.fleet_worker_version,
        "load_avg": h.metrics.load_avg,
        "mem_available_mb": h.metrics.mem_available_mb,
        "disk_free_mb": h.metrics.disk_free_mb,
        "last_heartbeat_at": h.last_heartbeat_at.map(|t| t.to_rfc3339()),
        "provisioned_at": h.provisioned_at.map(|t| t.to_rfc3339()),
    })
}

// ── fleet_reset_worker_breaker ───────────────────────────────────────────

async fn handle_reset_worker_breaker(
    ctx: &ToolContext,
    args: &Value,
) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

    let worker_id_str = args.get("worker_id").and_then(|v| v.as_str());
    let worker_name = args.get("worker_name").and_then(|v| v.as_str());

    let worker_id: WorkerId = match (worker_id_str, worker_name) {
        (Some(_), Some(_)) => {
            return Err(JsonRpcError::invalid_params(
                "provide exactly one of worker_id or worker_name, not both",
            ));
        }
        (Some(s), None) => s
            .parse()
            .map_err(|e| JsonRpcError::invalid_params(format!("invalid worker_id: {e}")))?,
        (None, Some(name)) => {
            let worker = ctx
                .state
                .store
                .get_worker_by_name(name)
                .await
                .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;
            match worker {
                Some(w) => w.id,
                None => return Ok(schema::tool_error(format!("worker not found: {name}"))),
            }
        }
        (None, None) => {
            return Err(JsonRpcError::invalid_params(
                "one of worker_id or worker_name is required",
            ));
        }
    };

    let from_state = ctx.state.breakers.state_of(worker_id);
    ctx.state.breakers.reset(worker_id);

    let from = breaker_state_to_circuit_state(from_state);
    let _ = ctx
        .state
        .store
        .update_worker_circuit_state(worker_id, CircuitState::Closed)
        .await;
    let _ = ctx
        .state
        .store
        .append_event(&FleetEvent::worker_circuit_changed(
            worker_id,
            from,
            CircuitState::Closed,
        ))
        .await;

    Ok(schema::tool_json(&json!({
        "worker_id": worker_id.to_string(),
        "previous_state": format!("{from_state:?}").to_lowercase(),
        "new_state": "closed",
    })))
}

fn breaker_state_to_circuit_state(s: BreakerState) -> CircuitState {
    match s {
        BreakerState::Closed => CircuitState::Closed,
        BreakerState::Open => CircuitState::Open,
        BreakerState::HalfOpen => CircuitState::HalfOpen,
    }
}

// ── fleet_list_bootstrap_tokens ──────────────────────────────────────────

async fn handle_list_bootstrap_tokens(
    ctx: &ToolContext,
    _args: &Value,
) -> Result<Value, JsonRpcError> {
    let tokens = ctx
        .state
        .store
        .list_bootstrap_tokens()
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    let summary: Vec<Value> = tokens
        .iter()
        .map(|t| {
            json!({
                "token_id": t.public_id(),
                "created_at": t.created_at.to_rfc3339(),
                "expires_at": t.expires_at.map(|e| e.to_rfc3339()),
                "max_uses": t.max_uses,
                "use_count": t.use_count,
                "usable": t.is_usable(),
                "notes": t.notes,
                "last_used_by": t.last_used_by,
                "last_used_at": t.last_used_at.map(|e| e.to_rfc3339()),
            })
        })
        .collect();

    Ok(schema::tool_json(&json!({
        "tokens": summary,
        "count": summary.len(),
    })))
}

// ── fleet_revoke_bootstrap_token ─────────────────────────────────────────

async fn handle_revoke_bootstrap_token(
    ctx: &ToolContext,
    args: &Value,
) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

    let token_id = args
        .get("token_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: token_id"))?;

    let token = ctx
        .state
        .store
        .list_bootstrap_tokens()
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?
        .into_iter()
        .find(|candidate| candidate.public_id() == token_id)
        .map(|candidate| candidate.token);

    let Some(token) = token else {
        return Ok(schema::tool_error("bootstrap token not found"));
    };

    let revoked = ctx
        .state
        .store
        .revoke_bootstrap_token(&token)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    if revoked {
        Ok(schema::tool_json(&json!({
            "token_id": token_id,
            "revoked": true,
        })))
    } else {
        Ok(schema::tool_error(format!(
            "bootstrap token not found"
        )))
    }
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
        use fleet_core::{FailureKind, TaskFailure};
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
    fn parse_task_status_filter_accepts_all_variants() {
        assert!(parse_task_status_filter("pending").is_ok());
        assert!(parse_task_status_filter("dispatched").is_ok());
        assert!(parse_task_status_filter("completed").is_ok());
        assert!(parse_task_status_filter("failed").is_ok());
        assert!(parse_task_status_filter("cancelled").is_ok());
        assert!(parse_task_status_filter("terminal").is_ok());
        assert!(parse_task_status_filter("active").is_ok());
    }

    #[test]
    fn parse_task_status_filter_rejects_unknown() {
        assert!(parse_task_status_filter("bogus").is_err());
        assert!(parse_task_status_filter("").is_err());
    }

    #[test]
    fn all_tools_includes_list_tasks() {
        let tools = schema::all_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        assert!(names.contains(&"fleet_list_tasks"));
        assert_eq!(tools.len(), 12);
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
        let id = TaskId::new();
        let mut task = Task::from_request(TaskRequest {
            prompt: "cargo build".into(),
            created_by: "test".into(),
            ..Default::default()
        });
        task.id = id;
        task.thread_id = id;
        task.status = TaskStatus::Completed(result);

        let summary = task_summary_with_options(&task, true);
        assert_eq!(summary["phase"], "completed");
        assert_eq!(summary["output"], "build finished");
        assert!(summary.get("output_bytes").is_none());
        // token_usage is None → field omitted.
        assert!(summary.get("token_usage").is_none());
    }

    #[test]
    fn task_summary_includes_token_usage_when_present() {
        use fleet_core::{TaskResult, TaskStatus, TokenUsage, WorkerId};
        let result = TaskResult {
            output: "done".into(),
            exit_code: 0,
            duration_secs: 5.0,
            token_usage: Some(TokenUsage {
                input_tokens: 100,
                output_tokens: 200,
                cache_read_tokens: 50,
            }),
            worker_id: WorkerId::new(),
            finished_at: chrono::Utc::now(),
        };
        let id = TaskId::new();
        let mut task = Task::from_request(TaskRequest {
            prompt: "test".into(),
            created_by: "test".into(),
            ..Default::default()
        });
        task.id = id;
        task.thread_id = id;
        task.status = TaskStatus::Completed(result);

        let summary = task_summary_with_options(&task, false);
        assert_eq!(summary["token_usage"]["input_tokens"], 100);
        assert_eq!(summary["token_usage"]["output_tokens"], 200);
        assert_eq!(summary["token_usage"]["cache_read_tokens"], 50);
        assert_eq!(summary["token_usage"]["total_tokens"], 300);
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
        let id = TaskId::new();
        let mut task = Task::from_request(TaskRequest {
            prompt: "cargo build".into(),
            created_by: "test".into(),
            ..Default::default()
        });
        task.id = id;
        task.thread_id = id;
        task.status = TaskStatus::Completed(result);

        let summary = task_summary_with_options(&task, false);
        assert_eq!(summary["phase"], "completed");
        assert!(summary.get("output").is_none());
        // "build finished" = 14 bytes
        assert_eq!(summary["output_bytes"], 14);
    }

    #[test]
    fn parse_host_status_accepts_known_values() {
        assert!(parse_host_status("provisioned").is_ok());
        assert!(parse_host_status("online").is_ok());
        assert!(parse_host_status("offline").is_ok());
        assert!(parse_host_status("failed").is_ok());
        assert!(parse_host_status("bogus").is_err());
    }

    #[test]
    fn breaker_state_maps_to_matching_circuit_state() {
        assert_eq!(
            breaker_state_to_circuit_state(BreakerState::Closed),
            CircuitState::Closed
        );
        assert_eq!(
            breaker_state_to_circuit_state(BreakerState::Open),
            CircuitState::Open
        );
        assert_eq!(
            breaker_state_to_circuit_state(BreakerState::HalfOpen),
            CircuitState::HalfOpen
        );
    }

    // ── 신규 도구(host/breaker/token) 핸들러 통합 테스트 ────────────────
    //
    // fleet_store::mem::MemStore(로드맵 #45)로 실제 ToolContext를 구성해
    // dispatch_tool을 그대로 호출한다 — 이 파일의 기존 handle_* 함수들은
    // (cross_client.rs의 subprocess+DB 통합 테스트를 빼면) 단위 수준에서
    // 검증된 적이 없었다.

    fn test_ctx(store: fleet_store::mem::MemStore) -> ToolContext {
        let store: Arc<dyn fleet_store::Store> = Arc::new(store);
        let transport = Arc::new(fleet_transport::MockTransport::new())
            as Arc<dyn fleet_transport::WorkerTransport>;
        let state = Arc::new(FleetState::new(
            store,
            transport,
            fleet_core::CircuitBreakerConfig::default(),
        ));
        let dispatcher = Arc::new(Dispatcher::new(state.clone()));
        ToolContext::new(state, dispatcher)
    }

    fn sample_host(hostname: &str, status: fleet_core::HostStatus) -> Host {
        Host {
            id: uuid::Uuid::new_v4(),
            hostname: hostname.into(),
            worker_id: None,
            status,
            ssh_host: Some(format!("{hostname}.example")),
            ssh_port: 22,
            ssh_user: Some("fleet".into()),
            grok_version: None,
            fleet_worker_version: None,
            os_info: None,
            metrics: Default::default(),
            last_heartbeat_at: None,
            provisioned_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn list_hosts_returns_seeded_hosts() {
        let store = fleet_store::mem::MemStore::new()
            .with_host(sample_host("node-a", fleet_core::HostStatus::Online))
            .with_host(sample_host("node-b", fleet_core::HostStatus::Failed));
        let ctx = test_ctx(store);

        let result = dispatch_tool(&ctx, TOOL_LIST_HOSTS, &json!({}))
            .await
            .unwrap();
        let body: Value = parse_tool_json(&result);
        assert_eq!(body["count"], 2);
    }

    #[tokio::test]
    async fn list_hosts_filters_by_status() {
        let store = fleet_store::mem::MemStore::new()
            .with_host(sample_host("node-a", fleet_core::HostStatus::Online))
            .with_host(sample_host("node-b", fleet_core::HostStatus::Failed));
        let ctx = test_ctx(store);

        let result = dispatch_tool(&ctx, TOOL_LIST_HOSTS, &json!({"status": "failed"}))
            .await
            .unwrap();
        let body: Value = parse_tool_json(&result);
        assert_eq!(body["count"], 1);
        assert_eq!(body["hosts"][0]["hostname"], "node-b");
    }

    #[tokio::test]
    async fn reset_worker_breaker_by_id_closes_open_breaker() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let worker_id = WorkerId::new();
        // Open 상태로 미리 생성 — check()를 여러 번 실패시키지 않고 직접 시딩.
        ctx.state.breakers.get(worker_id, CircuitState::Open);

        let result = dispatch_tool(
            &ctx,
            TOOL_RESET_WORKER_BREAKER,
            &json!({"worker_id": worker_id.to_string()}),
        )
        .await
        .unwrap();
        let body: Value = parse_tool_json(&result);
        assert_eq!(body["previous_state"], "open");
        assert_eq!(body["new_state"], "closed");
        assert_eq!(ctx.state.breakers.state_of(worker_id), BreakerState::Closed);
    }

    #[tokio::test]
    async fn reset_worker_breaker_by_name_resolves_worker() {
        let mut worker = fleet_core::Worker::new("resettable", "wss://resettable/ws");
        let worker_id = worker.id;
        worker.status = WorkerStatus::CircuitOpen;
        let store = fleet_store::mem::MemStore::new().with_worker(worker);
        let ctx = test_ctx(store);
        ctx.state.breakers.get(worker_id, CircuitState::Open);

        let result = dispatch_tool(
            &ctx,
            TOOL_RESET_WORKER_BREAKER,
            &json!({"worker_name": "resettable"}),
        )
        .await
        .unwrap();
        let body: Value = parse_tool_json(&result);
        assert_eq!(body["worker_id"], worker_id.to_string());
        assert_eq!(body["new_state"], "closed");
    }

    #[tokio::test]
    async fn reset_worker_breaker_unknown_name_returns_tool_error() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let result = dispatch_tool(
            &ctx,
            TOOL_RESET_WORKER_BREAKER,
            &json!({"worker_name": "ghost"}),
        )
        .await
        .unwrap();
        assert_eq!(result["isError"], true);
    }

    #[tokio::test]
    async fn reset_worker_breaker_requires_exactly_one_identifier() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        assert!(
            dispatch_tool(&ctx, TOOL_RESET_WORKER_BREAKER, &json!({}))
                .await
                .is_err()
        );
        assert!(dispatch_tool(
            &ctx,
            TOOL_RESET_WORKER_BREAKER,
            &json!({"worker_id": WorkerId::new().to_string(), "worker_name": "both"})
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn list_and_revoke_bootstrap_tokens_round_trip() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let token = fleet_core::BootstrapToken {
            token: "fbt_test123".into(),
            created_at: chrono::Utc::now(),
            created_by: None,
            expires_at: None,
            max_uses: 1,
            use_count: 0,
            notes: None,
            last_used_by: None,
            last_used_at: None,
        };
        ctx.state.store.create_bootstrap_token(&token).await.unwrap();

        let listed = dispatch_tool(&ctx, TOOL_LIST_BOOTSTRAP_TOKENS, &json!({}))
            .await
            .unwrap();
        let body: Value = parse_tool_json(&listed);
        assert_eq!(body["count"], 1);
        let token_id = body["tokens"][0]["token_id"].as_str().unwrap();
        assert_eq!(token_id, fleet_core::BootstrapToken::public_id_for("fbt_test123"));
        assert!(body["tokens"][0].get("token").is_none());

        let revoked = dispatch_tool(
            &ctx,
            TOOL_REVOKE_BOOTSTRAP_TOKEN,
            &json!({"token_id": token_id}),
        )
        .await
        .unwrap();
        let body: Value = parse_tool_json(&revoked);
        assert_eq!(body["revoked"], true);

        let listed_after = dispatch_tool(&ctx, TOOL_LIST_BOOTSTRAP_TOKENS, &json!({}))
            .await
            .unwrap();
        let body: Value = parse_tool_json(&listed_after);
        assert_eq!(body["count"], 0);
    }

    #[tokio::test]
    async fn revoke_unknown_bootstrap_token_returns_tool_error() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let result = dispatch_tool(
            &ctx,
            TOOL_REVOKE_BOOTSTRAP_TOKEN,
            &json!({"token_id": "bt_does-not-exist"}),
        )
        .await
        .unwrap();
        assert_eq!(result["isError"], true);
    }

    /// `schema::tool_json`이 만든 `{content:[{type:"text",text:"<json>"}]}`에서
    /// 원본 JSON 값을 다시 파싱.
    fn parse_tool_json(result: &Value) -> Value {
        let text = result["content"][0]["text"].as_str().unwrap();
        serde_json::from_str(text).unwrap()
    }
}
