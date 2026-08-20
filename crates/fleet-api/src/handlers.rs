//! HTTP 핸들러 구현.
//!
//! axum 라우터에 직접 연결되는 비동기 함수들. 비즈니스 로직은 Store를 경유하여
//! 실행되며, 핸들러 자체는 입력 검증 + 도메인 변환 + 응답 조립만 담당.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use opentelemetry::propagation::Extractor;
use tracing::{debug, info};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

use fleet_core::audit::{action, AuditEvent};
use fleet_core::{Worker, WorkerFilter, WorkerHeartbeat, WorkerId, WorkerStatus};

use crate::app::{AppState, AuthorizationContext};
use crate::error::ApiError;
use crate::schema::{
    BootstrapTokenSummary, CreateBootstrapTokenRequest, CreateBootstrapTokenResponse,
    CredentialSummary, DeregisterRequest, ExportedCredential, HealthResponse, HeartbeatRequest,
    HeartbeatResponse, HostRegisterRequest, HostRegisterResponse, JoinRequest, JoinResponse,
    PutCredentialRequest, PutCredentialResponse, RegisterRequest, RegisterResponse,
    RotateWorkerCredentialRequest, RotateWorkerCredentialResponse, WorkerSummary,
};

/// `GET /v1/health` — 단순 헬스 프로브.
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// `axum::http::HeaderMap`에서 W3C Trace Context(`traceparent`/`tracestate`)를
/// 읽기 위한 `opentelemetry::propagation::Extractor` 어댑터 (로드맵 #42).
struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// 들어온 요청의 `traceparent`/`tracestate` 헤더(있으면)를 현재(`#[instrument]`가
/// 만든) 스팬의 부모 컨텍스트로 잇는다 (로드맵 #42 — `fleet-worker`↔오케스트레이터
/// register/heartbeat 경로 한정, 다른 HTTP API 라우트에는 적용하지 않음). 헤더가
/// 없거나 파싱에 실패하면 조용히 아무 것도 하지 않는다 — 이 스팬은 그냥 로컬
/// 루트 스팬으로 남는다. `fleet-cli::logging::init()`이 전역 propagator를
/// 등록하지 않은 상태(테스트 등)에서도 no-op이라 안전하다.
fn continue_trace_from_headers(headers: &HeaderMap) {
    let parent_cx = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(headers))
    });
    tracing::Span::current().set_parent(parent_cx);
}

