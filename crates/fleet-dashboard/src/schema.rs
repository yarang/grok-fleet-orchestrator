//! 대시보드 API 응답 스키마.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use fleet_core::WorkerStatus;

/// `/api/overview` 응답.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewResponse {
    pub workers: WorkerCounts,
    pub tasks: TaskCounts,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCounts {
    pub online: u32,
    pub degraded: u32,
    pub offline: u32,
    pub circuit_open: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskCounts {
    pub pending: u32,
    pub dispatched: u32,
    pub completed: u32,
    pub failed: u32,
    pub cancelled: u32,
    pub total: u32,
}

/// `/api/workers` 배열 요소.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSummary {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub status: String,
    pub labels: HashMap<String, String>,
    pub active_tasks: u32,
    pub max_concurrent: u32,
    pub circuit_state: String,
    pub last_seen: Option<DateTime<Utc>>,
    pub registered_at: DateTime<Utc>,
}

impl WorkerSummary {
    pub fn status_str(s: WorkerStatus) -> &'static str {
        match s {
            WorkerStatus::Online => "online",
            WorkerStatus::Degraded => "degraded",
            WorkerStatus::Offline => "offline",
            WorkerStatus::CircuitOpen => "circuit_open",
        }
    }
}

/// `/api/tasks` 배열 요소.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub phase: String,
    pub prompt: String,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub worker_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
}
