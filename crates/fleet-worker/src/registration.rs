//! Orchestrator HTTP API 클라이언트 — register + heartbeat 루프.
//!
//! ## 등록 흐름
//!
//! 1. POST /v1/workers/register
//!    - body: `{ name, agent_endpoint, labels, max_concurrent_tasks, existing_worker_id? }`
//!    - Authorization: Bearer <bootstrap_token>
//!    - 응답: `{ worker_id, heartbeat_interval_secs, ... }`
//!
//! 2. worker_id를 반환받아 이후 heartbeat에 사용.
//!
//! ## 하트비트 루프
//!
//! - 주기: register 응답의 `heartbeat_interval_secs` (없으면 config의 값).
//! - 본문: `{ worker_id, active_tasks, load_avg, mem_available_mb, disk_free_mb, agent_healthy }`
//! - agent_healthy은 GrokRunner의 헬스체크 결과.
//!
//! ## 재시도 정책
//!
//! - register 실패: 5초 간격으로 무한 재시도 (워커는 orchestrator보다 먼저 뜰 수 있음).
//! - heartbeat 실패: warn 로그만, 다음 주기에 재시도.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::config::WorkerConfig;
use crate::error::WorkerError;
use crate::grok_process;

/// orchestrator와 통신하는 HTTP 클라이언트.
pub struct RegistrationClient {
    config: Arc<WorkerConfig>,
    http: reqwest::Client,
    /// 등록 후 발급받은 worker_id. None이면 아직 미등록.
    worker_id: tokio::sync::Mutex<Option<String>>,
}

/// `POST /v1/workers/register` 응답.
#[derive(Debug, Clone, Deserialize)]
pub struct RegisterResponse {
    pub worker_id: String,
    #[serde(default)]
    pub heartbeat_interval_secs: u32,
    #[allow(dead_code)]
    #[serde(default)]
    pub status: Option<String>,
}

/// `POST /v1/workers/register` 요청.
#[derive(Debug, Serialize)]
struct RegisterRequest {
    name: String,
    agent_endpoint: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    labels: Vec<(String, String)>,
    max_concurrent_tasks: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    existing_worker_id: Option<String>,
}

/// `POST /v1/workers/heartbeat` 요청.
#[derive(Debug, Serialize)]
struct HeartbeatRequest {
    worker_id: String,
    active_tasks: u32,
    load_avg: Option<Vec<f32>>,
    mem_available_mb: Option<u64>,
    disk_free_mb: Option<u64>,
    agent_healthy: bool,
}

/// `DELETE /v1/workers/:id` 요청.
#[derive(Debug, Serialize)]
struct DeregisterRequest {
    reason: String,
}

