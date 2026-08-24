//! `Store::{create,get,get_by_name,list,update_status}_project` /
//! `project_has_active_tasks` PostgreSQL 통합 테스트 (로드맵 #48, 1단계).
//!
//! 실제 PostgreSQL 데이터베이스가 필요합니다. `DATABASE_URL` 환경변수가
//! 설정되지 않으면 모든 테스트가 자동으로 skip됩니다 (`tests/integration.rs`와
//! 동일한 규약).
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test projects -- --test-threads=1
//! ```

use chrono::Duration;
use fleet_core::{Project, ProjectFilter, ProjectStatus, Task, TaskRequest, TaskStatus};
use fleet_store::{PgStore, Store, StoreError};
use sqlx::postgres::PgPoolOptions;

fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

async fn try_connect() -> Option<PgStore> {
    let url = database_url()?;
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
    Some(store)
}

macro_rules! require_db {
    ($store:ident) => {
        let $store = match try_connect().await {
            Some(s) => s,
            None => return,
        };
        let _ = sqlx::query("TRUNCATE tasks, projects CASCADE")
            .execute($store.pool())
            .await;
    };
}

fn sample_task(prompt: &str) -> Task {
    Task::from_request(TaskRequest {
        prompt: prompt.into(),
        created_by: "test".into(),
        ..Default::default()
    })
}

#[tokio::test]
async fn create_and_get_project_roundtrip() {
    require_db!(store);

    let project = Project::new("acme-web")
        .with_description("main web app")
        .with_created_by("alice");
    store.create_project(&project).await.unwrap();

    let by_id = store
        .get_project(project.id)
        .await
        .unwrap()
        .expect("just-created project must be found by id");
    assert_eq!(by_id, project);

    let by_name = store
        .get_project_by_name("acme-web")
        .await
        .unwrap()
        .expect("just-created project must be found by name");
    assert_eq!(by_name.id, project.id);

    assert!(store
        .get_project_by_name("no-such-project")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn get_project_returns_none_for_unknown_id() {
    require_db!(store);
    let bogus = Project::new("throwaway").id;
    assert!(store.get_project(bogus).await.unwrap().is_none());
}

#[tokio::test]
async fn duplicate_name_conflicts() {
    require_db!(store);

    store
        .create_project(&Project::new("dup-name"))
        .await
        .unwrap();

    let err = store
        .create_project(&Project::new("dup-name"))
        .await
        .expect_err("duplicate project name must conflict");
    assert!(matches!(err, StoreError::Conflict(_)));
}

#[tokio::test]
async fn list_projects_orders_newest_first_and_respects_limit() {
    require_db!(store);

    let mut older = Project::new("older");
    older.created_at -= Duration::seconds(10);
    let newer = Project::new("newer");
    store.create_project(&older).await.unwrap();
    store.create_project(&newer).await.unwrap();

    let all = store
        .list_projects(&ProjectFilter {
            status: None,
            limit: 100,
            offset: 0,
        })
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].name, "newer", "newest must come first");
    assert_eq!(all[1].name, "older");

    let limited = store
        .list_projects(&ProjectFilter {
            status: None,
            limit: 1,
            offset: 0,
        })
        .await
        .unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].name, "newer");
}

#[tokio::test]
async fn list_projects_filters_by_status() {
    require_db!(store);

    let active = Project::new("still-active");
    let mut archived = Project::new("already-archived");
    archived.status = ProjectStatus::Archived;
    store.create_project(&active).await.unwrap();
    store.create_project(&archived).await.unwrap();

    let active_only = store
        .list_projects(&ProjectFilter {
            status: Some(ProjectStatus::Active),
            limit: 100,
            offset: 0,
        })
        .await
        .unwrap();
    assert_eq!(active_only.len(), 1);
    assert_eq!(active_only[0].name, "still-active");

    let archived_only = store
        .list_projects(&ProjectFilter {
            status: Some(ProjectStatus::Archived),
            limit: 100,
            offset: 0,
        })
        .await
        .unwrap();
    assert_eq!(archived_only.len(), 1);
    assert_eq!(archived_only[0].name, "already-archived");
}

#[tokio::test]
async fn update_project_status_transitions_and_bumps_updated_at() {
    require_db!(store);

    let project = Project::new("draining-me");
    store.create_project(&project).await.unwrap();

    let updated = store
        .update_project_status(project.id, ProjectStatus::Draining)
        .await
        .unwrap();
    assert!(updated);

    let reloaded = store.get_project(project.id).await.unwrap().unwrap();
    assert_eq!(reloaded.status, ProjectStatus::Draining);
    assert!(
        reloaded.updated_at >= project.updated_at,
        "updated_at must advance"
    );
}

#[tokio::test]
async fn update_project_status_for_unknown_id_returns_false() {
    require_db!(store);
    let bogus = Project::new("throwaway").id;
    let updated = store
        .update_project_status(bogus, ProjectStatus::Archived)
        .await
        .unwrap();
    assert!(!updated);
}

#[tokio::test]
async fn project_has_active_tasks_reflects_pending_and_dispatched_but_not_terminal() {
    require_db!(store);

    let project = Project::new("busy-project");
    store.create_project(&project).await.unwrap();

    assert!(
        !store.project_has_active_tasks(project.id).await.unwrap(),
        "a project with no tasks at all must not report active tasks"
    );

    let mut cancelled = sample_task("done already");
    cancelled.project_id = Some(project.id);
    cancelled.status = TaskStatus::Cancelled {
        reason: "test".into(),
        cancelled_at: chrono::Utc::now(),
    };
    store.insert_task(&cancelled).await.unwrap();

    assert!(
        !store.project_has_active_tasks(project.id).await.unwrap(),
        "only a terminal (cancelled) task must not count as active"
    );

    let mut pending = sample_task("still queued");
    pending.project_id = Some(project.id);
    store.insert_task(&pending).await.unwrap();

    assert!(
        store.project_has_active_tasks(project.id).await.unwrap(),
        "a pending task referencing the project must count as active"
    );
}
