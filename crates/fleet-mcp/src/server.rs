//! MCP stdio 서버 메인 루프.
//!
//! newline-delimited JSON-RPC 2.0을 stdin에서 읽어 핸들러로 라우팅하고,
//! 응답을 stdout에 newline-delimited로 기록합니다. MCP 사양의 핵심 메서드
//! (`initialize`, `tools/list`, `tools/call`)와 `notifications/initialized`를 지원합니다.
//!
//! ## I/O 모델
//!
//! - 단일 스레드 비동기 루프. 한 번에 하나의 요청만 처리 (MCP stdio는 동시성 요구 없음).
//! - stdin은 `tokio::io::stdin()` + `BufReader`로 라인 단위 읽기.
//! - stdout은 `tokio::io::stdout()`으로 직렬화 후 flush.
//! - 로깅은 stderr (`tracing`)로 — stdout을 오염시키지 않음.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};

use fleet_core::{required_capability_for_transition, IssueStatus, PermissionKind};
use fleet_scheduler::{Dispatcher, FleetState};

use crate::handlers::ToolContext;
use crate::schema::{
    all_tools, initialize_result, JsonRpcError, JsonRpcRequest, JsonRpcResponse, PROTOCOL_VERSION,
};

/// MCP 서버. `ToolContext`를 들고 있으며 stdio 루프를 실행.
pub struct McpServer {
    ctx: ToolContext,
    authorization: McpAuthorization,
}

/// stdio MCP launcher가 명시적으로 부여한 capability 집합.
///
/// 이는 OS process 경계 밖의 bearer assertion을 대체하지 않는다. 다만 MCP client의
/// 자연어 입력이나 tool argument가 권한을 만들어낼 수 없도록, 서버 시작 시 고정한다.
#[derive(Debug, Clone)]
pub struct McpAuthorization {
    capabilities: Vec<PermissionKind>,
}

impl McpAuthorization {
    /// `FLEET_MCP_CAPABILITIES`의 쉼표 구분 capability를 읽는다.
    /// 빈 값 또는 알 수 없는 값은 fail-closed 한다.
    pub fn from_environment() -> std::io::Result<Self> {
        let raw = std::env::var("FLEET_MCP_CAPABILITIES").map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "FLEET_MCP_CAPABILITIES is required for MCP stdio",
            )
        })?;
        let mut capabilities = Vec::new();
        for value in raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let permission = PermissionKind::all()
                .iter()
                .copied()
                .find(|permission| permission.as_str() == value)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("unknown MCP capability: {value}"),
                    )
                })?;
            if !capabilities.contains(&permission) {
                capabilities.push(permission);
            }
        }
        if capabilities.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "FLEET_MCP_CAPABILITIES must contain at least one capability",
            ));
        }
        Ok(Self { capabilities })
    }

    fn permits_tool(&self, tool: &str) -> bool {
        // 요구 capability가 인자에 따라 달라지는 도구는 행렬 하나로 판정할 수
        // 없다(로드맵 #92 — `fleet_transition_issue`는 목표 상태마다 다른
        // capability를 요구한다). 여기서는 "전이 권한을 하나라도 가졌는가"로
        // 도구 노출만 결정하고, **정확한 판정은 핸들러가 한다** — 그래서
        // 아무 전이 권한도 없으면 도구 자체가 보이지 않고, 일부만 가진
        // 호출자는 자기가 가진 전이만 실제로 수행할 수 있다.
        if tool == crate::schema::TOOL_TRANSITION_ISSUE {
            return IssueStatus::ALL.iter().any(|s| {
                self.capabilities
                    .contains(&required_capability_for_transition(*s))
            });
        }
        required_permission(tool).is_some_and(|required| self.capabilities.contains(&required))
    }
}

