//! Phase 8.3 — `fleet-worker join` 흐름 통합 테스트.
//!
//! mock orchestrator (axum Router)로 `/v1/workers/join` 엔드포인트를 흉내내어
//! `fleet_worker::join::run_join` 의 클라이언트 로직을 검증.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{routing::post, Json, Router};
use fleet_worker::{join::run_join, JoinArgs};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct MockState {
    /// 수신된 join 요청들.
    received: Arc<Mutex<Vec<Value>>>,
    /// 다음 join 응답으로 반환할 상태 코드. 기본 200.
    next_status: Arc<Mutex<u16>>,
}

impl MockState {
    fn new() -> Self {
        Self {
            received: Arc::new(Mutex::new(Vec::new())),
            next_status: Arc::new(Mutex::new(200)),
        }
    }
}

async fn start_mock_orchestrator(state: MockState) -> String {
    let s = state.clone();
    let app = Router::new().route(
        "/v1/workers/join",
        post(move |Json(body): Json<Value>| {
            let s = s.clone();
            async move {
                s.received.lock().await.push(body.clone());
                let status_code = *s.next_status.lock().await;
                let toml_content = format!(
                    r#"# generated worker.toml
[worker]
name = "{}"
orchestrator_url = "https://fleet.example.com"
heartbeat_interval_secs = 15
bootstrap_token = "{}"
existing_worker_id = "00000000-0000-0000-0000-000000000001"

[grok]
bin = "/usr/local/bin/grok"
bind_addr = "127.0.0.1:2419"
secret = "auto-gen"
max_concurrent_tasks = 4
restart_delay_secs = 5
"#,
                    body["name"].as_str().unwrap_or("unknown"),
                    body["token"].as_str().unwrap_or(""),
                );
                let resp = json!({
                    "worker_id": "00000000-0000-0000-0000-000000000001",
                    "heartbeat_interval_secs": 15,
                    "config_revision": 1,
                    "orchestrator_version": "0.1.0",
                    "status": "online",
                    "worker_config_toml": toml_content,
                });
                (
                    axum::http::StatusCode::from_u16(status_code).unwrap(),
                    Json(resp),
                )
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn join_writes_config_to_disk() {
    let state = MockState::new();
    let url = start_mock_orchestrator(state.clone()).await;
    let tmp = TempDir::new().unwrap();
    let config_out = tmp.path().join("worker.toml");

    let args = JoinArgs {
        orchestrator_url: url,
        token: "fleet_testtok".into(),
        name: "w1".into(),
        labels: HashMap::new(),
        agent_endpoint: Some("ws://localhost:2419/ws?server-key=abc".into()),
        grok_secret: None,
        config_out: config_out.clone(),
        start: false,
        max_concurrent_tasks: None,
    };
    run_join(args).await.expect("join succeeds");

    // config 파일이 생성되었는지 확인.
    let content = std::fs::read_to_string(&config_out).unwrap();
    assert!(content.contains("name = \"w1\""));
    assert!(content.contains("bootstrap_token = \"fleet_testtok\""));
    assert!(content.contains("existing_worker_id"));

    // 요청이 정확히 도착했는지 확인.
    let received = state.received.lock().await;
    assert_eq!(received.len(), 1);
    assert_eq!(received[0]["name"], "w1");
    assert_eq!(received[0]["token"], "fleet_testtok");
    assert_eq!(
        received[0]["agent_endpoint"],
        "ws://localhost:2419/ws?server-key=abc"
    );
}

#[tokio::test]
async fn join_auto_generates_grok_secret_when_absent() {
    let state = MockState::new();
    let url = start_mock_orchestrator(state.clone()).await;
    let tmp = TempDir::new().unwrap();

    let args = JoinArgs {
        orchestrator_url: url.clone(),
        token: "tok".into(),
        name: "auto".into(),
        labels: HashMap::new(),
        agent_endpoint: None,           // 자동 생성 경로.
        grok_secret: None,              // 자동 생성.
        config_out: tmp.path().join("worker.toml"),
        start: false,
        max_concurrent_tasks: None,
    };
    run_join(args).await.unwrap();

    let received = state.received.lock().await;
    let endpoint = received[0]["agent_endpoint"].as_str().unwrap();
    // orchestrator host 기반 + server-key 포함.
    let host = url.strip_prefix("http://").unwrap();
    assert!(endpoint.starts_with(&format!("ws://{host}/ws?server-key=")));
    // secret이 32바이트 base64url → 43 chars.
    let secret_part = endpoint.strip_prefix(&format!("ws://{host}/ws?server-key=")).unwrap();
    assert_eq!(secret_part.len(), 43);
}

#[tokio::test]
async fn join_with_explicit_labels() {
    let state = MockState::new();
    let url = start_mock_orchestrator(state.clone()).await;
    let tmp = TempDir::new().unwrap();

    let mut labels = HashMap::new();
    labels.insert("arch".into(), "arm64".into());
    labels.insert("gpu".into(), "true".into());

    let args = JoinArgs {
        orchestrator_url: url,
        token: "tok".into(),
        name: "lbl".into(),
        labels,
        agent_endpoint: Some("ws://h/ws?s=k".into()),
        grok_secret: None,
        config_out: tmp.path().join("w.toml"),
        start: false,
        max_concurrent_tasks: Some(8),
    };
    run_join(args).await.unwrap();

    let received = state.received.lock().await;
    assert_eq!(received[0]["labels"]["arch"], "arm64");
    assert_eq!(received[0]["labels"]["gpu"], "true");
    assert_eq!(received[0]["max_concurrent_tasks"], 8);
}

#[tokio::test]
async fn join_fails_on_invalid_name() {
    let state = MockState::new();
    let url = start_mock_orchestrator(state).await;
    let tmp = TempDir::new().unwrap();

    let args = JoinArgs {
        orchestrator_url: url,
        token: "tok".into(),
        name: "bad name with space".into(),
        labels: HashMap::new(),
        agent_endpoint: Some("ws://h/ws".into()),
        grok_secret: None,
        config_out: tmp.path().join("w.toml"),
        start: false,
        max_concurrent_tasks: None,
    };
    let err = run_join(args).await;
    assert!(err.is_err());
    let msg = format!("{}", err.unwrap_err());
    assert!(msg.contains("name"));
}

#[tokio::test]
async fn join_fails_on_server_error() {
    let state = MockState::new();
    *state.next_status.lock().await = 401;
    let url = start_mock_orchestrator(state.clone()).await;
    let tmp = TempDir::new().unwrap();

    let args = JoinArgs {
        orchestrator_url: url,
        token: "bad".into(),
        name: "w".into(),
        labels: HashMap::new(),
        agent_endpoint: Some("ws://h/ws".into()),
        grok_secret: None,
        config_out: tmp.path().join("w.toml"),
        start: false,
        max_concurrent_tasks: None,
    };
    let err = run_join(args).await;
    assert!(err.is_err());
    let msg = format!("{}", err.unwrap_err());
    assert!(msg.contains("401"));
}

#[tokio::test]
async fn join_creates_parent_directories() {
    let state = MockState::new();
    let url = start_mock_orchestrator(state).await;
    let tmp = TempDir::new().unwrap();
    let nested = tmp.path().join("nested/sub/dir/worker.toml");

    let args = JoinArgs {
        orchestrator_url: url,
        token: "tok".into(),
        name: "w".into(),
        labels: HashMap::new(),
        agent_endpoint: Some("ws://h/ws".into()),
        grok_secret: None,
        config_out: nested.clone(),
        start: false,
        max_concurrent_tasks: None,
    };
    run_join(args).await.unwrap();
    assert!(nested.exists());
}
