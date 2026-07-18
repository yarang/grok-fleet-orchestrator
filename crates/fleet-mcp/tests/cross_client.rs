//! 크로스 클라이언트 MCP 프로토콜 호환성 테스트.
//!
//! 이 테스트는 실제 `fleet serve` 바이너리를 subprocess로 spawn하고,
//! 주요 MCP 클라이언트(grok build, Claude Code, Cursor, Gemini CLI, Codex)가
//! 전송하는 JSON-RPC 메시지 형태를 흉내 내어 보낸 뒤 응답이 사양을
//! 준수하는지 검증합니다.
//!
//! ## 의의
//!
//! Phase 2의 핵심 검증 항목 중 하나. 각 클라이언트는 initialize/initiated
//! 핸드셰이크, params 생략 여부, 문자열 vs 정수 id 등 미묘한 차이가 있습니다.
//! 이 테스트를 통해 "내 클라이언트로는 되는데" 류의 회귀를 방지합니다.
//!
//! ## 실행
//!
//! 데이터베이스가 필요하므로 DATABASE_URL이 없으면 자동 skip.
//!
//! ```bash
//! DATABASE_URL=postgres://user@host/db cargo test -p fleet-mcp --test cross_client
//! ```

use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

/// 테스트가 Postgres를 요구하므로 DATABASE_URL이 없으면 전체 skip.
fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())
}

/// subprocess에서 `fleet serve`를 spawn.
async fn spawn_server() -> Option<tokio::process::Child> {
    let _ = database_url()?;

    // workspace root를 추정 — CARGO_MANIFEST_DIR/fleet-mcp/../../target/debug/fleet
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fleet_bin = std::path::Path::new(manifest_dir)
        .join("../../target/debug/fleet")
        .canonicalize()
        .ok()?;

    if !fleet_bin.exists() {
        eprintln!("cross_client test: {fleet_bin:?} not found — run `cargo build -p fleet-cli` first");
        return None;
    }

    let child = Command::new(&fleet_bin)
        .arg("serve")
        .arg("--no-health-check") // 테스트에서는 헬스체크 노이즈 방지
        .arg("--transport")
        .arg("mock")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env("RUST_LOG", "error")
        .spawn()
        .ok()?;

    Some(child)
}

/// 한 줄을 newline로 종료하여 전송.
async fn write_line<W: AsyncWriteExt + Unpin>(
    stdin: &mut W,
    value: &Value,
) -> std::io::Result<()> {
    let mut s = serde_json::to_string(value)?;
    s.push('\n');
    stdin.write_all(s.as_bytes()).await
}

/// 한 줄 읽기 (timeout 포함).
async fn read_line<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Option<Value> {
    let mut buf = String::new();
    let n = timeout(Duration::from_secs(2), reader.read_line(&mut buf))
        .await
        .ok()?
        .ok()?;
    if n == 0 {
        return None;
    }
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

/// id별로 매칭되는 응답을 찾을 때까지 여러 줄 읽기 (notification skip 지원).
async fn read_response_for_id<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
    target_id: &Value,
) -> Option<Value> {
    for _ in 0..10 {
        let resp = read_line(reader).await?;
        if resp.get("id") == Some(target_id) {
            return Some(resp);
        }
    }
    None
}

/// initialize/tools/list/tools/call을 idempotent하게 미리 seed 워커 하나 등록.
/// 테스트를 위한 준비 단계 — DB에 직접 INSERT하는 것과 동일한 효과.
/// 여기서는 테스트 자체에서 SQL을 치는 대신, list_workers 호출만으로
/// 빈 리스트인 경우를 허용 (cross_client는 프로토콜 검증이 목적).
struct ServerFixture {
    child: tokio::process::Child,
    stdin: Option<tokio::process::ChildStdin>,
    stdout: Option<tokio::process::ChildStdout>,
}

impl ServerFixture {
    async fn start() -> Option<Self> {
        let mut child = spawn_server().await?;

        // stdin/stdout을 child에서 분리해 빌림 충돌 회피
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();

        Some(Self {
            child,
            stdin,
            stdout,
        })
    }
}

impl Drop for ServerFixture {
    fn drop(&mut self) {
        // 테스트 종료 시 자식 프로세스 kill (try_wait로 좀비 방지)
        let _ = self.child.start_kill();
    }
}

// ─────────────────────────────────────────────────────────────────────
//  클라이언트별 initialize 시퀀스
// ─────────────────────────────────────────────────────────────────────

/// Claude Code / Cursor / Gemini CLI / Codex는 표준 MCP initialize 시퀀스를 사용.
/// id는 보통 정수 (1) 이며 params는 capabilities 객체.
async fn standard_initialize<W: AsyncWriteExt + Unpin, R: AsyncBufReadExt + Unpin>(
    stdin: &mut W,
    stdout: &mut R,
    id: Value,
) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        }
    });
    write_line(stdin, &req).await.unwrap();

    let resp = read_response_for_id(stdout, &id).await.expect("no initialize response");

    // initialized notification 전송 (MCP 사양)
    let notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    write_line(stdin, &notif).await.unwrap();

    resp
}

