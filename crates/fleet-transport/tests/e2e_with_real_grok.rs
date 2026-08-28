//! End-to-end 테스트 — 실제 `grok agent serve` 프로세스와의 통신.
//!
//! 이 테스트는 `cargo test -- --ignored e2e_with_real_grok` 로만 실행.
//! CI에서는 `GROK_BIN` 환경변수가 설정된 경우에만 실행.
//!
//! ## 준비
//!
//! 1. `grok` CLI가 PATH에 있거나 `GROK_BIN`으로 경로 지정.
//! 2. 로컬 Postgres 인스턴스 필요 (DATABASE_URL 환경변수).
//! 3. ephemeral port (기본 2419) 가 사용 가능해야 함.
//!
//! ## 검증 항목
//!
//! - AcpClient가 `ws://127.0.0.1:2419/ws`에 연결되는가.
//! - `initialize` + `session/new` 가 정상 응답하는가.
//! - `session/prompt` 로 "hello" 보냈을 때 응답이 도착하는가.

#![cfg(feature = "acp")]

use std::process::Stdio;
use std::time::Duration;

use fleet_core::{TaskId, WorkerId};
use fleet_transport::{AcpTransport, WorkerEvent, WorkerTransport};
use tokio::process::Command;
use tokio::time::timeout;

/// `GROK_BIN`이 설정된 경우에만 실행.
fn grok_bin() -> Option<String> {
    std::env::var("GROK_BIN").ok()
}

/// `grok agent serve`를 백그라운드에서 시작.
async fn spawn_grok_agent(port: u16) -> Option<tokio::process::Child> {
    let bin = grok_bin()?;
    let secret = "test-secret-fleet";

    let child = Command::new(&bin)
        .arg("agent")
        .arg("serve")
        .arg("--bind")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--secret")
        .arg(secret)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;

    tracing::info!(port, "spawned grok agent serve, waiting for it to start");
    // 서버가 listen할 때까지 대기 (10초).
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .is_ok()
        {
            return Some(child);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tracing::error!("grok agent serve did not start listening within 10s");
    None
}

#[tokio::test]
#[ignore = "requires real grok agent (set GROK_BIN env var)"]
async fn e2e_dispatch_to_real_grok() {
    let _ = tracing_subscriber::fmt::try_init();

    let port: u16 = 12419;
    let endpoint = format!("ws://127.0.0.1:{port}/ws?server-key=test-secret-fleet");

    let _grok_guard = match spawn_grok_agent(port).await {
        Some(c) => c,
        None => {
            eprintln!("SKIP: GROK_BIN not set or spawn failed");
            return;
        }
    };

    // AcpTransport로 워커 등록.
    let transport = std::sync::Arc::new(AcpTransport::new());
    let mut events = transport.subscribe().await.expect("subscribe");

    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint, 4)
        .await
        .expect("register real worker");

    // 간단한 프롬프트 dispatch.
    let task_id = TaskId::new();
    let req = fleet_transport::DispatchRequest {
        task_id,
        worker_id: worker,
        prompt: "Say exactly: hello from grok".to_string(),
        // 로드맵 #69 — dispatch 게이트가 절대 경로 cwd를 요구한다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        model: None,
        max_turns: Some(1),
        timeout_secs: Some(60),
        checkpoint_branch: None,
        skills_required: vec![],
    };
    transport.dispatch(req).await.expect("dispatch");

    // Completed 이벤트 대기 (60초 타임아웃).
    let result = timeout(Duration::from_secs(60), async {
        loop {
            match events.recv().await {
                Some(WorkerEvent::Completed { task_id: t, result }) if t == task_id => {
                    return Ok(result);
                }
                Some(WorkerEvent::Failed { error, .. }) => return Err(error),
                Some(_) => continue,
                None => return Err("event stream closed".to_string()),
            }
        }
    })
    .await;

    match result {
        Ok(Ok(task_result)) => {
            println!("grok response: {}", task_result.output);
            assert!(
                !task_result.output.is_empty(),
                "grok should produce some output"
            );
        }
        Ok(Err(e)) => panic!("grok task failed: {e}"),
        Err(_) => panic!("timed out waiting for grok response"),
    }

    // 정리 — transport unregister, grok은 kill_on_drop으로 자동 종료.
    let _ = transport.unregister(worker).await;
}

