//! `Store::{create,get,get_by_name,list,update_status}_agent` /
//! `project_has_live_agents` PostgreSQL 통합 테스트 (로드맵 #49, 1단계).
//!
//! 실제 PostgreSQL 데이터베이스가 필요합니다. `DATABASE_URL` 환경변수가
//! 설정되지 않으면 모든 테스트가 자동으로 skip됩니다 (`tests/projects.rs`와
//! 동일한 규약).
//!
//! MemStore 쪽 동치는 `src/mem.rs`가 아니라 `src/project_rules.rs`의 규칙
//! 테스트가 덮는다. 여기서 굳이 PgStore를 따로 도는 이유는 두 가지가
//! 메모리 구현에 **존재하지 않기** 때문이다: `agents.project_id`의 FK와
//! `UNIQUE (project_id, name)` 제약. 둘 다 SQL 계층에서만 강제되므로,
//! 위반 시 `StoreError::Conflict`로 번역되는지는 실 DB로만 증명된다.
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test agents
//! ```
//!
//! **`--test-threads=1`을 요구하지 않는다.** 이 저장소의 다른 통합 테스트
//! 14개(`tests/projects.rs` 등)는 "매 테스트 시작 시 공유 테이블 TRUNCATE"
//! 패턴을 쓰고, 그래서 `ci.yml`이 세 잡 모두에 `-- --test-threads=1`을
//! 붙인다. 그 패턴은 cargo가 한 바이너리 안의 테스트를 기본으로 병렬
//! 실행하는 순간 성립하지 않는다 — A가 만든 행을 B의 TRUNCATE가 지운다
//! (2026-08-28 실측: `projects.rs`는 병렬에서 8건 중 3~4건 실패, 직렬에서
//! 8건 통과).
//!
//! 여기서 그 관례를 따르지 않는 이유는 취향이 아니다. `027`의 FK가 있어서
//! 같은 경합이 조용한 오답이 아니라 `Conflict(... violates foreign key
//! constraint ...)`로 터지고(초안이 실제로 10건 중 6건 실패했다), 그러면
//! "이 파일은 특정 스레드 수에서만 통과한다"는 전제가 파일 밖 CI 설정에
//! 숨는다. 대신 각 테스트가 **전역적으로 유일한 이름의 Project**를 만들고
//! 그 Project로만 조회를 좁힌다. 공유 상태를 지우는 대신 애초에 공유하지
//! 않는 쪽이라 스레드 수와 무관하다.

use fleet_core::{Agent, AgentFilter, AgentStatus, Project, ProjectStatus};
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
    };
}

/// 테스트마다 유일한 Project를 만든다.
///
/// `projects.name`은 022 마이그레이션에서 전역 UNIQUE라, `label`을 그대로
/// 쓰면 두 번째 실행부터 이름이 충돌한다. TRUNCATE로 그것을 피하는 대신
/// UUID 접미사를 붙여 실행 간에도 테스트 간에도 겹치지 않게 한다.
async fn seed_project(store: &PgStore, label: &str) -> Project {
    let project = Project::new(format!("agents-{label}-{}", uuid::Uuid::new_v4()));
    store.create_project(&project).await.unwrap();
    project
}

#[tokio::test]
async fn create_and_get_agent_roundtrip() {
    require_db!(store);
    let project = seed_project(&store, "roundtrip").await;

    let agent = Agent::new(project.id, "builder")
        .with_description("builds things")
        .with_created_by("alice");
    store.create_agent(&agent).await.unwrap();

    let fetched = store.get_agent(agent.id).await.unwrap().expect("agent row");
    assert_eq!(fetched.id, agent.id);
    assert_eq!(fetched.project_id, project.id);
    assert_eq!(fetched.name, "builder");
    assert_eq!(fetched.description.as_deref(), Some("builds things"));
    assert_eq!(fetched.created_by.as_deref(), Some("alice"));
    // 새 Agent는 항상 Ready다 — 1단계에는 생성 시 다른 상태로 들어올
    // 경로가 없다.
    assert_eq!(fetched.status, AgentStatus::Ready);
}

#[tokio::test]
async fn get_agent_returns_none_for_unknown_id() {
    require_db!(store);
    let missing = store.get_agent(fleet_core::AgentId::new()).await.unwrap();
    assert!(missing.is_none());
}

