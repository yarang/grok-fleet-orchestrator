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
    CircuitState, FleetEvent, Host, IssueId, ProjectId, Task, TaskFilter, TaskId, TaskRequest,
    TaskStatusFilter, WorkerFilter, WorkerId, WorkerStatus,
};
use fleet_scheduler::{BreakerState, Dispatcher, FleetState};
use tracing::debug;

use crate::schema::{
    self, JsonRpcError, TOOL_CANCEL_TASK, TOOL_COLLECT_RESULTS, TOOL_COMMENT_ISSUE,
    TOOL_CREATE_AGENT, TOOL_CREATE_ISSUE, TOOL_CREATE_PROJECT, TOOL_DELETE_PROJECT,
    TOOL_DISPATCH_TASK, TOOL_GET_TASK_STATUS, TOOL_LIST_AGENTS, TOOL_LIST_BOOTSTRAP_TOKENS,
    TOOL_LIST_HOSTS, TOOL_LIST_ISSUES, TOOL_LIST_PROJECTS, TOOL_LIST_TASKS, TOOL_LIST_WORKERS,
    TOOL_RESET_WORKER_BREAKER, TOOL_REVOKE_BOOTSTRAP_TOKEN, TOOL_STOP_AGENT,
    TOOL_STREAM_TASK_OUTPUT, TOOL_TRANSITION_ISSUE, TOOL_WAIT_FOR_TASK,
};

/// 도구 호출 컨텍스트. 핸들러가 필요로 하는 모든 의존성을 캡슐화.
#[derive(Clone)]
pub struct ToolContext {
    pub state: Arc<FleetState>,
    pub dispatcher: Arc<Dispatcher>,
    /// launcher가 부여한 capability 집합 (`FLEET_MCP_CAPABILITIES`).
    ///
    /// 대부분의 도구는 `server::required_permission` 행렬이 호출 **전에**
    /// 판정하므로 핸들러가 이걸 볼 일이 없다. 예외는 요구 capability가
    /// 인자에 따라 달라지는 도구다 — `fleet_transition_issue`는 목표
    /// 상태마다 다른 capability를 요구해(로드맵 #92) 행렬 하나로 판정할 수
    /// 없고, 핸들러가 직접 확인한다.
    ///
    /// 기본값은 **빈 집합**이다(fail-closed) — 명시적으로 부여하지 않으면
    /// 인자 의존 도구는 전부 거절된다.
    pub capabilities: Vec<fleet_core::PermissionKind>,
}

impl ToolContext {
    pub fn new(state: Arc<FleetState>, dispatcher: Arc<Dispatcher>) -> Self {
        Self {
            state,
            dispatcher,
            capabilities: Vec::new(),
        }
    }

    pub fn with_capabilities(mut self, capabilities: Vec<fleet_core::PermissionKind>) -> Self {
        self.capabilities = capabilities;
        self
    }

