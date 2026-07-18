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
/// 작업 취소 도구 (Phase 2).
pub const TOOL_CANCEL_TASK: &str = "fleet_cancel_task";
/// 작업 종료까지 대기 도구 (Phase 2).
pub const TOOL_WAIT_FOR_TASK: &str = "fleet_wait_for_task";
/// 작업 출력 폴링 도구 (Phase 3).
pub const TOOL_STREAM_TASK_OUTPUT: &str = "fleet_stream_task_output";
/// 여러 작업 결과 취합 도구 (Phase 3).
pub const TOOL_COLLECT_RESULTS: &str = "fleet_collect_results";

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
#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Phase 1에서 지원하는 도구 3개의 메타데이터.
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
                        "description": "Working directory on the worker (optional)."
                    },
                    "model": {
                        "type": "string",
                        "description": "Model slug to use (optional, e.g. 'gllm-5')."
                    },
                    "server_hint": {
                        "type": "string",
                        "description": "Pin this task to a specific worker by name. If the hinted worker is offline or circuit-open, the task fails (no fallback)."
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
                    }
                },
                "required": ["prompt"]
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