/// `POST /v1/workers/register`.
///
/// 신규 워커 등록 또는 재연결 처리:
/// 1. 동일 name이 존재하면 기존 레코드를 덮어씀 (재연결 시나리오)
/// 2. `existing_worker_id`가 있으면 해당 ID 유지
/// 3. last_seen을 now로 설정
/// 4. status를 Online으로 설정 (재등록 시 암묵적 복구)
#[tracing::instrument(skip(state, req, headers), fields(worker_name = %req.name))]
pub async fn register_worker(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AuthorizationContext>>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, ApiError> {
    // 로드맵 #42 — fleet-worker가 실어 보낸 traceparent가 있으면 이 스팬을
    // 거기 잇는다.
    continue_trace_from_headers(&headers);

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

    // Worker self-binding (로드맵 #60): operational credential로 인증된 요청은
    // 자기 자신의 worker_id만 (재)등록할 수 있다.
    enforce_worker_self_binding(ctx.as_deref(), worker_id)?;

    let worker = build_worker(
        NewWorkerParams {
            worker_id,
            name,
            endpoint: req.agent_endpoint.as_str(),
            labels: req.labels.clone(),
            max_concurrent: req.max_concurrent_tasks,
            worker_version: req.worker_version.clone(),
            liveness_mode: req.liveness_mode,
        },
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
#[tracing::instrument(skip(state, req, headers), fields(worker_id = %req.worker_id, active_tasks = req.active_tasks))]
pub async fn heartbeat(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AuthorizationContext>>,
    headers: HeaderMap,
    Json(req): Json<HeartbeatRequest>,
) -> Result<Json<HeartbeatResponse>, ApiError> {
    // 로드맵 #42 — register_worker와 동일하게 트레이스 컨텍스트 연결.
    continue_trace_from_headers(&headers);

    let worker_id = Uuid::parse_str(&req.worker_id)
        .map_err(|e| ApiError::BadRequest(format!("invalid worker_id: {e}")))?;

    let worker_id = WorkerId(worker_id);

    // Worker self-binding (로드맵 #60): operational credential로 인증된 요청은
    // 자기 자신의 heartbeat만 보낼 수 있다.
    enforce_worker_self_binding(ctx.as_deref(), worker_id)?;

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
        cpu_usage: req.cpu_usage,
        ram_usage: req.ram_usage,
        agent_healthy: req.agent_healthy,
        grok_version: req.grok_version.clone(),
        fleet_worker_version: req.fleet_worker_version.clone(),
        os_info: req.os_info.as_ref().map(|oi| fleet_core::OsInfo {
            os_type: oi.os_type.clone(),
            distro: oi.distro.clone(),
            kernel: oi.kernel.clone(),
            arch: oi.arch.clone(),
            hostname: oi.hostname.clone(),
        }),
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
        if worker.status != WorkerStatus::Draining && worker.status != new {
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

    // 호스트 인벤토리 동기화 — heartbeat로 수신된 버전/OS 정보를 hosts 테이블에 upsert.
    // 워커 name을 hostname으로 사용.
    let hostname = hb
        .os_info
        .as_ref()
        .filter(|oi| !oi.hostname.is_empty())
        .map(|oi| oi.hostname.clone())
        .unwrap_or_else(|| worker.name.clone());
    let host = fleet_core::Host {
        id: uuid::Uuid::new_v4(),
        hostname,
        worker_id: Some(worker_id),
        status: fleet_core::HostStatus::Online,
        ssh_host: None,
        ssh_port: 22,
        ssh_user: None,
        grok_version: req.grok_version.clone(),
        fleet_worker_version: req.fleet_worker_version.clone(),
        os_info: hb.os_info.clone(),
        metrics: fleet_core::HostMetrics {
            load_avg: req.load_avg.clone(),
            mem_available_mb: Some(req.mem_available_mb),
            disk_free_mb: Some(req.disk_free_mb),
            cpu_usage: req.cpu_usage,
            ram_usage: req.ram_usage,
        },
        last_heartbeat_at: Some(Utc::now()),
        provisioned_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let _ = state.store.upsert_host(&host).await;

    debug!(%worker_id, active = req.active_tasks, healthy = req.agent_healthy, "heartbeat");

    // 자가 드레인 평가: CPU나 RAM 사용량이 90%를 초과한 경우 또는 이미 Draining인 경우
    let mut is_overloaded = false;
    if let Some(cpu) = req.cpu_usage {
        if cpu > 90.0 {
            is_overloaded = true;
        }
    }
    if let Some(ram) = req.ram_usage {
        if ram > 90.0 {
            is_overloaded = true;
        }
    }

    let desired_state = if is_overloaded || worker.status == WorkerStatus::Draining {
        if worker.status != WorkerStatus::Draining {
            let mut updated = worker.clone();
            updated.status = WorkerStatus::Draining;
            state.store.upsert_worker(&updated).await?;
            tracing::info!(worker_id = %worker_id, "Worker CPU/RAM overloaded; transitioning to Draining");
        }
        "drain"
    } else {
        "running"
    };

    Ok(Json(HeartbeatResponse {
        ok: true,
        desired_state,
        server_time: Utc::now(),
    }))
}

/// `POST /v1/hosts/register` — 프로비저닝 완료 후 CLI가 호출하여 호스트를 등록.
///
/// 호스트를 upsert하고 프로비저닝 결과를 host_events에 기록한다.
pub async fn register_host(
    State(state): State<Arc<AppState>>,
    Json(req): Json<HostRegisterRequest>,
) -> Result<Json<HostRegisterResponse>, ApiError> {
    let host_id = uuid::Uuid::new_v4();
    let now = Utc::now();

    let host = fleet_core::Host {
        id: host_id,
        hostname: req.hostname.clone(),
        worker_id: None,
        status: if req.succeeded {
            fleet_core::HostStatus::Provisioned
        } else {
            fleet_core::HostStatus::Failed
        },
        ssh_host: Some(req.ssh_host.clone()),
        ssh_port: req.ssh_port,
        ssh_user: Some(req.ssh_user.clone()),
        grok_version: None,
        fleet_worker_version: None,
        os_info: None,
        metrics: fleet_core::HostMetrics::default(),
        last_heartbeat_at: None,
        provisioned_at: if req.succeeded { Some(now) } else { None },
        created_at: now,
        updated_at: now,
    };

    state.store.upsert_host(&host).await?;

    // 프로비저닝 이벤트 기록.
    let event = fleet_core::HostEvent {
        id: uuid::Uuid::new_v4(),
        host_id,
        event_type: if req.succeeded {
            "provision_ok".to_string()
        } else {
            "provision_fail".to_string()
        },
        severity: if req.succeeded {
            fleet_core::EventSeverity::Info
        } else {
            fleet_core::EventSeverity::Error
        },
        message: req.message.clone(),
        payload: std::collections::HashMap::new(),
        created_at: now,
    };
    let _ = state.store.append_host_event(&event).await;

    debug!(hostname = %req.hostname, succeeded = req.succeeded, "host registered");

    Ok(Json(HostRegisterResponse {
        ok: true,
        host_id: host_id.to_string(),
    }))
}

/// `GET /v1/workers` — 워커 목록. 쿼리 파라미터로 필터링.
#[derive(Debug, serde::Deserialize)]
pub struct ListWorkersQuery {
    pub status: Option<String>,
    /// 페이지 크기 (미지정 시 `WorkerFilter` 기본값).
    pub limit: Option<usize>,
    /// 건너뛸 행 수 (페이지네이션).
    pub offset: Option<usize>,
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
    let mut filter = WorkerFilter::default();
    let mut labels = HashMap::new();

    if let Some(s) = query.status {
        filter.status = Some(parse_status(&s)?);
    }
    if let Some(limit) = query.limit {
        filter.limit = limit;
    }
    if let Some(offset) = query.offset {
        filter.offset = offset;
    }
    for (k, v) in query.label_filters {
        if let Some(clean_key) = k.strip_prefix("label_") {
            if !clean_key.is_empty() {
                labels.insert(clean_key.to_string(), v);
            }
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
    ctx: Option<Extension<AuthorizationContext>>,
    Path(id_str): Path<String>,
    body: Option<Json<DeregisterRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let uuid = Uuid::parse_str(&id_str)
        .map_err(|e| ApiError::BadRequest(format!("invalid worker_id: {e}")))?;
    let worker_id = WorkerId(uuid);

    // Worker self-binding (로드맵 #60): operational credential로 인증된 요청은
    // 자기 자신만 등록 해제할 수 있다.
    enforce_worker_self_binding(ctx.as_deref(), worker_id)?;

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

/// `POST /v1/workers/:id/credential/rotate` — 관리자가 worker의 operational
/// credential을 새로 발급하고 이전 값을 즉시 무효화한다 (로드맵 #60 6단계).
///
/// `PermissionKind::WorkerCredentialManage` 필요 — worker operational
/// credential 인증(`worker:self`)에는 이 capability를 부여하지 않으므로,
/// worker 스스로 자기 credential을 회전시킬 수 없다(관리자 전용).
pub async fn rotate_worker_credential(
    State(state): State<Arc<AppState>>,
    Path(id_str): Path<String>,
    body: Option<Json<RotateWorkerCredentialRequest>>,
) -> Result<Json<RotateWorkerCredentialResponse>, ApiError> {
    let uuid = Uuid::parse_str(&id_str)
        .map_err(|e| ApiError::BadRequest(format!("invalid worker_id: {e}")))?;
    let worker_id = WorkerId(uuid);

    state
        .store
        .get_worker(worker_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("worker {id_str}")))?;

    let expires_in_secs = body.and_then(|Json(b)| b.expires_in_secs);
    let expires_at = expires_in_secs.map(|s| Utc::now() + chrono::Duration::seconds(s as i64));

    let new_token = format!(
        "fwo_{}",
        base64url(
            &generate_random_bytes(32)
                .map_err(|error| ApiError::Internal(format!("CSPRNG failure: {error}")))?,
        ),
    );
    let new_digest = fleet_core::BootstrapToken::digest_for(&new_token);

    let credential = state
        .store
        .rotate_worker_operational_credential(worker_id, &new_digest, expires_at)
        .await
        .map_err(|e| match e {
            fleet_store::StoreError::NotFound => {
                ApiError::NotFound(format!("no operational credential for worker {id_str}"))
            }
            other => other.into(),
        })?;

    info!(%worker_id, rotation_generation = credential.rotation_generation, "worker operational credential rotated — previous credential invalidated");

    Ok(Json(RotateWorkerCredentialResponse {
        worker_id: worker_id.to_string(),
        operational_token: new_token,
        rotation_generation: credential.rotation_generation,
        issued_at: credential.issued_at,
        expires_at: credential.expires_at,
    }))
}

/// `DELETE /v1/workers/:id/credential` — worker의 operational credential을
/// 즉시 회수한다 (로드맵 #60 6단계). 회수 뒤에는 이전 토큰으로 register/
/// heartbeat/deregister 모두 거부된다. Worker 자체는 삭제하지 않는다 —
/// worker 엔티티 삭제는 `DELETE /v1/workers/:id`(`deregister_worker`)의 역할.
pub async fn revoke_worker_credential(
    State(state): State<Arc<AppState>>,
    Path(id_str): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let uuid = Uuid::parse_str(&id_str)
        .map_err(|e| ApiError::BadRequest(format!("invalid worker_id: {e}")))?;
    let worker_id = WorkerId(uuid);

    let revoked = state
        .store
        .revoke_worker_operational_credential(worker_id)
        .await?;
    if !revoked {
        return Err(ApiError::NotFound(format!(
            "no active operational credential for worker {id_str}"
        )));
    }

    info!(%worker_id, "worker operational credential revoked");
    Ok(Json(serde_json::json!({
        "worker_id": id_str,
        "status": "revoked",
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
#[tracing::instrument(skip(state, req), fields(worker_name = %req.name))]
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

    // 1. 동일 name이 이미 존재하면 조기 거부 — 사용자에게 더 명확한 메시지를
    //    주기 위한 best-effort 사전 검사. 이 검사와 아래 `enroll_worker` 사이의
    //    TOCTOU race는 `enroll_worker`의 원자적 이름/digest 검사가 최종적으로
    //    막는다 (그 경우 일반 Conflict 메시지로 반환됨).
    if let Some(_existing) = state.store.get_worker_by_name(name).await? {
        return Err(ApiError::Conflict(format!(
            "worker name '{name}' already exists — use POST /v1/workers/register to re-register"
        )));
    }

    // 2. Worker 엔티티 + operational credential 준비 (아직 저장하지 않음).
    let worker_id = WorkerId::new();
    let worker = build_worker(
        NewWorkerParams {
            worker_id,
            name,
            endpoint: req.agent_endpoint.as_str(),
            labels: req.labels.clone(),
            max_concurrent: req.max_concurrent_tasks,
            worker_version: req.worker_version.clone(),
            liveness_mode: req.liveness_mode,
        },
        None,
    );

    // Bootstrap token은 join 승인에만 쓰며, 이후 Worker는 별도 operational credential을
    // 사용한다. 저장소에는 원문 대신 digest만 보관한다.
    let operational_token = format!(
        "fwo_{}",
        base64url(
            &generate_random_bytes(32)
                .map_err(|error| ApiError::Internal(format!("CSPRNG failure: {error}")))?,
        ),
    );
    let credential = fleet_store::WorkerOperationalCredential {
        worker_id,
        credential_digest: fleet_core::BootstrapToken::digest_for(&operational_token),
        issued_at: Utc::now(),
        expires_at: None,
        revoked_at: None,
        rotation_generation: 1,
    };

    // 3. bootstrap 토큰 소비 + worker 생성 + credential 저장을 하나의 단위로 실행
    //    (로드맵 #60). 셋 중 하나라도 실패하면 아무 것도 반영되지 않는다 —
    //    토큰은 소비되지 않고 worker도 생성되지 않는다.
    if let Err(e) = state
        .store
        .enroll_worker(&req.token, name, &worker, &credential)
        .await
    {
        match e {
            fleet_store::StoreError::BootstrapTokenInvalid(msg) => {
                return Err(ApiError::Unauthorized(format!(
                    "bootstrap token rejected: {msg}"
                )));
            }
            other => return Err(other.into()),
        }
    }

    // Transport 등록은 best-effort — 실패해도 이미 커밋된 Store enrollment는
    // 되돌리지 않는다 (register_worker와 동일한 정책, upsert_and_register 참고).
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
    let worker_config_toml = render_worker_config_toml(WorkerConfigTomlParams {
        name,
        agent_endpoint: &req.agent_endpoint,
        labels: &req.labels,
        operational_token: &operational_token,
        worker_id,
        heartbeat_interval_secs: state.heartbeat_interval_secs,
        max_concurrent_tasks: req.max_concurrent_tasks,
        liveness_mode: req.liveness_mode,
    });

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
        token_digest: fleet_core::BootstrapToken::digest_for(&token),
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
        token_id: bt.public_id(),
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

/// `DELETE /v1/bootstrap-tokens/:token_id` — 공개 식별자로 토큰 회수.
pub async fn revoke_bootstrap_token(
    State(state): State<Arc<AppState>>,
    Path(token_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token_digest = fleet_core::BootstrapToken::digest_from_public_id(&token_id)
        .ok_or_else(|| ApiError::NotFound("bootstrap token not found".into()))?;
    let revoked = state.store.revoke_bootstrap_token(&token_digest).await?;
    if !revoked {
        return Err(ApiError::NotFound("bootstrap token not found".into()));
    }
    info!("bootstrap token revoked");
    Ok(Json(serde_json::json!({
        "status": "revoked",
        "token_id": token_id,
    })))
}

/// [`render_worker_config_toml`]의 인자 묶음 (clippy::too_many_arguments 회피).
struct WorkerConfigTomlParams<'a> {
    name: &'a str,
    agent_endpoint: &'a str,
    labels: &'a HashMap<String, String>,
    operational_token: &'a str,
    worker_id: WorkerId,
    heartbeat_interval_secs: u32,
    max_concurrent_tasks: u32,
    liveness_mode: fleet_core::WorkerLivenessMode,
}

/// worker.toml 문자열 렌더링.
///
/// 클라이언트가 받아서 그대로 디스크에 기록할 수 있는 TOML을 생성.
/// `[worker] existing_worker_id`를 포함하여, 이후 재시작 시 동일 ID로 재등록 가능.
fn render_worker_config_toml(params: WorkerConfigTomlParams<'_>) -> String {
    let WorkerConfigTomlParams {
        name,
        agent_endpoint,
        labels,
        operational_token,
        worker_id,
        heartbeat_interval_secs,
        max_concurrent_tasks,
        liveness_mode,
    } = params;
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
    out.push_str(&format!("operational_token = \"{operational_token}\"\n"));
    out.push_str(&format!("existing_worker_id = \"{worker_id}\"\n"));
    if liveness_mode == fleet_core::WorkerLivenessMode::OnDemand {
        // 로드맵 #61 — on_demand는 아직 스키마/모니터링 예외 처리까지만
        // 구현되었다. dispatch 전 ACP probe(로드맵 #67 의존)가 없는 상태이므로
        // 실제로 이 모드를 켜서 운영하는 것은 지원하지 않는다.
        out.push_str("liveness_mode = \"on_demand\" # 로드맵 #61 3~5단계 미구현 — 아직 프로덕션에서 사용하지 말 것\n");
    }
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

/// Worker self-binding 검사 (로드맵 #60).
///
/// `ctx.worker_id`가 `Some`인 경우(요청자가 worker operational credential로
/// 인증됨) — 조작하려는 대상이 자기 자신이 아니면 403을 반환한다.
/// `ctx`가 없거나(`None`) `ctx.worker_id`가 `None`(admin bearer, development
/// no-auth, CF Access)이면 기존처럼 제한 없이 통과시킨다.
fn enforce_worker_self_binding(
    ctx: Option<&AuthorizationContext>,
    target: WorkerId,
) -> Result<(), ApiError> {
    if let Some(ctx_worker_id) = ctx.and_then(|c| c.worker_id) {
        if ctx_worker_id != target {
            return Err(ApiError::Forbidden(format!(
                "worker {ctx_worker_id} is not authorized to act on worker {target}"
            )));
        }
    }
    Ok(())
}

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

/// [`build_worker`]의 인자 묶음 (clippy::too_many_arguments 회피).
struct NewWorkerParams<'a> {
    worker_id: WorkerId,
    name: &'a str,
    endpoint: &'a str,
    labels: HashMap<String, String>,
    max_concurrent: u32,
    worker_version: Option<String>,
    liveness_mode: fleet_core::WorkerLivenessMode,
}

/// `Worker` 엔티티 생성. 기존 워커가 있으면 registered_at을 유지.
fn build_worker(params: NewWorkerParams<'_>, existing: Option<&Worker>) -> Worker {
    let NewWorkerParams {
        worker_id,
        name,
        endpoint,
        labels,
        max_concurrent,
        worker_version,
        liveness_mode,
    } = params;
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
        liveness_mode,
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
        liveness_mode: WorkerSummary::liveness_mode_str(w.liveness_mode).to_string(),
        registered_at: w.registered_at,
    }
}

// 사용하지 않을 수 있는 import 정리 — warning 방지

// ═══════════════════════════════════════════════════════════════════════
//  Phase 8.6: Worker credentials 핸들러
// ═══════════════════════════════════════════════════════════════════════

/// `PUT /v1/workers/:name/credentials` — API 키 저장/회전.
///
/// 요청 바디의 `api_key` (평문)를 마스터 키로 AES-256-GCM 암호화하여
/// DB에 저장. `master_key`가 설정되지 않은 경우 503 반환.
pub async fn put_worker_credential(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    ctx: Option<Extension<AuthorizationContext>>,
    Json(req): Json<PutCredentialRequest>,
) -> Result<Json<PutCredentialResponse>, ApiError> {
    // worker 존재 확인.
    let worker = state
        .store
        .get_worker_by_name(&name)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("worker '{name}' not found")))?;

    // 마스터 키 필수.
    let master_key = state.master_key.as_ref().ok_or_else(|| {
        ApiError::Internal("master key not configured on this orchestrator".into())
    })?;

    // 입력 검증.
    if req.api_key.is_empty() {
        return Err(ApiError::BadRequest("api_key must not be empty".into()));
    }
    if req.base_url.is_empty() {
        return Err(ApiError::BadRequest("base_url must not be empty".into()));
    }
    if req.model_id.is_empty() {
        return Err(ApiError::BadRequest("model_id must not be empty".into()));
    }

    // 암호화.
    let blob = master_key
        .encrypt(req.api_key.as_bytes())
        .map_err(|e| ApiError::Internal(format!("encryption failed: {e}")))?;

    // upsert.
    state
        .store
        .upsert_worker_credential(
            &worker.name,
            &req.model_id,
            blob.as_str(),
            &req.base_url,
            &req.api_backend,
            req.context_window,
            req.model_name.as_deref(),
        )
        .await?;

    // 회전 시간을 반환하기 위해 다시 조회.
    let stored = state
        .store
        .get_worker_credential(&worker.name, &req.model_id)
        .await?
        .ok_or_else(|| {
            ApiError::Internal("credential upsert succeeded but row not found".into())
        })?;

    info!(
        worker = %worker.name,
        model_id = %req.model_id,
        rotated_at = %stored.rotated_at,
        "worker credential stored/rotated"
    );

    // 변경은 이미 커밋됐으므로 감사 기록 실패로 요청을 되돌릴 수 없다.
    // export와 달리 여기서는 로그만 남기고 성공을 반환한다.
    record_credential_audit(
        &state,
        ctx.as_deref(),
        action::WORKER_LLM_CREDENTIAL_PUT,
        &worker.name,
        &req.model_id,
    )
    .await
    .unwrap_or_else(|e| {
        tracing::error!(
            worker = %worker.name,
            model_id = %req.model_id,
            error = %e,
            "failed to record audit event for LLM credential write"
        );
    });

    Ok(Json(PutCredentialResponse {
        status: "rotated",
        worker_name: worker.name,
        model_id: req.model_id,
        rotated_at: stored.rotated_at,
    }))
}

/// `GET /v1/workers/:name/credentials` — 메타데이터만 조회 (api_key 제외).
pub async fn list_worker_credentials(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Vec<CredentialSummary>>, ApiError> {
    // worker 존재 확인 (NotFound 명확화).
    let worker = state
        .store
        .get_worker_by_name(&name)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("worker '{name}' not found")))?;

    let creds = state.store.list_worker_credentials(&worker.name).await?;
    Ok(Json(
        creds.into_iter().map(CredentialSummary::from).collect(),
    ))
}

/// `GET /v1/workers/:name/credentials/:model_id/export` — 복호화된 전체 자격 증명.
///
/// **주의**: api_key가 평문으로 반환됨. 프로비저닝 스크립트 전용 엔드포인트.
///
/// 접근 통제는 `worker:llm_credential:export` capability가 담당한다
/// (`required_capability`, 로드맵 #66). 이 capability 매핑이 없던 동안에는
/// 인증만 통과하면 누구나 모든 워커의 LLM API 키를 평문으로 가져갈 수 있었다.
///
/// 감사 기록은 응답 **전에** 남기고, 기록에 실패하면 평문을 반환하지 않는다.
/// 키가 유출됐을 때 "누가 언제 어떤 키를 가져갔는지"가 회수 범위를 정하는
/// 유일한 근거이므로, 근거를 남기지 못하면 열람 자체를 허용하지 않는다.
pub async fn export_worker_credential(
    State(state): State<Arc<AppState>>,
    Path((name, model_id)): Path<(String, String)>,
    ctx: Option<Extension<AuthorizationContext>>,
) -> Result<Json<ExportedCredential>, ApiError> {
    let worker = state
        .store
        .get_worker_by_name(&name)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("worker '{name}' not found")))?;

    let master_key = state.master_key.as_ref().ok_or_else(|| {
        ApiError::Internal("master key not configured on this orchestrator".into())
    })?;

    let stored = state
        .store
        .get_worker_credential(&worker.name, &model_id)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "credential not found for worker '{name}' model '{model_id}'"
            ))
        })?;

    // 복호화.
    let blob = fleet_credentials::EncryptedBlob::from_string(stored.encrypted_blob.clone());
    let plaintext = master_key
        .decrypt(&blob)
        .map_err(|e| ApiError::Internal(format!("decryption failed: {e}")))?;
    let api_key = String::from_utf8(plaintext)
        .map_err(|e| ApiError::Internal(format!("decrypted bytes are not UTF-8: {e}")))?;

    // grok config 섹션 렌더링.
    let cred = fleet_credentials::WorkerCredentials {
        worker_name: stored.worker_name.clone(),
        model_id: stored.model_id.clone(),
        base_url: stored.base_url.clone(),
        api_key: api_key.clone(),
        api_backend: stored.api_backend.clone(),
        context_window: stored.context_window,
        model: stored.model_name.clone(),
        rotated_at: Some(stored.rotated_at),
    };
    let grok_config_section = cred.render_grok_config_section();

    record_credential_audit(
        &state,
        ctx.as_deref(),
        action::WORKER_LLM_CREDENTIAL_EXPORT,
        &stored.worker_name,
        &stored.model_id,
    )
    .await
    .map_err(|e| {
        tracing::error!(
            worker = %stored.worker_name,
            model_id = %stored.model_id,
            error = %e,
            "refusing credential export — audit event could not be recorded"
        );
        ApiError::Internal("audit log unavailable; credential export refused".into())
    })?;

    debug!(
        worker = %stored.worker_name,
        model_id = %stored.model_id,
        "credential exported (decrypted)"
    );

    Ok(Json(ExportedCredential {
        worker_name: stored.worker_name,
        model_id: stored.model_id,
        base_url: stored.base_url,
        api_key,
        api_backend: stored.api_backend,
        context_window: stored.context_window,
        model_name: stored.model_name,
        rotated_at: stored.rotated_at,
        grok_config_section,
    }))
}

/// `DELETE /v1/workers/:name/credentials/:model_id` — 자격 증명 제거.
pub async fn delete_worker_credential(
    State(state): State<Arc<AppState>>,
    Path((name, model_id)): Path<(String, String)>,
    ctx: Option<Extension<AuthorizationContext>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let deleted = state
        .store
        .delete_worker_credential(&name, &model_id)
        .await?;
    if !deleted {
        return Err(ApiError::NotFound(format!(
            "credential not found for worker '{name}' model '{model_id}'"
        )));
    }
    info!(worker = %name, model_id = %model_id, "credential deleted");

    // 삭제는 이미 반영됐다 — 감사 기록 실패로 200을 500으로 바꾸면 호출자에게
    // "삭제되지 않았다"는 잘못된 신호를 준다. 로그로만 경보한다.
    record_credential_audit(
        &state,
        ctx.as_deref(),
        action::WORKER_LLM_CREDENTIAL_DELETE,
        &name,
        &model_id,
    )
    .await
    .unwrap_or_else(|e| {
        tracing::error!(
            worker = %name,
            model_id = %model_id,
            error = %e,
            "failed to record audit event for LLM credential delete"
        );
    });

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "worker_name": name,
        "model_id": model_id,
    })))
}