    fn has(&self, capability: fleet_core::PermissionKind) -> bool {
        self.capabilities.contains(&capability)
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
        TOOL_CREATE_PROJECT => handle_create_project(ctx, arguments).await,
        TOOL_LIST_PROJECTS => handle_list_projects(ctx, arguments).await,
        TOOL_DELETE_PROJECT => handle_delete_project(ctx, arguments).await,
        TOOL_CREATE_AGENT => handle_create_agent(ctx, arguments).await,
        TOOL_LIST_AGENTS => handle_list_agents(ctx, arguments).await,
        TOOL_STOP_AGENT => handle_stop_agent(ctx, arguments).await,
        TOOL_LIST_ISSUES => handle_list_issues(ctx, arguments).await,
        TOOL_CREATE_ISSUE => handle_create_issue(ctx, arguments).await,
        TOOL_TRANSITION_ISSUE => handle_transition_issue(ctx, arguments).await,
        TOOL_COMMENT_ISSUE => handle_comment_issue(ctx, arguments).await,
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
    req.project_id = match args.get("project_id") {
        Some(value) => {
            let project_id = value
                .as_str()
                .ok_or_else(|| JsonRpcError::invalid_params("project_id must be a UUID string"))?
                .parse::<ProjectId>()
                .map_err(|_| JsonRpcError::invalid_params("project_id must be a UUID"))?;

            // 로드맵 #48 1단계 — 지금까지 project_id는 저장만 되고 아무도
            // 검증하지 않는 순수 메타데이터였다. 여기서 처음으로 실제 존재·
            // 상태 확인을 건다. 검증 규칙은 Dashboard `POST /api/tasks`와
            // 공유한다(`fleet_store::ensure_project_accepts_new_tasks`) —
            // 계약 문서가 두 표면의 동일 동작을 요구한다.
            fleet_store::ensure_project_accepts_new_tasks(ctx.state.store.as_ref(), project_id)
                .await
                .map_err(|e| match e {
                    fleet_store::ProjectAdmissionError::NotFound(_)
                    | fleet_store::ProjectAdmissionError::NotAccepting { .. } => {
                        JsonRpcError::invalid_params(e.to_string())
                    }
                    fleet_store::ProjectAdmissionError::Store(inner) => {
                        JsonRpcError::internal(format!("failed to look up project: {inner}"))
                    }
                })?;

            Some(project_id)
        }
        None => None,
    };
    req.skills_required = args
        .get("skills_required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    // 로드맵 #62 2단계 — 제출 멱등성. 빈 문자열은 키가 아니다: 클라이언트가
    // 실수로 빈 값을 보내면 "모두가 공유하는 하나의 키"가 되어, 서로 무관한
    // 제출들이 전부 중복으로 판정된다. `None`으로 접어 멱등성 검사를 끈다.
    req.idempotency_key = args
        .get("idempotency_key")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(String::from);
    // 이 표면에는 호출자 principal이 없다 — `ToolContext`는 capability 집합만
    // 들고 있고(server.rs의 `McpAuthorization`), stdio 런처가 신원을 전달하지
    // 않는다. 그래서 모든 MCP 제출이 `created_by = "mcp"` 한 버킷을 공유하고,
    // 멱등성 키 네임스페이스도 그 버킷 단위다(마이그레이션 024 참고).
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
            // 멱등성 키가 기존 Task를 되살렸다면 submit()은 우리가 만든 id가
            // 아니라 그 Task의 id를 돌려준다. 그 사실을 클라이언트에게 알린다 —
            // 모르면 "방금 새 작업을 시작했다"고 오해하고, 이미 완료된 작업의
            // 상태를 새 작업의 진행으로 읽는다.
            let deduplicated = returned_id != task_id;
            Ok(schema::tool_json(&json!({
                "task_id": returned_id.to_string(),
                "status": status,
                "deduplicated": deduplicated,
                "hint": if deduplicated {
                    "This idempotency_key already had a task; no new task was created. \
                     Poll fleet_get_task_status with the returned task_id."
                } else {
                    "Poll fleet_get_task_status with the task_id to observe completion."
                }
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
        // 로드맵 #75 — endpoint의 `server-key=` 값은 워커의 grok ACP 인증
        // 토큰 원문이다. MCP tool 호출자 중 그 값을 봐야 하는 사람은 없다.
        "endpoint": fleet_core::mask_server_key(&w.endpoint),
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

    let Some(token_digest) = fleet_core::BootstrapToken::digest_from_public_id(token_id) else {
        return Ok(schema::tool_error("bootstrap token not found"));
    };

    let revoked = ctx
        .state
        .store
        .revoke_bootstrap_token(&token_digest)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    if revoked {
        Ok(schema::tool_json(&json!({
            "token_id": token_id,
            "revoked": true,
        })))
    } else {
        Ok(schema::tool_error("bootstrap token not found".to_string()))
    }
}

// ── fleet_create_project / fleet_list_projects / fleet_delete_project ──
// (로드맵 #48, 1단계)

fn project_json(p: &fleet_core::Project) -> Value {
    json!({
        "id": p.id.to_string(),
        "name": p.name,
        "description": p.description,
        "created_by": p.created_by,
        "status": p.status.as_str(),
        "created_at": p.created_at.to_rfc3339(),
        "updated_at": p.updated_at.to_rfc3339(),
    })
}

async fn handle_create_project(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: name"))?
        .trim();
    if name.is_empty() {
        return Err(JsonRpcError::invalid_params("name must not be empty"));
    }

    let mut project = fleet_core::Project::new(name);
    if let Some(description) = args.get("description").and_then(|v| v.as_str()) {
        let description = description.trim();
        if !description.is_empty() {
            project = project.with_description(description);
        }
    }

    ctx.state
        .store
        .create_project(&project)
        .await
        .map_err(|e| match e {
            fleet_store::StoreError::Conflict(msg) => JsonRpcError::invalid_params(msg),
            other => JsonRpcError::internal(format!("store error: {other}")),
        })?;

    Ok(schema::tool_json(&project_json(&project)))
}

async fn handle_list_projects(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(100);
    let offset = args
        .get("offset")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(0);

    let projects = ctx
        .state
        .store
        .list_projects(&fleet_core::ProjectFilter {
            status: None,
            limit,
            offset,
        })
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    let summary: Vec<Value> = projects.iter().map(project_json).collect();
    Ok(schema::tool_json(&json!({
        "projects": summary,
        "count": summary.len(),
    })))
}

async fn handle_delete_project(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

    let project_id: ProjectId = args
        .get("project_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: project_id"))?
        .parse()
        .map_err(|_| JsonRpcError::invalid_params("project_id must be a UUID"))?;

    let Some(mut project) = ctx
        .state
        .store
        .get_project(project_id)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?
    else {
        return Ok(schema::tool_error("project not found"));
    };

    // archive 절차는 Dashboard `DELETE /api/projects/{id}`와 공유한다
    // (`fleet_store::advance_project_archive`). MCP 표면에는 아직 감사
    // 파이프라인이 없어 상태 전이 콜백은 무시한다 — 감사 확장은 `#95`.
    let progress =
        fleet_store::advance_project_archive(ctx.state.store.as_ref(), &mut project, |_| {})
            .await
            .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    // 게이트가 막았으면 사유도 함께 싣는다 — Dashboard와 **같은 어휘**를
    // 쓴다(`ArchiveBlockers::labels`). 여기서 문자열을 따로 지으면 같은 사유가
    // 표면마다 다른 이름으로 갈리고, 계약이 요구하는 "두 표면의 동일 응답"이
    // 다시 깨진다.
    let mut body = project_json(&project);
    if let fleet_store::ArchiveProgress::Draining(blockers) = progress {
        body["archive_blocked_by"] = json!(blockers.labels());
    }

    Ok(schema::tool_json(&body))
}

// ── fleet_create_agent / fleet_list_agents / fleet_stop_agent ──────────
// (로드맵 #49, 1단계)
//
// Dashboard `/api/agents`와 **같은 규칙**을 쓴다 — Project admission은
// `fleet_store::ensure_project_accepts_new_agents`가 단일 구현이다.

fn agent_json(a: &fleet_core::Agent) -> Value {
    json!({
        "id": a.id.to_string(),
        "project_id": a.project_id.to_string(),
        "name": a.name,
        "description": a.description,
        "created_by": a.created_by,
        "status": a.status.as_str(),
        "created_at": a.created_at.to_rfc3339(),
        "updated_at": a.updated_at.to_rfc3339(),
    })
}

async fn handle_create_agent(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

    let project_id: ProjectId = args
        .get("project_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: project_id"))?
        .parse()
        .map_err(|_| JsonRpcError::invalid_params("project_id must be a UUID"))?;

    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: name"))?
        .trim();
    if name.is_empty() {
        return Err(JsonRpcError::invalid_params("name must not be empty"));
    }

    // Project가 실재하고 새 Agent를 받는 상태인지 — Agent의 `project_id`는
    // 불변이므로 이 검사를 통과시키면 되돌릴 방법이 없다.
    fleet_store::ensure_project_accepts_new_agents(ctx.state.store.as_ref(), project_id)
        .await
        .map_err(|e| match e {
            fleet_store::ProjectAdmissionError::NotFound(_)
            | fleet_store::ProjectAdmissionError::NotAccepting { .. } => {
                JsonRpcError::invalid_params(e.to_string())
            }
            fleet_store::ProjectAdmissionError::Store(inner) => {
                JsonRpcError::internal(format!("store error: {inner}"))
            }
        })?;

    let mut agent = fleet_core::Agent::new(project_id, name);
    if let Some(description) = args.get("description").and_then(|v| v.as_str()) {
        let description = description.trim();
        if !description.is_empty() {
            agent = agent.with_description(description);
        }
    }

    ctx.state
        .store
        .create_agent(&agent)
        .await
        .map_err(|e| match e {
            fleet_store::StoreError::Conflict(msg) => JsonRpcError::invalid_params(msg),
            other => JsonRpcError::internal(format!("store error: {other}")),
        })?;

    Ok(schema::tool_json(&agent_json(&agent)))
}

async fn handle_list_agents(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(raw) => Some(
            raw.parse::<ProjectId>()
                .map_err(|_| JsonRpcError::invalid_params("project_id must be a UUID"))?,
        ),
        None => None,
    };
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(100);
    let offset = args
        .get("offset")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(0);

    let agents = ctx
        .state
        .store
        .list_agents(&fleet_core::AgentFilter {
            project_id,
            status: None,
            limit,
            offset,
        })
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    let summary: Vec<Value> = agents.iter().map(agent_json).collect();
    Ok(schema::tool_json(&json!({
        "agents": summary,
        "count": summary.len(),
    })))
}

async fn handle_stop_agent(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

    let agent_id: fleet_core::AgentId = args
        .get("agent_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: agent_id"))?
        .parse()
        .map_err(|_| JsonRpcError::invalid_params("agent_id must be a UUID"))?;

    let Some(mut agent) = ctx
        .state
        .store
        .get_agent(agent_id)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?
    else {
        return Ok(schema::tool_error("agent not found"));
    };

    // 이미 `Stopped`면 쓰지 않는다 — `updated_at`을 무의미하게 갱신하면
    // "언제 회수됐는가"라는 기록이 재호출마다 밀린다.
    if agent.status != fleet_core::AgentStatus::Stopped {
        ctx.state
            .store
            .update_agent_status(agent.id, fleet_core::AgentStatus::Stopped)
            .await
            .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;
        // 로컬 필드만 고치면 응답의 `updated_at`이 회수 **이전** 값이라
        // 재호출 때 다른 값이 나온다. 저장된 행을 다시 읽어 두 호출이
        // 같은 답을 주도록 한다.
        agent = ctx
            .state
            .store
            .get_agent(agent.id)
            .await
            .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?
            .ok_or_else(|| JsonRpcError::internal("agent disappeared during stop"))?;
    }

    Ok(schema::tool_json(&agent_json(&agent)))
}

// ── Issue 도구 (로드맵 #92) ─────────────────────────────────────────────
//
// Dashboard HTTP 표면과 **같은 규칙**을 쓴다 — 상태 기계는
// `fleet_core::Issue::transition_to`, 전이별 요구 capability는
// `fleet_core::required_capability_for_transition`이 단일 구현이다.

fn issue_json(i: &fleet_core::Issue, has_active_tasks: bool) -> Value {
    json!({
        "id": i.id.to_string(),
        "project_id": i.project_id.to_string(),
        "title": i.title,
        "body": i.body,
        "status": i.status.as_str(),
        "close_reason": i.close_reason.map(|r| r.as_str()),
        "severity": i.severity.as_str(),
        "labels": i.labels,
        "assignee": i.assignee,
        "created_by": i.created_by,
        "created_at": i.created_at.to_rfc3339(),
        "updated_at": i.updated_at.to_rfc3339(),
        // 파생 값 — 저장된 상태가 아니다(`InProgress`를 두지 않은 이유).
        "has_active_tasks": has_active_tasks,
    })
}

fn parse_issue_id_arg(args: &serde_json::Map<String, Value>) -> Result<IssueId, JsonRpcError> {
    args.get("issue_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: issue_id"))?
        .parse::<IssueId>()
        .map_err(|_| JsonRpcError::invalid_params("issue_id must be a UUID"))
}

async fn handle_list_issues(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(raw) => Some(
            raw.parse::<ProjectId>()
                .map_err(|_| JsonRpcError::invalid_params("project_id must be a UUID"))?,
        ),
        None => None,
    };
    let status = match args.get("status").and_then(|v| v.as_str()) {
        Some(raw) => Some(
            fleet_core::IssueStatus::parse_str(raw)
                .ok_or_else(|| JsonRpcError::invalid_params(format!("unknown status: {raw}")))?,
        ),
        None => None,
    };

