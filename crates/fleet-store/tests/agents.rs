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

use fleet_core::{
    Agent, AgentDesiredStatus, AgentFilter, AgentObservation, AgentObservationReason,
    AgentObservedStatus, AgentStatus, Project, ProjectStatus, Worker,
};
use fleet_store::{PgStore, SlotClaim, Store, StoreError};
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

/// 참가자 수만큼 **미리 열어 둔** 풀 위의 `PgStore` — 경합 테스트 전용
/// (로드맵 `#67` 구현 게이트 ①-A-2).
///
/// 공유 `try_connect`의 4개짜리 풀로 8-way 경합을 돌리면 커넥션 **체크아웃**
/// 에서 먼저 직렬화되어, `FOR UPDATE`가 없어도 테스트가 통과한다. 그런데
/// `max_connections`만 키우는 것으로는 부족하다. sqlx의 `connect()`는 커넥션
/// 하나만 세우고 `min_connections`의 기본값은 0이라, 배리어가 풀리는 순간
/// 나머지 N-1개를 **그 자리에서** 수립해야 한다. 그 수립 비용이 태스크들을
/// 어긋나게 해서 실효 동시성이 떨어진다 — 체크아웃 직렬화와 같은 결함이 한
/// 겹 아래에서 반복되는 것이다. 실측이 그것을 보여 준다: 잠금을 뺀 상태에서
/// 미리 채우지 않은 풀은 8회 시도 중 **매번 정확히 2건**만 겹쳤고, 미리 채운
/// 뒤에는 8건이 겹쳤다. 전자에서는 더 느리거나 더 빠른 기계가 1건을 내놓아
/// 잠금 없는 코드가 초록으로 통과할 수 있다.
///
/// `min_connections`를 올리는 것만으로도 부족하다 — 그것을 채우는 것은
/// 백그라운드 태스크라 `connect()`가 돌아온 시점에 다 찼다는 보장이 없고,
/// 여기서 필요한 것은 보장이다. 그래서 N개를 실제로 잡았다 놓는다.
async fn race_pool(n: usize) -> std::sync::Arc<PgStore> {
    let url = database_url().expect("race_pool은 DATABASE_URL이 있을 때만 부른다");
    let pool = PgPoolOptions::new()
        .max_connections(n as u32)
        .min_connections(n as u32)
        .connect(&url)
        .await
        .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but connection failed: {e}"));
    let mut warm = Vec::with_capacity(n);
    for _ in 0..n {
        warm.push(pool.acquire().await.expect("풀을 미리 채우지 못했다"));
    }
    drop(warm);
    let store = PgStore::from_pool(pool);
    store.migrate().await.unwrap();
    std::sync::Arc::new(store)
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
            worker_id: None,
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
            worker_id: None,
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
            worker_id: None,
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

// ── 로드맵 #67 4a: Agent → Worker 배정 ────────────────────────────────────
//
// 이 절이 실 DB를 요구하는 이유는 파일 상단이 적은 것과 같다 — 검증 대상이
// **SQL 계층에만 존재**한다. `agents_placement_complete` CHECK, `worker_id`의
// `ON DELETE SET NULL`, 그리고 그 SET NULL이 CHECK를 깨지 않도록 짝을 맞추는
// `BEFORE UPDATE OF worker_id` 트리거 셋 다 MemStore에는 없다(`src/mem.rs`
// 말미의 주석이 그 부재를 명시한다).

/// 테스트마다 유일한 Worker를 만든다. `seed_project`와 같은 이유다.
async fn seed_worker(store: &PgStore, label: &str) -> Worker {
    let name = format!("agents-w-{label}-{}", uuid::Uuid::new_v4());
    let worker = Worker::new(name.clone(), format!("http://{name}.invalid"));
    store.upsert_worker(&worker).await.unwrap();
    worker
}

#[tokio::test]
async fn placement_roundtrips_through_the_row() {
    require_db!(store);
    let project = seed_project(&store, "placed").await;
    let worker = seed_worker(&store, "placed").await;

    // 나노초 잔여를 **일부러** 심는다. `Utc::now()`는 Linux에서 나노초를
    // 주지만 macOS의 `CLOCK_REALTIME`은 잔여가 항상 0이라, 이 줄이 없으면
    // "Linux CI에서만 깨지는 테스트"가 된다(agent.md §3.4).
    let assigned_at = chrono::Utc::now() + chrono::Duration::nanoseconds(416);
    let agent = Agent::new(project.id, "placed").with_placement(worker.id, assigned_at);
    store.create_agent(&agent).await.unwrap();

    let fetched = store.get_agent(agent.id).await.unwrap().expect("agent row");
    assert_eq!(fetched.worker_id, Some(worker.id));
    // `timestamptz`는 마이크로초 해상도다. 메모리 값과 왕복 값을 직접 맞대지
    // 않고 양변을 내려서 비교한다.
    assert_eq!(
        fetched.assigned_at.unwrap().timestamp_micros(),
        assigned_at.timestamp_micros(),
    );
}

#[tokio::test]
async fn an_unplaced_agent_has_neither_half() {
    require_db!(store);
    let project = seed_project(&store, "unplaced").await;
    let agent = Agent::new(project.id, "floating");
    store.create_agent(&agent).await.unwrap();

    let fetched = store.get_agent(agent.id).await.unwrap().expect("agent row");
    assert!(fetched.worker_id.is_none());
    assert!(fetched.assigned_at.is_none());
}

#[tokio::test]
async fn assign_stamps_assigned_at_from_the_database() {
    require_db!(store);
    let project = seed_project(&store, "assign").await;
    let worker = seed_worker(&store, "assign").await;
    let agent = Agent::new(project.id, "late");
    store.create_agent(&agent).await.unwrap();

    let before = chrono::Utc::now();
    assert_eq!(
        store
            .assign_agent_worker(agent.id, worker.id)
            .await
            .unwrap(),
        SlotClaim::Claimed
    );

    // 호출자가 손에 든 `agent`는 여전히 미배정이다. 저장된 행을 **다시
    // 읽어야** `assigned_at`을 알 수 있다 — `NOW()`를 찍는 것이 DB이기
    // 때문이며, #49 1단계의 멱등 stop이 정확히 이 지점에서 깨졌었다.
    assert!(agent.assigned_at.is_none());

    let fetched = store.get_agent(agent.id).await.unwrap().expect("agent row");
    assert_eq!(fetched.worker_id, Some(worker.id));
    assert!(
        fetched.assigned_at.unwrap().timestamp_micros() >= before.timestamp_micros(),
        "assigned_at은 DB의 NOW()에서 온다"
    );
}

#[tokio::test]
async fn assign_reports_an_unknown_agent() {
    require_db!(store);
    let worker = seed_worker(&store, "noagent").await;
    let missing = fleet_core::AgentId::new();
    assert_eq!(
        store.assign_agent_worker(missing, worker.id).await.unwrap(),
        SlotClaim::NoSuchAgent,
        "없는 Agent는 오류가 아니라 판정값이다 — 호출자가 404로 번역한다"
    );
}

#[tokio::test]
async fn assign_to_an_unknown_worker_is_reported_not_raised() {
    require_db!(store);
    let project = seed_project(&store, "badworker").await;
    let agent = Agent::new(project.id, "target");
    store.create_agent(&agent).await.unwrap();

    // 예전에는 FK 위반이 `StoreError::Conflict`로 올라왔다. ①-A-2가 상한을
    // 보려고 `workers` 행을 `FOR UPDATE`로 먼저 잠그면서, 없는 Worker는
    // 그 SELECT가 먼저 알아낸다 — 오류가 아니라 판정값이 됐다.
    assert_eq!(
        store
            .assign_agent_worker(agent.id, fleet_core::WorkerId::new())
            .await
            .unwrap(),
        SlotClaim::NoSuchWorker
    );
}

#[tokio::test]
async fn deleting_the_worker_clears_both_halves() {
    require_db!(store);
    let project = seed_project(&store, "vanish").await;
    let worker = seed_worker(&store, "vanish").await;
    let agent = Agent::new(project.id, "orphan").with_placement(worker.id, chrono::Utc::now());
    store.create_agent(&agent).await.unwrap();

    store.delete_worker(worker.id).await.unwrap();

    let fetched = store.get_agent(agent.id).await.unwrap().expect("agent row");
    // FK는 `worker_id`만 NULL로 만들 수 있고, 그것만으로는 both-or-neither
    // CHECK가 깨진다. `assigned_at`까지 비어 있다는 것이 트리거가 실제로
    // 발화했다는 증거다 — FK가 유발한 UPDATE에도 `BEFORE UPDATE OF`가 걸린다.
    assert!(fetched.worker_id.is_none(), "ON DELETE SET NULL");
    assert!(fetched.assigned_at.is_none(), "trigger가 짝을 맞춘다");
    // 그리고 Agent 행 자체는 살아 있다 — Worker가 사라졌다고 Agent 정의가
    // 사라지면 안 된다.
    assert_eq!(fetched.status, AgentStatus::Ready);
}

#[tokio::test]
async fn the_ledger_counts_only_live_assigned_agents() {
    require_db!(store);
    let project = seed_project(&store, "ledger").await;
    let worker = seed_worker(&store, "ledger").await;

    for name in ["l1", "l2"] {
        let a = Agent::new(project.id, name).with_placement(worker.id, chrono::Utc::now());
        store.create_agent(&a).await.unwrap();
    }
    // 미배정 Agent는 어느 Worker의 부하도 아니다.
    store
        .create_agent(&Agent::new(project.id, "floating"))
        .await
        .unwrap();

    let load = store.count_agents_by_worker().await.unwrap();
    assert_eq!(load.get(&worker.id).copied(), Some(2));

    // 중지하면 슬롯이 풀린다 — 배정 회수 경로(`unassign_agent_worker`)를
    // 따로 만들지 않은 근거가 이것이다.
    let live = store
        .list_agents(&AgentFilter {
            worker_id: Some(worker.id),
            ..Default::default()
        })
        .await
        .unwrap();
    store
        .update_agent_status(live[0].id, AgentStatus::Stopped)
        .await
        .unwrap();

    let load = store.count_agents_by_worker().await.unwrap();
    assert_eq!(load.get(&worker.id).copied(), Some(1));

    // 다만 **배정 자체는 남는다** — 어디서 돌았는지가 기록이기 때문이다.
    let stopped = store.get_agent(live[0].id).await.unwrap().unwrap();
    assert_eq!(stopped.worker_id, Some(worker.id));
}

#[tokio::test]
async fn list_agents_filters_by_worker() {
    require_db!(store);
    let project = seed_project(&store, "byworker").await;
    let a_worker = seed_worker(&store, "byworker-a").await;
    let b_worker = seed_worker(&store, "byworker-b").await;

    let on_a = Agent::new(project.id, "on-a").with_placement(a_worker.id, chrono::Utc::now());
    let on_b = Agent::new(project.id, "on-b").with_placement(b_worker.id, chrono::Utc::now());
    store.create_agent(&on_a).await.unwrap();
    store.create_agent(&on_b).await.unwrap();
    store
        .create_agent(&Agent::new(project.id, "on-none"))
        .await
        .unwrap();

    let listed = store
        .list_agents(&AgentFilter {
            project_id: Some(project.id),
            worker_id: Some(a_worker.id),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, on_a.id);

    // 필터를 생략하면 미배정 Agent까지 전부 나온다. 반대로 "미배정만"을
    // 뜻하는 값은 없다 — 그 질문을 하는 호출자가 아직 없다.
    let all = store
        .list_agents(&AgentFilter {
            project_id: Some(project.id),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
}

// ── 수렴 프로토콜 (로드맵 #67 4b) ───────────────────────────────────
//
// 여기서 증명하는 것은 **전달**이지 수렴이 아니다. 프로세스를 관측하는
// 주체가 4c에 가서야 생기므로, 4b가 말할 수 있는 최대치는 "명령이 배정된
// Worker에 도달했고 그 Worker가 같은 세대를 되돌려줬다"까지다.
//
// 위 파일 머리말의 격리 규약을 그대로 따른다 — TRUNCATE 대신 테스트마다
// 유일한 Project·Worker를 만들고 그 안에서만 조회한다.

#[tokio::test]
async fn a_new_agent_wants_nothing_and_has_nothing_pending() {
    require_db!(store);
    let project = seed_project(&store, "conv-new").await;
    let agent = Agent::new(project.id, "fresh");
    store.create_agent(&agent).await.unwrap();

    let fetched = store.get_agent(agent.id).await.unwrap().expect("agent row");
    assert_eq!(fetched.desired_status, AgentDesiredStatus::Stopped);
    assert_eq!(fetched.command_generation, 0);
    assert_eq!(fetched.last_acked_generation, 0);
    // 두 세대가 0으로 같으므로 `command_delivered()`가 true다. 이것이 그
    // 값의 정확한 뜻을 드러낸다 — "미전달 명령이 없다"이지 "무언가
    // 확인됐다"가 아니다. 아직 발행된 명령 자체가 없다.
    assert!(fetched.command_delivered());
    assert!(!fetched.start_pending());
}

#[tokio::test]
async fn desired_status_bumps_the_generation_only_when_the_value_changes() {
    require_db!(store);
    let project = seed_project(&store, "conv-bump").await;
    let agent = Agent::new(project.id, "bumper");
    store.create_agent(&agent).await.unwrap();

    assert!(store
        .set_agent_desired_status(agent.id, AgentDesiredStatus::Running)
        .await
        .unwrap());
    let first = store.get_agent(agent.id).await.unwrap().unwrap();
    assert_eq!(first.command_generation, 1);
    assert!(
        first.start_pending(),
        "ready + running + 관측 없음 = 시작 대기"
    );

    // 같은 의도를 다시 눌러도 세대는 그대로다. 매 호출마다 올리면 이미
    // 확인된 명령이 반복 클릭만으로 미확인으로 되돌아간다.
    assert!(store
        .set_agent_desired_status(agent.id, AgentDesiredStatus::Running)
        .await
        .unwrap());
    let again = store.get_agent(agent.id).await.unwrap().unwrap();
    assert_eq!(again.command_generation, 1);

    // 존재하지 않는 id는 `false`다. 이 구분이 있으려면 UPDATE 술어에
    // `desired_status <> $2`를 넣으면 안 된다 — 넣으면 "바뀐 것이 없음"과
    // "그런 Agent가 없음"이 같은 0-row가 된다.
    assert!(!store
        .set_agent_desired_status(fleet_core::AgentId::new(), AgentDesiredStatus::Running)
        .await
        .unwrap());
}

#[tokio::test]
async fn commands_go_only_to_the_assigned_worker() {
    require_db!(store);
    let project = seed_project(&store, "conv-route").await;
    let mine = seed_worker(&store, "conv-route-mine").await;
    let other = seed_worker(&store, "conv-route-other").await;

    let agent = Agent::new(project.id, "routed").with_placement(mine.id, chrono::Utc::now());
    store.create_agent(&agent).await.unwrap();
    store
        .set_agent_desired_status(agent.id, AgentDesiredStatus::Running)
        .await
        .unwrap();

    let cmds = store.list_agent_commands(mine.id).await.unwrap();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].agent_id, agent.id);
    assert_eq!(cmds[0].desired_status, AgentDesiredStatus::Running);
    assert_eq!(cmds[0].generation, 1);

    assert!(store
        .list_agent_commands(other.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_late_ack_cannot_confirm_a_command_that_was_already_superseded() {
    require_db!(store);
    let project = seed_project(&store, "conv-late").await;
    let worker = seed_worker(&store, "conv-late").await;
    let agent = Agent::new(project.id, "late").with_placement(worker.id, chrono::Utc::now());
    store.create_agent(&agent).await.unwrap();

    store
        .set_agent_desired_status(agent.id, AgentDesiredStatus::Running)
        .await
        .unwrap(); // 세대 1
    store
        .update_agent_status(agent.id, AgentStatus::Stopped)
        .await
        .unwrap(); // 회수가 desired를 내리며 세대 2

    // 세대 1을 뒤늦게 확인해도 반영되지 않는다. 이것이 큐 모델에서 따로
    // 만들어야 했던 "만료" 장치를 대신한다 — 신선도 판정을 세대가 한다.
    let applied = store
        .ack_agent_commands(
            worker.id,
            &[fleet_core::AgentAck {
                agent_id: agent.id,
                generation: 1,
            }],
        )
        .await
        .unwrap();
    assert_eq!(applied, 0);

    let fetched = store.get_agent(agent.id).await.unwrap().unwrap();
    assert_eq!(fetched.command_generation, 2);
    assert_eq!(fetched.last_acked_generation, 0);
    assert!(!fetched.command_delivered());
}

#[tokio::test]
async fn an_ack_from_a_worker_that_no_longer_owns_the_agent_is_rejected() {
    require_db!(store);
    let project = seed_project(&store, "conv-steal").await;
    let old = seed_worker(&store, "conv-steal-old").await;
    let new = seed_worker(&store, "conv-steal-new").await;
    let agent = Agent::new(project.id, "moved").with_placement(old.id, chrono::Utc::now());
    store.create_agent(&agent).await.unwrap();
    store
        .set_agent_desired_status(agent.id, AgentDesiredStatus::Running)
        .await
        .unwrap(); // 세대 1

    // 재배정도 세대를 올린다 — 새 Worker는 이 명령을 아직 본 적이 없으므로
    // 세대를 올리지 않으면 `command_delivered()`가 "새 Worker가 확인했다"는
    // 거짓을 말하게 된다.
    assert_eq!(
        store.assign_agent_worker(agent.id, new.id).await.unwrap(),
        SlotClaim::Claimed
    );
    let moved = store.get_agent(agent.id).await.unwrap().unwrap();
    assert_eq!(moved.command_generation, 2);
    assert_eq!(moved.last_acked_generation, 0);

    // 옛 Worker가 현재 세대를 정확히 들고 와도 통과하지 못한다. 세대만
    // 보면 맞지만 자기 것이 아니다.
    let applied = store
        .ack_agent_commands(
            old.id,
            &[fleet_core::AgentAck {
                agent_id: agent.id,
                generation: 2,
            }],
        )
        .await
        .unwrap();
    assert_eq!(applied, 0);

    let applied = store
        .ack_agent_commands(
            new.id,
            &[fleet_core::AgentAck {
                agent_id: agent.id,
                generation: 2,
            }],
        )
        .await
        .unwrap();
    assert_eq!(applied, 1);
    assert!(store
        .get_agent(agent.id)
        .await
        .unwrap()
        .unwrap()
        .command_delivered());
}

#[tokio::test]
async fn a_repeated_ack_of_the_same_generation_is_a_noop() {
    require_db!(store);
    let project = seed_project(&store, "conv-dup").await;
    let worker = seed_worker(&store, "conv-dup").await;
    let agent = Agent::new(project.id, "dup").with_placement(worker.id, chrono::Utc::now());
    store.create_agent(&agent).await.unwrap();
    store
        .set_agent_desired_status(agent.id, AgentDesiredStatus::Running)
        .await
        .unwrap();

    let ack = [fleet_core::AgentAck {
        agent_id: agent.id,
        generation: 1,
    }];
    assert_eq!(store.ack_agent_commands(worker.id, &ack).await.unwrap(), 1);
    // 매 beat 전체 목록을 다시 싣는 프로토콜이므로 같은 ACK가 계속 온다.
    // 반환값이 "실제로 새로 확인된 수"가 되려면 두 번째는 0이어야 한다.
    assert_eq!(store.ack_agent_commands(worker.id, &ack).await.unwrap(), 0);
}

#[tokio::test]
async fn a_stop_command_stays_on_the_list_until_it_is_acked() {
    require_db!(store);
    let project = seed_project(&store, "conv-stop").await;
    let worker = seed_worker(&store, "conv-stop").await;
    let agent = Agent::new(project.id, "stopper").with_placement(worker.id, chrono::Utc::now());
    store.create_agent(&agent).await.unwrap();
    store
        .set_agent_desired_status(agent.id, AgentDesiredStatus::Running)
        .await
        .unwrap();
    store
        .ack_agent_commands(
            worker.id,
            &[fleet_core::AgentAck {
                agent_id: agent.id,
                generation: 1,
            }],
        )
        .await
        .unwrap();

    // 회수. `status <> 'stopped'`만으로 목록을 걸렀다면 여기서 올라간 세대
    // 2가 **영원히 전달되지 않고**, 회수된 모든 Agent의
    // `command_delivered()`가 항상 false로 남는다.
    store
        .update_agent_status(agent.id, AgentStatus::Stopped)
        .await
        .unwrap();
    let cmds = store.list_agent_commands(worker.id).await.unwrap();
    assert_eq!(cmds.len(), 1, "회수 명령도 전달되어야 한다");
    assert_eq!(cmds[0].desired_status, AgentDesiredStatus::Stopped);
    assert_eq!(cmds[0].generation, 2);

    // 확인되면 조용해진다 — 목록이 무한히 자라지 않는다는 것이 이 술어를
    // 넓혀도 안전한 이유다.
    store
        .ack_agent_commands(
            worker.id,
            &[fleet_core::AgentAck {
                agent_id: agent.id,
                generation: 2,
            }],
        )
        .await
        .unwrap();
    assert!(store
        .list_agent_commands(worker.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn acking_does_not_move_updated_at() {
    require_db!(store);
    let project = seed_project(&store, "conv-touch").await;
    let worker = seed_worker(&store, "conv-touch").await;
    let agent = Agent::new(project.id, "untouched").with_placement(worker.id, chrono::Utc::now());
    store.create_agent(&agent).await.unwrap();
    // 먼저 running으로 올려 두지 않으면 회수가 세대를 올리지 않는다
    // (`desired_status`가 이미 `stopped`이므로 `CASE`가 0을 준다). 그러면
    // 확인할 명령 자체가 없어 이 테스트가 아무것도 증명하지 못한다.
    store
        .set_agent_desired_status(agent.id, AgentDesiredStatus::Running)
        .await
        .unwrap(); // 세대 1
    store
        .update_agent_status(agent.id, AgentStatus::Stopped)
        .await
        .unwrap(); // 세대 2
    let stopped_at = store.get_agent(agent.id).await.unwrap().unwrap().updated_at;

    let applied = store
        .ack_agent_commands(
            worker.id,
            &[fleet_core::AgentAck {
                agent_id: agent.id,
                generation: 2,
            }],
        )
        .await
        .unwrap();
    assert_eq!(applied, 1);

    // 두 회수 표면(Dashboard·MCP)이 이미 `Stopped`인 Agent에 대해 쓰기를
    // 건너뛰는 것은 "언제 회수됐는가"를 지키기 위해서다. ACK가
    // `updated_at`을 밀면 한 beat 뒤에 그 불변식이 무효가 된다 — ACK는
    // 운영자의 조작이 아니라 프로토콜 부기다.
    let after = store.get_agent(agent.id).await.unwrap().unwrap();
    assert_eq!(
        after.updated_at.timestamp_micros(),
        stopped_at.timestamp_micros(),
        "ACK는 회수 시각을 밀지 않는다"
    );
    assert!(after.command_delivered());
}

// ---------------------------------------------------------------------------
// 관측 (로드맵 #67 4c-B)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_observation_roundtrips_and_ends_start_pending() {
    require_db!(store);
    let project = seed_project(&store, "obs-rt").await;
    let worker = seed_worker(&store, "obs-rt").await;
    let agent = Agent::new(project.id, "watched").with_placement(worker.id, chrono::Utc::now());
    store.create_agent(&agent).await.unwrap();
    store
        .set_agent_desired_status(agent.id, AgentDesiredStatus::Running)
        .await
        .unwrap();

    let before = store.get_agent(agent.id).await.unwrap().unwrap();
    assert!(
        before.start_pending(),
        "명령만 냈고 관측이 없으면 시작 대기다"
    );
    assert!(before.observed_status.is_none());

    let changed = store
        .apply_agent_observations(
            worker.id,
            &[AgentObservation::Running { agent_id: agent.id }],
        )
        .await
        .unwrap();
    assert_eq!(changed, 1);

    let after = store.get_agent(agent.id).await.unwrap().unwrap();
    assert_eq!(after.observed_status, Some(AgentObservedStatus::Running));
    assert!(after.observed_at.is_some());
    assert_eq!(after.observed_reason, None);
    // 관측은 생명주기를 건드리지 않는다 — 두 축이 한 컬럼을 나눠 쓰면
    // 회수 직후 도착한 beat이 `stopped`를 되돌린다.
    assert_eq!(after.status, AgentStatus::Ready);
    assert_eq!(after.desired_status, AgentDesiredStatus::Running);
    assert!(
        !after.start_pending(),
        "Worker가 돌고 있다고 보고했으면 더는 시작 대기가 아니다"
    );
}

#[tokio::test]
async fn a_failed_observation_carries_its_reason() {
    require_db!(store);
    let project = seed_project(&store, "obs-fail").await;
    let worker = seed_worker(&store, "obs-fail").await;
    let agent = Agent::new(project.id, "rejected").with_placement(worker.id, chrono::Utc::now());
    store.create_agent(&agent).await.unwrap();

    store
        .apply_agent_observations(
            worker.id,
            &[AgentObservation::Failed {
                agent_id: agent.id,
                reason: AgentObservationReason::CapReached,
            }],
        )
        .await
        .unwrap();

    let after = store.get_agent(agent.id).await.unwrap().unwrap();
    assert_eq!(after.observed_status, Some(AgentObservedStatus::Failed));
    assert_eq!(
        after.observed_reason,
        Some(AgentObservationReason::CapReached)
    );
    // 4c-A에서 이 사실은 워커 로그 한 줄이 전부였다. 그것이 여기 도달하는
    // 것이 4c-B의 전부이므로, 이유가 유실되면 이 단계는 아무것도 하지 않은
    // 것과 같다.
    assert!(
        !after.start_pending(),
        "못 띄웠다는 보고도 보고다 — 무한히 시작 대기로 남지 않는다"
    );
}

#[tokio::test]
async fn an_unspoken_agent_loses_its_observation() {
    require_db!(store);
    let project = seed_project(&store, "obs-clear").await;
    let worker = seed_worker(&store, "obs-clear").await;
    let kept = Agent::new(project.id, "kept").with_placement(worker.id, chrono::Utc::now());
    let recalled = Agent::new(project.id, "recalled").with_placement(worker.id, chrono::Utc::now());
    store.create_agent(&kept).await.unwrap();
    store.create_agent(&recalled).await.unwrap();

    store
        .apply_agent_observations(
            worker.id,
            &[
                AgentObservation::Running { agent_id: kept.id },
                AgentObservation::Running {
                    agent_id: recalled.id,
                },
            ],
        )
        .await
        .unwrap();

    // 회수 뒤 Worker는 그 Agent를 더는 언급하지 않는다. 목록이 권위 있는
    // 전체 집합이 아니라면 이 관측을 지울 사람이 아무도 없어서 회수된
    // Agent가 영원히 `running`으로 남는다.
    let changed = store
        .apply_agent_observations(
            worker.id,
            &[AgentObservation::Running { agent_id: kept.id }],
        )
        .await
        .unwrap();
    assert_eq!(changed, 2, "지운 하나 + 다시 쓴 하나");

    assert_eq!(
        store
            .get_agent(recalled.id)
            .await
            .unwrap()
            .unwrap()
            .observed_status,
        None
    );
    assert_eq!(
        store
            .get_agent(kept.id)
            .await
            .unwrap()
            .unwrap()
            .observed_status,
        Some(AgentObservedStatus::Running)
    );

    // 빈 목록은 "하나도 안 돈다"는 적극적인 주장이다. `None`(필드 부재)과
    // 달리 여기까지 도달하며, 남은 관측을 전부 지운다.
    let changed = store
        .apply_agent_observations(worker.id, &[])
        .await
        .unwrap();
    assert_eq!(changed, 1);
    assert_eq!(
        store
            .get_agent(kept.id)
            .await
            .unwrap()
            .unwrap()
            .observed_status,
        None
    );
}

#[tokio::test]
async fn an_observation_from_a_worker_that_does_not_own_the_agent_is_ignored() {
    require_db!(store);
    let project = seed_project(&store, "obs-steal").await;
    let owner = seed_worker(&store, "obs-steal-owner").await;
    let other = seed_worker(&store, "obs-steal-other").await;
    let agent = Agent::new(project.id, "owned").with_placement(owner.id, chrono::Utc::now());
    store.create_agent(&agent).await.unwrap();

    // `ack_agent_commands`의 세 CAS 조건 중 `worker_id`만 여기로 넘어온다.
    // 나머지 둘은 세대에 관한 것이고 관측에는 세대가 없다 — 프로세스는
    // 아무 명령 없이도 죽을 수 있기 때문이다.
    let obs = [AgentObservation::Running { agent_id: agent.id }];

    let changed = store
        .apply_agent_observations(other.id, &obs)
        .await
        .unwrap();
    assert_eq!(changed, 0, "남의 Agent에 대한 관측은 반영되지 않는다");
    assert_eq!(
        store
            .get_agent(agent.id)
            .await
            .unwrap()
            .unwrap()
            .observed_status,
        None
    );

    let changed = store
        .apply_agent_observations(owner.id, &obs)
        .await
        .unwrap();
    assert_eq!(changed, 1, "소유자의 관측은 반영된다");
}

/// 관측은 프로토콜 부기이지 운영자의 조작이 아니다 — ACK와 같은 이유로
/// `updated_at`을 밀지 않는다. 밀면 "언제 회수됐는가"가 한 beat 뒤에
/// 무효가 된다.
#[tokio::test]
async fn observing_does_not_move_updated_at() {
    require_db!(store);
    let project = seed_project(&store, "obs-utime").await;
    let worker = seed_worker(&store, "obs-utime").await;
    let agent = Agent::new(project.id, "quiet").with_placement(worker.id, chrono::Utc::now());
    store.create_agent(&agent).await.unwrap();
    let before = store.get_agent(agent.id).await.unwrap().unwrap().updated_at;

    store
        .apply_agent_observations(
            worker.id,
            &[AgentObservation::Running { agent_id: agent.id }],
        )
        .await
        .unwrap();

    let after = store.get_agent(agent.id).await.unwrap().unwrap();
    assert_eq!(
        after.updated_at.timestamp_micros(),
        before.timestamp_micros(),
        "관측은 Agent의 갱신 시각을 밀지 않는다"
    );
}

/// 상한이 **집행되는 불변식**인지 (로드맵 `#67` 구현 게이트 ①-A-2).
///
/// ①-A-1이 넣은 것은 `choose_worker`의 후보 필터였고, 그것은 읽은 시점의
/// 카운트로 판정하므로 동시에 들어온 두 배정이 **둘 다** 상한 미만을 보고
/// 둘 다 통과할 수 있었다. 여기서 증명하려는 것은 그 창이 닫혔다는 것이다.
///
/// **이 테스트는 MemStore로 대체할 수 없다.** 잠금이 하는 일을 흉내 내는
/// 것이 아니라 잠금이 실재하는지를 보는 것이므로 실 DB여야 한다. 하네스가
/// 판정을 위조하지 않게 하는 자리는 `race_pool`이 맡는다 — 왜 그것이
/// 필요한지는 그쪽 주석에 있다.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_placements_cannot_exceed_the_cap() {
    const N: usize = 8;
    if database_url().is_none() {
        return;
    }
    let store = race_pool(N).await;

    let project = seed_project(&store, "cap-race").await;
    let name = format!("agents-w-caprace-{}", uuid::Uuid::new_v4());
    let mut worker = Worker::new(name.clone(), format!("http://{name}.invalid"));
    // 상한 1 — 성공해야 하는 배정이 정확히 하나여야 세는 실수가 숨지 않는다.
    worker.max_agent_processes = Some(1);
    store.upsert_worker(&worker).await.unwrap();

    let mut ids = Vec::with_capacity(N);
    for i in 0..N {
        let agent = Agent::new(project.id, format!("racer-{i}"));
        store.create_agent(&agent).await.unwrap();
        ids.push(agent.id);
    }

    // Barrier가 없으면 앞선 태스크가 끝난 뒤에 다음이 시작해서, 경합이
    // 일어났는지가 스케줄러의 기분에 달리게 된다. 확률적으로 통과하는
    // 테스트는 통과해도 아무것도 증명하지 않는다.
    let gate = std::sync::Arc::new(tokio::sync::Barrier::new(N));
    let mut handles = Vec::with_capacity(N);
    for id in ids {
        let store = store.clone();
        let gate = gate.clone();
        let worker_id = worker.id;
        handles.push(tokio::spawn(async move {
            gate.wait().await;
            store.assign_agent_worker(id, worker_id).await.unwrap()
        }));
    }

    let mut claimed = 0usize;
    let mut cap_reached = 0usize;
    for h in handles {
        match h.await.unwrap() {
            SlotClaim::Claimed => claimed += 1,
            SlotClaim::CapReached => cap_reached += 1,
            other => panic!("배정도 상한도 아닌 판정이 나왔다: {other:?}"),
        }
    }
    assert_eq!(
        (claimed, cap_reached),
        (1, N - 1),
        "상한 1인 Worker에 {N}개가 동시에 달려들면 하나만 성공해야 한다"
    );

    // 반환값만 보면 Store가 거짓말을 해도 통과한다. 저장된 행을 센다.
    let load = store.count_agents_by_worker().await.unwrap();
    assert_eq!(
        load.get(&worker.id).copied().unwrap_or(0),
        1,
        "판정과 저장된 행이 어긋나면 판정 쪽이 거짓이다"
    );
}

/// 동시 **생성**은 상한을 넘기지 못한다 (로드맵 `#67` 구현 게이트 ①-A-2).
///
/// 바로 위의 테스트가 증명하는 것은 `assign_agent_worker`의 UPDATE 경로다.
/// 그런데 ①-A가 애초에 지목한 시나리오는 **생성** 쪽이었다 — "동시에 들어온
/// 두 생성 요청이 같은 Worker를 골라 그 Worker의 프로세스 상한을 넘긴다".
/// 두 경로는 각자 따로 잠금을 걸므로 한쪽의 붉은 증거가 다른 쪽을 대신하지
/// 않는다. 한쪽만 붉혀 놓고 "둘 다 닫혔다"고 적으면 증거보다 강한 주장이
/// 된다.
///
/// 생성 트랜잭션은 pin 검증까지 포함해 배정보다 길다. 잠금이 없으면 창이
/// 그만큼 넓어지므로 실패는 여기서 더 크게 나타난다.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_creates_cannot_exceed_the_cap() {
    const N: usize = 8;
    if database_url().is_none() {
        return;
    }
    let store = race_pool(N).await;

    let project = seed_project(&store, "create-race").await;
    let name = format!("agents-w-createrace-{}", uuid::Uuid::new_v4());
    let mut worker = Worker::new(name.clone(), format!("http://{name}.invalid"));
    worker.max_agent_processes = Some(1);
    store.upsert_worker(&worker).await.unwrap();

    let gate = std::sync::Arc::new(tokio::sync::Barrier::new(N));
    let mut handles = Vec::with_capacity(N);
    for i in 0..N {
        let store = store.clone();
        let gate = gate.clone();
        let worker_id = worker.id;
        let project_id = project.id;
        handles.push(tokio::spawn(async move {
            // Agent 구성은 배리어 **앞에서** 끝낸다. 배리어 뒤에 남는 것이
            // 적을수록 겹치는 구간이 정확히 선점 트랜잭션이 된다.
            let agent = Agent::new(project_id, format!("creator-{i}"))
                .with_placement(worker_id, chrono::Utc::now());
            let id = agent.id;
            gate.wait().await;
            (id, store.create_agent(&agent).await.unwrap())
        }));
    }

    let mut placed = 0usize;
    let mut unplaced = 0usize;
    for h in handles {
        let (id, result) = h.await.unwrap();
        match result {
            Some(w) => {
                assert_eq!(w, worker.id, "요청한 Worker가 아닌 곳에 배정됐다");
                placed += 1;
            }
            None => unplaced += 1,
        }
        // 어느 쪽이든 Agent 자체는 살아 있어야 한다. 상한은 배정을 떨어뜨릴
        // 뿐 생성을 되돌리지 않는다 — 4a가 정한 것이다.
        assert!(
            store.get_agent(id).await.unwrap().is_some(),
            "상한이 Agent 생성을 되돌렸다"
        );
    }
    assert_eq!(
        (placed, unplaced),
        (1, N - 1),
        "상한 1인 Worker로 {N}개가 동시에 생성되면 배정은 하나뿐이어야 한다"
    );

    // 반환값이 아니라 저장된 행을 센다. 둘이 어긋나면 거짓인 쪽은 반환값이고,
    // 상위 계층의 감사 로그는 그 반환값을 그대로 적는다.
    let load = store.count_agents_by_worker().await.unwrap();
    assert_eq!(
        load.get(&worker.id).copied().unwrap_or(0),
        1,
        "판정과 저장된 행이 어긋나면 판정 쪽이 거짓이다"
    );
}

/// 상한에 걸린 배정은 생성을 되돌리지 않는다 (로드맵 `#67` 구현 게이트 ①-A-2).
///
/// 4a가 정한 것은 "Agent 정의가 Worker 가용성에 인질로 잡히지 않는다"였다.
/// 선점을 생성 트랜잭션 안으로 옮기면서 그 결정이 뒤집히지 않았는지 본다 —
/// 그리고 반환값이 **실제로 기록된 배정**인지도 함께 본다. 반환값이
/// 거짓이면 상위 계층의 감사 로그가 일어나지 않은 배정을 적는다.
#[tokio::test]
async fn creating_onto_a_full_worker_drops_the_placement_not_the_agent() {
    require_db!(store);
    let project = seed_project(&store, "create-cap").await;
    let name = format!("agents-w-createcap-{}", uuid::Uuid::new_v4());
    let mut worker = Worker::new(name.clone(), format!("http://{name}.invalid"));
    worker.max_agent_processes = Some(1);
    store.upsert_worker(&worker).await.unwrap();

    let first = Agent::new(project.id, "first").with_placement(worker.id, chrono::Utc::now());
    assert_eq!(store.create_agent(&first).await.unwrap(), Some(worker.id));

    let second = Agent::new(project.id, "second").with_placement(worker.id, chrono::Utc::now());
    assert_eq!(
        store.create_agent(&second).await.unwrap(),
        None,
        "상한에 걸린 배정은 `None`으로 보고된다"
    );

    let stored = store
        .get_agent(second.id)
        .await
        .unwrap()
        .expect("agent row");
    assert!(stored.worker_id.is_none(), "Agent 자체는 생성됐다");
    assert!(
        stored.assigned_at.is_none(),
        "`030`의 CHECK가 요구하는 대로 두 반쪽이 함께 비어야 한다"
    );
}

/// 같은 Worker로의 재배정은 자기 슬롯을 두 번 세지 않는다
/// (로드맵 `#67` 구현 게이트 ①-A-2).
///
/// 카운트에서 자기 자신을 빼지 않으면, 정확히 가득 찬 Worker에 이미 있는
/// Agent를 같은 Worker로 다시 배정하는 것이 거절된다. 그 재배정은 슬롯을
/// 하나도 더 쓰지 않으므로 상한과 아무 상관이 없다.
#[tokio::test]
async fn replacing_onto_the_same_full_worker_is_not_capped() {
    require_db!(store);
    let project = seed_project(&store, "self-cap").await;
    let name = format!("agents-w-selfcap-{}", uuid::Uuid::new_v4());
    let mut worker = Worker::new(name.clone(), format!("http://{name}.invalid"));
    worker.max_agent_processes = Some(1);
    store.upsert_worker(&worker).await.unwrap();

    let agent = Agent::new(project.id, "resident").with_placement(worker.id, chrono::Utc::now());
    assert_eq!(store.create_agent(&agent).await.unwrap(), Some(worker.id));

    assert_eq!(
        store
            .assign_agent_worker(agent.id, worker.id)
            .await
            .unwrap(),
        SlotClaim::Claimed,
        "이미 그 Worker에 있는 Agent의 재배정은 슬롯을 더 쓰지 않는다"
    );
}
