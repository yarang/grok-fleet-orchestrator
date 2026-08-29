//! AgentTemplate 도메인 모델 (로드맵 #86, 1단계).
//!
//! [AgentTemplate 정본](../../../docs/architecture/agents/agent-template.md)이
//! 정본이다. 이 파일은 그 결정의 **정체성·revision 계층**을 구현한다.
//!
//! # 왜 두 계층인가
//!
//! `agent_templates`는 정체성(이름, 소유 Project, 수명 주기 상태)을,
//! `agent_template_revisions`는 **불변 본문**을 담는다. 하나로 합치면
//! "본문을 고치면 새 revision"이라는 규칙을 표현할 방법이 없어진다 —
//! 상태 전이(`Deprecated` 등)마다 본문이 복제되거나, 본문 수정이 과거
//! revision을 덮어써 `#65`의 재현성이 깨진다.
//!
//! 참조는 3-tuple `(template_id, content_revision, content_hash)`다.
//! content_hash 단독으로는 revoke 상태를 붙일 곳이 없고 Project 사이의
//! 인가 경계가 합쳐지며, id+revision 단독으로는 저장소 마이그레이션 뒤
//! 본문이 그대로라는 증명이 없다.
//!
//! # 1단계가 만들지 않는 것
//!
//! | 항목 | 왜 미뤘나 | 선행 |
//! |---|---|---|
//! | `isolation_class` 필드 | 등급을 표현할 타입이 코드에 없다 — `ReadOnly` 같은 이름이 저장소 전체에 0건이다. 검증할 집합 없이 자유 문자열로 만들면 값이 아무 뜻도 갖지 않는다 | `#52` |
//! | Project 정책과의 교집합(`tools_effective`) | `projects`에 `allow`/`deny` 정책 컬럼이 하나도 없어 교집합할 상대가 없다. 판정 시점인 Task admission도 없다 | `#48` 정책 컬럼 |
//! | `projects.default_agent_template_id` | 쓰는 주체(자동 provisioning)도 읽는 주체도 없다 | `#49` 2단계 |
//! | `builtin/default@1` 시드 삽입 | 정본이 요구하는 "tool binding이 `ReadOnly` 등급 한정"을 단정할 타입이 없다. 대신 본문 상수와 그 `content_hash`를 [`AgentTemplateBody::builtin_default`]로 고정해 두 Store가 같은 값을 내는지는 지금 증명한다 | 위 `#52`와 동일 |
//! | retire 시 의존 집합 해시 | 1단계에는 의존 주체가 **pin된 Agent 하나뿐**이라 집합이 좁다. 그래서 해시는 구현하되(`#87`의 Attempt pin이 들어와도 인코딩이 그대로 확장된다) 그 범위를 문서로 못박는다 | `#87` |
//!
//! `027_agents.sql`은 `agent_template_id` 컬럼을 만들지 않으면서 그 이유를
//! "채울 주체(AgentTemplate `#86`)가 없어 항상 NULL인 컬럼이 된다"로 적었다.
//! 그 전제가 이 항목에서 해소되므로 `029`가 그 컬럼을 만든다. 부수 효과가
//! 본질이다 — pin이 생겨야 의존 집합이 공집합이 아니게 되고, retire와
//! revision revoke가 비로소 **막을 대상**을 갖는다.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::PermissionKind;
use crate::ids::{AgentId, AgentTemplateId, AgentTemplateRevisionId, ProjectId};