    let issues = ctx
        .state
        .store
        .list_issues(&fleet_core::IssueFilter {
            project_id,
            status,
            open_only: args
                .get("open_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            limit: 1000,
            offset: 0,
        })
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    let mut out = Vec::with_capacity(issues.len());
    for issue in &issues {
        let active = ctx
            .state
            .store
            .issue_has_active_tasks(issue.id)
            .await
            .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;
        out.push(issue_json(issue, active));
    }
    Ok(schema::tool_json(&json!({
        "issues": out,
        "count": out.len(),
    })))
}

async fn handle_create_issue(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: title"))?
        .trim();
    if title.is_empty() {
        return Err(JsonRpcError::invalid_params("title must not be empty"));
    }
    let project_id = args
        .get("project_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: project_id"))?
        .parse::<ProjectId>()
        .map_err(|_| JsonRpcError::invalid_params("project_id must be a UUID"))?;

    // Issue는 항상 Project 경계 안에 있다. Project **상태**는 보지 않는다 —
    // 계약이 "`Draining` 중에도 Issue 쓰기는 허용한다"고 명시했다(Dashboard
    // `create_issue_api`와 동일).
    if ctx
        .state
        .store
        .get_project(project_id)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?
        .is_none()
    {
        return Err(JsonRpcError::invalid_params(format!(
            "no such project: {project_id}"
        )));
    }