/// 감사 로그에 남길 행위자 표시 문자열.
///
/// 인증 컨텍스트가 없는 경우(개발용 무인증 모드에서 미들웨어가 컨텍스트를
/// 넣지 못한 경우 등)에도 "누군지 모른다"는 사실 자체를 기록한다. 행위자를
/// 특정하지 못했다는 이유로 기록을 생략하면, 가장 수상한 접근이 흔적 없이
/// 사라진다.
fn audit_actor_label(ctx: Option<&AuthorizationContext>) -> String {
    match ctx {
        Some(ctx) => ctx.principal_id.clone(),
        None => "unattributed".to_string(),
    }
}

/// worker LLM credential 관련 감사 이벤트 기록 (로드맵 #66).
///
/// **비밀 값을 detail에 넣지 않는다** — 어떤 워커의 어떤 모델인지까지만 남긴다.
async fn record_credential_audit(
    state: &AppState,
    ctx: Option<&AuthorizationContext>,
    action_name: &str,
    worker_name: &str,
    model_id: &str,
) -> Result<(), fleet_store::StoreError> {
    let mut detail = serde_json::json!({
        "worker_name": worker_name,
        "model_id": model_id,
    });
    if let Some(ctx) = ctx {
        detail["authentication_method"] =
            serde_json::json!(format!("{:?}", ctx.authentication_method));
    }
    let event = AuditEvent::success(audit_actor_label(ctx), action_name)
        .target("worker_llm_credential", format!("{worker_name}/{model_id}"))
        .detail(detail);
    state.store.record_audit_event(&event).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 로드맵 #42: register/heartbeat 요청의 traceparent 수신 처리 ──────

    #[test]
    fn continue_trace_from_headers_links_span_to_incoming_traceparent() {
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
        let subscriber = tracing_subscriber::Registry::default()
            .with(tracing_opentelemetry::layer().with_tracer(tracer));

        // fleet-worker가 보냈을 법한, 알려진 trace-id를 가진 traceparent.
        let known_trace_id = "4bf92f3577b34da6a3ce929d0e0e4736";
        let mut headers = HeaderMap::new();
        headers.insert(
            "traceparent",
            format!("00-{known_trace_id}-00f067aa0b902b7-01")
                .parse()
                .unwrap(),
        );

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("register_worker");
            let _guard = span.enter();
            continue_trace_from_headers(&headers);
        });

        let spans = exporter
            .get_finished_spans()
            .expect("exporter should have received the finished span");
        assert_eq!(spans.len(), 1, "exactly one span should have been recorded");
        assert_eq!(
            spans[0].span_context.trace_id().to_string(),
            known_trace_id,
            "span exported after continue_trace_from_headers must share the incoming trace-id"
        );
    }

    #[test]
    fn continue_trace_from_headers_is_noop_without_traceparent_header() {
        // 헤더가 없으면 panic 없이 조용히 넘어가야 한다(로컬 루트 스팬으로
        // 남는다) — OTel이 비활성인 배포에서도 안전해야 하기 때문.
        let headers = HeaderMap::new();
        continue_trace_from_headers(&headers);
    }

    #[test]
    fn header_extractor_reads_case_insensitively() {
        // `axum::http::HeaderMap`은 헤더 이름을 대소문자 구분 없이 저장한다 —
        // Extractor 어댑터가 이를 그대로 위임하는지 확인.
        let mut headers = HeaderMap::new();
        headers.insert("TraceParent", "00-abc-def-01".parse().unwrap());
        let extractor = HeaderExtractor(&headers);
        assert_eq!(extractor.get("traceparent"), Some("00-abc-def-01"));
    }
}
