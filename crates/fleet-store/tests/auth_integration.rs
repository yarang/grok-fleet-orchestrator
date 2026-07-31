//! fleet-store 인증(RBAC) 통합 테스트 — User / Session / LoginAttempt /
//! PasswordResetToken / EmailVerificationToken.
//!
//! 실제 PostgreSQL 데이터베이스가 필요합니다. `DATABASE_URL` 환경변수가
//! 설정되지 않거나 연결할 수 없으면 모든 테스트가 자동으로 skip됩니다.
//!
//! ## 배경
//!
//! `/forgot-password`, `/reset-password`, `/resend-verification`의 rate
//! limiter가 실제 트래픽에서 전혀 동작하지 않던 P0 버그가 발견/수정되었다.
//! 근본 원인은 `PgStore::count_recent_failed_attempts`의 SQL이
//! `ip_address IS NOT DISTINCT FROM $2`만 사용해 `$2 = NULL`(=identifier
//! 단독 카운트) 요청 시 `ip_address IS NULL`인 행만 세어 카운터가 구조적으로
//! 항상 0이었던 것 — `/login`의 identifier당 5회 제한도 이 버그로 인해
//! 죽어 있었고(IP당 20회 제한만 동작), 다른 세 엔드포인트는 IP 제한조차
//! 없어 완전 무방비였다.
//!
//! 이 파일은 그 회귀를 다시 잡아내는 store-level 테스트
//! (`ip_scoping_regression`)와, 수정된 rate limiter가 실제로 각 엔드포인트의
//! identifier 포맷/threshold로 차단에 도달하는지 검증하는 시나리오 테스트를
//! 포함한다.
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test auth_integration -- --test-threads=1
//! ```

use chrono::{DateTime, Duration, Utc};
use fleet_core::{
    EmailVerificationToken, LoginAttempt, PasswordResetToken, Session, SessionId, User, UserId,
};
use fleet_store::{PgStore, Store, StoreError};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

/// `fleet-dashboard/src/auth.rs`의 rate limit 상수를 그대로 복제.
/// (fleet-store → fleet-dashboard 의존은 계층 역전이므로 상수만 복제하고
/// 값이 바뀌면 이 파일도 함께 갱신해야 한다.)
const MAX_FAILED_ATTEMPTS: u64 = 5; // identifier 단독 (사용자명/이메일당)
const MAX_IP_FAILED_ATTEMPTS: u64 = 20; // IP 단독
const WINDOW_SECS: i64 = 60;

/// 테스트용 데이터베이스 URL. `DATABASE_URL` 환경변수가 설정된 경우에만 사용.
/// 설정되지 않으면 모든 테스트가 자동으로 skip됩니다.
fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

