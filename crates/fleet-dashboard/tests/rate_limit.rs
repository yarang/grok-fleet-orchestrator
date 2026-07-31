//! 인증 보조 엔드포인트 rate limiting 회귀 테스트.
//!
//! ## 배경 (P0 취약점)
//!
//! `/forgot-password`, `/reset-password`, `/resend-verification`은
//! `check_rate_limit`을 호출했지만 카운터를 증가시키는 `record_login_failure`를
//! **이미 차단된 `if !allowed` 블록 안에서만** 호출했다. 정상 요청은 어떤 행도
//! 남기지 않으므로 카운터가 0에서 올라가지 않고, 차단 분기는 도달 불가능했다.
//! 즉 세 엔드포인트의 rate limiting이 완전히 무동작이었다.
//!
//! `/reset-password`는 추가로 식별자에 `form.token`을 썼는데, 토큰은 시도마다
//! 새로 생성되므로 식별자별 카운터가 구조적으로 항상 0이었다.
//!
//! 아래 테스트는 "N회 이후 실제로 차단되는가"를 HTTP 레벨에서 검증한다.
//! 스토어 레이어(SQL) 쪽 회귀는 `fleet-store/tests/integration.rs`의
//! `login_attempts_*` 테스트가 담당한다 (실 Postgres 필요).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use fleet_core::{
    BootstrapToken, EventEntry, FleetEvent, LoginAttempt, Task, TaskFilter, TaskId, TaskOutput,
    TaskStatus, Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
};
use fleet_dashboard::{build_dashboard_app, DashboardState};
use fleet_store::{Store, StoreError};
use sqlx::postgres::PgPoolOptions;
use tokio::task::JoinHandle;

/// `auth.rs`의 `MAX_FAILED_ATTEMPTS`와 동일해야 한다.
const MAX_FAILED_ATTEMPTS: usize = 5;

// ═══════════════════════════════════════════════════════════════════════
//  login_attempts만 구현한 최소 인메모리 Store
//
//  PgStore와 동일한 의미를 갖도록 구현한다:
//  - ip = None  → 모든 IP 합산
//  - ip = Some  → 해당 IP로 한정
//  주의: 이 구현이 SQL과 일치하는지는 여기서 보장되지 않는다. SQL 자체의
//  회귀는 fleet-store 통합 테스트가 실 DB로 검증한다.
// ═══════════════════════════════════════════════════════════════════════

#[derive(Default)]
struct AttemptStore {
    attempts: Mutex<Vec<LoginAttempt>>,
}

impl AttemptStore {
    fn new() -> Self {
        Self::default()
    }

    /// 특정 identifier로 기록된 실패 건수 (테스트 검증용).
    fn failure_count(&self, identifier: &str) -> usize {
        self.attempts
            .lock()
            .unwrap()
            .iter()
            .filter(|a| a.identifier == identifier && !a.success)
            .count()
    }
}

#[async_trait]
impl Store for AttemptStore {
    // ── rate limiting 관련 (실제 구현) ──────────────────────────────

    async fn record_login_attempt(&self, attempt: &LoginAttempt) -> Result<(), StoreError> {
        self.attempts.lock().unwrap().push(attempt.clone());
        Ok(())
    }

    async fn count_recent_failed_attempts(
        &self,
        identifier: &str,
        ip: Option<&str>,
        window_secs: i64,
    ) -> Result<u64, StoreError> {
        let cutoff = Utc::now() - Duration::seconds(window_secs);
        let n = self
            .attempts
            .lock()
            .unwrap()
            .iter()
            .filter(|a| {
                a.identifier == identifier
                    && !a.success
                    && a.attempted_at >= cutoff
                    // ip = None → 모든 IP 합산. (MSRV 1.75 — `is_none_or` 사용 불가)
                    && match ip {
                        Some(want) => a.ip_address.as_deref() == Some(want),
                        None => true,
                    }
            })
            .count();
        Ok(n as u64)
    }

    async fn count_recent_ip_failures(
        &self,
        ip: &str,
        window_secs: i64,
    ) -> Result<u64, StoreError> {
        let cutoff = Utc::now() - Duration::seconds(window_secs);
        let n = self
            .attempts
            .lock()
            .unwrap()
            .iter()
            .filter(|a| {
                a.ip_address.as_deref() == Some(ip) && !a.success && a.attempted_at >= cutoff
            })
            .count();
        Ok(n as u64)
    }

    async fn clear_login_attempts(
        &self,
        identifier: &str,
        _ip: Option<&str>,
    ) -> Result<u64, StoreError> {
        let mut attempts = self.attempts.lock().unwrap();
        let before = attempts.len();
        attempts.retain(|a| a.identifier != identifier);
        Ok((before - attempts.len()) as u64)
    }

    async fn delete_old_login_attempts(&self, _before: DateTime<Utc>) -> Result<u64, StoreError> {
        Ok(0)
    }

    /// 토큰은 항상 미존재 → `/reset-password`의 "invalid token" 실패 경로 진입.
    async fn get_password_reset_token(
        &self,
        _token_hash: &str,
    ) -> Result<Option<fleet_core::PasswordResetToken>, StoreError> {
        Ok(None)
    }