/// AgentTemplate 정체성의 수명 주기 상태.
///
/// 다섯 상태 전부 **관리 전이만으로 도달한다** — 실행 중인 Agent 프로세스도
/// Worker control stream(`#67` 4단계)도 필요하지 않다. [`crate::agent::AgentStatus`]가
/// 목표 8-상태 중 둘만 낸 것과 대비되는 지점이며, 그래서 `#86`은 다른
/// Agent 항목들과 달라 지금 착수할 수 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTemplateStatus {
    /// 작성 중. revision을 붙일 수 있지만 pin 대상이 아니다.
    Draft,
    /// 사용 가능. 새 pin을 받는다.
    Published,
    /// 소프트 경고. **기동을 막지 않는다** — 정본이 "`Deprecated`는 경고
    /// metric일 뿐"으로 규정한다. 새 pin도 계속 허용한다.
    Deprecated,
    /// 종료. 새 pin을 거절한다. 이미 pin한 Agent의 기동은 admission에서
    /// `TemplateUnavailable`로 막히는데, 그 admission이 없는 1단계에서는
    /// **새 pin 거절**이 이 상태가 집행하는 전부다.
    Retired,
    /// `Draft`를 publish하지 않고 버렸다. `Published`를 거친 템플릿은 이
    /// 상태로 갈 수 없다 — 이미 누군가 pin했을 수 있기 때문이다.
    Discarded,
}

impl AgentTemplateStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
            Self::Deprecated => "deprecated",
            Self::Retired => "retired",
            Self::Discarded => "discarded",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "published" => Some(Self::Published),
            "deprecated" => Some(Self::Deprecated),
            "retired" => Some(Self::Retired),
            "discarded" => Some(Self::Discarded),
            _ => None,
        }
    }

    /// 허용된 전이인지.
    ///
    /// **`Retired → Published` 간선은 없다** — 정본의 구현 게이트 1이
    /// 명시적으로 그 부재를 시험하라고 요구한다. 종료된 템플릿을 되살리는
    /// 대신 새 정체성을 만들라는 뜻이며, 그래야 "이 이름은 한때 종료됐다"는
    /// 사실이 감사 기록에서 사라지지 않는다.
    ///
    /// `Deprecated → Published`는 **있다**. 경고를 거두는 것은 아무 권한도
    /// 넓히지 않는다 — `Deprecated`가 이미 새 pin을 허용하고 있으므로
    /// 되돌려도 도달 가능한 것이 늘지 않는다.
    pub fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }

    /// 이 상태에서 갈 수 있는 상태들 — **전이표의 유일한 정의**.
    ///
    /// [`Self::can_transition_to`]가 이것을 조회하도록 뒤집어 둔 이유는
    /// 관리 화면이다. 화면은 누를 수 있는 버튼만 그려야 하는데, 그러려면
    /// 표를 **열거**할 수 있어야 한다. 판정 함수만 있으면 호출자가 후보
    /// 상태 목록을 직접 들고 있어야 하고, 그 목록이 두 번째 표가 된다.
    ///
    /// `self`에 대해 exhaustive하므로 상태를 추가하면 컴파일이 깨진다 —
    /// 새 상태가 "아무 데도 못 가는 상태"로 조용히 취급되지 않는다.
    pub fn allowed_transitions(self) -> &'static [Self] {
        match self {
            Self::Draft => &[Self::Published, Self::Discarded],
            Self::Published => &[Self::Deprecated, Self::Retired],
            Self::Deprecated => &[Self::Published, Self::Retired],
            // 둘 다 종료 상태다. 나가는 간선이 없다는 것이 종료의 정의다.
            Self::Retired | Self::Discarded => &[],
        }
    }

    /// 이 상태의 템플릿에 새 pin을 붙일 수 있는지.
    pub fn accepts_new_pins(self) -> bool {
        matches!(self, Self::Published | Self::Deprecated)
    }

    /// 이 상태에서 새 revision을 만들 수 있는지.
    ///
    /// `Retired`/`Discarded`는 종료 상태라 본문을 더 붙일 수 없다. 종료된
    /// 템플릿에 revision을 붙일 수 있으면 "종료"가 뜻을 잃는다.
    pub fn accepts_new_revisions(self) -> bool {
        matches!(self, Self::Draft | Self::Published | Self::Deprecated)
    }
}

