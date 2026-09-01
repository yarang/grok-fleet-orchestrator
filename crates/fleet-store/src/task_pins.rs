//! Task 제출 시점의 "핀" 검증 (로드맵 `#49` 2단계).
//!
//! 핀은 요청자가 배정을 좁히려고 붙이는 값이다 — 오늘은 `server_hint`(Worker
//! 지목)와 `agent_id`(Agent 지목) 둘이다. 둘의 검증을 여기 한 번만 두는 이유는
//! [`project_rules`](crate::project_rules)와 같다: Dashboard `POST /api/tasks`와
//! MCP `fleet_dispatch_task`가 같은 규칙을 각자 구현하면 시간이 지나며 갈라진다.
//!
//! **왜 dispatch가 아니라 제출인가.** 존재하지 않는 Agent를 지목한 요청은 그
//! 자체로 틀렸고, dispatch까지 미루면 Task가 만들어진 뒤에 실패한다 — 요청자는
//! 이미 응답을 받고 떠난 뒤다. 이 저장소가 `project_id`에 대해 이미 같은
//! 판단을 했다(`ensure_project_accepts_new_tasks`). 반대로 **가용성**(Agent가
//! 지금 돌고 있는가, 그 Worker가 살아 있는가)은 제출 시점에 참이어도 dispatch
//! 시점에 거짓일 수 있으므로 여기서 검사하지 않는다. 그쪽은 selector의
//! `Agent*` 계열 `SelectionError`가 판정한다.

use fleet_core::{AgentId, TaskRequest};

use crate::project_rules::{ensure_project_accepts_new_tasks, ProjectAdmissionError};
use crate::{Store, StoreError};

/// 핀 검증 거절 사유.
///
/// 호출부는 이걸 자기 표면의 에러로 옮긴다 — `Store`만 서버 문제(5xx /
/// internal)이고 나머지는 전부 요청자 입력 문제(4xx / invalid_params)다.
#[derive(Debug, thiserror::Error)]
pub enum TaskPinError {
    #[error("no such agent: {0}")]
    AgentNotFound(AgentId),

