//! `/metrics` 엔드포인트 통합 테스트.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::task::JoinHandle;

use fleet_api::AppState;
use fleet_core::{
    BootstrapToken, EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput, TaskRequest,
    TaskStatus, Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
};
use fleet_store::{Store, StoreError};

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
    async fn list_tasks(&self, f: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        let mut all: Vec<Task> = self.tasks.lock().unwrap().values().cloned().collect();
        all.sort_by_key(|t| t.created_at);
        all.truncate(f.limit);
        Ok(all)
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
        let mut all: Vec<Worker> = self.workers.lock().unwrap().values().cloned().collect();
        if let Some(status) = f.status {
            all.retain(|w| w.status == status);
        }
        all.sort_by_key(|w| w.registered_at);
        all.truncate(f.limit);
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
    async fn list_events(&self, after: u64, limit: u32) -> Result<Vec<EventEntry>, StoreError> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.seq > after)
            .take(limit as usize)
            .cloned()
            .collect())
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
    async fn upsert_worker_credential(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: u32,
        _: Option<&str>,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn get_worker_credential(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<fleet_store::StoredCredential>, StoreError> {
        unimplemented!()
    }
    async fn list_worker_credentials(
        &self,
        _: &str,
    ) -> Result<Vec<fleet_store::StoredCredential>, StoreError> {
        unimplemented!()
    }
    async fn delete_worker_credential(&self, _: &str, _: &str) -> Result<bool, StoreError> {
        unimplemented!()
    }
}

struct Server {
    addr: SocketAddr,
    _handle: JoinHandle<()>,
}

async fn spawn_server() -> Server {
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    let state = Arc::new(AppState::new(store));
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

#[tokio::test]
async fn metrics_returns_prometheus_text() {
    let srv = spawn_server().await;
    let resp = reqwest::get(format!("http://{}/metrics", srv.addr))
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
    assert!(
        ct.starts_with("text/plain"),
        "expected text/plain content-type, got: {ct}"
    );
    let body = resp.text().await.unwrap();
    assert!(body.contains("# HELP fleet_up"));
    assert!(body.contains("# TYPE fleet_up gauge"));
    assert!(body.contains("fleet_up 1"));
    assert!(body.contains("fleet_workers_total{status=\"online\"}"));
    assert!(body.contains("fleet_tasks_total{phase=\"pending\"}"));
    assert!(body.contains("fleet_workers_capacity_total"));
    assert!(body.contains("fleet_workers_active_tasks_total"));
    assert!(body.contains("fleet_events_written_total"));
    assert!(body.contains("fleet_task_dispatch_latency_seconds"));
}

#[tokio::test]
async fn metrics_does_not_require_auth() {
    // 인증이 활성화된 AppState에서도 /metrics는 인증 미들웨어 바깥에 있음.
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    let state = Arc::new(AppState::new(store).with_tokens(vec!["secret".into()]));
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = fleet_api::build_app(state);
    let _handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // Authorization 헤더 없이 호출해도 200이어야 함.
    let resp = reqwest::get(format!("http://{}/metrics", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn metrics_reflects_state_changes() {
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    store
        .upsert_worker(&Worker::new("w1", "wss://1"))
        .await
        .unwrap();
    store
        .insert_task(&Task::from_request(TaskRequest {
            prompt: "p".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let state = Arc::new(AppState::new(store));
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = fleet_api::build_app(state);
    let _handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let body = reqwest::get(format!("http://{}/metrics", addr))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("fleet_workers_total{status=\"online\"} 1"));
    assert!(body.contains("fleet_tasks_total{phase=\"pending\"} 1"));
    assert!(body.contains("fleet_workers_capacity_total 4"));
}

/// HTTP 지연 히스토그램이 실제 요청을 통해 누적되는지 검증한다.
///
/// 미들웨어가 라우터에 제대로 걸려 있지 않으면 단위 테스트는 통과해도
/// 값이 영원히 0이므로, 실제 라우터를 거치는 경로로 확인한다.
#[tokio::test]
async fn http_duration_histogram_records_requests() {
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    let state = Arc::new(AppState::new(store));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let app = fleet_api::build_app(state);
    let _handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // 첫 스크랩 — 이 요청 자체는 아직 관측 전이므로 0건일 수 있다.
    let first = reqwest::get(format!("http://{addr}/metrics"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        first.contains("# TYPE fleet_http_request_duration_seconds histogram"),
        "히스토그램 메타데이터가 노출되어야 한다:\n{first}"
    );

    // 몇 건 더 요청한 뒤 다시 스크랩하면 count가 증가해 있어야 한다.
    for _ in 0..3 {
        let _ = reqwest::get(format!("http://{addr}/metrics"))
            .await
            .unwrap();
    }

    let body = reqwest::get(format!("http://{addr}/metrics"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let count_line = body
        .lines()
        .find(|l| l.starts_with("fleet_http_request_duration_seconds_count "))
        .expect("count 라인이 있어야 한다");
    let count: u64 = count_line
        .rsplit(' ')
        .next()
        .unwrap()
        .parse()
        .expect("count는 정수여야 한다");
    assert!(
        count >= 4,
        "이전 요청들이 관측되어야 한다 (실제 count={count})\n{count_line}"
    );

    // +Inf 버킷은 전체 관측 수와 같아야 한다 (Prometheus 규약).
    let inf_line = body
        .lines()
        .find(|l| l.starts_with("fleet_http_request_duration_seconds_bucket{le=\"+Inf\"}"))
        .expect("+Inf 버킷이 있어야 한다");
    let inf: u64 = inf_line.rsplit(' ').next().unwrap().parse().unwrap();
    assert_eq!(inf, count, "+Inf 버킷은 count와 일치해야 한다");
}