#[tokio::test]
async fn agent_name_is_unique_per_project_not_globally() {
    require_db!(store);
    let a = seed_project(&store, "proj-a").await;
    let b = seed_project(&store, "proj-b").await;

    store
        .create_agent(&Agent::new(a.id, "worker"))
        .await
        .unwrap();

    // 같은 Project 안에서는 충돌.
    let dup = store.create_agent(&Agent::new(a.id, "worker")).await;
    assert!(
        matches!(dup, Err(StoreError::Conflict(_))),
        "duplicate name in the same project must conflict, got {dup:?}"
    );

    // 다른 Project에서는 같은 이름이 허용된다 — 이름은 소속 Project
    // 안에서만 의미를 가지며, Agent는 옮길 수 없어 그 범위가 영구히
    // 안정적이다.
    store
        .create_agent(&Agent::new(b.id, "worker"))
        .await
        .expect("same name in a different project must be allowed");
}

#[tokio::test]
async fn get_agent_by_name_is_scoped_to_the_project() {
    require_db!(store);
    let a = seed_project(&store, "scoped-a").await;
    let b = seed_project(&store, "scoped-b").await;
    let agent = Agent::new(a.id, "shared-name");
    store.create_agent(&agent).await.unwrap();
    store
        .create_agent(&Agent::new(b.id, "shared-name"))
        .await
        .unwrap();

    let found = store
        .get_agent_by_name(a.id, "shared-name")
        .await
        .unwrap()
        .expect("agent in project a");
    assert_eq!(
        found.id, agent.id,
        "lookup must not cross the project boundary"
    );
}

#[tokio::test]
async fn create_agent_rejects_unknown_project() {
    require_db!(store);
    // FK 위반. 호출부는 `ensure_project_accepts_new_agents`로 먼저
    // 거르지만, 그 검사와 INSERT 사이의 경합을 DB가 최종 방어한다.
    let orphan = Agent::new(fleet_core::ProjectId::new(), "orphan");
    let result = store.create_agent(&orphan).await;
    assert!(
        matches!(result, Err(StoreError::Conflict(_))),
        "unknown project_id must be rejected, got {result:?}"
    );
}

#[tokio::test]
async fn list_agents_filters_by_project_and_status() {
    require_db!(store);
    let a = seed_project(&store, "list-a").await;
    let b = seed_project(&store, "list-b").await;

    let ready = Agent::new(a.id, "ready-one");
    let stopped = Agent::new(a.id, "stopped-one");
    store.create_agent(&ready).await.unwrap();
    store.create_agent(&stopped).await.unwrap();
    store
        .create_agent(&Agent::new(b.id, "other"))
        .await
        .unwrap();
    store
        .update_agent_status(stopped.id, AgentStatus::Stopped)
        .await
        .unwrap();

    let in_a = store
        .list_agents(&AgentFilter {
            project_id: Some(a.id),
            status: None,
            limit: 100,
            offset: 0,
        })
        .await
        .unwrap();
    assert_eq!(
        in_a.len(),
        2,
        "project filter must exclude the other project"
    );

    let ready_in_a = store
        .list_agents(&AgentFilter {
            project_id: Some(a.id),
            status: Some(AgentStatus::Ready),
            limit: 100,
            offset: 0,
        })
        .await
        .unwrap();
    assert_eq!(ready_in_a.len(), 1);
    assert_eq!(ready_in_a[0].id, ready.id);

    // `project_id: None`이 "필터 없음"으로 동작하는지만 본다. 정확한
    // 개수를 단언하지 않는 이유는 이 테이블이 더 이상 테스트마다
    // 비워지지 않기 때문이다 — 같은 실행의 다른 테스트도, 이전 실행이
    // 남긴 행도 여기 섞인다. 단언할 수 있는 것은 "우리 행이 빠지지
    // 않는다"이지 "우리 행만 있다"가 아니다.
    let all = store
        .list_agents(&AgentFilter {
            project_id: None,
            status: None,
            limit: 1000,
            offset: 0,
        })
        .await
        .unwrap();
    let ids: std::collections::HashSet<_> = all.iter().map(|a| a.id).collect();
    for expected in [ready.id, stopped.id] {
        assert!(
            ids.contains(&expected),
            "필터 없는 조회는 project 경계를 넘어 모든 Agent를 포함해야 한다"
        );
    }
}