/// DB 연결 가능 여부 확인. `DATABASE_URL`이 아예 설정되지 않은 경우에만
/// `None`(테스트 skip)을 반환한다.
///
/// **중요**: `DATABASE_URL`이 설정되어 있는데 연결이나 마이그레이션이
/// 실패하면 여기서 `panic!`한다 — 절대 `None`을 반환해 조용히 skip시키지
/// 않는다. 예전에는 이 두 경우(URL 미설정 vs 연결/마이그레이션 실패)를
/// 구분하지 않고 둘 다 `None`으로 뭉뚱그렸는데, 그 결과 `migrations/004_rbac.sql`
/// 의 부분 인덱스 술어(`WHERE expires_at > NOW()`, STABLE 함수라 Postgres가
/// "must be marked IMMUTABLE"로 거부)가 마이그레이션 전체를 깨뜨렸을 때도
/// 이 테스트 파일의 35개 테스트가 전부 `... ok`로 "통과" 표시되면서 실제로는
/// 단 한 줄의 assert도 실행되지 않았다. CI에서도 동일하게 조용히 무의미한
/// 통과가 발생할 수 있는 구조적 위험이었다.
async fn try_connect() -> Option<PgStore> {
    let url = database_url()?;
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

/// 테스트 헬퍼: 스토어 초기화 + 클린업. 연결 불가 시 early return.
macro_rules! require_db {
    ($store:ident) => {
        let $store = match try_connect().await {
            Some(s) => s,
            None => return,
        };
        // 각 테스트 전 인증 관련 테이블을 비운다 (FK CASCADE로 users 삭제 시
        // sessions/user_roles도 함께 삭제됨).
        let _ = sqlx::query(
            "TRUNCATE login_attempts, password_reset_tokens, email_verification_tokens, \
             sessions, user_roles, users CASCADE",
        )
        .execute($store.pool())
        .await;
    };
}

fn sample_user(email: &str, username: &str) -> User {
    User {
        id: UserId::new(),
        username: username.to_string(),
        email: Some(email.to_string()),
        email_verified: false,
        password_hash: "argon2id$dummy$test-hash".to_string(),
        enabled: true,
        created_at: Utc::now(),
        last_login_at: None,
    }
}

fn sample_session(user_id: UserId, token_hash: &str) -> Session {
    Session {
        id: SessionId::new(),
        user_id,
        token_hash: token_hash.to_string(),
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(8),
        ip_address: Some("127.0.0.1".to_string()),
        user_agent: Some("integration-test-agent".to_string()),
    }
}

fn sample_email_verification_token(user_id: UserId, token_hash: &str) -> EmailVerificationToken {
    EmailVerificationToken {
        id: Uuid::new_v4(),
        user_id,
        token_hash: token_hash.to_string(),
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(24),
        consumed_at: None,
    }
}

fn sample_password_reset_token(user_id: UserId, token_hash: &str) -> PasswordResetToken {
    // PasswordResetToken은 EmailVerificationToken의 타입 별칭.
    PasswordResetToken {
        id: Uuid::new_v4(),
        user_id,
        token_hash: token_hash.to_string(),
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(1),
        consumed_at: None,
    }
}

fn sample_login_attempt(
    identifier: &str,
    ip: Option<&str>,
    success: bool,
    reason: Option<&str>,
    at: DateTime<Utc>,
) -> LoginAttempt {
    LoginAttempt {
        id: Uuid::new_v4(),
        identifier: identifier.to_string(),
        ip_address: ip.map(|s| s.to_string()),
        success,
        failure_reason: reason.map(|s| s.to_string()),
        attempted_at: at,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  User CRUD
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn user_create_and_get_by_id() {
    require_db!(store);

    let user = sample_user("alice@example.com", "alice");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let fetched = store
        .get_user_by_id(user_id)
        .await
        .unwrap()
        .expect("user should exist");
    assert_eq!(fetched.id, user_id);
    assert_eq!(fetched.username, "alice");
    assert_eq!(fetched.email.as_deref(), Some("alice@example.com"));
    assert!(!fetched.email_verified);
    assert!(fetched.enabled);
    assert!(fetched.last_login_at.is_none());
}

#[tokio::test]
async fn user_get_by_id_nonexistent_returns_none() {
    require_db!(store);

    let result = store.get_user_by_id(UserId::new()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn user_get_by_username() {
    require_db!(store);

    let user = sample_user("bob@example.com", "bob");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let fetched = store
        .get_user_by_username("bob")
        .await
        .unwrap()
        .expect("user should exist");
    assert_eq!(fetched.id, user_id);
}

#[tokio::test]
async fn user_get_by_email() {
    require_db!(store);

    let user = sample_user("carol@example.com", "carol");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let fetched = store
        .get_user_by_email("carol@example.com")
        .await
        .unwrap()
        .expect("user should exist");
    assert_eq!(fetched.id, user_id);
}

#[tokio::test]
async fn user_list_and_count() {
    require_db!(store);

    store
        .create_user(&sample_user("u1@example.com", "user1"))
        .await
        .unwrap();
    store
        .create_user(&sample_user("u2@example.com", "user2"))
        .await
        .unwrap();

    let users = store.list_users().await.unwrap();
    assert_eq!(users.len(), 2);

    let count = store.count_users().await.unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn user_create_duplicate_username_conflicts() {
    require_db!(store);

    let u1 = sample_user("dup1@example.com", "dupuser");
    let mut u2 = sample_user("dup2@example.com", "dupuser"); // same username
    u2.id = UserId::new();

    store.create_user(&u1).await.unwrap();
    let result = store.create_user(&u2).await;
    assert!(matches!(result, Err(StoreError::Conflict(_))));
}

#[tokio::test]
async fn user_create_duplicate_email_conflicts() {
    require_db!(store);

    let u1 = sample_user("same@example.com", "userone");
    let mut u2 = sample_user("same@example.com", "usertwo"); // same email
    u2.id = UserId::new();

    store.create_user(&u1).await.unwrap();
    let result = store.create_user(&u2).await;
    assert!(matches!(result, Err(StoreError::Conflict(_))));
}

#[tokio::test]
async fn user_update_password() {
    require_db!(store);

    let user = sample_user("pwtest@example.com", "pwuser");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    store
        .update_user_password(user_id, "new-argon2-hash")
        .await
        .unwrap();

    let fetched = store.get_user_by_id(user_id).await.unwrap().unwrap();
    assert_eq!(fetched.password_hash, "new-argon2-hash");
}

#[tokio::test]
async fn user_update_last_login() {
    require_db!(store);

    let user = sample_user("lastlogin@example.com", "lluser");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let now = Utc::now();
    store.update_user_last_login(user_id, now).await.unwrap();

    let fetched = store.get_user_by_id(user_id).await.unwrap().unwrap();
    assert!(fetched.last_login_at.is_some());
}

#[tokio::test]
async fn user_set_enabled_toggle() {
    require_db!(store);

    let user = sample_user("toggle@example.com", "toggleuser");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    store.set_user_enabled(user_id, false).await.unwrap();
    let fetched = store.get_user_by_id(user_id).await.unwrap().unwrap();
    assert!(!fetched.enabled);

    store.set_user_enabled(user_id, true).await.unwrap();
    let fetched = store.get_user_by_id(user_id).await.unwrap().unwrap();
    assert!(fetched.enabled);
}

#[tokio::test]
async fn user_delete_cascades_sessions() {
    require_db!(store);

    let user = sample_user("delcascade@example.com", "delcascade");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let session = sample_session(user_id, "delcascade-token-hash");
    store.create_session(&session).await.unwrap();

    store.delete_user(user_id).await.unwrap();

    assert!(store.get_user_by_id(user_id).await.unwrap().is_none());
    // FK ON DELETE CASCADE — 세션도 함께 삭제되어야 함.
    assert!(store
        .get_session_by_token_hash("delcascade-token-hash")
        .await
        .unwrap()
        .is_none());
}

// ═══════════════════════════════════════════════════════════════════════
//  Sessions
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn session_create_and_get_by_token_hash() {
    require_db!(store);

    let user = sample_user("sess1@example.com", "sessuser1");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let session = sample_session(user_id, "session-hash-1");
    let session_id = session.id;
    store.create_session(&session).await.unwrap();

    let fetched = store
        .get_session_by_token_hash("session-hash-1")
        .await
        .unwrap()
        .expect("session should exist");
    assert_eq!(fetched.id, session_id);
    assert_eq!(fetched.user_id, user_id);
    assert_eq!(fetched.ip_address.as_deref(), Some("127.0.0.1"));
    assert!(!fetched.is_expired());
}

#[tokio::test]
async fn session_get_by_token_hash_nonexistent_returns_none() {
    require_db!(store);

    let result = store
        .get_session_by_token_hash("nonexistent-hash")
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn session_delete() {
    require_db!(store);

    let user = sample_user("sess2@example.com", "sessuser2");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let session = sample_session(user_id, "session-hash-2");
    let session_id = session.id;
    store.create_session(&session).await.unwrap();

    store.delete_session(session_id).await.unwrap();
    assert!(store
        .get_session_by_token_hash("session-hash-2")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn session_delete_expired_removes_expired_only() {
    require_db!(store);

    let user = sample_user("sess3@example.com", "sessuser3");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let mut expired = sample_session(user_id, "expired-hash");
    expired.expires_at = Utc::now() - Duration::hours(1);
    store.create_session(&expired).await.unwrap();

    let mut active = sample_session(user_id, "active-hash");
    active.expires_at = Utc::now() + Duration::hours(1);
    store.create_session(&active).await.unwrap();

    let deleted = store.delete_expired_sessions().await.unwrap();
    assert_eq!(deleted, 1);

    assert!(store
        .get_session_by_token_hash("expired-hash")
        .await
        .unwrap()
        .is_none());
    assert!(store
        .get_session_by_token_hash("active-hash")
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn session_delete_user_sessions_removes_all_for_user() {
    require_db!(store);

    let user = sample_user("sess4@example.com", "sessuser4");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    store
        .create_session(&sample_session(user_id, "multi-hash-1"))
        .await
        .unwrap();
    store
        .create_session(&sample_session(user_id, "multi-hash-2"))
        .await
        .unwrap();

    let deleted = store.delete_user_sessions(user_id).await.unwrap();
    assert_eq!(deleted, 2);
    assert!(store
        .get_session_by_token_hash("multi-hash-1")
        .await
        .unwrap()
        .is_none());
}

// ═══════════════════════════════════════════════════════════════════════
//  Email verification tokens
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn email_verification_token_create_and_get() {
    require_db!(store);

    let user = sample_user("verify1@example.com", "verifyuser1");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let token = sample_email_verification_token(user_id, "verify-token-hash-1");
    store.create_email_verification_token(&token).await.unwrap();

    let fetched = store
        .get_email_verification_token("verify-token-hash-1")
        .await
        .unwrap()
        .expect("token should exist");
    assert_eq!(fetched.user_id, user_id);
    assert!(!fetched.is_expired());
    assert!(!fetched.is_consumed());
}

#[tokio::test]
async fn email_verification_token_consume_sets_consumed_at() {
    require_db!(store);

    let user = sample_user("verify2@example.com", "verifyuser2");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let token = sample_email_verification_token(user_id, "verify-token-hash-2");
    let token_id = token.id;
    store.create_email_verification_token(&token).await.unwrap();

    store
        .consume_email_verification_token(token_id, Utc::now())
        .await
        .unwrap();

    let fetched = store
        .get_email_verification_token("verify-token-hash-2")
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.is_consumed());
}

#[tokio::test]
async fn email_verification_token_consume_is_idempotent() {
    require_db!(store);

    let user = sample_user("verify3@example.com", "verifyuser3");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let token = sample_email_verification_token(user_id, "verify-token-hash-3");
    let token_id = token.id;
    store.create_email_verification_token(&token).await.unwrap();

    let first_consume_at = Utc::now();
    store
        .consume_email_verification_token(token_id, first_consume_at)
        .await
        .unwrap();

    // 두 번째 소비는 `consumed_at IS NULL` 조건에 걸려 no-op — 최초 소비
    // 시각이 그대로 유지되어야 한다 (재사용 공격 방지).
    store
        .consume_email_verification_token(token_id, Utc::now() + Duration::seconds(30))
        .await
        .unwrap();

    let fetched = store
        .get_email_verification_token("verify-token-hash-3")
        .await
        .unwrap()
        .unwrap();
    let consumed_at = fetched.consumed_at.expect("should be consumed");
    assert!((consumed_at - first_consume_at).num_seconds().abs() < 2);
}

#[tokio::test]
async fn set_user_email_verified_toggle() {
    require_db!(store);

    let user = sample_user("verify4@example.com", "verifyuser4");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    store.set_user_email_verified(user_id, true).await.unwrap();
    let fetched = store.get_user_by_id(user_id).await.unwrap().unwrap();
    assert!(fetched.email_verified);

    store.set_user_email_verified(user_id, false).await.unwrap();
    let fetched = store.get_user_by_id(user_id).await.unwrap().unwrap();
    assert!(!fetched.email_verified);
}

// ═══════════════════════════════════════════════════════════════════════
//  Password reset tokens
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn password_reset_token_create_and_get() {
    require_db!(store);

    let user = sample_user("reset1@example.com", "resetuser1");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let token = sample_password_reset_token(user_id, "reset-token-hash-1");
    store.create_password_reset_token(&token).await.unwrap();

    let fetched = store
        .get_password_reset_token("reset-token-hash-1")
        .await
        .unwrap()
        .expect("token should exist");
    assert_eq!(fetched.user_id, user_id);
    assert!(!fetched.is_expired());
    assert!(!fetched.is_consumed());
}

#[tokio::test]
async fn password_reset_token_consume_sets_consumed_at() {
    require_db!(store);

    let user = sample_user("reset2@example.com", "resetuser2");
    let user_id = user.id;
    store.create_user(&user).await.unwrap();

    let token = sample_password_reset_token(user_id, "reset-token-hash-2");
    let token_id = token.id;
    store.create_password_reset_token(&token).await.unwrap();

    store
        .consume_password_reset_token(token_id, Utc::now())
        .await
        .unwrap();

    let fetched = store
        .get_password_reset_token("reset-token-hash-2")
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.is_consumed());
}

#[tokio::test]
async fn password_reset_token_get_nonexistent_returns_none() {
    require_db!(store);

    let result = store
        .get_password_reset_token("no-such-hash")
        .await
        .unwrap();
    assert!(result.is_none());
}

// ═══════════════════════════════════════════════════════════════════════
//  Login attempts — 기본 CRUD
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn login_attempt_record_and_count_recent_failed_attempts() {
    require_db!(store);

    let now = Utc::now();
    store
        .record_login_attempt(&sample_login_attempt(
            "alice",
            None,
            false,
            Some("bad_password"),
            now,
        ))
        .await
        .unwrap();
    store
        .record_login_attempt(&sample_login_attempt(
            "alice",
            None,
            false,
            Some("bad_password"),
            now,
        ))
        .await
        .unwrap();

    let count = store
        .count_recent_failed_attempts("alice", None, WINDOW_SECS)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn count_recent_failed_attempts_only_counts_failures() {
    require_db!(store);

    let now = Utc::now();
    store
        .record_login_attempt(&sample_login_attempt(
            "bob",
            None,
            false,
            Some("bad_password"),
            now,
        ))
        .await
        .unwrap();
    // 성공한 로그인은 실패 카운트에 포함되면 안 됨.
    store
        .record_login_attempt(&sample_login_attempt("bob", None, true, None, now))
        .await
        .unwrap();

    let count = store
        .count_recent_failed_attempts("bob", None, WINDOW_SECS)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn count_recent_failed_attempts_respects_time_window() {
    require_db!(store);

    let now = Utc::now();
    // 윈도우(60초) 밖의 오래된 실패 — 카운트에서 제외되어야 함 (자연 복구).
    store
        .record_login_attempt(&sample_login_attempt(
            "carol",
            None,
            false,
            Some("bad_password"),
            now - Duration::seconds(WINDOW_SECS + 5),
        ))
        .await
        .unwrap();
    // 윈도우 안의 최근 실패.
    store
        .record_login_attempt(&sample_login_attempt(
            "carol",
            None,
            false,
            Some("bad_password"),
            now,
        ))
        .await
        .unwrap();

    let count = store
        .count_recent_failed_attempts("carol", None, WINDOW_SECS)
        .await
        .unwrap();
    assert_eq!(count, 1, "only the in-window failure should be counted");
}

#[tokio::test]
async fn count_recent_ip_failures_basic() {
    require_db!(store);

    let now = Utc::now();
    store
        .record_login_attempt(&sample_login_attempt(
            "dave",
            Some("10.0.0.1"),
            false,
            Some("bad_password"),
            now,
        ))
        .await
        .unwrap();
    store
        .record_login_attempt(&sample_login_attempt(
            "erin",
            Some("10.0.0.1"),
            false,
            Some("bad_password"),
            now,
        ))
        .await
        .unwrap();

    // IP 카운트는 identifier와 무관하게 합산되어야 함.
    let count = store
        .count_recent_ip_failures("10.0.0.1", WINDOW_SECS)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn clear_login_attempts_removes_matching_rows() {
    require_db!(store);

    let now = Utc::now();
    store
        .record_login_attempt(&sample_login_attempt(
            "frank",
            Some("10.0.0.2"),
            false,
            Some("bad_password"),
            now,
        ))
        .await
        .unwrap();

    let deleted = store
        .clear_login_attempts("frank", Some("10.0.0.2"))
        .await
        .unwrap();
    assert_eq!(deleted, 1);

    let count = store
        .count_recent_failed_attempts("frank", None, WINDOW_SECS)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn delete_old_login_attempts_removes_before_cutoff() {
    require_db!(store);

    let now = Utc::now();
    store
        .record_login_attempt(&sample_login_attempt(
            "grace",
            None,
            false,
            Some("bad_password"),
            now - Duration::days(31),
        ))
        .await
        .unwrap();
    store
        .record_login_attempt(&sample_login_attempt(
            "grace",
            None,
            false,
            Some("bad_password"),
            now,
        ))
        .await
        .unwrap();

    let deleted = store
        .delete_old_login_attempts(now - Duration::days(30))
        .await
        .unwrap();
    assert_eq!(deleted, 1);
}

// ═══════════════════════════════════════════════════════════════════════
//  회귀 테스트 — `count_recent_failed_attempts`의 IS NOT DISTINCT FROM 버그
//
//  이전 SQL은 `ip_address IS NOT DISTINCT FROM $2`만 사용했다. `$2 = NULL`로
//  identifier 단독(모든 IP 합산) 카운트를 요청해도, 이 조건은
//  `ip_address IS NULL`인 행만 매칭시켜 카운트가 구조적으로 항상 0이었다.
//  즉 `/login`의 identifier당 5회 제한이 실제 트래픽에서 전혀 동작하지
//  않았고 (IP당 20회 제한만 우연히 동작), forgot/resend/reset 엔드포인트는
//  이 카운터에 전적으로 의존하므로 완전 무방비였다.
//
//  이 테스트는 coder가 수정한 계약(`$2::text IS NULL OR ip_address IS NOT
//  DISTINCT FROM $2`)을 고정한다 — 회귀 시 반드시 실패해야 한다.
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn count_recent_failed_attempts_ip_scoping_regression() {
    require_db!(store);

    let now = Utc::now();
    let identifier = "victim@example.com";

    // 같은 identifier에 대해 IP A에서 3회, IP B에서 2회 실패 기록.
    for _ in 0..3 {
        store
            .record_login_attempt(&sample_login_attempt(
                identifier,
                Some("192.168.1.10"),
                false,
                Some("bad_password"),
                now,
            ))
            .await
            .unwrap();
    }
    for _ in 0..2 {
        store
            .record_login_attempt(&sample_login_attempt(
                identifier,
                Some("192.168.1.20"),
                false,
                Some("bad_password"),
                now,
            ))
            .await
            .unwrap();
    }

    // ip=None → 모든 IP 합산 (5). 버그가 있었다면 이 값은 0이었다.
    let total = store
        .count_recent_failed_attempts(identifier, None, WINDOW_SECS)
        .await
        .unwrap();
    assert_eq!(
        total, 5,
        "identifier-only count must sum failures across all IPs (regression: IS NOT DISTINCT FROM bug)"
    );

    // ip=Some(A) → IP A로 한정 (3).
    let ip_a_only = store
        .count_recent_failed_attempts(identifier, Some("192.168.1.10"), WINDOW_SECS)
        .await
        .unwrap();
    assert_eq!(
        ip_a_only, 3,
        "IP-scoped count must filter to the given IP only"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Rate limiter 시나리오 회귀 — forgot / resend / reset
//
//  fleet-dashboard의 `check_rate_limit`은 다음 두 store 호출의 얇은
//  래퍼일 뿐이다:
//    - `count_recent_failed_attempts(identifier, None, 60) >= 5`  → 차단
//    - `count_recent_ip_failures(ip, 60) >= 20`                   → 차단
//  실제 handler 로직을 재구현하지 않고, 그 계약이 의존하는 store 동작을
//  각 엔드포인트의 identifier 포맷으로 직접 검증한다.
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rate_limiter_blocks_forgot_password_after_5_requests() {
    require_db!(store);

    let email = "target@example.com";
    let identifier = format!("forgot:{email}");
    let now = Utc::now();

    // forgot/resend는 성공/실패 구분 없이 "통과한 모든 요청"을 실패 행으로
    // 기록한다 (계정 열거 방지) — record_rate_limited_request와 동일하게
    // success=false, reason="forgot_password_request"로 기록.
    for _ in 0..(MAX_FAILED_ATTEMPTS - 1) {
        store
            .record_login_attempt(&sample_login_attempt(
                &identifier,
                Some("172.16.0.1"),
                false,
                Some("forgot_password_request"),
                now,
            ))
            .await
            .unwrap();
    }
    let count_before = store
        .count_recent_failed_attempts(&identifier, None, WINDOW_SECS)
        .await
        .unwrap();
    assert!(
        count_before < MAX_FAILED_ATTEMPTS,
        "should not be blocked yet at {count_before} requests"
    );

    // 5번째 요청 — 임계값 도달.
    store
        .record_login_attempt(&sample_login_attempt(
            &identifier,
            Some("172.16.0.1"),
            false,
            Some("forgot_password_request"),
            now,
        ))
        .await
        .unwrap();

    let count_after = store
        .count_recent_failed_attempts(&identifier, None, WINDOW_SECS)
        .await
        .unwrap();
    assert!(
        count_after >= MAX_FAILED_ATTEMPTS,
        "6th /forgot-password request for the same email must be blocked"
    );
}

#[tokio::test]
async fn rate_limiter_blocks_resend_verification_after_5_requests() {
    require_db!(store);

    let email = "resend-target@example.com";
    let identifier = format!("resend:{email}");
    let now = Utc::now();

    for _ in 0..MAX_FAILED_ATTEMPTS {
        store
            .record_login_attempt(&sample_login_attempt(
                &identifier,
                Some("172.16.0.2"),
                false,
                Some("resend_verification_request"),
                now,
            ))
            .await
            .unwrap();
    }

    let count = store
        .count_recent_failed_attempts(&identifier, None, WINDOW_SECS)
        .await
        .unwrap();
    assert!(
        count >= MAX_FAILED_ATTEMPTS,
        "/resend-verification must block after {MAX_FAILED_ATTEMPTS} requests for the same email"
    );
}

#[tokio::test]
async fn rate_limiter_blocks_reset_password_after_5_requests_same_ip() {
    require_db!(store);

    // reset-password는 폼에 이메일이 없으므로 identifier가 요청 출처 IP다.
    let ip = "203.0.113.7";
    let identifier = format!("reset:{ip}");
    let now = Utc::now();

    let reasons = [
        "reset_token_invalid",
        "reset_token_invalid",
        "reset_token_expired",
        "reset_token_consumed",
        "reset_token_invalid",
    ];
    assert_eq!(reasons.len() as u64, MAX_FAILED_ATTEMPTS);

    for reason in reasons {
        store
            .record_login_attempt(&sample_login_attempt(
                &identifier,
                Some(ip),
                false,
                Some(reason),
                now,
            ))
            .await
            .unwrap();
    }

    let count = store
        .count_recent_failed_attempts(&identifier, None, WINDOW_SECS)
        .await
        .unwrap();
    assert!(
        count >= MAX_FAILED_ATTEMPTS,
        "/reset-password must block after {MAX_FAILED_ATTEMPTS} bad tokens from the same IP"
    );
}

#[tokio::test]
async fn rate_limiter_ip_limit_requires_21_distinct_identifiers() {
    require_db!(store);

    // 동일 IP가 서로 다른 identifier(이메일)로 반복 요청하면 identifier당
    // 카운터(5)에는 걸리지 않지만, IP 단독 카운터(20)는 채워진다 — IP 회전
    // 없이도 동일 IP에서 서로 다른 피해자를 노리는 credential-stuffing류
    // 공격을 잡아내는 경로. 계약대로 IP 제한(20)을 넘기려면 서로 다른
    // identifier가 21개 필요하다 (같은 identifier를 반복하면 그 전에
    // identifier 제한 5에 먼저 걸린다).
    let ip = "198.51.100.9";
    let now = Utc::now();

    for i in 0..21 {
        let identifier = format!("forgot:user{i}@example.com");
        store
            .record_login_attempt(&sample_login_attempt(
                &identifier,
                Some(ip),
                false,
                Some("forgot_password_request"),
                now,
            ))
            .await
            .unwrap();
    }

    let ip_count = store
        .count_recent_ip_failures(ip, WINDOW_SECS)
        .await
        .unwrap();
    assert!(
        ip_count >= MAX_IP_FAILED_ATTEMPTS,
        "21 distinct identifiers from the same IP must trip the IP-wide limit (20)"
    );

    // 각 identifier는 딱 1회뿐이므로 individually 차단되지 않아야 함 — IP
    // 제한이 이 케이스를 잡아내는 유일한 방어선임을 확인.
    let single_identifier_count = store
        .count_recent_failed_attempts("forgot:user0@example.com", None, WINDOW_SECS)
        .await
        .unwrap();
    assert_eq!(single_identifier_count, 1);
}

#[tokio::test]
async fn rate_limiter_window_recovery_after_60s() {
    require_db!(store);

    // "차단된 요청은 기록하지 않는다"는 계약 덕분에, 정확히 5회만 기록된
    // 채로 60초가 지나면 카운터는 자연 복구되어야 한다 (더 이상 요청을
    // 안 보내면 잠금이 무한 연장되지 않음).
    let identifier = "forgot:recovers@example.com";
    let stale = Utc::now() - Duration::seconds(WINDOW_SECS + 1);

    for _ in 0..MAX_FAILED_ATTEMPTS {
        store
            .record_login_attempt(&sample_login_attempt(
                identifier,
                Some("172.16.0.3"),
                false,
                Some("forgot_password_request"),
                stale,
            ))
            .await
            .unwrap();
    }

    let count = store
        .count_recent_failed_attempts(identifier, None, WINDOW_SECS)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "attempts older than the window must not count toward the limit"
    );
}
