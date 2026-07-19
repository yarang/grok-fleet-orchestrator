//! JSON-RPC 2.0 메시지 직렬화 + ACP 메서드 params/results 타입.
//!
//! ACP 스펙(Zed Industries Agent Client Protocol) 기반. 전체 스펙이 아닌
//! fleet이 사용하는 최소 세트(initialize, session/new, session/prompt,
//! session/cancel, session/update)만 모델링.
//!
//! 참고: <https://github.com/Zed-Industries/agent-client-protocol>

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ============================================================================
// JSON-RPC 2.0 envelope
// ============================================================================

/// JSON-RPC 요청 (id 있음) 또는 notification (id 없음).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct RpcRequest {
    pub jsonrpc: &'static str,
    /// `None`이면 notification. `Some(id)`이면 요청.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub method: String,
    /// 메서드 파라미터 (raw JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl RpcRequest {
    /// 요청 생성 (id 있음).
    pub fn request(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id: Some(id),
            method: method.into(),
            params,
        }
    }

    /// notification 생성 (id 없음).
    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id: None,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 에러 객체.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// 서버로부터 수신한 JSON-RPC 메시지 (response 또는 notification).
#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
pub struct RpcMessage {
    pub jsonrpc: String,
    /// 요청에 대한 응답인 경우, 원래 요청의 id.
    /// notification은 이 필드가 없거나 null.
    #[serde(default)]
    pub id: Option<u64>,
    /// notification의 method.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// 성공 응답의 result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// 실패 응답의 error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    /// notification의 params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl RpcMessage {
    /// 응답인지 (id + result 또는 error).
    pub fn is_response(&self) -> bool {
        self.id.is_some() && (self.result.is_some() || self.error.is_some())
    }

    /// notification인지 (method + params, id 없음).
    pub fn is_notification(&self) -> bool {
        self.id.is_none() && self.method.is_some()
    }
}

// ============================================================================
// ACP method-specific params and results
// ============================================================================

// --- initialize ---

