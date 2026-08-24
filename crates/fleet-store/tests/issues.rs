//! Issue 저장소 통합 테스트 (로드맵 #88).
//!
//! [Issue 추적 계약](../../../docs/architecture/issues.md)의 구현 게이트 중
//! 저장소 계층이 책임지는 것들을 검증한다:
//!
//! - 게이트 2 — **교착 없음**: 열린 Issue가 있어도 Task가 터미널까지 도달하고,
//!   비터미널 Task가 있어도 Issue close가 성공한다.
//! - 게이트 10 — **MemStore/PgStore 공유 행동 테스트**: 같은 시나리오를 두
//!   구현에 그대로 돌려 동작이 갈라지지 않음을 확인한다. 이게 이 파일의
//!   구조를 정하는 요구사항이라, 모든 케이스를 `Arc<dyn Store>`를 받는
//!   함수로 쓰고 두 백엔드에서 각각 호출한다.
//!
//! `DATABASE_URL`이 없으면 PgStore 쪽 케이스만 skip되고 MemStore 쪽은 항상
//! 실행된다 — 두 구현이 갈라지는 것을 CI에서도 최소한 절반은 잡는다.
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test issues -- --test-threads=1
//! ```

use std::sync::Arc;

use fleet_core::{
    CloseReason, Issue, IssueComment, IssueFilter, IssueSeverity, IssueStatus, IssueTaskLink,
    Project, Task, TaskRequest, TaskStatus,
};
use fleet_store::mem::MemStore;
use fleet_store::{PgStore, Store};
use sqlx::postgres::PgPoolOptions;

// ── 백엔드 준비 ─────────────────────────────────────────────────────────

async fn mem_backend() -> Arc<dyn Store> {
    Arc::new(MemStore::new())
}

/// `DATABASE_URL`이 없으면 `None` — 호출부가 skip한다.
async fn pg_backend() -> Option<Arc<dyn Store>> {
    let url = std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but connection failed: {e}"));
    let store = PgStore::from_pool(pool);
    store
        .migrate()
        .await
        .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but migration failed: {e}"));
    sqlx::query("TRUNCATE issue_task_links, issue_comments, issues, tasks, projects CASCADE")
        .execute(store.pool())
        .await
        .expect("truncate");
    Some(Arc::new(store))
}

/// 두 백엔드에 같은 시나리오를 돌린다 (게이트 10).
macro_rules! both_backends {
    ($name:ident, $body:expr) => {
        #[tokio::test]
        async fn $name() {
            let scenario: fn(Arc<dyn Store>) -> _ = $body;
            scenario(mem_backend().await).await;
            if let Some(pg) = pg_backend().await {
                scenario(pg).await;
            }
        }
    };
}

// ── 픽스처 ──────────────────────────────────────────────────────────────

async fn seed_project(store: &Arc<dyn Store>, name: &str) -> Project {
    let project = Project::new(name);
    store.create_project(&project).await.unwrap();
    project
}

async fn seed_task(store: &Arc<dyn Store>, prompt: &str, status: TaskStatus) -> Task {
    let mut task = Task::from_request(TaskRequest {
        prompt: prompt.into(),
        created_by: "test".into(),
        ..Default::default()
    });
    task.status = status;
    store.insert_task(&task).await.unwrap();
    task
}

/// 테스트마다 다른 이름을 써서 PgStore를 재사용할 때 유니크 제약과
/// 충돌하지 않게 한다.
fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

// ── 기본 CRUD ───────────────────────────────────────────────────────────