// ─────────────────────────────────────────────────────────────────────
//  테스트
// ─────────────────────────────────────────────────────────────────────

/// initialize 응답이 MCP 사양의 필수 필드를 모두 포함하는지 검증.
#[tokio::test]
async fn claude_code_initialize_handshake() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping: DATABASE_URL not set or binary not built");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let resp = standard_initialize(&mut stdin, &mut reader, json!(1)).await;

    assert_eq!(resp["jsonrpc"], "2.0", "jsonrpc field must be 2.0");
    assert_eq!(resp["id"], 1);
    assert!(
        resp.get("result").is_some(),
        "must have result field, got: {resp}"
    );
    assert!(
        resp.get("error").is_none(),
        "must not have error field"
    );

    let result = &resp["result"];
    // MCP 사양 필수 필드
    assert!(result.get("protocolVersion").is_some(), "missing protocolVersion");
    assert!(result.get("capabilities").is_some(), "missing capabilities");
    assert!(result.get("serverInfo").is_some(), "missing serverInfo");
    assert!(
        result["serverInfo"].get("name").is_some(),
        "missing serverInfo.name"
    );
    assert!(
        result["serverInfo"].get("version").is_some(),
        "missing serverInfo.version"
    );
}

/// Cursor는 id로 문자열을 사용하는 경우가 있음 — 호환성 확인.
#[tokio::test]
async fn cursor_string_id_initialize() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let resp = standard_initialize(&mut stdin, &mut reader, json!("init-1")).await;
    assert_eq!(resp["id"], "init-1", "string id must round-trip");
    assert!(resp["result"]["protocolVersion"].is_string());
}

/// tools/list 응답 형태 검증 — 사양 요구사항:
/// - result는 배열이어야 함 (키 이름 `tools` 아님 — 사양은 result 자체가 배열)
/// - 각 도구는 name, description, inputSchema 필드 포함
#[tokio::test]
async fn gemini_cli_tools_list_shape() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    standard_initialize(&mut stdin, &mut reader, json!(1)).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });
    write_line(&mut stdin, &req).await.unwrap();

    let resp = read_response_for_id(&mut reader, &json!(2)).await.expect("no tools/list response");
    let tools = resp["result"].as_array().expect("tools/list result must be array");

    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"fleet_dispatch_task"), "names: {names:?}");
    assert!(names.contains(&"fleet_get_task_status"));
    assert!(names.contains(&"fleet_list_workers"));
    assert!(names.contains(&"fleet_cancel_task"));
    assert!(names.contains(&"fleet_wait_for_task"));

    // 각 도구의 스키마 검증
    for t in tools {
        assert!(t["name"].is_string(), "tool name missing");
        assert!(t["description"].is_string(), "tool description missing");
        assert!(t["input_schema"].is_object(), "input_schema must be object");
        assert_eq!(t["input_schema"]["type"], "object");
    }
}

/// grok build 스타일: tools/call에서 빈 arguments (params로 객체만).
#[tokio::test]
async fn grok_build_tools_call_no_arguments() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    standard_initialize(&mut stdin, &mut reader, json!(1)).await;

    // arguments 누락 — list_workers는 인자 없이 동작해야 함.
    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "fleet_list_workers"
        }
    });
    write_line(&mut stdin, &req).await.unwrap();
    let resp = read_response_for_id(&mut reader, &json!(2)).await.expect("no response");

    assert_eq!(resp["jsonrpc"], "2.0");
    assert!(resp.get("result").is_some(), "expected result, got: {resp}");
    let result = &resp["result"];
    // MCP tools/call 응답: { content: [{type:"text", text:...}], isError }
    assert!(result["content"].is_array(), "content must be array");
    assert!(result["content"][0]["type"] == "text");
    assert!(result["content"][0]["text"].is_string());
    assert!(result["isError"].is_boolean(), "isError must be boolean");
    // 빈 워커 리스트여도 isError=false
    assert_eq!(result["isError"], false);
}

/// Codex 스타일: tools/call로 fleet_dispatch_task 호출 시 prompt만 최소 제공.
#[tokio::test]
async fn codex_dispatch_task_minimal_prompt() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    standard_initialize(&mut stdin, &mut reader, json!(1)).await;

    // prompt만 보내고 dispatch 시도 — 등록된 워커가 없으면
    // isError=true + NoWorker 에러 메시지가 나와야 함 (정상 동작).
    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "fleet_dispatch_task",
            "arguments": { "prompt": "test prompt" }
        }
    });
    write_line(&mut stdin, &req).await.unwrap();
    let resp = read_response_for_id(&mut reader, &json!(2)).await.expect("no response");

    let result = &resp["result"];
    assert!(result["content"].is_array());
    assert!(result["isError"].is_boolean());
    // 워커가 없으면 isError=true여야 함 (NoWorker).
    // 단, 워커가 미리 DB에 있다면 false. 두 경우 모두 허용.
}