#[derive(Debug, Clone, Serialize)]
#[allow(non_snake_case)]
pub struct InitializeParams {
    pub protocolVersion: u32,
    pub clientCapabilities: ClientCapabilities,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ClientCapabilities {
    /// fleet은 streaming을 사용.
    pub streaming: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
pub struct InitializeResult {
    pub protocolVersion: u32,
    /// 서버가 지원하는 확장. raw JSON으로 보관 (필요 시 나중에 파싱).
    #[serde(default)]
    pub serverCapabilities: Value,
}

impl Default for InitializeParams {
    fn default() -> Self {
        Self {
            protocolVersion: 1,
            clientCapabilities: ClientCapabilities { streaming: true },
        }
    }
}

// --- session/new ---

/// `session/new` 파라미터. grok 0.2.x ACP 서버는 `cwd`와 `mcpServers`(sequence)를
/// 필수로 요구한다 (이전 ACP 스펙 revision 변경).
#[derive(Debug, Clone, Serialize)]
#[allow(non_snake_case)]
pub struct SessionNewParams {
    /// 워킹 디렉토리. 미지정 시 서버 기본값 (대부분 "/").
    pub cwd: String,
    /// MCP 서버 정의 목록. 빈 시퀀스여도 필드 자체는 반드시 보내야 함.
    #[serde(default)]
    pub mcpServers: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
pub struct SessionNewResult {
    pub sessionId: String,
    /// 서버가 보낸 시스템 지시문 (있으면).
    #[serde(default)]
    pub instructions: Option<String>,
}

// --- session/prompt ---

/// ACP 프롬프트의 user message 콘텐츠.
#[derive(Debug, Clone, Serialize)]
pub struct AgentMessage {
    pub role: &'static str,
    pub content: Vec<ContentBlock>,
}

impl AgentMessage {
    /// 단일 텍스트 프롬프트에서 user 메시지 생성.
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user",
            content: vec![ContentBlock::text(text)],
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: String,
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: "text",
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[allow(non_snake_case)]
pub struct SessionPromptParams {
    pub sessionId: String,
    pub prompt: Vec<AgentMessage>,
}

/// `session/prompt` 응답. 스트리밍 모드에서는 end_of_turn=true로 도착.
#[derive(Debug, Clone, Deserialize)]
pub struct PromptResult {
    /// 서버가 할당한 프롬프트 식별자.
    #[serde(default)]
    pub prompt_id: Option<u64>,
    /// 에이전트의 최종 메시지 (텍스트 블록들).
    #[serde(default)]
    pub agent_message: Vec<Value>,
    /// true면 턴이 종료됨 (보통 true).
    #[serde(default)]
    pub end_of_turn: bool,
    /// 토큰 사용량.
    #[serde(default)]
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

// --- session/cancel ---

#[derive(Debug, Clone, Serialize)]
#[allow(non_snake_case)]
pub struct SessionCancelParams {
    pub sessionId: String,
    pub promptId: u64,
}

// ============================================================================
// session/update notification (서버 → 클라이언트)
// ============================================================================

/// `session/update` notification의 params (raw에서 파싱).
///
/// ACP 스펙에서 update는 `update` 배열로 오거나 단일 update로 올 수 있음.
/// fleet은 핵심 variant만 처리하고 나머지는 무시.
#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
pub struct SessionUpdate {
    /// 업데이트가 속한 세션.
    #[serde(default)]
    pub sessionId: Option<String>,
    /// 업데이트가 속한 프롬프트 (초기 executing 상태에서는 없을 수 있음).
    #[serde(default)]
    pub promptId: Option<u64>,
    /// 실제 업데이트 콘텐츠.
    pub update: UpdateContent,
}

/// 업데이트 variant. 알 수 없는 variant는 `Unknown`으로 보관 (raw JSON).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum UpdateContent {
    /// 에이전트 출력 청크 (스트리밍 텍스트).
    #[serde(rename = "agent_message_chunk")]
    AgentMessageChunk { content: MessageChunk },
    /// 턴 종료.
    #[serde(rename = "end_of_turn")]
    EndOfTurn,
    /// 에러.
    #[serde(rename = "error")]
    Error { content: ErrorContent },
    /// 그 외 — 무시하지만 raw 보관.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageChunk {
    #[serde(default)]
    pub agent_message: Option<Vec<Value>>,
    /// 일부 구현은 단일 text 필드 사용.
    #[serde(default)]
    pub text: Option<String>,
}

impl MessageChunk {
    /// 청크에서 텍스트를 추출. 여러 형식을 시도.
    pub fn extract_text(&self) -> Option<String> {
        // 1. 직접 text 필드
        if let Some(t) = &self.text {
            return Some(t.clone());
        }
        // 2. agent_message 배열에서 type=text 블록들
        if let Some(blocks) = &self.agent_message {
            let mut out = String::new();
            for block in blocks {
                if let Some(obj) = block.as_object() {
                    if obj.get("type").and_then(|v| v.as_str()) == Some("text") {
                        if let Some(t) = obj.get("text").and_then(|v| v.as_str()) {
                            out.push_str(t);
                        }
                    }
                }
            }
            if !out.is_empty() {
                return Some(out);
            }
        }
        None
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorContent {
    #[serde(default)]
    pub message: String,
}

// ============================================================================
// Helper builders
// ============================================================================

/// `initialize` 요청 빌더.
pub fn build_initialize(id: u64) -> RpcRequest {
    RpcRequest::request(id, "initialize", Some(json!(InitializeParams::default())))
}

/// `session/new` 요청 빌더. `cwd`는 기본값 "/"를 사용하고 `mcpServers`는
/// 빈 시퀀스를 보낸다. grok 0.2.x ACP 서버는 두 필드 모두 필수로 요구.
pub fn build_session_new(id: u64, cwd: Option<&str>) -> RpcRequest {
    let params = SessionNewParams {
        cwd: cwd.unwrap_or("/").to_string(),
        mcpServers: Vec::new(),
    };
    RpcRequest::request(id, "session/new", Some(json!(params)))
}

/// `session/prompt` 요청 빌더. 단일 텍스트 프롬프트를 user message로 래핑.
pub fn build_session_prompt(id: u64, session_id: &str, prompt: &str) -> RpcRequest {
    let params = SessionPromptParams {
        sessionId: session_id.to_string(),
        prompt: vec![AgentMessage::user_text(prompt)],
    };
    RpcRequest::request(id, "session/prompt", Some(json!(params)))
}

/// `session/cancel` 요청 빌더.
pub fn build_session_cancel(id: u64, session_id: &str, prompt_id: u64) -> RpcRequest {
    let params = SessionCancelParams {
        sessionId: session_id.to_string(),
        promptId: prompt_id,
    };
    RpcRequest::request(id, "session/cancel", Some(json!(params)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serialization_omits_null_fields() {
        let r = RpcRequest::request(1, "initialize", Some(json!({"x": 1})));
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"id\":1"));
        assert!(s.contains("\"method\":\"initialize\""));
        assert!(s.contains("\"jsonrpc\":\"2.0\""));
    }

    #[test]
    fn notification_has_no_id() {
        let n = RpcRequest::notification("foo", None);
        let s = serde_json::to_string(&n).unwrap();
        assert!(!s.contains("\"id\""));
    }

    #[test]
    fn parse_response_with_result() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let m: RpcMessage = serde_json::from_str(raw).unwrap();
        assert!(m.is_response());
        assert!(!m.is_notification());
        assert_eq!(m.id, Some(1));
        assert!(m.result.is_some());
    }

    #[test]
    fn parse_notification() {
        let raw = r#"{"jsonrpc":"2.0","method":"session/update","params":{"x":1}}"#;
        let m: RpcMessage = serde_json::from_str(raw).unwrap();
        assert!(m.is_notification());
        assert_eq!(m.method.as_deref(), Some("session/update"));
    }

    #[test]
    fn parse_response_with_error() {
        let raw = r#"{"jsonrpc":"2.0","id":5,"error":{"code":-32600,"message":"bad"}}"#;
        let m: RpcMessage = serde_json::from_str(raw).unwrap();
        assert!(m.is_response());
        assert_eq!(m.error.unwrap().code, -32600);
    }

    #[test]
    fn message_chunk_extract_text_from_text_field() {
        let chunk = MessageChunk {
            agent_message: None,
            text: Some("hello".to_string()),
        };
        assert_eq!(chunk.extract_text().as_deref(), Some("hello"));
    }

    #[test]
    fn message_chunk_extract_text_from_agent_message() {
        let chunk = MessageChunk {
            agent_message: Some(vec![
                json!({"type": "text", "text": "foo"}),
                json!({"type": "text", "text": "bar"}),
            ]),
            text: None,
        };
        assert_eq!(chunk.extract_text().as_deref(), Some("foobar"));
    }

    #[test]
    fn update_content_unknown_variant_parses_safely() {
        // serde(other)가 Unknown으로 라우팅되는지 검증.
        let raw = r#"{"type":"some_future_variant","content":{}}"#;
        let u: UpdateContent = serde_json::from_str(raw).unwrap();
        assert!(matches!(u, UpdateContent::Unknown));
    }
}
