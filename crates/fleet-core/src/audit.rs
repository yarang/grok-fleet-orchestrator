//! 구조화된 감사 로그 타입.
//!
//! 인증·권한 이벤트를 `tracing` 출력이 아니라 질의 가능한 형태로 보존하기
//! 위한 타입들. 저장은 `fleet-store`의 `record_audit_event`가 담당한다.
//!
//! ## 기록 원칙
//!
//! - **비밀 값을 넣지 않는다.** 비밀번호, 세션 토큰, 재설정 토큰 원문은
//!   `detail`에도 절대 넣지 않는다. 감사 로그는 보관 기간이 길고 열람 범위가
//!   넓어서, 여기 유출되면 회수할 방법이 없다.
//! - **행위자 표시 문자열을 함께 남긴다.** 사용자가 삭제되면 `actor_user_id`는
//!   NULL이 되지만 `actor_label`은 남아 누구였는지 추적할 수 있다.
//! - **실패도 기록한다.** 감사에서 중요한 건 성공한 행위보다 거부된 시도인
//!   경우가 많다.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::UserId;

/// 감사 이벤트의 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditOutcome {
    Success,
    Failure,
}

impl AuditOutcome {
    /// DB 저장용 문자열 (`outcome` 컬럼의 CHECK 제약과 일치해야 함).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
        }
    }

    /// DB 문자열에서 복원. 알 수 없는 값은 `None`.
    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "success" => Some(Self::Success),
            "failure" => Some(Self::Failure),
            _ => None,
        }
    }
}

/// 감사 액션 이름 상수.
///
/// 문자열 리터럴을 호출부에 흩뿌리면 오타가 나도 컴파일이 통과하고, 나중에
/// 액션별 질의가 조용히 빈 결과를 낸다. 여기 모아 단일 출처로 둔다.
pub mod action {
    /// 로그인 성공/실패.
    pub const AUTH_LOGIN: &str = "auth.login";
    /// 로그아웃.
    pub const AUTH_LOGOUT: &str = "auth.logout";
    /// 비밀번호 재설정 완료.
    pub const AUTH_PASSWORD_RESET: &str = "auth.password_reset";
    /// 이메일 인증 완료.
    pub const AUTH_EMAIL_VERIFIED: &str = "auth.email_verified";
    /// 관리자 부트스트랩.
    pub const AUTH_BOOTSTRAP: &str = "auth.bootstrap";
    /// 사용자 생성.
    pub const USER_CREATE: &str = "user.create";
    /// 사용자 활성/비활성 전환.
    pub const USER_TOGGLE: &str = "user.toggle";
    /// 사용자 삭제.
    pub const USER_DELETE: &str = "user.delete";
    /// worker LLM credential 평문 export (로드맵 #66).
    ///
    /// 이 액션은 "누군가 API 키 원문을 가져갔다"는 뜻이다. 키가 유출됐을 때
    /// 회수 범위를 정하려면 이 기록이 유일한 근거이므로, 기록에 실패하면
    /// export 자체를 거부한다.
    pub const WORKER_LLM_CREDENTIAL_EXPORT: &str = "worker.llm_credential.export";
    /// worker LLM credential 저장/회전 (로드맵 #66).
    pub const WORKER_LLM_CREDENTIAL_PUT: &str = "worker.llm_credential.put";
    /// worker LLM credential 삭제 (로드맵 #66).
    pub const WORKER_LLM_CREDENTIAL_DELETE: &str = "worker.llm_credential.delete";
    /// bootstrap token 발급 (로드맵 #76).
    pub const TOKEN_BOOTSTRAP_ISSUE: &str = "token.bootstrap.issue";
    /// bootstrap token 회수 (로드맵 #76).
    pub const TOKEN_BOOTSTRAP_REVOKE: &str = "token.bootstrap.revoke";
    /// admin API bearer token 발급 (로드맵 #76).
    pub const ADMIN_TOKEN_CREATE: &str = "admin_token.create";
    /// admin API bearer token 회전 (로드맵 #76).
    pub const ADMIN_TOKEN_ROTATE: &str = "admin_token.rotate";
    /// admin API bearer token 회수 (로드맵 #76).
    pub const ADMIN_TOKEN_REVOKE: &str = "admin_token.revoke";
    /// worker 등록/재등록 (로드맵 #76). 고빈도인 heartbeat는 감사 대상이
    /// 아니다 — register만 identity 변경에 해당한다.
    pub const WORKER_REGISTER: &str = "worker.register";
    /// worker 등록 해제 (로드맵 #76).
    pub const WORKER_DEREGISTER: &str = "worker.deregister";
    /// host 등록 (로드맵 #76).
    pub const HOST_REGISTER: &str = "host.register";
    /// HTTP capability 거절 (로드맵 #76). 인증까지 통과한 principal이
    /// 대상이다 — 미인증 요청은 이 이벤트 이전에 이미 401로 걸러진다.
    pub const HTTP_CAPABILITY_DENIED: &str = "http.capability_denied";
    /// Project 생성 (로드맵 #48).
    pub const PROJECT_CREATE: &str = "project.create";
    /// Project archive 요청(`Active → Draining`) (로드맵 #48).
    pub const PROJECT_ARCHIVE_REQUESTED: &str = "project.archive_requested";
    /// Project가 실제로 `Archived`에 도달함 (로드맵 #48).
    pub const PROJECT_ARCHIVED: &str = "project.archived";
    /// Issue 생성 (로드맵 #92).
    pub const ISSUE_CREATE: &str = "issue.create";
    /// Issue 상태 전이 (로드맵 #92). `detail.to`에 목표 상태가 들어간다 —
    /// `ready_for_agent`로의 전이는 Agent 자동 착수의 인가 지점이므로
    /// 누가 승인했는지가 감사에 남아야 한다.
    pub const ISSUE_TRANSITION: &str = "issue.transition";
    /// Task 영구 삭제 시도 — 성공·거부 모두 기록한다 (로드맵 #96).
    ///
    /// `events.task_id`는 `ON DELETE SET NULL`이지만 `events.payload`는
    /// `FleetEvent`를 통째로 JSONB로 담고 있어 원본 `task_id`를 잃지 않는다
    /// (`docs/architecture/tasks/management.md` "무엇이 함께 사라지는가"
    /// 참고). 이 감사 이벤트가 증언하는 것은 "그 Task가 존재했다"가 아니라
    /// "언제 누구에 의해 지워졌는가"이며, `actor`/`target`이 인덱스가 있는
    /// 자리에 남는 조회 가능한 유일한 경로라는 뜻이다.
    pub const TASK_DELETE: &str = "task.delete";
}