both_backends!(create_and_get_issue_roundtrip, |store| async move {
    let project = seed_project(&store, &unique("proj")).await;
    let issue = Issue::new(project.id, "login is broken", "alice")
        .with_body("steps to reproduce: ...")
        .with_severity(IssueSeverity::High)
        .with_labels(vec!["bug".into(), "auth".into()]);

    store.create_issue(&issue).await.unwrap();

    let fetched = store.get_issue(issue.id).await.unwrap().expect("must exist");
    assert_eq!(fetched.title, "login is broken");
    assert_eq!(fetched.body, "steps to reproduce: ...");
    assert_eq!(fetched.status, IssueStatus::Open);
    assert_eq!(fetched.severity, IssueSeverity::High);
    assert_eq!(fetched.labels, vec!["bug".to_string(), "auth".to_string()]);
    assert_eq!(fetched.close_reason, None);
    assert_eq!(fetched.project_id, project.id);
});

both_backends!(get_unknown_issue_returns_none, |store| async move {
    assert!(store
        .get_issue(fleet_core::IssueId::new())
        .await
        .unwrap()
        .is_none());
});

both_backends!(list_issues_filters_by_project_and_status, |store| async move {
    let a = seed_project(&store, &unique("proj-a")).await;
    let b = seed_project(&store, &unique("proj-b")).await;

    let in_a = Issue::new(a.id, "issue in a", "alice");
    let in_b = Issue::new(b.id, "issue in b", "alice");
    store.create_issue(&in_a).await.unwrap();
    store.create_issue(&in_b).await.unwrap();

    let only_a = store
        .list_issues(&IssueFilter {
            project_id: Some(a.id),
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(only_a.len(), 1);
    assert_eq!(only_a[0].id, in_a.id);

    // 하나를 닫고 open_only 필터를 확인.
    let mut closing = in_a.clone();
    closing
        .transition_to(IssueStatus::Closed, Some(CloseReason::Fixed))
        .unwrap();
    store
        .transition_issue(closing.id, closing.status, closing.close_reason)
        .await
        .unwrap();

    let open_in_a = store
        .list_issues(&IssueFilter {
            project_id: Some(a.id),
            open_only: true,
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(open_in_a.is_empty(), "the only issue in A was closed");

    let closed_in_a = store
        .list_issues(&IssueFilter {
            project_id: Some(a.id),
            status: Some(IssueStatus::Closed),
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(closed_in_a.len(), 1);
});

both_backends!(transition_persists_status_and_close_reason, |store| async move {
    let project = seed_project(&store, &unique("proj")).await;
    let mut issue = Issue::new(project.id, "needs triage", "alice");
    store.create_issue(&issue).await.unwrap();

    issue.transition_to(IssueStatus::Triaged, None).unwrap();
    store
        .transition_issue(issue.id, issue.status, issue.close_reason)
        .await
        .unwrap();
    let fetched = store.get_issue(issue.id).await.unwrap().unwrap();
    assert_eq!(fetched.status, IssueStatus::Triaged);
    assert_eq!(fetched.close_reason, None);

    issue
        .transition_to(IssueStatus::Closed, Some(CloseReason::WontFix))
        .unwrap();
    store
        .transition_issue(issue.id, issue.status, issue.close_reason)
        .await
        .unwrap();
    let fetched = store.get_issue(issue.id).await.unwrap().unwrap();
    assert_eq!(fetched.status, IssueStatus::Closed);
    assert_eq!(fetched.close_reason, Some(CloseReason::WontFix));

    // reopen — close_reason이 지워져야 한다.
    issue.transition_to(IssueStatus::Open, None).unwrap();
    store
        .transition_issue(issue.id, issue.status, issue.close_reason)
        .await
        .unwrap();
    let fetched = store.get_issue(issue.id).await.unwrap().unwrap();
    assert_eq!(fetched.status, IssueStatus::Open);
    assert_eq!(
        fetched.close_reason, None,
        "reopen must clear close_reason in storage too"
    );
});

both_backends!(update_fields_does_not_touch_status, |store| async move {
    // `issue:update`(오탈자 수정)와 `issue:close`(종결)를 분리한 계약이
    // 저장소 API 수준에서도 유지되는지 — update_issue_fields로는 상태를
    // 바꿀 수 없어야 한다.
    let project = seed_project(&store, &unique("proj")).await;
    let mut issue = Issue::new(project.id, "typo in title", "alice");
    store.create_issue(&issue).await.unwrap();
    issue.transition_to(IssueStatus::Triaged, None).unwrap();
    store
        .transition_issue(issue.id, issue.status, issue.close_reason)
        .await
        .unwrap();

    // 로컬 사본의 status를 Closed로 바꿔 놓고 update_issue_fields를 호출해도
    // 저장된 상태는 Triaged 그대로여야 한다.
    let mut tampered = store.get_issue(issue.id).await.unwrap().unwrap();
    tampered.title = "fixed title".into();
    tampered.status = IssueStatus::Closed;
    tampered.close_reason = Some(CloseReason::Fixed);
    store.update_issue_fields(&tampered).await.unwrap();

    let fetched = store.get_issue(issue.id).await.unwrap().unwrap();
    assert_eq!(fetched.title, "fixed title", "field update must apply");
    assert_eq!(
        fetched.status,
        IssueStatus::Triaged,
        "update_issue_fields must not be able to change status"
    );
    assert_eq!(fetched.close_reason, None);
});

both_backends!(update_and_transition_on_unknown_issue_return_false, |store| async move {
    let project = seed_project(&store, &unique("proj")).await;
    let ghost = Issue::new(project.id, "never stored", "alice");
    assert!(!store.update_issue_fields(&ghost).await.unwrap());
    assert!(!store
        .transition_issue(ghost.id, IssueStatus::Triaged, None)
        .await
        .unwrap());
});

// ── 코멘트 ──────────────────────────────────────────────────────────────

both_backends!(comments_are_appended_in_order, |store| async move {
    let project = seed_project(&store, &unique("proj")).await;
    let issue = Issue::new(project.id, "discuss me", "alice");
    store.create_issue(&issue).await.unwrap();

    let first = IssueComment::new(issue.id, "alice", "first thought");
    store.add_issue_comment(&first).await.unwrap();
    // created_at 정렬이 안정적이도록 약간 벌린다.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let second = IssueComment::new(issue.id, "bob", "second thought");
    store.add_issue_comment(&second).await.unwrap();

    let comments = store.list_issue_comments(issue.id).await.unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].body, "first thought");
    assert_eq!(comments[1].body, "second thought");
    assert_eq!(comments[1].author, "bob");
});

// ── Task 연관 ───────────────────────────────────────────────────────────

both_backends!(link_and_unlink_task, |store| async move {
    let project = seed_project(&store, &unique("proj")).await;
    let issue = Issue::new(project.id, "tracked work", "alice");
    store.create_issue(&issue).await.unwrap();
    let task = seed_task(&store, "do the work", TaskStatus::Pending).await;

    let link = IssueTaskLink {
        issue_id: issue.id,
        task_id: Some(task.id),
        task_label: "do the work".into(),
        linked_by: "alice".into(),
        linked_at: chrono::Utc::now(),
    };
    assert!(store.link_issue_task(&link).await.unwrap());

    let links = store.list_issue_task_links(issue.id).await.unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].task_id, Some(task.id));
    assert_eq!(links[0].task_label, "do the work");

    // 같은 Task를 다시 연결해도 중복되지 않는다(멱등).
    assert!(
        !store.link_issue_task(&link).await.unwrap(),
        "re-linking the same task must be a no-op"
    );
    assert_eq!(store.list_issue_task_links(issue.id).await.unwrap().len(), 1);

    assert!(store.unlink_issue_task(issue.id, task.id).await.unwrap());
    assert!(store
        .list_issue_task_links(issue.id)
        .await
        .unwrap()
        .is_empty());
    assert!(
        !store.unlink_issue_task(issue.id, task.id).await.unwrap(),
        "unlinking twice must report false, not error"
    );
});

both_backends!(issue_has_active_tasks_is_derived_not_stored, |store| async move {
    // "진행 중"은 파생 값이다 — Issue의 status는 이 값과 무관하게 그대로
    // 남아야 한다(`InProgress` 상태를 두지 않은 이유).
    let project = seed_project(&store, &unique("proj")).await;
    let issue = Issue::new(project.id, "has work in flight", "alice");
    store.create_issue(&issue).await.unwrap();

    assert!(!store.issue_has_active_tasks(issue.id).await.unwrap());

    let task = seed_task(&store, "in flight", TaskStatus::Pending).await;
    store
        .link_issue_task(&IssueTaskLink {
            issue_id: issue.id,
            task_id: Some(task.id),
            task_label: "in flight".into(),
            linked_by: "alice".into(),
            linked_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    assert!(store.issue_has_active_tasks(issue.id).await.unwrap());
    assert_eq!(
        store.get_issue(issue.id).await.unwrap().unwrap().status,
        IssueStatus::Open,
        "an in-flight task must not change the issue's stored status"
    );

    // Task가 종결되면 파생 값도 따라 내려간다.
    store
        .update_task_status(
            task.id,
            &TaskStatus::Cancelled {
                reason: "done".into(),
                cancelled_at: chrono::Utc::now(),
            },
        )
        .await
        .unwrap();
    assert!(!store.issue_has_active_tasks(issue.id).await.unwrap());
});

// ── 교착 없음 (구현 게이트 2) ───────────────────────────────────────────

both_backends!(open_issue_does_not_block_a_task_from_reaching_terminal, |store| async move {
    // I1 — Task 전이는 issue.status를 읽지 않는다. 열린 Issue(그것도
    // ReadyForAgent 상태)에 연관된 Task가 아무 방해 없이 터미널까지 간다.
    let project = seed_project(&store, &unique("proj")).await;
    let mut issue = Issue::new(project.id, "still very much open", "alice");
    store.create_issue(&issue).await.unwrap();
    issue.transition_to(IssueStatus::Triaged, None).unwrap();
    issue.transition_to(IssueStatus::ReadyForAgent, None).unwrap();
    store
        .transition_issue(issue.id, issue.status, issue.close_reason)
        .await
        .unwrap();

    let task = seed_task(&store, "work", TaskStatus::Pending).await;
    store
        .link_issue_task(&IssueTaskLink {
            issue_id: issue.id,
            task_id: Some(task.id),
            task_label: "work".into(),
            linked_by: "alice".into(),
            linked_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    // Task를 터미널까지 전이 — 어떤 단계에서도 Issue 때문에 막히지 않는다.
    store
        .update_task_status(
            task.id,
            &TaskStatus::Cancelled {
                reason: "terminal despite an open issue".into(),
                cancelled_at: chrono::Utc::now(),
            },
        )
        .await
        .unwrap();

    let fetched = store.get_task(task.id).await.unwrap().unwrap();
    assert!(
        fetched.is_terminal(),
        "an open (even ReadyForAgent) issue must not keep a task from terminating"
    );
    // Issue는 그대로 열려 있다.
    assert_eq!(
        store.get_issue(issue.id).await.unwrap().unwrap().status,
        IssueStatus::ReadyForAgent
    );
});

both_backends!(non_terminal_task_does_not_block_closing_an_issue, |store| async move {
    // I2 — Issue의 close에는 Task 상태에 대한 선행 조건이 없다.
    let project = seed_project(&store, &unique("proj")).await;
    let mut issue = Issue::new(project.id, "closing early", "alice");
    store.create_issue(&issue).await.unwrap();

    let task = seed_task(&store, "still running", TaskStatus::Pending).await;
    store
        .link_issue_task(&IssueTaskLink {
            issue_id: issue.id,
            task_id: Some(task.id),
            task_label: "still running".into(),
            linked_by: "alice".into(),
            linked_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    assert!(
        store.issue_has_active_tasks(issue.id).await.unwrap(),
        "precondition: the task really is non-terminal"
    );

    issue
        .transition_to(IssueStatus::Closed, Some(CloseReason::WontFix))
        .unwrap();
    let updated = store
        .transition_issue(issue.id, issue.status, issue.close_reason)
        .await
        .unwrap();
    assert!(
        updated,
        "closing an issue must succeed even with a non-terminal linked task"
    );
    assert_eq!(
        store.get_issue(issue.id).await.unwrap().unwrap().status,
        IssueStatus::Closed
    );

    // 그리고 Task는 여전히 살아 있다 — close가 Task를 건드리지 않는다.
    assert!(!store.get_task(task.id).await.unwrap().unwrap().is_terminal());
});

// ── DB 레벨 불변식 (PgStore 전용) ───────────────────────────────────────
//
// 아래 세 CHECK 제약은 애플리케이션(`Issue::transition_to`)과 **같은 규칙을
// DB에도** 둔 것이다 — 다른 경로로 들어온 쓰기(수동 SQL, 향후 다른 서비스)도
// 이 불변식을 깨지 못하게 하기 위함. MemStore는 이걸 재현하지 않으므로
// PgStore에서만 검증한다. `DATABASE_URL`이 없으면 skip된다.

/// 위 `pg_backend`와 달리 raw SQL을 쓰기 위해 구체 타입을 돌려준다.
async fn pg_raw() -> Option<PgStore> {
    let url = std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .ok()?;
    let store = PgStore::from_pool(pool);
    store.migrate().await.ok()?;
    sqlx::query("TRUNCATE issue_task_links, issue_comments, issues, tasks, projects CASCADE")
        .execute(store.pool())
        .await
        .ok()?;
    Some(store)
}

/// 제약 위반 INSERT를 시도하고 에러 메시지를 돌려준다.
async fn insert_raw_issue(
    store: &PgStore,
    project_id: uuid::Uuid,
    status: &str,
    close_reason: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO issues (id, project_id, title, status, close_reason, created_by) \
         VALUES ($1, $2, 'raw', $3, $4, 'test')",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(project_id)
    .bind(status)
    .bind(close_reason)
    .execute(store.pool())
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[tokio::test]
async fn db_rejects_closed_issue_without_a_reason() {
    let Some(store) = pg_raw().await else { return };
    let project = Project::new(unique("proj"));
    store.create_project(&project).await.unwrap();

    let err = insert_raw_issue(&store, project.id.0, "closed", None)
        .await
        .expect_err("closed without a reason must violate the CHECK constraint");
    assert!(
        err.contains("issues_close_reason_matches_status"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn db_rejects_close_reason_on_a_non_closed_issue() {
    let Some(store) = pg_raw().await else { return };
    let project = Project::new(unique("proj"));
    store.create_project(&project).await.unwrap();

    let err = insert_raw_issue(&store, project.id.0, "open", Some("fixed"))
        .await
        .expect_err("a close_reason on an open issue must violate the CHECK constraint");
    assert!(
        err.contains("issues_close_reason_matches_status"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn db_rejects_an_in_progress_status() {
    // 구현 게이트 3 — `InProgress` 부재를 스키마 레벨에서도 고정한다.
    // 애플리케이션 enum에 없어도 DB에 직접 쓰는 경로가 있으면 들어올 수
    // 있으므로, CHECK 제약이 두 번째 방어선이 된다.
    let Some(store) = pg_raw().await else { return };
    let project = Project::new(unique("proj"));
    store.create_project(&project).await.unwrap();

    let err = insert_raw_issue(&store, project.id.0, "in_progress", None)
        .await
        .expect_err("in_progress must not be a storable issue status");
    assert!(
        err.contains("issues_status_check"),
        "unexpected error: {err}"
    );
}
