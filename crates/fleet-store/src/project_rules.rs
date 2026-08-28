//! Project 관련 규칙의 단일 구현 (로드맵 #48).
//!
//! [Project 관리 계약](../../../docs/contracts/project-management.md)의 활성화
//! 게이트는 "Dashboard와 MCP의 동일한 권한·오류 응답"을 요구한다. 두 표면이
//! 같은 규칙을 각자 구현하면 시간이 지나며 갈라지므로, 규칙 자체는 여기 한
//! 번만 두고 각 표면은 결과를 자기 에러 타입(HTTP `ApiError` / JSON-RPC
//! `JsonRpcError`)으로 옮기기만 한다.
//!
//! `fleet-core`가 아니라 `fleet-store`에 있는 이유: 두 규칙 다 [`Store`] 조회가
//! 필요한데 `fleet-core`는 의존성 없는 leaf 크레이트다.

use fleet_core::{Project, ProjectId, ProjectStatus};

use crate::{Store, StoreError};

/// [`ensure_project_accepts_new_tasks`]와 [`ensure_project_accepts_new_agents`]의
/// 거절 사유.
///
/// 호출부는 이걸 자기 표면의 에러로 옮긴다 — `NotFound`/`NotAccepting`은
/// 호출자 입력 문제(4xx / invalid_params), `Store`는 서버 문제(5xx /
/// internal)로 매핑하는 게 두 표면 공통 관례다.
#[derive(Debug, thiserror::Error)]
pub enum ProjectAdmissionError {
    #[error("no such project: {0}")]
    NotFound(ProjectId),

    /// `what`은 거절된 대상("tasks" / "agents")이다. 메시지에만 쓰이며
    /// 호출부의 분기 대상이 아니다 — 두 경우 모두 같은 4xx로 매핑된다.
    #[error("project '{name}' is {status} and does not accept new {what}")]
    NotAccepting {
        name: String,
        status: &'static str,
        what: &'static str,
    },

