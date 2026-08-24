//! Issue 도메인 모델과 상태 머신 (로드맵 #88).
//!
//! [Issue 추적 계약](../../../docs/architecture/issues.md)이 정본이다. Issue는
//! **Project가 해결해야 할 일감**이지 orchestrator의 인프라 장애 추적이 아니다
//! — 워커 도달 불가·credential 미프로비저닝 같은 운영 사건은 alert이며
//! [관측성·재조정](../../../docs/architecture/observability-and-reconciliation.md)이
//! 소유한다.
//!
//! ## 교착 없음 불변식 (I1/I2)
//!
//! Issue와 Task는 부모-자식이 아니라 **연관**이며, 두 상태 머신은 서로를 읽지
//! 않는다:
//!
//! - **I1**: 어떤 Task/Attempt 전이 조건도 `issue.status`를 읽지 않는다.
//! - **I2**: Issue의 close에는 Task 상태에 대한 선행 조건이 없다.
//!
//! 이 파일이 `crate::task`를 import하지 않는 것이 I1의 구조적 근거다 —
//! `Task` 상태 머신은 이 타입들의 존재조차 모른다. 연관은 `tasks` 테이블의
//! 컬럼이 아니라 join 테이블(`issue_task_links`)이 소유한다. `tasks`에
//! `issue_id`를 넣는 순간 Task 상태 머신이 Issue를 읽어야 하는 압력이 생기기
//! 때문이다.
//!
//! ## `InProgress`가 없는 이유
//!
//! "진행 중"은 비터미널 연관 Task가 있다는 사실에서 유도 가능하다. 상태로
//! 승격하면 Task 상태를 복제하게 되고, 그것이 두 상태 머신 경쟁의 시작점이다.
//! UI는 파생 배지로 표시하고 저장하지 않는다.
//!
//! ## 이 증분(#88)의 범위
//!
//! 엔티티·상태 머신·연관까지다. Agent가 여는 Issue(dedup key, `occurrence_count`,
//! `origin_attempt_id`, `author_kind=agent`)는 Worker control stream 보고 경로가
//! 필요해 `#89`, Agent의 backlog claim(claim lease, Project 예산, 계보 깊이
//! 상한)은 `#93`, HTTP/MCP 관리 표면은 `#92`가 소유한다. 그 필드들을 지금
//! 미리 만들지 않는 이유는 `#48`/`#70`과 같다 — 채울 방법이 없는 컬럼은 항상
//! `NULL`인 죽은 컬럼이 된다.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{IssueId, ProjectId, TaskId};

/// Issue 상태.
///
/// **`InProgress`가 없다** — 위 모듈 문서 참고.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    /// 사람 또는 agent가 막 보고한 상태.
    Open,
    /// 사람이 severity·labels·assignee를 지정했다.
    Triaged,
    /// 사람이 agent 착수를 승인했다. **이 상태 자체가 인가다** — Agent는
    /// 이 상태의 Issue만 claim할 수 있고(`#93`), 이 상태로의 전이는 사람만
    /// 할 수 있다(`issue:approve_agent_work`).
    ReadyForAgent,
    /// 해결 증거와 함께 사람이 판정했다. Task 성공이 자동으로 여기 오지
    /// 않는다 — 한 Issue가 여러 Task를 낳을 수 있고, Task가 성공해도 문제가
    /// 남아 있을 수 있다.
    Resolved,
    /// 종결. [`CloseReason`]을 반드시 가진다.
    Closed,
}

impl IssueStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Triaged => "triaged",
            Self::ReadyForAgent => "ready_for_agent",
            Self::Resolved => "resolved",
            Self::Closed => "closed",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "triaged" => Some(Self::Triaged),
            "ready_for_agent" => Some(Self::ReadyForAgent),
            "resolved" => Some(Self::Resolved),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }

    pub const ALL: [IssueStatus; 5] = [
        Self::Open,
        Self::Triaged,
        Self::ReadyForAgent,
        Self::Resolved,
        Self::Closed,
    ];

    /// 이 상태의 Issue가 아직 "열려 있는가" — `Closed`만 아니면 열린 것으로
    /// 본다. `Resolved`도 열려 있다(사람의 검증 후 `Closed`로 간다).
    ///
    /// dedup 부분 유니크 인덱스(`#89`)의 술어와 같은 정의다.
    pub fn is_open(self) -> bool {
        !matches!(self, Self::Closed)
    }
}

