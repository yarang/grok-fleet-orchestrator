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

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};

use fleet_scheduler::{Dispatcher, FleetState};

use crate::handlers::ToolContext;
use crate::schema::{
    all_tools, initialize_result, JsonRpcError, JsonRpcRequest, JsonRpcResponse, PROTOCOL_VERSION,
};

/// MCP 서버. `ToolContext`를 들고 있으며 stdio 루프를 실행.
pub struct McpServer {
    ctx: ToolContext,
}

impl McpServer {
    /// 서버 인스턴스 생성. FleetState와 Dispatcher는 외부에서 주입.
    pub fn new(state: Arc<FleetState>, dispatcher: Arc<Dispatcher>) -> Self {
        Self {
            ctx: ToolContext::new(state, dispatcher),
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
                let json = serde_json::to_string(&resp)
                    .map_err(std::io::Error::other)?;
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
                    ),
                    ),
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

            "tools/list" => Ok(serde_json::to_value(all_tools()).map_err(|e| {
                JsonRpcError::internal(format!("failed to serialize tool list: {e}"))
            })?),

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
        let obj = params.as_object().ok_or_else(|| {
            JsonRpcError::invalid_params("tools/call params must be an object")
        })?;

        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JsonRpcError::invalid_params("missing field: name"))?;

        let arguments = obj.get("arguments").cloned().unwrap_or(Value::Null);

        crate::handlers::dispatch_tool(&self.ctx, name, &arguments).await
    }
}

/// 편의 함수: 서버를 즉시 실행.
pub async fn run_mcp_server(
    state: Arc<FleetState>,
    dispatcher: Arc<Dispatcher>,
) -> std::io::Result<()> {
    let server = McpServer::new(state, dispatcher);
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

    // 더 깊은 통합 테스트는 fleet-cli/tests/에서 수행 (실제 Dispatcher + Store 필요).
    // 여기서는 라우팅 로직을 단위 테스트하기 어려움 (FleetState가 concrete Store 필요).
    // server.rs는 얇은 레이어이므로, 핸들러 테스트가 대부분의 커버리지를 제공.
    //
    // TODO(0.2.0): test_utils 크레이트를 만들어 mock Store를 공유하면
    // 서버 라우팅 단위 테스트 추가 가능.
}
