//! JSON-RPC 2.0 + MCP 프로토콜 타입 정의.
//!
//! 모든 직렬화/역직렬화는 이 모듈을 경유합니다. 상위 계층(`server`, `handlers`)은
//! 도메인 로직에만 집중할 수 있도록 JSON-RPC 봉투(envelope) 처리를 캡슐화합니다.
//!
//! ## MCP 프로토콜 호환성
//!
//! - `initialize` — capabilities, server info, protocol version 반환
//! - `tools/list` — 도구 메타데이터 (이름, 설명, JSON Schema)
//! - `tools/call` — 도구 호출, 결과는 `{content: [{type:"text", text:...}], isError: bool}` 형태
//! - 모든 도구 이름은 `^[a-zA-Z_][a-zA-Z0-9_-]{0,63}$` 준수

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 지원하는 MCP 프로토콜 버전 (2024-11-05 사양).
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// 서버 식별자 (`initialize` 응답용).
pub const SERVER_NAME: &str = "grok-fleet-orchestrator";

/// 서버 버전 (Cargo 패키지 버전에서 자동 추출).
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ── 도구 이름 상수 ──────────────────────────────────────────────────────

/// 작업 디스패치 도구.
pub const TOOL_DISPATCH_TASK: &str = "fleet_dispatch_task";
/// 작업 상태 조회 도구.
pub const TOOL_GET_TASK_STATUS: &str = "fleet_get_task_status";
/// 워커 목록 조회 도구.
pub const TOOL_LIST_WORKERS: &str = "fleet_list_workers";
/// 태스크 목록 조회 도구.
pub const TOOL_LIST_TASKS: &str = "fleet_list_tasks";
/// 작업 취소 도구 (Phase 2).
pub const TOOL_CANCEL_TASK: &str = "fleet_cancel_task";
/// 작업 종료까지 대기 도구 (Phase 2).
pub const TOOL_WAIT_FOR_TASK: &str = "fleet_wait_for_task";
/// 작업 출력 폴링 도구 (Phase 3).
pub const TOOL_STREAM_TASK_OUTPUT: &str = "fleet_stream_task_output";
/// 여러 작업 결과 취합 도구 (Phase 3).
pub const TOOL_COLLECT_RESULTS: &str = "fleet_collect_results";
/// 호스트 인벤토리 조회 도구 (로드맵 #28).
pub const TOOL_LIST_HOSTS: &str = "fleet_list_hosts";
/// 워커 CircuitBreaker 강제 리셋 도구 (로드맵 #28).
pub const TOOL_RESET_WORKER_BREAKER: &str = "fleet_reset_worker_breaker";
/// 부트스트랩 토큰 목록 조회 도구 (로드맵 #28).
pub const TOOL_LIST_BOOTSTRAP_TOKENS: &str = "fleet_list_bootstrap_tokens";
/// 부트스트랩 토큰 폐기 도구 (로드맵 #28).
pub const TOOL_REVOKE_BOOTSTRAP_TOKEN: &str = "fleet_revoke_bootstrap_token";
/// Project 생성 도구 (로드맵 #48, 1단계).
pub const TOOL_CREATE_PROJECT: &str = "fleet_create_project";
/// Project 목록 조회 도구 (로드맵 #48, 1단계).
pub const TOOL_LIST_PROJECTS: &str = "fleet_list_projects";
/// Project archive 요청 도구 (로드맵 #48, 1단계). 영구 삭제가 아니다.
pub const TOOL_DELETE_PROJECT: &str = "fleet_delete_project";
/// Agent 생성 도구 (로드맵 #49, 1단계).
pub const TOOL_CREATE_AGENT: &str = "fleet_create_agent";
/// Agent 목록 조회 도구 (로드맵 #49, 1단계).
pub const TOOL_LIST_AGENTS: &str = "fleet_list_agents";
/// Agent 회수 도구 (로드맵 #49, 1단계).
pub const TOOL_STOP_AGENT: &str = "fleet_stop_agent";
/// Agent의 desired state를 `running`으로 올리는 도구 (로드맵 #67 4b).
///
/// `fleet_create_agent`가 이것을 대신하지 않는다 — 생성은 **정의** 조작이고,
/// 4a가 생성 시점에 자동 배정하므로 "생성 ⇒ running"으로 두면 Agent를 미리
/// 정의해 두는 정상 사용이 사라진다. `fleet_stop_agent`와 대칭이다.
pub const TOOL_START_AGENT: &str = "fleet_start_agent";
/// Agent를 Worker에 (재)배정하는 운영자 도구 (로드맵 #67 4a).
///
/// 생성 시점 배정은 자동이므로 이 도구는 **정상 경로가 아니다**. 필요한
/// 이유는 배정이 실패해도 생성은 성공하기 때문이다 — 그때 `worker_id`가
/// `NULL`인 채로 남는데, 이 도구가 없으면 그 상태가 **회복 불가능**해진다.
pub const TOOL_PLACE_AGENT: &str = "fleet_place_agent";
/// Issue 목록 조회 도구 (로드맵 #92).
pub const TOOL_LIST_ISSUES: &str = "fleet_list_issues";
/// Issue 생성 도구 (로드맵 #92).
pub const TOOL_CREATE_ISSUE: &str = "fleet_create_issue";
/// Issue 상태 전이 도구 (로드맵 #92). 목표 상태마다 요구 capability가 다르다.
pub const TOOL_TRANSITION_ISSUE: &str = "fleet_transition_issue";
/// Issue 코멘트 추가 도구 (로드맵 #92).
pub const TOOL_COMMENT_ISSUE: &str = "fleet_comment_issue";

