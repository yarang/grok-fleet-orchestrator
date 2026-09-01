//! fleet-dashboard HTTP API 통합 테스트.
//!
//! 실제 Postgres 없이 `MemStore`만으로 엔드포인트를 검증합니다.
//! SSE(/api/events/stream)는 PgPool LISTEN/NOTIFY가 필요하므로 본 테스트에서 제외.
//!
//! Phase 9.1 RBAC 도입 후 모든 보호 경로는 `require_session` 미들웨어를 통과합니다.
//! 테스트는 MemStore에 테스트용 사용자 + 세션을 사전 주입하고, 세션 쿠키를
//! 포함하여 요청을 보냅니다.

#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use chrono::{Duration, Utc};
use fleet_core::{
    auth::PermissionKind, Permission, PermissionId, Session, SessionId, Task, TaskId, User, UserId,
    Worker, WorkerId, WorkerStatus,
};
use fleet_dashboard::{build_dashboard_app, DashboardState, SESSION_DURATION_SECS};
use fleet_store::mem::MemStore;
use fleet_store::Store;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use tokio::task::JoinHandle;

// ═══════════════════════════════════════════════════════════════════════
//  세션 시딩 헬퍼 (인메모리 Store는 fleet_store::mem::MemStore 공용 구현 사용)
// ═══════════════════════════════════════════════════════════════════════

/// 테스트 관리자 사용자 + 유효한 세션을 주입하고, 세션 쿠키 raw 값을 반환.
async fn seed_test_session(store: MemStore) -> (MemStore, String) {
    let user = User {
        id: UserId::new(),
        username: "test_admin".into(),
        email: Some("test@example.com".into()),
        email_verified: true,
        password_hash: String::new(),
        enabled: true,
        created_at: Utc::now(),
        last_login_at: None,
    };

    // 쿠키 원문 토큰 (테스트 고정값)
    let raw_token = "test-session-token-for-integration-tests".to_string();
    let hash = sha256_hex(raw_token.as_bytes());

    let session = Session {
        id: SessionId::new(),
        user_id: user.id,
        token_hash: hash,
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(8),
        ip_address: None,
        user_agent: None,
    };

    let uid = user.id;
    store.create_user(&user).await.unwrap();

    // 테스트 관리자에게 모든 권한 부여.
    let all_perms: Vec<Permission> = PermissionKind::all()
        .iter()
        .map(|pk| Permission {
            id: PermissionId::new(),
            name: pk.as_str().to_string(),
            description: None,
        })
        .collect();
    store.seed_permissions(uid, all_perms);

    store.create_session(&session).await.unwrap();

    (store, raw_token)
}

/// `seed_test_session`과 동일하되 권한을 직접 지정한다 — 권한 부족(403) 경로를
/// 테스트할 때 사용.
async fn seed_test_session_with_perms(
    store: MemStore,
    perm_kinds: &[PermissionKind],
) -> (MemStore, String) {
    let user = User {
        id: UserId::new(),
        username: "test_limited".into(),
        email: Some("limited@example.com".into()),
        email_verified: true,
        password_hash: String::new(),
        enabled: true,
        created_at: Utc::now(),
        last_login_at: None,
    };

    let raw_token = "test-limited-session-token".to_string();
    let hash = sha256_hex(raw_token.as_bytes());
    let session = Session {
        id: SessionId::new(),
        user_id: user.id,
        token_hash: hash,
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(8),
        ip_address: None,
        user_agent: None,
    };

    let uid = user.id;
    store.create_user(&user).await.unwrap();

    let perms: Vec<Permission> = perm_kinds
        .iter()
        .map(|pk| Permission {
            id: PermissionId::new(),
            name: pk.as_str().to_string(),
            description: None,
        })
        .collect();
    store.seed_permissions(uid, perms);

    store.create_session(&session).await.unwrap();

    (store, raw_token)
}

/// SHA-256 hex 계산 (auth_util 과 동일 로직, 테스트 격리용).
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

// ═══════════════════════════════════════════════════════════════════════
//  테스트 헬퍼
// ═══════════════════════════════════════════════════════════════════════

struct TestServer {
    addr: SocketAddr,
    _handle: JoinHandle<()>,
}

/// 인증 없이 서버 시작 (public 경로 테스트용).
async fn spawn_server(store: MemStore) -> TestServer {
    spawn_server_inner(store).await
}

/// 테스트 관리자 세션을 주입하고 서버 시작.
/// 반환값: (TestServer, session_cookie_value)
async fn spawn_authed_server(store: MemStore) -> (TestServer, String) {
    let (store, cookie) = seed_test_session(store).await;
    (spawn_server_inner(store).await, cookie)
}

async fn spawn_server_inner(store: MemStore) -> TestServer {
    // connect_lazy: 실제 연결 없이 PgPool 핸들만 생성 (SSE 미사용 테스트용).
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://__test_unused__@localhost/__none__")
        .expect("connect_lazy must not perform I/O");

    let state = Arc::new(DashboardState::new(
        Arc::new(store) as Arc<dyn Store>,
        pool,
        None,
    ));
    let app = build_dashboard_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    TestServer {
        addr,
        _handle: handle,
    }
}

/// `spawn_authed_server`와 동일하지만 `base_path`(예: `"/dashboard"`)를 명시적으로
/// 지정한다. `DashboardState::new`가 읽는 `FLEET_DASHBOARD_BASE_PATH` env를 테스트에서
/// `set_var`로 흔들면 병렬 실행되는 다른 테스트와 경합한다 — 대신 필드를 직접 덮어써
/// 완전히 격리한다.
async fn spawn_authed_server_with_base_path(
    store: MemStore,
    base_path: &str,
) -> (TestServer, String) {
    let (store, cookie) = seed_test_session(store).await;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://__test_unused__@localhost/__none__")
        .expect("connect_lazy must not perform I/O");
    let mut state = DashboardState::new(Arc::new(store) as Arc<dyn Store>, pool, None);
    state.base_path = base_path.to_string();
    let app = build_dashboard_app(Arc::new(state));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (
        TestServer {
            addr,
            _handle: handle,
        },
        cookie,
    )
}

/// 실제 `Dispatcher`(+ `MockTransport`)를 연결한 서버 — `submit_task_api`가
/// dispatcher 부재로 503을 내지 않고 실제 dispatch 경로까지 타야 하는
/// (예: parent_task_id 상속) 테스트 전용. 반환된 `(TestServer, String)`의
/// 문자열은 `session_cookie`.
async fn spawn_server_with_dispatcher(store: MemStore, worker: Worker) -> (TestServer, String) {
    let (store, cookie) = seed_test_session(store).await;
    let store = Arc::new(store) as Arc<dyn Store>;
    store.upsert_worker(&worker).await.unwrap();

    let transport = fleet_transport::MockTransport::new();
    transport
        .add_worker(fleet_transport::MockWorker::new(
            worker.id,
            worker.endpoint.clone(),
        ))
        .await;
    let event_rx = fleet_transport::WorkerTransport::subscribe(&transport)
        .await
        .unwrap();
    let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(transport);

    let fleet_state = Arc::new(fleet_scheduler::FleetState::new(
        store.clone(),
        transport,
        fleet_core::CircuitBreakerConfig::default(),
    ));
    let dispatcher = Arc::new(fleet_scheduler::Dispatcher::new(fleet_state));
    dispatcher.attach_event_receiver(event_rx).await;
    let bg = dispatcher.clone();
    tokio::spawn(async move {
        bg.run_event_loop().await;
    });

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://__test_unused__@localhost/__none__")
        .expect("connect_lazy must not perform I/O");
    let state = Arc::new(DashboardState::new(store, pool, Some(dispatcher)));
    let app = build_dashboard_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (
        TestServer {
            addr,
            _handle: handle,
        },
        cookie,
    )
}

/// 세션 쿠키를 포함한 GET 요청.
fn authed_get(client: &reqwest::Client, url: &str, cookie: &str) -> reqwest::RequestBuilder {
    client
        .get(url)
        .header("cookie", format!("fleet_session={cookie}"))
}

