//! 대시보드 API 응답 스키마.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use fleet_core::{Agent, Project, WorkerStatus};

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

/// `DELETE /api/projects/{id}` 응답 (로드맵 #48 1단계, 사유는 `#49` 1단계 후속).
///
/// [`ProjectSummary`]에 **게이트를 막은 사유**를 덧붙인 것이다. 상태만
/// 돌려주던 동안 화면은 사유를 짐작할 수밖에 없었고, 게이트에 Agent 조건이
/// 추가되자 Task가 0건인 Project에 "tasks still running"이라고 표시했다.
/// 사유는 게이트를 평가한 쪽이 말한다([`fleet_store::ArchiveBlockers`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectArchiveResponse {
    #[serde(flatten)]
    pub project: ProjectSummary,
    /// `draining`에 머문 이유. 게이트를 통과했으면 비어 있고, 비어 있으면
    /// 실리지 않는다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archive_blocked_by: Vec<String>,
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

// ── Agent (로드맵 #49, 1단계) ──────────────────────────────────────────

/// `/api/agents` 배열 요소 (로드맵 #49, 1단계).
///
/// `project_id`는 응답에 항상 들어간다 — 불변 필드라 "이 Agent가 어느
/// 경계 안에 있는가"가 그 Agent에 대해 영구히 참인 사실이고, 목록을
/// Project로 필터링한 뒤에도 클라이언트가 그것을 다시 확인할 수 있어야
/// 한다. 갱신 요청 본문(`UpdateAgentRequest`)은 없다 — `project_id`를
/// 바꿀 수 없다는 규칙을 표면에서도 "경로가 아예 없음"으로 집행한다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    pub id: String,
    pub project_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&Agent> for AgentSummary {
    fn from(a: &Agent) -> Self {
        Self {
            id: a.id.to_string(),
            project_id: a.project_id.to_string(),
            name: a.name.clone(),
            description: a.description.clone(),
            created_by: a.created_by.clone(),
            status: a.status.as_str().to_string(),
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

/// `POST /api/agents` 요청 본문.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateAgentRequest {
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

// ── Issue (로드맵 #92, Issue 표면) ──────────────────────────────────────

/// `/api/issues` 배열 요소.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueSummary {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub body: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<String>,
    pub severity: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// **파생 값** — 비터미널 연관 Task가 있는지. `InProgress` 상태를 두지
    /// 않은 이유가 이것이다(계약: "UI는 파생 배지로 표시하고 저장하지
    /// 않는다"). 저장된 필드가 아니라 조회 시점에 계산한다.
    pub has_active_tasks: bool,
}

impl IssueSummary {
    /// `has_active_tasks`는 Store 조회가 필요해 호출부가 넘긴다 — 이 변환
    /// 자체는 순수 함수로 남긴다.
    pub fn from_issue(i: &fleet_core::Issue, has_active_tasks: bool) -> Self {
        Self {
            id: i.id.to_string(),
            project_id: i.project_id.to_string(),
            title: i.title.clone(),
            body: i.body.clone(),
            status: i.status.as_str().to_string(),
            close_reason: i.close_reason.map(|r| r.as_str().to_string()),
            severity: i.severity.as_str().to_string(),
            labels: i.labels.clone(),
            assignee: i.assignee.clone(),
            created_by: i.created_by.clone(),
            created_at: i.created_at,
            updated_at: i.updated_at,
            has_active_tasks,
        }
    }
}

/// `POST /api/issues` 요청 본문.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateIssueRequest {
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
}

/// `PATCH /api/issues/{id}` 요청 본문 — **상태는 바꿀 수 없다**.
/// 상태 전이는 `POST /api/issues/{id}/transition`이 담당하며, 그쪽은
/// 목표 상태별로 다른 capability를 요구한다.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateIssueRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    /// `Some(None)`(JSON `null`)이면 assignee 해제, 생략이면 변경 없음.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Option<String>>,
}

/// `POST /api/issues/{id}/transition` 요청 본문.
#[derive(Debug, Clone, Deserialize)]
pub struct TransitionIssueRequest {
    pub status: String,
    #[serde(default)]
    pub close_reason: Option<String>,
}

/// `POST /api/issues/{id}/comments` 요청 본문.
#[derive(Debug, Clone, Deserialize)]
pub struct AddIssueCommentRequest {
    pub body: String,
}

/// `/api/issues/{id}/comments` 배열 요소.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueCommentSummary {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

/// `POST /api/issues/{id}/links` 요청 본문.
#[derive(Debug, Clone, Deserialize)]
pub struct LinkIssueTaskRequest {
    pub task_id: String,
}

/// `/api/issues/{id}/links` 배열 요소.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueTaskLinkSummary {
    /// Task가 삭제되면 `None`이 되고 `task_label`만 남는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub task_label: String,
    pub linked_by: String,
    pub linked_at: DateTime<Utc>,
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
