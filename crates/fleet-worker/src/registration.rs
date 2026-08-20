//! Orchestrator HTTP API 클라이언트 — register + heartbeat 루프.
//!
//! ## 등록 흐름
//!
//! 1. POST /v1/workers/register
//!    - body: `{ name, agent_endpoint, labels, max_concurrent_tasks, existing_worker_id? }`
//!    - Authorization: Bearer <operational_token>
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

use opentelemetry::propagation::Injector;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tracing::{info, warn};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::config::WorkerConfig;
use crate::error::WorkerError;
use crate::grok_process;

use std::sync::OnceLock;

/// `reqwest::header::HeaderMap`에 W3C Trace Context를 쓰기 위한
/// `opentelemetry::propagation::Injector` 어댑터 (로드맵 #42).
struct HeaderInjector<'a>(&'a mut reqwest::header::HeaderMap);

impl Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(key.as_bytes()),
            reqwest::header::HeaderValue::from_str(&value),
        ) {
            self.0.insert(name, val);
        }
    }
}

/// 현재 스팬의 트레이스 컨텍스트를 `traceparent`/`tracestate` 헤더로 담은
/// `HeaderMap`을 만든다 (로드맵 #42). `fleet_worker::init_tracing()`이 등록한
/// 전역 propagator를 사용한다 — OTel이 비활성이거나(OTLP endpoint 미설정)
/// 호출 시점에 활성 스팬이 없으면 유효한 컨텍스트가 없어 헤더가 비어
/// 있는 채로 반환된다(무해한 no-op — orchestrator 쪽은 헤더가 없으면 그냥
/// 로컬 루트 스팬으로 처리한다).
fn trace_context_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    let cx = tracing::Span::current().context();
    opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&cx, &mut HeaderInjector(&mut headers));
    });
    headers
}

/// orchestrator와 통신하는 HTTP 클라이언트.
pub struct RegistrationClient {
    config: Arc<WorkerConfig>,
    http: reqwest::Client,
    /// 등록 후 발급받은 worker_id. None이면 아직 미등록.
    worker_id: tokio::sync::Mutex<Option<String>>,
    /// 디스크 여유 공간 캐시. blocking syscall을 피하기 위해 백그라운드 수집 + TTL 캐싱.
    disk_cache: Arc<DiskCache>,
    /// grok CLI 버전 (변하지 않으므로 최초 1회만 수집).
    grok_version: OnceLock<Option<String>>,
    /// OS 정보 (변하지 않으므로 최초 1회만 수집).
    os_info: OnceLock<Option<WorkerOsInfo>>,
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
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty", default)]
    labels: std::collections::HashMap<String, String>,
    max_concurrent_tasks: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    existing_worker_id: Option<String>,
    /// liveness 보고 방식 (로드맵 #61). `Default`(periodic)이면 orchestrator
    /// 쪽 `#[serde(default)]`가 기존과 동일하게 처리하므로 항상 실어 보내도
    /// 하위 호환에 문제없다.
    liveness_mode: fleet_core::WorkerLivenessMode,
}

/// `POST /v1/workers/heartbeat` 요청.
#[derive(Debug, Serialize)]
struct HeartbeatRequest {
    worker_id: String,
    active_tasks: u32,
    load_avg: Option<Vec<f32>>,
    mem_available_mb: Option<u64>,
    disk_free_mb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu_usage: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ram_usage: Option<f32>,
    agent_healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    grok_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fleet_worker_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    os_info: Option<WorkerOsInfo>,
}

/// heartbeat 요청용 OS 정보 (fleet-core::OsInfo와 동일 구조).
#[derive(Debug, Clone, Serialize)]
struct WorkerOsInfo {
    os_type: String,
    distro: String,
    kernel: String,
    arch: String,
    hostname: String,
}