    // ── 이하 트레이트 필수 메서드 (본 테스트에서 미사용) ──────────────

    async fn insert_task(&self, _task: &Task) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn get_task(&self, _id: TaskId) -> Result<Option<Task>, StoreError> {
        Ok(None)
    }
    async fn update_task_status(
        &self,
        _id: TaskId,
        _status: &TaskStatus,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn list_tasks(&self, _filter: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        Ok(Vec::new())
    }
    async fn upsert_worker(&self, _worker: &Worker) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn get_worker(&self, _id: WorkerId) -> Result<Option<Worker>, StoreError> {
        Ok(None)
    }
    async fn get_worker_by_name(&self, _name: &str) -> Result<Option<Worker>, StoreError> {
        Ok(None)
    }
    async fn list_workers(&self, _filter: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
        Ok(Vec::new())
    }
    async fn delete_worker(&self, _id: WorkerId) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn update_worker_heartbeat(
        &self,
        _id: WorkerId,
        _heartbeat: &WorkerHeartbeat,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn append_event(&self, _event: &FleetEvent) -> Result<u64, StoreError> {
        Ok(0)
    }
    async fn list_events(
        &self,
        _after_seq: u64,
        _limit: u32,
    ) -> Result<Vec<EventEntry>, StoreError> {
        Ok(Vec::new())
    }
    async fn append_output(&self, _task_id: TaskId, _chunk: &str) -> Result<u64, StoreError> {
        Ok(0)
    }
    async fn get_output(
        &self,
        _task_id: TaskId,
        _after_seq: u64,
    ) -> Result<TaskOutput, StoreError> {
        unimplemented!()
    }
    async fn migrate(&self) -> Result<(), StoreError> {
        Ok(())
    }
    async fn create_bootstrap_token(&self, _token: &BootstrapToken) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn consume_bootstrap_token(
        &self,
        _token: &str,
        _used_by: &str,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn list_bootstrap_tokens(&self) -> Result<Vec<BootstrapToken>, StoreError> {
        Ok(Vec::new())
    }
    async fn revoke_bootstrap_token(&self, _token: &str) -> Result<bool, StoreError> {
        Ok(false)
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  테스트 헬퍼
// ═══════════════════════════════════════════════════════════════════════

struct TestServer {
    addr: SocketAddr,
    store: Arc<AttemptStore>,
    _handle: JoinHandle<()>,
}

async fn spawn_server() -> TestServer {
    let store = Arc::new(AttemptStore::new());
    // connect_lazy: 실제 연결 없이 PgPool 핸들만 생성 (SSE 미사용).
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://__test_unused__@localhost/__none__")
        .expect("connect_lazy must not perform I/O");

    let state = Arc::new(DashboardState::new(store.clone() as Arc<dyn Store>, pool));
    let app = build_dashboard_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        // ConnectInfo<SocketAddr> 추출을 위해 connect_info 서비스 사용.
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    TestServer {
        addr,
        store,
        _handle: handle,
    }
}

/// CSRF 더블 서밋: 쿠키 값과 폼 필드 값이 일치하기만 하면 통과한다.
const CSRF: &str = "test-csrf-token";

/// 폼 POST. 반환값은 응답 본문(HTML).
async fn post_form(server: &TestServer, path: &str, fields: &[(&str, &str)]) -> String {
    let client = reqwest::Client::new();
    let mut form: HashMap<&str, &str> = fields.iter().copied().collect();
    form.insert("csrf_token", CSRF);

    let resp = client
        .post(format!("http://{}{}", server.addr, path))
        .header("cookie", format!("fleet_csrf={CSRF}"))
        .form(&form)
        .send()
        .await
        .expect("request must succeed");

    resp.text().await.unwrap()
}

fn is_rate_limited(body: &str) -> bool {
    body.contains("Too many requests")
}

// ═══════════════════════════════════════════════════════════════════════
//  회귀 테스트
// ═══════════════════════════════════════════════════════════════════════

/// `/forgot-password`: 정상 요청이 카운터를 증가시키고 N회 초과 시 차단되어야 한다.
///
/// 수정 전에는 기록이 차단 블록 안에만 있어 카운터가 0에 머물렀고, 무한 요청이
/// 가능했다(이메일 폭탄).
#[tokio::test]
async fn forgot_password_blocks_after_max_attempts() {
    let server = spawn_server().await;
    let email = "victim@example.com";

    // 존재하지 않는 이메일이어도 요청 자체가 카운트되어야 한다
    // (계정 열거 방지로 응답이 항상 동일하므로 "실패 경로"가 없다).
    for i in 0..MAX_FAILED_ATTEMPTS {
        let body = post_form(&server, "/forgot-password", &[("email", email)]).await;
        assert!(
            !is_rate_limited(&body),
            "{}번째 요청은 통과해야 한다",
            i + 1
        );
    }

    // 카운터가 실제로 증가했는지 확인 (핵심 회귀 지점).
    assert_eq!(
        server.store.failure_count(&format!("forgot:{email}")),
        MAX_FAILED_ATTEMPTS,
        "정상 요청이 카운터를 증가시켜야 한다"
    );

    // N회 초과 → 차단.
    let body = post_form(&server, "/forgot-password", &[("email", email)]).await;
    assert!(
        is_rate_limited(&body),
        "{}회 초과 시 차단되어야 한다",
        MAX_FAILED_ATTEMPTS
    );

    // 차단된 요청은 기록하지 않는다 (락아웃 무한 연장 방지).
    assert_eq!(
        server.store.failure_count(&format!("forgot:{email}")),
        MAX_FAILED_ATTEMPTS,
        "차단된 요청은 카운터를 더 늘리지 않아야 한다"
    );
}

/// `/resend-verification`: 동일하게 N회 초과 시 차단되어야 한다.
#[tokio::test]
async fn resend_verification_blocks_after_max_attempts() {
    let server = spawn_server().await;
    let email = "victim2@example.com";

    for i in 0..MAX_FAILED_ATTEMPTS {
        let body = post_form(&server, "/resend-verification", &[("email", email)]).await;
        assert!(
            !is_rate_limited(&body),
            "{}번째 요청은 통과해야 한다",
            i + 1
        );
    }

    assert_eq!(
        server.store.failure_count(&format!("resend:{email}")),
        MAX_FAILED_ATTEMPTS
    );

    let body = post_form(&server, "/resend-verification", &[("email", email)]).await;
    assert!(is_rate_limited(&body), "초과 시 차단되어야 한다");
}

/// `/reset-password`: **매번 다른 토큰**으로 시도해도 차단되어야 한다.
///
/// 수정 전에는 식별자가 `form.token`이라 시도마다 카운터가 새로 시작되어
/// 토큰 브루트포스를 무제한 허용했다. 식별자를 IP 기준으로 바꾼 것이 이 테스트의
/// 검증 대상이다.
#[tokio::test]
async fn reset_password_blocks_token_enumeration_with_rotating_tokens() {
    let server = spawn_server().await;

    for i in 0..MAX_FAILED_ATTEMPTS {
        // 공격자가 매 시도마다 새 토큰을 생성하는 상황을 재현.
        let token = format!("guessed-token-{i}");
        let body = post_form(
            &server,
            "/reset-password",
            &[
                ("token", &token),
                ("password", "Str0ng!Passw0rd"),
                ("password_confirm", "Str0ng!Passw0rd"),
            ],
        )
        .await;
        assert!(
            !is_rate_limited(&body),
            "{}번째 시도는 통과해야 한다",
            i + 1
        );
        assert!(
            body.contains("Invalid or unknown reset token"),
            "미존재 토큰은 invalid 응답이어야 한다"
        );
    }

    // 토큰이 매번 달라도 IP 기준 식별자로 누적되어야 한다.
    let body = post_form(
        &server,
        "/reset-password",
        &[
            ("token", "guessed-token-final"),
            ("password", "Str0ng!Passw0rd"),
            ("password_confirm", "Str0ng!Passw0rd"),
        ],
    )
    .await;
    assert!(
        is_rate_limited(&body),
        "토큰을 바꿔가며 시도해도 {}회 초과 시 차단되어야 한다",
        MAX_FAILED_ATTEMPTS
    );
}

/// 엔드포인트별 카운터는 서로 독립이어야 한다.
///
/// 순수 email을 식별자로 쓰면 `/forgot-password` 스팸이 `/login`의 동일 email
/// 카운터를 소진시켜 피해자를 로그인 불가 상태로 만들 수 있다(교차 엔드포인트
/// 락아웃). 네임스페이스 접두사가 이를 막는다.
#[tokio::test]
async fn forgot_password_spam_does_not_consume_login_counter() {
    let server = spawn_server().await;
    let email = "victim3@example.com";

    for _ in 0..MAX_FAILED_ATTEMPTS {
        post_form(&server, "/forgot-password", &[("email", email)]).await;
    }

    assert_eq!(
        server.store.failure_count(&format!("forgot:{email}")),
        MAX_FAILED_ATTEMPTS
    );
    assert_eq!(
        server.store.failure_count(email),
        0,
        "/login(email) 카운터는 오염되지 않아야 한다"
    );
}

/// CSRF 토큰이 없으면 rate limit 카운터를 소모하지 않아야 한다.
/// (인증 안 된 제3자가 피해자 이메일의 카운터를 소진시키는 것을 방지)
#[tokio::test]
async fn csrf_failure_does_not_consume_counter() {
    let server = spawn_server().await;
    let email = "victim4@example.com";
    let client = reqwest::Client::new();

    for _ in 0..MAX_FAILED_ATTEMPTS + 3 {
        let mut form = HashMap::new();
        form.insert("email", email);
        form.insert("csrf_token", "mismatched-token");
        let _ = client
            .post(format!("http://{}/forgot-password", server.addr))
            .header("cookie", format!("fleet_csrf={CSRF}"))
            .form(&form)
            .send()
            .await
            .unwrap();
    }

    assert_eq!(
        server.store.failure_count(&format!("forgot:{email}")),
        0,
        "CSRF 실패 요청은 카운터를 소모하지 않아야 한다"
    );
}
