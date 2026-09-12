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
use fleet_store::{ControlFence, PgStore, Store, StoreError};
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

    let mut project = Project::new("acme-web")
        .with_description("main web app")
        .with_created_by("alice");
    // macOS의 `Utc::now()`는 마이크로초 해상도라 나노초 성분이 항상 0이다.
    // 그대로 두면 아래 왕복은 Linux(나노초 해상도)에서만 절삭 경로를 지나므로,
    // 로컬에서 초록인데 CI에서만 깨진다 — 실제로 그렇게 깨졌다. 나노초를
    // 명시적으로 주입해 플랫폼과 무관하게 절삭을 재현한다.
    project.created_at += Duration::nanoseconds(661);
    project.updated_at += Duration::nanoseconds(661);
    store.create_project(&project).await.unwrap();

    let by_id = store
        .get_project(project.id)
        .await
        .unwrap()
        .expect("just-created project must be found by id");
    // Postgres `timestamptz`는 마이크로초까지만 저장하므로, `Utc::now()`가
    // 만든 나노초 성분은 왕복에서 절삭된다. 구조체 전체를 비교하면 값이
    // 옳아도 이 절삭 때문에 항상 실패하므로, 저장소가 실제로 보장하는
    // 정밀도로 필드별 비교한다(`auth_integration`/`issues` 왕복 테스트와
    // 같은 방식). 절삭을 도메인 모델에서 없애지 않는 이유는 그것이
    // 저장소의 성질이지 `Project`의 성질이 아니기 때문이다.
    assert_eq!(by_id.id, project.id);
    assert_eq!(by_id.name, project.name);
    assert_eq!(by_id.description, project.description);
    assert_eq!(by_id.created_by, project.created_by);
    assert_eq!(by_id.status, project.status);
    assert_eq!(
        by_id.created_at.timestamp_micros(),
        project.created_at.timestamp_micros()
    );
    assert_eq!(
        by_id.updated_at.timestamp_micros(),
        project.updated_at.timestamp_micros()
    );

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
        .update_project_status(project.id, ProjectStatus::Draining, None)
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
        .update_project_status(bogus, ProjectStatus::Archived, None)
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

/// lease를 만료시켜 다른 instance가 가져가게 하고 `(살아 있는, 낡은)` fence를
/// 돌려준다. cluster_id는 테스트마다 유일하다.
async fn fences(store: &PgStore, label: &str) -> (ControlFence, ControlFence) {
    let cluster = format!("projects-fence-{label}-{}", uuid::Uuid::new_v4());
    let first = store
        .acquire_control_lease(
            &cluster,
            "instance-a",
            std::time::Duration::from_millis(1),
            None,
        )
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let second = store
        .acquire_control_lease(
            &cluster,
            "instance-b",
            std::time::Duration::from_secs(30),
            None,
        )
        .await
        .unwrap();
    assert!(second.epoch > first.epoch, "가로채면 epoch이 오른다");
    (
        ControlFence {
            cluster_id: cluster.clone(),
            epoch: second.epoch,
        },
        ControlFence {
            cluster_id: cluster,
            epoch: first.epoch,
        },
    )
}

/// 제어권을 잃은 인스턴스는 Project를 archive하지 못한다 (로드맵 `#70`).
///
/// **이 경로에는 2026-09-12까지 fence도 `lease_allows_control()` 검사도 없었다.**
/// `update_project_status`는 조건 없는 `UPDATE`였고, MCP·Dashboard 어느 핸들러도
/// lease를 묻지 않았다 — 즉 fenced 인스턴스가 Project를 archive할 수 있었다.
/// dispatch/cancel은 `#62`·`#63`에서 막혔는데 archive만 남아 있었다.
///
/// 시험을 실제 Postgres로 두는 이유는 fence가 **SQL 술어**이기 때문이다.
/// 스케줄러 단위 시험은 `MemStore`로 돌아 이 `AND EXISTS`를 한 줄도 검증하지
/// 않는다.
#[tokio::test]
async fn a_fenced_instance_cannot_archive_a_project() {
    require_db!(store);

    let (live, stale) = fences(&store, "archive").await;

    let project = Project::new(format!("fenced-{}", uuid::Uuid::new_v4()));
    store.create_project(&project).await.unwrap();

    // 낡은 fence로는 Active → Draining 전이조차 적용되지 않는다.
    assert!(
        !store
            .update_project_status(project.id, ProjectStatus::Draining, Some(&stale))
            .await
            .unwrap(),
        "제어권을 잃은 인스턴스의 쓰기는 0행이어야 한다"
    );
    assert_eq!(
        store.get_project(project.id).await.unwrap().unwrap().status,
        ProjectStatus::Active,
        "거절된 쓰기가 상태를 바꿨다면 fence가 술어로 걸리지 않은 것이다"
    );

    // 살아 있는 fence로는 같은 전이가 적용된다 — 술어가 전이 자체를 막는
    // 것이 아니라 **누가 하느냐**를 가른다는 것을 이 대조가 보인다.
    assert!(
        store
            .update_project_status(project.id, ProjectStatus::Draining, Some(&live))
            .await
            .unwrap(),
        "lease를 쥔 인스턴스는 통과해야 한다"
    );
    assert_eq!(
        store.get_project(project.id).await.unwrap().unwrap().status,
        ProjectStatus::Draining
    );

    // fence가 `None`인 배포(HA lease 미사용)는 그대로 통과한다.
    assert!(
        store
            .update_project_status(project.id, ProjectStatus::Archived, None)
            .await
            .unwrap(),
        "lease를 켜지 않은 단일 인스턴스 배포는 막히면 안 된다"
    );
}

/// `advance_project_archive`가 fenced를 `Draining`과 **다른 값**으로 보고한다.
///
/// 둘을 뭉개면 호출부가 "아직 막는 것이 있다"로 읽고 재시도하는데, fenced
/// 인스턴스의 재시도는 영원히 성공하지 않는다.
#[tokio::test]
async fn archive_reports_fenced_separately_from_blocked() {
    require_db!(store);

    let (_live, stale) = fences(&store, "progress").await;

    let mut project = Project::new(format!("fenced-prog-{}", uuid::Uuid::new_v4()));
    store.create_project(&project).await.unwrap();

    let progress = fleet_store::advance_project_archive(&store, &mut project, Some(&stale), |_| {})
        .await
        .unwrap();

    assert!(
        matches!(progress, fleet_store::ArchiveProgress::Fenced),
        "fenced는 Draining이 아니라 Fenced로 보고되어야 한다 (got {progress:?})"
    );
    assert_eq!(
        project.status,
        ProjectStatus::Active,
        "거절됐으면 메모리 상태도 전이되면 안 된다"
    );
}
