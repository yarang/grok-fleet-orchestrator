//! HTTP 핸들러 구현.
//!
//! axum 라우터에 직접 연결되는 비동기 함수들. 비즈니스 로직은 Store를 경유하여
//! 실행되며, 핸들러 자체는 입력 검증 + 도메인 변환 + 응답 조립만 담당.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::Utc;
use tracing::{debug, info};
use uuid::Uuid;

use fleet_core::{Worker, WorkerFilter, WorkerHeartbeat, WorkerId, WorkerStatus};

use crate::app::AppState;
use crate::error::ApiError;
use crate::schema::{
    BootstrapTokenSummary, CreateBootstrapTokenRequest, CreateBootstrapTokenResponse,
    DeregisterRequest, HealthResponse, HeartbeatRequest, HeartbeatResponse, JoinRequest,
    JoinResponse, RegisterRequest, RegisterResponse, WorkerSummary,
};

/// `GET /v1/health` — 단순 헬스 프로브.
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// `POST /v1/workers/register`.
///
/// 신규 워커 등록 또는 재연결 처리:
/// 1. 동일 name이 존재하면 기존 레코드를 덮어씀 (재연결 시나리오)
/// 2. `existing_worker_id`가 있으면 해당 ID 유지
/// 3. last_seen을 now로 설정
/// 4. status를 Online으로 설정 (재등록 시 암묵적 복구)
pub async fn register_worker(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, ApiError> {
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name must not be empty".into()));
    }
    if req.agent_endpoint.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "agent_endpoint must not be empty".into(),
        ));
    }

    // DNS-safe 이름 검증 (간단한 버전)
    let name = req.name.trim();
    validate_worker_name(name)?;

    // 1. 기존 워커 조회 (name 기준 또는 existing_worker_id)
    let existing_by_name = state.store.get_worker_by_name(name).await?;

    let existing_by_id = if let Some(id_str) = &req.existing_worker_id {
        let uuid = Uuid::parse_str(id_str)
            .map_err(|e| ApiError::BadRequest(format!("invalid existing_worker_id: {e}")))?;
        state.store.get_worker(WorkerId(uuid)).await?
    } else {
        None
    };

    // 충돌 검증: 둘 다 존재하고 서로 다르면 ambiguous
    if let (Some(by_name), Some(by_id)) = (&existing_by_name, &existing_by_id) {
        if by_name.id != by_id.id {
            return Err(ApiError::Conflict(format!(
                "name '{name}' maps to worker {} but existing_worker_id points to {}",
                by_name.id, by_id.id
            )));
        }
    }

    let worker_id = existing_by_id
        .as_ref()
        .or(existing_by_name.as_ref())
        .map(|w| w.id)
        .unwrap_or_else(WorkerId::new);

    let worker = build_worker(
        worker_id,
        name,
        req.agent_endpoint.as_str(),
        req.labels.clone(),
        req.max_concurrent_tasks,
        req.worker_version.clone(),
        existing_by_name.as_ref().or(existing_by_id.as_ref()),
    );

    let worker_id = upsert_and_register(&state, &worker).await?;

    // 4. WorkerJoined 이벤트 (재등록인지 신규인지 구분)
    let now = Utc::now();
    let is_new = existing_by_name.is_none() && existing_by_id.is_none();
    let event = if is_new {
        info!(%worker_id, name = %worker.name, "worker registered");
        fleet_core::FleetEvent::worker_joined(worker_id, &worker.name, &worker.endpoint)
    } else {
        info!(%worker_id, name = %worker.name, "worker re-registered");
        fleet_core::FleetEvent::WorkerHeartbeat {
            worker_id,
            active_tasks: 0,
            agent_healthy: true,
            at: now,
        }
    };
    let _ = state.store.append_event(&event).await;

    Ok(Json(RegisterResponse {
        worker_id: worker_id.to_string(),
        heartbeat_interval_secs: state.heartbeat_interval_secs,
        config_revision: 1,
        orchestrator_version: env!("CARGO_PKG_VERSION"),
        status: "online",
    }))
}

