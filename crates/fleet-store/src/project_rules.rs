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

/// [`ensure_project_accepts_new_tasks`]의 거절 사유.
///
/// 호출부는 이걸 자기 표면의 에러로 옮긴다 — `NotFound`/`NotAccepting`은
/// 호출자 입력 문제(4xx / invalid_params), `Store`는 서버 문제(5xx /
/// internal)로 매핑하는 게 두 표면 공통 관례다.
#[derive(Debug, thiserror::Error)]
pub enum ProjectAdmissionError {
    #[error("no such project: {0}")]
    NotFound(ProjectId),

    #[error("project '{name}' is {status} and does not accept new tasks")]
    NotAccepting {
        name: String,
        status: &'static str,
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
    let project = store
        .get_project(project_id)
        .await?
        .ok_or(ProjectAdmissionError::NotFound(project_id))?;
    if !project.status.accepts_new_tasks() {
        return Err(ProjectAdmissionError::NotAccepting {
            name: project.name,
            status: project.status.as_str(),
        });
    }
    Ok(project)
}

/// [`advance_project_archive`]의 결과 — 이 호출로 Project가 도달한 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveProgress {
    /// 비종료 Task가 아직 남아 있어 `Draining`에 머물렀다.
    Draining,
    /// 게이트를 통과해 `Archived`에 도달했다(또는 이미 `Archived`였다).
    Archived,
}

/// Project archive 요청을 한 단계 진행시킨다 (로드맵 #48 1단계 축소판).
///
/// `Active`면 `Draining`으로 전이하고, `Draining`이면 archive 게이트를
/// 평가해 통과 시 `Archived`까지 진행한다. **idempotent** — 이미 `Archived`인
/// Project에 다시 호출해도 안전하며 현재 상태를 그대로 보고한다.
///
/// 1단계 게이트는 "이 Project를 참조하는 비종료 Task가 없다" 하나뿐이다.
/// 목표 계약의 나머지 게이트(Attempt terminal 여부, effect hold 해소, Agent
/// process·lease·credential grant cleanup 증거)는 그 대상 자체가 아직 이
/// 저장소에 없다 — [Lifecycle 계약](../../../docs/architecture/project-task-agent-lifecycle.md)
/// 참고.
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
        if store.project_has_active_tasks(project.id).await? {
            return Ok(ArchiveProgress::Draining);
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
    use fleet_core::{Task, TaskRequest, TaskStatus};
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
        let progress = advance_project_archive(store.as_ref(), &mut project, |s| {
            transitions.push(s)
        })
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
        let progress = advance_project_archive(store.as_ref(), &mut project, |s| {
            transitions.push(s)
        })
        .await
        .unwrap();

        assert_eq!(progress, ArchiveProgress::Draining);
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
    async fn archiving_an_already_archived_project_is_idempotent() {
        let store = store();
        let mut project = Project::new("twice");
        store.create_project(&project).await.unwrap();

        advance_project_archive(store.as_ref(), &mut project, |_| {})
            .await
            .unwrap();

        let mut transitions = Vec::new();
        let progress = advance_project_archive(store.as_ref(), &mut project, |s| {
            transitions.push(s)
        })
        .await
        .unwrap();

        assert_eq!(progress, ArchiveProgress::Archived);
        assert!(
            transitions.is_empty(),
            "a no-op re-archive must not emit spurious audit transitions"
        );
    }
}
