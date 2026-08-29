//! AgentTemplate / revision 저장소 계약 테스트 (로드맵 #86, 1단계).
//!
//! [설계 정본](../../../docs/architecture/agents/agent-template.md)의 구현
//! 게이트 중 이 커밋에서 **도달 가능한** 것들을 덮는다.
//!
//! | 게이트 | 여기서 | 왜 |
//! |---|---|---|
//! | 1 전이표와 `Retired → Published` 간선 부재 | 덮음 | 코어의 표 + Store의 집행 |
//! | 2 content 변경 거절 / 같은 content 재발행 | 덮음 | revision immutability의 요지 |
//! | 3 retire의 dependent set 해시 | 덮음 | `agents.agent_template_id`가 이 커밋에 생겼다 |
//! | 8 MemStore/PgStore 공유 행동 | 덮음 | `#78`의 교훈 |
//! | 4 실행 중 retire와 harness manifest | 미도달 | 실행 중인 Agent 프로세스가 없다 |
//! | 5 `Hibernated → Starting` admission 거절 | 미도달 | 같은 이유 |
//! | 6 Project grant 교집합 | 미도달 | `projects`에 정책 컬럼이 없다(`#48`) |
//! | 6b 필드별 게이팅 | 코어 단위 테스트 | 권한 판단은 Store 계층이 아니다 |
//! | 7 `builtin/default@1` 시드 | 미도달 | 시드를 넣는 주체가 없다(`#52`) |
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test agent_templates
//! ```
//!
//! `DATABASE_URL`이 없으면 PgStore 쪽만 skip되고 MemStore 쪽은 그대로 돈다.
//!
//! **`--test-threads=1`을 요구하지 않는다** — `tests/agents.rs`와 같은
//! 관례로, 공유 테이블을 TRUNCATE하는 대신 매 테스트가 전역적으로 유일한
//! 이름을 쓰고 자기가 만든 행으로만 조회를 좁힌다. 여기서는 템플릿 이름의
//! UNIQUE 범위가 `(project_id, name)`이라 Project를 유일하게 만드는 것만으로
//! 이름 충돌이 사라진다. 전역 템플릿(`project_id IS NULL`)을 만드는
//! 테스트만 이름 자체에 UUID를 붙인다.

use fleet_core::{
    Agent, AgentTemplate, AgentTemplateBody, AgentTemplateFilter, AgentTemplatePin,
    AgentTemplateStatus, Project,
};
use fleet_store::mem::MemStore;
use fleet_store::{PgStore, Store, StoreError};
use sqlx::postgres::PgPoolOptions;