fn required_permission(tool: &str) -> Option<PermissionKind> {
    use crate::schema::*;
    Some(match tool {
        TOOL_DISPATCH_TASK => PermissionKind::TaskCreate,
        TOOL_GET_TASK_STATUS | TOOL_LIST_TASKS | TOOL_WAIT_FOR_TASK | TOOL_COLLECT_RESULTS => {
            PermissionKind::TaskRead
        }
        TOOL_STREAM_TASK_OUTPUT => PermissionKind::TaskOutput,
        TOOL_CANCEL_TASK => PermissionKind::TaskCancel,
        TOOL_LIST_WORKERS | TOOL_LIST_HOSTS => PermissionKind::WorkerList,
        TOOL_RESET_WORKER_BREAKER => PermissionKind::WorkerDelete,
        TOOL_LIST_BOOTSTRAP_TOKENS => PermissionKind::TokenList,
        TOOL_REVOKE_BOOTSTRAP_TOKEN => PermissionKind::TokenRevoke,
        TOOL_CREATE_PROJECT => PermissionKind::ProjectCreate,
        TOOL_LIST_PROJECTS => PermissionKind::ProjectRead,
        TOOL_DELETE_PROJECT => PermissionKind::ProjectDelete,
        TOOL_CREATE_AGENT | TOOL_STOP_AGENT | TOOL_PLACE_AGENT => PermissionKind::AgentManage,
        TOOL_LIST_AGENTS => PermissionKind::AgentRead,
        TOOL_LIST_ISSUES => PermissionKind::IssueRead,
        TOOL_CREATE_ISSUE => PermissionKind::IssueCreate,
        TOOL_COMMENT_ISSUE => PermissionKind::IssueComment,
        // `fleet_transition_issue`는 여기서 판정하지 않는다 — 요구 capability가
        // **목표 상태에 따라 다르기** 때문이다. launcher가 부여한 capability
        // 집합을 핸들러가 직접 확인한다(`handle_transition_issue`).
        _ => return None,
    })
}

impl McpServer {
    /// 서버 인스턴스 생성. FleetState와 Dispatcher는 외부에서 주입.
    pub fn new(state: Arc<FleetState>, dispatcher: Arc<Dispatcher>) -> Self {
        Self::new_with_authorization(
            state,
            dispatcher,
            McpAuthorization {
                capabilities: Vec::new(),
            },
        )
    }

    pub fn new_with_authorization(
        state: Arc<FleetState>,
        dispatcher: Arc<Dispatcher>,
        authorization: McpAuthorization,
    ) -> Self {
        Self {
            ctx: ToolContext::new(state, dispatcher)
                .with_capabilities(authorization.capabilities.clone()),
            authorization,
        }
    }

    /// stdio JSON-RPC 루프 진입. EOF 또는 치명적 I/O 에러 시 종료.
    pub async fn run(self) -> std::io::Result<()> {
        info!(
            version = env!("CARGO_PKG_VERSION"),
            protocol = PROTOCOL_VERSION,
            "MCP server starting on stdio"
        );

        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin).lines();

        // 한 줄씩 읽기. MCP 사양은 newline-delimited JSON.
        while let Some(line) = reader.next_line().await? {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // 요청 디코딩. 파싱 실패는 JSON-RPC 파스 에러로 응답 (id 없이).
            let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                Ok(req) => self.handle_request(&req).await,
                Err(e) => {
                    warn!(error = %e, "failed to parse JSON-RPC line");
                    let resp = JsonRpcResponse::error(Value::Null, JsonRpcError::parse_error());
                    Some(resp)
                }
            };

            if let Some(resp) = response {
                let json = serde_json::to_string(&resp).map_err(std::io::Error::other)?;
                stdout.write_all(json.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
                debug!(len = json.len(), "wrote response");
            }
        }