    /// `agent_id`와 `server_hint`를 함께 준 경우.
    ///
    /// 일치 여부를 검사해서 통과시키는 방안은 택하지 않았다: Agent가 아직
    /// 배정되지 않았으면(`worker_id IS NULL`, 회복 가능한 정상 상태) 일치를
    /// 판정할 수 없어, 같은 요청이 Agent의 배정 상태에 따라 통과했다 거절됐다
    /// 한다. 요청자가 예측할 수 없는 규칙이 되는 것보다 항상 거절하는 편이 낫다.
    #[error(
        "agent_id and server_hint cannot be given together — agent_id already implies a worker"
    )]
    ConflictingPins,

    /// Task와 Agent가 서로 다른 Project에 속한 경우.
    ///
    /// `task_project_matches_issue_project`(`#58`)와 같은 계열의 경계
    /// 불변식이다 — Project 경계를 넘는 참조를 만들지 않는다.
    #[error("task project {task_project} does not match agent project {agent_project}")]
    ProjectMismatch {
        task_project: fleet_core::ProjectId,
        agent_project: fleet_core::ProjectId,
    },

    /// 물려받은 Project가 새 Task를 받지 않는 상태(보관·drain 중).
    ///
    /// 명시된 `project_id`는 호출부가 이미 같은 술어로 검증하지만, **물려받은
    /// 값은 그 검증을 건너뛴다** — 그대로 두면 보관된 Project의 Agent를
    /// 지목하는 것만으로 새 Task를 밀어 넣는 우회로가 된다. 검증을 호출부가
    /// 아니라 이 함수 안에 두는 이유가 그것이다: 두 표면이 각자 기억해야 하는
    /// 규칙을 하나 줄인다.
    #[error(transparent)]
    Project(#[from] ProjectAdmissionError),

    #[error("store error: {0}")]
    Store(#[from] StoreError),
}

/// `agent_id` 핀을 검증하고, 비어 있는 `project_id`를 Agent에서 물려받는다.
///
/// `agent_id`가 없으면 아무 일도 하지 않는다 — 지목 없는 제출이 정상 경로이고,
/// 그 경우 dispatch는 종전대로 Worker만 고른다.
///
/// **`project_id`를 물려받는 이유.** `Agent::project_id`는 `Option`이 아니라
/// 항상 값이 있으므로, Agent를 지목한 순간 그 Task가 속할 Project는 이미
/// 정해진 것이나 다름없다. 거절하는 쪽을 택하면 요청자가 오케스트레이터가
/// 이미 아는 사실을 매번 반복해야 하고, 생략을 오류로 만들면서 정답은 하나뿐인
/// 상황이 된다. 반대로 둘 다 주고 서로 다르면 그건 요청자의 두 진술이 모순인
/// 것이므로 거절한다 — 한쪽을 조용히 이기게 두면 어느 쪽이 이겼는지 요청자가
/// 알 수 없다.
pub async fn apply_agent_pin(store: &dyn Store, req: &mut TaskRequest) -> Result<(), TaskPinError> {
    let Some(agent_id) = req.agent_id else {
        return Ok(());
    };
    if req.server_hint.is_some() {
        return Err(TaskPinError::ConflictingPins);
    }

    let agent = store
        .get_agent(agent_id)
        .await?
        .ok_or(TaskPinError::AgentNotFound(agent_id))?;

    match req.project_id {
        None => {
            ensure_project_accepts_new_tasks(store, agent.project_id).await?;
            req.project_id = Some(agent.project_id);
        }
        Some(task_project) if task_project != agent.project_id => {
            return Err(TaskPinError::ProjectMismatch {
                task_project,
                agent_project: agent.project_id,
            });
        }
        Some(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::MemStore;
    use fleet_core::{Agent, Project, ProjectStatus};

    /// Project 하나와 그 안의 Agent 하나를 만든 store를 돌려준다.
    async fn store_with_agent(status: ProjectStatus) -> (MemStore, Project, Agent) {
        let store = MemStore::new();
        let mut project = Project::new(format!("p-{}", status.as_str()));
        project.status = status;
        store.create_project(&project).await.unwrap();
        let agent = Agent::new(project.id, "planner");
        store.create_agent(&agent).await.unwrap();
        (store, project, agent)
    }

    fn req() -> TaskRequest {
        TaskRequest {
            prompt: "hello".into(),
            created_by: "test".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn no_pin_is_a_no_op() {
        let store = MemStore::new();
        let mut r = req();
        apply_agent_pin(&store, &mut r).await.unwrap();
        assert_eq!(r.project_id, None, "지목이 없으면 아무것도 채우지 않는다");
    }

    #[tokio::test]
    async fn conflicting_pins_are_rejected_before_the_agent_is_looked_up() {
        // Agent가 **존재하지 않는데도** `ConflictingPins`가 나와야 한다.
        // 순서가 반대면 요청자는 핀 충돌 대신 "그런 Agent 없음"을 먼저 받고,
        // Agent를 만들어 고친 뒤에야 진짜 이유를 알게 된다.
        let store = MemStore::new();
        let mut r = req();
        r.agent_id = Some(fleet_core::AgentId::new());
        r.server_hint = Some("w1".into());
        let err = apply_agent_pin(&store, &mut r).await.unwrap_err();
        assert!(matches!(err, TaskPinError::ConflictingPins), "got {err:?}");
    }

    #[tokio::test]
    async fn unknown_agent_is_rejected() {
        let store = MemStore::new();
        let mut r = req();
        r.agent_id = Some(fleet_core::AgentId::new());
        let err = apply_agent_pin(&store, &mut r).await.unwrap_err();
        assert!(matches!(err, TaskPinError::AgentNotFound(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn project_is_inherited_from_the_agent() {
        let (store, project, agent) = store_with_agent(ProjectStatus::Active).await;
        let mut r = req();
        r.agent_id = Some(agent.id);
        apply_agent_pin(&store, &mut r).await.unwrap();
        assert_eq!(
            r.project_id,
            Some(project.id),
            "Agent를 지목한 Task는 일반 풀로 떨어지지 않는다"
        );
    }

    #[tokio::test]
    async fn matching_project_passes_unchanged() {
        let (store, project, agent) = store_with_agent(ProjectStatus::Active).await;
        let mut r = req();
        r.agent_id = Some(agent.id);
        r.project_id = Some(project.id);
        apply_agent_pin(&store, &mut r).await.unwrap();
        assert_eq!(r.project_id, Some(project.id));
    }

    #[tokio::test]
    async fn mismatched_project_is_rejected() {
        let (store, project, agent) = store_with_agent(ProjectStatus::Active).await;
        let other = Project::new("other");
        store.create_project(&other).await.unwrap();
        let mut r = req();
        r.agent_id = Some(agent.id);
        r.project_id = Some(other.id);
        let err = apply_agent_pin(&store, &mut r).await.unwrap_err();
        match err {
            TaskPinError::ProjectMismatch {
                task_project,
                agent_project,
            } => {
                assert_eq!(task_project, other.id);
                assert_eq!(agent_project, project.id);
            }
            other => panic!("expected ProjectMismatch, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inherited_project_must_still_accept_new_tasks() {
        // 이것이 이 검증을 호출부가 아니라 `apply_agent_pin` 안에 둔 이유다.
        // 호출부는 **명시된** `project_id`만 검증하므로, 상속 경로를 여기서
        // 막지 않으면 보관된 Project의 Agent를 지목하는 것만으로 새 Task를
        // 밀어 넣을 수 있다.
        for status in [ProjectStatus::Draining, ProjectStatus::Archived] {
            let (store, _project, agent) = store_with_agent(status).await;
            let mut r = req();
            r.agent_id = Some(agent.id);
            let err = apply_agent_pin(&store, &mut r).await.unwrap_err();
            match err {
                TaskPinError::Project(ProjectAdmissionError::NotAccepting {
                    status: s, ..
                }) => assert_eq!(s, status.as_str()),
                other => panic!("expected NotAccepting for {status:?}, got {other:?}"),
            }
            assert_eq!(r.project_id, None, "거절된 요청에 값을 채워 두지 않는다");
        }
    }
}