/// `POST /v1/workers/heartbeat`.
pub async fn heartbeat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<HeartbeatRequest>,
) -> Result<Json<HeartbeatResponse>, ApiError> {
    let worker_id = Uuid::parse_str(&req.worker_id)
        .map_err(|e| ApiError::BadRequest(format!("invalid worker_id: {e}")))?;

    let worker_id = WorkerId(worker_id);

    // 존재 확인
    let worker = state
        .store
        .get_worker(worker_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("worker {worker_id}")))?;

    // 하트비트 갱신
    let hb = WorkerHeartbeat {
        worker_id,
        active_tasks: req.active_tasks,
        load_avg: req.load_avg.clone(),
        mem_available_mb: req.mem_available_mb,
        disk_free_mb: req.disk_free_mb,
        agent_healthy: req.agent_healthy,
    };
    state.store.update_worker_heartbeat(worker_id, &hb).await?;

    // health가 true면 status를 Online으로 승격 (오프라인이었던 경우 복구)
    // agent가 unhealthy면 Degraded로 전환 (단, Offline은 건드리지 않음)
    let new_status = if req.agent_healthy {
        Some(WorkerStatus::Online)
    } else {
        Some(WorkerStatus::Degraded)
    };
    if let Some(new) = new_status {
        if worker.status != new {
            let mut updated = worker.clone();
            updated.status = new;
            state.store.upsert_worker(&updated).await?;
            debug!(%worker_id, ?worker.status, new = ?updated.status, "status updated via heartbeat");
        }
    }

    // WorkerHeartbeat 이벤트
    let _ = state
        .store
        .append_event(&fleet_core::FleetEvent::WorkerHeartbeat {
            worker_id,
            active_tasks: req.active_tasks,
            agent_healthy: req.agent_healthy,
            at: Utc::now(),
        })
        .await;

    debug!(%worker_id, active = req.active_tasks, healthy = req.agent_healthy, "heartbeat");

    Ok(Json(HeartbeatResponse {
        ok: true,
        desired_state: "running",
        server_time: Utc::now(),
    }))
}

/// `GET /v1/workers` — 워커 목록. 쿼리 파라미터로 필터링.
#[derive(Debug, serde::Deserialize)]
pub struct ListWorkersQuery {
    pub status: Option<String>,
    /// `labels`는 `key=value` 형태의 반복 파라미터로 받음.
    /// axum Query는 단순한 구조체만 지원하므로 여기서는 label_key/label_value 쌍을 쓰지 않고
    /// 단순화: `?label_arch=arm64` 같은 접두사 폼.
    #[serde(flatten)]
    pub label_filters: HashMap<String, String>,
}

pub async fn list_workers(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListWorkersQuery>,
) -> Result<Json<Vec<WorkerSummary>>, ApiError> {
    // label_filters에서 status 키를 빼고 나머지는 라벨로 처리
    let mut filter = WorkerFilter::default();
    let mut labels = HashMap::new();

    if let Some(s) = query.status {
        filter.status = Some(parse_status(&s)?);
    }
    for (k, v) in query.label_filters {
        if k != "status" {
            labels.insert(k, v);
        }
    }
    if !labels.is_empty() {
        filter.labels = labels;
    }

    let workers = state.store.list_workers(&filter).await?;
    let summaries = workers.iter().map(worker_to_summary).collect();
    Ok(Json(summaries))
}

/// `GET /v1/workers/:id`.
pub async fn get_worker(
    State(state): State<Arc<AppState>>,
    Path(id_str): Path<String>,
) -> Result<Json<WorkerSummary>, ApiError> {
    let uuid = Uuid::parse_str(&id_str)
        .map_err(|e| ApiError::BadRequest(format!("invalid worker_id: {e}")))?;
    let worker = state
        .store
        .get_worker(WorkerId(uuid))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("worker {id_str}")))?;
    Ok(Json(worker_to_summary(&worker)))
}

/// `DELETE /v1/workers/:id`.
pub async fn deregister_worker(
    State(state): State<Arc<AppState>>,
    Path(id_str): Path<String>,
    body: Option<Json<DeregisterRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let uuid = Uuid::parse_str(&id_str)
        .map_err(|e| ApiError::BadRequest(format!("invalid worker_id: {e}")))?;
    let worker_id = WorkerId(uuid);

    let reason = body
        .and_then(|Json(b)| b.reason)
        .unwrap_or_else(|| "deregistered by admin".to_string());

    let worker = state
        .store
        .get_worker(worker_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("worker {id_str}")))?;

    // 이벤트 먼저 발행 (삭제 전에 이름 보존)
    let _ = state
        .store
        .append_event(&fleet_core::FleetEvent::worker_left(worker_id, &reason))
        .await;

    // Transport에서 워커 제거 (설정된 경우).
    // best-effort: 실패해도 Store 삭제는 진행.
    if let Some(transport) = &state.transport {
        if let Err(e) = transport.unregister(worker_id).await {
            tracing::warn!(
                %worker_id,
                error = %e,
                "transport.unregister failed — proceeding with Store delete"
            );
        }
    }

    state.store.delete_worker(worker_id).await?;

    info!(%worker_id, name = %worker.name, reason = %reason, "worker deregistered");
    Ok(Json(serde_json::json!({
        "worker_id": id_str,
        "status": "deregistered",
        "reason": reason,
    })))
}