/// revision 본문 — 이것이 `content_hash`의 유일한 입력이다.
///
/// `isolation_class`가 없는 이유는 모듈 문서의 유예 표 참고.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTemplateBody {
    pub role_prompt: String,
    /// 허용 tool 이름. **집합**으로 다룬다 — 정렬·중복 제거된 형태만
    /// 저장한다([`AgentTemplateBody::normalized`]).
    #[serde(default)]
    pub tools: Vec<String>,
    /// 요구 skill 이름. `tools`와 같은 정규화 규칙.
    #[serde(default)]
    pub skills: Vec<String>,
}

fn hash_field(h: &mut Sha256, name: &[u8], value: &[u8]) {
    h.update(name);
    h.update(b":");
    h.update(value.len().to_string().as_bytes());
    h.update(b":");
    h.update(value);
    h.update(b"\n");
}

fn hash_list(h: &mut Sha256, name: &[u8], items: &[String]) {
    h.update(name);
    h.update(b":");
    h.update(items.len().to_string().as_bytes());
    h.update(b"\n");
    for item in items {
        hash_field(h, b"item", item.as_bytes());
    }
}

impl AgentTemplateBody {
    pub fn new(role_prompt: impl Into<String>) -> Self {
        Self {
            role_prompt: role_prompt.into(),
            tools: Vec::new(),
            skills: Vec::new(),
        }
    }

    pub fn with_tools<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tools = tools.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_skills<I, S>(mut self, skills: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.skills = skills.into_iter().map(Into::into).collect();
        self
    }

    /// 정규화 — `tools`/`skills`를 정렬하고 중복을 제거한다.
    ///
    /// 이것이 있어야 "같은 내용이면 같은 `content_hash`"가 **집합 의미로**
    /// 참이 된다. 목록 순서가 hash를 바꾸면 같은 권한 집합이 다른 revision을
    /// 만들고, 그러면 정본이 요구하는 "같은 내용을 다시 publish하면 새
    /// revision id에 같은 `content_hash`"가 입력 순서에 따라 깨진다.
    ///
    /// 저장 전에 호출해 **정규화된 형태만** 저장한다 — 저장된 값과 hash의
    /// 입력이 달라지면 나중에 hash를 재계산해 검증할 수 없다.
    pub fn normalized(&self) -> Self {
        let norm = |v: &Vec<String>| -> Vec<String> {
            let mut out: Vec<String> = v
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            out.sort();
            out.dedup();
            out
        };
        Self {
            role_prompt: self.role_prompt.clone(),
            tools: norm(&self.tools),
            skills: norm(&self.skills),
        }
    }

    /// 정규화된 본문의 SHA-256 (hex).
    ///
    /// 인코딩은 **길이 접두**를 쓴다. 단순 연결이면 `["ab", "c"]`와
    /// `["a", "bc"]`가 같은 바이트열이 되어 서로 다른 tool 집합이 같은
    /// hash를 갖는다 — 이 hash가 인가 경계의 재현성 근거이므로 그 충돌을
    /// 허용할 수 없다.
    ///
    /// HashMap 순회나 부동소수처럼 플랫폼마다 달라지는 입력이 없으므로
    /// MemStore와 PgStore, macOS와 Linux가 같은 값을 낸다.
    pub fn content_hash(&self) -> String {
        let n = self.normalized();
        let mut h = Sha256::new();
        h.update(b"agent_template/v1\n");
        hash_field(&mut h, b"role_prompt", n.role_prompt.as_bytes());
        hash_list(&mut h, b"tools", &n.tools);
        hash_list(&mut h, b"skills", &n.skills);
        hex::encode(h.finalize())
    }

