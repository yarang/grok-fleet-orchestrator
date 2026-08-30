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
//!   로드맵 `#67` 4단계. 따라서 `Starting`/`Running`/`Failed`로 전이시킬
//!   주체가 없다. (이 자리는 `#89`로 적혀 있었으나 오표기였다. `#89`는 그
//!   스트림 위에서 Agent가 Issue를 여는 소비자이고 선행이 `#67`이다.)
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

use crate::agent_template::AgentTemplatePin;
use crate::ids::{AgentId, ProjectId, WorkerId};

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
    /// 있을 때만 `Stopped`"는 프로세스를 띄우는 `#67` 4단계에서 성립한다.
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
/// 오케스트레이터가 이 Agent에 **바라는** 상태 (로드맵 `#67` 4b).
///
/// [`AgentStatus`]와 다른 축이다. `AgentStatus`는 **관측**이고 이것은
/// **의도**다. 그래서 값 집합도 같지 않다 — 관측에는 `Ready`처럼 "아직
/// 아무 명령도 받지 않았다"가 있지만, 의도에는 그런 값이 없다(바라는 바가
/// 없다는 의도는 곧 "돌지 않기를 바란다"이다).
///
/// 기본값이 [`Self::Stopped`]인 이유는 `AgentStatus::Ready`의 정의
/// ("정의는 끝났고 시작 명령을 **받을 수 있다**")와 맞추기 위해서다.
/// 생성만으로 돌기 시작하면 `Ready`가 뜻을 잃는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDesiredStatus {
    /// 이 Agent의 프로세스가 살아 있기를 바란다.
    Running,
    /// 이 Agent의 프로세스가 없기를 바란다.
    Stopped,
}

impl AgentDesiredStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "running" => Some(Self::Running),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
}

/// heartbeat 응답에 실려 Worker로 가는 Agent 하나에 대한 명령
/// (로드맵 `#67` 4b).
///
/// **여기에 포트·secret·cwd는 없다.** 그 셋이 들어오는 순간 heartbeat
/// 응답은 통째로 로깅해도 안전한 값이 아니게 되며, 그것들을 만들 프로세스
/// 매니저는 4c다. 지금은 세 필드가 전부다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentCommand {
    pub agent_id: AgentId,
    pub desired_status: AgentDesiredStatus,
    /// 이 명령의 세대. Worker는 같은 값을 [`AgentAck`]로 돌려준다.
    pub generation: i64,
}