/// 종결 사유. `Closed` 상태의 필수 동반 값이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    /// 실제로 고쳤다.
    Fixed,
    /// 고치지 않기로 판단했다.
    WontFix,
    /// 다른 Issue와 중복이다.
    Duplicate,
    /// 더 이상 유효하지 않은 문제다.
    Obsolete,
}

impl CloseReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::WontFix => "wont_fix",
            Self::Duplicate => "duplicate",
            Self::Obsolete => "obsolete",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "fixed" => Some(Self::Fixed),
            "wont_fix" => Some(Self::WontFix),
            "duplicate" => Some(Self::Duplicate),
            "obsolete" => Some(Self::Obsolete),
            _ => None,
        }
    }

    pub const ALL: [CloseReason; 4] = [
        Self::Fixed,
        Self::WontFix,
        Self::Duplicate,
        Self::Obsolete,
    ];
}

/// Issue 심각도.
///
/// 값 어휘는 [관측성 계약](../../../docs/architecture/observability-and-reconciliation.md)의
/// alert severity(Critical/High/Medium)와 맞췄다 — 운영자가 두 곳에서 다른
/// 등급 체계를 외우지 않게 하기 위함이다. `Low`는 alert 표에는 없지만 일감
/// 트래커에는 필요해 추가했다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Critical,
    High,
    #[default]
    Medium,
    Low,
}

impl IssueSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "critical" => Some(Self::Critical),
            "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }

    pub const ALL: [IssueSeverity; 4] = [Self::Critical, Self::High, Self::Medium, Self::Low];
}

/// 상태 전이 거절 사유.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("issue transition {from} -> {to} is not allowed")]
    NotAllowed {
        from: &'static str,
        to: &'static str,
    },

    #[error("closing an issue requires a close_reason")]
    CloseReasonRequired,

    #[error("close_reason is only valid when closing an issue")]
    CloseReasonNotApplicable,
}

