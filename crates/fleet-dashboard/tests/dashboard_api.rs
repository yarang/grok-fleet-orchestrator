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
    auth::PermissionKind, Permission, PermissionId, Session, SessionId, Task, TaskId, User,
    UserId, Worker, WorkerId, WorkerStatus,
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
async fn seed_test_session_with_perms(store: MemStore, perm_kinds: &[PermissionKind]) -> (MemStore, String) {
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
async fn spawn_authed_server_with_base_path(store: MemStore, base_path: &str) -> (TestServer, String) {
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
        circuit_state: fleet_core::CircuitState::Closed,
        last_seen: None,
        worker_version: None,
        liveness_mode: fleet_core::WorkerLivenessMode::Periodic,
        registered_at: chrono::Utc::now(),
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
    let (server, cookie) =
        spawn_authed_server_with_base_path(MemStore::new(), "/dashboard").await;
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

/// `POST /logout`의 redirect Location이 base_path를 포함해야, prefix 뒤에
/// 마운트된 배포에서도 브라우저가 nginx가 실제로 라우팅하는 경로로 이동한다.
#[tokio::test]
async fn logout_redirect_includes_configured_base_path() {
    let (server, cookie) =
        spawn_authed_server_with_base_path(MemStore::new(), "/dashboard").await;
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
    let (store, cookie) = seed_test_session_with_perms(MemStore::new(), &[PermissionKind::TaskList]).await;
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
        &[("prompt", "1부터 5까지 더해줘"), ("csrf_token", TEST_CSRF)],
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