impl RegistrationClient {
    /// 새 클라이언트 생성.
    pub fn new(config: Arc<WorkerConfig>) -> Result<Self, WorkerError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(WorkerError::Http)?;
        Ok(Self {
            config,
            http,
            worker_id: tokio::sync::Mutex::new(None),
        })
    }

    /// orchestrator에 등록. 실패 시 5초 간격으로 무한 재시도.
    pub async fn register_with_retry(&self) -> Result<RegisterResponse, WorkerError> {
        loop {
            match self.register_once().await {
                Ok(resp) => {
                    *self.worker_id.lock().await = Some(resp.worker_id.clone());
                    return Ok(resp);
                }
                Err(e) => {
                    warn!(error = %e, "register failed — retrying in 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    /// 1회 등록 시도. 성공 시 worker_id를 내부 상태에 저장.
    pub async fn register_once(&self) -> Result<RegisterResponse, WorkerError> {
        let endpoint = self.config.agent_endpoint();
        let labels: Vec<(String, String)> = self
            .config
            .worker
            .labels
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let body = RegisterRequest {
            name: self.config.worker.name.clone(),
            agent_endpoint: endpoint,
            labels,
            max_concurrent_tasks: self.config.grok.max_concurrent_tasks,
            existing_worker_id: self.config.worker.existing_worker_id.clone(),
        };

        let url = format!(
            "{}/v1/workers/register",
            self.config.worker.orchestrator_url
        );
        let mut req = self.http.post(&url).json(&body);
        if let Some(token) = &self.config.worker.bootstrap_token {
            req = req.bearer_auth(token);
        }

        let resp = req.send().await.map_err(WorkerError::Http)?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(WorkerError::OrchestratorApi(format!(
                "register returned {status}: {text}"
            )));
        }

        let register_resp: RegisterResponse = resp.json().await.map_err(WorkerError::Http)?;
        // 성공 시 worker_id 저장.
        *self.worker_id.lock().await = Some(register_resp.worker_id.clone());
        info!(worker_id = %register_resp.worker_id, "registered with orchestrator");
        Ok(register_resp)
    }

    /// 하트비트 1회 전송.
    pub async fn heartbeat_once(&self, agent_healthy: bool) -> Result<(), WorkerError> {
        let worker_id = self
            .worker_id
            .lock()
            .await
            .clone()
            .ok_or_else(|| WorkerError::OrchestratorApi("not registered yet".into()))?;

        // 시스템 메트릭 수집.
        let (load_avg, mem_available_mb, disk_free_mb, active_tasks) = collect_system_metrics();

        let body = HeartbeatRequest {
            worker_id,
            active_tasks,
            load_avg,
            mem_available_mb,
            disk_free_mb,
            agent_healthy,
        };

        let url = format!(
            "{}/v1/workers/heartbeat",
            self.config.worker.orchestrator_url
        );
        let mut req = self.http.post(&url).json(&body);
        if let Some(token) = &self.config.worker.bootstrap_token {
            req = req.bearer_auth(token);
        }

        let resp = req.send().await.map_err(WorkerError::Http)?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(WorkerError::OrchestratorApi(format!(
                "heartbeat returned {status}: {text}"
            )));
        }
        Ok(())
    }

    /// 하트비트 루프. shutdown_rx가 true가 될 때까지.
    pub async fn run_heartbeat_loop(
        &self,
        interval_secs: u32,
        grok_bind_addr: String,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        let interval = Duration::from_secs(interval_secs.max(1) as u64);
        info!(interval_secs, "starting heartbeat loop");

        loop {
            // shutdown 체크.
            if *shutdown_rx.borrow() {
                info!("heartbeat loop shutting down");
                return;
            }

            // 헬스체크: grok bind_addr에 TCP 연결 시도.
            let agent_healthy = grok_process::health_check(&grok_bind_addr, 1000)
                .await
                .is_ok();

            // heartbeat 전송.
            if let Err(e) = self.heartbeat_once(agent_healthy).await {
                warn!(error = %e, "heartbeat failed — will retry next interval");
            }

            // 다음 주기까지 대기. shutdown 시 즉시 반환.
            tokio::select! {
                _ = tokio::time::sleep(interval) => continue,
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("heartbeat loop received shutdown");
                        return;
                    }
                }
            }
        }
    }

    /// 등록 해제. best-effort.
    pub async fn deregister(&self, reason: &str) {
        let worker_id = match self.worker_id.lock().await.clone() {
            Some(id) => id,
            None => return,
        };

        let url = format!(
            "{}/v1/workers/{worker_id}",
            self.config.worker.orchestrator_url
        );
        let body = DeregisterRequest {
            reason: reason.to_string(),
        };
        let mut req = self.http.delete(&url).json(&body);
        if let Some(token) = &self.config.worker.bootstrap_token {
            req = req.bearer_auth(token);
        }

        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                info!(%worker_id, "deregistered");
            }
            Ok(resp) => {
                warn!(
                    status = %resp.status(),
                    "deregister failed (best-effort)"
                );
            }
            Err(e) => {
                warn!(error = %e, "deregister request failed (best-effort)");
            }
        }
    }

    /// 현재 worker_id 반환 (없으면 None).
    pub async fn worker_id(&self) -> Option<String> {
        self.worker_id.lock().await.clone()
    }
}