#[derive(Debug, Deserialize)]
pub struct HeartbeatResponse {
    pub ok: bool,
    pub desired_state: String,
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
            disk_cache: Arc::new(DiskCache::new()),
            grok_version: OnceLock::new(),
            os_info: OnceLock::new(),
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
    #[tracing::instrument(skip(self), fields(worker_name = %self.config.worker.name))]
    pub async fn register_once(&self) -> Result<RegisterResponse, WorkerError> {
        let endpoint = self.config.agent_endpoint();
        let labels = self.config.worker.labels.clone();

        let body = RegisterRequest {
            name: self.config.worker.name.clone(),
            agent_endpoint: endpoint,
            labels,
            max_concurrent_tasks: self.config.grok.max_concurrent_tasks,
            existing_worker_id: self.config.worker.existing_worker_id.clone(),
            liveness_mode: self.config.worker.liveness_mode,
        };

        let url = format!(
            "{}/v1/workers/register",
            self.config.worker.orchestrator_url
        );
        // 로드맵 #42 — 이 스팬의 트레이스 컨텍스트를 traceparent/tracestate
        // 헤더로 실어 보낸다. fleet-api::handlers가 이를 받아 자신의 스팬을
        // 여기에 이어붙이면, 오케스트레이터 로그/트레이스에서 "어느 워커의
        // 어느 register 시도가 이 요청을 만들었는지"를 추적할 수 있다.
        let mut req = self
            .http
            .post(&url)
            .headers(trace_context_headers())
            .json(&body);
        if let Some(token) = &self.config.worker.operational_token {
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
    #[tracing::instrument(skip(self))]
    pub async fn heartbeat_once(&self, agent_healthy: bool) -> Result<HeartbeatResponse, WorkerError> {
        let worker_id = self
            .worker_id
            .lock()
            .await
            .clone()
            .ok_or_else(|| WorkerError::OrchestratorApi("not registered yet".into()))?;

        // 빠른 시스템 메트릭 수집 (load_avg, mem, cpu, ram).
        let (load_avg, mem_available_mb, active_tasks, cpu_usage, ram_usage) = collect_fast_metrics();

        // 디스크 여유 공간: 캐시된 값을 사용하고, 필요하면 백그라운드 새로고침 트리거.
        // blocking syscall을 heartbeat 루프에서 분리하여 런타임 블로킹 방지.
        let disk_free_mb = self.disk_cache.get_or_schedule_refresh();

        // grok 버전 — 캐시된 값 사용 (변하지 않으므로 최초 1회만 수집).
        let grok_version = self
            .grok_version
            .get_or_init(|| self.detect_version())
            .clone();

        // OS 정보 — 캐시된 값 사용 (변하지 않으므로 최초 1회만 수집).
        let os_info = self
            .os_info
            .get_or_init(|| self.collect_system_info())
            .clone();

        let body = HeartbeatRequest {
            worker_id,
            active_tasks,
            load_avg,
            mem_available_mb,
            disk_free_mb,
            cpu_usage,
            ram_usage,
            agent_healthy,
            grok_version,
            fleet_worker_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            os_info,
        };

        let url = format!(
            "{}/v1/workers/heartbeat",
            self.config.worker.orchestrator_url
        );
        // 로드맵 #42 — register_once와 동일하게 트레이스 컨텍스트 전파.
        let mut req = self
            .http
            .post(&url)
            .headers(trace_context_headers())
            .json(&body);
        if let Some(token) = &self.config.worker.operational_token {
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
        let body: HeartbeatResponse = resp.json().await.map_err(WorkerError::Http)?;
        Ok(body)
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
            match self.heartbeat_once(agent_healthy).await {
                Ok(resp) => {
                    if resp.desired_state == "drain" {
                        info!("Worker is in Draining state by Orchestrator direction");
                    }
                }
                Err(e) => {
                    warn!(error = %e, "heartbeat failed — will retry next interval");
                }
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
        if let Some(token) = &self.config.worker.operational_token {
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

    /// grok CLI 버전을 감지하여 반환 (최초 1회만 실행, 이후 캐시).
    fn detect_version(&self) -> Option<String> {
        let grok_path = &self.config.grok.bin;
        detect_grok_version(grok_path)
    }

    /// OS 정보를 수집하여 반환 (최초 1회만 실행, 이후 캐시).
    fn collect_system_info(&self) -> Option<WorkerOsInfo> {
        Some(collect_os_info())
    }
}

/// 시스템 메트릭 수집 (sysinfo 사용).
///
/// `load_avg`, `mem_available_mb`는 매 호출마다 수집 (마이크로초 단위로 빠름).
/// `disk_free_mb`는 blocking syscall이며 환경에 따라 수 초가 소요될 수 있으므로
/// `DiskCache`를 통해 백그라운드에서 비동기 수집 및 캐싱.
///
/// active_tasks는 fleet-worker가 관리하는 실행 중인 세션 카운터를 반환.
fn collect_fast_metrics() -> (Option<Vec<f32>>, Option<u64>, u32, Option<f32>, Option<f32>) {
    use sysinfo::System;

    let mut sys = System::new();
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    // cpu usage calculations require two measurements with a delay or call interval.
    // Since this is run inside collect_fast_metrics, we can sleep briefly.
    std::thread::sleep(std::time::Duration::from_millis(50));
    sys.refresh_cpu_usage();
    let cpu_usage = sys.global_cpu_usage();

    // sysinfo 0.32에서 load_average는 associated function.
    let load_avg = System::load_average();
    // orchestrator API는 f32를 기대하므로 f64 → f32로 캐스팅 (손실 무시 가능).
    let load_vec = vec![
        load_avg.one as f32,
        load_avg.five as f32,
        load_avg.fifteen as f32,
    ];

    let mem_available_mb = sys.available_memory() / 1024; // KiB → MiB
    let total_mem = sys.total_memory();
    let ram_usage = if total_mem > 0 {
        let used_mem = total_mem.saturating_sub(sys.available_memory());
        Some((used_mem as f32 / total_mem as f32) * 100.0)
    } else {
        None
    };

    // active_tasks: 전역 세션 카운터에서 가져옴.
    let active_tasks = crate::ACTIVE_SESSIONS.load(std::sync::atomic::Ordering::Relaxed);

    (Some(load_vec), Some(mem_available_mb), active_tasks, Some(cpu_usage), ram_usage)
}

/// grok CLI 버전 감지. `grok --version` 출력에서 추출.
///
/// 출력 형식: `grok 0.2.103 (89c3d36fb6)`
/// 두 번째 토큰(버전 번호)을 추출한다. 실패 시 None.
fn detect_grok_version(grok_path: &str) -> Option<String> {
    let output = std::process::Command::new(grok_path)
        .arg("--version")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    // 첫 번째 줄에서 버전 번호 패턴(x.y.z) 추출.
    let line = combined.lines().next()?;
    let tokens: Vec<&str> = line.split_whitespace().collect();

    // "grok 0.2.103 (89c3d36fb6)" → 두 번째 토큰이 버전.
    for token in &tokens[1..] {
        let clean = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.');
        // 버전 패턴: 숫자로 시작하고 점을 포함.
        if clean
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
            && clean.contains('.')
        {
            return Some(clean.to_string());
        }
    }
    None
}

/// OS 정보 수집 (sysinfo 사용).
fn collect_os_info() -> WorkerOsInfo {
    use sysinfo::System;

    let os_type = System::name().unwrap_or_default();
    let distro = System::long_os_version().unwrap_or_default();
    let kernel = System::kernel_version().unwrap_or_default();
    let arch = std::env::consts::ARCH.to_string();
    let hostname = System::host_name().unwrap_or_default();

    WorkerOsInfo {
        os_type,
        distro,
        kernel,
        arch,
        hostname,
    }
}

/// 디스크 여유 공간 수집 (blocking).
///
/// macOS autofs 마운트 포인트 등 환경에 따라 수 초가 소요될 수 있으므로
/// `spawn_blocking` 컨텍스트에서만 호출해야 함.
fn collect_disk_free_mb() -> u64 {
    use sysinfo::Disks;

    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .map(|d| (d.total_space() - d.available_space()) / 1024 / 1024)
        .sum()
}

/// 디스크 여유 공간 캐시.
///
/// `Disks::new_with_refreshed_list()`는 blocking syscall이며 환경에 따라
/// 수 초가 소요될 수 있음 (예: macOS autofs 마운트 타임아웃).
/// 이 캐시는:
/// 1. heartbeat 루프를 블록하지 않도록 백그라운드 수집(`spawn_blocking`)을 트리거
/// 2. 60초 TTL로 빈도를 크게 낮춤 (디스크 용량은 자주 변하지 않음)
/// 3. 첫 heartbeat는 캐시가 비어 있으면 `None`을 반환 (orchestrator가 옵션 필드를 허용)
struct DiskCache {
    state: std::sync::Mutex<DiskCacheState>,
    /// 캐시 유효 기간. 테스트에서 짧은 주기를 검증하기 위해 오버라이드 가능.
    ttl: Duration,
}

enum DiskCacheState {
    /// 아직 수집되지 않음. 다음 heartbeat에서 백그라운드 수집 시작.
    Initial,
    /// 백그라운드 수집 진행 중. 중복 수집 방지.
    Refreshing,
    /// 수집 완료. TTL 만료 시 재수집.
    Ready {
        free_mb: u64,
        refreshed_at: std::time::Instant,
    },
}

impl DiskCache {
    const DEFAULT_TTL: Duration = Duration::from_secs(60);

    fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(DiskCacheState::Initial),
            ttl: Self::DEFAULT_TTL,
        }
    }

    #[cfg(test)]
    fn with_ttl(ttl: Duration) -> Self {
        Self {
            state: std::sync::Mutex::new(DiskCacheState::Initial),
            ttl,
        }
    }

    /// 캐시된 값을 반환. 만료되었거나 없으면 `None`.
    fn get(&self) -> Option<u64> {
        let state = self.state.lock().unwrap();
        match &*state {
            DiskCacheState::Ready {
                free_mb,
                refreshed_at,
            } if *refreshed_at + self.ttl > std::time::Instant::now() => Some(*free_mb),
            _ => None,
        }
    }

    /// 백그라운드 새로고침이 필요한지 확인하고, 필요하면 `Refreshing`로 전환.
    /// 반환값이 `true`이면 호출자가 `spawn_blocking`으로 수집을 시작해야 함.
    fn begin_refresh(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        let needs = match &*state {
            DiskCacheState::Initial => true,
            DiskCacheState::Refreshing => false,
            DiskCacheState::Ready { refreshed_at, .. } => {
                *refreshed_at + self.ttl <= std::time::Instant::now()
            }
        };
        if needs {
            *state = DiskCacheState::Refreshing;
        }
        needs
    }

    /// 수집 결과 저장.
    fn set(&self, free_mb: u64) {
        *self.state.lock().unwrap() = DiskCacheState::Ready {
            free_mb,
            refreshed_at: std::time::Instant::now(),
        };
    }

    /// 캐시된 값을 반환하고, 필요하면 백그라운드 새로고침을 트리거.
    /// 새로고침은 논블로킹으로 진행되므로 이 메서드는 즉시 반환.
    fn get_or_schedule_refresh(self: &Arc<Self>) -> Option<u64> {
        let cached = self.get();
        if self.begin_refresh() {
            let cache = self.clone();
            tokio::task::spawn_blocking(move || {
                let free = collect_disk_free_mb();
                cache.set(free);
            });
        }
        cached
    }
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
        /// register 요청의 Authorization 헤더 원문 (없으면 None) — operational_token이
        /// 실제로 bearer로 전송되는지 검증하는 데 쓴다 (로드맵 #60 8단계).
        register_auth_headers: Arc<TokioMutex<Vec<Option<String>>>>,
    }

    impl Default for MockState {
        fn default() -> Self {
            Self {
                registers: Arc::new(TokioMutex::new(Vec::new())),
                heartbeats: Arc::new(TokioMutex::new(Vec::new())),
                deregisters: Arc::new(TokioMutex::new(Vec::new())),
                register_status: Arc::new(TokioMutex::new(200)),
                register_auth_headers: Arc::new(TokioMutex::new(Vec::new())),
            }
        }
    }

    async fn start_mock_orchestrator(state: MockState) -> String {
        use axum::extract::Path;
        use axum::http::HeaderMap;
        use axum::routing::delete;

        let register_state = state.clone();
        let hb_state = state.clone();
        let dereg_state = state.clone();

        let app = Router::new()
            .route(
                "/v1/workers/register",
                post(move |headers: HeaderMap, Json(body): Json<Value>| {
                    let s = register_state.clone();
                    async move {
                        let auth = headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string);
                        s.register_auth_headers.lock().await.push(auth);
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
                            Json(serde_json::json!({"ok": true, "desired_state": "running"})),
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

    /// 로드맵 #60 8단계 회귀 테스트 — `worker.operational_token`이 실제로
    /// register/heartbeat/deregister의 Authorization bearer로 전송되는지 확인.
    /// (이 배선이 없으면 join이 발급한 credential이 조용히 무시되고, 워커는
    /// 인증 없이 요청을 보내다 보호된 orchestrator에서 매번 401을 받는다.)
    #[tokio::test]
    async fn operational_token_is_sent_as_register_bearer() {
        let state = MockState::default();
        let url = start_mock_orchestrator(state.clone()).await;

        let config = Arc::new(
            WorkerConfig::for_test()
                .orchestrator_url(url)
                .operational_token("fwo_test-secret")
                .build(),
        );
        let client = RegistrationClient::new(config).unwrap();
        client.register_once().await.unwrap();

        let headers = state.register_auth_headers.lock().await;
        assert_eq!(
            headers.last().cloned().flatten().as_deref(),
            Some("Bearer fwo_test-secret")
        );
    }

    #[tokio::test]
    async fn no_operational_token_sends_no_authorization_header() {
        let state = MockState::default();
        let url = start_mock_orchestrator(state.clone()).await;

        let config = Arc::new(WorkerConfig::for_test().orchestrator_url(url).build());
        let client = RegistrationClient::new(config).unwrap();
        client.register_once().await.unwrap();

        let headers = state.register_auth_headers.lock().await;
        assert_eq!(headers.last().cloned().flatten(), None);
    }

    /// 회귀 테스트: 비어있지 않은 labels가 JSON *배열*이 아니라 *객체*로 직렬화되는지
    /// 검증. fleet-api의 `RegisterRequest.labels: HashMap<String,String>`은 map만
    /// 받아들이므로, 과거 `Vec<(String,String)>` 구현은 라벨이 하나라도 있으면
    /// 실제 오케스트레이터에서 422로 거부됐다 (labels가 비어있을 때만
    /// `skip_serializing_if`로 필드 자체가 생략되어 우연히 가려져 있던 버그).
    #[tokio::test]
    async fn register_with_labels_serializes_as_json_object_not_array() {
        let state = MockState::default();
        let url = start_mock_orchestrator(state.clone()).await;

        let config = Arc::new(
            WorkerConfig::for_test()
                .orchestrator_url(url)
                .label("model", "gemini")
                .label("arch", "arm64")
                .build(),
        );
        let client = RegistrationClient::new(config).unwrap();
        client.register_once().await.unwrap();

        let registers = state.registers.lock().await;
        assert_eq!(registers.len(), 1);
        let labels = &registers[0]["labels"];
        assert!(
            labels.is_object(),
            "labels must serialize as a JSON object (map), got: {labels:?}"
        );
        assert_eq!(labels["model"], "gemini");
        assert_eq!(labels["arch"], "arm64");
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

    // ── DiskCache 단위 테스트 ──

    #[test]
    fn disk_cache_initial_get_returns_none() {
        let cache = DiskCache::new();
        assert!(cache.get().is_none());
    }

    #[test]
    fn disk_cache_begin_refresh_from_initial() {
        let cache = DiskCache::new();
        assert!(cache.begin_refresh());
        // Refreshing 상태 — 중복 수집 방지.
        assert!(!cache.begin_refresh());
    }

    #[test]
    fn disk_cache_set_then_get() {
        let cache = DiskCache::new();
        cache.set(12345);
        assert_eq!(cache.get(), Some(12345));
    }

    #[test]
    fn disk_cache_begin_refresh_after_ttl_expiry() {
        // TTL 0 → 즉시 만료.
        let cache = DiskCache::with_ttl(Duration::from_millis(0));
        cache.set(999);
        // TTL 0이므로 이미 만료 — begin_refresh는 true.
        assert!(cache.begin_refresh());
    }

    #[test]
    fn disk_cache_no_refresh_within_ttl() {
        let cache = DiskCache::with_ttl(Duration::from_secs(3600));
        cache.set(42);
        // TTL 내 — 새로고침 불필요.
        assert!(!cache.begin_refresh());
        assert_eq!(cache.get(), Some(42));
    }

    #[tokio::test]
    async fn disk_cache_get_or_schedule_refresh_populates_background() {
        let cache = Arc::new(DiskCache::new());
        // 첫 호출 — 캐시 비어 있음, 백그라운드 수집 트리거.
        assert!(cache.get_or_schedule_refresh().is_none());

        // 백그라운드 spawn_blocking 완료 대기 (최대 10초).
        for _ in 0..100 {
            if cache.get().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // 수집 완료 확인.
        assert!(cache.get().is_some(), "disk cache should be populated");

        // 두 번째 호출 — 캐시 hit, 추가 수집 트리거 안 함.
        assert!(cache.get_or_schedule_refresh().is_some());
    }

    // ── 로드맵 #42: register/heartbeat HTTP 요청의 traceparent 전파 ──────

    #[test]
    fn trace_context_headers_injects_active_span_trace_id() {
        use opentelemetry_sdk::testing::trace::InMemorySpanExporter;
        use opentelemetry_sdk::trace::TracerProvider;
        use tracing_subscriber::layer::SubscriberExt;

        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );

        let exporter = InMemorySpanExporter::default();
        let provider = TracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let tracer = opentelemetry::trace::TracerProvider::tracer(&provider, "test");
        let subscriber =
            tracing_subscriber::Registry::default().with(tracing_opentelemetry::layer().with_tracer(tracer));

        // 활성 스팬이 있을 때 — traceparent 헤더가 그 스팬의 trace-id를
        // 담아 나가야 한다.
        let headers = tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("register_once");
            let _guard = span.enter();
            trace_context_headers()
        });

        let traceparent = headers
            .get("traceparent")
            .expect("traceparent header should be present when called inside an active span")
            .to_str()
            .unwrap();

        // W3C 포맷: "00-<32자리 hex trace-id>-<16자리 hex span-id>-<flags>"
        let parts: Vec<&str> = traceparent.split('-').collect();
        assert_eq!(
            parts.len(),
            4,
            "unexpected traceparent format: {traceparent}"
        );
        assert_eq!(parts[0], "00", "version byte must be 00");
        assert_eq!(parts[1].len(), 32, "trace-id must be 32 hex chars");
        assert_ne!(
            parts[1],
            "0".repeat(32),
            "trace-id must not be all-zero (would mean no real span was active)"
        );
    }

    #[test]
    fn trace_context_headers_is_empty_without_active_span() {
        // OTel 레이어가 없는(=활성 스팬 컨텍스트가 없는) 기본 상태에서는
        // traceparent를 만들 게 없으므로 헤더가 비어 있어야 한다 — panic 없이
        // 조용한 no-op이어야 한다(OTLP 미설정 배포에서의 기본 동작).
        let headers = trace_context_headers();
        assert!(
            headers.get("traceparent").is_none(),
            "no traceparent should be injected without an active OTel span"
        );
    }
}
