//! Agent 도메인 모델 (로드맵 #49, 1단계).
//!
//! [Agent provisioning](../../../docs/architecture/agents/provisioning.md)과
//! [배치와 컨텍스트](../../../docs/architecture/entity-placement-and-context.md)가
//! 정본이다. 이 파일은 그 목표 모델의 **의도적으로 축소된 1단계**를 구현한다.
//!
//! # 왜 목표 8-상태가 아니라 2-상태인가
//!
//! 정본의 상태 기계는 `Ready → Starting → Running → WarmIdle|Hibernated →
//! Draining → Stopped`이고 `Failed`가 임의의 실행 상태에서 도달 가능하다.
//! 그 여섯 개는 전부 **Worker가 Agent 프로세스를 실제로 띄우고 그 결과를
//! ACK로 돌려줄 때** 비로소 도달한다. 이 저장소에는 아직:
//!
//! - Worker control stream(명령 봉투·ACK·`worker_incarnation`)이 없다 —
//!   로드맵 `#89`. 따라서 `Starting`/`Running`/`Failed`로 전이시킬 주체가
//!   없다.
//! - execution lease가 없다 — 로드맵 `#67` 2단계. `WarmIdle`은 정본상
//!   execution lease와 Worker 프로세스 슬롯을 **점유한 채** 머무는 상태라,
//!   lease 없이 만들면 아무것도 점유하지 않는 이름뿐인 상태가 된다.
//! - `Hibernated`는 스냅샷 불일치 판정(runtime/image·isolation·workspace·
//!   Tool/Skill·egress 정책)이 있어야 의미가 생기는데, 그 스냅샷을 만드는
//!   AgentTemplate(`#86`)과 harness 구성(`#51`)이 없다.
//!
//! 그래서 1단계의 [`AgentStatus`]는 오늘 실제로 도달 가능한 두 상태만 갖는다.
//! 나머지를 지금 선언하면 아무도 생성하지 않는 죽은 variant가 된다 —
//! `ProjectStatus`를 목표 5-상태가 아니라 3-상태로 낸 것과 같은 판단이고,
//! `#70` 조사에서 `FailureKind`의 죽은 variant 세 개를 발견해 제거해야 했던
//! 비용을 반복하지 않기 위한 것이다.
//!
//! # 1단계에서도 Agent 행은 죽은 데이터가 아니다
//!
//! 엔티티만 만들고 아무도 읽지 않으면 `022_projects.sql`이 지적한
//! `tasks.project_id`의 상황("참조 대상이 없어 순수 미검증 메타데이터")을
//! 재생산하게 된다. 1단계는 판독자를 같은 커밋에 함께 넣는다:
//!
//! 1. 생성 시 `project_id`가 실재하고 `Active`인지 검증한다
//!    (`fleet_store::project_rules::ensure_project_accepts_new_agents`).
//! 2. Project archive 게이트가 살아 있는 Agent를 센다 — `Ready` Agent가
//!    남아 있으면 `Draining`에 머문다. 정본의 `ArchiveBlocked` 조건 중
//!    "Agent cleanup 증거"의, 오늘 확인 가능한 부분이다.
//! 3. `agent:read`/`agent:manage` capability가 처음으로 검사 대상을 갖는다
//!    ([인가와 감사](../../../docs/security/authorization-and-audit.md)).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{AgentId, ProjectId};

/// Agent 운영 상태 (목표 8-상태의 1단계 부분집합 — 위 모듈 문서 참고).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// 정의는 끝났고 시작 명령을 받을 수 있다. 1단계에는 시작 명령을 보낼
    /// Worker control stream이 없으므로, 생성된 Agent는 모두 여기서 시작해
    /// `Stopped`로만 나간다.
    Ready,
    /// 회수됐다. 1단계에서는 프로세스가 없으므로 정리할 대상도 없어
    /// cleanup 증거 없이 곧바로 도달한다 — 정본이 요구하는 "cleanup 증거가
    /// 있을 때만 `Stopped`"는 프로세스를 띄우는 `#89`에서 성립한다.
    Stopped,
}

impl AgentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Stopped => "stopped",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "ready" => Some(Self::Ready),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }

    /// 이 Agent가 소속 Project의 archive를 막는지.
    ///
    /// `Stopped`가 아닌 Agent는 아직 회수되지 않았으므로, 그 Project를
    /// archive하면 참조 무결성이 아니라 **운영 의미**가 깨진다 — 조회만
    /// 가능해야 할 Project에 시작 가능한 Agent가 남는다.
    pub fn blocks_project_archive(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Agent 한 건.
///
/// `project_id`는 **생성 시점에 정해지고 이후 바뀌지 않는다**. 정본은 Agent를
/// 옮기는 대신 새로 만들라고 규정한다 — 소속을 바꿀 수 있으면 Project 경계가
/// 사후적으로 무너져 감사 추적이 불가능해지기 때문이다. 그래서 이 필드에는
/// 갱신 경로 자체를 만들지 않는다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub id: AgentId,
    /// 불변 소속 Project. 위 구조체 문서 참고.
    pub project_id: ProjectId,
    /// Project 안에서 고유한 표시 이름. Store가 `(project_id, name)`
    /// 유일성을 강제한다 — 이름은 Project 경계 안에서만 뜻이 통하므로
    /// 전역 유일성(`projects.name`)과 달리 범위를 좁힌다.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub status: AgentStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Agent {
    /// 신규 Agent 생성 (항상 `Ready`로 시작 — 위 모듈 문서 참고).
    pub fn new(project_id: ProjectId, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: AgentId::new(),
            project_id,
            name: name.into(),
            description: None,
            created_by: None,
            status: AgentStatus::Ready,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_created_by(mut self, created_by: impl Into<String>) -> Self {
        self.created_by = Some(created_by.into());
        self
    }
}

/// Agent 목록 조회 필터.
///
/// `project_id`가 `None`이면 모든 Project의 Agent를 본다. principal 단위
/// Project scope가 아직 없으므로 오늘은 이것이 관리자 조회이며, scope가
/// 들어오면 이 필터가 그 강제 지점이 된다.
#[derive(Debug, Clone, Default)]
pub struct AgentFilter {
    pub project_id: Option<ProjectId>,
    pub status: Option<AgentStatus>,
    pub limit: usize,
    pub offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_str_roundtrip() {
        for status in [AgentStatus::Ready, AgentStatus::Stopped] {
            assert_eq!(AgentStatus::parse_str(status.as_str()), Some(status));
        }
        assert_eq!(AgentStatus::parse_str("bogus"), None);
    }

    #[test]
    fn only_ready_blocks_project_archive() {
        assert!(AgentStatus::Ready.blocks_project_archive());
        assert!(!AgentStatus::Stopped.blocks_project_archive());
    }

    #[test]
    fn new_agent_starts_ready_with_matching_timestamps() {
        let project_id = ProjectId::new();
        let a = Agent::new(project_id, "reviewer");
        assert_eq!(a.status, AgentStatus::Ready);
        assert_eq!(a.project_id, project_id);
        assert_eq!(a.name, "reviewer");
        assert!(a.description.is_none());
        assert_eq!(a.created_at, a.updated_at);
    }

    #[test]
    fn builder_methods_set_optional_fields() {
        let a = Agent::new(ProjectId::new(), "reviewer")
            .with_description("reviews PRs")
            .with_created_by("alice");
        assert_eq!(a.description.as_deref(), Some("reviews PRs"));
        assert_eq!(a.created_by.as_deref(), Some("alice"));
    }
}