    /// `builtin/default` 템플릿의 본문.
    ///
    /// 아직 어떤 Store도 이것을 자동 삽입하지 않는다 — 정본이 요구하는
    /// "tool binding이 `ReadOnly` 등급 한정"을 단정할 타입이 없기 때문이다
    /// (모듈 문서의 유예 표). 그럼에도 상수로 두는 이유는, 시드가 들어올 때
    /// 두 Store가 같은 `content_hash`를 내야 한다는 요구가 **본문이
    /// 결정적인지**에 달려 있고 그것은 지금 증명할 수 있기 때문이다.
    pub fn builtin_default() -> Self {
        Self {
            role_prompt: "You are a general-purpose software engineering agent. \
Read before you write, and state what you could not verify."
                .to_string(),
            tools: vec!["fs.read".to_string(), "shell.run".to_string()],
            skills: Vec::new(),
        }
    }

    /// 이 편집이 요구하는 capability 집합 (정본의 "편집 권한의 필드별 게이팅").
    ///
    /// `role_prompt`는 **데이터이지 권한이 아니다** — 어떤 capability 판정의
    /// 입력도 되지 않으므로 `agent_template:update`만으로 고칠 수 있다.
    /// 반면 `tools`/`skills`는 Agent가 무엇을 할 수 있는지를 정하므로 Agent
    /// tool-binding 권한을 함께 요구한다. 정본은 그 권한을 `AgentManage`
    /// 상당으로 규정했다.
    ///
    /// 두 필드가 동시에 바뀌면 두 권한을 모두 요구한다 — 한쪽만 있으면
    /// 거절이지 "가능한 부분만 적용"이 아니다. 부분 적용은 요청자가 보낸
    /// 것과 다른 본문을 저장하는 것이고, 그 본문이 그대로 hash되어 감사
    /// 근거가 된다.
    pub fn required_permissions_for_change(&self, next: &Self) -> Vec<PermissionKind> {
        let a = self.normalized();
        let b = next.normalized();
        let mut out = Vec::new();
        if a != b {
            out.push(PermissionKind::AgentTemplateUpdate);
        }
        if a.tools != b.tools || a.skills != b.skills {
            out.push(PermissionKind::AgentManage);
        }
        out
    }
}

/// AgentTemplate 정체성 한 건.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub id: AgentTemplateId,
    /// `None`이면 전역 템플릿이다. 전역 템플릿의 관리에는 별도 capability
    /// (`agent_template:manage_global`)를 요구한다 — Project 관리자가 전역
    /// 템플릿을 고칠 수 있으면 자기 경계 밖에 영향을 준다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub status: AgentTemplateStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentTemplate {
    /// 새 템플릿은 항상 `Draft`로 시작한다.
    pub fn new(project_id: Option<ProjectId>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: AgentTemplateId::new(),
            project_id,
            name: name.into(),
            description: None,
            created_by: None,
            status: AgentTemplateStatus::Draft,
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

/// 불변 revision 한 건.
///
/// **갱신 경로를 두지 않는다.** 본문 필드를 고치는 Store 메서드가 없는 것이
/// immutability의 집행 방법이다 — DB 트리거가 아니라 "그런 함수가 없음"으로
/// 막는다. [`crate::agent::Agent`]의 `project_id`가 불변인 것과 같은 방식.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTemplateRevision {
    pub id: AgentTemplateRevisionId,
    pub template_id: AgentTemplateId,
    /// 템플릿 안에서 1부터 증가. Store가 할당한다 — 호출자가 정하면 경합에서
    /// 같은 번호 두 개가 생긴다.
    pub content_revision: i32,
    /// [`AgentTemplateBody::content_hash`]의 값. 저장된 본문에서 재계산해
    /// 검증할 수 있다.
    pub content_hash: String,
    pub body: AgentTemplateBody,
    /// `Some`이면 이 revision에 **새 pin을 붙일 수 없다**. 이미 pin한
    /// Agent는 영향받지 않는다 — 과거 실행의 재현성을 사후에 깨뜨리지
    /// 않는다는 것이 revision immutability의 요지다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl AgentTemplateRevision {
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }
}