    #[error("store error: {0}")]
    Store(#[from] StoreError),
}

/// Issue에 링크하려는 Task가 그 Issue와 같은 Project 경계 안에 있는지 검사한다
/// (로드맵 `#58`).
///
/// `issue_task_links`는 링크된 Task를 Issue를 통해 노출하므로, 서로 다른
/// Project의 Task를 그대로 받아들이면 Project 경계 무결성이 깨진다 —
/// principal 단위 Project scope가 아직 없는 지금도 리소스 사이의 경계 자체는
/// 지켜야 한다. principal scope가 나중에 들어오면 이 불변식이 그대로 보안
/// 통제로 격상된다.
///
/// 일반 풀 Task(`task_project_id: None`)는 어느 Project에도 속하지 않으므로
/// 검사 대상이 아니다 — 계속 링크를 허용한다.
///
/// [`Store`] 조회가 필요 없는 순수 판정이지만, 다른 Project 규칙
/// ([`ensure_project_accepts_new_tasks`], [`advance_project_archive`])과 같은
/// 주제이므로 같은 모듈에 둔다. **오늘 호출부는 Dashboard의
/// `link_issue_task_api` 하나뿐이다** — MCP에는 아직 링크 도구가 없다. 그럼에도
/// surface 크레이트가 아니라 여기 두는 이유는, `#92`에서 Dashboard에 넣어 둔
/// Issue 전이 규칙을 MCP 표면이 생긴 뒤에야 `fleet-core`로 옮겨야 했던 비용을
/// 반복하지 않기 위해서다. 두 번째 호출부가 생기기 전까지 이 함수가 단일 호출부
/// 헬퍼라는 점은 숨기지 않는다.
pub fn task_project_matches_issue_project(
    task_project_id: Option<ProjectId>,
    issue_project_id: ProjectId,
) -> bool {
    match task_project_id {
        Some(task_project_id) => task_project_id == issue_project_id,
        None => true,
    }
}

/// Task 제출 시점의 `project_id` 검증.
///
/// Project가 존재하고 새 Task를 받는 상태([`ProjectStatus::accepts_new_tasks`])
/// 여야 통과한다. 통과하면 조회된 [`Project`]를 그대로 돌려준다 — 호출부가
/// 에러 메시지에 이름을 쓰거나 후속 판단에 재사용할 수 있게.
pub async fn ensure_project_accepts_new_tasks(
    store: &dyn Store,
    project_id: ProjectId,
) -> Result<Project, ProjectAdmissionError> {
    ensure_project_active(store, project_id, "tasks").await
}

/// Agent 생성 시점의 `project_id` 검증 (로드맵 #49 1단계).
///
/// Agent의 `project_id`는 생성 이후 바뀌지 않으므로 **이 시점이 소속을
/// 검증할 수 있는 유일한 순간이다** — 나중에 고칠 수 있는 값이 아니다.
/// 이 함수가 `agents` 테이블을 죽은 데이터가 아니게 만드는 첫 번째 판독자다
/// (`crates/fleet-core/src/agent.rs` 모듈 문서 참고).
///
/// 판정은 Task 제출과 같은 술어([`ProjectStatus::accepts_new_tasks`])를 쓴다.
/// 두 질문이 우연히 같은 게 아니라, `Draining`의 정의 자체가 "새 작업을 받지
/// 않는다"이고 Agent 생성은 그 Project에 새 작업 능력을 추가하는 행위이기
/// 때문이다. 술어가 갈라져야 할 이유가 생기면(예: drain 중 교체 Agent 허용)
/// 그때 `ProjectStatus`에 별도 술어를 만든다 — 지금 미리 나누면 두 이름이
/// 항상 같은 값을 반환하는 중복이 된다.
pub async fn ensure_project_accepts_new_agents(
    store: &dyn Store,
    project_id: ProjectId,
) -> Result<Project, ProjectAdmissionError> {
    ensure_project_active(store, project_id, "agents").await
}

/// 위 두 진입점의 공통 구현. `what`은 거절 메시지에만 들어간다.
async fn ensure_project_active(
    store: &dyn Store,
    project_id: ProjectId,
    what: &'static str,
) -> Result<Project, ProjectAdmissionError> {
    let project = store
        .get_project(project_id)
        .await?
        .ok_or(ProjectAdmissionError::NotFound(project_id))?;
    if !project.status.accepts_new_tasks() {
        return Err(ProjectAdmissionError::NotAccepting {
            name: project.name,
            status: project.status.as_str(),
            what,
        });
    }
    Ok(project)
}

/// archive 게이트에서 실제로 막은 조건 — 각 항목은 게이트의 조건 하나에
/// 대응한다.
///
/// 게이트가 `Draining`을 반환한 이유를 **호출자가 추측하지 않게** 하려고
/// 있다. 이 구조체가 없던 동안 Dashboard는 사유를 "비종료 Task"로 하드코딩했고,
/// `#49`가 Agent 조건을 추가하자 Task가 0건인 화면이 "tasks still running"이라고
/// 말했다(2026-08-28 브라우저 검증에서 발견). 조건이 늘어나면 여기 필드를
/// 추가하는 것으로 두 표면이 함께 갱신된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ArchiveBlockers {
    /// 이 Project를 참조하는 비종료 Task가 남아 있다 (`#48` 1단계).
    pub active_tasks: bool,
    /// 회수되지 않은(`Ready`) Agent가 남아 있다 (`#49` 1단계).
    pub live_agents: bool,
}

impl ArchiveBlockers {
    /// 하나라도 막고 있는가.
    pub fn any(&self) -> bool {
        self.active_tasks || self.live_agents
    }

    /// 외부 표면이 그대로 실어 보내는 라벨 목록.
    ///
    /// 어휘를 여기 한 번만 두는 이유는 [`advance_project_archive`]를 공유하는
    /// 이유와 같다 — Dashboard와 MCP가 각자 문자열을 지으면 같은 사유가 두
    /// 이름으로 갈린다.
    pub fn labels(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.active_tasks {
            out.push("tasks");
        }
        if self.live_agents {
            out.push("agents");
        }
        out
    }
}

/// [`advance_project_archive`]의 결과 — 이 호출로 Project가 도달한 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveProgress {
    /// 게이트가 막혀 `Draining`에 머물렀다 — 무엇이 막았는지는 실린
    /// [`ArchiveBlockers`]가 말한다.
    Draining(ArchiveBlockers),
    /// 게이트를 통과해 `Archived`에 도달했다(또는 이미 `Archived`였다).
    Archived,
}