// ═══════════════════════════════════════════════════════════════════════
//  JSON-RPC 2.0 봉투
// ═══════════════════════════════════════════════════════════════════════

/// JSON-RPC 2.0 요청 (클라이언트 → 서버).
///
/// `id`가 생략된 경우(notification) 응답을 보내지 않습니다.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    /// 요청 식별자. `null`/생략 시 notification으로 간주 (응답 없음).
    #[serde(default)]
    pub id: Value,
    /// 호출할 메서드 이름.
    pub method: String,
    /// 메서드 인자. 객체 형태를 권장하지만 사양상 임의 값 허용.
    #[serde(default)]
    pub params: Value,
}

impl JsonRpcRequest {
    /// 이 요청이 응답을 기대하는지 (id가 null이 아닌지) 반환.
    pub fn expects_response(&self) -> bool {
        // id가 null이거나 생략된 경우 notification
        !self.id.is_null()
    }
}

/// JSON-RPC 2.0 에러.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    /// JSON 파싱 실패 (-32700).
    pub fn parse_error() -> Self {
        Self {
            code: -32700,
            message: "Parse error".into(),
            data: None,
        }
    }

    /// 잘못된 요청 (-32600).
    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: msg.into(),
            data: None,
        }
    }

    /// 알 수 없는 메서드 (-32601).
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {method}"),
            data: None,
        }
    }

    /// 인자 검증 실패 (-32602).
    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
            data: None,
        }
    }

    /// 내부 에러 (-32603).
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: msg.into(),
            data: None,
        }
    }
}

/// JSON-RPC 2.0 응답 빌더.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// 성공 응답.
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    /// 에러 응답.
    pub fn error(id: Value, err: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(err),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  MCP 도구 결과 헬퍼
// ═══════════════════════════════════════════════════════════════════════

/// 텍스트 도구 결과 (`isError: false`).
pub fn tool_text(text: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": text.into() }],
        "isError": false
    })
}

/// 텍스트 도구 에러 (`isError: true`). MCP 클라이언트가 에러로 표시.
///
/// 참고: 이것은 JSON-RPC 레벨의 에러가 아니라 도구 호출의 논리적 실패입니다.
/// 클라이언트는 응답을 정상적으로 수신하지만 `isError` 플래그를 검사합니다.
pub fn tool_error(text: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": text.into() }],
        "isError": true
    })
}

