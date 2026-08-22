//! FLEET PROTOTYPE (2026-08-11): 공식 SDK로 실제 프로덕션 워커(`grok agent serve`)에
//! WebSocket 연결 → session/new → session/prompt → 스트리밍 수신까지 되는지 검증하는
//! 일회성 프로토타입. Fleet 마이그레이션 결정을 위한 것으로, 본 이식 완료 후 제거될 수 있다.
//!
//! 실행:
//! ```bash
//! FLEET_WORKER_WS_URL='wss://fleet.agentthread.dev/ws/worker-ec1?server-key=...' \
//!   cargo run --example fleet_prototype --features client
//! ```

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, SessionNotification,
    TextContent,
};
use agent_client_protocol::{Agent, Client, ConnectionTo};
use agent_client_protocol_http::HttpClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Fleet의 fleet-cli main()과 동일 — rustls는 프로세스 단위 CryptoProvider가
    // 필요하다(자동 감지 안 됨, 명시적 설치 필수).
    let _install_result = rustls::crypto::ring::default_provider().install_default();

    let url = std::env::var("FLEET_WORKER_WS_URL")
        .expect("set FLEET_WORKER_WS_URL to a real wss://.../ws/<worker>?server-key=... endpoint");

    eprintln!("🔌 connecting to {url}");
    let ws_client = HttpClient::with_endpoint(&url)?;

    Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                println!("[update] {:?}", notification.update);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(ws_client, |connection: ConnectionTo<Agent>| async move {
            eprintln!("🤝 initialize...");
            let init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            eprintln!("✓ initialized: {:?}", init.agent_info);

            eprintln!("📝 session/new...");
            let session = connection
                .send_request(NewSessionRequest::new(std::path::PathBuf::from("/tmp")))
                .block_task()
                .await?;
            eprintln!("✓ session: {:?}", session.session_id);

            eprintln!("💬 session/prompt...");
            let result = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(
                        "1 더하기 1은 얼마인가? 숫자만 답해줘.".to_string(),
                    ))],
                ))
                .block_task()
                .await?;
            eprintln!("✅ done. stop_reason={:?}", result.stop_reason);

            Ok(())
        })
        .await?;

    Ok(())
}