fn sample_worker(name: &str, status: WorkerStatus) -> Worker {
    let id = WorkerId::new();
    Worker {
        id,
        name: name.into(),
        endpoint: format!("https://{name}.example"),
        status,
        labels: HashMap::from([("env".into(), "test".into())]),
        active_tasks: 0,
        max_concurrent: 4,
        max_agent_processes: None,
        circuit_state: fleet_core::CircuitState::Closed,
        last_seen: None,
        worker_version: None,
        liveness_mode: fleet_core::WorkerLivenessMode::Periodic,
        registered_at: chrono::Utc::now(),
        incarnation_started_at: chrono::Utc::now(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  테스트 케이스
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn health_endpoint_returns_ok() {
    // /health 는 public 경로 — 인증 불필요.
    let server = spawn_server(MemStore::new()).await;
    let resp = reqwest::get(format!("http://{}/health", server.addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}

/// 대시보드가 리버스 프록시 prefix(`/dashboard`) 뒤에 마운트된 배포에서, HTML
/// 응답의 `<head>` 바로 뒤에 `<base href="/dashboard/">`가 주입되는지 확인한다.
/// 이게 있어야 페이지 안의 모든 상대경로(`href="login"` 등)가 브라우저에서
/// prefix 포함해 다시 요청된다 — nginx가 `/dashboard/` 뒤에서 prefix 없는
/// 절대경로로 리다이렉트를 받으면 404가 나는 걸 이 메커니즘으로 막는다.
#[tokio::test]
async fn html_response_gets_base_href_injected_for_configured_base_path() {
    let (server, cookie) = spawn_authed_server_with_base_path(MemStore::new(), "/dashboard").await;
    let client = reqwest::Client::new();
    let resp = authed_get(&client, &format!("http://{}/", server.addr), &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains(r#"<base href="/dashboard/">"#),
        "expected injected <base> tag, got: {body}"
    );
    // 원본 <head> 태그 자체는 그대로 남아있어야 한다(치환이 아니라 삽입).
    assert!(body.contains("<head>"));
}

/// base_path가 빈 문자열(루트 마운트)이면 `<base href="/">`가 들어간다 — 상대경로
/// 해석 기준이 항상 origin root로 고정되어, 기존 루트 배포와 동일하게 동작한다.
#[tokio::test]
async fn html_response_gets_root_base_href_when_base_path_unset() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let resp = authed_get(&client, &format!("http://{}/", server.addr), &cookie)
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert!(
        body.contains(r#"<base href="/">"#),
        "expected root <base> tag, got: {body}"
    );
}

/// 위 두 테스트가 지키는 주입은 **바이트 리터럴 `<head>` 6글자**를 찾는다
/// (`app.rs`의 `bytes.windows(6).position(|w| w == b"<head>")`). 그래서
/// `<head lang="en">`이나 `<HEAD>`로 쓴 페이지는 base 태그를 조용히 받지
/// 못하고, 루트 마운트에서는 아무 증상도 없다 — `<base href="/">`가 없어도
/// 상대경로가 어차피 origin root로 풀리기 때문이다. 증상은 리버스 프록시
/// prefix 아래에서만, 그것도 그 페이지에서만 나타난다.
///
/// 위 두 테스트는 각각 **한 페이지**(`/`)만 본다. 그것으로는 새로 추가되는
/// 페이지를 덮을 수 없어서 자산 전체를 훑는 이 단정을 따로 둔다. 개수를 정확히
/// 1로 보는 이유는 두 개일 때 주입이 첫 번째에만 들어가 어느 쪽이 실제
/// `<head>`인지에 따라 결과가 갈리기 때문이다.
#[test]
fn every_html_asset_has_exactly_one_bare_head_tag() {
    use fleet_dashboard::assets::Asset;

    let pages: Vec<String> = Asset::iter()
        .map(|p| p.to_string())
        .filter(|p| p.ends_with(".html"))
        .collect();
    // 바닥 단정 — 자산이 비면 아래 루프가 조용히 통과한다.
    assert!(
        pages.len() >= 15,
        "html 자산이 {}개뿐이다 — 임베드가 비었을 가능성",
        pages.len()
    );

    for path in pages {
        let body = Asset::get(&path).expect("자산 조회");
        let text = String::from_utf8_lossy(body.data.as_ref());
        let count = text.matches("<head>").count();
        assert_eq!(
            count, 1,
            "{path}: `<head>`(정확히 이 6바이트)가 {count}개다 — \
             `inject_base_href`가 base 태그를 넣지 못하거나 엉뚱한 곳에 넣는다"
        );
    }
}

/// `POST /logout`의 redirect Location이 base_path를 포함해야, prefix 뒤에
/// 마운트된 배포에서도 브라우저가 nginx가 실제로 라우팅하는 경로로 이동한다.
#[tokio::test]
async fn logout_redirect_includes_configured_base_path() {
    let (server, cookie) = spawn_authed_server_with_base_path(MemStore::new(), "/dashboard").await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client
        .post(format!("http://{}/logout", server.addr))
        .header(
            "cookie",
            format!("fleet_session={cookie}; fleet_csrf={TEST_CSRF}"),
        )
        .header("x-csrf-token", TEST_CSRF)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 303);
    let location = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(location, "/dashboard/login");
}

#[tokio::test]
async fn index_serves_html() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let resp = authed_get(&client, &format!("http://{}/", server.addr), &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<!DOCTYPE html>") || body.contains("<html"));
    let resp2 = authed_get(&client, &format!("http://{}/", server.addr), &cookie)
        .send()
        .await
        .unwrap();
    let ct = resp2
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(ct.starts_with("text/html"), "unexpected content-type: {ct}");
}

#[tokio::test]
async fn overview_aggregates_counts() {
    let store = MemStore::new()
        .with_worker(sample_worker("w1", WorkerStatus::Online))
        .with_worker(sample_worker("w2", WorkerStatus::Offline))
        .with_worker(sample_worker("w3", WorkerStatus::Degraded));
    let (server, cookie) = spawn_authed_server(store).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!("http://{}/api/overview", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let workers = &body["workers"];
    assert_eq!(workers["total"], 3);
    assert_eq!(workers["online"], 1);
    assert_eq!(workers["offline"], 1);
    assert_eq!(workers["degraded"], 1);
    assert_eq!(workers["circuit_open"], 0);
}

#[tokio::test]
async fn workers_list_returns_summaries() {
    let store = MemStore::new()
        .with_worker(sample_worker("alpha", WorkerStatus::Online))
        .with_worker(sample_worker("beta", WorkerStatus::Offline));
    let (server, cookie) = spawn_authed_server(store).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!("http://{}/api/workers", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let arr: serde_json::Value = resp.json().await.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    let names: Vec<&str> = arr.iter().map(|v| v["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
}

/// `endpoint`의 `server-key=` 값은 워커의 grok ACP 인증 토큰 원문이다 —
/// 대시보드 뷰어 중 그 값을 봐야 하는 사람은 없다(로드맵 #75).
#[tokio::test]
async fn workers_list_never_leaks_raw_server_key() {
    let mut worker = sample_worker("gamma", WorkerStatus::Online);
    worker.endpoint = "wss://gamma.example/ws?server-key=leaked-secret".into();
    let store = MemStore::new().with_worker(worker);
    let (server, cookie) = spawn_authed_server(store).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!("http://{}/api/workers", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        !body.contains("leaked-secret"),
        "response body must not contain the raw server-key: {body}"
    );
    assert!(body.contains("<redacted>"));
}

#[tokio::test]
async fn workers_list_status_filter() {
    let store = MemStore::new()
        .with_worker(sample_worker("online-w", WorkerStatus::Online))
        .with_worker(sample_worker("offline-w", WorkerStatus::Offline));
    let (server, cookie) = spawn_authed_server(store).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!("http://{}/api/workers?status=online", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let arr: serde_json::Value = resp.json().await.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "online-w");
    assert_eq!(arr[0]["status"], "online");
}

#[tokio::test]
async fn tasks_list_returns_array() {
    use fleet_core::TaskRequest;

    let mk_task = |prompt: &str| {
        Task::from_request(TaskRequest {
            prompt: prompt.into(),
            created_by: "tester".into(),
            ..Default::default()
        })
    };

    let store = MemStore::new()
        .with_task(mk_task("hello"))
        .with_task(mk_task("world"));
    let (server, cookie) = spawn_authed_server(store).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let arr: serde_json::Value = resp.json().await.unwrap();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    for t in arr {
        assert_eq!(t["phase"], "pending");
    }
}

#[tokio::test]
async fn events_list_returns_empty_array() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let resp = authed_get(
        &client,
        &format!("http://{}/api/events", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["count"], 0);
    assert!(body["events"].is_array());
}

/// 회귀 테스트: `/static/*`는 세션 없이도 200을 반환해야 한다. 과거 이 라우트가
/// `require_session` 뒤(protected 그룹)에 있어서, 로그인 페이지 자신의
/// `login.css`조차 303(→ /login으로 리다이렉트, 세션 없으므로)을 받아 로그인
/// 화면이 스타일 없이(unstyled) 뜨는 순환 버그가 있었다 — 프로덕션에서 직접
/// 관측.
#[tokio::test]
async fn static_asset_css_served_without_session() {
    let server = spawn_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/static/styles.css", server.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "static assets must be servable pre-auth so /login can load its own CSS"
    );
}

#[tokio::test]
async fn static_asset_css_served() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let resp = authed_get(
        &client,
        &format!("http://{}/static/styles.css", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(ct.contains("css"), "unexpected content-type: {ct}");
    let body = resp.text().await.unwrap();
    assert!(!body.is_empty());
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let (server, _cookie) = spawn_authed_server(MemStore::new()).await;
    let resp = reqwest::get(format!("http://{}/nope", server.addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn protected_route_without_cookie_returns_401() {
    // 인증 없이 보호 경로 접근 시 401 (API 요청으로 식별되도록 Accept 헤더 전송).
    let server = spawn_server(MemStore::new()).await;
    let resp = reqwest::Client::new()
        .get(format!("http://{}/api/overview", server.addr))
        .header("accept", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

// ═══════════════════════════════════════════════════════════════════════
//  Host inventory API 테스트
// ═══════════════════════════════════════════════════════════════════════

fn sample_host(hostname: &str, status: fleet_core::HostStatus) -> fleet_core::Host {
    fleet_core::Host {
        id: uuid::Uuid::new_v4(),
        hostname: hostname.into(),
        worker_id: None,
        status,
        ssh_host: Some(format!("{hostname}.example")),
        ssh_port: 22,
        ssh_user: Some("fleet".into()),
        grok_version: Some("0.2.112".into()),
        fleet_worker_version: Some("0.1.0".into()),
        os_info: None,
        metrics: fleet_core::HostMetrics::default(),
        last_heartbeat_at: None,
        provisioned_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn hosts_list_returns_summaries() {
    let store = MemStore::new()
        .with_host(sample_host("node-a", fleet_core::HostStatus::Online))
        .with_host(sample_host("node-b", fleet_core::HostStatus::Offline))
        .with_host(sample_host("node-c", fleet_core::HostStatus::Provisioned));
    let (server, cookie) = spawn_authed_server(store).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!("http://{}/api/hosts", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    let hostnames: Vec<&str> = arr
        .iter()
        .map(|h| h["hostname"].as_str().unwrap())
        .collect();
    assert!(hostnames.contains(&"node-a"));
    assert!(hostnames.contains(&"node-b"));
    assert!(hostnames.contains(&"node-c"));
}

#[tokio::test]
async fn host_detail_returns_info_and_events() {
    let host = sample_host("web-1", fleet_core::HostStatus::Online);
    let host_id = host.id;

    // host_events에 몇 개 이벤트 추가.
    let store = MemStore::new().with_host(host);
    store
        .append_host_event(&fleet_core::HostEvent {
            id: uuid::Uuid::new_v4(),
            host_id,
            event_type: "heartbeat".into(),
            severity: fleet_core::EventSeverity::Info,
            message: Some("heartbeat received".into()),
            payload: HashMap::new(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    store
        .append_host_event(&fleet_core::HostEvent {
            id: uuid::Uuid::new_v4(),
            host_id,
            event_type: "provision_ok".into(),
            severity: fleet_core::EventSeverity::Info,
            message: Some("provisioned successfully".into()),
            payload: HashMap::new(),
            created_at: chrono::Utc::now() - chrono::Duration::minutes(5),
        })
        .await
        .unwrap();

    let (server, cookie) = spawn_authed_server(store).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!("http://{}/api/hosts/web-1", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    // HostDetail uses #[serde(flatten)] on summary — fields are top-level.
    assert_eq!(body["hostname"], "web-1");
    assert_eq!(body["status"], "online");
    assert_eq!(body["grok_version"], "0.2.112");
    let events = body["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    // 최신순 정렬 확인 — heartbeat가 provision_ok보다 먼저.
    assert_eq!(events[0]["event_type"], "heartbeat");
    assert_eq!(events[1]["event_type"], "provision_ok");
}

#[tokio::test]
async fn host_detail_not_found_returns_404() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!("http://{}/api/hosts/nonexistent", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn list_workers_filtering_and_pagination() {
    let mut w1 = sample_worker("worker-1", fleet_core::WorkerStatus::Online);
    w1.labels.insert("arch".into(), "arm64".into());
    w1.labels.insert("gpu".into(), "true".into());

    let mut w2 = sample_worker("worker-2", fleet_core::WorkerStatus::Online);
    w2.labels.insert("arch".into(), "x86_64".into());
    w2.labels.insert("gpu".into(), "true".into());

    let mut w3 = sample_worker("worker-3", fleet_core::WorkerStatus::Offline);
    w3.labels.insert("arch".into(), "arm64".into());

    let store = MemStore::new()
        .with_worker(w1)
        .with_worker(w2)
        .with_worker(w3);

    let (server, cookie) = spawn_authed_server(store).await;
    let client = reqwest::Client::new();

    // 1. 라벨 필터링 테스트: label_arch=arm64 && label_gpu=true
    let resp = authed_get(
        &client,
        &format!(
            "http://{}/api/workers?label_arch=arm64&label_gpu=true",
            server.addr
        ),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let workers: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0]["name"], "worker-1");

    // 2. 페이지네이션 테스트: limit=1 & offset=1
    let resp = authed_get(
        &client,
        &format!("http://{}/api/workers?limit=1&offset=1", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let workers_paginated: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(workers_paginated.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
//  POST /api/tasks — 대시보드 태스크 제출
// ═══════════════════════════════════════════════════════════════════════
//
// 이 하네스는 `DashboardState.dispatcher`를 항상 `None`으로 구성한다(스토어만
// 검증하는 다른 테스트와의 일관성 유지) — 그래서 여기서는 "요청 검증" 계층
// (권한/CSRF/빈 프롬프트/dispatcher 미구성)만 검증한다. 실제 워커 선택→dispatch
// 성공 경로는 이미 `fleet-scheduler/tests/dispatch_e2e.rs`가 다룬다.

const TEST_CSRF: &str = "test-csrf-token-fixed";

fn authed_post_form(
    client: &reqwest::Client,
    url: &str,
    cookie: &str,
    form: &[(&str, &str)],
) -> reqwest::RequestBuilder {
    client
        .post(url)
        .header(
            "cookie",
            format!("fleet_session={cookie}; fleet_csrf={TEST_CSRF}"),
        )
        .form(form)
}

#[tokio::test]
async fn submit_task_denies_without_task_create_permission() {
    // task:create가 없는 세션(TaskList만 보유) — 403이어야 함.
    let (store, cookie) =
        seed_test_session_with_perms(MemStore::new(), &[PermissionKind::TaskList]).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[("prompt", "do something"), ("csrf_token", TEST_CSRF)],
    )
    .send()
    .await
    .unwrap();

    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn submit_task_rejects_invalid_csrf() {
    let (store, cookie) = seed_test_session(MemStore::new()).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    // 폼의 csrf_token이 쿠키(TEST_CSRF)와 일치하지 않음.
    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[("prompt", "do something"), ("csrf_token", "wrong-token")],
    )
    .send()
    .await
    .unwrap();

    assert_eq!(resp.status(), 403);
}

/// FLEET FIX (2026-08-12) 회귀 테스트 — 실제 프로덕션 장애 재현.
///
/// `fleet_csrf` 쿠키의 수명이 세션 쿠키(8h)보다 짧게(고정 1h) 잡혀 있으면,
/// 세션은 아직 유효한데 CSRF 쿠키만 브라우저에서 만료·삭제된 상태로 폼을
/// 제출하게 된다 — `csrf_valid()`는 쿠키가 아예 없는 `None` 분기를 타
/// "CSRF token invalid"를 반환한다. `submit_task_rejects_invalid_csrf`는
/// 쿠키와 폼 값이 "서로 다른" 경우만 검증해 이 실패 모드를 놓쳤었다.
#[tokio::test]
async fn submit_task_rejects_missing_csrf_cookie() {
    let (store, cookie) = seed_test_session(MemStore::new()).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    // fleet_session만 있고 fleet_csrf 쿠키는 없음 — 만료되어 브라우저가
    // 삭제한 상태를 재현. 폼에는 (이제는 무의미한) 값이 담겨 온다.
    let resp = client
        .post(format!("http://{}/api/tasks", server.addr))
        .header("cookie", format!("fleet_session={cookie}"))
        .form(&[("prompt", "do something"), ("csrf_token", TEST_CSRF)])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 403);
}

/// FLEET FIX (2026-08-12) 회귀 테스트.
///
/// 세션은 유효하지만 `fleet_csrf` 쿠키가 없는 상태(만료·삭제 등)로 아무
/// 보호된 페이지나 열람하면, `require_session` 미들웨어가 응답에 새
/// `fleet_csrf` 쿠키를 발급해야 한다 — 그래야 사용자가 다음 폼을 제출할 때
/// 다시 CSRF 오류를 겪지 않는다. 이 미들웨어 갱신이 없으면(수정 전 상태),
/// `/tasks/new`처럼 쿠키를 건드리지 않는 핸들러에서는 재로그인 전까지
/// 영원히 CSRF 오류에 갇힌다.
#[tokio::test]
async fn authenticated_request_without_csrf_cookie_gets_one_issued() {
    let (store, cookie) = seed_test_session(MemStore::new()).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    // fleet_session만 보내고 fleet_csrf는 아예 없음.
    let resp = authed_get(
        &client,
        &format!("http://{}/api/overview", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);

    let set_cookie_values: Vec<String> = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok().map(String::from))
        .collect();
    let csrf_cookie = set_cookie_values
        .iter()
        .find(|c| c.starts_with("fleet_csrf="))
        .unwrap_or_else(|| panic!("expected a fleet_csrf Set-Cookie, got: {set_cookie_values:?}"));

    assert!(
        csrf_cookie.contains(&format!("Max-Age={SESSION_DURATION_SECS}")),
        "CSRF cookie must live as long as the session, got: {csrf_cookie}"
    );
}

/// 기존 `fleet_csrf` 쿠키가 있는 상태로 요청하면, 미들웨어는 값을 새로
/// 굴리지 않고 그대로 재사용해야 한다 — 이미 화면에 렌더링된(혹은 다른 탭이
/// 들고 있는) 토큰을 이 요청 하나가 무효화시키면 안 된다. 수명만 슬라이딩
/// 갱신된다.
#[tokio::test]
async fn authenticated_request_preserves_existing_csrf_cookie_value() {
    let (store, cookie) = seed_test_session(MemStore::new()).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://{}/api/overview", server.addr))
        .header(
            "cookie",
            format!("fleet_session={cookie}; fleet_csrf={TEST_CSRF}"),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let csrf_cookie = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok().map(String::from))
        .find(|c| c.starts_with("fleet_csrf="))
        .expect("expected a fleet_csrf Set-Cookie");

    assert!(
        csrf_cookie.starts_with(&format!("fleet_csrf={TEST_CSRF};")),
        "existing CSRF value must be preserved, got: {csrf_cookie}"
    );
}

#[tokio::test]
async fn submit_task_rejects_empty_prompt() {
    let (store, cookie) = seed_test_session(MemStore::new()).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[("prompt", "   "), ("csrf_token", TEST_CSRF)],
    )
    .send()
    .await
    .unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn submit_task_rejects_unknown_parent_task_id() {
    let worker = sample_worker("w1", WorkerStatus::Online);
    let (server, cookie) = spawn_server_with_dispatcher(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    let bogus_parent = TaskId::new().to_string();
    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[
            ("prompt", "이어서 해줘"),
            ("parent_task_id", &bogus_parent),
            ("csrf_token", TEST_CSRF),
        ],
    )
    .send()
    .await
    .unwrap();

    assert_eq!(resp.status(), 400);
}

/// FLEET (2026-08-12): 대시보드 "Reply" 기능의 HTTP 레벨 통합 테스트.
///
/// `Task::inherit_from_parent`의 순수 로직은 fleet-core에서, dispatch 시점
/// 문맥 재구성은 fleet-scheduler에서 이미 검증했다 — 여기서는 그 사이,
/// `submit_task_api`가 폼의 `parent_task_id` 문자열을 실제로 파싱해 부모를
/// 조회하고 상속을 호출하는 HTTP 핸들러 레이어 자체를 검증한다.
#[tokio::test]
async fn submit_task_reply_inherits_thread_from_parent() {
    let worker = sample_worker("w1", WorkerStatus::Online);
    let (server, cookie) = spawn_server_with_dispatcher(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    // 1. 부모 태스크 제출.
    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[
            ("prompt", "1부터 5까지 더해줘"),
            ("cwd", "/srv/fleet/workspaces/test"),
            ("csrf_token", TEST_CSRF),
        ],
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let parent_id = body["task_id"].as_str().unwrap().to_string();

    // 2. 완료될 때까지 폴링 (MockTransport는 거의 즉시 완료).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let resp = authed_get(
            &client,
            &format!("http://{}/api/tasks/{parent_id}", server.addr),
            &cookie,
        )
        .send()
        .await
        .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        if body["task"]["phase"] == "completed" {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("parent task did not complete in time: {body:?}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // 3. 이어가기 제출 — parent_task_id로 방금 완료된 태스크를 지정.
    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[
            ("prompt", "거기에 10을 더하면?"),
            ("parent_task_id", &parent_id),
            ("csrf_token", TEST_CSRF),
        ],
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let reply_id = body["task_id"].as_str().unwrap().to_string();

    // 4. reply가 parent와 같은 thread_id, parent_task_id를 갖는지 확인.
    let parent_detail: serde_json::Value = authed_get(
        &client,
        &format!("http://{}/api/tasks/{parent_id}", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let reply_detail: serde_json::Value = authed_get(
        &client,
        &format!("http://{}/api/tasks/{reply_id}", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert_eq!(
        reply_detail["task"]["thread_id"],
        parent_detail["task"]["thread_id"]
    );
    assert_eq!(reply_detail["task"]["parent_task_id"], parent_id);

    // 5. /api/tasks/:id/thread가 두 태스크 모두 시간순으로 반환하는지 확인.
    let thread: serde_json::Value = authed_get(
        &client,
        &format!("http://{}/api/tasks/{reply_id}/thread", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let ids: Vec<&str> = thread["thread"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![parent_id.as_str(), reply_id.as_str()]);
}

#[tokio::test]
async fn submit_task_without_dispatcher_returns_unavailable_and_creates_no_task() {
    // 이 하네스의 DashboardState.dispatcher는 항상 None — "기능 미구성" 경로.
    let (store, cookie) = seed_test_session(MemStore::new()).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[("prompt", "do something useful"), ("csrf_token", TEST_CSRF)],
    )
    .send()
    .await
    .unwrap();

    assert_eq!(resp.status(), 503);

    // dispatcher 미구성 체크는 태스크 생성 이전에 이뤄지므로, 목록에는 아무것도
    // 남지 않아야 한다.
    let resp = authed_get(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    let tasks: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(tasks.is_empty(), "no task should be created: {tasks:?}");
}

// ═══════════════════════════════════════════════════════════════════════
//  Project API (로드맵 #48, 1단계)
// ═══════════════════════════════════════════════════════════════════════

/// 세션 쿠키 + CSRF 쿠키/헤더를 포함한 JSON body POST/DELETE 요청.
/// `logout`(form 없는 endpoint)과 동일한 헤더 CSRF variant — project API는
/// HTML form이 아니라 JSON body를 받는다.
fn authed_json(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    cookie: &str,
) -> reqwest::RequestBuilder {
    client
        .request(method, url)
        .header(
            "cookie",
            format!("fleet_session={cookie}; fleet_csrf={TEST_CSRF}"),
        )
        .header("x-csrf-token", TEST_CSRF)
}

/// `spawn_authed_server`와 동일하지만 삽입에 쓸 `Arc<dyn Store>` 핸들도
/// 함께 반환한다 — project archive 게이트(비종료 Task 존재 여부) 테스트가
/// HTTP API를 거치지 않고 task를 직접 주입해야 한다.
async fn spawn_authed_server_with_store_handle(
    store: MemStore,
) -> (TestServer, String, Arc<dyn Store>) {
    let (store, cookie) = seed_test_session(store).await;
    let store = Arc::new(store) as Arc<dyn Store>;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://__test_unused__@localhost/__none__")
        .expect("connect_lazy must not perform I/O");
    let state = Arc::new(DashboardState::new(store.clone(), pool, None));
    let app = build_dashboard_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (
        TestServer {
            addr,
            _handle: handle,
        },
        cookie,
        store,
    )
}

#[tokio::test]
async fn list_projects_requires_project_read_permission() {
    let (store, cookie) =
        seed_test_session_with_perms(MemStore::new(), &[PermissionKind::DashboardView]).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!("http://{}/api/projects", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn create_project_requires_project_create_permission() {
    let (store, cookie) =
        seed_test_session_with_perms(MemStore::new(), &[PermissionKind::ProjectRead]).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/projects", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"name": "should-not-be-created"}))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn create_and_list_project_roundtrip() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    let create_resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/projects", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"name": "acme-web", "description": "main web app"}))
    .send()
    .await
    .unwrap();
    assert_eq!(create_resp.status(), 200);
    let created: serde_json::Value = create_resp.json().await.unwrap();
    assert_eq!(created["name"], "acme-web");
    assert_eq!(created["description"], "main web app");
    assert_eq!(created["status"], "active");
    assert_eq!(created["created_by"], "test_admin");

    let list_resp = authed_get(
        &client,
        &format!("http://{}/api/projects", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(list_resp.status(), 200);
    let projects: Vec<serde_json::Value> = list_resp.json().await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["id"], created["id"]);

    let detail_resp = authed_get(
        &client,
        &format!(
            "http://{}/api/projects/{}",
            server.addr,
            created["id"].as_str().unwrap()
        ),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(detail_resp.status(), 200);
    let detail: serde_json::Value = detail_resp.json().await.unwrap();
    assert_eq!(detail["id"], created["id"]);
}

#[tokio::test]
async fn create_project_rejects_empty_name() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/projects", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"name": "   "}))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn create_project_without_csrf_header_is_rejected() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    // 쿠키/헤더 CSRF 없이 세션 쿠키만.
    let resp = client
        .post(format!("http://{}/api/projects", server.addr))
        .header("cookie", format!("fleet_session={cookie}"))
        .json(&serde_json::json!({"name": "no-csrf"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn create_project_conflicts_on_duplicate_name() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    for _ in 0..2 {
        let resp = authed_json(
            &client,
            reqwest::Method::POST,
            &format!("http://{}/api/projects", server.addr),
            &cookie,
        )
        .json(&serde_json::json!({"name": "dup-name"}))
        .send()
        .await
        .unwrap();
        let status = resp.status();
        if status != 200 {
            assert_eq!(status, 409);
            return;
        }
    }
    panic!("second create with the same name must not both succeed with 200");
}

#[tokio::test]
async fn get_project_detail_returns_404_for_unknown_id() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!(
            "http://{}/api/projects/{}",
            server.addr,
            fleet_core::ProjectId::new()
        ),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn delete_project_archives_immediately_when_no_active_tasks() {
    let (server, cookie, _store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();

    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/projects", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"name": "empty-project"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    let delete_resp = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!(
            "http://{}/api/projects/{}",
            server.addr,
            created["id"].as_str().unwrap()
        ),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(delete_resp.status(), 200);
    let archived: serde_json::Value = delete_resp.json().await.unwrap();
    assert_eq!(
        archived["status"], "archived",
        "a project with no active tasks must archive immediately"
    );
}

#[tokio::test]
async fn delete_project_stays_draining_when_active_tasks_exist() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();

    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/projects", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"name": "busy-project"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let project_id: fleet_core::ProjectId = created["id"].as_str().unwrap().parse().unwrap();

    let mut task = Task::from_request(fleet_core::TaskRequest {
        prompt: "still running".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    task.project_id = Some(project_id);
    store.insert_task(&task).await.unwrap();

    let delete_resp = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/projects/{}", server.addr, project_id),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(delete_resp.status(), 200);
    let draining: serde_json::Value = delete_resp.json().await.unwrap();
    assert_eq!(
        draining["status"], "draining",
        "a project with a pending task must not archive yet"
    );
    // 상태만으로는 화면이 사유를 지어낼 수밖에 없다 — 무엇이 막았는지도
    // 응답에 실려야 한다.
    assert_eq!(
        draining["archive_blocked_by"],
        serde_json::json!(["tasks"]),
        "the blocker must be reported, and it is the task — no agent exists here"
    );

    // 다시 호출해도(같은 pending task가 아직 남아 있으므로) 여전히 draining —
    // idempotent 재확인.
    let second_delete = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/projects/{}", server.addr, project_id),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(second_delete.status(), 200);
    let still_draining: serde_json::Value = second_delete.json().await.unwrap();
    assert_eq!(still_draining["status"], "draining");

    // task를 종결시키면, 다음 DELETE 호출이 archive까지 진행한다.
    task.status = fleet_core::TaskStatus::Cancelled {
        reason: "test cleanup".into(),
        cancelled_at: Utc::now(),
    };
    store
        .update_task_status(task.id, &task.status)
        .await
        .unwrap();

    let final_delete = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/projects/{}", server.addr, project_id),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(final_delete.status(), 200);
    let archived: serde_json::Value = final_delete.json().await.unwrap();
    assert_eq!(
        archived["status"], "archived",
        "once the referencing task is terminal, a repeat DELETE must finish archiving"
    );
}

#[tokio::test]
async fn delete_project_on_already_archived_project_is_a_harmless_noop() {
    let (server, cookie, _store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();

    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/projects", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"name": "archive-twice"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let url = format!(
        "http://{}/api/projects/{}",
        server.addr,
        created["id"].as_str().unwrap()
    );

    for _ in 0..2 {
        let resp = authed_json(&client, reqwest::Method::DELETE, &url, &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "archived");
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Agent API (로드맵 #49, 1단계)
// ═══════════════════════════════════════════════════════════════════════

/// Project를 만들고 그 id를 돌려준다 — Agent는 반드시 Project 안에서만
/// 생성되므로 모든 Agent 테스트의 전제다.
async fn create_project_via_api(
    client: &reqwest::Client,
    addr: std::net::SocketAddr,
    cookie: &str,
    name: &str,
) -> String {
    let created: serde_json::Value = authed_json(
        client,
        reqwest::Method::POST,
        &format!("http://{addr}/api/projects"),
        cookie,
    )
    .json(&serde_json::json!({ "name": name }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    created["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn list_agents_requires_agent_read_permission() {
    let (store, cookie) =
        seed_test_session_with_perms(MemStore::new(), &[PermissionKind::DashboardView]).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn create_agent_requires_agent_manage_permission() {
    // `agent:read`만으로는 만들 수 없다 — 읽기와 생성·회수를 다른
    // capability로 나눈 것을 표면에서 확인한다.
    let (store, cookie) = seed_test_session_with_perms(
        MemStore::new(),
        &[PermissionKind::ProjectRead, PermissionKind::AgentRead],
    )
    .await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({
        "project_id": fleet_core::ProjectId::new().to_string(),
        "name": "should-not-be-created",
    }))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn create_list_and_stop_agent_roundtrip() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "agent-home").await;

    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({
        "project_id": project_id,
        "name": "builder",
        "description": "builds things",
    }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(created["name"], "builder");
    assert_eq!(created["project_id"], project_id);
    assert_eq!(created["status"], "ready");
    let agent_id = created["id"].as_str().unwrap().to_string();

    let listed: Vec<serde_json::Value> = authed_get(
        &client,
        &format!(
            "http://{}/api/agents?project_id={}",
            server.addr, project_id
        ),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], agent_id);

    let stopped: serde_json::Value = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/agents/{}", server.addr, agent_id),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(stopped["status"], "stopped");
}

#[tokio::test]
async fn create_agent_without_csrf_header_is_rejected() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "csrf-guard").await;

    let resp = client
        .post(format!("http://{}/api/agents", server.addr))
        .header("cookie", format!("fleet_session={cookie}"))
        .json(&serde_json::json!({"project_id": project_id, "name": "no-csrf"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn create_agent_rejects_unknown_project() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({
        "project_id": fleet_core::ProjectId::new().to_string(),
        "name": "orphan",
    }))
    .send()
    .await
    .unwrap();
    // `project_id`는 불변이라 생성 시점이 이 값을 검증할 수 있는 유일한
    // 순간이다 — 통과시키면 되돌릴 방법이 없다.
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn create_agent_rejects_archived_project() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "closed-shop").await;

    authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/projects/{}", server.addr, project_id),
        &cookie,
    )
    .send()
    .await
    .unwrap();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "too-late"}))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn create_agent_conflicts_on_duplicate_name_within_a_project() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let a = create_project_via_api(&client, server.addr, &cookie, "dup-a").await;
    let b = create_project_via_api(&client, server.addr, &cookie, "dup-b").await;

    let post = |project_id: String| {
        authed_json(
            &client,
            reqwest::Method::POST,
            &format!("http://{}/api/agents", server.addr),
            &cookie,
        )
        .json(&serde_json::json!({"project_id": project_id, "name": "worker"}))
    };

    assert_eq!(post(a.clone()).send().await.unwrap().status(), 200);
    assert_eq!(
        post(a).send().await.unwrap().status(),
        409,
        "같은 Project 안의 중복 이름은 409여야 한다"
    );
    assert_eq!(
        post(b).send().await.unwrap().status(),
        200,
        "이름 유일성은 Project 범위다 — 다른 Project에서는 허용된다"
    );
}

#[tokio::test]
async fn stop_agent_is_idempotent() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "idempotent").await;

    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "once"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let url = format!(
        "http://{}/api/agents/{}",
        server.addr,
        created["id"].as_str().unwrap()
    );

    let mut seen: Option<String> = None;
    for _ in 0..2 {
        let body: serde_json::Value = authed_json(&client, reqwest::Method::DELETE, &url, &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(body["status"], "stopped");
        let updated_at = body["updated_at"].as_str().unwrap().to_string();
        // 재호출이 `updated_at`을 밀면 "언제 회수됐는가"가 호출 횟수만큼
        // 뒤로 이동한다.
        if let Some(first) = &seen {
            assert_eq!(first, &updated_at, "재호출은 회수 시각을 갱신하지 않는다");
        }
        seen = Some(updated_at);
    }
}

#[tokio::test]
async fn stop_agent_returns_404_for_unknown_id() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    let resp = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!(
            "http://{}/api/agents/{}",
            server.addr,
            fleet_core::AgentId::new()
        ),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn a_ready_agent_keeps_the_project_draining() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "held-open").await;

    // Task는 하나도 만들지 않는다 — Project를 draining에 붙잡는 것이 오직
    // Agent 행뿐임을 이 표면에서도 증명한다.
    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "holder"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let agent_url = format!(
        "http://{}/api/agents/{}",
        server.addr,
        created["id"].as_str().unwrap()
    );
    let project_url = format!("http://{}/api/projects/{}", server.addr, project_id);

    let body: serde_json::Value =
        authed_json(&client, reqwest::Method::DELETE, &project_url, &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(
        body["status"], "draining",
        "Ready Agent가 남아 있으면 archive가 완료되면 안 된다"
    );
    // 이 단정이 결함을 잡는 지점이다: Task가 0건인데 사유가 "tasks"로
    // 나오면 화면은 없는 Task를 기다리라고 안내한다(2026-08-28 실제 발생).
    assert_eq!(
        body["archive_blocked_by"],
        serde_json::json!(["agents"]),
        "Task는 하나도 없으므로 사유는 Agent여야 한다"
    );

    authed_json(&client, reqwest::Method::DELETE, &agent_url, &cookie)
        .send()
        .await
        .unwrap();

    let body: serde_json::Value =
        authed_json(&client, reqwest::Method::DELETE, &project_url, &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(body["status"], "archived");
}

// ═══════════════════════════════════════════════════════════════════════
//  Task 제출의 project_id 검증 (로드맵 #48, 2단계)
// ═══════════════════════════════════════════════════════════════════════

/// `spawn_server_with_dispatcher`와 동일하되 store 핸들도 함께 반환한다 —
/// project를 시딩하고 제출된 task의 project_id를 되읽어야 하는 테스트용.
async fn spawn_dispatcher_server_with_store(
    store: MemStore,
    worker: Worker,
) -> (TestServer, String, Arc<dyn Store>) {
    let (store, cookie) = seed_test_session(store).await;
    let store = Arc::new(store) as Arc<dyn Store>;
    store.upsert_worker(&worker).await.unwrap();

    let transport = fleet_transport::MockTransport::new();
    transport
        .add_worker(fleet_transport::MockWorker::new(
            worker.id,
            worker.endpoint.clone(),
        ))
        .await;
    let event_rx = fleet_transport::WorkerTransport::subscribe(&transport)
        .await
        .unwrap();
    let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(transport);

    let fleet_state = Arc::new(fleet_scheduler::FleetState::new(
        store.clone(),
        transport,
        fleet_core::CircuitBreakerConfig::default(),
    ));
    let dispatcher = Arc::new(fleet_scheduler::Dispatcher::new(fleet_state));
    dispatcher.attach_event_receiver(event_rx).await;
    let bg = dispatcher.clone();
    tokio::spawn(async move {
        bg.run_event_loop().await;
    });

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://__test_unused__@localhost/__none__")
        .expect("connect_lazy must not perform I/O");
    let state = Arc::new(DashboardState::new(store.clone(), pool, Some(dispatcher)));
    let app = build_dashboard_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (
        TestServer {
            addr,
            _handle: handle,
        },
        cookie,
        store,
    )
}

#[tokio::test]
async fn submit_task_with_active_project_id_records_it_on_the_task() {
    let worker = sample_worker("w1", WorkerStatus::Online);
    let (server, cookie, store) = spawn_dispatcher_server_with_store(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    let project = fleet_core::Project::new("acme-web");
    store.create_project(&project).await.unwrap();

    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[
            ("prompt", "scoped work"),
            ("cwd", "/srv/fleet/workspaces/test"),
            ("project_id", &project.id.to_string()),
            ("csrf_token", TEST_CSRF),
        ],
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id: fleet_core::TaskId = body["task_id"].as_str().unwrap().parse().unwrap();

    let stored = store.get_task(task_id).await.unwrap().unwrap();
    assert_eq!(stored.project_id, Some(project.id));

    // 응답 스키마로도 되읽을 수 있어야 한다 — 대시보드가 project_id를 설정할
    // 수 있게 됐으니 보여줄 수도 있어야 한다.
    let detail = authed_get(
        &client,
        &format!("http://{}/api/tasks/{task_id}", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json::<serde_json::Value>()
    .await
    .unwrap();
    assert_eq!(detail["task"]["project_id"], project.id.to_string());
}

#[tokio::test]
async fn submit_task_with_unknown_project_id_is_rejected_and_creates_no_task() {
    let worker = sample_worker("w1", WorkerStatus::Online);
    let (server, cookie, store) = spawn_dispatcher_server_with_store(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[
            ("prompt", "orphan work"),
            ("project_id", &fleet_core::ProjectId::new().to_string()),
            ("csrf_token", TEST_CSRF),
        ],
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 400);

    // 검증이 dispatch 이전이므로 태스크 행 자체가 만들어지면 안 된다.
    let tasks = store
        .list_tasks(&fleet_core::TaskFilter::default())
        .await
        .unwrap();
    assert!(tasks.is_empty(), "no task should be created: {tasks:?}");
}

#[tokio::test]
async fn submit_task_with_archived_project_id_is_rejected() {
    let worker = sample_worker("w1", WorkerStatus::Online);
    let (server, cookie, store) = spawn_dispatcher_server_with_store(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    let mut project = fleet_core::Project::new("closed-shop");
    project.status = fleet_core::ProjectStatus::Archived;
    store.create_project(&project).await.unwrap();

    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[
            ("prompt", "too late"),
            ("project_id", &project.id.to_string()),
            ("csrf_token", TEST_CSRF),
        ],
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn submit_task_with_malformed_project_id_is_rejected() {
    let worker = sample_worker("w1", WorkerStatus::Online);
    let (server, cookie, _store) =
        spawn_dispatcher_server_with_store(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[
            ("prompt", "bad id"),
            ("project_id", "not-a-uuid"),
            ("csrf_token", TEST_CSRF),
        ],
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn reply_inherits_project_from_parent_task() {
    // 로드맵 #48 2단계 — 이어가기는 같은 작업 흐름이므로 Project 경계도
    // 물려받는다. 폼에 project_id를 다시 넣지 않아도 유지돼야 한다.
    let worker = sample_worker("w1", WorkerStatus::Online);
    let (server, cookie, store) = spawn_dispatcher_server_with_store(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    let project = fleet_core::Project::new("continuing");
    store.create_project(&project).await.unwrap();

    // 부모: project_id를 명시해 제출한다. 직접 store에 넣지 않고 API를
    // 거치는 이유는, 이어가기 경로가 실제 저장된 부모를 조회해 상속하기
    // 때문이다.
    let mut parent = fleet_core::Task::from_request(fleet_core::TaskRequest {
        prompt: "parent".into(),
        created_by: "test_admin".into(),
        project_id: Some(project.id),
        // 로드맵 #69 — 이어가기는 부모의 `cwd`를 물려받는다. 부모에 없으면
        // 상속 결과도 비어 이어가기가 `cwd` 게이트에서 400으로 거절되고,
        // 그러면 이 테스트가 재려던 판정(project 경계)이 아니라 엉뚱한
        // 이유로 초록/빨강이 갈린다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    parent.status = fleet_core::TaskStatus::Cancelled {
        reason: "seeded terminal".into(),
        cancelled_at: Utc::now(),
    };
    store.insert_task(&parent).await.unwrap();

    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[
            ("prompt", "이어서 해줘"),
            ("parent_task_id", &parent.id.to_string()),
            ("csrf_token", TEST_CSRF),
        ],
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let reply_id: fleet_core::TaskId = body["task_id"].as_str().unwrap().parse().unwrap();

    let reply = store.get_task(reply_id).await.unwrap().unwrap();
    assert_eq!(
        reply.project_id,
        Some(project.id),
        "a reply must stay inside the parent's project"
    );
}

#[tokio::test]
async fn reply_is_rejected_when_the_parents_project_has_since_been_archived() {
    // 상속된 project_id도 명시 입력과 똑같이 검증 대상이다 — 닫힌 Project는
    // 이어가기라 해도 새 Task를 받지 않는다.
    let worker = sample_worker("w1", WorkerStatus::Online);
    let (server, cookie, store) = spawn_dispatcher_server_with_store(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    let mut project = fleet_core::Project::new("archived-mid-thread");
    store.create_project(&project).await.unwrap();

    let mut parent = fleet_core::Task::from_request(fleet_core::TaskRequest {
        prompt: "parent".into(),
        created_by: "test_admin".into(),
        project_id: Some(project.id),
        // 로드맵 #69 — 이어가기는 부모의 `cwd`를 물려받는다. 부모에 없으면
        // 상속 결과도 비어 이어가기가 `cwd` 게이트에서 400으로 거절되고,
        // 그러면 이 테스트가 재려던 판정(project 경계)이 아니라 엉뚱한
        // 이유로 초록/빨강이 갈린다.
        cwd: Some("/srv/fleet/workspaces/test".into()),
        ..Default::default()
    });
    parent.status = fleet_core::TaskStatus::Cancelled {
        reason: "seeded terminal".into(),
        cancelled_at: Utc::now(),
    };
    store.insert_task(&parent).await.unwrap();

    // 부모 제출 이후 Project가 archive됐다.
    project.status = fleet_core::ProjectStatus::Archived;
    store
        .update_project_status(project.id, fleet_core::ProjectStatus::Archived)
        .await
        .unwrap();

    let resp = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[
            ("prompt", "이어서 해줘"),
            ("parent_task_id", &parent.id.to_string()),
            ("csrf_token", TEST_CSRF),
        ],
    )
    .send()
    .await
    .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "continuing into an archived project must be refused"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Issue API (로드맵 #92, Issue 표면)
// ═══════════════════════════════════════════════════════════════════════

/// Issue 테스트용 서버 — store 핸들과 시딩된 Project를 함께 돌려준다.
async fn spawn_issue_server(
    perms: Option<&[PermissionKind]>,
) -> (TestServer, String, Arc<dyn Store>, fleet_core::Project) {
    let (store, cookie) = match perms {
        Some(p) => seed_test_session_with_perms(MemStore::new(), p).await,
        None => seed_test_session(MemStore::new()).await,
    };
    let store = Arc::new(store) as Arc<dyn Store>;
    let project = fleet_core::Project::new("issue-project");
    store.create_project(&project).await.unwrap();

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://__test_unused__@localhost/__none__")
        .expect("connect_lazy must not perform I/O");
    let state = Arc::new(DashboardState::new(store.clone(), pool, None));
    let app = build_dashboard_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (
        TestServer {
            addr,
            _handle: handle,
        },
        cookie,
        store,
        project,
    )
}

/// 목표 상태별 요구 capability가 계약과 일치하는지 — 이 표면의 보안 핵심.
#[test]
fn transition_capability_mapping_matches_the_contract() {
    use fleet_core::IssueStatus::*;
    use fleet_dashboard::required_capability_for_transition as cap;

    // Agent 자동 착수의 유일한 인가 지점.
    assert_eq!(
        cap(ReadyForAgent),
        PermissionKind::IssueApproveAgentWork,
        "promoting to ReadyForAgent is the agent-work authorization point"
    );
    // 승인 철회는 approve 권한을 요구하지 않는다 — 권한 회수가 부여보다
    // 어려우면 잘못된 승인을 되돌리기가 더 힘들어진다.
    assert_eq!(cap(Triaged), PermissionKind::IssueUpdate);
    assert_ne!(cap(Triaged), PermissionKind::IssueApproveAgentWork);
    // "문제가 처리됐다"는 판정은 둘 다 close 권한.
    assert_eq!(cap(Resolved), PermissionKind::IssueClose);
    assert_eq!(cap(Closed), PermissionKind::IssueClose);
    // 텍스트 편집 권한으로 문제를 종결할 수 없어야 한다.
    assert_ne!(cap(Resolved), PermissionKind::IssueUpdate);
    assert_ne!(cap(Closed), PermissionKind::IssueUpdate);
    assert_eq!(cap(Open), PermissionKind::IssueReopen);
}

#[tokio::test]
async fn create_and_get_issue_roundtrip() {
    let (server, cookie, _store, project) = spawn_issue_server(None).await;
    let client = reqwest::Client::new();

    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/issues", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({
        "project_id": project.id.to_string(),
        "title": "login is broken",
        "body": "steps: ...",
        "severity": "high",
        "labels": ["bug", "auth"],
    }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert_eq!(created["title"], "login is broken");
    assert_eq!(created["status"], "open");
    assert_eq!(created["severity"], "high");
    assert_eq!(created["created_by"], "test_admin");
    assert_eq!(created["has_active_tasks"], false);
    assert!(created.get("close_reason").is_none());

    let fetched: serde_json::Value = authed_get(
        &client,
        &format!(
            "http://{}/api/issues/{}",
            server.addr,
            created["id"].as_str().unwrap()
        ),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(fetched["id"], created["id"]);

    let listed: Vec<serde_json::Value> = authed_get(
        &client,
        &format!("http://{}/api/issues", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(listed.len(), 1);
}

#[tokio::test]
async fn create_issue_rejects_unknown_project() {
    let (server, cookie, _store, _project) = spawn_issue_server(None).await;
    let client = reqwest::Client::new();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/issues", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({
        "project_id": fleet_core::ProjectId::new().to_string(),
        "title": "orphan",
    }))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn create_issue_requires_issue_create_permission() {
    let (server, cookie, _store, project) =
        spawn_issue_server(Some(&[PermissionKind::IssueRead])).await;
    let client = reqwest::Client::new();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/issues", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({
        "project_id": project.id.to_string(),
        "title": "should not be created",
    }))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 403);
}

/// 헬퍼: Issue 하나를 만들고 id를 돌려준다.
async fn make_issue(
    client: &reqwest::Client,
    server: &TestServer,
    cookie: &str,
    project_id: &str,
    title: &str,
) -> String {
    let created: serde_json::Value = authed_json(
        client,
        reqwest::Method::POST,
        &format!("http://{}/api/issues", server.addr),
        cookie,
    )
    .json(&serde_json::json!({ "project_id": project_id, "title": title }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    created["id"].as_str().unwrap().to_string()
}

async fn transition(
    client: &reqwest::Client,
    server: &TestServer,
    cookie: &str,
    issue_id: &str,
    status: &str,
    close_reason: Option<&str>,
) -> reqwest::Response {
    let mut body = serde_json::json!({ "status": status });
    if let Some(r) = close_reason {
        body["close_reason"] = serde_json::json!(r);
    }
    authed_json(
        client,
        reqwest::Method::POST,
        &format!("http://{}/api/issues/{issue_id}/transition", server.addr),
        cookie,
    )
    .json(&body)
    .send()
    .await
    .unwrap()
}

#[tokio::test]
async fn full_transition_lifecycle_through_the_api() {
    let (server, cookie, _store, project) = spawn_issue_server(None).await;
    let client = reqwest::Client::new();
    let id = make_issue(
        &client,
        &server,
        &cookie,
        &project.id.to_string(),
        "lifecycle",
    )
    .await;

    for (status, reason, expect_status) in [
        ("triaged", None, "triaged"),
        ("ready_for_agent", None, "ready_for_agent"),
        ("resolved", None, "resolved"),
    ] {
        let resp = transition(&client, &server, &cookie, &id, status, reason).await;
        assert_eq!(resp.status(), 200, "transition to {status}");
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], expect_status);
    }

    let resp = transition(&client, &server, &cookie, &id, "closed", Some("fixed")).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "closed");
    assert_eq!(body["close_reason"], "fixed");

    // reopen — close_reason이 사라져야 한다.
    let resp = transition(&client, &server, &cookie, &id, "open", None).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "open");
    assert!(body.get("close_reason").is_none());
}

#[tokio::test]
async fn disallowed_transition_returns_409() {
    let (server, cookie, _store, project) = spawn_issue_server(None).await;
    let client = reqwest::Client::new();
    let id = make_issue(
        &client,
        &server,
        &cookie,
        &project.id.to_string(),
        "no shortcut",
    )
    .await;

    // Open -> ReadyForAgent 간선은 없다(사람의 triage를 반드시 거친다).
    let resp = transition(&client, &server, &cookie, &id, "ready_for_agent", None).await;
    assert_eq!(resp.status(), 409);
}

#[tokio::test]
async fn closing_without_a_reason_returns_409() {
    let (server, cookie, _store, project) = spawn_issue_server(None).await;
    let client = reqwest::Client::new();
    let id = make_issue(
        &client,
        &server,
        &cookie,
        &project.id.to_string(),
        "needs reason",
    )
    .await;

    let resp = transition(&client, &server, &cookie, &id, "closed", None).await;
    assert_eq!(resp.status(), 409);
}

#[tokio::test]
async fn promoting_to_ready_for_agent_requires_the_approval_capability() {
    // 계약의 핵심 — issue:update만 가진 사람은 Agent 착수를 승인할 수 없다.
    let (server, cookie, store, project) = spawn_issue_server(Some(&[
        PermissionKind::IssueRead,
        PermissionKind::IssueCreate,
        PermissionKind::IssueUpdate,
    ]))
    .await;
    let client = reqwest::Client::new();
    let id = make_issue(&client, &server, &cookie, &project.id.to_string(), "gated").await;

    // triage는 issue:update로 가능하다.
    let resp = transition(&client, &server, &cookie, &id, "triaged", None).await;
    assert_eq!(resp.status(), 200);

    // 승인은 막힌다.
    let resp = transition(&client, &server, &cookie, &id, "ready_for_agent", None).await;
    assert_eq!(
        resp.status(),
        403,
        "issue:update alone must not authorize agent pickup"
    );

    // 저장된 상태도 그대로여야 한다.
    let issue_id: fleet_core::IssueId = id.parse().unwrap();
    assert_eq!(
        store.get_issue(issue_id).await.unwrap().unwrap().status,
        fleet_core::IssueStatus::Triaged
    );
}

#[tokio::test]
async fn withdrawing_approval_does_not_require_the_approval_capability() {
    // 권한 회수가 부여보다 어려우면 잘못된 승인을 되돌리기가 더 힘들어진다.
    let (server, cookie, _store, project) = spawn_issue_server(None).await;
    let client = reqwest::Client::new();
    let id = make_issue(
        &client,
        &server,
        &cookie,
        &project.id.to_string(),
        "withdraw",
    )
    .await;
    transition(&client, &server, &cookie, &id, "triaged", None).await;
    transition(&client, &server, &cookie, &id, "ready_for_agent", None).await;

    // approve 권한이 없는 별도 세션으로는 만들 수 없으므로, 매핑 자체로
    // 확인한다(위 단위 테스트와 짝) — 여기서는 전이가 실제로 동작하는지만.
    let resp = transition(&client, &server, &cookie, &id, "triaged", None).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "triaged");
}

#[tokio::test]
async fn closing_requires_close_capability_not_update() {
    let (server, cookie, _store, project) = spawn_issue_server(Some(&[
        PermissionKind::IssueRead,
        PermissionKind::IssueCreate,
        PermissionKind::IssueUpdate,
    ]))
    .await;
    let client = reqwest::Client::new();
    let id = make_issue(
        &client,
        &server,
        &cookie,
        &project.id.to_string(),
        "cannot close",
    )
    .await;

    let resp = transition(&client, &server, &cookie, &id, "closed", Some("wont_fix")).await;
    assert_eq!(
        resp.status(),
        403,
        "a typo-fixer must not be able to close a problem"
    );
}

#[tokio::test]
async fn patch_updates_fields_but_cannot_change_status() {
    let (server, cookie, store, project) = spawn_issue_server(None).await;
    let client = reqwest::Client::new();
    let id = make_issue(
        &client,
        &server,
        &cookie,
        &project.id.to_string(),
        "typo ttile",
    )
    .await;
    transition(&client, &server, &cookie, &id, "triaged", None).await;

    // status 필드를 본문에 끼워 넣어도 무시돼야 한다(스키마에 아예 없다).
    let resp = authed_json(
        &client,
        reqwest::Method::PATCH,
        &format!("http://{}/api/issues/{id}", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({
        "title": "typo title",
        "severity": "critical",
        "status": "closed",
        "close_reason": "fixed",
    }))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "typo title");
    assert_eq!(body["severity"], "critical");
    assert_eq!(
        body["status"], "triaged",
        "PATCH must not be able to change status"
    );

    let issue_id: fleet_core::IssueId = id.parse().unwrap();
    let stored = store.get_issue(issue_id).await.unwrap().unwrap();
    assert_eq!(stored.status, fleet_core::IssueStatus::Triaged);
    assert_eq!(stored.close_reason, None);
}

#[tokio::test]
async fn changing_assignee_requires_the_assign_capability() {
    let (server, cookie, _store, project) = spawn_issue_server(Some(&[
        PermissionKind::IssueRead,
        PermissionKind::IssueCreate,
        PermissionKind::IssueUpdate,
    ]))
    .await;
    let client = reqwest::Client::new();
    let id = make_issue(
        &client,
        &server,
        &cookie,
        &project.id.to_string(),
        "assign me",
    )
    .await;

    // title만 바꾸는 건 통과.
    let resp = authed_json(
        &client,
        reqwest::Method::PATCH,
        &format!("http://{}/api/issues/{id}", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({ "title": "renamed" }))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);

    // assignee를 건드리면 403.
    let resp = authed_json(
        &client,
        reqwest::Method::PATCH,
        &format!("http://{}/api/issues/{id}", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({ "assignee": "bob" }))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn comments_round_trip() {
    let (server, cookie, _store, project) = spawn_issue_server(None).await;
    let client = reqwest::Client::new();
    let id = make_issue(
        &client,
        &server,
        &cookie,
        &project.id.to_string(),
        "discuss",
    )
    .await;

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/issues/{id}/comments", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({ "body": "first thought" }))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);

    let comments: Vec<serde_json::Value> = authed_get(
        &client,
        &format!("http://{}/api/issues/{id}/comments", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0]["body"], "first thought");
    assert_eq!(comments[0]["author"], "test_admin");
}

#[tokio::test]
async fn linking_a_task_surfaces_the_derived_in_progress_badge() {
    // "진행 중"은 파생 값이다 — Issue status는 그대로 두고
    // has_active_tasks만 바뀐다.
    let (server, cookie, store, project) = spawn_issue_server(None).await;
    let client = reqwest::Client::new();
    let id = make_issue(
        &client,
        &server,
        &cookie,
        &project.id.to_string(),
        "has work",
    )
    .await;

    let task = fleet_core::Task::from_request(fleet_core::TaskRequest {
        prompt: "do the work".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    store.insert_task(&task).await.unwrap();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/issues/{id}/links", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({ "task_id": task.id.to_string() }))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.json::<serde_json::Value>().await.unwrap()["created"],
        true
    );

    let fetched: serde_json::Value = authed_get(
        &client,
        &format!("http://{}/api/issues/{id}", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(fetched["has_active_tasks"], true);
    assert_eq!(
        fetched["status"], "open",
        "a linked in-flight task must not change the stored status"
    );

    let links: Vec<serde_json::Value> = authed_get(
        &client,
        &format!("http://{}/api/issues/{id}/links", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["task_label"], "do the work");

    // 해제.
    let resp = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/issues/{id}/links/{}", server.addr, task.id),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.json::<serde_json::Value>().await.unwrap()["removed"],
        true
    );
}

/// `#58`: `issue:link`는 호출자의 Project 범위를 검사하지 않으므로, 다른
/// Project 소속 Task를 링크로 끌어올 수 있으면 그 Task의 존재·label을
/// 노출하는 confused-deputy가 열린다. 이 시험은 그 경로가 막혀 있는지,
/// 그리고 거절 응답이 "Task 없음"과 구분되지 않는지(존재 은닉) 확인한다.
#[tokio::test]
async fn linking_a_task_from_another_project_is_rejected_like_a_missing_task() {
    let (server, cookie, store, project) = spawn_issue_server(None).await;
    let client = reqwest::Client::new();
    let id = make_issue(&client, &server, &cookie, &project.id.to_string(), "scoped").await;

    // 같은 task_id를 두 세계 상태(존재하지 않음 → 다른 Project에 존재함)에
    // 재사용한다. 호출자가 어차피 아는 값(자기가 보낸 task_id)을 응답이
    // 그대로 반향하는 것은 정보 노출이 아니다 — 증명해야 할 것은 그 id가
    // "없다"와 "다른 Project 소속이다" 두 경우에 **동일한** 응답을 낳는다는
    // 것이므로, id를 고정하고 세계 상태만 바꿔 바이트 단위로 비교한다.
    let task_id = fleet_core::TaskId::new();

    let missing_resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/issues/{id}/links", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({ "task_id": task_id.to_string() }))
    .send()
    .await
    .unwrap();
    let missing_status = missing_resp.status();
    let missing_body: serde_json::Value = missing_resp.json().await.unwrap();

    let other_project = fleet_core::Project::new("other-project");
    store.create_project(&other_project).await.unwrap();
    let mut foreign_task = fleet_core::Task::from_request(fleet_core::TaskRequest {
        prompt: "belongs to the other project".into(),
        created_by: "test".into(),
        project_id: Some(other_project.id),
        ..Default::default()
    });
    foreign_task.id = task_id;
    foreign_task.thread_id = task_id;
    store.insert_task(&foreign_task).await.unwrap();

    let cross_project_resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/issues/{id}/links", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({ "task_id": task_id.to_string() }))
    .send()
    .await
    .unwrap();
    let cross_project_status = cross_project_resp.status();
    let cross_project_body: serde_json::Value = cross_project_resp.json().await.unwrap();

    assert_eq!(missing_status, 400);
    assert_eq!(
        cross_project_status, missing_status,
        "a task in another project must be rejected the same way as one that does not exist"
    );
    assert_eq!(
        cross_project_body, missing_body,
        "the error body must not reveal that the task exists in a different project"
    );

    let links: Vec<serde_json::Value> = authed_get(
        &client,
        &format!("http://{}/api/issues/{id}/links", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert!(
        links.is_empty(),
        "the rejected link must not have been persisted"
    );
}

#[tokio::test]
async fn issue_endpoints_require_read_permission() {
    let (server, cookie, _store, _project) =
        spawn_issue_server(Some(&[PermissionKind::DashboardView])).await;
    let client = reqwest::Client::new();

    let resp = authed_get(
        &client,
        &format!("http://{}/api/issues", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn issue_mutations_require_csrf() {
    let (server, cookie, _store, project) = spawn_issue_server(None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://{}/api/issues", server.addr))
        .header("cookie", format!("fleet_session={cookie}"))
        .json(&serde_json::json!({
            "project_id": project.id.to_string(),
            "title": "no csrf",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

// ═══════════════════════════════════════════════════════════════════════
//  Project 화면 권한 게이팅 (로드맵 #48 / UI 설계 §3.9·§3.10)
// ═══════════════════════════════════════════════════════════════════════
//
// 페이지 자체를 권한으로 가린다 — 폼을 보여준 뒤 제출 시점에야 403을 주는
// 것보다 낫다. `serve_page_if_permitted`가 403 HTML을 돌려준다.

async fn page_status(client: &reqwest::Client, url: &str, cookie: &str) -> u16 {
    authed_get(client, url, cookie)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn project_pages_are_served_with_the_right_permissions() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    for path in ["projects", "projects/new"] {
        assert_eq!(
            page_status(&client, &format!("http://{}/{path}", server.addr), &cookie).await,
            200,
            "/{path} must render for an admin"
        );
    }
    // 상세는 존재하지 않는 id여도 껍데기 HTML을 준다 — 실제 조회는 JS가
    // `/api/projects/:id`로 하고 404를 화면에 표시한다(host-detail과 동일 관례).
    assert_eq!(
        page_status(
            &client,
            &format!(
                "http://{}/projects/{}",
                server.addr,
                fleet_core::ProjectId::new()
            ),
            &cookie
        )
        .await,
        200
    );
}

#[tokio::test]
async fn projects_list_page_is_forbidden_without_project_read() {
    let (store, cookie) =
        seed_test_session_with_perms(MemStore::new(), &[PermissionKind::DashboardView]).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    assert_eq!(
        page_status(
            &client,
            &format!("http://{}/projects", server.addr),
            &cookie
        )
        .await,
        403
    );
}

#[tokio::test]
async fn new_project_page_is_forbidden_without_project_create() {
    // 읽기만 가능한 사용자에게 생성 폼을 보여주면 안 된다 — 채워 넣고
    // 제출한 뒤에야 거절당하는 경험이 된다.
    let (store, cookie) =
        seed_test_session_with_perms(MemStore::new(), &[PermissionKind::ProjectRead]).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    assert_eq!(
        page_status(
            &client,
            &format!("http://{}/projects", server.addr),
            &cookie
        )
        .await,
        200,
        "read-only user can still see the list"
    );
    assert_eq!(
        page_status(
            &client,
            &format!("http://{}/projects/new", server.addr),
            &cookie
        )
        .await,
        403,
        "but not the create form"
    );
}

// ── AgentTemplate 화면과 단건 조회 (로드맵 #92) ─────────────────────────
//
// 이 표면의 AgentTemplate 엔드포인트는 `6d3763e`에서 들어왔지만 통합 테스트가
// 한 건도 없었다(그 커밋이 "이 표면에는 아직 화면이 없고 JSON API뿐"이라고
// 남긴 검증 한계가 정확히 이것이다). 화면을 붙이면서 함께 닫는다.

/// 테스트용 AgentTemplate 하나. `status`를 인자로 받는 이유는 파생 필드
/// (`allowed_transitions`/`accepts_new_revisions`)가 상태에 따라 갈리기 때문이다.
fn sample_agent_template(
    name: &str,
    status: fleet_core::AgentTemplateStatus,
) -> fleet_core::AgentTemplate {
    let now = Utc::now();
    fleet_core::AgentTemplate {
        id: fleet_core::AgentTemplateId::new(),
        project_id: None,
        name: name.into(),
        description: Some("seeded".into()),
        created_by: Some("test_admin".into()),
        status,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn agent_template_pages_are_served_with_the_right_permissions() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    for path in ["agent-templates", "agent-templates/new"] {
        assert_eq!(
            page_status(&client, &format!("http://{}/{path}", server.addr), &cookie).await,
            200,
            "/{path} must render for an admin"
        );
    }
    // 상세는 존재하지 않는 id여도 껍데기 HTML을 준다 — projects/hosts와 같은
    // 관례로, 실제 조회와 404 표시는 JS가 `/api/agent-templates/:id`로 한다.
    assert_eq!(
        page_status(
            &client,
            &format!(
                "http://{}/agent-templates/{}",
                server.addr,
                fleet_core::AgentTemplateId::new()
            ),
            &cookie
        )
        .await,
        200
    );
}

#[tokio::test]
async fn agent_template_pages_are_forbidden_without_their_permissions() {
    // 목록/상세는 read로, 생성 폼은 create로 가려진다. 읽기만 가능한 사용자에게
    // 생성 폼을 보여주면 다 채워 넣고 제출한 뒤에야 403을 받는 경험이 된다.
    let (store, cookie) =
        seed_test_session_with_perms(MemStore::new(), &[PermissionKind::DashboardView]).await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();
    assert_eq!(
        page_status(
            &client,
            &format!("http://{}/agent-templates", server.addr),
            &cookie
        )
        .await,
        403
    );

    let (store, cookie) =
        seed_test_session_with_perms(MemStore::new(), &[PermissionKind::AgentTemplateRead]).await;
    let server = spawn_server_inner(store).await;
    assert_eq!(
        page_status(
            &client,
            &format!("http://{}/agent-templates", server.addr),
            &cookie
        )
        .await,
        200,
        "read-only user can still see the list"
    );
    assert_eq!(
        page_status(
            &client,
            &format!("http://{}/agent-templates/new", server.addr),
            &cookie
        )
        .await,
        403,
        "but not the create form"
    );
}

#[tokio::test]
async fn get_agent_template_api_ships_the_derived_transition_fields() {
    // 화면이 전이표를 다시 구현하지 않게 하려고 서버가 열거를 실어 보낸다.
    // 여기서 잠그는 것은 값 자체가 아니라 **파생이 실제로 실린다**는 것이다 —
    // 표의 내용은 `fleet-core`의 단위 테스트가 소유한다.
    let store = MemStore::new();
    let draft = sample_agent_template("draft-one", fleet_core::AgentTemplateStatus::Draft);
    let retired = sample_agent_template("retired-one", fleet_core::AgentTemplateStatus::Retired);
    store.create_agent_template(&draft).await.unwrap();
    store.create_agent_template(&retired).await.unwrap();
    let (server, cookie) = spawn_authed_server(store).await;
    let client = reqwest::Client::new();

    let body: serde_json::Value = authed_get(
        &client,
        &format!("http://{}/api/agent-templates/{}", server.addr, draft.id),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(body["status"], "draft");
    assert_eq!(
        body["allowed_transitions"],
        serde_json::json!(["published", "discarded"])
    );
    assert_eq!(body["accepts_new_revisions"], true);

    // 종료 상태에서는 목록이 비고 revision도 못 붙는다. 이 두 값이 있어야
    // 화면이 "버튼 없음"과 "아직 못 받아옴"을 구분할 수 있다.
    let body: serde_json::Value = authed_get(
        &client,
        &format!("http://{}/api/agent-templates/{}", server.addr, retired.id),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(body["allowed_transitions"], serde_json::json!([]));
    assert_eq!(body["accepts_new_revisions"], false);
}

#[tokio::test]
async fn get_agent_template_api_separates_missing_from_forbidden() {
    // 목록을 받아 클라이언트에서 거르면 "없는 id"와 "볼 권한이 없는 표면"이
    // 둘 다 빈 결과가 되어 상세 화면이 어느 쪽인지 말할 수 없다. 단건 조회를
    // 따로 둔 이유가 이 구분이므로, 구분 자체를 테스트한다.
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let resp = authed_get(
        &client,
        &format!(
            "http://{}/api/agent-templates/{}",
            server.addr,
            fleet_core::AgentTemplateId::new()
        ),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 404);

    let store = MemStore::new();
    let t = sample_agent_template("hidden", fleet_core::AgentTemplateStatus::Published);
    store.create_agent_template(&t).await.unwrap();
    let (store, cookie) =
        seed_test_session_with_perms(store, &[PermissionKind::DashboardView]).await;
    let server = spawn_server_inner(store).await;
    let resp = authed_get(
        &client,
        &format!("http://{}/api/agent-templates/{}", server.addr, t.id),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "존재하는 템플릿이어도 read 없이는 403이어야 한다 — 404를 주면 존재 여부가 샌다"
    );
}

#[tokio::test]
async fn every_sidebar_page_links_to_projects_and_agent_templates() {
    // 사이드바는 HTML 파일마다 손으로 복제돼 있고 동기화 자동화가 없다
    // (`<!-- sidebar:start -->` 마커만 있고 그걸 읽는 코드는 없다). 링크를
    // 하나 추가할 때 일부 파일을 빠뜨리기 쉬우므로 여기서 강제한다.
    //
    // 아래 경로 목록은 **손으로 유지된다** — 그 자체가 이 테스트가 막으려는
    // 결함과 같은 모양이라, 목록에 없는 새 페이지는 라우팅되는데도 검사되지
    // 않는다. 그래서 뒤에 자산 전체를 훑는 단정을 하나 더 둔다: 앞의 것은
    // **서빙된 응답**을, 뒤의 것은 **빠짐없음**을 지킨다.
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    for path in [
        "",
        "tasks",
        "tasks/new",
        "hosts",
        "hosts/provision",
        "admin/ssh-keys",
        "admin/users",
        "admin/activity",
        "admin/tools",
        "projects",
        "projects/new",
        "agent-templates",
        "agent-templates/new",
    ] {
        let body = authed_get(&client, &format!("http://{}/{path}", server.addr), &cookie)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        for link in ["<span>Projects</span>", "<span>Agent Templates</span>"] {
            assert!(
                body.contains(link),
                "/{path} is missing the {link} sidebar link"
            );
        }
    }

    // 사이드바를 가진 자산이면 예외 없이 두 링크를 다 갖는다. `login.html`과
    // `bootstrap.html`은 인증 전 화면이라 사이드바 자체가 없으므로 마커로
    // 걸러 낸다 — 파일 이름으로 거르면 세 번째 인증 전 화면이 생겼을 때
    // 목록을 또 손으로 고쳐야 한다.
    let mut checked = 0;
    for path in fleet_dashboard::assets::Asset::iter().filter(|p| p.ends_with(".html")) {
        let body = fleet_dashboard::assets::Asset::get(&path).expect("자산 조회");
        let text = String::from_utf8_lossy(body.data.as_ref());
        if !text.contains("<!-- sidebar:start -->") {
            continue;
        }
        checked += 1;
        for link in ["<span>Projects</span>", "<span>Agent Templates</span>"] {
            assert!(
                text.contains(link),
                "{path} is missing the {link} sidebar link"
            );
        }
    }
    assert!(checked >= 15, "사이드바를 가진 자산이 {checked}개뿐이다");
}

// ── 로드맵 #62 2단계 게이트 3: 대시보드 HTTP 표면의 멱등성 ────────────────
//
// UI에는 `idempotency_key` 입력 컨트롤이 없다(브라우저 클릭으로는 이 코드에
// 도달할 수 없다) — 이 필드는 JSON/폼 API 소비자를 위한 것이다. 따라서 검증도
// HTTP 수준에서 한다. `spawn_dispatcher_server_with_store`는 실제 axum 서버와
// 실제 `Dispatcher`를 띄우므로, 이 두 테스트는 라우팅·CSRF·핸들러·디스패처·
// store를 관통하는 진짜 왕복이다.

/// 같은 키·같은 페이로드로 두 번 제출하면 두 번 다 200이고 **같은 task_id**이며,
/// 두 번째 응답은 `deduplicated: true`로 표시된다. 행은 하나만 남는다.
#[tokio::test]
async fn submit_task_with_same_idempotency_key_returns_the_same_task() {
    let worker = sample_worker("idem-w", WorkerStatus::Online);
    let (server, cookie, store) = spawn_dispatcher_server_with_store(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    let form = [
        ("prompt", "build the thing"),
        ("cwd", "/srv/fleet/workspaces/test"),
        ("csrf_token", TEST_CSRF),
        ("idempotency_key", "dash-once"),
    ];

    let first = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &form,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(first.status(), 200);
    let first: serde_json::Value = first.json().await.unwrap();
    assert_eq!(
        first["deduplicated"], false,
        "최초 제출은 중복이 아니다: {first}"
    );

    let second = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &form,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(
        second.status(),
        200,
        "같은 페이로드 재제출은 에러가 아니라 흡수다"
    );
    let second: serde_json::Value = second.json().await.unwrap();

    assert_eq!(
        second["task_id"], first["task_id"],
        "재제출은 최초 task_id를 돌려줘야 한다"
    );
    assert_eq!(
        second["deduplicated"], true,
        "중복임을 명시해야 한다 — 그렇지 않으면 `dispatched: false` + warning 없음이 \
         '재시도 예약됨'으로 오독된다: {second}"
    );

    let tasks = store
        .list_tasks(&fleet_core::TaskFilter::default())
        .await
        .unwrap();
    assert_eq!(tasks.len(), 1, "행은 하나여야 한다: {tasks:?}");
}

/// 같은 키에 **다른** 페이로드가 오면 409 Conflict이고, 두 번째 행은 생기지 않는다.
/// 이 경로만이 이 핸들러에서 4xx다 — 나머지 디스패치 실패는 행이 이미 존재하므로
/// `200 + dispatched:false + warning`이다.
#[tokio::test]
async fn submit_task_with_conflicting_payload_returns_409() {
    let worker = sample_worker("idem-w2", WorkerStatus::Online);
    let (server, cookie, store) = spawn_dispatcher_server_with_store(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    let first = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[
            ("prompt", "build the thing"),
            ("cwd", "/srv/fleet/workspaces/test"),
            ("csrf_token", TEST_CSRF),
            ("idempotency_key", "dash-once"),
        ],
    )
    .send()
    .await
    .unwrap();
    assert_eq!(first.status(), 200);

    let conflicting = authed_post_form(
        &client,
        &format!("http://{}/api/tasks", server.addr),
        &cookie,
        &[
            ("prompt", "delete the thing"),
            ("cwd", "/srv/fleet/workspaces/test"),
            ("csrf_token", TEST_CSRF),
            ("idempotency_key", "dash-once"),
        ],
    )
    .send()
    .await
    .unwrap();
    assert_eq!(
        conflicting.status(),
        409,
        "같은 키에 다른 페이로드는 409여야 한다"
    );

    let tasks = store
        .list_tasks(&fleet_core::TaskFilter::default())
        .await
        .unwrap();
    assert_eq!(
        tasks.len(),
        1,
        "거절된 제출은 행을 남기면 안 된다: {tasks:?}"
    );
}

/// 빈 `idempotency_key`(HTML 폼이 비어 있는 입력에 대해 보내는 `""`)는 키가
/// 아니다 — 접지 않으면 키를 쓰지 않는 모든 제출이 `""` 하나를 공유해 서로를
/// 중복으로 판정한다. 이 회귀는 조용하고 치명적이라 명시적으로 고정한다.
#[tokio::test]
async fn empty_idempotency_key_does_not_deduplicate() {
    let worker = sample_worker("idem-w3", WorkerStatus::Online);
    let (server, cookie, store) = spawn_dispatcher_server_with_store(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    for prompt in ["first thing", "second thing"] {
        let resp = authed_post_form(
            &client,
            &format!("http://{}/api/tasks", server.addr),
            &cookie,
            &[
                ("prompt", prompt),
                ("cwd", "/srv/fleet/workspaces/test"),
                ("csrf_token", TEST_CSRF),
                ("idempotency_key", ""),
            ],
        )
        .send()
        .await
        .unwrap();
        assert_eq!(resp.status(), 200, "prompt={prompt}");
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(
            body["deduplicated"], false,
            "빈 문자열은 키가 아니다: {body}"
        );
    }

    let tasks = store
        .list_tasks(&fleet_core::TaskFilter::default())
        .await
        .unwrap();
    assert_eq!(tasks.len(), 2, "두 제출 모두 살아 있어야 한다: {tasks:?}");
}

/// 로드맵 #69 — `cwd` 없는/잘못된 제출은 400이고 **행을 남기지 않는다**.
///
/// 게이트가 `Dispatcher::submit()` 안에도 있지만, 대시보드 핸들러에서 먼저
/// 판정해야 사용자가 `dispatch failed`가 아니라 400을 본다. 그리고 판정이
/// `insert_task_idempotent` **앞**이어야 영원히 디스패치될 수 없는 Pending
/// 행이 남지 않는다 — 그 점을 store를 직접 조회해 확인한다.
#[tokio::test]
async fn submit_task_without_a_valid_cwd_is_rejected_and_creates_nothing() {
    let worker = sample_worker("cwd-w", WorkerStatus::Online);
    let (server, cookie, store) = spawn_dispatcher_server_with_store(MemStore::new(), worker).await;
    let client = reqwest::Client::new();

    // 빈 문자열은 "폼에 입력하지 않음"이고, 나머지는 규칙 위반이다.
    for bad in ["", "relative/path", "/srv/../etc", "/", "."] {
        let resp = authed_post_form(
            &client,
            &format!("http://{}/api/tasks", server.addr),
            &cookie,
            &[
                ("prompt", "do something"),
                ("cwd", bad),
                ("csrf_token", TEST_CSRF),
            ],
        )
        .send()
        .await
        .unwrap();
        assert_eq!(resp.status(), 400, "cwd={bad:?} must be refused");
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("cwd"),
            "the refusal must name cwd so the operator knows which field to fix: {body}"
        );
    }

    let tasks = store
        .list_tasks(&fleet_core::TaskFilter::default())
        .await
        .unwrap();
    assert!(
        tasks.is_empty(),
        "a refused submission must not leave a row behind: {tasks:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Task 삭제 + 스레드 목록 (로드맵 #96)
// ═══════════════════════════════════════════════════════════════════════

/// terminal(Cancelled) Task는 204로 지워지고, 스토어에서 실제로 사라진다.
#[tokio::test]
async fn delete_task_removes_a_terminal_task() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();

    let mut task = Task::from_request(fleet_core::TaskRequest {
        prompt: "done already".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    task.status = fleet_core::TaskStatus::Cancelled {
        reason: "test setup".into(),
        cancelled_at: Utc::now(),
    };
    store.insert_task(&task).await.unwrap();

    let resp = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/tasks/{}", server.addr, task.id),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 204);

    let remaining = store.get_task(task.id).await.unwrap();
    assert!(
        remaining.is_none(),
        "deleted task must be gone from the store"
    );
}

/// Pending(비종결) Task는 삭제가 거절된다 — 409, 스토어에는 그대로 남는다.
#[tokio::test]
async fn delete_task_rejects_a_non_terminal_task() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();

    let task = Task::from_request(fleet_core::TaskRequest {
        prompt: "still pending".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    store.insert_task(&task).await.unwrap();

    let resp = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/tasks/{}", server.addr, task.id),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 409);

    let still_there = store.get_task(task.id).await.unwrap();
    assert!(
        still_there.is_some(),
        "a rejected delete must not remove the task"
    );
}

/// 존재하지 않는 Task id는 404.
#[tokio::test]
async fn delete_task_unknown_id_returns_404() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    let resp = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/tasks/{}", server.addr, TaskId::new()),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 404);
}

/// terminal이어도, 아직 Pending인 다른 Task가 이것을 dependency로 걸고
/// 있으면 삭제가 막힌다 — 409, 본문에 의존하는 task id가 언급된다.
#[tokio::test]
async fn delete_task_blocked_by_pending_dependents() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();

    let mut blocker = Task::from_request(fleet_core::TaskRequest {
        prompt: "the one everyone depends on".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    blocker.status = fleet_core::TaskStatus::Cancelled {
        reason: "test setup".into(),
        cancelled_at: Utc::now(),
    };
    store.insert_task(&blocker).await.unwrap();

    let dependent = Task::from_request(fleet_core::TaskRequest {
        prompt: "waiting on blocker".into(),
        created_by: "test".into(),
        dependency_ids: vec![blocker.id],
        ..Default::default()
    });
    store.insert_task(&dependent).await.unwrap();

    let resp = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/tasks/{}", server.addr, blocker.id),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 409);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains(&dependent.id.to_string()),
        "409 body must name the blocking dependent: {body}"
    );

    let still_there = store.get_task(blocker.id).await.unwrap();
    assert!(still_there.is_some());
}

/// `TaskDelete` 권한이 없으면 403 — terminal 여부와 무관하게 권한이 먼저 걸린다.
#[tokio::test]
async fn delete_task_requires_task_delete_permission() {
    let (store, cookie) =
        seed_test_session_with_perms(MemStore::new(), &[PermissionKind::TaskList]).await;
    let store = Arc::new(store) as Arc<dyn Store>;

    let mut task = Task::from_request(fleet_core::TaskRequest {
        prompt: "irrelevant".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    task.status = fleet_core::TaskStatus::Cancelled {
        reason: "test setup".into(),
        cancelled_at: Utc::now(),
    };
    store.insert_task(&task).await.unwrap();

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://__test_unused__@localhost/__none__")
        .expect("connect_lazy must not perform I/O");
    let state = Arc::new(DashboardState::new(store.clone(), pool, None));
    let app = build_dashboard_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let client = reqwest::Client::new();
    let resp = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{addr}/api/tasks/{}", task.id),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 403);

    let still_there = store.get_task(task.id).await.unwrap();
    assert!(still_there.is_some());
}

/// CSRF 헤더 없이는 삭제도 거절된다 — 다른 뮤테이션과 같은 게이트를 탄다.
#[tokio::test]
async fn delete_task_without_csrf_header_is_rejected() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();

    let mut task = Task::from_request(fleet_core::TaskRequest {
        prompt: "should survive".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    task.status = fleet_core::TaskStatus::Cancelled {
        reason: "test setup".into(),
        cancelled_at: Utc::now(),
    };
    store.insert_task(&task).await.unwrap();

    let resp = client
        .delete(format!("http://{}/api/tasks/{}", server.addr, task.id))
        .header(
            "cookie",
            format!("fleet_session={cookie}; fleet_csrf={TEST_CSRF}"),
        )
        // 의도적으로 x-csrf-token 헤더를 생략.
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);

    let still_there = store.get_task(task.id).await.unwrap();
    assert!(still_there.is_some());
}

/// `GET /api/task-threads`는 Task 단위가 아니라 스레드 단위로 묶어서 반환한다.
/// 루트+답장 한 쌍이 한 스레드로 묶이고, 독립 Task는 자기 혼자만 있는
/// 스레드로 별도 항목이 된다.
#[tokio::test]
async fn list_task_threads_groups_by_thread_id() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();

    let root = Task::from_request(fleet_core::TaskRequest {
        prompt: "root of a thread".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    store.insert_task(&root).await.unwrap();

    let mut reply = Task::from_request(fleet_core::TaskRequest {
        prompt: "a reply".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    reply.inherit_from_parent(&root);
    store.insert_task(&reply).await.unwrap();

    let solo = Task::from_request(fleet_core::TaskRequest {
        prompt: "a standalone task".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    store.insert_task(&solo).await.unwrap();

    let resp = authed_json(
        &client,
        reqwest::Method::GET,
        &format!("http://{}/api/task-threads", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let threads = body["threads"].as_array().unwrap();
    assert_eq!(
        threads.len(),
        2,
        "root+reply와 solo, 두 스레드여야 한다: {threads:?}"
    );

    let thread_with_root = threads
        .iter()
        .find(|t| t["thread_id"] == root.id.to_string())
        .expect("root의 스레드가 있어야 한다");
    assert_eq!(thread_with_root["root"]["id"], root.id.to_string());
    let members = thread_with_root["members"].as_array().unwrap();
    assert_eq!(
        members.len(),
        2,
        "root+reply 둘 다 멤버여야 한다: {members:?}"
    );

    let thread_with_solo = threads
        .iter()
        .find(|t| t["thread_id"] == solo.id.to_string())
        .expect("solo의 스레드가 있어야 한다");
    assert_eq!(thread_with_solo["members"].as_array().unwrap().len(), 1);
}

/// 루트 Task가 삭제된 스레드는 `root`가 `null`이 되지만, 남은 멤버(답장)는
/// 여전히 그 스레드에 묶여서 조회된다 — `parent_task_id`의
/// `ON DELETE SET NULL`과 달리 `thread_id`는 건드리지 않는다는 설계
/// 그대로다(`ui-design.md` §3.3).
#[tokio::test]
async fn list_task_threads_survives_a_deleted_root() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();

    let mut root = Task::from_request(fleet_core::TaskRequest {
        prompt: "will be deleted".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    root.status = fleet_core::TaskStatus::Cancelled {
        reason: "test setup".into(),
        cancelled_at: Utc::now(),
    };
    store.insert_task(&root).await.unwrap();

    let mut reply = Task::from_request(fleet_core::TaskRequest {
        prompt: "outlives its root".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    reply.inherit_from_parent(&root);
    store.insert_task(&reply).await.unwrap();

    let outcome = store.delete_task(root.id).await.unwrap();
    assert_eq!(outcome, fleet_core::TaskDeleteOutcome::Deleted);

    let resp = authed_json(
        &client,
        reqwest::Method::GET,
        &format!("http://{}/api/task-threads", server.addr),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let threads = body["threads"].as_array().unwrap();
    assert_eq!(threads.len(), 1);
    assert!(
        threads[0]["root"].is_null(),
        "deleted root must serialize as a null root: {:?}",
        threads[0]
    );
    let members = threads[0]["members"].as_array().unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["id"], reply.id.to_string());
}

/// 삭제 계약 완료 게이트 6번째: 삭제 성공·거부 모두 `task.delete` 감사
/// 이벤트를 남긴다 (docs/architecture/tasks/management.md "삭제 계약의
/// 완료 게이트").
#[tokio::test]
async fn delete_task_records_a_task_delete_audit_event_on_success_and_rejection() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();

    let mut deletable = Task::from_request(fleet_core::TaskRequest {
        prompt: "will be deleted".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    deletable.status = fleet_core::TaskStatus::Cancelled {
        reason: "test setup".into(),
        cancelled_at: Utc::now(),
    };
    store.insert_task(&deletable).await.unwrap();

    let pending = Task::from_request(fleet_core::TaskRequest {
        prompt: "still pending".into(),
        created_by: "test".into(),
        ..Default::default()
    });
    store.insert_task(&pending).await.unwrap();

    let ok_resp = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/tasks/{}", server.addr, deletable.id),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(ok_resp.status(), 204);

    let rejected_resp = authed_json(
        &client,
        reqwest::Method::DELETE,
        &format!("http://{}/api/tasks/{}", server.addr, pending.id),
        &cookie,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(rejected_resp.status(), 409);

    let events = store
        .list_audit_events(&fleet_core::AuditFilter {
            action: Some(fleet_core::audit::action::TASK_DELETE.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        events.len(),
        2,
        "성공 1건 + 거부 1건, task.delete 감사 이벤트가 정확히 둘이어야 한다: {events:?}"
    );

    let success_event = events
        .iter()
        .find(|e| e.target_id.as_deref() == Some(&deletable.id.to_string()))
        .expect("삭제 성공에 대한 감사 이벤트가 있어야 한다");
    assert_eq!(success_event.outcome, fleet_core::AuditOutcome::Success);
    assert_eq!(success_event.target_type.as_deref(), Some("task"));

    let rejected_event = events
        .iter()
        .find(|e| e.target_id.as_deref() == Some(&pending.id.to_string()))
        .expect("삭제 거부에 대한 감사 이벤트가 있어야 한다");
    assert_eq!(rejected_event.outcome, fleet_core::AuditOutcome::Failure);
}

// ═══════════════════════════════════════════════════════════════════════
//  로드맵 #67 4a — Agent → Worker 배정
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_agent_places_it_on_an_online_worker() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();
    let worker = Worker::new("placer", "wss://placer.invalid/ws");
    store.upsert_worker(&worker).await.unwrap();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "placed-home").await;

    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "builder"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert_eq!(created["worker_id"], worker.id.to_string());
    assert!(created["assigned_at"].is_string());

    // 생성 시 배정은 별도의 `agent.assign` 이벤트를 내지 않는다 —
    // `agent.assign`이 정확히 **이동 횟수**를 세도록 하기 위해서다.
    // 대신 배정 대상은 `agent.create`의 detail에 실린다.
    let created_events = store
        .list_audit_events(&fleet_core::AuditFilter {
            action: Some(fleet_core::audit::action::AGENT_CREATE.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(created_events.len(), 1);
    assert_eq!(created_events[0].detail["worker_id"], worker.id.to_string());

    let assign_events = store
        .list_audit_events(&fleet_core::AuditFilter {
            action: Some(fleet_core::audit::action::AGENT_ASSIGN.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        assign_events.is_empty(),
        "생성 시 배정은 이동이 아니다: {assign_events:?}"
    );
}

#[tokio::test]
async fn create_agent_succeeds_when_no_worker_qualifies() {
    // Worker가 한 대도 없어도 생성은 200이다. 여기서 실패시키면 Worker를
    // 붙이기 전에 Project와 Agent를 정의하는 정상 사용이 막힌다.
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "no-fleet").await;

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "lonely"}))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);

    let created: serde_json::Value = resp.json().await.unwrap();
    // `AgentSummary`는 두 필드를 `skip_serializing_if = "Option::is_none"`으로
    // 두므로, 미배정 Agent에서는 키 자체가 **없다**. null과 부재를 구분하지
    // 않는 단정을 쓰면 이 계약이 조용히 바뀌어도 통과한다.
    assert!(created.get("worker_id").is_none(), "{created}");
    assert!(created.get("assigned_at").is_none(), "{created}");
}

/// 감사 로그는 **일어나지 않은 배정**을 적지 않는다
/// (로드맵 `#67` 구현 게이트 ①-A-2).
///
/// `create_agent`의 반환형을 `Option<WorkerId>`로 넓힌 유일한 이유가 이것이다.
/// 응답과 감사 detail이 **둘 다** 핸들러의 로컬 `agent` 구조체에서 나오므로,
/// Store가 상한 잠금 아래에서 선점에 실패했는데 되맞추지 않으면 두 곳이 함께
/// 거짓말을 한다. 응답의 거짓은 다시 조회하면 드러나지만 감사 로그의 거짓은
/// 남는다.
///
/// **왜 경합으로만 볼 수 있는가.** ①-A-1의 후보 필터와 ①-A-2의 선점은 같은
/// 술어(`status <> 'stopped'`)로 센다. 그래서 순차 경로에서는 필터가 가득 찬
/// Worker를 애초에 지명하지 않고, 되맞춤 분기에 도달할 방법이 없다. 그 분기는
/// 정의상 경합 전용이다.
///
/// **그런데도 이 테스트는 flaky하지 않다.** 단정 두 개가 경합이 일어났든 아니든
/// 항상 참이어야 하는 것들이기 때문이다 — 배정은 정확히 하나, 그리고 모든
/// 감사 detail이 저장된 행과 일치. 경합이 실제로 겹치는지 여부는 되맞춤 분기가
/// **실행되는지**를 정할 뿐 판정을 흔들지 않는다. 즉 이 테스트는 결함을 확률적
/// 으로 잡고 결정적으로 통과한다. 되맞춤을 지우면 겹칠 때마다 붉어진다.
/// 상한 1인 Worker에 N개의 생성을 동시에 던져도 배정은 하나뿐이다 —
/// HTTP 표면을 통과한 상한 집행을 본다.
///
/// **이 테스트는 되맞춤 분기(`if placed != agent.worker_id`)에 닿지
/// 못한다.** MemStore의 연산 사이에는 `.await` 양보점이 없어
/// `place_on_create`와 `create_agent`가 한 태스크 안에서 이어 붙기
/// 때문이다. 실측으로도 되맞춤을 지운 트리에서 12회 전부 통과했다
/// (2-way `join!` 12회, 8-way 배리어 12회 모두 0건). 그 분기는 바로
/// 아래 `create_agent_follows_the_store_when_the_slot_claim_drops_the_placement`
/// 가 결정적으로 덮는다.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_creates_through_the_api_cannot_exceed_the_cap() {
    const N: usize = 8;
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();
    let mut worker = Worker::new("capped", "wss://capped.invalid/ws");
    worker.max_agent_processes = Some(1);
    store.upsert_worker(&worker).await.unwrap();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "audit-race").await;

    // 배리어로 요청 **구성** 편차를 걷어낸다. 없으면 마지막 요청이 만들어질
    // 때 첫 요청은 이미 저장을 끝냈을 수 있고, 그러면 순차 실행과 구분되지
    // 않는다.
    let barrier = Arc::new(tokio::sync::Barrier::new(N));
    let mut handles = Vec::with_capacity(N);
    for i in 0..N {
        let client = client.clone();
        let cookie = cookie.clone();
        let project_id = project_id.clone();
        let barrier = Arc::clone(&barrier);
        let addr = server.addr;
        handles.push(tokio::spawn(async move {
            let req = authed_json(
                &client,
                reqwest::Method::POST,
                &format!("http://{addr}/api/agents"),
                &cookie,
            )
            .json(&serde_json::json!({
                "project_id": project_id,
                "name": format!("racer-{i}"),
            }));
            barrier.wait().await;
            let resp = req.send().await.unwrap();
            assert_eq!(resp.status(), 200);
            resp.json::<serde_json::Value>().await.unwrap()
        }));
    }
    let mut bodies = Vec::with_capacity(N);
    for h in handles {
        bodies.push(h.await.unwrap());
    }

    let placed_in_responses = bodies
        .iter()
        .filter(|v| {
            v.get("worker_id")
                .and_then(serde_json::Value::as_str)
                .is_some()
        })
        .count();
    assert_eq!(
        placed_in_responses, 1,
        "상한 1인 Worker에 배정을 보고한 응답이 정확히 하나여야 한다: {bodies:?}"
    );

    // 감사 로그는 저장된 행과 한 글자도 달라선 안 된다. 응답의 거짓은 다시
    // 조회하면 드러나지만 감사 로그의 거짓은 남는다.
    let events = store
        .list_audit_events(&fleet_core::AuditFilter {
            action: Some(fleet_core::audit::action::AGENT_CREATE.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(events.len(), N, "{events:?}");

    for event in &events {
        let id: fleet_core::AgentId = event
            .target_id
            .as_deref()
            .expect("agent.create는 대상 id를 실어야 한다")
            .parse()
            .unwrap();
        let stored = store.get_agent(id).await.unwrap().expect("agent row");
        let audited = event.detail["worker_id"].as_str().map(str::to_owned);
        assert_eq!(
            audited,
            stored.worker_id.map(|w| w.to_string()),
            "감사 로그가 저장된 배정과 다르다 — 기록된 쪽이 거짓이다: {event:?}"
        );
    }
}

/// 상한 잠금이 선점에 실패하면 생성은 성공하고 **배정만** 떨어진다. 그때
/// 핸들러가 저장된 사실에 되맞추지 않으면 응답과 감사 로그가 일어나지 않은
/// 배정을 말한다.
///
/// 그 분기는 순차 경로에서 도달 불가능하고(①-A-1의 필터와 ①-A-2의 선점이
/// `status <> 'stopped'`라는 같은 술어를 쓰므로 `choose_worker`가 가득 찬
/// Worker를 지목하지 않는다) 경합으로도 닿지 못하므로(위 테스트),
/// `MemStore::dropping_placements`로 **결과를 직접 세운다**. 상한 경합
/// 자체는 `fleet-store`의 `concurrent_creates_cannot_exceed_the_cap`이
/// Postgres 위에서 따로 증명한다 — 둘을 합쳐야 논증이 닫힌다.
#[tokio::test]
async fn create_agent_follows_the_store_when_the_slot_claim_drops_the_placement() {
    let (server, cookie, store) =
        spawn_authed_server_with_store_handle(MemStore::new().dropping_placements()).await;
    let client = reqwest::Client::new();
    // 배정 **가능한** Worker가 있어야 한다. 없으면 `place_on_create`가 후보를
    // 고르지 않아 되맞춤 분기에 애초에 들어가지 않고, 이 테스트는 평범한
    // 미배정 생성과 구분되지 않는다.
    let worker = Worker::new("claimable", "wss://claimable.invalid/ws");
    store.upsert_worker(&worker).await.unwrap();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "claim-drop").await;

    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "dropped"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    // 헛돌지 않는지 먼저 본다: 스위치는 `create_agent`만 건드리므로 후보
    // 지명은 살아 있어야 한다. 이 단정이 없으면 "Worker가 없어서 미배정"과
    // "선점에 실패해서 미배정"이 구분되지 않는다.
    let nominated = fleet_scheduler::placement::place_on_create(store.as_ref()).await;
    assert!(
        nominated.is_some(),
        "후보 지명이 죽어 있으면 되맞춤 분기에 들어가지 않는다"
    );

    assert!(
        created
            .get("worker_id")
            .and_then(serde_json::Value::as_str)
            .is_none(),
        "선점이 실패했는데 응답이 배정을 말한다: {created}"
    );

    let agent_id: fleet_core::AgentId = created["id"].as_str().unwrap().parse().unwrap();
    let stored = store.get_agent(agent_id).await.unwrap().expect("agent row");
    assert_eq!(stored.worker_id, None);
    assert_eq!(stored.assigned_at, None);

    let events = store
        .list_audit_events(&fleet_core::AuditFilter {
            action: Some(fleet_core::audit::action::AGENT_CREATE.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(
        events[0].detail["worker_id"].is_null(),
        "감사 로그가 일어나지 않은 배정을 적었다: {:?}",
        events[0]
    );
}

#[tokio::test]
async fn place_agent_recovers_an_unplaced_agent_and_audits_the_move() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "recovery").await;

    // Worker 없이 만든다 → 미배정.
    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "stray"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert!(created.get("worker_id").is_none());
    let agent_id = created["id"].as_str().unwrap().to_string();

    // 나중에 Worker가 붙는다.
    let first = Worker::new("first", "wss://first.invalid/ws");
    store.upsert_worker(&first).await.unwrap();

    let placed: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents/{}/place", server.addr, agent_id),
        &cookie,
    )
    .json(&serde_json::json!({}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(placed["worker_id"], first.id.to_string());
    assert!(placed["assigned_at"].is_string());

    // 두 번째 Worker로 옮긴다 — `previous_worker_id`가 기록되어야 한다.
    let second = Worker::new("second", "wss://second.invalid/ws");
    store.upsert_worker(&second).await.unwrap();
    let moved: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents/{}/place", server.addr, agent_id),
        &cookie,
    )
    .json(&serde_json::json!({"worker_id": second.id.to_string()}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(moved["worker_id"], second.id.to_string());

    let events = store
        .list_audit_events(&fleet_core::AuditFilter {
            action: Some(fleet_core::audit::action::AGENT_ASSIGN.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(events.len(), 2, "배정 두 번 = 이벤트 둘: {events:?}");
    // 감사 이벤트만으로 이동 궤적을 재구성할 수 있어야 한다 — 그러려면
    // 도착지뿐 아니라 출발지가 있어야 하고, 첫 배정의 출발지는 null이다.
    let details: Vec<_> = events.iter().map(|e| e.detail.clone()).collect();
    assert!(
        details
            .iter()
            .any(|d| d["worker_id"] == first.id.to_string() && d["previous_worker_id"].is_null()),
        "{details:?}"
    );
    assert!(
        details
            .iter()
            .any(|d| d["worker_id"] == second.id.to_string()
                && d["previous_worker_id"] == first.id.to_string()),
        "{details:?}"
    );
}

#[tokio::test]
async fn place_agent_returns_409_when_no_worker_qualifies() {
    // "배정할 Worker가 없다"는 서버 결함이 아니라 지금 fleet의 상태다.
    // 500으로 답하면 운영자가 오케스트레이터를 의심하게 된다.
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "conflict").await;
    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "waiting"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!(
            "http://{}/api/agents/{}/place",
            server.addr,
            created["id"].as_str().unwrap()
        ),
        &cookie,
    )
    .json(&serde_json::json!({}))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 409);
}

#[tokio::test]
async fn place_agent_rejects_an_unknown_worker_with_400() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "badworker").await;
    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "target"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!(
            "http://{}/api/agents/{}/place",
            server.addr,
            created["id"].as_str().unwrap()
        ),
        &cookie,
    )
    .json(&serde_json::json!({"worker_id": WorkerId::new().to_string()}))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn place_agent_requires_agent_manage_permission() {
    let (store, cookie) = seed_test_session_with_perms(
        MemStore::new(),
        &[PermissionKind::ProjectRead, PermissionKind::AgentRead],
    )
    .await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!(
            "http://{}/api/agents/{}/place",
            server.addr,
            fleet_core::AgentId::new()
        ),
        &cookie,
    )
    .json(&serde_json::json!({}))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn place_agent_without_csrf_header_is_rejected() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!(
            "http://{}/api/agents/{}/place",
            server.addr,
            fleet_core::AgentId::new()
        ))
        .header("cookie", format!("fleet_session={cookie}"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

// ── POST /api/agents/{id}/start (로드맵 #67 4b) ─────────────────────
//
// 이 표면이 증명하는 것은 **의도의 기록과 발행**까지다. 프로세스가 떴는지는
// 4c의 관측 주체가 생겨야 말할 수 있다.

#[tokio::test]
async fn start_agent_issues_a_command_and_audits_the_generation() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();
    let worker = Worker::new("starter", "wss://starter.invalid/ws");
    store.upsert_worker(&worker).await.unwrap();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "starting").await;

    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "runner"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let agent_id = created["id"].as_str().unwrap().to_string();
    // 생성이 start를 대신하지 않는다는 것이 이 설계의 전제다 — 4a가 생성
    // 시점에 자동 배정하므로, 생성이 running을 뜻하면 `ready`("정의는 끝났고
    // 시작 명령을 받을 수 있다")가 표현할 수 있는 상태가 사라진다.
    assert_eq!(created["desired_status"], "stopped");
    assert_eq!(created["start_pending"], false);

    let started: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents/{}/start", server.addr, agent_id),
        &cookie,
    )
    .json(&serde_json::json!({}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(started["desired_status"], "running");
    assert_eq!(started["status"], "ready", "관측은 아직 바뀌지 않는다");
    assert_eq!(started["start_pending"], true);
    assert_eq!(started["command_generation"], 1);
    assert_eq!(started["command_delivered"], false);

    let events = store
        .list_audit_events(&fleet_core::AuditFilter {
            action: Some(fleet_core::audit::action::AGENT_START.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
    // 세대를 detail에 싣는 이유: 이것이 있어야 나중에 Worker의 ACK와 맞대어
    // "그 명령이 실제로 전달됐는가"를 감사 로그만으로 답할 수 있다.
    assert_eq!(events[0].detail["generation"], 1, "{:?}", events[0].detail);
    assert_eq!(
        events[0].detail["worker_id"],
        worker.id.to_string(),
        "{:?}",
        events[0].detail
    );
}

#[tokio::test]
async fn starting_an_already_running_agent_issues_nothing() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();
    store
        .upsert_worker(&Worker::new("idem", "wss://idem.invalid/ws"))
        .await
        .unwrap();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "idempotent").await;
    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "again"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let agent_id = created["id"].as_str().unwrap().to_string();

    let url = format!("http://{}/api/agents/{}/start", server.addr, agent_id);
    for _ in 0..2 {
        authed_json(&client, reqwest::Method::POST, &url, &cookie)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
    }

    let events = store
        .list_audit_events(&fleet_core::AuditFilter {
            action: Some(fleet_core::audit::action::AGENT_START.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    // 두 번 눌렀지만 이벤트는 하나다. 값이 바뀔 때만 세대가 오르므로, 여기서
    // 이벤트를 내면 세대가 같은 줄이 여러 개 남아 "몇 번 시작을 명령했나"가
    // 무의미해진다.
    assert_eq!(events.len(), 1, "{events:?}");
}

#[tokio::test]
async fn start_agent_rejects_a_stopped_agent_with_400() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "terminal").await;
    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "gone"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let agent_id: fleet_core::AgentId = created["id"].as_str().unwrap().parse().unwrap();
    store
        .update_agent_status(agent_id, fleet_core::AgentStatus::Stopped)
        .await
        .unwrap();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents/{}/start", server.addr, agent_id),
        &cookie,
    )
    .json(&serde_json::json!({}))
    .send()
    .await
    .unwrap();
    // 404가 아니라 400이다 — 행은 존재하고, 거절 사유는 그 상태다. 이
    // 구분이 유지되려면 판정을 store의 UPDATE 술어로 내리면 안 된다.
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn start_agent_accepts_an_agent_with_no_worker() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "unplaced-start").await;
    // Worker를 하나도 등록하지 않았으므로 생성 시 자동 배정이 실패한다.
    let created: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents", server.addr),
        &cookie,
    )
    .json(&serde_json::json!({"project_id": project_id, "name": "orphan-start"}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert!(created.get("worker_id").is_none());
    let agent_id = created["id"].as_str().unwrap().to_string();

    let started: serde_json::Value = authed_json(
        &client,
        reqwest::Method::POST,
        &format!("http://{}/api/agents/{}/start", server.addr, agent_id),
        &cookie,
    )
    .json(&serde_json::json!({}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(started["desired_status"], "running");
    assert_eq!(started["command_generation"], 1);
}

#[tokio::test]
async fn start_agent_requires_agent_manage_permission() {
    let (store, cookie) = seed_test_session_with_perms(
        MemStore::new(),
        &[PermissionKind::ProjectRead, PermissionKind::AgentRead],
    )
    .await;
    let server = spawn_server_inner(store).await;
    let client = reqwest::Client::new();

    let resp = authed_json(
        &client,
        reqwest::Method::POST,
        &format!(
            "http://{}/api/agents/{}/start",
            server.addr,
            fleet_core::AgentId::new()
        ),
        &cookie,
    )
    .json(&serde_json::json!({}))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn start_agent_without_csrf_header_is_rejected() {
    let (server, cookie) = spawn_authed_server(MemStore::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!(
            "http://{}/api/agents/{}/start",
            server.addr,
            fleet_core::AgentId::new()
        ))
        .header("cookie", format!("fleet_session={cookie}"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn list_agents_filters_by_worker() {
    let (server, cookie, store) = spawn_authed_server_with_store_handle(MemStore::new()).await;
    let client = reqwest::Client::new();
    let worker = Worker::new("holder", "wss://holder.invalid/ws");
    store.upsert_worker(&worker).await.unwrap();
    let project_id = create_project_via_api(&client, server.addr, &cookie, "byworker").await;

    for name in ["a", "b"] {
        authed_json(
            &client,
            reqwest::Method::POST,
            &format!("http://{}/api/agents", server.addr),
            &cookie,
        )
        .json(&serde_json::json!({"project_id": project_id, "name": name}))
        .send()
        .await
        .unwrap();
    }

    let listed: Vec<serde_json::Value> = authed_get(
        &client,
        &format!("http://{}/api/agents?worker_id={}", server.addr, worker.id),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(listed.len(), 2);

    let none: Vec<serde_json::Value> = authed_get(
        &client,
        &format!(
            "http://{}/api/agents?worker_id={}",
            server.addr,
            WorkerId::new()
        ),
        &cookie,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert!(none.is_empty());
}
