//! fleet-dashboard HTTP API 통합 테스트.
//!
//! 실제 Postgres 없이 `MemStore`만으로 엔드포인트를 검증합니다.
//! SSE(/api/events/stream)는 PgPool LISTEN/NOTIFY가 필요하므로 본 테스트에서 제외.

#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fleet_core::{
    BootstrapToken, EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput, TaskStatus,
    Worker, WorkerFilter, WorkerHeartbeat, WorkerId, WorkerStatus,
};
use fleet_dashboard::{build_dashboard_app, DashboardState};
use fleet_store::{Store, StoreError};
use sqlx::postgres::PgPoolOptions;
use tokio::task::JoinHandle;

// ═══════════════════════════════════════════════════════════════════════
//  인메모리 Store (실제 DB 없이 테스트용)
// ═══════════════════════════════════════════════════════════════════════

struct MemStore {
    workers: Mutex<HashMap<WorkerId, Worker>>,
    tasks: Mutex<HashMap<TaskId, Task>>,
    events: Mutex<Vec<EventEntry>>,
}

impl MemStore {
    fn new() -> Self {
        Self {
            workers: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            events: Mutex::new(Vec::new()),
        }
    }

    fn with_worker(self, w: Worker) -> Self {
        self.workers.lock().unwrap().insert(w.id, w);
        self
    }

    fn with_task(self, t: Task) -> Self {
        self.tasks.lock().unwrap().insert(t.id, t);
        self
    }
}