/// Worker가 heartbeat 요청에 실어 보내는 명령 수신 확인 (로드맵 `#67` 4b).
///
/// **관측 상태를 싣지 않는다.** 4b에는 볼 프로세스가 없으므로 "받았고
/// 받아들였다"가 Worker가 정직하게 말할 수 있는 최대치이며, 관측
/// 결과(process/container ID·cleanup 증거)는 4c가 얹는다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentAck {
    pub agent_id: AgentId,
    pub generation: i64,
}

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
    /// 어느 AgentTemplate revision으로 만들어졌는지 (로드맵 #86).
    ///
    /// `None`은 템플릿 없이 만든 Agent다 — `027`이 이 컬럼을 만들기 전에
    /// 생성된 행이 그렇고, 앞으로도 템플릿을 지정하지 않은 생성은 허용된다.
    ///
    /// **`project_id`와 마찬가지로 갱신 경로를 두지 않는다.** 나중에 pin을
    /// 바꿀 수 있으면 "이 Agent는 어떤 본문으로 만들어졌나"에 대한 답이
    /// 시간에 따라 달라져 감사 기록이 무의미해진다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_pin: Option<AgentTemplatePin>,
    /// 이 Agent를 실행할 Worker (로드맵 `#67` 4a).
    ///
    /// **`project_id`/`template_pin`과 달리 불변이 아니다.** 저 둘은 정체성이라
    /// 바뀌면 감사 기록이 무의미해지지만, 배정은 운영 상태다 — Worker가
    /// 등록 해제되면 배정은 더는 참이 아니고(`030`의 `ON DELETE SET NULL`),
    /// 운영자가 다른 Worker로 옮길 수 있어야 한다. 그래서 이 필드에는
    /// 갱신 경로([`crate::audit::action::AGENT_ASSIGN`])를 만든다.
    ///
    /// `None`은 배정되지 않았다는 뜻이며 오늘 정상적으로 도달한다 — 생성
    /// 시점에 배정 가능한 Worker가 하나도 없으면 배정 없이 생성된다. 생성을
    /// 실패시키지 않는 이유는 Agent 정의가 Worker 가용성에 인질로 잡히면
    /// 안 되기 때문이다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<WorkerId>,
    /// 언제 현재 Worker에 배정됐는지. `worker_id`와 항상 함께 있거나 함께
    /// 없다 — `030`의 `agents_placement_complete` CHECK가 강제한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_at: Option<DateTime<Utc>>,
    /// 오케스트레이터가 이 Agent에 바라는 상태 (로드맵 `#67` 4b).
    ///
    /// `status`(관측)와 함께 읽어야 뜻이 산다: `status == Ready &&
    /// desired_status == Running`이 정본의 `Starting`이며, 그것을 컬럼으로
    /// 만들지 않은 이유는 [`Agent::is_starting`]에 적었다.
    #[serde(default = "default_desired_status")]
    pub desired_status: AgentDesiredStatus,
    /// `desired_status` 또는 배정이 바뀔 때마다 증가하는 세대 번호.
    ///
    /// **배정 변경에서도 올라간다.** 새 Worker는 이전 Worker가 받은 명령을
    /// 본 적이 없으므로, 올리지 않으면 `last_acked_generation ==
    /// command_generation`이 "새 Worker가 확인했다"는 거짓을 말한다.
    #[serde(default)]
    pub command_generation: i64,
    /// Worker가 마지막으로 수신 확인한 세대.
    ///
    /// `command_generation`과 같다는 것은 **전달·수락**이지 **수렴**이
    /// 아니다 — 4b에는 프로세스가 없고, Worker는 "이 세대의 명령을 받았다"만
    /// 말한다. 4c가 이 등식을 "돌고 있다"로 읽으면 어떤 테스트도 잡지 못하는
    /// 조용한 오탐이 된다.
    #[serde(default)]
    pub last_acked_generation: i64,
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
            template_pin: None,
            worker_id: None,
            assigned_at: None,
            desired_status: AgentDesiredStatus::Stopped,
            command_generation: 0,
            last_acked_generation: 0,
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

    /// 템플릿 revision에 pin한다. 유효성(템플릿이 새 pin을 받는 상태인지,
    /// revision이 revoke되지 않았는지)은 Store가 `create_agent` 안에서
    /// 검사한다 — 표면이 둘(MCP·Dashboard)이므로 각자 검사하게 두면 한쪽만
    /// 고쳐지는 순간 불변식이 깨진다.
    pub fn with_template_pin(mut self, pin: AgentTemplatePin) -> Self {
        self.template_pin = Some(pin);
        self
    }

    /// 생성 시점에 Worker를 배정한다 (로드맵 `#67` 4a).
    ///
    /// 배정을 INSERT **전에** 정해서 한 번의 쓰기로 끝내는 이유는, 생성 후
    /// UPDATE로 나누면 INSERT는 성공하고 UPDATE가 실패했을 때 배정되지 않은
    /// 행이 남는데 그것을 되돌릴 주체가 없기 때문이다. 배정 선택 자체는
    /// `fleet_scheduler::placement::choose_worker`가 한다 — 코어에 두면
    /// 도메인 모델이 Worker 목록 조회에 의존하게 된다.
    pub fn with_placement(mut self, worker_id: WorkerId, assigned_at: DateTime<Utc>) -> Self {
        self.worker_id = Some(worker_id);
        self.assigned_at = Some(assigned_at);
        self
    }

    /// 마지막 명령이 배정된 Worker에 전달·수락됐는지 (로드맵 `#67` 4b).
    ///
    /// 다시 강조하면 **수렴이 아니다**. 프로세스가 떴다는 뜻이 아니라 명령이
    /// 도달했다는 뜻이며, 운영자에게는 "이 Worker가 아직 명령을 집어가지
    /// 않았다"와 "집어갔는데 반영이 안 됐다"를 가르는 값이다.
    pub fn command_delivered(&self) -> bool {
        self.last_acked_generation == self.command_generation
    }

    /// 정본의 `Starting` — 시작을 바라지만 아직 돌고 있다고 관측되지 않은 상태.
    ///
    /// **컬럼으로 만들지 않은 이유**: 이 값은 `(status, desired_status)`의 순수
    /// 함수다. 컬럼으로 두면 (a) `027`의 `status IN ('ready','stopped')` CHECK를
    /// 아무도 관측하지 않는 값 때문에 넓혀야 하고, (b) generation 컬럼들과
    /// 어긋날 수 있는 두 번째 진실 원천이 생긴다. `#67` 3단계가 워커 자기보고
    /// 대신 오케스트레이터 원장을 택한 것과 같은 판단이다.
    ///
    /// 4b에는 여기서 나가는 문이 없다 — `Running`은 프로세스를 볼 수 있는
    /// 4c만 관측할 수 있기 때문이다. 이것은 "나갈 수 없는 위상"이 아니다:
    /// `status`는 여전히 `Ready`라는 정상 값이고, 여기서 나가는 길(회수)도
    /// 있다. 미충족된 의도는 수렴 프로토콜의 정상 상태다.
    pub fn is_starting(&self) -> bool {
        self.status == AgentStatus::Ready && self.desired_status == AgentDesiredStatus::Running
    }
}

