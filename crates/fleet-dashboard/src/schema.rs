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
    /// 어느 AgentTemplate revision으로 만들어졌는지 (로드맵 #86). 둘은 항상
    /// 함께 있거나 함께 없다 — 코어의 `AgentTemplatePin`이 절반만 채워진
    /// 상태를 표현할 수 없게 되어 있고, 여기서는 그것을 두 필드로 편다.
    /// 응답에서 둘을 합친 객체가 아니라 평평한 두 필드로 두는 것은 기존
    /// 필드들과 같은 모양을 유지하기 위함이다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_template_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_template_revision_id: Option<String>,
    /// 어느 Worker에 배정됐는지 (로드맵 #67 4a). `template_pin`과 마찬가지로
    /// 둘은 항상 함께 있거나 함께 없지만, 이유는 정반대다 — pin은 **불변
    /// 정체성**이라 절반이 불가능하고, 배정은 **가변 운영 상태**라 절반이
    /// 되지 않도록 DB의 CHECK와 트리거가 막는다. `None`은 오류가 아니라
    /// "지금 어느 Worker에도 배정되어 있지 않음"이며, 생성 시 배정에
    /// 실패했거나 배정됐던 Worker가 등록 해제된 경우에 나타난다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_at: Option<DateTime<Utc>>,
    /// 수렴 프로토콜의 세 값 (로드맵 #67 4b). `status`가 **관측**이라면 이
    /// 셋은 **의도와 그 전달 상태**다 — 둘을 한 필드로 합치지 않는 이유가
    /// 여기 있다: 관측 주체(4c의 프로세스 매니저)가 아직 없어서, 지금
    /// 오케스트레이터가 아는 것은 "무엇을 원했고 Worker가 그것을 받았는가"
    /// 까지뿐이다.
    pub desired_status: String,
    pub command_generation: i64,
    pub last_acked_generation: i64,
    /// 위 두 세대가 같은가 — 즉 Worker가 현재 명령을 **받고 받아들였는가**.
    /// 파생이지만 서버가 내보낸다: 두 클라이언트가 각자 비교하다가 한쪽이
    /// `<=`로 쓰면 조용히 다른 답을 낸다.
    pub command_delivered: bool,
    /// `ready`이면서 desired가 `running`인 구간. `AgentStatus`의 variant가
    /// **아니라** 파생인 이유는 `Starting`이 관측이 아니라 두 필드의 순수
    /// 함수이기 때문이다 — variant로 만들었다면 `027`의
    /// `status IN ('ready','stopped')` CHECK를 고치고, 아무도 관측하지 않는
    /// 상태를 저장하게 된다.
    pub is_starting: bool,
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
            agent_template_id: a.template_pin.map(|p| p.template_id.to_string()),
            agent_template_revision_id: a.template_pin.map(|p| p.revision_id.to_string()),
            worker_id: a.worker_id.map(|w| w.to_string()),
            assigned_at: a.assigned_at,
            desired_status: a.desired_status.as_str().to_string(),
            command_generation: a.command_generation,
            last_acked_generation: a.last_acked_generation,
            command_delivered: a.command_delivered(),
            is_starting: a.is_starting(),
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

/// `POST /api/agents/{id}/place` 요청 본문 (로드맵 #67 4a).
///
/// 본문 전체가 선택적이다 — `worker_id`를 생략하면 오케스트레이터가
/// least-loaded로 고른다. `CreateAgentRequest`와 달리 필수 필드가 하나도
/// 없는 이유는, 이 경로의 주 용도가 "생성 때 배정에 실패한 Agent를 지금
/// 배정하라"이고 그때 운영자는 **어느 Worker인지 모르기 때문**이다.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlaceAgentRequest {
    #[serde(default)]
    pub worker_id: Option<String>,
}

/// `POST /api/agents` 요청 본문.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateAgentRequest {
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// 템플릿 pin (로드맵 #86). 둘 다 주거나 둘 다 생략한다 — 한쪽만 주면
    /// `400`이다. 핸들러가 그것을 먼저 걸러 코어의 `AgentTemplatePin`을
    /// 만들며, 유효성(revoke·retire 여부)은 Store가 트랜잭션 안에서 본다.
    #[serde(default)]
    pub agent_template_id: Option<String>,
    #[serde(default)]
    pub agent_template_revision_id: Option<String>,
}