/// Agent가 어느 템플릿 revision으로 만들어졌는지.
///
/// 두 id를 **하나의 구조체로 묶는다.** 별개의 `Option` 필드 두 개로 두면 한쪽만
/// 채워진 값이 타입 차원에서 표현 가능해지고, 그러면 `029`의
/// `agents_template_pin_complete` CHECK가 런타임에만 걸리는 방어가 된다.
/// 묶어 두면 그 상태를 애초에 만들 수 없다.
///
/// `content_hash`는 여기 없다 — revision id가 이미 본문 한 판을 유일하게
/// 가리키므로 hash를 복제하면 두 곳이 어긋날 여지만 생긴다. 3-tuple 참조가
/// 필요한 곳은 [`AgentTemplateRef`]가 담당한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTemplatePin {
    pub template_id: AgentTemplateId,
    pub revision_id: AgentTemplateRevisionId,
}

/// 정본의 3-tuple 참조 `(template_id, content_revision, content_hash)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTemplateRef {
    pub template_id: AgentTemplateId,
    pub content_revision: i32,
    pub content_hash: String,
}

impl From<&AgentTemplateRevision> for AgentTemplateRef {
    fn from(r: &AgentTemplateRevision) -> Self {
        Self {
            template_id: r.template_id,
            content_revision: r.content_revision,
            content_hash: r.content_hash.clone(),
        }
    }
}

/// 템플릿 목록 조회 필터.
#[derive(Debug, Clone, Default)]
pub struct AgentTemplateFilter {
    /// `Some(None)`은 "전역 템플릿만", `Some(Some(p))`는 "그 Project만",
    /// `None`은 "전부". 두 겹의 `Option`이 어색하지만, 전역 템플릿을
    /// **명시적으로** 고를 수 있어야 관리 화면이 그 범위를 표현할 수 있다.
    pub project_scope: Option<Option<ProjectId>>,
    pub status: Option<AgentTemplateStatus>,
    pub limit: usize,
    pub offset: usize,
}

