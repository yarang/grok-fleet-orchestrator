//! 대시보드 API 응답 스키마.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use fleet_core::{Project, WorkerStatus};

/// `/api/overview` 응답.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewResponse {
    pub workers: WorkerCounts,
    pub tasks: TaskCounts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenStats>,
    pub generated_at: DateTime<Utc>,
}

/// 완료된 작업에서 집계한 LLM 토큰 사용량.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCounts {
    pub online: u32,
    pub degraded: u32,
    pub draining: u32,
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

/// `/api/projects` 배열 요소 (로드맵 #48, 1단계).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&Project> for ProjectSummary {
    fn from(p: &Project) -> Self {
        Self {
            id: p.id.to_string(),
            name: p.name.clone(),
            description: p.description.clone(),
            created_by: p.created_by.clone(),
            status: p.status.as_str().to_string(),
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

/// `POST /api/projects` 요청 본문.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
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
            WorkerStatus::Draining => "draining",
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenStats>,
    /// 이 태스크가 속한 스레드(연속 대화)의 루트 id. 스레드 자체가 태스크
    /// 1개짜리(=자기 자신)면 이어가기 히스토리가 없다는 뜻.
    pub thread_id: String,
    /// "이어가기(Reply)"라면 직전 태스크의 id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    /// 이 태스크가 묶인 Project (로드맵 #48 2단계). 일반 풀 Task면 없음.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════
//  Host inventory (P1.5)
// ═══════════════════════════════════════════════════════════════════════

/// `/api/hosts` 배열 요소.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSummary {
    pub id: String,
    pub hostname: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grok_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_worker_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisioned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// `/api/hosts/:hostname` 상세 응답.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDetail {
    #[serde(flatten)]
    pub summary: HostSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_host: Option<String>,
    pub ssh_port: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_user: Option<String>,
    #[serde(default)]
    pub os_info: Option<OsInfoSummary>,
    #[serde(default)]
    pub metrics: HostMetricsSummary,
    #[serde(default)]
    pub events: Vec<HostEventSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsInfoSummary {
    #[serde(default)]
    pub os_type: String,
    #[serde(default)]
    pub distro: String,
    #[serde(default)]
    pub kernel: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub hostname: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostMetricsSummary {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub load_avg: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_available_mb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_free_mb: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostEventSummary {
    pub id: String,
    pub event_type: String,
    pub severity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ═══════════════════════════════════════════════════════════════════════
//  Worker detail (P1)
// ═══════════════════════════════════════════════════════════════════════

/// `/api/workers/:id` 상세 응답.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerDetail {
    #[serde(flatten)]
    pub summary: WorkerSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_version: Option<String>,
    /// 최근 태스크 (limit 20).
    #[serde(default)]
    pub recent_tasks: Vec<TaskSummary>,
}

// ═══════════════════════════════════════════════════════════════════════
//  User management (P1)
// ═══════════════════════════════════════════════════════════════════════

/// `/api/users` 배열 요소.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSummary {
    pub id: String,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub roles: Vec<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_login_at: Option<DateTime<Utc>>,
}
