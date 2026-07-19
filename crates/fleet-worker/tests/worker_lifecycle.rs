//! Worker lifecycle 통합 테스트.
//!
//! `RegistrationClient` + heartbeat 루프 + 가짜 grok TCP 리스너를 결합해서
//! 실제 fleet-worker의 정상 동작 시나리오를 in-process로 검증.
//!
//! 시나리오:
//! 1. mock orchestrator (axum) 시작 — register/heartbeat/deregister 카운터 유지.
//! 2. 가짜 grok 엔드포인트: TcpListener이 연결을 받으면 agent_healthy=true.
//! 3. WorkerConfig 빌드 → RegistrationClient 생성.
//! 4. register_with_retry() → worker_id 발급.
//! 5. run_heartbeat_loop() 백그라운드 실행.
//! 6. ≥2개의 heartbeat가 agent_healthy=true로 도착 확인.
//! 7. shutdown 신호 → 루프 종료.
//! 8. deregister() → DELETE 요청 도착 확인.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::Path,
    routing::{delete, post},
    Json, Router,
};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::{watch, Mutex as TokioMutex};

use fleet_worker::{RegistrationClient, WorkerConfig};

/// mock orchestrator가 수신한 요청을 추적하는 공유 상태.
#[derive(Clone, Default)]
struct MockOrchestratorState {
    registers: Arc<TokioMutex<Vec<Value>>>,
    heartbeats: Arc<TokioMutex<Vec<Value>>>,
    deregisters: Arc<TokioMutex<Vec<Value>>>,
}

/// mock orchestrator 시작. base URL 반환.
async fn start_mock_orchestrator(state: MockOrchestratorState) -> String {
    let reg_state = state.clone();
    let hb_state = state.clone();
    let dereg_state = state.clone();

    let app = Router::new()
        .route(
            "/v1/workers/register",
            post(move |Json(body): Json<Value>| {
                let s = reg_state.clone();
                async move {
                    s.registers.lock().await.push(body);
                    (
                        axum::http::StatusCode::OK,
                        Json(serde_json::json!({
                            "worker_id": "lifecycle-uuid-001",
                            "heartbeat_interval_secs": 1,
                            "status": "online",
                        })),
                    )
                }
            }),
        )
        .route(
            "/v1/workers/heartbeat",
            post(move |Json(body): Json<Value>| {
                let s = hb_state.clone();
                async move {
                    s.heartbeats.lock().await.push(body);
                    (axum::http::StatusCode::OK, Json(serde_json::json!({"ok": true})))
                }
            }),
        )
        .route(
            "/v1/workers/:id",
            delete(move |Path(id): Path<String>, body: Option<Json<Value>>| {
                let s = dereg_state.clone();
                async move {
                    let mut entry = serde_json::json!({"id": id});
                    if let Some(Json(b)) = body {
                        if let Some(reason) = b.get("reason") {
                            entry["reason"] = reason.clone();
                        }
                    }
                    s.deregisters.lock().await.push(entry);
                    (
                        axum::http::StatusCode::OK,
                        Json(serde_json::json!({"status": "deregistered"})),
                    )
                }
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

/// 가짜 grok 엔드포인트 — TCP 연결을 받기만 해도 health_check가 성공으로 간주.
/// 반환된 watch Sender로 true를 보내면 백그라운드 태스크가 종료됨.
async fn start_fake_grok() -> (String, watch::Sender<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                accept = listener.accept() => {
                    if let Ok((mut stream, _)) = accept {
                        // 연결만 받고 즉시 닫기 — grok health_check는 connect 성공 여부만 본다.
                        // keep stream alive briefly to avoid RST before peer reads.
                        tokio::time::timeout(Duration::from_millis(50), async {
                            use tokio::io::AsyncReadExt;
                            let mut buf = [0u8; 64];
                            let _ = stream.read(&mut buf).await;
                        }).await.ok();
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() { break; }
                }
            }
        }
    });
    (addr.to_string(), shutdown_tx)
}

#[tokio::test]
async fn full_lifecycle_register_heartbeat_deregister() {
    let state = MockOrchestratorState::default();
    let orch_url = start_mock_orchestrator(state.clone()).await;
    let (grok_addr, grok_cancel) = start_fake_grok().await;

    // config 빌드 — fake grok 주소 + mock orchestrator URL.
    let config = Arc::new(
        WorkerConfig::for_test()
            .orchestrator_url(orch_url)
            .bind_addr(&grok_addr)
            .build(),
    );

    // 1. 등록.
    let client = Arc::new(RegistrationClient::new(config).unwrap());
    let resp = client.register_with_retry().await.unwrap();
    assert_eq!(resp.worker_id, "lifecycle-uuid-001");
    assert_eq!(resp.heartbeat_interval_secs, 1);

    // 2. heartbeat 루프 시작.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let hb_client = client.clone();
    let hb_handle = tokio::spawn(async move {
        hb_client
            .run_heartbeat_loop(1, grok_addr, shutdown_rx)
            .await;
    });

    // 3. 3초 대기 → heartbeat ≥2개 도착 예상.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // heartbeat 카운트 확인 (루프 종료 전).
    {
        let hbs = state.heartbeats.lock().await;
        assert!(
            hbs.len() >= 2,
            "expected at least 2 heartbeats, got {}",
            hbs.len()
        );
        // agent_healthy이 모두 true여야 함 (fake grok이 연결을 받음).
        for (i, hb) in hbs.iter().enumerate() {
            assert_eq!(
                hb["agent_healthy"], true,
                "heartbeat #{i} should report agent_healthy=true"
            );
            assert_eq!(hb["worker_id"], "lifecycle-uuid-001");
        }
    }

    // 4. shutdown 신호.
    let _ = shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), hb_handle).await;

    // 5. deregister.
    client.deregister("test-shutdown").await;

    // 6. 최종 검증.
    let registers = state.registers.lock().await;
    assert_eq!(registers.len(), 1);
    assert_eq!(registers[0]["name"], "test-worker");
    assert!(registers[0]["agent_endpoint"]
        .as_str()
        .unwrap()
        .contains("server-key="));

    let deregisters = state.deregisters.lock().await;
    assert_eq!(deregisters.len(), 1);
    assert_eq!(deregisters[0]["id"], "lifecycle-uuid-001");
    assert_eq!(deregisters[0]["reason"], "test-shutdown");

    // fake grok 정리.
    let _ = grok_cancel.send(true);
}