/// 감사 로그 한 건.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Uuid,
    /// 행위자. 미인증 이벤트(로그인 실패 등)는 `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_user_id: Option<UserId>,
    /// 행위자 표시 문자열 (username / email / "system").
    /// 사용자가 삭제되어 `actor_user_id`가 NULL이 되어도 남는다.
    pub actor_label: String,
    /// [`action`] 모듈의 상수 중 하나.
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub outcome: AuditOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    /// 액션별 추가 맥락. **비밀 값 금지.**
    #[serde(default)]
    pub detail: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl AuditEvent {
    /// 성공 이벤트 생성 (id/created_at 자동).
    pub fn success(actor_label: impl Into<String>, action: impl Into<String>) -> Self {
        Self::new(actor_label, action, AuditOutcome::Success)
    }

    /// 실패 이벤트 생성 (id/created_at 자동).
    pub fn failure(actor_label: impl Into<String>, action: impl Into<String>) -> Self {
        Self::new(actor_label, action, AuditOutcome::Failure)
    }

    fn new(
        actor_label: impl Into<String>,
        action: impl Into<String>,
        outcome: AuditOutcome,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            actor_user_id: None,
            actor_label: actor_label.into(),
            action: action.into(),
            target_type: None,
            target_id: None,
            outcome,
            ip_address: None,
            detail: serde_json::Value::Null,
            created_at: Utc::now(),
        }
    }

    /// 행위자 사용자 ID 지정.
    pub fn actor(mut self, user_id: UserId) -> Self {
        self.actor_user_id = Some(user_id);
        self
    }

    /// 대상 지정 (종류, 식별자).
    pub fn target(mut self, kind: impl Into<String>, id: impl Into<String>) -> Self {
        self.target_type = Some(kind.into());
        self.target_id = Some(id.into());
        self
    }

    /// 요청 출처 IP 지정.
    pub fn ip(mut self, ip: impl Into<String>) -> Self {
        self.ip_address = Some(ip.into());
        self
    }

    /// 추가 맥락 지정. **비밀 값을 넣지 말 것.**
    pub fn detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = detail;
        self
    }
}

/// 감사 로그 조회 필터.
#[derive(Debug, Clone)]
pub struct AuditFilter {
    /// 특정 행위자만.
    pub actor_user_id: Option<UserId>,
    /// 특정 액션만 (정확히 일치).
    pub action: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

impl Default for AuditFilter {
    fn default() -> Self {
        Self {
            actor_user_id: None,
            action: None,
            limit: 100,
            offset: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_roundtrip() {
        assert_eq!(
            AuditOutcome::parse_str(AuditOutcome::Success.as_str()),
            Some(AuditOutcome::Success)
        );
        assert_eq!(
            AuditOutcome::parse_str(AuditOutcome::Failure.as_str()),
            Some(AuditOutcome::Failure)
        );
        assert_eq!(AuditOutcome::parse_str("bogus"), None);
    }

    #[test]
    fn builder_sets_fields() {
        let user_id = UserId::new();
        let ev = AuditEvent::success("alice", action::USER_DELETE)
            .actor(user_id)
            .target("user", "bob-id")
            .ip("10.0.0.1")
            .detail(serde_json::json!({ "reason": "offboarding" }));

        assert_eq!(ev.actor_label, "alice");
        assert_eq!(ev.action, "user.delete");
        assert_eq!(ev.actor_user_id, Some(user_id));
        assert_eq!(ev.target_type.as_deref(), Some("user"));
        assert_eq!(ev.target_id.as_deref(), Some("bob-id"));
        assert_eq!(ev.ip_address.as_deref(), Some("10.0.0.1"));
        assert_eq!(ev.outcome, AuditOutcome::Success);
        assert_eq!(ev.detail["reason"], "offboarding");
    }

    #[test]
    fn failure_events_have_failure_outcome() {
        let ev = AuditEvent::failure("attacker@example.com", action::AUTH_LOGIN);
        assert_eq!(ev.outcome, AuditOutcome::Failure);
        assert!(ev.actor_user_id.is_none());
    }
}