#[tokio::test]
async fn update_agent_status_persists_and_bumps_updated_at() {
    require_db!(store);
    let project = seed_project(&store, "status-bump").await;
    let agent = Agent::new(project.id, "bumped");
    store.create_agent(&agent).await.unwrap();
    let before = store.get_agent(agent.id).await.unwrap().unwrap();

    store
        .update_agent_status(agent.id, AgentStatus::Stopped)
        .await
        .unwrap();

    let after = store.get_agent(agent.id).await.unwrap().unwrap();
    assert_eq!(after.status, AgentStatus::Stopped);
    assert!(
        after.updated_at >= before.updated_at,
        "회수 시각이 기록돼야 감사에서 '언제 멈췄는가'를 답할 수 있다"
    );
}

#[tokio::test]
async fn project_has_live_agents_ignores_stopped_and_other_projects() {
    require_db!(store);
    let a = seed_project(&store, "live-a").await;
    let b = seed_project(&store, "live-b").await;

    assert!(
        !store.project_has_live_agents(a.id).await.unwrap(),
        "Agent가 없는 Project는 archive를 막을 것이 없다"
    );

    let agent = Agent::new(a.id, "holder");
    store.create_agent(&agent).await.unwrap();
    assert!(store.project_has_live_agents(a.id).await.unwrap());
    assert!(
        !store.project_has_live_agents(b.id).await.unwrap(),
        "다른 Project의 Agent가 이 Project를 막아서는 안 된다"
    );

    store
        .update_agent_status(agent.id, AgentStatus::Stopped)
        .await
        .unwrap();
    assert!(
        !store.project_has_live_agents(a.id).await.unwrap(),
        "Stopped Agent는 더 이상 archive를 막지 않는다"
    );
}

#[tokio::test]
async fn a_live_agent_holds_the_project_in_draining() {
    require_db!(store);
    // Task는 하나도 만들지 않는다 — Project를 `Draining`에 붙잡아 두는
    // 것이 오직 Agent 행뿐임을 이 테스트가 증명한다. 즉 `agents` 테이블은
    // 1단계에서도 아무도 읽지 않는 죽은 데이터가 아니다.
    let mut project = seed_project(&store, "archive-gate").await;
    let agent = Agent::new(project.id, "blocker");
    store.create_agent(&agent).await.unwrap();

    let progress = fleet_store::advance_project_archive(&store, &mut project, |_| {})
        .await
        .unwrap();
    assert_eq!(
        project.status,
        ProjectStatus::Draining,
        "Ready Agent가 남아 있으면 archive가 완료되면 안 된다"
    );
    // 상태뿐 아니라 **사유**도 실 DB 질의를 거쳐 나온다 — `project_has_*`
    // 두 술어가 각각 Postgres에 묻고, 그 답이 그대로 라벨이 된다.
    assert_eq!(
        progress,
        fleet_store::ArchiveProgress::Draining(fleet_store::ArchiveBlockers {
            active_tasks: false,
            live_agents: true,
        }),
        "Task가 0건이므로 게이트는 Agent만 가리켜야 한다"
    );

    store
        .update_agent_status(agent.id, AgentStatus::Stopped)
        .await
        .unwrap();
    fleet_store::advance_project_archive(&store, &mut project, |_| {})
        .await
        .unwrap();
    assert_eq!(project.status, ProjectStatus::Archived);
}

#[tokio::test]
async fn archived_project_survives_agent_rows() {
    require_db!(store);
    // `ON DELETE RESTRICT`가 지키는 것: Agent 행은 archive 이후에도 남는다.
    // Project 물리 삭제 경로가 없으므로 이 테스트는 archive가 행을 지우지
    // 않는다는 사실만 고정한다 — 감사 기록의 대상이 사라지면 안 된다.
    let mut project = seed_project(&store, "survivor").await;
    let agent = Agent::new(project.id, "kept");
    store.create_agent(&agent).await.unwrap();
    store
        .update_agent_status(agent.id, AgentStatus::Stopped)
        .await
        .unwrap();

    fleet_store::advance_project_archive(&store, &mut project, |_| {})
        .await
        .unwrap();
    assert_eq!(project.status, ProjectStatus::Archived);

    let still_there = store.get_agent(agent.id).await.unwrap();
    assert!(still_there.is_some(), "archive는 Agent 행을 지우지 않는다");
}