    let mut issue = fleet_core::Issue::new(project_id, title, "mcp");
    if let Some(body) = args.get("body").and_then(|v| v.as_str()) {
        issue.body = body.to_string();
    }
    if let Some(sev) = args.get("severity").and_then(|v| v.as_str()) {
        issue.severity = fleet_core::IssueSeverity::parse_str(sev)
            .ok_or_else(|| JsonRpcError::invalid_params(format!("unknown severity: {sev}")))?;
    }
    if let Some(labels) = args.get("labels").and_then(|v| v.as_array()) {
        issue.labels = labels
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    ctx.state
        .store
        .create_issue(&issue)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    Ok(schema::tool_json(&issue_json(&issue, false)))
}

async fn handle_transition_issue(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

    let to = fleet_core::IssueStatus::parse_str(
        args.get("status")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JsonRpcError::invalid_params("missing required field: status"))?,
    )
    .ok_or_else(|| JsonRpcError::invalid_params("unknown status"))?;

    // **인자 의존 인가** — 요구 capability가 목표 상태에 따라 달라 서버의
    // capability 행렬이 판정할 수 없다(server.rs의 `permits_tool` 참고).
    // Dashboard와 같은 함수를 쓴다.
    let required = fleet_core::required_capability_for_transition(to);
    if !ctx.has(required) {
        return Err(JsonRpcError::invalid_request(format!(
            "transition to '{}' requires the '{}' capability",
            to.as_str(),
            required.as_str()
        )));
    }

    let close_reason =
        match args.get("close_reason").and_then(|v| v.as_str()) {
            Some(raw) => Some(fleet_core::CloseReason::parse_str(raw).ok_or_else(|| {
                JsonRpcError::invalid_params(format!("unknown close_reason: {raw}"))
            })?),
            None => None,
        };

    let issue_id = parse_issue_id_arg(args)?;
    let Some(mut issue) = ctx
        .state
        .store
        .get_issue(issue_id)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?
    else {
        return Ok(schema::tool_error("issue not found"));
    };

    // 상태 기계 검증은 도메인 타입이 소유한다.
    if let Err(e) = issue.transition_to(to, close_reason) {
        return Ok(schema::tool_error(e.to_string()));
    }

    ctx.state
        .store
        .transition_issue(issue.id, issue.status, issue.close_reason)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    let active = ctx
        .state
        .store
        .issue_has_active_tasks(issue.id)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;
    Ok(schema::tool_json(&issue_json(&issue, active)))
}