        info!("MCP server stdin closed, shutting down");
        Ok(())
    }

    /// 단일 JSON-RPC 요청 처리.
    ///
    /// 반환값이 `None`이면 응답을 보내지 않음 (notification).
    async fn handle_request(&self, req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
        // 프로토콜 버전 검증 (느슨하게 — 사양은 "2.0" 요구)
        if req.jsonrpc != "2.0" {
            if req.expects_response() {
                return Some(JsonRpcResponse::error(
                    req.id.clone(),
                    JsonRpcError::invalid_request(format!(
                        "unsupported jsonrpc version: '{}' (expected '2.0')",
                        req.jsonrpc
                    )),
                ));
            }
            return None;
        }

        let result = self.dispatch_method(req).await;

        // notification은 응답 없음
        if !req.expects_response() {
            return None;
        }

        Some(match result {
            Ok(value) => JsonRpcResponse::ok(req.id.clone(), value),
            Err(err) => JsonRpcResponse::error(req.id.clone(), err),
        })
    }

    /// 메서드별 라우팅.
    async fn dispatch_method(&self, req: &JsonRpcRequest) -> Result<Value, JsonRpcError> {
        match req.method.as_str() {
            "initialize" => Ok(initialize_result()),

            "initialized" | "notifications/initialized" => {
                debug!("client sent initialized notification");
                Ok(Value::Null)
            }

            // MCP `ListToolsResult`는 `{ "tools": [...] }` 객체다. 배열을 그대로
            // 내보내면 표준 클라이언트가 응답을 인식하지 못한다(schema.rs `ToolInfo` 참고).
            "tools/list" => {
                let tools = serde_json::to_value(
                    all_tools()
                        .into_iter()
                        .filter(|tool| self.authorization.permits_tool(tool.name))
                        .collect::<Vec<_>>(),
                )
                .map_err(|e| {
                    JsonRpcError::internal(format!("failed to serialize tool list: {e}"))
                })?;
                Ok(json!({ "tools": tools }))
            }

            "tools/call" => self.handle_tools_call(&req.params).await,

            "ping" => Ok(Value::Null),

            // 알 수 없는 메서드
            other => {
                warn!(method = other, "unknown method");
                Err(JsonRpcError::method_not_found(other))
            }
        }
    }

    /// `tools/call` 파라미터 검증 + 핸들러 호출.
    async fn handle_tools_call(&self, params: &Value) -> Result<Value, JsonRpcError> {
        let obj = params
            .as_object()
            .ok_or_else(|| JsonRpcError::invalid_params("tools/call params must be an object"))?;

        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JsonRpcError::invalid_params("missing field: name"))?;

        let arguments = obj.get("arguments").cloned().unwrap_or(Value::Null);

        // 권한 게이트보다 **존재 여부**를 먼저 판정한다. 카탈로그에 아예 없는
        // 이름까지 "권한 없음"(-32600)으로 뭉뚱그리면 호출자 입장에서 오타와
        // 권한 부족이 구분되지 않아, 어느 쪽을 고쳐야 하는지 알 수 없다.
        // 모르는 이름은 아래 `dispatch_tool`의 fallback이 `-32601`
        // (method_not_found)로 답한다.
        //
        // 존재 판정의 정본은 `all_tools()` 카탈로그다 — `required_permission()`을
        // 쓰면 안 된다. 그 함수는 `fleet_transition_issue`처럼 **존재하지만**
        // 요구 capability가 인자에 따라 달라지는 도구에도 `None`을 반환하므로,
        // `None`을 "없는 도구"로 읽는 순간 오판한다.
        //
        // 존재 여부가 드러나는 것을 감수하는 근거는 문서가 아니라 **같은 채널의
        // 관측 가능한 사실**이다. `tools/list`가 이미 이 launcher에 부여된 전체
        // 집합을 같은 비인증 stdio로 열거하므로, `-32600`이 추가로 흘리는 정보는
        // "내가 받지 못한 capability에 속한 도구가 카탈로그에 있다"뿐이다. 이를
        // 감추기로 정한다면 `tools/list`부터 바꿔야 하며, 이 분기만 뭉뚱그리는
        // 것은 은닉이 아니라 진단 정보만 잃는 선택이다.
        let known = all_tools().iter().any(|tool| tool.name == name);
        if known && !self.authorization.permits_tool(name) {
            return Err(JsonRpcError::invalid_request(
                "tool is not permitted for this MCP launcher",
            ));
        }

        crate::handlers::dispatch_tool(&self.ctx, name, &arguments).await
    }
}