/// 의존 집합 해시.
///
/// 정본은 retire에 "의존 집합 해시 제출"을 요구하고 집합이 바뀌었으면
/// `409`를 내라고 규정한다. 확인 화면이 보여준 목록과 실제로 회수되는
/// 목록이 다를 수 있기 때문이다.
///
/// **1단계의 의존 집합은 이 템플릿을 pin한 Agent뿐이다.** `#87`이 Attempt
/// pin을 넣으면 그 id들이 같은 인코딩으로 이어 붙는다 — 길이 접두라
/// 항목 종류가 늘어도 충돌하지 않는다.
pub fn dependent_set_hash(agent_ids: &[AgentId]) -> String {
    let mut sorted: Vec<String> = agent_ids.iter().map(|a| a.to_string()).collect();
    sorted.sort();
    let mut h = Sha256::new();
    h.update(b"agent_template_dependents/v1\n");
    hash_list(&mut h, b"agents", &sorted);
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_str_roundtrip() {
        for s in [
            AgentTemplateStatus::Draft,
            AgentTemplateStatus::Published,
            AgentTemplateStatus::Deprecated,
            AgentTemplateStatus::Retired,
            AgentTemplateStatus::Discarded,
        ] {
            assert_eq!(AgentTemplateStatus::parse_str(s.as_str()), Some(s));
        }
        assert_eq!(AgentTemplateStatus::parse_str("bogus"), None);
    }

    /// 정본 구현 게이트 1 — 전이표와 `Retired → Published`의 부재.
    #[test]
    fn retired_is_terminal_and_never_returns_to_published() {
        use AgentTemplateStatus::*;
        for next in [Draft, Published, Deprecated, Retired, Discarded] {
            assert!(
                !Retired.can_transition_to(next),
                "Retired must have no outgoing edge, found one to {next:?}"
            );
            assert!(
                !Discarded.can_transition_to(next),
                "Discarded must have no outgoing edge, found one to {next:?}"
            );
        }
    }

    #[test]
    fn transition_table_matches_canon() {
        use AgentTemplateStatus::*;
        assert!(Draft.can_transition_to(Published));
        assert!(Draft.can_transition_to(Discarded));
        assert!(Published.can_transition_to(Deprecated));
        assert!(Published.can_transition_to(Retired));
        assert!(Deprecated.can_transition_to(Published));
        assert!(Deprecated.can_transition_to(Retired));
        // published 이후에는 폐기(discard)할 수 없다 — 이미 pin됐을 수 있다.
        assert!(!Published.can_transition_to(Discarded));
        assert!(!Deprecated.can_transition_to(Discarded));
        // Draft는 pin 대상이 아니므로 곧바로 종료할 수 없다.
        assert!(!Draft.can_transition_to(Retired));
        // 자기 자신으로의 전이는 없다 — 호출부가 no-op을 전이로 착각하면
        // `updated_at`이 의미 없이 밀린다.
        for s in [Draft, Published, Deprecated, Retired, Discarded] {
            assert!(!s.can_transition_to(s));
        }
    }

    #[test]
    fn allowed_transitions_enumerates_without_duplicates() {
        use AgentTemplateStatus::*;
        // `can_transition_to`는 이제 이 목록의 `contains`이므로 위 테스트가
        // 간선의 유무는 전부 덮는다. 덮지 못하는 것이 하나 있다 — **중복**
        // 이다. `[Published, Published]`도 `contains`는 똑같이 통과하지만
        // 관리 화면은 같은 버튼을 두 번 그린다. 열거를 소비하는 쪽이 생겼기
        // 때문에 생긴 새 요구라 여기서 따로 잠근다.
        for s in [Draft, Published, Deprecated, Retired, Discarded] {
            let list = s.allowed_transitions();
            let mut uniq = list.to_vec();
            uniq.sort_by_key(|t| t.as_str());
            uniq.dedup();
            assert_eq!(uniq.len(), list.len(), "{s:?}의 전이 목록에 중복이 있다");
        }
        assert!(Retired.allowed_transitions().is_empty());
        assert!(Discarded.allowed_transitions().is_empty());
    }

    #[test]
    fn only_published_and_deprecated_accept_pins() {
        use AgentTemplateStatus::*;
        assert!(Published.accepts_new_pins());
        // Deprecated는 경고일 뿐 차단이 아니다 — 정본 명시.
        assert!(Deprecated.accepts_new_pins());
        assert!(!Draft.accepts_new_pins());
        assert!(!Retired.accepts_new_pins());
        assert!(!Discarded.accepts_new_pins());
    }

    #[test]
    fn terminal_states_accept_no_new_revisions() {
        use AgentTemplateStatus::*;
        assert!(Draft.accepts_new_revisions());
        assert!(Published.accepts_new_revisions());
        assert!(Deprecated.accepts_new_revisions());
        assert!(!Retired.accepts_new_revisions());
        assert!(!Discarded.accepts_new_revisions());
    }

    #[test]
    fn normalization_sorts_and_dedups() {
        let b = AgentTemplateBody::new("p").with_tools(["b", "a", " b ", "", "  "]);
        let n = b.normalized();
        assert_eq!(n.tools, vec!["a".to_string(), "b".to_string()]);
    }

    /// 정본 구현 게이트 2의 절반 — 같은 내용이면 같은 `content_hash`.
    #[test]
    fn content_hash_is_order_and_whitespace_independent() {
        let a = AgentTemplateBody::new("prompt").with_tools(["fs.read", "shell.run"]);
        let b = AgentTemplateBody::new("prompt").with_tools([" shell.run ", "fs.read", "fs.read"]);
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn content_hash_distinguishes_fields() {
        let base = AgentTemplateBody::new("p");
        assert_ne!(
            base.content_hash(),
            AgentTemplateBody::new("q").content_hash()
        );
        assert_ne!(
            base.clone().with_tools(["a"]).content_hash(),
            base.clone().with_skills(["a"]).content_hash(),
            "a name in tools must not hash the same as the same name in skills"
        );
    }

    /// 길이 접두가 없으면 이 두 입력이 같은 바이트열이 된다.
    #[test]
    fn content_hash_resists_concatenation_collision() {
        let a = AgentTemplateBody::new("p").with_tools(["ab", "c"]);
        let b = AgentTemplateBody::new("p").with_tools(["a", "bc"]);
        assert_ne!(a.content_hash(), b.content_hash());
    }

    /// 시드 본문이 결정적인지 — 두 Store·두 플랫폼이 같은 값을 내야 한다는
    /// 요구는 결국 이 상수가 고정돼 있느냐다. 이 값이 바뀌면 시드 템플릿의
    /// 정체가 바뀐 것이므로, 인코딩을 손대는 변경은 반드시 여기서 걸린다.
    ///
    /// 이 값은 구현이 낸 것을 그대로 옮긴 것이 **아니다** — 같은 인코딩 규칙을
    /// 파이썬으로 따로 구현해 얻은 값이다(2026-08-29). 구현의 출력을 붙이면
    /// 자기 자신과의 비교라 인코딩이 잘못돼도 함께 잘못된 값이 고정된다.
    #[test]
    fn builtin_default_body_hash_is_pinned() {
        assert_eq!(
            AgentTemplateBody::builtin_default().content_hash(),
            "bf84a87505a008af38b3bccefedbd8f4503525f5652ee60ded223c30429bf1f9"
        );
    }

    /// 정본의 필드별 게이팅 표.
    #[test]
    fn role_prompt_edit_needs_only_update_permission() {
        let a = AgentTemplateBody::new("old").with_tools(["fs.read"]);
        let b = AgentTemplateBody::new("new").with_tools(["fs.read"]);
        assert_eq!(
            a.required_permissions_for_change(&b),
            vec![PermissionKind::AgentTemplateUpdate]
        );
    }

    #[test]
    fn tool_edit_additionally_needs_tool_binding_permission() {
        let a = AgentTemplateBody::new("p").with_tools(["fs.read"]);
        let b = AgentTemplateBody::new("p").with_tools(["fs.read", "shell.run"]);
        assert_eq!(
            a.required_permissions_for_change(&b),
            vec![
                PermissionKind::AgentTemplateUpdate,
                PermissionKind::AgentManage
            ]
        );
        // skill도 같은 등급이다 — skill이 tool을 끌어오므로 우회로가 된다.
        let c = AgentTemplateBody::new("p")
            .with_tools(["fs.read"])
            .with_skills(["deploy"]);
        assert!(a
            .required_permissions_for_change(&c)
            .contains(&PermissionKind::AgentManage));
    }

    #[test]
    fn no_change_needs_no_permission() {
        let a = AgentTemplateBody::new("p").with_tools(["b", "a"]);
        let b = AgentTemplateBody::new("p").with_tools(["a", "b"]);
        assert!(a.required_permissions_for_change(&b).is_empty());
    }

    #[test]
    fn dependent_set_hash_is_order_independent_and_size_sensitive() {
        let a1 = AgentId::new();
        let a2 = AgentId::new();
        assert_eq!(dependent_set_hash(&[a1, a2]), dependent_set_hash(&[a2, a1]));
        assert_ne!(dependent_set_hash(&[a1]), dependent_set_hash(&[a1, a2]));
        assert_ne!(dependent_set_hash(&[]), dependent_set_hash(&[a1]));
    }

    #[test]
    fn new_template_starts_as_draft() {
        let t = AgentTemplate::new(None, "reviewer");
        assert_eq!(t.status, AgentTemplateStatus::Draft);
        assert!(t.project_id.is_none());
        assert_eq!(t.created_at, t.updated_at);
    }
}