// ── 헬퍼 ────────────────────────────────────────────────────────────────

/// `POST /v1/workers/join` — 부트스트랩 토큰으로 신규 워커 등록 (Phase 8.3).
///
/// `/register`와 달리:
/// - 요청 본문의 `token`을 Store.consume_bootstrap_token으로 atomic하게 검증.
///   (인증 미들웨어의 bearer token과 별개)
/// - 응답에 worker_config_toml을 포함하여 클라이언트가 디스크에 바로 기록 가능.
/// - 항상 신규 worker_id 발급 (재등록은 `/register` 사용).
pub async fn join_worker(
    State(state): State<Arc<AppState>>,
    Json(req): Json<JoinRequest>,
) -> Result<Json<JoinResponse>, ApiError> {
    if req.token.trim().is_empty() {
        return Err(ApiError::BadRequest("token must not be empty".into()));
    }
    let name = req.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("name must not be empty".into()));
    }
    validate_worker_name(name)?;
    if req.agent_endpoint.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "agent_endpoint must not be empty".into(),
        ));
    }

    // 1. 부트스트랩 토큰 atomic 소비.
    if let Err(e) = state.store.consume_bootstrap_token(&req.token, name).await {
        match e {
            fleet_store::StoreError::BootstrapTokenInvalid(msg) => {
                return Err(ApiError::Unauthorized(format!(
                    "bootstrap token rejected: {msg}"
                )));
            }
            other => return Err(other.into()),
        }
    }

    // 2. 동일 name이 이미 존재하면 거부 — join은 항상 신규.
    if let Some(_existing) = state.store.get_worker_by_name(name).await? {
        return Err(ApiError::Conflict(format!(
            "worker name '{name}' already exists — use POST /v1/workers/register to re-register"
        )));
    }

    // 3. Worker 엔티티 생성 + 등록.
    let worker_id = WorkerId::new();
    let worker = build_worker(
        worker_id,
        name,
        req.agent_endpoint.as_str(),
        req.labels.clone(),
        req.max_concurrent_tasks,
        req.worker_version.clone(),
        None,
    );
    let worker_id = upsert_and_register(&state, &worker).await?;

    info!(%worker_id, name = %worker.name, "worker joined via bootstrap token");
    let _ = state
        .store
        .append_event(&fleet_core::FleetEvent::worker_joined(
            worker_id,
            &worker.name,
            &worker.endpoint,
        ))
        .await;

    // 4. worker.toml 렌더링.
    let worker_config_toml = render_worker_config_toml(
        name,
        &req.agent_endpoint,
        &req.labels,
        &req.token,
        worker_id,
        state.heartbeat_interval_secs,
        req.max_concurrent_tasks,
    );

    Ok(Json(JoinResponse {
        worker_id: worker_id.to_string(),
        heartbeat_interval_secs: state.heartbeat_interval_secs,
        config_revision: 1,
        orchestrator_version: env!("CARGO_PKG_VERSION"),
        status: "online",
        worker_config_toml,
    }))
}