// ── AgentTemplate (로드맵 #86, 1단계) ──────────────────────────────────

/// `/api/agent-templates` 배열 요소.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplateSummary {
    pub id: String,
    /// `None`이면 전역 템플릿.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub status: String,
    /// **파생 값** — 현재 상태에서 갈 수 있는 상태들
    /// ([`fleet_core::AgentTemplateStatus::allowed_transitions`]).
    ///
    /// 저장하지 않고 조회 시점에 계산해 싣는다(`IssueSummary`의
    /// `has_active_tasks`와 같은 원칙). 관리 화면이 전이표를 다시 구현하지
    /// 않게 하려는 것이 목적이다 — JS가 자기 표를 들면 코어의 표와 갈라지고,
    /// 갈라진 쪽이 화면에서는 조용히 이긴다. 서버가 준 목록만 그리면 코어를
    /// 고쳤을 때 화면이 저절로 따라온다.
    pub allowed_transitions: Vec<String>,
    /// **파생 값** — 이 상태에 새 revision을 붙일 수 있는지. 위와 같은 이유로
    /// 화면이 `retired`/`discarded`를 스스로 판별하지 않게 한다.
    pub accepts_new_revisions: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&fleet_core::AgentTemplate> for AgentTemplateSummary {
    fn from(t: &fleet_core::AgentTemplate) -> Self {
        Self {
            id: t.id.to_string(),
            project_id: t.project_id.map(|p| p.to_string()),
            name: t.name.clone(),
            description: t.description.clone(),
            created_by: t.created_by.clone(),
            status: t.status.as_str().to_string(),
            allowed_transitions: t
                .status
                .allowed_transitions()
                .iter()
                .map(|s| s.as_str().to_string())
                .collect(),
            accepts_new_revisions: t.status.accepts_new_revisions(),
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

/// revision 한 건. 본문 전체를 담는다 — 이것이 감사 대상이며, `content_hash`가
/// 그 본문에서 재계산 가능해야 기록으로서 뜻이 있다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplateRevisionSummary {
    pub id: String,
    pub template_id: String,
    pub content_revision: i32,
    pub content_hash: String,
    pub role_prompt: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<&fleet_core::AgentTemplateRevision> for AgentTemplateRevisionSummary {
    fn from(r: &fleet_core::AgentTemplateRevision) -> Self {
        Self {
            id: r.id.to_string(),
            template_id: r.template_id.to_string(),
            content_revision: r.content_revision,
            content_hash: r.content_hash.clone(),
            role_prompt: r.body.role_prompt.clone(),
            tools: r.body.tools.clone(),
            skills: r.body.skills.clone(),
            revoked_at: r.revoked_at,
            created_by: r.created_by.clone(),
            created_at: r.created_at,
        }
    }
}

/// `POST /api/agent-templates` 요청 본문.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateAgentTemplateRequest {
    /// 생략하면 전역 템플릿이며, 그 경우 `agent_template:manage_global`을
    /// 추가로 요구한다.
    #[serde(default)]
    pub project_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// `POST /api/agent-templates/{id}/revisions` 요청 본문.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateAgentTemplateRevisionRequest {
    pub role_prompt: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

/// `POST /api/agent-templates/{id}/status` 요청 본문.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentTemplateStatusRequest {
    pub status: String,
    /// `retired`로 전이할 때 **필수**. `GET /api/agent-templates/{id}/dependents`가
    /// 돌려준 해시를 그대로 되보낸다. 그 사이에 의존 집합이 바뀌었으면 `409`다.
    #[serde(default)]
    pub dependent_set_hash: Option<String>,
}

/// `GET /api/agent-templates/{id}/dependents` 응답.
///
/// 목록과 해시를 **함께** 준다. 해시만 주면 확인 화면이 무엇을 못 쓰게 만드는지
/// 보여줄 수 없고, 목록만 주면 클라이언트가 해시를 직접 계산해야 해서 인코딩
/// 규칙이 서버 밖으로 새어 나간다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplateDependents {
    pub template_id: String,
    pub agent_ids: Vec<String>,
    pub dependent_set_hash: String,
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
