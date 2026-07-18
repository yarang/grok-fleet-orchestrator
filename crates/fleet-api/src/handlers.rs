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

use crate::error::ApiError;
use crate::schema::{
    DeregisterRequest, HealthResponse, HeartbeatRequest, HeartbeatResponse, RegisterRequest,
    RegisterResponse, WorkerSummary,
};
use crate::app::AppState;

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
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(ApiError::BadRequest(
            "name must be alphanumeric, '-', '_', or '.' only".into(),
        ));
    }

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

    // 2. Worker 엔티티 구성
    let now = Utc::now();
    let registered_at = existing_by_name
        .as_ref()
        .or(existing_by_id.as_ref())
        .map(|w| w.registered_at)
        .unwrap_or(now);

    let worker = Worker {
        id: worker_id,
        name: name.to_string(),
        endpoint: req.agent_endpoint.clone(),
        labels: req.labels.clone(),
        status: WorkerStatus::Online,
        last_seen: Some(now),
        active_tasks: 0, // 재등록 시 0으로 리셋
        max_concurrent: req.max_concurrent_tasks,
        circuit_state: fleet_core::CircuitState::Closed,
        worker_version: req.worker_version.clone(),
        registered_at,
    };

    // 3. Store에 upsert
    state.store.upsert_worker(&worker).await?;

    // 4. WorkerJoined 이벤트 (재등록인지 신규인지 구분)
    let is_new = existing_by_name.is_none() && existing_by_id.is_none();
    let event = if is_new {
        info!(%worker_id, name = %worker.name, "worker registered");
        fleet_core::FleetEvent::worker_joined(
            worker_id,
            &worker.name,
            &worker.endpoint,
        )
    } else {
        info!(%worker_id, name = %worker.name, "worker re-registered");
        // 재등록은 별도 이벤트가 없으므로 WorkerHeartbeat로 처리
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
        .append_event(&fleet_core::FleetEvent::worker_left(
            worker_id,
            &reason,
        ))
        .await;

    state.store.delete_worker(worker_id).await?;

    info!(%worker_id, name = %worker.name, reason = %reason, "worker deregistered");
    Ok(Json(serde_json::json!({
        "worker_id": id_str,
        "status": "deregistered",
        "reason": reason,
    })))
}

// ── 헬퍼 ────────────────────────────────────────────────────────────────

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
