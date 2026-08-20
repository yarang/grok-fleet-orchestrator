//! HTTP API 통합 테스트.
//!
//! `register → heartbeat → list → get → deregister` 흐름을 실제 TCP 리스너와
//! reqwest HTTP 클라이언트로 end-to-end 검증합니다. Postgres 없이 인메모리
//! Store 구현체를 사용합니다.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::json;
use tokio::task::JoinHandle;

use fleet_api::AppState;
use fleet_store::Store;

// ── 인메모리 Store (테스트 픽스처) ──────────────────────────────────────

use fleet_store::mem::MemStore;

// ── 테스트 헬퍼 ─────────────────────────────────────────────────────────

struct Server {
    addr: SocketAddr,
    _handle: JoinHandle<()>,
}

async fn spawn_server() -> Server {
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    let state = Arc::new(AppState::new(store));
    // ephemeral port
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = fleet_api::build_app(state);
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Server {
        addr,
        _handle: handle,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().build().expect("reqwest client")
}

// ── 테스트 ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let srv = spawn_server().await;
    let resp = client()
        .get(format!("http://{}/v1/health", srv.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());
}

#[tokio::test]
async fn register_then_list_shows_worker() {
    let srv = spawn_server().await;

    // register
    let mut labels = HashMap::new();
    labels.insert("arch".to_string(), "arm64".to_string());
    let resp = client()
        .post(format!("http://{}/v1/workers/register", srv.addr))
        .json(&json!({
            "name": "build-01",
            "agent_endpoint": "wss://10.0.1.10:2419/ws",
            "labels": labels,
            "max_concurrent_tasks": 4,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "register should succeed");
    let reg: serde_json::Value = resp.json().await.unwrap();
    let _worker_id = reg["worker_id"].as_str().unwrap().to_string();
    assert_eq!(reg["status"], "online");
    assert_eq!(reg["heartbeat_interval_secs"], 15);

    // list
    let resp = client()
        .get(format!("http://{}/v1/workers", srv.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let workers: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0]["name"], "build-01");
    assert_eq!(workers[0]["labels"]["arch"], "arm64");
    assert_eq!(workers[0]["status"], "online");
    // 로드맵 #61 1단계 — liveness_mode 생략 시 기존 클라이언트와 동일하게
    // periodic으로 취급되어야 한다.
    assert_eq!(workers[0]["liveness_mode"], "periodic");
}

/// 로드맵 #61 1단계 — 신규 워커가 `liveness_mode: "on_demand"`로 등록을
/// 요청하면 그 값이 그대로 저장/조회된다. (실제 on-demand dispatch 동작은
/// 이 증분의 범위 밖 — 스키마 배관만 검증.)
#[tokio::test]
async fn register_with_on_demand_liveness_mode_is_persisted() {
    let srv = spawn_server().await;

    let resp = client()
        .post(format!("http://{}/v1/workers/register", srv.addr))
        .json(&json!({
            "name": "on-demand-01",
            "agent_endpoint": "wss://10.0.1.20:2419/ws",
            "liveness_mode": "on_demand",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "register should succeed");

    let resp = client()
        .get(format!("http://{}/v1/workers", srv.addr))
        .send()
        .await
        .unwrap();
    let workers: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0]["liveness_mode"], "on_demand");
}

#[tokio::test]
async fn heartbeat_updates_active_tasks() {
    let srv = spawn_server().await;

    // register
    let reg: serde_json::Value = client()
        .post(format!("http://{}/v1/workers/register", srv.addr))
        .json(&json!({
            "name": "runner-99",
            "agent_endpoint": "wss://10.0.1.99:2419/ws",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let worker_id = reg["worker_id"].as_str().unwrap().to_string();

    // heartbeat
    let resp = client()
        .post(format!("http://{}/v1/workers/heartbeat", srv.addr))
        .json(&json!({
            "worker_id": worker_id,
            "active_tasks": 3,
            "agent_healthy": true,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let hb: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(hb["ok"], true);

    // verify via get
    let resp = client()
        .get(format!("http://{}/v1/workers/{}", srv.addr, worker_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let w: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(w["active_tasks"], 3);
    assert!(w["last_seen"].is_string());
}

#[tokio::test]
async fn heartbeat_unknown_worker_returns_not_found() {
    let srv = spawn_server().await;
    let bogus_id = uuid::Uuid::new_v4().to_string();
    let resp = client()
        .post(format!("http://{}/v1/workers/heartbeat", srv.addr))
        .json(&json!({
            "worker_id": bogus_id,
            "active_tasks": 0,
            "agent_healthy": true,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("worker"));
}

#[tokio::test]
async fn heartbeat_unhealthy_promotes_to_degraded() {
    let srv = spawn_server().await;

    let reg: serde_json::Value = client()
        .post(format!("http://{}/v1/workers/register", srv.addr))
        .json(&json!({
            "name": "flaky-01",
            "agent_endpoint": "wss://10.0.2.1:2419/ws",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let worker_id = reg["worker_id"].as_str().unwrap().to_string();

    // unhealthy heartbeat
    let resp = client()
        .post(format!("http://{}/v1/workers/heartbeat", srv.addr))
        .json(&json!({
            "worker_id": worker_id,
            "active_tasks": 0,
            "agent_healthy": false,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // verify status changed
    let resp = client()
        .get(format!("http://{}/v1/workers/{}", srv.addr, worker_id))
        .send()
        .await
        .unwrap();
    let w: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(w["status"], "degraded");
}

#[tokio::test]
async fn deregister_removes_worker() {
    let srv = spawn_server().await;

    let reg: serde_json::Value = client()
        .post(format!("http://{}/v1/workers/register", srv.addr))
        .json(&json!({
            "name": "ephemeral-01",
            "agent_endpoint": "wss://10.0.3.1:2419/ws",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let worker_id = reg["worker_id"].as_str().unwrap().to_string();

    // delete
    let resp = client()
        .delete(format!("http://{}/v1/workers/{}", srv.addr, worker_id))
        .json(&json!({"reason": "scaling down"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "deregistered");

    // subsequent get → 404
    let resp = client()
        .get(format!("http://{}/v1/workers/{}", srv.addr, worker_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn reregister_same_name_keeps_worker_id() {
    let srv = spawn_server().await;

    let first: serde_json::Value = client()
        .post(format!("http://{}/v1/workers/register", srv.addr))
        .json(&json!({
            "name": "stable-01",
            "agent_endpoint": "wss://10.0.4.1:2419/ws",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let first_id = first["worker_id"].as_str().unwrap().to_string();

    // re-register same name with existing_worker_id
    let second: serde_json::Value = client()
        .post(format!("http://{}/v1/workers/register", srv.addr))
        .json(&json!({
            "name": "stable-01",
            "agent_endpoint": "wss://10.0.4.1:2419/ws",
            "existing_worker_id": first_id,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(second["worker_id"], first_id);
}

#[tokio::test]
async fn list_filter_by_status() {
    let srv = spawn_server().await;

    // 2 workers: one healthy, one degraded
    for (name, healthy) in &[("ok-01", true), ("bad-01", false)] {
        let reg: serde_json::Value = client()
            .post(format!("http://{}/v1/workers/register", srv.addr))
            .json(&json!({
                "name": name,
                "agent_endpoint": format!("wss://{name}:2419/ws"),
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let wid = reg["worker_id"].as_str().unwrap();
        client()
            .post(format!("http://{}/v1/workers/heartbeat", srv.addr))
            .json(&json!({
                "worker_id": wid,
                "agent_healthy": healthy,
            }))
            .send()
            .await
            .unwrap();
    }

    // filter online only
    let resp = client()
        .get(format!("http://{}/v1/workers?status=online", srv.addr))
        .send()
        .await
        .unwrap();
    let workers: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0]["name"], "ok-01");

    // filter degraded
    let resp = client()
        .get(format!("http://{}/v1/workers?status=degraded", srv.addr))
        .send()
        .await
        .unwrap();
    let workers: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0]["name"], "bad-01");
}

#[tokio::test]
async fn register_validates_name() {
    let srv = spawn_server().await;
    // name with invalid char (space)
    let resp = client()
        .post(format!("http://{}/v1/workers/register", srv.addr))
        .json(&json!({
            "name": "bad name!",
            "agent_endpoint": "wss://x:1/ws",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn register_with_empty_endpoint_rejected() {
    let srv = spawn_server().await;
    let resp = client()
        .post(format!("http://{}/v1/workers/register", srv.addr))
        .json(&json!({
            "name": "no-ep",
            "agent_endpoint": "",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn list_workers_label_filtering_and_pagination() {
    let srv = spawn_server().await;

    // 두 개의 워커 등록
    let resp1 = client()
        .post(format!("http://{}/v1/workers/register", srv.addr))
        .json(&json!({
            "name": "w-arm",
            "agent_endpoint": "wss://127.0.0.1:8080/ws",
            "labels": { "arch": "arm64", "gpu": "true" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status(), 200);

    let resp2 = client()
        .post(format!("http://{}/v1/workers/register", srv.addr))
        .json(&json!({
            "name": "w-x86",
            "agent_endpoint": "wss://127.0.0.1:8081/ws",
            "labels": { "arch": "x86_64", "gpu": "true" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 200);

    // 1. 라벨 필터링 테스트: label_arch=arm64
    let resp = client()
        .get(format!("http://{}/v1/workers?label_arch=arm64", srv.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let workers: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0]["name"], "w-arm");

    // 2. 페이지네이션 테스트: limit=1 & offset=1
    let resp = client()
        .get(format!("http://{}/v1/workers?limit=1&offset=1", srv.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let workers_paginated: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(workers_paginated.len(), 1);
}