#[async_trait]
impl Store for MemStore {
    async fn insert_task(&self, t: &Task) -> Result<(), StoreError> {
        self.tasks.lock().unwrap().insert(t.id, t.clone());
        Ok(())
    }
    async fn get_task(&self, id: TaskId) -> Result<Option<Task>, StoreError> {
        Ok(self.tasks.lock().unwrap().get(&id).cloned())
    }
    async fn update_task_status(&self, id: TaskId, status: &TaskStatus) -> Result<(), StoreError> {
        if let Some(t) = self.tasks.lock().unwrap().get_mut(&id) {
            t.status = status.clone();
        }
        Ok(())
    }
    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        let mut all: Vec<Task> = self.tasks.lock().unwrap().values().cloned().collect();
        all.sort_by_key(|a| a.created_at);
        all.truncate(filter.limit);
        Ok(all)
    }
    async fn upsert_worker(&self, w: &Worker) -> Result<(), StoreError> {
        self.workers.lock().unwrap().insert(w.id, w.clone());
        Ok(())
    }
    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
        Ok(self.workers.lock().unwrap().get(&id).cloned())
    }
    async fn get_worker_by_name(&self, _: &str) -> Result<Option<Worker>, StoreError> {
        Ok(None)
    }
    async fn list_workers(&self, filter: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
        let mut all: Vec<Worker> = self.workers.lock().unwrap().values().cloned().collect();
        if let Some(status) = filter.status {
            all.retain(|w| w.status == status);
        }
        all.sort_by_key(|w| w.registered_at);
        all.truncate(filter.limit);
        Ok(all)
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
            w.last_seen = Some(chrono::Utc::now());
            let _ = hb;
        }
        Ok(())
    }
    async fn append_event(&self, _: &FleetEvent) -> Result<u64, StoreError> {
        Ok(0)
    }
    async fn list_events(&self, after_seq: u64, limit: u32) -> Result<Vec<EventEntry>, StoreError> {
        let all = self.events.lock().unwrap();
        let filtered: Vec<EventEntry> = all
            .iter()
            .filter(|e| e.seq > after_seq)
            .take(limit as usize)
            .cloned()
            .collect();
        Ok(filtered)
    }
    async fn append_output(&self, _: TaskId, _: &str) -> Result<u64, StoreError> {
        Ok(0)
    }
    async fn get_output(&self, id: TaskId, _: u64) -> Result<TaskOutput, StoreError> {
        Ok(TaskOutput {
            task_id: id,
            chunks: Vec::new(),
            next_offset: 0,
        })
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

// ═══════════════════════════════════════════════════════════════════════
//  테스트 헬퍼
// ═══════════════════════════════════════════════════════════════════════

struct TestServer {
    addr: SocketAddr,
    _handle: JoinHandle<()>,
}

async fn spawn_server(store: MemStore) -> TestServer {
    // connect_lazy: 실제 연결 없이 PgPool 핸들만 생성 (SSE 미사용 테스트용).
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://__test_unused__@localhost/__none__")
        .expect("connect_lazy must not perform I/O");

    let state = Arc::new(DashboardState::new(
        Arc::new(store) as Arc<dyn Store>,
        pool,
    ));
    let app = build_dashboard_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    TestServer { addr, _handle: handle }
}

fn sample_worker(name: &str, status: WorkerStatus) -> Worker {
    let id = WorkerId::new();
    Worker {
        id,
        name: name.into(),
        endpoint: format!("https://{name}.example"),
        status,
        labels: HashMap::from([("env".into(), "test".into())]),
        active_tasks: 0,
        max_concurrent: 4,
        circuit_state: fleet_core::CircuitState::Closed,
        last_seen: None,
        worker_version: None,
        registered_at: chrono::Utc::now(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  테스트 케이스
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let server = spawn_server(MemStore::new()).await;
    let resp = reqwest::get(format!("http://{}/health", server.addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn index_serves_html() {
    let server = spawn_server(MemStore::new()).await;
    let resp = reqwest::get(format!("http://{}/", server.addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<!DOCTYPE html>") || body.contains("<html"));
    let ct = reqwest::get(format!("http://{}/", server.addr))
        .await
        .unwrap()
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(ct.starts_with("text/html"), "unexpected content-type: {ct}");
}

#[tokio::test]
async fn overview_aggregates_counts() {
    let store = MemStore::new()
        .with_worker(sample_worker("w1", WorkerStatus::Online))
        .with_worker(sample_worker("w2", WorkerStatus::Offline))
        .with_worker(sample_worker("w3", WorkerStatus::Degraded));
    let server = spawn_server(store).await;

    let resp = reqwest::get(format!("http://{}/api/overview", server.addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let workers = &body["workers"];
    assert_eq!(workers["total"], 3);
    assert_eq!(workers["online"], 1);
    assert_eq!(workers["offline"], 1);
    assert_eq!(workers["degraded"], 1);
    assert_eq!(workers["circuit_open"], 0);
}

#[tokio::test]
async fn workers_list_returns_summaries() {
    let store = MemStore::new()
        .with_worker(sample_worker("alpha", WorkerStatus::Online))
        .with_worker(sample_worker("beta", WorkerStatus::Offline));
    let server = spawn_server(store).await;

    let resp = reqwest::get(format!("http://{}/api/workers", server.addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let arr: serde_json::Value = resp.json().await.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    let names: Vec<&str> = arr
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
}

#[tokio::test]
async fn workers_list_status_filter() {
    let store = MemStore::new()
        .with_worker(sample_worker("online-w", WorkerStatus::Online))
        .with_worker(sample_worker("offline-w", WorkerStatus::Offline));
    let server = spawn_server(store).await;

    let resp = reqwest::get(format!(
        "http://{}/api/workers?status=online",
        server.addr
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let arr: serde_json::Value = resp.json().await.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "online-w");
    assert_eq!(arr[0]["status"], "online");
}

#[tokio::test]
async fn tasks_list_returns_array() {
    use fleet_core::TaskRequest;

    let mk_task = |prompt: &str| {
        Task::from_request(TaskRequest {
            prompt: prompt.into(),
            created_by: "tester".into(),
            ..Default::default()
        })
    };

    let store = MemStore::new()
        .with_task(mk_task("hello"))
        .with_task(mk_task("world"));
    let server = spawn_server(store).await;

    let resp = reqwest::get(format!("http://{}/api/tasks", server.addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let arr: serde_json::Value = resp.json().await.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    // 모두 pending 단계.
    for t in arr {
        assert_eq!(t["phase"], "pending");
    }
}

#[tokio::test]
async fn events_list_returns_empty_array() {
    let server = spawn_server(MemStore::new()).await;
    let resp = reqwest::get(format!("http://{}/api/events", server.addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["count"], 0);
    assert!(body["events"].is_array());
}

#[tokio::test]
async fn static_asset_css_served() {
    let server = spawn_server(MemStore::new()).await;
    let resp = reqwest::get(format!("http://{}/static/styles.css", server.addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(ct.contains("css"), "unexpected content-type: {ct}");
    let body = resp.text().await.unwrap();
    assert!(!body.is_empty());
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let server = spawn_server(MemStore::new()).await;
    let resp = reqwest::get(format!("http://{}/nope", server.addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