/// JSON 객체를 텍스트로 직렬화한 도구 결과.
pub fn tool_json<T: Serialize>(value: &T) -> Value {
    match serde_json::to_string_pretty(value) {
        Ok(s) => tool_text(s),
        Err(e) => tool_error(format!("failed to serialize result: {e}")),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  tools/list 메타데이터
// ═══════════════════════════════════════════════════════════════════════

/// `tools/list` 응답용 도구 메타데이터 하나.
///
/// wire 필드명은 MCP `2024-11-05`의 `Tool`이 정본이라 **camelCase**다
/// (`inputSchema`). Rust 필드명(`input_schema`)을 그대로 내보내면 표준 MCP
/// 클라이언트의 `ListToolsResult` 검증에 걸려 도구가 하나도 노출되지 않는다 —
/// 서버는 응답을 보내지만 클라이언트가 그 응답을 인식하지 못해 요청이 타임아웃한다.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// 이 서버가 제공하는 전체 도구 카탈로그.
///
/// 실제로 `tools/list`에 나오는 것은 이 중 launcher의
/// `FLEET_MCP_CAPABILITIES` allow-list가 허용한 부분집합이다.
pub fn all_tools() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: TOOL_DISPATCH_TASK,
            description: "Dispatch a long-running task to a fleet worker. Returns a task_id that can be polled with fleet_get_task_status. The task runs asynchronously — completion is observed via status polling, not blocking.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "The prompt to send to the worker agent (e.g. 'cargo build --release')."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Absolute working directory on the worker where the agent session is opened. Required — the orchestrator does not know the worker's filesystem and will not invent a default. Must start with '/', must not contain '..' segments, and must not be '/' itself. Note this is validated lexically only: the orchestrator cannot confirm the path exists on the worker or that it lies inside a workspace."
                    },
                    "model": {
                        "type": "string",
                        "description": "Model slug to route this task to (optional, e.g. 'gemini'). Only workers whose 'model' label exactly matches are eligible; the task fails immediately if none are online. Omit to let the scheduler pick any available worker regardless of configured model."
                    },
                    "server_hint": {
                        "type": "string",
                        "description": "Pin this task to a specific worker by name. If the hinted worker is offline or circuit-open, the task fails (no fallback)."
                    },
                    "agent_id": {
                        "type": "string",
                        "format": "uuid",
                        "description": "Pin this task to a specific Agent by id. The task runs on the worker that Agent is placed on; the scheduler honours the pin and never picks an Agent on its own. Rejected at submission if the Agent does not exist, if 'server_hint' is also given (the two pins cannot be reconciled before the Agent is placed), or if 'project_id' is given and differs from the Agent's project. When 'project_id' is omitted it is inherited from the Agent. At dispatch the task fails (no fallback) if the Agent is stopped, failed, not yet placed, not yet observed running, or its worker is unavailable or at capacity."
                    },
                    "required_labels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Labels the worker must have (e.g. [\"gpu\"])."
                    },
                    "max_turns": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Maximum agent turns (optional)."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Per-task timeout in seconds (optional)."
                    },
                    "project_id": {
                        "type": "string",
                        "format": "uuid",
                        "description": "Project boundary for this task (optional). Stored now; policy enforcement arrives with the Project control-plane feature."
                    },
                    "idempotency_key": {
                        "type": "string",
                        "description": "Client-chosen key making this submit idempotent (optional). Resending the same key with the same payload returns the existing task instead of creating a duplicate — use it when a call may time out and be retried. Reusing the key with a different payload is rejected. Note: all MCP submissions share one key namespace per orchestrator, so pick keys that are unlikely to collide (e.g. include a UUID)."
                    }
                },
                "required": ["prompt", "cwd"]
            }),
        },
        ToolInfo {
            name: TOOL_GET_TASK_STATUS,
            description: "Look up the current status of a task by ID. Returns phase (pending/dispatched/completed/failed/cancelled), worker assignment, output (if completed), or failure details.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "UUID of the task (as returned by fleet_dispatch_task)."
                    }
                },
                "required": ["task_id"]
            }),
        },
        ToolInfo {
            name: TOOL_LIST_WORKERS,
            description: "List registered workers with their current status, labels, active task count, and circuit breaker state. Optionally filter by status or labels.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["online", "degraded", "offline", "circuit_open"],
                        "description": "Filter by worker status (optional)."
                    },
                    "labels": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Filter workers by exact label match (optional)."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 500,
                        "description": "Maximum workers to return (default 100)."
                    }
                }
            }),
        },
        ToolInfo {
            name: TOOL_LIST_TASKS,
            description: "List tasks with optional status filtering and pagination. Returns task summaries with phase, worker assignment, creation time, and (for completed tasks) output and exit code. Useful for monitoring fleet activity or finding task IDs to inspect further.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["pending", "dispatched", "completed", "failed", "cancelled", "terminal", "active"],
                        "description": "Filter by task phase (optional). 'terminal' = completed+failed+cancelled, 'active' = pending+dispatched."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 200,
                        "default": 50,
                        "description": "Maximum tasks to return (default 50)."
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 0,
                        "description": "Number of tasks to skip for pagination (default 0)."
                    }
                }
            }),
        },
        ToolInfo {
            name: TOOL_CANCEL_TASK,
            description: "Cancel a pending or in-flight task. The worker receives a cancellation signal; the task transitions to the 'cancelled' phase. Tasks already in a terminal state (completed/failed/cancelled) cannot be cancelled. Cancellation is best-effort on the worker side but the task status is updated regardless.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "UUID of the task to cancel."
                    },
                    "reason": {
                        "type": "string",
                        "description": "Optional human-readable reason recorded in the event log.",
                        "default": "cancelled by user"
                    }
                },
                "required": ["task_id"]
            }),
        },
        ToolInfo {
            name: TOOL_WAIT_FOR_TASK,
            description: "Block until the task reaches a terminal state (completed/failed/cancelled) or the timeout expires. Returns the final task snapshot. Use sparingly — long-running tasks block the MCP client. Prefer polling with fleet_get_task_status unless synchronous semantics are required.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "UUID of the task to wait for."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 3600,
                        "default": 300,
                        "description": "Maximum seconds to wait (default 300). Returns isError=true on timeout."
                    }
                },
                "required": ["task_id"]
            }),
        },
        ToolInfo {
            name: TOOL_STREAM_TASK_OUTPUT,
            description: "Poll a task's streamed output (stdout/stderr chunks) until it reaches a terminal state or the polling budget is exhausted. Concatenates all new chunks observed during the poll window and returns them along with the current task phase. Useful for tailing long-running builds/tests without repeatedly calling fleet_get_task_status.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "UUID of the task whose output should be streamed."
                    },
                    "from_offset": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 0,
                        "description": "Start reading chunks whose seq is strictly greater than this offset (default 0, i.e. from the beginning)."
                    },
                    "poll_interval_secs": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 30,
                        "default": 1,
                        "description": "Seconds between polls (default 1)."
                    },
                    "max_polls": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 600,
                        "default": 60,
                        "description": "Maximum number of polls before returning (default 60). Total wait is roughly poll_interval_secs × max_polls."
                    }
                },
                "required": ["task_id"]
            }),
        },
        ToolInfo {
            name: TOOL_COLLECT_RESULTS,
            description: "Collect the final status of multiple tasks in parallel by task_id. Returns one entry per task_id with phase, output (if completed), or error. Tasks still running at query time are reported with phase 'pending' or 'dispatched' and no output. Useful after dispatching a batch with fleet_dispatch_task.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "maxItems": 200,
                        "description": "List of task UUIDs to collect results for."
                    },
                    "include_output": {
                        "type": "boolean",
                        "default": true,
                        "description": "Include the full output string for completed tasks (default true). Set to false to get a compact phase-only summary."
                    }
                },
                "required": ["task_ids"]
            }),
        },
        ToolInfo {
            name: TOOL_LIST_HOSTS,
            description: "List the host inventory — all physical/virtual hosts fleet knows about (provisioned, online, offline, or failed), not just currently-registered workers. Useful for finding hosts that were provisioned but never joined as a worker, or diagnosing hosts whose grok/fleet-worker versions have drifted. Distinct from fleet_list_workers, which only shows hosts with an active worker registration.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["provisioned", "online", "offline", "failed"],
                        "description": "Filter by host status (optional)."
                    }
                }
            }),
        },
        ToolInfo {
            name: TOOL_RESET_WORKER_BREAKER,
            description: "Force a worker's CircuitBreaker back to Closed, discarding its recent failure history. Use after fixing whatever caused a worker to trip (e.g. a transient network partition or a bad deploy that has since been rolled back) to make it immediately eligible for dispatch again, instead of waiting out the cooldown/half-open probe cycle. The reset is persisted to the store and broadcast to other fleet serve instances sharing the same database.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "worker_id": {
                        "type": "string",
                        "description": "UUID of the worker (mutually exclusive with worker_name; provide exactly one)."
                    },
                    "worker_name": {
                        "type": "string",
                        "description": "Name of the worker (mutually exclusive with worker_id; provide exactly one)."
                    }
                }
            }),
        },
        ToolInfo {
            name: TOOL_LIST_BOOTSTRAP_TOKENS,
            description: "List all worker bootstrap (join) tokens, newest first, including usage counts and expiry. Does not reveal the raw token strings (only shown once at creation time via `fleet token create`). Useful for auditing which tokens are still active before onboarding new workers.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: TOOL_REVOKE_BOOTSTRAP_TOKEN,
            description: "Revoke a worker bootstrap (join) token immediately, preventing any further worker registrations with it. Already-joined workers are unaffected — this only blocks future use of the token. Irreversible; a new token must be created (via `fleet token create`) if joining is needed again.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "token_id": {
                        "type": "string",
                        "description": "The public token_id returned by fleet_list_bootstrap_tokens. Raw bootstrap token strings are never accepted by this tool."
                    }
                },
                "required": ["token_id"]
            }),
        },
        ToolInfo {
            name: TOOL_CREATE_PROJECT,
            description: "Create a new Project — a grouping boundary for Tasks under a development goal. Once created, pass its id as project_id to fleet_dispatch_task to scope tasks under it. This is stage-1 support: Project policy (agent slots, worker eligibility) is not enforced yet, only identity and lifecycle (active/draining/archived).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Unique display name for the project."
                    },
                    "description": {
                        "type": "string",
                        "description": "Optional free-text description."
                    }
                },
                "required": ["name"]
            }),
        },
        ToolInfo {
            name: TOOL_LIST_PROJECTS,
            description: "List Projects, newest first.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Max number of projects to return (default 100)."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Number of projects to skip (pagination)."
                    }
                }
            }),
        },
        ToolInfo {
            name: TOOL_DELETE_PROJECT,
            description: "Request that a Project be archived (not permanently deleted). Active projects transition to draining and stop accepting new tasks or agents; it finishes archiving to archived once every task that referenced the project has reached a terminal state AND every agent in it has been stopped (fleet_stop_agent). Safe to call again on the same project_id — it reports current progress rather than erroring.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "UUID of the project (as returned by fleet_create_project or fleet_list_projects)."
                    }
                },
                "required": ["project_id"]
            }),
        },
        ToolInfo {
            name: TOOL_CREATE_AGENT,
            description: "Create an Agent inside a Project — a named role with its own policy and context. The project_id is fixed at creation and can never be changed; to move work to another Project, create a new Agent there. The Agent is also placed on a worker at creation (least-loaded among online, probed workers with a closed circuit); if no worker qualifies, creation still succeeds with worker_id null and you can place it later with fleet_place_agent. There is no process behind the Agent yet, so it cannot be started, attached to, or assigned a task — its lifecycle here is ready -> stopped. Runtime/image, isolation, workspace, and tool bindings are not settable yet.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "UUID of the owning project (as returned by fleet_create_project or fleet_list_projects). Must be an active project — draining and archived projects reject new agents."
                    },
                    "name": {
                        "type": "string",
                        "description": "Display name, unique within the project (not globally)."
                    },
                    "description": {
                        "type": "string",
                        "description": "Optional free-text description of the agent's role."
                    }
                },
                "required": ["project_id", "name"]
            }),
        },
        ToolInfo {
            name: TOOL_LIST_AGENTS,
            description: "List Agents, newest first. Omit project_id to list agents across all projects. Each entry carries worker_id/assigned_at: null means the agent is not currently placed on any worker — either no worker qualified at creation, or the worker it was placed on was deregistered.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Only agents in this project (UUID). Omit for all projects."
                    },
                    "worker_id": {
                        "type": "string",
                        "description": "Only agents placed on this worker (UUID). Omit for all workers. Note this cannot select unplaced agents; read worker_id from the results for that."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max number of agents to return (default 100)."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Number of agents to skip (pagination)."
                    }
                }
            }),
        },
        ToolInfo {
            name: TOOL_STOP_AGENT,
            description: "Reclaim an Agent, moving it to stopped. A stopped Agent no longer blocks its Project from finishing archiving. Safe to call again — it reports the current state rather than erroring. In this stage there is no process to terminate, so reclaiming is immediate; once agents run for real, this will additionally require cleanup evidence before reaching stopped.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "UUID of the agent (as returned by fleet_create_agent or fleet_list_agents)."
                    }
                },
                "required": ["agent_id"]
            }),
        },
        ToolInfo {
            name: TOOL_START_AGENT,
            description: "Ask the orchestrator to run an agent: sets its desired state to running so the assigned worker picks it up on its next heartbeat. Creating an agent does not start it — creation only defines it. Idempotent: starting an already-running agent changes nothing and issues no new command. Stopped agents are rejected (stop is terminal). An agent with no worker yet is accepted; the command is delivered when it is next placed. In this stage delivery is all that is tracked — nothing reports back that a process actually came up.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "UUID of the agent (as returned by fleet_create_agent or fleet_list_agents)."
                    }
                },
                "required": ["agent_id"]
            }),
        },
        ToolInfo {
            name: TOOL_PLACE_AGENT,
            description: "Place an Agent on a worker, or move it to a different one. Only needed when creation could not place it (worker_id is null) or when you want to override the automatic choice. Omit worker_id to let the orchestrator pick the least-loaded online worker; pass it to target a specific one. Stopped agents are rejected. This records the intended location only — in this stage nothing is started or migrated as a result.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "UUID of the agent (as returned by fleet_create_agent or fleet_list_agents)."
                    },
                    "worker_id": {
                        "type": "string",
                        "description": "UUID of the target worker (as returned by fleet_list_workers). Omit to choose automatically."
                    }
                },
                "required": ["agent_id"]
            }),
        },
        ToolInfo {
            name: TOOL_LIST_ISSUES,
            description: "List issues — work items a project needs to resolve. Not infrastructure alerts (unreachable workers, missing credentials are alerts, not issues). Each entry includes has_active_tasks, a derived flag meaning a non-terminal task is linked; there is deliberately no 'in progress' status.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Only issues in this project (UUID). Omit for all projects."
                    },
                    "status": {
                        "type": "string",
                        "description": "Filter by status: open, triaged, ready_for_agent, resolved, closed."
                    },
                    "open_only": {
                        "type": "boolean",
                        "description": "Only issues that are not closed (resolved still counts as open)."
                    }
                }
            }),
        },
        ToolInfo {
            name: TOOL_CREATE_ISSUE,
            description: "Open a new issue against a project. Always starts in status 'open' — triage and agent approval are separate human transitions (see fleet_transition_issue).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "UUID of the owning project." },
                    "title": { "type": "string", "description": "Short problem statement." },
                    "body": { "type": "string", "description": "Optional detail / reproduction steps." },
                    "severity": {
                        "type": "string",
                        "description": "critical | high | medium (default) | low."
                    },
                    "labels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional labels."
                    }
                },
                "required": ["project_id", "title"]
            }),
        },
        ToolInfo {
            name: TOOL_TRANSITION_ISSUE,
            description: "Move an issue to a new status. Allowed edges follow the issue contract; promoting to ready_for_agent is the single authorization point for agent pickup and needs issue:approve_agent_work, while resolving/closing needs issue:close and reopening needs issue:reopen. Closing requires a close_reason. An edge the state machine does not allow is refused rather than silently applied.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "issue_id": { "type": "string", "description": "UUID of the issue." },
                    "status": {
                        "type": "string",
                        "description": "Target status: open, triaged, ready_for_agent, resolved, closed."
                    },
                    "close_reason": {
                        "type": "string",
                        "description": "Required when status is 'closed': fixed | wont_fix | duplicate | obsolete. Rejected for any other target status."
                    }
                },
                "required": ["issue_id", "status"]
            }),
        },
        ToolInfo {
            name: TOOL_COMMENT_ISSUE,
            description: "Append a comment to an issue thread. Comments are append-only; they never change issue status.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "issue_id": { "type": "string", "description": "UUID of the issue." },
                    "body": { "type": "string", "description": "Comment text." }
                },
                "required": ["issue_id", "body"]
            }),
        },
    ]
}