/// 시스템 메트릭 수집 (sysinfo 사용).
/// 반환: (load_avg, mem_available_mb, disk_free_mb, active_tasks).
///
/// active_tasks는 현재 0 (grok에게 위임). Phase 8 후반에서 실제 카운트 추가.
fn collect_system_metrics() -> (Option<Vec<f32>>, Option<u64>, Option<u64>, u32) {
    use sysinfo::{Disks, System};

    let mut sys = System::new();
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    // sysinfo 0.32에서 load_average는 associated function.
    let load_avg = System::load_average();
    // orchestrator API는 f32를 기대하므로 f64 → f32로 캐스팅 (손실 무시 가능).
    let load_vec = vec![
        load_avg.one as f32,
        load_avg.five as f32,
        load_avg.fifteen as f32,
    ];

    let mem_available_mb = sys.available_memory() / 1024; // KiB → MiB

    let disks = Disks::new_with_refreshed_list();
    let disk_free_mb: u64 = disks
        .list()
        .iter()
        .map(|d| (d.total_space() - d.available_space()) / 1024 / 1024)
        .sum();

    (
        Some(load_vec),
        Some(mem_available_mb),
        Some(disk_free_mb),
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    use serde_json::Value;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex as TokioMutex;

    /// mock orchestrator의 공유 상태.
    #[derive(Clone)]
    struct MockState {
        registers: Arc<TokioMutex<Vec<Value>>>,
        heartbeats: Arc<TokioMutex<Vec<Value>>>,
        deregisters: Arc<TokioMutex<Vec<Value>>>,
        /// 등록 응답 상태 코드.
        register_status: Arc<TokioMutex<u16>>,
    }

    impl Default for MockState {
        fn default() -> Self {
            Self {
                registers: Arc::new(TokioMutex::new(Vec::new())),
                heartbeats: Arc::new(TokioMutex::new(Vec::new())),
                deregisters: Arc::new(TokioMutex::new(Vec::new())),
                register_status: Arc::new(TokioMutex::new(200)),
            }
        }
    }

    async fn start_mock_orchestrator(state: MockState) -> String {
        use axum::extract::Path;
        use axum::routing::delete;

        let register_state = state.clone();
        let hb_state = state.clone();
        let dereg_state = state.clone();

        let app = Router::new()
            .route(
                "/v1/workers/register",
                post(move |Json(body): Json<Value>| {
                    let s = register_state.clone();
                    async move {
                        s.registers.lock().await.push(body);
                        let status = *s.register_status.lock().await;
                        if status == 200 {
                            (
                                axum::http::StatusCode::OK,
                                Json(serde_json::json!({
                                    "worker_id": "test-uuid-123",
                                    "heartbeat_interval_secs": 1,
                                    "status": "online",
                                })),
                            )
                        } else {
                            (
                                axum::http::StatusCode::from_u16(status).unwrap(),
                                Json(serde_json::json!({"error": "simulated"})),
                            )
                        }
                    }
                }),
            )
            .route(
                "/v1/workers/heartbeat",
                post(move |Json(body): Json<Value>| {
                    let s = hb_state.clone();
                    async move {
                        s.heartbeats.lock().await.push(body);
                        (
                            axum::http::StatusCode::OK,
                            Json(serde_json::json!({"ok": true})),
                        )
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
                            if let Some(obj) = entry.as_object_mut() {
                                if let Some(reason) = b.get("reason") {
                                    obj.insert("reason".to_string(), reason.clone());
                                }
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
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn register_success_returns_worker_id() {
        let state = MockState::default();
        let url = start_mock_orchestrator(state.clone()).await;

        let config = Arc::new(WorkerConfig::for_test().orchestrator_url(url).build());
        let client = RegistrationClient::new(config).unwrap();
        let resp = client.register_once().await.unwrap();

        assert_eq!(resp.worker_id, "test-uuid-123");
        assert_eq!(resp.heartbeat_interval_secs, 1);

        // 요청 본문 검증.
        let registers = state.registers.lock().await;
        assert_eq!(registers.len(), 1);
        assert_eq!(registers[0]["name"], "test-worker");
        assert!(registers[0]["agent_endpoint"]
            .as_str()
            .unwrap()
            .contains("server-key="));
    }

    #[tokio::test]
    async fn register_failure_returns_error() {
        let state = MockState::default();
        *state.register_status.lock().await = 500;
        let url = start_mock_orchestrator(state).await;

        let config = Arc::new(WorkerConfig::for_test().orchestrator_url(url).build());
        let client = RegistrationClient::new(config).unwrap();
        let result = client.register_once().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn heartbeat_after_register_succeeds() {
        let state = MockState::default();
        let url = start_mock_orchestrator(state.clone()).await;

        let config = Arc::new(WorkerConfig::for_test().orchestrator_url(url).build());
        let client = RegistrationClient::new(config).unwrap();

        // 등록 없이 heartbeat 시도 → 실패.
        let r = client.heartbeat_once(false).await;
        assert!(r.is_err());

        // 등록 후 heartbeat.
        client.register_once().await.unwrap();
        client.heartbeat_once(true).await.unwrap();

        let hbs = state.heartbeats.lock().await;
        assert_eq!(hbs.len(), 1);
        assert_eq!(hbs[0]["agent_healthy"], true);
    }

    #[tokio::test]
    async fn heartbeat_loop_sends_multiple_then_stops_on_shutdown() {
        let state = MockState::default();
        let url = start_mock_orchestrator(state.clone()).await;

        let config = Arc::new(WorkerConfig::for_test().orchestrator_url(url).build());
        let client = Arc::new(RegistrationClient::new(config).unwrap());
        client.register_once().await.unwrap();

        // heartbeat_loop를 백그라운드로 spawn.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let hb_client = client.clone();
        let hb_handle = tokio::spawn(async move {
            hb_client
                .run_heartbeat_loop(1, "127.0.0.1:1".into(), shutdown_rx)
                .await;
        });

        // 3초 대기 → 3개 정도 heartbeat 도착 예상.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let _ = shutdown_tx.send(true);

        // 종료 대기.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), hb_handle).await;

        let hbs = state.heartbeats.lock().await;
        assert!(
            hbs.len() >= 2,
            "expected at least 2 heartbeats, got {}",
            hbs.len()
        );
    }

    #[tokio::test]
    async fn deregister_after_register_calls_delete() {
        let state = MockState::default();
        let url = start_mock_orchestrator(state.clone()).await;

        let config = Arc::new(WorkerConfig::for_test().orchestrator_url(url).build());
        let client = RegistrationClient::new(config).unwrap();
        client.register_once().await.unwrap();
        client.deregister("test shutdown").await;

        let deregisters = state.deregisters.lock().await;
        assert_eq!(deregisters.len(), 1);
        assert_eq!(deregisters[0]["id"], "test-uuid-123");
        assert_eq!(deregisters[0]["reason"], "test shutdown");
    }

    #[tokio::test]
    async fn deregister_without_register_is_noop() {
        let state = MockState::default();
        let url = start_mock_orchestrator(state.clone()).await;

        let config = Arc::new(WorkerConfig::for_test().orchestrator_url(url).build());
        let client = RegistrationClient::new(config).unwrap();
        // register 없이 deregister — 조용히 무시.
        client.deregister("nothing").await;

        let deregisters = state.deregisters.lock().await;
        assert_eq!(deregisters.len(), 0, "no deregister should be sent");
    }
}