/// 알 수 없는 도구 이름 — JSON-RPC method_not_found 에러.
#[tokio::test]
async fn unknown_tool_returns_error() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    standard_initialize(&mut stdin, &mut reader, json!(1)).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "nonexistent_tool",
            "arguments": {}
        }
    });
    write_line(&mut stdin, &req).await.unwrap();
    let resp = read_response_for_id(&mut reader, &json!(2)).await.expect("no response");

    assert_eq!(resp["jsonrpc"], "2.0");
    let err = resp.get("error").expect("unknown tool must return error");
    assert_eq!(err["code"], -32601, "method_not_found code");
    assert!(err["message"].as_str().unwrap().contains("nonexistent_tool"));
}

/// 잘못된 JSON-RPC 버전 — invalid_request 에러.
#[tokio::test]
async fn wrong_jsonrpc_version_rejected() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let req = json!({
        "jsonrpc": "1.0",
        "id": 1,
        "method": "initialize"
    });
    write_line(&mut stdin, &req).await.unwrap();
    let resp = read_response_for_id(&mut reader, &json!(1)).await.expect("no response");

    let err = resp.get("error").expect("must reject version 1.0");
    assert_eq!(err["code"], -32600, "invalid_request code");
}

/// 빈 prompt — invalid_params 에러 (도구별 validation).
#[tokio::test]
async fn empty_prompt_rejected() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    standard_initialize(&mut stdin, &mut reader, json!(1)).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "fleet_dispatch_task",
            "arguments": { "prompt": "   " }
        }
    });
    write_line(&mut stdin, &req).await.unwrap();
    let resp = read_response_for_id(&mut reader, &json!(2)).await.expect("no response");

    let err = resp.get("error").expect("empty prompt must be rejected");
    assert_eq!(err["code"], -32602, "invalid_params code");
}

/// notification (id 없음) — 응답 없음 검증.
#[tokio::test]
async fn notifications_have_no_response() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // initialized notification (id 없음) — 응답이 오면 안 됨
    let notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    write_line(&mut stdin, &notif).await.unwrap();

    // 300ms 동안 응답이 오지 않아야 함
    let result = timeout(Duration::from_millis(300), reader.read_line(&mut String::new())).await;
    assert!(
        result.is_err() || result.unwrap().unwrap() == 0,
        "notifications must not produce a response"
    );
}

/// protocolVersion이 사양의 버전 문자열과 정확히 일치.
#[tokio::test]
async fn protocol_version_matches_spec() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let resp = standard_initialize(&mut stdin, &mut reader, json!(1)).await;
    let pv = resp["result"]["protocolVersion"].as_str().unwrap();
    assert_eq!(pv, "2024-11-05", "protocol version must match spec");
}

/// ping 메서드 — Claude Code는 주기적으로 ping을 보냄.
#[tokio::test]
async fn ping_returns_null_result() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    standard_initialize(&mut stdin, &mut reader, json!(1)).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "ping"
    });
    write_line(&mut stdin, &req).await.unwrap();
    let resp = read_response_for_id(&mut reader, &json!(2)).await.expect("no ping response");
    assert!(resp.get("result").is_some());
}

// ─────────────────────────────────────────────────────────────────────
//  잘못된 메시지에 대한 복원력
// ─────────────────────────────────────────────────────────────────────

/// 빈 줄 전송 — 서버가 무시하고 종료되지 않아야 함.
#[tokio::test]
async fn blank_lines_ignored() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // 빈 줄들
    stdin.write_all(b"\n\n  \n").await.unwrap();

    // 그 후 정상 요청이 동작해야 함
    let resp = standard_initialize(&mut stdin, &mut reader, json!(1)).await;
    assert!(resp["result"].is_object());
}

/// 깨진 JSON — parse_error 응답.
#[tokio::test]
async fn malformed_json_returns_parse_error() {
    let Some(mut fx) = ServerFixture::start().await else {
        eprintln!("skipping");
        return;
    };
    let mut stdin = fx.stdin.take().unwrap();
    let stdout = fx.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    stdin.write_all(b"{ this is not json\n").await.unwrap();

    // parse_error 응답이 와야 함 (id는 null)
    let resp = read_line(&mut reader).await.expect("no response for bad json");
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], Value::Null);
    let err = resp.get("error").expect("must have error");
    assert_eq!(err["code"], -32700, "parse_error code");
}