async fn try_connect() -> Option<PgStore> {
    let url = std::env::var("DATABASE_URL").ok()?;
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

/// 두 구현에 같은 시나리오를 돌린다 (게이트 8).
///
/// `#78`에서 MemStore에만 있던 규칙이 PgStore에 없어 실 배포에서만 깨진 적이
/// 있다. 시나리오를 함수 하나로 두면 한쪽에만 고치는 실수가 컴파일 단계에서
/// 드러나지는 않지만, 최소한 **같은 단정**이 양쪽에 걸린다.
async fn seed_project(store: &dyn Store, label: &str) -> Project {
    let project = Project::new(format!("agent-templates-{label}-{}", uuid::Uuid::new_v4()));
    store.create_project(&project).await.unwrap();
    project
}

fn body(prompt: &str, tools: &[&str]) -> AgentTemplateBody {
    AgentTemplateBody::new(prompt).with_tools(tools.iter().map(|t| t.to_string()))
}

// ── 게이트 1: 전이 ────────────────────────────────────────────────────

async fn gate1_transitions(store: &dyn Store) {
    let project = seed_project(store, "g1").await;
    let template = AgentTemplate::new(Some(project.id), "reviewer");
    store.create_agent_template(&template).await.unwrap();

    let fetched = store
        .get_agent_template(template.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.status,
        AgentTemplateStatus::Draft,
        "새 템플릿은 항상 Draft"
    );

    assert!(store
        .update_agent_template_status(template.id, AgentTemplateStatus::Published)
        .await
        .unwrap());

    // `Retired`는 종착이다. Store가 아니라 코어의 표가 그것을 정하며, 표면은
    // 그 표를 물어본다 — 여기서는 표가 실제로 그렇게 답하는지만 확인한다.
    assert!(!AgentTemplateStatus::Retired.can_transition_to(AgentTemplateStatus::Published));

    // 없는 id는 오류가 아니라 `false`.
    assert!(!store
        .update_agent_template_status(
            fleet_core::AgentTemplateId::new(),
            AgentTemplateStatus::Published
        )
        .await
        .unwrap());
}

// ── 게이트 2: revision immutability ──────────────────────────────────

async fn gate2_revision_immutability(store: &dyn Store) {
    let project = seed_project(store, "g2").await;
    let template = AgentTemplate::new(Some(project.id), "reviewer");
    store.create_agent_template(&template).await.unwrap();
    store
        .update_agent_template_status(template.id, AgentTemplateStatus::Published)
        .await
        .unwrap();

    let first = store
        .create_agent_template_revision(template.id, &body("review carefully", &["fs.read"]), None)
        .await
        .unwrap();
    assert_eq!(first.content_revision, 1);

    let second = store
        .create_agent_template_revision(template.id, &body("review boldly", &["fs.read"]), None)
        .await
        .unwrap();
    assert_eq!(second.content_revision, 2);
    assert_ne!(first.content_hash, second.content_hash);

    // 같은 content를 다시 발행하면 **새 revision id**에 **같은 hash**다.
    // hash에 UNIQUE를 걸지 않은 이유가 이것이다 — 되돌리기는 정당한 조작이고,
    // 그때 이력은 세 건이어야 한다.
    let third = store
        .create_agent_template_revision(template.id, &body("review carefully", &["fs.read"]), None)
        .await
        .unwrap();
    assert_eq!(third.content_revision, 3);
    assert_ne!(third.id, first.id);
    assert_eq!(third.content_hash, first.content_hash);

    // 저장된 본문에서 hash를 **재계산**할 수 있어야 감사 대조가 성립한다.
    let stored = store
        .get_agent_template_revision(first.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.body.content_hash(), stored.content_hash);

    let listed = store
        .list_agent_template_revisions(template.id)
        .await
        .unwrap();
    assert_eq!(listed.len(), 3);
    assert_eq!(
        listed.first().map(|r| r.content_revision),
        Some(3),
        "최신순"
    );

    // 본문을 바꾸는 메서드는 트레이트에 **없다**. 그래서 "변경 시도가
    // 거절된다"를 런타임으로 확인할 자리가 없고, 대신 이 단정이 그 부재를
    // 대신한다: 세 번 발행했는데 첫 revision의 본문은 그대로다.
    assert_eq!(stored.body.role_prompt, "review carefully");

    // 종착 상태에서는 새 revision을 못 만든다.
    let hash = fleet_core::dependent_set_hash(&[]);
    assert!(store
        .retire_agent_template(template.id, &hash)
        .await
        .unwrap());
    let err = store
        .create_agent_template_revision(template.id, &body("too late", &[]), None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::Conflict(_)),
        "Retired 템플릿의 새 revision은 Conflict여야 하는데 {err:?}"
    );
}

// ── 게이트 3: retire의 dependent set 해시 ─────────────────────────────