/// `POST /v1/bootstrap-tokens` — 어드민이 부트스트랩 토큰 발급.
pub async fn create_bootstrap_token(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateBootstrapTokenRequest>,
) -> Result<Json<CreateBootstrapTokenResponse>, ApiError> {
    if !(8..=256).contains(&req.bytes) {
        return Err(ApiError::BadRequest(format!(
            "bytes must be between 8 and 256 (got {})",
            req.bytes
        )));
    }
    if req.max_uses == 0 {
        return Err(ApiError::BadRequest("max_uses must be >= 1".into()));
    }
    if req
        .prefix
        .chars()
        .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
    {
        return Err(ApiError::BadRequest(
            "prefix must be alphanumeric, '_', or '-' only".into(),
        ));
    }

    let raw = generate_random_bytes(req.bytes)
        .map_err(|e| ApiError::Internal(format!("CSPRNG failure: {e}")))?;
    let encoded = base64url(&raw);
    let token = if req.prefix.is_empty() {
        encoded
    } else {
        format!("{}_{}", req.prefix, encoded)
    };
    let now = Utc::now();
    let expires_at = req
        .expires_in_secs
        .map(|s| now + chrono::Duration::seconds(s as i64));

    let bt = fleet_core::BootstrapToken {
        token: token.clone(),
        created_at: now,
        created_by: req.created_by.clone(),
        expires_at,
        max_uses: req.max_uses,
        use_count: 0,
        notes: req.notes.clone(),
        last_used_by: None,
        last_used_at: None,
    };
    state.store.create_bootstrap_token(&bt).await?;

    info!(token_prefix = %req.prefix, max_uses = req.max_uses, "bootstrap token issued");
    Ok(Json(CreateBootstrapTokenResponse {
        token,
        created_at: now,
        expires_at,
        max_uses: req.max_uses,
    }))
}

/// `GET /v1/bootstrap-tokens` — 발급된 토큰 목록.
pub async fn list_bootstrap_tokens(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<BootstrapTokenSummary>>, ApiError> {
    let tokens = state.store.list_bootstrap_tokens().await?;
    Ok(Json(
        tokens
            .into_iter()
            .map(BootstrapTokenSummary::from)
            .collect(),
    ))
}

/// `DELETE /v1/bootstrap-tokens/:token` — 토큰 회수.
pub async fn revoke_bootstrap_token(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let revoked = state.store.revoke_bootstrap_token(&token).await?;
    if !revoked {
        return Err(ApiError::NotFound(format!(
            "bootstrap token not found: {token}"
        )));
    }
    info!("bootstrap token revoked");
    Ok(Json(serde_json::json!({
        "status": "revoked",
        "token": token,
    })))
}

/// worker.toml 문자열 렌더링.
///
/// 클라이언트가 받아서 그대로 디스크에 기록할 수 있는 TOML을 생성.
/// `[worker] existing_worker_id`를 포함하여, 이후 재시작 시 동일 ID로 재등록 가능.
fn render_worker_config_toml(
    name: &str,
    agent_endpoint: &str,
    labels: &HashMap<String, String>,
    bootstrap_token: &str,
    worker_id: WorkerId,
    heartbeat_interval_secs: u32,
    max_concurrent_tasks: u32,
) -> String {
    // server-key 시크릿 추출 (agent_endpoint에 포함된 경우).
    let grok_secret = agent_endpoint
        .find("server-key=")
        .map(|i| {
            let start = i + "server-key=".len();
            let rest = &agent_endpoint[start..];
            let end = rest.find(['&', '#']).unwrap_or(rest.len());
            &rest[..end]
        })
        .unwrap_or("<replace-with-grok-secret>");

    // bind_addr 추출 시도 (endpoint에서 host:port).
    let bind_addr = agent_endpoint
        .split("://")
        .nth(1)
        .map(|rest| {
            let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            &rest[..host_end]
        })
        .filter(|s| !s.is_empty())
        .unwrap_or("127.0.0.1:2419");

    let mut out = String::new();
    out.push_str("# worker.toml — generated by fleet orchestrator (Phase 8.3 join)\n\n");

    out.push_str("[worker]\n");
    out.push_str(&format!("name = \"{name}\"\n"));
    out.push_str("orchestrator_url = \"<set-to-your-orchestrator-url>\"\n");
    out.push_str(&format!(
        "heartbeat_interval_secs = {heartbeat_interval_secs}\n"
    ));
    out.push_str(&format!("bootstrap_token = \"{bootstrap_token}\"\n"));
    out.push_str(&format!("existing_worker_id = \"{worker_id}\"\n"));
    if !labels.is_empty() {
        let mut sorted: Vec<_> = labels.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        let pairs: Vec<String> = sorted
            .iter()
            .map(|(k, v)| format!("{k} = \"{v}\""))
            .collect();
        out.push_str(&format!("labels = {{ {} }}\n", pairs.join(", ")));
    }
    out.push_str("\n[grok]\n");
    out.push_str("bin = \"/usr/local/bin/grok\"\n");
    out.push_str(&format!("bind_addr = \"{bind_addr}\"\n"));
    out.push_str(&format!("secret = \"{grok_secret}\"\n"));
    out.push_str(&format!("max_concurrent_tasks = {max_concurrent_tasks}\n"));
    out.push_str("restart_delay_secs = 5\n");

    out
}

/// 운영체제 CSPRNG에서 n 바이트 읽기.
fn generate_random_bytes(n: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    #[cfg(unix)]
    {
        let mut f = std::fs::File::open("/dev/urandom")?;
        f.read_exact(&mut buf)?;
    }
    #[cfg(not(unix))]
    {
        let mut filled = 0;
        while filled < n {
            let id = uuid::Uuid::new_v4();
            let b = id.as_bytes();
            let take = (n - filled).min(b.len());
            buf[filled..filled + take].copy_from_slice(&b[..take]);
            filled += take;
        }
    }
    Ok(buf)
}

/// base64url-no-pad 인코딩.
fn base64url(input: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((input.len() * 4).div_ceil(3));
    let mut chunks = input.chunks_exact(3);
    for c in &mut chunks {
        let n = ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | (c[2] as u32);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(n & 0x3F) as usize] as char);
    }
    let rem = chunks.remainder();
    match rem.len() {
        1 => {
            let n = (rem[0] as u32) << 16;
            out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        }
        2 => {
            let n = ((rem[0] as u32) << 16) | ((rem[1] as u32) << 8);
            out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        }
        _ => {}
    }
    out
}

