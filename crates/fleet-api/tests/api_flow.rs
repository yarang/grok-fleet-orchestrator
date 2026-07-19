//! HTTP API 통합 테스트.
//!
//! `register → heartbeat → list → get → deregister` 흐름을 실제 TCP 리스너와
//! reqwest HTTP 클라이언트로 end-to-end 검증합니다. Postgres 없이 인메모리
//! Store 구현체를 사용합니다.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;
use tokio::task::JoinHandle;

use fleet_api::AppState;
use fleet_core::{
    BootstrapToken, EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput, TaskStatus,
    Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
};
use fleet_store::{Store, StoreError};

// ── 인메모리 Store (테스트 픽스처) ──────────────────────────────────────

struct MemStore {
    workers: Mutex<HashMap<WorkerId, Worker>>,
    events: Mutex<Vec<EventEntry>>,
}

impl MemStore {
    fn new() -> Self {
        Self {
            workers: Mutex::new(HashMap::new()),
            events: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Store for MemStore {
    async fn insert_task(&self, _: &Task) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn get_task(&self, _: TaskId) -> Result<Option<Task>, StoreError> {
        unimplemented!()
    }
    async fn update_task_status(&self, _: TaskId, _: &TaskStatus) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn list_tasks(&self, _: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        unimplemented!()
    }
    async fn upsert_worker(&self, w: &Worker) -> Result<(), StoreError> {
        self.workers.lock().unwrap().insert(w.id, w.clone());
        Ok(())
    }
    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
        Ok(self.workers.lock().unwrap().get(&id).cloned())
    }
    async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError> {
        Ok(self
            .workers
            .lock()
            .unwrap()
            .values()
            .find(|w| w.name == name)
            .cloned())
    }
    async fn list_workers(&self, f: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
        Ok(self
            .workers
            .lock()
            .unwrap()
            .values()
            .filter(|w| f.status.map_or(true, |s| w.status == s))
            .filter(|w| {
                f.labels
                    .iter()
                    .all(|(k, v)| w.labels.get(k) == Some(v))
            })
            .cloned()
            .collect())
    }
    async fn delete_worker(&self, id: WorkerId) -> Result<(), StoreError> {
        self.workers.lock().unwrap().remove(&id);
        Ok(())
    }
    async fn update_worker_heartbeat(
        &self,
        id: WorkerId,
        hb: &WorkerHeartbeat,
    ) -> Result<(), StoreError> {
        if let Some(w) = self.workers.lock().unwrap().get_mut(&id) {
            w.active_tasks = hb.active_tasks;
            w.last_seen = Some(chrono::Utc::now());
        }
        Ok(())
    }
    async fn append_event(&self, e: &FleetEvent) -> Result<u64, StoreError> {
        let mut events = self.events.lock().unwrap();
        let seq = (events.len() + 1) as u64;
        events.push(EventEntry {
            seq,
            event: e.clone(),
        });
        Ok(seq)
    }
    async fn list_events(&self, _: u64, _: u32) -> Result<Vec<EventEntry>, StoreError> {
        Ok(self.events.lock().unwrap().clone())
    }
    async fn append_output(&self, _: TaskId, _: &str) -> Result<u64, StoreError> {
        unimplemented!()
    }
    async fn get_output(&self, _: TaskId, _: u64) -> Result<TaskOutput, StoreError> {
        unimplemented!()
    }
    async fn migrate(&self) -> Result<(), StoreError> {
        Ok(())
    }
    async fn create_bootstrap_token(&self, _: &BootstrapToken) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn consume_bootstrap_token(&self, _: &str, _: &str) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn list_bootstrap_tokens(&self) -> Result<Vec<BootstrapToken>, StoreError> {
        unimplemented!()
    }
    async fn revoke_bootstrap_token(&self, _: &str) -> Result<bool, StoreError> {
        unimplemented!()
    }
}

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
    Server { addr, _handle: handle }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .build()
        .expect("reqwest client")
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
    assert!(body["error"]["message"].as_str().unwrap().contains("worker"));
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