async fn gate3_retire_dependent_set(store: &dyn Store) {
    let project = seed_project(store, "g3").await;
    let template = AgentTemplate::new(Some(project.id), "reviewer");
    store.create_agent_template(&template).await.unwrap();
    store
        .update_agent_template_status(template.id, AgentTemplateStatus::Published)
        .await
        .unwrap();
    let revision = store
        .create_agent_template_revision(template.id, &body("review", &["fs.read"]), None)
        .await
        .unwrap();

    // 이 커밋 전이었다면 의존 집합은 **항상 비어 있었다**. `agents`에 pin
    // 컬럼이 생겨서 비로소 이 게이트가 공허하지 않다.
    let agent = Agent::new(project.id, "a1").with_template_pin(AgentTemplatePin {
        template_id: template.id,
        revision_id: revision.id,
    });
    store.create_agent(&agent).await.unwrap();

    let dependents = store.agent_template_dependents(template.id).await.unwrap();
    assert_eq!(dependents, vec![agent.id]);
    let hash = fleet_core::dependent_set_hash(&dependents);

    // 빈 집합의 해시로는 retire할 수 없다 — 확인 화면이 "아무도 안 쓴다"를
    // 보여줬는데 실제로는 한 명이 쓰고 있는 상황이 정확히 이 경우다.
    let err = store
        .retire_agent_template(template.id, &fleet_core::dependent_set_hash(&[]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::Conflict(_)),
        "해시 불일치는 Conflict여야 하는데 {err:?}"
    );
    assert_eq!(
        store
            .get_agent_template(template.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentTemplateStatus::Published,
        "거절된 retire는 상태를 바꾸지 않아야 한다"
    );

    // 맞는 해시로는 통과한다.
    assert!(store
        .retire_agent_template(template.id, &hash)
        .await
        .unwrap());
    assert_eq!(
        store
            .get_agent_template(template.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentTemplateStatus::Retired
    );

    // 없는 id는 오류가 아니라 `false`.
    assert!(!store
        .retire_agent_template(fleet_core::AgentTemplateId::new(), &hash)
        .await
        .unwrap());
}

// ── pin 유효성: FK가 못 보는 것 ────────────────────────────────────────

async fn pin_validation(store: &dyn Store) {
    let project = seed_project(store, "pin").await;
    let template = AgentTemplate::new(Some(project.id), "reviewer");
    store.create_agent_template(&template).await.unwrap();

    let draft_revision = store
        .create_agent_template_revision(template.id, &body("draft body", &[]), None)
        .await
        .unwrap();

    // `Draft`는 pin을 받지 않는다 — 아직 아무에게도 공개되지 않은 본문이다.
    let err = store
        .create_agent(
            &Agent::new(project.id, "on-draft").with_template_pin(AgentTemplatePin {
                template_id: template.id,
                revision_id: draft_revision.id,
            }),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::Conflict(_)),
        "Draft 템플릿 pin은 Conflict여야 하는데 {err:?}"
    );

    store
        .update_agent_template_status(template.id, AgentTemplateStatus::Published)
        .await
        .unwrap();
    let revision = store
        .create_agent_template_revision(template.id, &body("published body", &[]), None)
        .await
        .unwrap();

    // revoke는 **새** pin만 막는다. 먼저 하나 만들어 두고 revoke한 뒤,
    // 기존 Agent가 멀쩡한지와 새 pin이 막히는지를 함께 본다.
    let existing = Agent::new(project.id, "existing").with_template_pin(AgentTemplatePin {
        template_id: template.id,
        revision_id: revision.id,
    });
    store.create_agent(&existing).await.unwrap();
    assert!(store
        .revoke_agent_template_revision(revision.id)
        .await
        .unwrap());
    // 두 번째 revoke는 오류가 아니라 `false` (idempotent).
    assert!(!store
        .revoke_agent_template_revision(revision.id)
        .await
        .unwrap());

    let kept = store.get_agent(existing.id).await.unwrap().unwrap();
    assert_eq!(
        kept.template_pin.map(|p| p.revision_id),
        Some(revision.id),
        "revoke는 이미 pin한 Agent를 건드리지 않는다"
    );

    let err = store
        .create_agent(
            &Agent::new(project.id, "after-revoke").with_template_pin(AgentTemplatePin {
                template_id: template.id,
                revision_id: revision.id,
            }),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::Conflict(_)),
        "revoke된 revision pin은 Conflict여야 하는데 {err:?}"
    );

    // 다른 템플릿의 revision id를 이 템플릿 id와 섞어서 pin할 수 없다.
    let other = AgentTemplate::new(Some(project.id), "other");
    store.create_agent_template(&other).await.unwrap();
    store
        .update_agent_template_status(other.id, AgentTemplateStatus::Published)
        .await
        .unwrap();
    let other_revision = store
        .create_agent_template_revision(other.id, &body("other body", &[]), None)
        .await
        .unwrap();
    let err = store
        .create_agent(
            &Agent::new(project.id, "mismatched").with_template_pin(AgentTemplatePin {
                template_id: template.id,
                revision_id: other_revision.id,
            }),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::Conflict(_)),
        "template/revision 짝이 안 맞는 pin은 Conflict여야 하는데 {err:?}"
    );
}

