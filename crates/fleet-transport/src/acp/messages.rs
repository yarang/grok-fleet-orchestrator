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

/// ACP `session/prompt`의 `prompt` 필드는 콘텐츠 블록의 평면 배열이다 —
/// `{role, content}`로 감싼 메시지 객체가 아니다. grok 자신의 공식 ACP SDK
/// 예제(`~/.grok/README.md`의 Python `create()`)가 정확히 이 형태를 쓴다:
/// `prompt = [{"type": "text", "text": m["content"]} for m in messages]`.
/// 과거엔 `Vec<AgentMessage>`(`{role:"user", content:[...]}`)로 감싸 보냈는데,
/// grok 0.2.103이 이를 `-32602 Invalid params`로 거부했다 — 배포 후 처음으로
/// end-to-end 디스패치가 실제 워커까지 도달했을 때 드러난 버그.
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
    pub prompt: Vec<ContentBlock>,
}

/// `session/prompt` 응답. 스트리밍 모드에서는 end_of_turn=true로 도착.
#[derive(Debug, Clone, Deserialize)]
pub struct PromptResult {
    /// 서버가 할당한 프롬프트 식별자.
    ///
    /// 회귀 버그: 실제 grok 응답은 이 필드를 camelCase `promptId`로 보내는데
    /// (이 파일의 다른 모든 ACP 필드 — `sessionId`, `SessionUpdate.promptId`
    /// 등 — 와 동일한 관례), rename 없이 snake_case `prompt_id`로만 받고
    /// `#[serde(default)]`가 걸려 있어 항상 조용히 `None`으로 떨어졌다. 매칭되는
    /// JSON 키가 없어도 에러 없이 그냥 기본값(None)으로 넘어가는 바람에 프로덕션
    /// 로그 어디에도 이 실패가 드러나지 않았다 — Completed 이벤트가 prompt_id
    /// 없이 나가는 근본 원인이었다.
    #[serde(default, rename = "promptId")]
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
///
/// 2026-08-11 실측 수정: 이전 구현은 이 구조를 실제 grok 페이로드와 다르게
/// 가정하고 있었고, 그 결과 `session/update` notification이 **단 한 건도
/// 파싱되지 않고 전부 드롭**되고 있었다(`failed to parse session/update
/// error=missing field 'type'`) — 스트리밍 응답 텍스트가 대시보드/CLI 어디에도
/// 안 남는 근본 원인이었다. 실제 페이로드 예:
/// ```json
/// {"sessionId": "...", "update": {
///   "_meta": {"promptId": "f4f3c069-...", ...},
///   "content": {"text": "...", "type": "text"},
///   "sessionUpdate": "agent_message_chunk"
/// }}
/// ```
/// 즉 (a) 톱레벨에 `promptId`가 없다 — `update._meta.promptId`에 **문자열**로
/// 있다. `PromptId`가 현재 `u64` 기반이라 곧바로 상호 변환할 수 없어, 이
/// 필드는 실전에서 항상 `None`으로 남는다(알려진 제약 — 실질적 라우팅은
/// `WorkerSession::sole_in_flight_task` 폴백이 담당; 완전한 지원엔 `PromptId`를
/// String 기반으로 바꾸는 더 큰 리팩터가 필요). (b) variant 태그 키는 `type`이
/// 아니라 `sessionUpdate`.
#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
pub struct SessionUpdate {
    /// 업데이트가 속한 세션.
    #[serde(default)]
    pub sessionId: Option<String>,
    /// 톱레벨에는 실전에서 존재하지 않음(위 주석 참조) — 항상 None.
    #[serde(default)]
    pub promptId: Option<u64>,
    /// 실제 업데이트 콘텐츠.
    pub update: UpdateContent,
}

/// 업데이트 variant. 알 수 없는 variant는 `Unknown`으로 보관 (raw JSON).
/// 태그 키는 `sessionUpdate` (2026-08-11 실측 — 이전엔 `type`으로 잘못
/// 가정되어 있어 모든 notification이 파싱 실패했다).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "sessionUpdate")]
pub enum UpdateContent {
    /// 에이전트 출력 청크 (스트리밍 텍스트).
    #[serde(rename = "agent_message_chunk")]
    AgentMessageChunk { content: MessageChunk },
    /// 사용자가 보낸 프롬프트의 에코 — 에이전트 출력이 아니므로 무시한다.
    #[serde(rename = "user_message_chunk")]
    UserMessageChunk { content: MessageChunk },
    /// 슬래시 커맨드 목록 광고 — 무시한다.
    #[serde(rename = "available_commands_update")]
    AvailableCommandsUpdate,
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

/// `session/prompt` 요청 빌더. 단일 텍스트 프롬프트를 콘텐츠 블록 하나로 래핑.
pub fn build_session_prompt(id: u64, session_id: &str, prompt: &str) -> RpcRequest {
    let params = SessionPromptParams {
        sessionId: session_id.to_string(),
        prompt: vec![ContentBlock::text(prompt)],
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

    /// 회귀 테스트: `session/prompt`의 `prompt` 필드는 콘텐츠 블록의 평면
    /// 배열이어야 한다(`[{"type":"text","text":"..."}]`) — `{role,content}`로
    /// 감싼 메시지 객체(구 `AgentMessage`)가 아니다. 감싸서 보내면 grok
    /// 0.2.103이 `-32602 Invalid params`로 거부한다 — 프로덕션에서 다른 3개
    /// 커넥션 버그를 전부 고친 뒤에야 처음으로 드러난 다섯 번째 버그였다.
    #[test]
    fn session_prompt_params_prompt_is_flat_content_block_array() {
        let req = build_session_prompt(7, "sess-1", "hello");
        let v = serde_json::to_value(&req).unwrap();
        let prompt = &v["params"]["prompt"];
        assert!(
            prompt.is_array(),
            "prompt must be a JSON array, got: {prompt}"
        );
        let block = &prompt[0];
        assert_eq!(
            block["type"], "text",
            "block must have a top-level 'type', not be wrapped in {{role, content}}"
        );
        assert_eq!(block["text"], "hello");
        assert!(
            block.get("role").is_none() && block.get("content").is_none(),
            "prompt entries must be raw content blocks, not {{role, content}} messages — got: {block}"
        );
        assert_eq!(v["params"]["sessionId"], "sess-1");
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
        // 태그 키는 sessionUpdate (2026-08-11 실측 — type이 아님).
        let raw = r#"{"sessionUpdate":"some_future_variant","content":{}}"#;
        let u: UpdateContent = serde_json::from_str(raw).unwrap();
        assert!(matches!(u, UpdateContent::Unknown));
    }

    /// 2026-08-11 P0 재발방지: 실제 grok가 보내는 session/update 페이로드
    /// (톱레벨에 promptId 없음, 태그는 sessionUpdate, content는
    /// {text, type} 단일 객체)가 파싱돼야 한다. 이전엔 tag="type" 가정 때문에
    /// 모든 실제 notification이 "missing field `type`"으로 파싱 실패했다.
    #[test]
    fn session_update_parses_real_grok_agent_message_chunk_payload() {
        let raw = r#"{
            "sessionId": "019ff035-0f0c-7313-95e2-da1700a12a75",
            "update": {
                "_meta": {"promptId": "f4f3c069-af0a-4e9b-b17b-1b36bf310f02"},
                "content": {"text": "2 더하기 2는 4입니다.", "type": "text"},
                "sessionUpdate": "agent_message_chunk"
            }
        }"#;
        let u: SessionUpdate = serde_json::from_str(raw).expect("must parse real grok payload");
        match u.update {
            UpdateContent::AgentMessageChunk { content } => {
                assert_eq!(
                    content.extract_text().as_deref(),
                    Some("2 더하기 2는 4입니다.")
                );
            }
            other => panic!("expected AgentMessageChunk, got {other:?}"),
        }
    }

    /// user_message_chunk(사용자 프롬프트 에코)도 에러 없이 파싱은 되어야
    /// 한다 — 호출자가 무시할 뿐, 파싱 실패로 전체 notification이 드롭되면
    /// 안 된다.
    #[test]
    fn session_update_parses_real_grok_user_message_chunk_payload() {
        let raw = r#"{
            "sessionId": "s1",
            "update": {
                "content": {"text": "질문", "type": "text"},
                "sessionUpdate": "user_message_chunk"
            }
        }"#;
        let u: SessionUpdate = serde_json::from_str(raw).expect("must parse");
        assert!(matches!(u.update, UpdateContent::UserMessageChunk { .. }));
    }
}
