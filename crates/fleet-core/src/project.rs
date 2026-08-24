//! Project 도메인 모델 (로드맵 #48, 1단계).
//!
//! [Project 모델과 거버넌스](../../../docs/architecture/project-feature-design.md)와
//! [Lifecycle 계약](../../../docs/architecture/project-task-agent-lifecycle.md)이
//! 정본이다. 이 파일은 그 목표 모델의 **의도적으로 축소된 1단계**를 구현한다 —
//! Agent·TaskAttempt·effect ledger가 이 저장소에 아직 없어서, 목표 상태
//! 기계(`Draft`/`Active`/`Draining`/`ArchiveBlocked`/`Archived`)의 다음 부분은
//! 지금 만들 수 없다:
//!
//! - `Draft`: Agent template·policy·capacity 검증이 필요한데 그 검증 대상
//!   (AgentTemplate, 로드맵 #86)이 없다. 1단계에서는 모든 Project가 생성과
//!   동시에 `Active`다.
//! - `ArchiveBlocked`: Agent process/lease/credential grant cleanup 증거가
//!   필요한데 Agent·lease(로드맵 #67)·credential grant가 없다. 1단계의 archive
//!   게이트는 "이 Project를 참조하는 비종료 Task가 없다"는, 오늘 실제로 확인
//!   가능한 조건 하나뿐이다.
//!
//! `ProjectStatus`는 그래서 목표 5-상태가 아니라 `Active`/`Draining`/`Archived`
//! 3-상태다. 나머지 두 상태는 그 상태를 채울 하부 구조가 생기기 전까지
//! 추가하지 않는다 — 지금 만들면 항상 도달 불가능한 죽은 variant가 된다
//! (`#70` 조사에서 `FailureKind`의 죽은 variant 세 개를 발견해 제거한 것과
//! 같은 이유로, 처음부터 만들지 않는 쪽을 택한다).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::ProjectId;

/// Project 운영 상태 (목표 5-상태의 1단계 부분집합 — 위 모듈 문서 참고).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    /// 새 Task를 받는다. Task가 끝나도 Project는 그대로 Active다.
    Active,
    /// 새 Task를 막는다. 실행 중이던 Task는 이미 제출된 것이므로 그대로
    /// 끝까지 진행한다 — 1단계에는 deadline 강제 취소가 없다(Agent/Attempt
    /// 없이는 "실행 중인 것을 강제 종료"할 대상 자체가 없다).
    Draining,
    /// 조회는 가능하지만 새 Task를 받지 않는다. `Draining → Archived`는
    /// 이 Project를 참조하는 비종료 Task가 하나도 없을 때만 성립한다.
    Archived,
}

impl ProjectStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Draining => "draining",
            Self::Archived => "archived",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "draining" => Some(Self::Draining),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }

    /// 이 상태의 Project에 새 Task를 제출할 수 있는지.
    pub fn accepts_new_tasks(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// Project 한 건.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    /// 고유 표시 이름. Store가 유일성을 강제한다.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub status: ProjectStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Project {
    /// 신규 Project 생성 (항상 `Active`로 시작 — 위 모듈 문서의 `Draft` 관련
    /// 설명 참고).
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: ProjectId::new(),
            name: name.into(),
            description: None,
            created_by: None,
            status: ProjectStatus::Active,
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

/// Project 목록 조회 필터.
#[derive(Debug, Clone, Default)]
pub struct ProjectFilter {
    pub status: Option<ProjectStatus>,
    pub limit: usize,
    pub offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_str_roundtrip() {
        for status in [
            ProjectStatus::Active,
            ProjectStatus::Draining,
            ProjectStatus::Archived,
        ] {
            assert_eq!(ProjectStatus::parse_str(status.as_str()), Some(status));
        }
        assert_eq!(ProjectStatus::parse_str("bogus"), None);
    }

    #[test]
    fn only_active_accepts_new_tasks() {
        assert!(ProjectStatus::Active.accepts_new_tasks());
        assert!(!ProjectStatus::Draining.accepts_new_tasks());
        assert!(!ProjectStatus::Archived.accepts_new_tasks());
    }

    #[test]
    fn new_project_starts_active_with_matching_timestamps() {
        let p = Project::new("acme-web");
        assert_eq!(p.status, ProjectStatus::Active);
        assert_eq!(p.name, "acme-web");
        assert!(p.description.is_none());
        assert_eq!(p.created_at, p.updated_at);
    }

    #[test]
    fn builder_methods_set_optional_fields() {
        let p = Project::new("acme-web")
            .with_description("main web app")
            .with_created_by("alice");
        assert_eq!(p.description.as_deref(), Some("main web app"));
        assert_eq!(p.created_by.as_deref(), Some("alice"));
    }
}