/// 편의 함수: 서버를 즉시 실행.
pub async fn run_mcp_server(
    state: Arc<FleetState>,
    dispatcher: Arc<Dispatcher>,
) -> std::io::Result<()> {
    let authorization = McpAuthorization::from_environment()?;
    let server = McpServer::new_with_authorization(state, dispatcher, authorization);
    server.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_constant_present() {
        // 단순 컴파일 보장용
        assert_eq!(PROTOCOL_VERSION, "2024-11-05");
    }

    #[test]
    fn launcher_capabilities_gate_tools() {
        let authorization = McpAuthorization {
            capabilities: vec![PermissionKind::TaskRead],
        };
        assert!(authorization.permits_tool(crate::schema::TOOL_GET_TASK_STATUS));
        assert!(authorization.permits_tool(crate::schema::TOOL_LIST_TASKS));
        assert!(!authorization.permits_tool(crate::schema::TOOL_DISPATCH_TASK));
        assert!(!authorization.permits_tool(crate::schema::TOOL_REVOKE_BOOTSTRAP_TOKEN));
    }

    /// 로드맵 #67 4a — 배정은 Agent의 실행 위치를 바꾸는 쓰기이므로
    /// 읽기 권한만 가진 launcher에게는 보이지 않아야 한다. 생성·중지와 같은
    /// `AgentManage`에 둔 이유는 셋 다 "이 Agent가 어디서 도는가"를 바꾸기
    /// 때문이다 — 배정만 더 낮은 권한으로 두면 읽기 권한자가 남의 Agent를
    /// 다른 Worker로 옮길 수 있다.
    #[test]
    fn place_agent_tool_needs_agent_manage() {
        let read_only = McpAuthorization {
            capabilities: vec![PermissionKind::AgentRead, PermissionKind::TaskRead],
        };
        assert!(!read_only.permits_tool(crate::schema::TOOL_PLACE_AGENT));

        let manager = McpAuthorization {
            capabilities: vec![PermissionKind::AgentManage],
        };
        assert!(manager.permits_tool(crate::schema::TOOL_PLACE_AGENT));
        assert!(manager.permits_tool(crate::schema::TOOL_CREATE_AGENT));
        assert!(manager.permits_tool(crate::schema::TOOL_STOP_AGENT));
    }

    /// 인자 의존 도구(`fleet_transition_issue`)의 **노출** 판정 — 정확한
    /// 인가는 핸들러가 하지만, 전이 권한을 하나도 갖지 않은 launcher에게는
    /// 도구 자체가 보이지 않아야 한다(로드맵 #92).
    #[test]
    fn transition_issue_tool_is_hidden_without_any_transition_capability() {
        let none = McpAuthorization {
            capabilities: vec![PermissionKind::IssueRead, PermissionKind::IssueCreate],
        };
        assert!(
            !none.permits_tool(crate::schema::TOOL_TRANSITION_ISSUE),
            "read/create alone must not expose the transition tool"
        );

        // 전이 권한을 하나라도 가지면 도구는 보인다 — 어느 전이를 실제로
        // 수행할 수 있는지는 핸들러가 목표 상태별로 판정한다.
        for cap in [
            PermissionKind::IssueUpdate,
            PermissionKind::IssueApproveAgentWork,
            PermissionKind::IssueClose,
            PermissionKind::IssueReopen,
        ] {
            let some = McpAuthorization {
                capabilities: vec![cap],
            };
            assert!(
                some.permits_tool(crate::schema::TOOL_TRANSITION_ISSUE),
                "{} should expose the transition tool",
                cap.as_str()
            );
        }
    }

    // 더 깊은 통합 테스트는 fleet-cli/tests/에서 수행 (실제 Dispatcher + Store 필요).
    // 여기서는 라우팅 로직을 단위 테스트하기 어려움 (FleetState가 concrete Store 필요).
    // server.rs는 얇은 레이어이므로, 핸들러 테스트가 대부분의 커버리지를 제공.
    //
    // TODO(0.2.0): test_utils 크레이트를 만들어 mock Store를 공유하면
    // 서버 라우팅 단위 테스트 추가 가능.
}
