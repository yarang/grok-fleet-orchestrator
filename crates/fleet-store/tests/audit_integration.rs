//! 감사 로그(`audit_log`) 통합 테스트.
//!
//! 실제 PostgreSQL이 필요합니다. `DATABASE_URL`이 설정되지 않으면 skip하고,
//! **설정되었는데 연결/마이그레이션이 실패하면 panic**합니다 (조용한 skip은
//! 마이그레이션이 깨져도 "통과"로 보이게 만들어 위험합니다).
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test audit_integration -- --test-threads=1
//! ```

use chrono::{Duration, Utc};
use fleet_core::audit::action;
use fleet_core::{AuditEvent, AuditFilter, AuditOutcome, User, UserId};
use fleet_store::{PgStore, Store};
use sqlx::postgres::PgPoolOptions;

async fn try_connect() -> Option<PgStore> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
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
        // audit_log는 users를 참조하므로 users TRUNCATE CASCADE에 함께 포함된다.
        let _ = sqlx::query("TRUNCATE audit_log, sessions, user_roles, users CASCADE")
            .execute($store.pool())
            .await;
    };
}

fn sample_user(username: &str) -> User {
    User {
        id: UserId::new(),
        username: username.to_string(),
        email: Some(format!("{username}@example.com")),
        email_verified: true,
        password_hash: "argon2id$dummy$test-hash".to_string(),
        enabled: true,
        created_at: Utc::now(),
        last_login_at: None,
    }
}

#[tokio::test]
async fn audit_event_roundtrip_preserves_all_fields() {
    require_db!(store);

    let user = sample_user("alice");
    store.create_user(&user).await.unwrap();

    let event = AuditEvent::success("alice", action::USER_DELETE)
        .actor(user.id)
        .target("user", "target-id-123")
        .ip("10.0.0.7")
        .detail(serde_json::json!({ "target_username": "bob" }));
    let event_id = event.id;

    store.record_audit_event(&event).await.unwrap();

    let events = store
        .list_audit_events(&AuditFilter::default())
        .await
        .unwrap();
    let found = events
        .iter()
        .find(|e| e.id == event_id)
        .expect("recorded event must be listed");

    assert_eq!(found.actor_user_id, Some(user.id));
    assert_eq!(found.actor_label, "alice");
    assert_eq!(found.action, action::USER_DELETE);
    assert_eq!(found.target_type.as_deref(), Some("user"));
    assert_eq!(found.target_id.as_deref(), Some("target-id-123"));
    assert_eq!(found.outcome, AuditOutcome::Success);
    assert_eq!(found.ip_address.as_deref(), Some("10.0.0.7"));
    assert_eq!(found.detail["target_username"], "bob");
}

/// 미인증 이벤트(로그인 실패)는 actor_user_id 없이 기록될 수 있어야 한다.
#[tokio::test]
async fn audit_event_without_actor_is_recorded() {
    require_db!(store);

    let event = AuditEvent::failure("attacker@example.com", action::AUTH_LOGIN)
        .ip("203.0.113.9")
        .detail(serde_json::json!({ "reason": "invalid_credentials" }));
    store.record_audit_event(&event).await.unwrap();

    let events = store
        .list_audit_events(&AuditFilter::default())
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].actor_user_id.is_none());
    assert_eq!(events[0].actor_label, "attacker@example.com");
    assert_eq!(events[0].outcome, AuditOutcome::Failure);
}