/// Issue 한 건.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub id: IssueId,
    /// 이 Issue가 속한 Project. Issue는 항상 Project 경계 안에 있다 —
    /// 일반 풀 Task와 달리 project 없는 Issue는 없다.
    pub project_id: ProjectId,
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub status: IssueStatus,
    /// `Closed`일 때만 `Some`. [`Issue::transition_to`]가 이 불변식을 강제한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<CloseReason>,
    pub severity: IssueSeverity,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Issue {
    /// 새 Issue. 항상 `Open`으로 시작한다 — 사람이 열든 agent가 열든(`#89`)
    /// 마찬가지다.
    pub fn new(
        project_id: ProjectId,
        title: impl Into<String>,
        created_by: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: IssueId::new(),
            project_id,
            title: title.into(),
            body: String::new(),
            status: IssueStatus::Open,
            close_reason: None,
            severity: IssueSeverity::default(),
            labels: Vec::new(),
            assignee: None,
            created_by: created_by.into(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    pub fn with_severity(mut self, severity: IssueSeverity) -> Self {
        self.severity = severity;
        self
    }

    pub fn with_labels(mut self, labels: Vec<String>) -> Self {
        self.labels = labels;
        self
    }

    /// 상태 전이를 검증하고 적용한다.
    ///
    /// 허용 간선은 [Issue 추적 계약](../../../docs/architecture/issues.md)의
    /// 상태 다이어그램 그대로다. **모든 간선이 사람의 행위다** — Agent는
    /// `[*] → Open`과 append만 할 수 있고(`#89`), 어떤 전이도 할 수 없다.
    /// 그 인가는 capability 계층이 강제하며(`issue:approve_agent_work` 등)
    /// 이 함수는 "상태 기계상 가능한 간선인가"만 판정한다.
    ///
    /// `Closed`로 갈 때는 `close_reason`이 필수이고, 그 외 상태로 갈 때는
    /// 반드시 `None`이어야 한다 — reopen이 이전 종결 사유를 끌고 다니면
    /// "왜 닫혔었나"와 "지금 왜 열려 있나"가 뒤섞인다.
    pub fn transition_to(
        &mut self,
        to: IssueStatus,
        close_reason: Option<CloseReason>,
    ) -> Result<(), TransitionError> {
        if !Self::transition_allowed(self.status, to) {
            return Err(TransitionError::NotAllowed {
                from: self.status.as_str(),
                to: to.as_str(),
            });
        }
        match (to, close_reason) {
            (IssueStatus::Closed, None) => return Err(TransitionError::CloseReasonRequired),
            (status, Some(_)) if status != IssueStatus::Closed => {
                return Err(TransitionError::CloseReasonNotApplicable)
            }
            _ => {}
        }
        self.status = to;
        self.close_reason = close_reason;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// 상태 기계의 허용 간선 판정 (순수 함수 — 테스트가 전이표 전체를 훑을
    /// 수 있게 공개한다).
    pub fn transition_allowed(from: IssueStatus, to: IssueStatus) -> bool {
        use IssueStatus::*;
        matches!(
            (from, to),
            (Open, Triaged)
                | (Triaged, ReadyForAgent)
                | (ReadyForAgent, Triaged)
                | (Open, Resolved)
                | (Triaged, Resolved)
                | (ReadyForAgent, Resolved)
                | (Open, Closed)
                | (Triaged, Closed)
                | (Resolved, Closed)
                | (Resolved, Open)
                | (Closed, Open)
        )
    }
}

/// Issue 코멘트 한 건 (append-only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueComment {
    pub id: uuid::Uuid,
    pub issue_id: IssueId,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

impl IssueComment {
    pub fn new(issue_id: IssueId, author: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            issue_id,
            author: author.into(),
            body: body.into(),
            created_at: Utc::now(),
        }
    }
}

/// Issue ↔ Task 연관 한 건.
///
/// `tasks`에 `issue_id` 컬럼을 두지 않는 이유는 모듈 문서(I1) 참고. Task가
/// 삭제되면 `task_id`는 `NULL`이 되고 `task_label`이 남는다 — 어떤 Task와
/// 엮여 있었는지는 Issue 이력의 일부이므로 Task와 함께 사라지면 안 된다
/// (`011_audit_log.sql`의 `actor_label` 보존과 같은 패턴).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueTaskLink {
    pub issue_id: IssueId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Task가 삭제돼도 남는 표시 문자열.
    pub task_label: String,
    pub linked_by: String,
    pub linked_at: DateTime<Utc>,
}

/// Issue 목록 조회 필터.
#[derive(Debug, Clone, Default)]
pub struct IssueFilter {
    pub project_id: Option<ProjectId>,
    pub status: Option<IssueStatus>,
    /// `true`면 `Closed`가 아닌 것만 (`IssueStatus::is_open`).
    pub open_only: bool,
    pub limit: usize,
    pub offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue() -> Issue {
        Issue::new(ProjectId::new(), "something is broken", "alice")
    }

    #[test]
    fn status_str_roundtrip() {
        for s in IssueStatus::ALL {
            assert_eq!(IssueStatus::parse_str(s.as_str()), Some(s));
        }
        assert_eq!(IssueStatus::parse_str("in_progress"), None);
    }

    #[test]
    fn close_reason_str_roundtrip() {
        for r in CloseReason::ALL {
            assert_eq!(CloseReason::parse_str(r.as_str()), Some(r));
        }
        assert_eq!(CloseReason::parse_str("bogus"), None);
    }

    #[test]
    fn severity_str_roundtrip() {
        for s in IssueSeverity::ALL {
            assert_eq!(IssueSeverity::parse_str(s.as_str()), Some(s));
        }
        assert_eq!(IssueSeverity::parse_str("bogus"), None);
    }

    // 구현 게이트 3 — `InProgress` 상태 부재. 문자열로도 파싱되지 않아야
    // 한다(과거 DB 값이나 외부 입력으로 슬쩍 들어오는 것까지 막는다).
    #[test]
    fn there_is_no_in_progress_status() {
        assert_eq!(IssueStatus::ALL.len(), 5);
        for s in IssueStatus::ALL {
            assert_ne!(s.as_str(), "in_progress");
        }
        assert!(IssueStatus::parse_str("in_progress").is_none());
    }

    #[test]
    fn new_issue_starts_open_with_no_close_reason() {
        let i = issue();
        assert_eq!(i.status, IssueStatus::Open);
        assert!(i.close_reason.is_none());
        assert_eq!(i.severity, IssueSeverity::Medium);
        assert_eq!(i.created_at, i.updated_at);
    }

    #[test]
    fn open_is_true_for_everything_except_closed() {
        for s in IssueStatus::ALL {
            assert_eq!(s.is_open(), s != IssueStatus::Closed, "{}", s.as_str());
        }
    }

    #[test]
    fn allowed_transitions_match_the_contract_diagram() {
        use IssueStatus::*;
        let expected = [
            (Open, Triaged),
            (Triaged, ReadyForAgent),
            (ReadyForAgent, Triaged),
            (Open, Resolved),
            (Triaged, Resolved),
            (ReadyForAgent, Resolved),
            (Open, Closed),
            (Triaged, Closed),
            (Resolved, Closed),
            (Resolved, Open),
            (Closed, Open),
        ];
        // 전이표 전체를 훑어 기대 집합과 정확히 일치하는지 확인 — 간선을
        // 하나 더 열거나 지우면 여기서 잡힌다.
        for from in IssueStatus::ALL {
            for to in IssueStatus::ALL {
                let allowed = Issue::transition_allowed(from, to);
                let should = expected.contains(&(from, to));
                assert_eq!(
                    allowed,
                    should,
                    "{} -> {} : allowed={allowed} expected={should}",
                    from.as_str(),
                    to.as_str()
                );
            }
        }
    }

    #[test]
    fn ready_for_agent_cannot_be_reached_directly_from_open() {
        // 사람의 triage를 반드시 거치게 하는 간선 부재 — agent 착수 승인이
        // 두 단계(triage → approve)를 거치도록 설계된 결과다.
        let mut i = issue();
        let err = i
            .transition_to(IssueStatus::ReadyForAgent, None)
            .unwrap_err();
        assert!(matches!(err, TransitionError::NotAllowed { .. }));
        assert_eq!(i.status, IssueStatus::Open, "거절된 전이는 상태를 바꾸지 않는다");
    }

    #[test]
    fn closing_requires_a_reason() {
        let mut i = issue();
        let err = i.transition_to(IssueStatus::Closed, None).unwrap_err();
        assert_eq!(err, TransitionError::CloseReasonRequired);
        assert_eq!(i.status, IssueStatus::Open);

        i.transition_to(IssueStatus::Closed, Some(CloseReason::WontFix))
            .unwrap();
        assert_eq!(i.status, IssueStatus::Closed);
        assert_eq!(i.close_reason, Some(CloseReason::WontFix));
    }

    #[test]
    fn non_close_transitions_reject_a_close_reason() {
        let mut i = issue();
        let err = i
            .transition_to(IssueStatus::Triaged, Some(CloseReason::Fixed))
            .unwrap_err();
        assert_eq!(err, TransitionError::CloseReasonNotApplicable);
    }

    #[test]
    fn reopening_clears_the_close_reason() {
        let mut i = issue();
        i.transition_to(IssueStatus::Closed, Some(CloseReason::Duplicate))
            .unwrap();
        assert_eq!(i.close_reason, Some(CloseReason::Duplicate));

        i.transition_to(IssueStatus::Open, None).unwrap();
        assert_eq!(i.status, IssueStatus::Open);
        assert!(
            i.close_reason.is_none(),
            "reopen must not carry the previous close reason forward"
        );
    }

    #[test]
    fn full_human_lifecycle_open_to_ready_to_resolved_to_closed() {
        let mut i = issue();
        i.transition_to(IssueStatus::Triaged, None).unwrap();
        i.transition_to(IssueStatus::ReadyForAgent, None).unwrap();
        i.transition_to(IssueStatus::Resolved, None).unwrap();
        i.transition_to(IssueStatus::Closed, Some(CloseReason::Fixed))
            .unwrap();
        assert_eq!(i.status, IssueStatus::Closed);
        assert_eq!(i.close_reason, Some(CloseReason::Fixed));
    }

    #[test]
    fn approval_can_be_withdrawn_back_to_triaged() {
        let mut i = issue();
        i.transition_to(IssueStatus::Triaged, None).unwrap();
        i.transition_to(IssueStatus::ReadyForAgent, None).unwrap();
        i.transition_to(IssueStatus::Triaged, None).unwrap();
        assert_eq!(i.status, IssueStatus::Triaged);
    }
}