// ── 이름 범위와 목록 필터 ─────────────────────────────────────────────

async fn name_scope_and_filter(store: &dyn Store) {
    let a = seed_project(store, "scope-a").await;
    let b = seed_project(store, "scope-b").await;

    store
        .create_agent_template(&AgentTemplate::new(Some(a.id), "shared-name"))
        .await
        .unwrap();
    // 다른 Project라면 같은 이름이 허용된다.
    store
        .create_agent_template(&AgentTemplate::new(Some(b.id), "shared-name"))
        .await
        .unwrap();
    // 같은 Project라면 안 된다.
    let err = store
        .create_agent_template(&AgentTemplate::new(Some(a.id), "shared-name"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::Conflict(_)),
        "같은 Project의 이름 중복은 Conflict여야 하는데 {err:?}"
    );

    // 전역 템플릿 두 건이 같은 이름을 갖지 못한다. Postgres의 `UNIQUE`는
    // NULL을 서로 다른 값으로 보므로 `UNIQUE (project_id, name)` 하나로는
    // 이것이 막히지 않는다 — 그래서 029가 부분 인덱스 두 개를 쓴다.
    let global_name = format!("global-{}", uuid::Uuid::new_v4());
    store
        .create_agent_template(&AgentTemplate::new(None, global_name.clone()))
        .await
        .unwrap();
    let err = store
        .create_agent_template(&AgentTemplate::new(None, global_name.clone()))
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::Conflict(_)),
        "전역 이름 중복은 Conflict여야 하는데 {err:?}"
    );

    // `project_scope`의 3상태. `Some(None)`은 전역만인데, SQL에서 이것을
    // `project_id = NULL`로 쓰면 한 건도 안 잡힌다(`IS NOT DISTINCT FROM`).
    let scoped = store
        .list_agent_templates(&AgentTemplateFilter {
            project_scope: Some(Some(a.id)),
            status: None,
            limit: 100,
            offset: 0,
        })
        .await
        .unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].project_id, Some(a.id));

    let globals = store
        .list_agent_templates(&AgentTemplateFilter {
            project_scope: Some(None),
            status: None,
            limit: 1000,
            offset: 0,
        })
        .await
        .unwrap();
    assert!(
        globals.iter().any(|t| t.name == global_name),
        "전역 범위 조회가 방금 만든 전역 템플릿을 못 찾았다"
    );
    assert!(
        globals.iter().all(|t| t.project_id.is_none()),
        "전역 범위 조회에 Project 템플릿이 섞였다"
    );
}

// ── 두 구현에 같은 시나리오를 건다 (게이트 8) ─────────────────────────

macro_rules! both_stores {
    ($name:ident, $scenario:ident) => {
        mod $name {
            use super::*;

            #[tokio::test]
            async fn mem() {
                $scenario(&MemStore::new()).await;
            }

            #[tokio::test]
            async fn pg() {
                let Some(store) = try_connect().await else {
                    return;
                };
                $scenario(&store).await;
            }
        }
    };
}

both_stores!(gate1, gate1_transitions);
both_stores!(gate2, gate2_revision_immutability);
both_stores!(gate3, gate3_retire_dependent_set);
both_stores!(pin, pin_validation);
both_stores!(scope, name_scope_and_filter);