// ── 기존 헬퍼 ────────────────────────────────────────────────────────────

/// DNS-safe 워커 이름 검증.
fn validate_worker_name(name: &str) -> Result<(), ApiError> {
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(ApiError::BadRequest(
            "name must be alphanumeric, '-', '_', or '.' only".into(),
        ));
    }
    Ok(())
}

/// `Worker` 엔티티 생성. 기존 워커가 있으면 registered_at을 유지.
fn build_worker(
    worker_id: WorkerId,
    name: &str,
    endpoint: &str,
    labels: HashMap<String, String>,
    max_concurrent: u32,
    worker_version: Option<String>,
    existing: Option<&Worker>,
) -> Worker {
    let now = Utc::now();
    let registered_at = existing.map(|w| w.registered_at).unwrap_or(now);
    Worker {
        id: worker_id,
        name: name.to_string(),
        endpoint: endpoint.to_string(),
        labels,
        status: WorkerStatus::Online,
        last_seen: Some(now),
        active_tasks: 0,
        max_concurrent,
        circuit_state: fleet_core::CircuitState::Closed,
        worker_version,
        registered_at,
    }
}

/// Store upsert + transport.register 호출. transport 실패는 warn 로그만.
async fn upsert_and_register(state: &AppState, worker: &Worker) -> Result<WorkerId, ApiError> {
    state.store.upsert_worker(worker).await?;
    if let Some(transport) = &state.transport {
        if let Err(e) = transport
            .register(worker.id, &worker.endpoint, worker.max_concurrent)
            .await
        {
            tracing::warn!(
                worker_id = %worker.id,
                endpoint = %worker.endpoint,
                max_concurrent = worker.max_concurrent,
                error = %e,
                "transport.register failed — worker is in Store but cannot accept tasks until healthy"
            );
        }
    }
    Ok(worker.id)
}

fn parse_status(s: &str) -> Result<WorkerStatus, ApiError> {
    match s {
        "online" => Ok(WorkerStatus::Online),
        "degraded" => Ok(WorkerStatus::Degraded),
        "offline" => Ok(WorkerStatus::Offline),
        "circuit_open" => Ok(WorkerStatus::CircuitOpen),
        other => Err(ApiError::BadRequest(format!(
            "invalid status '{other}': expected online, degraded, offline, or circuit_open"
        ))),
    }
}

fn worker_to_summary(w: &Worker) -> WorkerSummary {
    WorkerSummary {
        id: w.id.to_string(),
        name: w.name.clone(),
        endpoint: w.endpoint.clone(),
        status: WorkerSummary::status_str(w.status).to_string(),
        labels: w.labels.clone(),
        active_tasks: w.active_tasks,
        max_concurrent: w.max_concurrent,
        circuit_state: format!("{:?}", w.circuit_state).to_lowercase(),
        last_seen: w.last_seen,
        registered_at: w.registered_at,
    }
}

// 사용하지 않을 수 있는 import 정리 — warning 방지