/// Project archive 요청을 한 단계 진행시킨다 (로드맵 #48 1단계 축소판).
///
/// `Active`면 `Draining`으로 전이하고, `Draining`이면 archive 게이트를
/// 평가해 통과 시 `Archived`까지 진행한다. **idempotent** — 이미 `Archived`인
/// Project에 다시 호출해도 안전하며 현재 상태를 그대로 보고한다.
///
/// 게이트는 두 조건이다:
///
/// 1. 이 Project를 참조하는 비종료 Task가 없다 (`#48` 1단계).
/// 2. 이 Project에 회수되지 않은(`Ready`) Agent가 없다 (`#49` 1단계).
///
/// 2번은 정본이 `ArchiveBlocked`에 요구하는 "Agent process·lease·credential
/// grant cleanup 증거" 중 **오늘 실제로 확인 가능한 부분**이다. 프로세스도
/// lease도 credential grant도 없으므로 정리할 대상은 Agent 행 자체뿐이고,
/// 그것이 `Stopped`인지는 지금 답할 수 있다. 나머지 게이트(Attempt terminal
/// 여부, effect hold 해소, 실제 프로세스 cleanup 증거)는 그 대상이 아직 이
/// 저장소에 없다 — [Lifecycle 계약](../../../docs/architecture/project-task-agent-lifecycle.md)
/// 참고.
///
/// 두 조건 다 통과해야 `Archived`에 도달한다. 하나라도 막히면 `Draining`에
/// 머물며, 호출자는 막은 쪽을 해소한 뒤 다시 호출하면 된다(idempotent).
/// 이때 **무엇이 막았는지**를 [`ArchiveBlockers`]로 함께 돌려준다 — 두 조건은
/// 해소 방법이 다르므로(Task는 끝나기를 기다리고, Agent는 사람이 회수한다)
/// 사유 없이 "draining"만 알려 주면 호출자가 알릴 수 있는 것은 추측뿐이다.
///
/// `on_transition`은 상태가 실제로 바뀔 때마다 호출된다(감사 기록용). 두
/// 표면이 서로 다른 감사 파이프라인을 쓰므로(Dashboard는 `crate::audit::record`,
/// MCP는 아직 없음) 콜백으로 분리했다.
pub async fn advance_project_archive(
    store: &dyn Store,
    project: &mut Project,
    mut on_transition: impl FnMut(ProjectStatus),
) -> Result<ArchiveProgress, StoreError> {
    if project.status == ProjectStatus::Active {
        store
            .update_project_status(project.id, ProjectStatus::Draining)
            .await?;
        project.status = ProjectStatus::Draining;
        on_transition(ProjectStatus::Draining);
    }

    if project.status == ProjectStatus::Draining {
        // 두 질의를 `||`로 단락시키지 않고 **둘 다** 평가한다. 단락시키면
        // Task가 막고 있을 때 Agent 조건을 묻지 않은 채로 답하게 되고, 호출자는
        // Task를 전부 끝낸 뒤에야 Agent도 막고 있었다는 사실을 알게 된다.
        // 추가 비용은 Task가 막는 경우의 질의 한 번뿐이다.
        let blockers = ArchiveBlockers {
            active_tasks: store.project_has_active_tasks(project.id).await?,
            live_agents: store.project_has_live_agents(project.id).await?,
        };
        if blockers.any() {
            return Ok(ArchiveProgress::Draining(blockers));
        }
        store
            .update_project_status(project.id, ProjectStatus::Archived)
            .await?;
        project.status = ProjectStatus::Archived;
        on_transition(ProjectStatus::Archived);
    }

    Ok(ArchiveProgress::Archived)
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;
    use crate::mem::MemStore;
    use fleet_core::{Agent, AgentStatus, Task, TaskRequest, TaskStatus};
    use std::sync::Arc;

    fn store() -> Arc<dyn Store> {
        Arc::new(MemStore::new())
    }

    #[test]
    fn general_pool_task_matches_any_issue_project() {
        assert!(task_project_matches_issue_project(None, ProjectId::new()));
    }

    #[test]
    fn task_in_same_project_matches() {
        let project_id = ProjectId::new();
        assert!(task_project_matches_issue_project(
            Some(project_id),
            project_id
        ));
    }

    #[test]
    fn task_in_different_project_does_not_match() {
        assert!(!task_project_matches_issue_project(
            Some(ProjectId::new()),
            ProjectId::new()
        ));
    }

    #[tokio::test]
    async fn active_project_is_admitted() {
        let store = store();
        let project = Project::new("acme");
        store.create_project(&project).await.unwrap();

        let admitted = ensure_project_accepts_new_tasks(store.as_ref(), project.id)
            .await
            .unwrap();
        assert_eq!(admitted.id, project.id);
    }

    #[tokio::test]
    async fn unknown_project_is_not_found() {
        let store = store();
        let err = ensure_project_accepts_new_tasks(store.as_ref(), ProjectId::new())
            .await
            .unwrap_err();
        assert!(matches!(err, ProjectAdmissionError::NotFound(_)));
    }

    #[tokio::test]
    async fn draining_and_archived_projects_are_rejected() {
        let store = store();
        for status in [ProjectStatus::Draining, ProjectStatus::Archived] {
            let mut project = Project::new(format!("p-{}", status.as_str()));
            project.status = status;
            store.create_project(&project).await.unwrap();

            let err = ensure_project_accepts_new_tasks(store.as_ref(), project.id)
                .await
                .unwrap_err();
            match err {
                ProjectAdmissionError::NotAccepting { status: s, .. } => {
                    assert_eq!(s, status.as_str())
                }
                other => panic!("expected NotAccepting, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn archive_advances_straight_to_archived_when_no_active_tasks() {
        let store = store();
        let mut project = Project::new("empty");
        store.create_project(&project).await.unwrap();

        let mut transitions = Vec::new();
        let progress =
            advance_project_archive(store.as_ref(), &mut project, |s| transitions.push(s))
                .await
                .unwrap();

        assert_eq!(progress, ArchiveProgress::Archived);
        assert_eq!(project.status, ProjectStatus::Archived);
        assert_eq!(
            transitions,
            vec![ProjectStatus::Draining, ProjectStatus::Archived],
            "both transitions must be reported for audit"
        );
    }

    #[tokio::test]
    async fn archive_stops_at_draining_while_a_task_is_active() {
        let store = store();
        let mut project = Project::new("busy");
        store.create_project(&project).await.unwrap();

        let mut task = Task::from_request(TaskRequest {
            prompt: "running".into(),
            created_by: "test".into(),
            ..Default::default()
        });
        task.project_id = Some(project.id);
        store.insert_task(&task).await.unwrap();

        let mut transitions = Vec::new();
        let progress =
            advance_project_archive(store.as_ref(), &mut project, |s| transitions.push(s))
                .await
                .unwrap();

        // 사유까지 확인한다 — Task만 막았고 Agent는 하나도 없다.
        assert_eq!(
            progress,
            ArchiveProgress::Draining(ArchiveBlockers {
                active_tasks: true,
                live_agents: false,
            })
        );
        assert_eq!(project.status, ProjectStatus::Draining);
        assert_eq!(transitions, vec![ProjectStatus::Draining]);

        // task가 종결되면 다음 호출이 archive를 마무리한다.
        task.status = TaskStatus::Cancelled {
            reason: "done".into(),
            cancelled_at: chrono::Utc::now(),
        };
        store
            .update_task_status(task.id, &task.status)
            .await
            .unwrap();

        let progress = advance_project_archive(store.as_ref(), &mut project, |_| {})
            .await
            .unwrap();
        assert_eq!(progress, ArchiveProgress::Archived);
    }

    #[tokio::test]
    async fn active_project_admits_new_agents() {
        let store = store();
        let project = Project::new("acme");
        store.create_project(&project).await.unwrap();

        let admitted = ensure_project_accepts_new_agents(store.as_ref(), project.id)
            .await
            .unwrap();
        assert_eq!(admitted.id, project.id);
    }

    #[tokio::test]
    async fn draining_project_rejects_new_agents_and_says_so() {
        let store = store();
        let mut project = Project::new("winding-down");
        project.status = ProjectStatus::Draining;
        store.create_project(&project).await.unwrap();

        let err = ensure_project_accepts_new_agents(store.as_ref(), project.id)
            .await
            .unwrap_err();
        // 메시지가 "tasks"가 아니라 "agents"라고 말해야 한다 — 두 진입점이
        // 같은 술어를 공유하지만 거절 사유는 호출자가 시도한 것을 가리켜야
        // 한다.
        assert!(
            err.to_string().contains("does not accept new agents"),
            "unexpected message: {err}"
        );
    }

    #[tokio::test]
    async fn a_ready_agent_keeps_the_project_draining() {
        let store = store();
        let mut project = Project::new("has-agent");
        store.create_project(&project).await.unwrap();

        let agent = Agent::new(project.id, "reviewer");
        store.create_agent(&agent).await.unwrap();

        // Task는 하나도 없다 — 막는 것은 오직 Agent다. 게이트가 그렇게
        // **말하는지**까지 확인한다: 이 단정이 없으면 사유가 "tasks"로
        // 잘못 붙어도 테스트는 통과한다(2026-08-28에 실제로 그랬다).
        let progress = advance_project_archive(store.as_ref(), &mut project, |_| {})
            .await
            .unwrap();
        assert_eq!(
            progress,
            ArchiveProgress::Draining(ArchiveBlockers {
                active_tasks: false,
                live_agents: true,
            })
        );
        assert_eq!(project.status, ProjectStatus::Draining);

        // Agent를 회수하면 다음 호출이 archive를 마무리한다.
        store
            .update_agent_status(agent.id, AgentStatus::Stopped)
            .await
            .unwrap();
        let progress = advance_project_archive(store.as_ref(), &mut project, |_| {})
            .await
            .unwrap();
        assert_eq!(progress, ArchiveProgress::Archived);
    }

    #[tokio::test]
    async fn both_blockers_are_reported_in_one_pass() {
        let store = store();
        let mut project = Project::new("doubly-blocked");
        store.create_project(&project).await.unwrap();

        let mut task = Task::from_request(TaskRequest {
            prompt: "still running".into(),
            created_by: "test".into(),
            ..Default::default()
        });
        task.project_id = Some(project.id);
        store.insert_task(&task).await.unwrap();
        store
            .create_agent(&Agent::new(project.id, "also-blocking"))
            .await
            .unwrap();

        let progress = advance_project_archive(store.as_ref(), &mut project, |_| {})
            .await
            .unwrap();

        // 두 조건을 `||`로 단락 평가하면 여기서 `live_agents: false`가 나온다 —
        // Task가 먼저 참이라 Agent를 묻지도 않기 때문이다. 그러면 사용자는
        // Task를 전부 끝낸 **다음에야** Agent도 막고 있었다는 것을 알게 되고,
        // 같은 화면을 두 번 왕복해야 한다. 이 테스트가 그 회귀를 막는다.
        assert_eq!(
            progress,
            ArchiveProgress::Draining(ArchiveBlockers {
                active_tasks: true,
                live_agents: true,
            })
        );
        assert_eq!(
            ArchiveBlockers {
                active_tasks: true,
                live_agents: true,
            }
            .labels(),
            vec!["tasks", "agents"]
        );
    }

    #[tokio::test]
    async fn an_agent_in_another_project_does_not_block_archive() {
        let store = store();
        let mut project = Project::new("empty-one");
        store.create_project(&project).await.unwrap();
        let other = Project::new("other-one");
        store.create_project(&other).await.unwrap();
        store
            .create_agent(&Agent::new(other.id, "reviewer"))
            .await
            .unwrap();

        let progress = advance_project_archive(store.as_ref(), &mut project, |_| {})
            .await
            .unwrap();
        assert_eq!(progress, ArchiveProgress::Archived);
    }

    #[tokio::test]
    async fn archiving_an_already_archived_project_is_idempotent() {
        let store = store();
        let mut project = Project::new("twice");
        store.create_project(&project).await.unwrap();

        advance_project_archive(store.as_ref(), &mut project, |_| {})
            .await
            .unwrap();

        let mut transitions = Vec::new();
        let progress =
            advance_project_archive(store.as_ref(), &mut project, |s| transitions.push(s))
                .await
                .unwrap();

        assert_eq!(progress, ArchiveProgress::Archived);
        assert!(
            transitions.is_empty(),
            "a no-op re-archive must not emit spurious audit transitions"
        );
    }
}