#[tokio::test]
async fn heartbeat_reports_unhealthy_when_grok_down() {
    let state = MockOrchestratorState::default();
    let orch_url = start_mock_orchestrator(state.clone()).await;

    // fake grok을 띄우지 않음 — bind_addr로 아무것도 listen하지 않는 포트 사용.
    // 127.0.0.1:9는 discard 포트라 연결 시도가 확실하게 실패.
    let config = Arc::new(
        WorkerConfig::for_test()
            .orchestrator_url(orch_url)
            .bind_addr("127.0.0.1:9")
            .build(),
    );

    let client = Arc::new(RegistrationClient::new(config).unwrap());
    client.register_with_retry().await.unwrap();

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let hb_client = client.clone();
    let hb_handle = tokio::spawn(async move {
        hb_client
            .run_heartbeat_loop(1, "127.0.0.1:9".into(), shutdown_rx)
            .await;
    });

    tokio::time::sleep(Duration::from_secs(2)).await;
    let _ = shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), hb_handle).await;

    let hbs = state.heartbeats.lock().await;
    assert!(!hbs.is_empty(), "expected at least one heartbeat");
    for hb in hbs.iter() {
        assert_eq!(
            hb["agent_healthy"], false,
            "agent should be unhealthy when nothing listens on grok bind_addr"
        );
    }
}

#[tokio::test]
async fn register_includes_labels_and_max_concurrent_tasks() {
    let state = MockOrchestratorState::default();
    let orch_url = start_mock_orchestrator(state.clone()).await;
    let (grok_addr, _grok_cancel) = start_fake_grok().await;

    let mut labels = std::collections::HashMap::<String, String>::new();
    labels.insert("arch".into(), "arm64".into());
    labels.insert("gpu".into(), "true".into());

    let config = Arc::new(
        WorkerConfig::for_test()
            .orchestrator_url(orch_url)
            .bind_addr(&grok_addr)
            .max_concurrent(8)
            .label("arch", "arm64")
            .label("gpu", "true")
            .build(),
    );

    let client = RegistrationClient::new(config).unwrap();
    client.register_with_retry().await.unwrap();

    let registers = state.registers.lock().await;
    assert_eq!(registers.len(), 1);
    let label_array = registers[0]["labels"].as_array().unwrap();
    // 라벨이 모두 포함되어야 함 (순서 무관).
    let label_pairs: Vec<(String, String)> = label_array
        .iter()
        .map(|v| {
            let arr = v.as_array().unwrap();
            (
                arr[0].as_str().unwrap().to_string(),
                arr[1].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let _ = labels; // for documentation
    assert!(label_pairs.contains(&("arch".into(), "arm64".into())));
    assert!(label_pairs.contains(&("gpu".into(), "true".into())));
    assert_eq!(registers[0]["max_concurrent_tasks"], 8);
}