#[tokio::test]
#[ignore = "requires real grok agent (set GROK_BIN env var)"]
async fn e2e_ping_real_worker() {
    let port: u16 = 12420;
    let endpoint = format!("ws://127.0.0.1:{port}/ws?server-key=test-secret-fleet");

    let _grok_guard = match spawn_grok_agent(port).await {
        Some(c) => c,
        None => {
            eprintln!("SKIP: GROK_BIN not set or spawn failed");
            return;
        }
    };

    let transport = std::sync::Arc::new(AcpTransport::new());
    let worker = WorkerId::new();
    transport
        .register(worker, &endpoint, 4)
        .await
        .expect("register");

    // ping (실제로는 is_connected로 갈음하지만 호출은 검증).
    let _ = transport.ping(worker).await.expect("ping");
    assert!(transport.is_connected(worker).await);

    let _ = transport.unregister(worker).await;
}

/// 로드맵 `#94` 게이트 — secret이 **URL 밖으로** 나가고도 실제 grok에
/// 인증되는가.
///
/// 이 테스트가 `#94`의 완료 조건이다. 단위 테스트(`ws_auth_parts`)는 URL에서
/// secret이 제거된다는 것만 증명하고, raw socket probe는 grok이 헤더를
/// 받아들인다는 것만 증명한다 — **fleet 자신의 다이얼이 그 조합으로 붙는지**는
/// 둘 중 어느 것도 증명하지 못한다. 여기서 그것을 증명한다.
///
/// 양성/음성 한 쌍으로 구성한 이유: 양성만 있으면 "grok이 애초에 인증을
/// 강제하지 않아서 붙은 것"과 구분되지 않는다. 같은 프로세스에 secret 없이
/// 붙는 것이 실패해야만, 양성의 성공을 **헤더 덕분**이라고 말할 수 있다.
#[tokio::test]
#[ignore = "requires real grok agent (set GROK_BIN env var)"]
async fn e2e_94_secret_travels_in_header_not_url() {
    let _ = tracing_subscriber::fmt::try_init();

    let port: u16 = 12421;
    let _grok_guard = match spawn_grok_agent(port).await {
        Some(c) => c,
        None => {
            eprintln!("SKIP: GROK_BIN not set or spawn failed");
            return;
        }
    };

    // ── 음성 대조: secret이 아예 없으면 붙지 못해야 한다. ──────────────
    // 이것이 실패하면(=붙어 버리면) 이 grok은 인증을 강제하지 않는 것이고,
    // 아래 양성 결과는 아무것도 증명하지 못한다.
    let no_auth = format!("ws://127.0.0.1:{port}/ws");
    let transport = std::sync::Arc::new(AcpTransport::new());
    let unauthenticated = WorkerId::new();
    let negative = transport.register(unauthenticated, &no_auth, 4).await;
    assert!(
        negative.is_err(),
        "grok이 인증 없는 /ws를 받아들였다 — 이 실행에서는 아래 양성 결과가 \
         헤더 인증의 증거가 되지 못한다"
    );
    let _ = transport.unregister(unauthenticated).await;

    // ── 양성: 저장 형태는 그대로 `?server-key=`지만 다이얼은 헤더로. ────
    // `AcpAuthMode::Header`(기본값)에서 `ws_auth_parts`가 URL의 secret을
    // 떼어 `Authorization: Bearer …`로 옮긴다. grok이 101을 돌려주면
    // 헤더가 실제로 인증에 쓰였다는 뜻이다.
    let stored = format!("ws://127.0.0.1:{port}/ws?server-key=test-secret-fleet");
    let worker = WorkerId::new();
    transport
        .register(worker, &stored, 4)
        .await
        .expect("헤더로 옮긴 secret으로 실제 grok에 연결되어야 한다 (#94)");
    assert!(
        transport.is_connected(worker).await,
        "register 성공 후에는 연결이 살아 있어야 한다"
    );

    let _ = transport.unregister(worker).await;
}