async fn handle_comment_issue(ctx: &ToolContext, args: &Value) -> Result<Value, JsonRpcError> {
    let args = args
        .as_object()
        .ok_or_else(|| JsonRpcError::invalid_params("arguments must be a JSON object"))?;

    let body = args
        .get("body")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("missing required field: body"))?
        .trim();
    if body.is_empty() {
        return Err(JsonRpcError::invalid_params("body must not be empty"));
    }

    let issue_id = parse_issue_id_arg(args)?;
    if ctx
        .state
        .store
        .get_issue(issue_id)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?
        .is_none()
    {
        return Ok(schema::tool_error("issue not found"));
    }

    let comment = fleet_core::IssueComment::new(issue_id, "mcp", body);
    ctx.state
        .store
        .add_issue_comment(&comment)
        .await
        .map_err(|e| JsonRpcError::internal(format!("store error: {e}")))?;

    Ok(schema::tool_json(&json!({
        "id": comment.id.to_string(),
        "issue_id": issue_id.to_string(),
        "author": comment.author,
        "body": comment.body,
        "created_at": comment.created_at.to_rfc3339(),
    })))
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
    fn worker_summary_never_leaks_raw_server_key() {
        // 로드맵 #75 — MCP tool 호출자 중 endpoint의 server-key 원문을
        // 봐야 하는 사람은 없다.
        let w = fleet_core::Worker::new("leaky", "wss://leaky.example/ws?server-key=leaked-secret");
        let summary = worker_summary(&w);
        let rendered = summary.to_string();
        assert!(!rendered.contains("leaked-secret"));
        assert!(rendered.contains("<redacted>"));
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

    #[tokio::test]
    async fn dispatch_task_rejects_invalid_project_id() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let result = dispatch_tool(
            &ctx,
            TOOL_DISPATCH_TASK,
            &json!({"prompt": "test", "project_id": "not-a-uuid"}),
        )
        .await;

        assert!(result.is_err());
    }

    #[test]
    fn all_tools_includes_list_tasks() {
        let tools = schema::all_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        assert!(names.contains(&"fleet_list_tasks"));
        assert_eq!(tools.len(), 22);
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
        test_ctx_with_caps(store, fleet_core::PermissionKind::all().to_vec())
    }

    /// capability를 명시해 컨텍스트를 만든다 — 인자 의존 인가
    /// (`fleet_transition_issue`) 테스트용.
    fn test_ctx_with_caps(
        store: fleet_store::mem::MemStore,
        caps: Vec<fleet_core::PermissionKind>,
    ) -> ToolContext {
        build_ctx(store).with_capabilities(caps)
    }

    fn build_ctx(store: fleet_store::mem::MemStore) -> ToolContext {
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
        assert!(dispatch_tool(&ctx, TOOL_RESET_WORKER_BREAKER, &json!({}))
            .await
            .is_err());
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
            token_digest: fleet_core::BootstrapToken::digest_for("fbt_test123"),
            created_at: chrono::Utc::now(),
            created_by: None,
            expires_at: None,
            max_uses: 1,
            use_count: 0,
            notes: None,
            last_used_by: None,
            last_used_at: None,
        };
        ctx.state
            .store
            .create_bootstrap_token(&token)
            .await
            .unwrap();

        let listed = dispatch_tool(&ctx, TOOL_LIST_BOOTSTRAP_TOKENS, &json!({}))
            .await
            .unwrap();
        let body: Value = parse_tool_json(&listed);
        assert_eq!(body["count"], 1);
        let token_id = body["tokens"][0]["token_id"].as_str().unwrap();
        assert_eq!(
            token_id,
            fleet_core::BootstrapToken::public_id_for("fbt_test123")
        );
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

    // ── Issue 도구 (로드맵 #92) ─────────────────────────────────────

    use fleet_core::PermissionKind as PK;

    async fn seed_project(ctx: &ToolContext, name: &str) -> ProjectId {
        let p = fleet_core::Project::new(name);
        ctx.state.store.create_project(&p).await.unwrap();
        p.id
    }

    async fn make_issue(ctx: &ToolContext, project_id: ProjectId, title: &str) -> String {
        let created = dispatch_tool(
            ctx,
            TOOL_CREATE_ISSUE,
            &json!({ "project_id": project_id.to_string(), "title": title }),
        )
        .await
        .unwrap();
        parse_tool_json(&created)["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn create_list_and_comment_issue_round_trip() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let project_id = seed_project(&ctx, "mcp-issues").await;

        let created = dispatch_tool(
            &ctx,
            TOOL_CREATE_ISSUE,
            &json!({
                "project_id": project_id.to_string(),
                "title": "flaky retry path",
                "severity": "high",
                "labels": ["bug"],
            }),
        )
        .await
        .unwrap();
        let body = parse_tool_json(&created);
        assert_eq!(body["title"], "flaky retry path");
        assert_eq!(body["status"], "open");
        assert_eq!(body["severity"], "high");
        assert_eq!(body["has_active_tasks"], false);
        let id = body["id"].as_str().unwrap().to_string();

        let listed = dispatch_tool(&ctx, TOOL_LIST_ISSUES, &json!({}))
            .await
            .unwrap();
        let body = parse_tool_json(&listed);
        assert_eq!(body["count"], 1);

        let commented = dispatch_tool(
            &ctx,
            TOOL_COMMENT_ISSUE,
            &json!({ "issue_id": id, "body": "looked into it" }),
        )
        .await
        .unwrap();
        assert_eq!(parse_tool_json(&commented)["body"], "looked into it");
    }

    #[tokio::test]
    async fn create_issue_rejects_unknown_project() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let result = dispatch_tool(
            &ctx,
            TOOL_CREATE_ISSUE,
            &json!({ "project_id": ProjectId::new().to_string(), "title": "orphan" }),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn transition_follows_the_state_machine() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let project_id = seed_project(&ctx, "mcp-lifecycle").await;
        let id = make_issue(&ctx, project_id, "lifecycle").await;

        // Open -> ReadyForAgent 간선은 없다(사람의 triage를 반드시 거친다).
        let refused = dispatch_tool(
            &ctx,
            TOOL_TRANSITION_ISSUE,
            &json!({ "issue_id": id, "status": "ready_for_agent" }),
        )
        .await
        .unwrap();
        assert_eq!(refused["isError"], true);

        for status in ["triaged", "ready_for_agent", "resolved"] {
            let ok = dispatch_tool(
                &ctx,
                TOOL_TRANSITION_ISSUE,
                &json!({ "issue_id": id, "status": status }),
            )
            .await
            .unwrap();
            assert_eq!(parse_tool_json(&ok)["status"], status);
        }

        // 사유 없는 close는 거절.
        let refused = dispatch_tool(
            &ctx,
            TOOL_TRANSITION_ISSUE,
            &json!({ "issue_id": id, "status": "closed" }),
        )
        .await
        .unwrap();
        assert_eq!(refused["isError"], true);

        let ok = dispatch_tool(
            &ctx,
            TOOL_TRANSITION_ISSUE,
            &json!({ "issue_id": id, "status": "closed", "close_reason": "fixed" }),
        )
        .await
        .unwrap();
        let body = parse_tool_json(&ok);
        assert_eq!(body["status"], "closed");
        assert_eq!(body["close_reason"], "fixed");
    }

    // **인자 의존 인가** — 같은 도구라도 목표 상태에 따라 다른 capability를
    // 요구한다. 서버의 capability 행렬은 도구 이름만 보므로 판정할 수 없고,
    // 핸들러가 직접 확인한다.
    #[tokio::test]
    async fn transition_capability_is_checked_per_target_status() {
        let store = fleet_store::mem::MemStore::new();
        // triage는 가능하지만 agent 승인·종결은 불가능한 capability 집합.
        let ctx = test_ctx_with_caps(store, vec![PK::IssueRead, PK::IssueCreate, PK::IssueUpdate]);
        let project_id = seed_project(&ctx, "mcp-gated").await;
        let id = make_issue(&ctx, project_id, "gated").await;

        // triaged는 issue:update로 통과.
        let ok = dispatch_tool(
            &ctx,
            TOOL_TRANSITION_ISSUE,
            &json!({ "issue_id": id, "status": "triaged" }),
        )
        .await
        .unwrap();
        assert_eq!(parse_tool_json(&ok)["status"], "triaged");

        // ready_for_agent는 issue:approve_agent_work가 없어 거절.
        let err = dispatch_tool(
            &ctx,
            TOOL_TRANSITION_ISSUE,
            &json!({ "issue_id": id, "status": "ready_for_agent" }),
        )
        .await
        .unwrap_err();
        assert!(
            err.message.contains("issue:approve_agent_work"),
            "error must name the missing capability: {}",
            err.message
        );

        // closed는 issue:close가 없어 거절 — 오탈자 수정 권한으로 문제를
        // 종결할 수 없다.
        let err = dispatch_tool(
            &ctx,
            TOOL_TRANSITION_ISSUE,
            &json!({ "issue_id": id, "status": "closed", "close_reason": "fixed" }),
        )
        .await
        .unwrap_err();
        assert!(err.message.contains("issue:close"), "{}", err.message);

        // 저장된 상태는 triaged 그대로여야 한다.
        let issue_id: IssueId = id.parse().unwrap();
        assert_eq!(
            ctx.state
                .store
                .get_issue(issue_id)
                .await
                .unwrap()
                .unwrap()
                .status,
            fleet_core::IssueStatus::Triaged
        );
    }

    #[tokio::test]
    async fn transition_and_comment_on_unknown_issue_return_tool_errors() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let ghost = IssueId::new().to_string();

        let r = dispatch_tool(
            &ctx,
            TOOL_TRANSITION_ISSUE,
            &json!({ "issue_id": ghost, "status": "triaged" }),
        )
        .await
        .unwrap();
        assert_eq!(r["isError"], true);

        let r = dispatch_tool(
            &ctx,
            TOOL_COMMENT_ISSUE,
            &json!({ "issue_id": ghost, "body": "hello" }),
        )
        .await
        .unwrap();
        assert_eq!(r["isError"], true);
    }

    /// `schema::tool_json`이 만든 `{content:[{type:"text",text:"<json>"}]}`에서
    /// 원본 JSON 값을 다시 파싱.
    fn parse_tool_json(result: &Value) -> Value {
        let text = result["content"][0]["text"].as_str().unwrap();
        serde_json::from_str(text).unwrap()
    }

    // ── fleet_create_project / fleet_list_projects / fleet_delete_project
    // (로드맵 #48, 1단계) ─────────────────────────────────────────────

    #[tokio::test]
    async fn create_list_and_delete_project_round_trip() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());

        let created = dispatch_tool(
            &ctx,
            TOOL_CREATE_PROJECT,
            &json!({"name": "acme-web", "description": "main web app"}),
        )
        .await
        .unwrap();
        let body = parse_tool_json(&created);
        assert_eq!(body["name"], "acme-web");
        assert_eq!(body["description"], "main web app");
        assert_eq!(body["status"], "active");
        let project_id = body["id"].as_str().unwrap().to_string();

        let listed = dispatch_tool(&ctx, TOOL_LIST_PROJECTS, &json!({}))
            .await
            .unwrap();
        let body = parse_tool_json(&listed);
        assert_eq!(body["count"], 1);
        assert_eq!(body["projects"][0]["id"], project_id);

        // 참조하는 Task가 없으므로 한 번의 delete 호출로 곧바로 archived까지
        // 진행된다.
        let deleted = dispatch_tool(
            &ctx,
            TOOL_DELETE_PROJECT,
            &json!({"project_id": project_id}),
        )
        .await
        .unwrap();
        let body = parse_tool_json(&deleted);
        assert_eq!(body["status"], "archived");
    }

    #[tokio::test]
    async fn create_project_rejects_empty_name() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let result = dispatch_tool(&ctx, TOOL_CREATE_PROJECT, &json!({"name": "   "})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn create_project_rejects_duplicate_name() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        dispatch_tool(&ctx, TOOL_CREATE_PROJECT, &json!({"name": "dup"}))
            .await
            .unwrap();
        let result = dispatch_tool(&ctx, TOOL_CREATE_PROJECT, &json!({"name": "dup"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_unknown_project_returns_tool_error() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let result = dispatch_tool(
            &ctx,
            TOOL_DELETE_PROJECT,
            &json!({"project_id": fleet_core::ProjectId::new().to_string()}),
        )
        .await
        .unwrap();
        assert_eq!(result["isError"], true);
    }

    #[tokio::test]
    async fn delete_project_stays_draining_while_a_task_still_references_it() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());

        let created = dispatch_tool(&ctx, TOOL_CREATE_PROJECT, &json!({"name": "busy"}))
            .await
            .unwrap();
        let project_id: ProjectId = parse_tool_json(&created)["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();

        // test_ctx에는 워커가 하나도 없어 fleet_dispatch_task를 실제로 타면
        // 즉시 Failed(terminal)로 확정된다 — "아직 실행 중"인 상황을 흉내낼
        // 수 없다. 대신 store에 Pending task를 직접 주입한다(dashboard의
        // delete_project_stays_draining_when_active_tasks_exist와 동일한
        // 접근).
        let mut task = Task::from_request(TaskRequest {
            prompt: "still running".into(),
            created_by: "test".into(),
            ..Default::default()
        });
        task.project_id = Some(project_id);
        ctx.state.store.insert_task(&task).await.unwrap();

        let deleted = dispatch_tool(
            &ctx,
            TOOL_DELETE_PROJECT,
            &json!({"project_id": project_id.to_string()}),
        )
        .await
        .unwrap();
        assert_eq!(
            parse_tool_json(&deleted)["status"],
            "draining",
            "must not archive while a non-terminal task still references the project"
        );
        assert_eq!(
            parse_tool_json(&deleted)["archive_blocked_by"],
            json!(["tasks"]),
            "MCP도 Dashboard와 같은 어휘로 사유를 실어야 한다"
        );
    }

    // ── fleet_create_agent / fleet_list_agents / fleet_stop_agent
    // (로드맵 #49, 1단계) ─────────────────────────────────────────────

    async fn create_project_for_agents(ctx: &ToolContext, name: &str) -> String {
        let created = dispatch_tool(ctx, TOOL_CREATE_PROJECT, &json!({"name": name}))
            .await
            .unwrap();
        parse_tool_json(&created)["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn create_list_and_stop_agent_round_trip() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let project_id = create_project_for_agents(&ctx, "agent-home").await;

        let created = dispatch_tool(
            &ctx,
            TOOL_CREATE_AGENT,
            &json!({"project_id": project_id, "name": "builder", "description": "builds"}),
        )
        .await
        .unwrap();
        let body = parse_tool_json(&created);
        assert_eq!(body["name"], "builder");
        assert_eq!(body["project_id"], project_id);
        assert_eq!(body["status"], "ready");
        let agent_id = body["id"].as_str().unwrap().to_string();

        let listed = dispatch_tool(&ctx, TOOL_LIST_AGENTS, &json!({"project_id": project_id}))
            .await
            .unwrap();
        let body = parse_tool_json(&listed);
        assert_eq!(body["count"], 1);
        assert_eq!(body["agents"][0]["id"], agent_id);

        let stopped = dispatch_tool(&ctx, TOOL_STOP_AGENT, &json!({"agent_id": agent_id}))
            .await
            .unwrap();
        assert_eq!(parse_tool_json(&stopped)["status"], "stopped");
    }

    #[tokio::test]
    async fn stop_agent_is_idempotent_and_does_not_move_the_reclaim_time() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let project_id = create_project_for_agents(&ctx, "idempotent").await;
        let created = dispatch_tool(
            &ctx,
            TOOL_CREATE_AGENT,
            &json!({"project_id": project_id, "name": "once"}),
        )
        .await
        .unwrap();
        let agent_id = parse_tool_json(&created)["id"]
            .as_str()
            .unwrap()
            .to_string();

        let first = parse_tool_json(
            &dispatch_tool(&ctx, TOOL_STOP_AGENT, &json!({"agent_id": agent_id}))
                .await
                .unwrap(),
        );
        let second = parse_tool_json(
            &dispatch_tool(&ctx, TOOL_STOP_AGENT, &json!({"agent_id": agent_id}))
                .await
                .unwrap(),
        );
        assert_eq!(second["status"], "stopped");
        // 재호출이 `updated_at`을 밀면 "언제 회수됐는가"라는 기록이 호출
        // 횟수만큼 뒤로 이동한다 — 그래서 이미 Stopped면 쓰지 않는다.
        assert_eq!(
            first["updated_at"], second["updated_at"],
            "재호출은 회수 시각을 갱신하지 않아야 한다"
        );
    }

    #[tokio::test]
    async fn create_agent_rejects_unknown_project() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let result = dispatch_tool(
            &ctx,
            TOOL_CREATE_AGENT,
            &json!({
                "project_id": fleet_core::ProjectId::new().to_string(),
                "name": "orphan",
            }),
        )
        .await;
        assert!(
            result.is_err(),
            "Agent의 project_id는 불변이라 생성 시점이 검증할 수 있는 유일한 순간이다"
        );
    }

    #[tokio::test]
    async fn create_agent_rejects_archived_project() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let project_id = create_project_for_agents(&ctx, "closed").await;
        dispatch_tool(
            &ctx,
            TOOL_DELETE_PROJECT,
            &json!({"project_id": project_id}),
        )
        .await
        .unwrap();

        let result = dispatch_tool(
            &ctx,
            TOOL_CREATE_AGENT,
            &json!({"project_id": project_id, "name": "too-late"}),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn create_agent_rejects_duplicate_name_within_the_project() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let a = create_project_for_agents(&ctx, "dup-a").await;
        let b = create_project_for_agents(&ctx, "dup-b").await;

        dispatch_tool(
            &ctx,
            TOOL_CREATE_AGENT,
            &json!({"project_id": a, "name": "worker"}),
        )
        .await
        .unwrap();
        let dup = dispatch_tool(
            &ctx,
            TOOL_CREATE_AGENT,
            &json!({"project_id": a, "name": "worker"}),
        )
        .await;
        assert!(dup.is_err());

        // 다른 Project에서는 같은 이름이 허용된다 — MemStore와 Postgres의
        // 유일성 범위가 같아야 두 Store가 같은 입력에 같은 답을 준다.
        dispatch_tool(
            &ctx,
            TOOL_CREATE_AGENT,
            &json!({"project_id": b, "name": "worker"}),
        )
        .await
        .expect("same name in another project must be allowed");
    }

    #[tokio::test]
    async fn stop_unknown_agent_returns_tool_error() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let result = dispatch_tool(
            &ctx,
            TOOL_STOP_AGENT,
            &json!({"agent_id": fleet_core::AgentId::new().to_string()}),
        )
        .await
        .unwrap();
        assert_eq!(result["isError"], true);
    }

    #[tokio::test]
    async fn a_ready_agent_keeps_delete_project_in_draining() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let project_id = create_project_for_agents(&ctx, "held-open").await;
        // Task는 하나도 만들지 않는다 — archive를 막는 것이 오직 Agent임을
        // 이 도구 표면에서도 증명한다.
        let created = dispatch_tool(
            &ctx,
            TOOL_CREATE_AGENT,
            &json!({"project_id": project_id, "name": "holder"}),
        )
        .await
        .unwrap();
        let agent_id = parse_tool_json(&created)["id"]
            .as_str()
            .unwrap()
            .to_string();

        let deleted = dispatch_tool(
            &ctx,
            TOOL_DELETE_PROJECT,
            &json!({"project_id": project_id}),
        )
        .await
        .unwrap();
        assert_eq!(parse_tool_json(&deleted)["status"], "draining");
        assert_eq!(
            parse_tool_json(&deleted)["archive_blocked_by"],
            json!(["agents"]),
            "Task는 하나도 없으므로 사유는 Agent여야 한다"
        );

        dispatch_tool(&ctx, TOOL_STOP_AGENT, &json!({"agent_id": agent_id}))
            .await
            .unwrap();
        let deleted = dispatch_tool(
            &ctx,
            TOOL_DELETE_PROJECT,
            &json!({"project_id": project_id}),
        )
        .await
        .unwrap();
        assert_eq!(parse_tool_json(&deleted)["status"], "archived");
    }

    #[tokio::test]
    async fn dispatch_task_rejects_unknown_project_id() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());
        let result = dispatch_tool(
            &ctx,
            TOOL_DISPATCH_TASK,
            &json!({"prompt": "test", "project_id": fleet_core::ProjectId::new().to_string()}),
        )
        .await;
        assert!(
            result.is_err(),
            "dispatching against a nonexistent project must be rejected"
        );
    }

    #[tokio::test]
    async fn dispatch_task_rejects_archived_project_id() {
        let ctx = test_ctx(fleet_store::mem::MemStore::new());

        let created = dispatch_tool(&ctx, TOOL_CREATE_PROJECT, &json!({"name": "closed-shop"}))
            .await
            .unwrap();
        let project_id = parse_tool_json(&created)["id"]
            .as_str()
            .unwrap()
            .to_string();
        dispatch_tool(
            &ctx,
            TOOL_DELETE_PROJECT,
            &json!({"project_id": project_id}),
        )
        .await
        .unwrap();

        let result = dispatch_tool(
            &ctx,
            TOOL_DISPATCH_TASK,
            &json!({"prompt": "too late", "project_id": project_id}),
        )
        .await;
        assert!(
            result.is_err(),
            "dispatching against an archived project must be rejected"
        );
    }
}