/// `initialize` 결과 객체.
pub fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_serializes_2_0() {
        let resp = JsonRpcResponse::ok(json!(1), json!({"ok": true}));
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["ok"], true);
        assert!(v.get("error").is_none());
    }

    #[test]
    fn error_response_has_code() {
        let resp = JsonRpcResponse::error(json!(2), JsonRpcError::method_not_found("foo"));
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32601);
        assert!(v["error"]["message"].as_str().unwrap().contains("foo"));
    }

    #[test]
    fn tool_text_format() {
        let v = tool_text("hello");
        assert_eq!(v["content"][0]["type"], "text");
        assert_eq!(v["content"][0]["text"], "hello");
        assert_eq!(v["isError"], false);
    }

    #[test]
    fn tool_error_format() {
        let v = tool_error("oops");
        assert_eq!(v["isError"], true);
    }

    #[test]
    fn all_tools_have_valid_names() {
        for t in all_tools() {
            // MCP 도구 이름 규칙: ^[a-zA-Z_][a-zA-Z0-9_-]{0,63}$
            let mut chars = t.name.chars();
            let first = chars.next().unwrap();
            assert!(
                first.is_ascii_alphabetic() || first == '_',
                "tool name '{}' has invalid first char",
                t.name
            );
            for c in chars {
                assert!(
                    c.is_ascii_alphanumeric() || c == '_' || c == '-',
                    "tool name '{}' has invalid char '{c}'",
                    t.name
                );
            }
            assert!(t.name.len() <= 64);
        }
    }

    #[test]
    fn initialize_result_has_protocol_version() {
        let v = initialize_result();
        assert_eq!(v["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(v["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(v["capabilities"]["tools"]["listChanged"], false);
    }

    #[test]
    fn notification_detection() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Value::Null,
            method: "notifications/initialized".into(),
            params: json!({}),
        };
        assert!(!req.expects_response());

        let req2 = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: json!(42),
            method: "tools/list".into(),
            params: json!({}),
        };
        assert!(req2.expects_response());
    }
}