/// `serde(default)`용 — `AgentDesiredStatus`는 `Default`를 구현하지 않는다.
/// 기본이 `Stopped`인 것은 이 타입의 성질이 아니라 **Agent 생성 시의 정책**
/// 이므로(위 `AgentDesiredStatus` 문서 참고), 타입에 붙이면 다른 문맥에서도
/// 조용히 기본값이 생긴다.
fn default_desired_status() -> AgentDesiredStatus {
    AgentDesiredStatus::Stopped
}

/// Agent 목록 조회 필터.
///
/// `project_id`가 `None`이면 모든 Project의 Agent를 본다. principal 단위
/// Project scope가 아직 없으므로 오늘은 이것이 관리자 조회이며, scope가
/// 들어오면 이 필터가 그 강제 지점이 된다.
#[derive(Debug, Clone)]
pub struct AgentFilter {
    pub project_id: Option<ProjectId>,
    pub status: Option<AgentStatus>,
    /// 이 Worker에 배정된 Agent만 (로드맵 `#67` 4a). 4b의 heartbeat 응답이
    /// "이 Worker에 배정된 Agent들의 desired state"를 실으려면 이 조회가
    /// 필요하다. 오늘의 판독자는 운영자 조회(Dashboard `GET /api/agents`,
    /// MCP `fleet_list_agents`)다 — 배정이 실제로 어디로 갔는지 확인할 수
    /// 없으면 컬럼이 죽은 데이터가 된다.
    ///
    /// 배정되지 **않은** Agent만 보는 조회는 만들지 않았다. 그 값을 쓰는
    /// 곳이 없다 — 재배정은 운영자가 특정 Agent를 지목해 호출한다.
    pub worker_id: Option<WorkerId>,
    pub limit: usize,
    pub offset: usize,
}

/// `limit`이 0인 기본값을 만들지 않는다. 두 Store 모두 0을 **조용히 1로**
/// 올리므로(`MemStore`는 `filter.limit.max(1)`, `PgStore`는
/// `filter.limit.clamp(1, 1000)`), 파생 `Default`를 쓰면
/// `..Default::default()`가 "필터 없음"이 아니라 "첫 한 행만"을 뜻하게 된다.
/// 오류도 빈 목록도 아니어서 호출자가 알아차릴 방법이 없다 — `#67` 4a에서
/// 실제로 세 개의 테스트가 이 방식으로 잘못된 개수를 세다 깨졌다.
/// `TaskFilter`·`WorkerFilter`·`AuditFilter`가 같은 이유로 손으로 쓴
/// `Default`를 갖고 있으며, 그쪽과 같은 100을 쓴다.
impl Default for AgentFilter {
    fn default() -> Self {
        Self {
            project_id: None,
            status: None,
            worker_id: None,
            limit: 100,
            offset: 0,
        }
    }
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