/// **핵심 설계 보증**: 사용자를 삭제해도 감사 기록은 남아야 한다.
///
/// FK가 CASCADE였다면 계정을 지우는 것만으로 그 사용자의 모든 감사 흔적이
/// 사라진다 — 감사 로그의 존재 의의가 없어진다. ON DELETE SET NULL로
/// actor_user_id만 비우고 actor_label로 추적 가능해야 한다.
#[tokio::test]
async fn audit_events_survive_user_deletion() {
    require_db!(store);

    let user = sample_user("mallory");
    store.create_user(&user).await.unwrap();

    store
        .record_audit_event(
            &AuditEvent::success("mallory", action::AUTH_LOGIN)
                .actor(user.id)
                .ip("10.0.0.1"),
        )
        .await
        .unwrap();

    store.delete_user(user.id).await.unwrap();

    let events = store
        .list_audit_events(&AuditFilter::default())
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "사용자를 지워도 감사 기록은 남아야 한다");
    assert!(
        events[0].actor_user_id.is_none(),
        "삭제된 사용자 참조는 NULL이 되어야 한다"
    );
    assert_eq!(
        events[0].actor_label, "mallory",
        "누구였는지는 actor_label로 추적 가능해야 한다"
    );
}

#[tokio::test]
async fn audit_events_are_listed_newest_first() {
    require_db!(store);

    let now = Utc::now();
    for (i, label) in ["first", "second", "third"].iter().enumerate() {
        let mut ev = AuditEvent::success(*label, action::AUTH_LOGIN);
        ev.created_at = now + Duration::seconds(i as i64);
        store.record_audit_event(&ev).await.unwrap();
    }

    let events = store
        .list_audit_events(&AuditFilter::default())
        .await
        .unwrap();
    let labels: Vec<&str> = events.iter().map(|e| e.actor_label.as_str()).collect();
    assert_eq!(labels, vec!["third", "second", "first"]);
}

#[tokio::test]
async fn audit_filter_by_action() {
    require_db!(store);

    store
        .record_audit_event(&AuditEvent::success("alice", action::AUTH_LOGIN))
        .await
        .unwrap();
    store
        .record_audit_event(&AuditEvent::success("alice", action::USER_DELETE))
        .await
        .unwrap();

    let filter = AuditFilter {
        action: Some(action::AUTH_LOGIN.to_string()),
        ..Default::default()
    };
    let events = store.list_audit_events(&filter).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].action, action::AUTH_LOGIN);
}

#[tokio::test]
async fn audit_filter_by_actor() {
    require_db!(store);

    let alice = sample_user("alice");
    let bob = sample_user("bob");
    store.create_user(&alice).await.unwrap();
    store.create_user(&bob).await.unwrap();

    store
        .record_audit_event(&AuditEvent::success("alice", action::AUTH_LOGIN).actor(alice.id))
        .await
        .unwrap();
    store
        .record_audit_event(&AuditEvent::success("bob", action::AUTH_LOGIN).actor(bob.id))
        .await
        .unwrap();

    let filter = AuditFilter {
        actor_user_id: Some(alice.id),
        ..Default::default()
    };
    let events = store.list_audit_events(&filter).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].actor_label, "alice");
}

#[tokio::test]
async fn audit_pagination_with_limit_and_offset() {
    require_db!(store);

    let now = Utc::now();
    for i in 0..5 {
        let mut ev = AuditEvent::success(format!("user-{i}"), action::AUTH_LOGIN);
        ev.created_at = now + Duration::seconds(i);
        store.record_audit_event(&ev).await.unwrap();
    }

    let page = |limit: usize, offset: usize| AuditFilter {
        limit,
        offset,
        ..Default::default()
    };

    let first = store.list_audit_events(&page(2, 0)).await.unwrap();
    let second = store.list_audit_events(&page(2, 2)).await.unwrap();
    let third = store.list_audit_events(&page(2, 4)).await.unwrap();

    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    assert_eq!(third.len(), 1);

    // 최신순이므로 user-4가 첫 페이지 맨 앞.
    assert_eq!(first[0].actor_label, "user-4");
    // 페이지 간 중복이 없어야 한다.
    let all: Vec<String> = first
        .iter()
        .chain(second.iter())
        .chain(third.iter())
        .map(|e| e.actor_label.clone())
        .collect();
    let mut dedup = all.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(all.len(), dedup.len(), "페이지 간 중복이 없어야 한다");
}
