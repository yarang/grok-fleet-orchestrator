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
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use fleet_core::{
    auth::PermissionKind, BootstrapToken, EventEntry, FleetEvent, Permission, PermissionId,
    Session, SessionId, Task, TaskFilter, TaskId, TaskOutput, TaskStatus, User, UserId, Worker,
    WorkerFilter, WorkerHeartbeat, WorkerId, WorkerStatus,
};
use fleet_dashboard::{build_dashboard_app, DashboardState};
use fleet_store::{Store, StoreError};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use tokio::task::JoinHandle;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
//  인메모리 Store (실제 DB 없이 테스트용)
// ═══════════════════════════════════════════════════════════════════════

struct MemStore {
    workers: Mutex<HashMap<WorkerId, Worker>>,
    tasks: Mutex<HashMap<TaskId, Task>>,
    events: Mutex<Vec<EventEntry>>,
    // RBAC (Phase 9.1 테스트용)
    users: Mutex<HashMap<UserId, User>>,
    sessions: Mutex<HashMap<String, Session>>, // token_hash → Session
    user_permissions: Mutex<HashMap<UserId, Vec<Permission>>>,
    // Host inventory (Phase P1.5 테스트용)
    hosts: Mutex<Vec<fleet_core::Host>>,
    host_events: Mutex<Vec<fleet_core::HostEvent>>,
}

impl MemStore {
    fn new() -> Self {
        Self {
            workers: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            events: Mutex::new(Vec::new()),
            users: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            user_permissions: Mutex::new(HashMap::new()),
            hosts: Mutex::new(Vec::new()),
            host_events: Mutex::new(Vec::new()),
        }
    }

    fn with_worker(self, w: Worker) -> Self {
        self.workers.lock().unwrap().insert(w.id, w);
        self
    }

    fn with_task(self, t: Task) -> Self {
        self.tasks.lock().unwrap().insert(t.id, t);
        self
    }

    fn with_host(self, h: fleet_core::Host) -> Self {
        self.hosts.lock().unwrap().push(h);
        self
    }

    /// `seed_test_session`과 동일하되 권한을 직접 지정한다 — 권한 부족(403) 경로를
    /// 테스트할 때 사용.
    fn seed_test_session_with_perms(self, perm_kinds: &[PermissionKind]) -> (Self, String) {
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
        self.users.lock().unwrap().insert(uid, user);

        let perms: Vec<Permission> = perm_kinds
            .iter()
            .map(|pk| Permission {
                id: PermissionId::new(),
                name: pk.as_str().to_string(),
                description: None,
            })
            .collect();
        self.user_permissions.lock().unwrap().insert(uid, perms);

        self.sessions
            .lock()
            .unwrap()
            .insert(sha256_hex(raw_token.as_bytes()), session);

        (self, raw_token)
    }

    /// 테스트용 관리자 사용자 + 유효한 세션을 주입하고,
    /// 세션 쿠키 raw 값을 반환.
    fn seed_test_session(self) -> (Self, String) {
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
        self.users.lock().unwrap().insert(uid, user);

        // 테스트 관리자에게 모든 권한 부여.
        let all_perms: Vec<Permission> = PermissionKind::all()
            .iter()
            .map(|pk| Permission {
                id: PermissionId::new(),
                name: pk.as_str().to_string(),
                description: None,
            })
            .collect();
        self.user_permissions.lock().unwrap().insert(uid, all_perms);

        self.sessions
            .lock()
            .unwrap()
            .insert(sha256_hex(raw_token.as_bytes()), session);

        (self, raw_token)
    }
}

/// SHA-256 hex 계산 (auth_util 과 동일 로직, 테스트 격리용).
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[async_trait]
impl Store for MemStore {
    async fn insert_task(&self, t: &Task) -> Result<(), StoreError> {
        self.tasks.lock().unwrap().insert(t.id, t.clone());
        Ok(())
    }
    async fn get_task(&self, id: TaskId) -> Result<Option<Task>, StoreError> {
        Ok(self.tasks.lock().unwrap().get(&id).cloned())
    }
    async fn update_task_status(&self, id: TaskId, status: &TaskStatus) -> Result<(), StoreError> {
        if let Some(t) = self.tasks.lock().unwrap().get_mut(&id) {
            t.status = status.clone();
        }
        Ok(())
    }
    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        let mut all: Vec<Task> = self.tasks.lock().unwrap().values().cloned().collect();
        all.sort_by_key(|a| a.created_at);
        all.truncate(filter.limit);
        Ok(all)
    }
    async fn upsert_worker(&self, w: &Worker) -> Result<(), StoreError> {
        self.workers.lock().unwrap().insert(w.id, w.clone());
        Ok(())
    }
    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
        Ok(self.workers.lock().unwrap().get(&id).cloned())
    }
    async fn get_worker_by_name(&self, _: &str) -> Result<Option<Worker>, StoreError> {
        Ok(None)
    }
    async fn list_workers(&self, filter: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
        let mut all: Vec<Worker> = self.workers.lock().unwrap().values().cloned().collect();
        if let Some(status) = filter.status {
            all.retain(|w| w.status == status);
        }
        if !filter.labels.is_empty() {
            all.retain(|w| {
                filter
                    .labels
                    .iter()
                    .all(|(k, v)| w.labels.get(k) == Some(v))
            });
        }
        all.sort_by_key(|w| w.registered_at);
        all.reverse();

        let start = filter.offset.min(all.len());
        let end = (start + filter.limit).min(all.len());
        Ok(all[start..end].to_vec())
    }
    async fn delete_worker(&self, id: WorkerId) -> Result<(), StoreError> {
        self.workers.lock().unwrap().remove(&id);
        Ok(())
    }
    async fn update_worker_heartbeat(
        &self,
        id: WorkerId,
        hb: &WorkerHeartbeat,
    ) -> Result<(), StoreError> {
        if let Some(w) = self.workers.lock().unwrap().get_mut(&id) {
            w.last_seen = Some(chrono::Utc::now());
            let _ = hb;
        }
        Ok(())
    }
    async fn append_event(&self, _: &FleetEvent) -> Result<u64, StoreError> {
        Ok(0)
    }
    async fn list_events(&self, after_seq: u64, limit: u32) -> Result<Vec<EventEntry>, StoreError> {
        let all = self.events.lock().unwrap();
        let filtered: Vec<EventEntry> = all
            .iter()
            .filter(|e| e.seq > after_seq)
            .take(limit as usize)
            .cloned()
            .collect();
        Ok(filtered)
    }
    async fn append_output(&self, _: TaskId, _: &str) -> Result<u64, StoreError> {
        Ok(0)
    }
    async fn get_output(&self, id: TaskId, _: u64) -> Result<TaskOutput, StoreError> {
        Ok(TaskOutput {
            task_id: id,
            chunks: Vec::new(),
            next_offset: 0,
        })
    }
    async fn migrate(&self) -> Result<(), StoreError> {
        Ok(())
    }
    async fn create_bootstrap_token(&self, _: &BootstrapToken) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn consume_bootstrap_token(&self, _: &str, _: &str) -> Result<(), StoreError> {
        unimplemented!()
    }
    async fn list_bootstrap_tokens(&self) -> Result<Vec<BootstrapToken>, StoreError> {
        unimplemented!()
    }
    async fn revoke_bootstrap_token(&self, _: &str) -> Result<bool, StoreError> {
        unimplemented!()
    }

    // ── RBAC (테스트 지원 구현체) ──────────────────────────────────────

    async fn create_user(&self, user: &User) -> Result<(), StoreError> {
        self.users.lock().unwrap().insert(user.id, user.clone());
        Ok(())
    }
    async fn get_user_by_id(&self, id: UserId) -> Result<Option<User>, StoreError> {
        Ok(self.users.lock().unwrap().get(&id).cloned())
    }
    async fn get_user_by_username(&self, name: &str) -> Result<Option<User>, StoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .values()
            .find(|u| u.username == name)
            .cloned())
    }
    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, StoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .values()
            .find(|u| u.email.as_deref() == Some(email))
            .cloned())
    }
    async fn count_users(&self) -> Result<u64, StoreError> {
        Ok(self.users.lock().unwrap().len() as u64)
    }
    async fn list_user_permissions(&self, uid: UserId) -> Result<Vec<Permission>, StoreError> {
        Ok(self
            .user_permissions
            .lock()
            .unwrap()
            .get(&uid)
            .cloned()
            .unwrap_or_default())
    }
    async fn create_session(&self, session: &Session) -> Result<(), StoreError> {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.token_hash.clone(), session.clone());
        Ok(())
    }
    async fn get_session_by_token_hash(&self, hash: &str) -> Result<Option<Session>, StoreError> {
        Ok(self.sessions.lock().unwrap().get(hash).cloned())
    }
    async fn delete_session(&self, id: SessionId) -> Result<(), StoreError> {
        self.sessions.lock().unwrap().retain(|_, s| s.id != id);
        Ok(())
    }

    // ── Email verification (테스트 stub) ─────────────────────────────

    async fn create_email_verification_token(
        &self,
        _token: &fleet_core::EmailVerificationToken,
    ) -> Result<(), StoreError> {
        Ok(())
    }
    async fn get_email_verification_token(
        &self,
        _token_hash: &str,
    ) -> Result<Option<fleet_core::EmailVerificationToken>, StoreError> {
        Ok(None)
    }
    async fn consume_email_verification_token(
        &self,
        _token_id: Uuid,
        _at: chrono::DateTime<Utc>,
    ) -> Result<(), StoreError> {
        Ok(())
    }
    async fn set_user_email_verified(
        &self,
        _user_id: UserId,
        _verified: bool,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    // ── Host inventory (테스트 지원 구현체) ───────────────────────────

    async fn upsert_host(&self, host: &fleet_core::Host) -> Result<(), StoreError> {
        let mut hosts = self.hosts.lock().unwrap();
        if let Some(existing) = hosts.iter_mut().find(|h| h.hostname == host.hostname) {
            *existing = host.clone();
        } else {
            hosts.push(host.clone());
        }
        Ok(())
    }

    async fn get_host_by_hostname(
        &self,
        hostname: &str,
    ) -> Result<Option<fleet_core::Host>, StoreError> {
        Ok(self
            .hosts
            .lock()
            .unwrap()
            .iter()
            .find(|h| h.hostname == hostname)
            .cloned())
    }

    async fn get_host_by_worker(
        &self,
        worker_id: WorkerId,
    ) -> Result<Option<fleet_core::Host>, StoreError> {
        Ok(self
            .hosts
            .lock()
            .unwrap()
            .iter()
            .find(|h| h.worker_id == Some(worker_id))
            .cloned())
    }

    async fn list_hosts(&self) -> Result<Vec<fleet_core::Host>, StoreError> {
        Ok(self.hosts.lock().unwrap().clone())
    }

    async fn append_host_event(&self, event: &fleet_core::HostEvent) -> Result<(), StoreError> {
        self.host_events.lock().unwrap().push(event.clone());
        Ok(())
    }

    async fn list_host_events(
        &self,
        host_id: Uuid,
        limit: u32,
    ) -> Result<Vec<fleet_core::HostEvent>, StoreError> {
        let events = self.host_events.lock().unwrap();
        let mut filtered: Vec<fleet_core::HostEvent> = events
            .iter()
            .filter(|e| e.host_id == host_id)
            .cloned()
            .collect();
        filtered.sort_by_key(|e| std::cmp::Reverse(e.created_at));
        filtered.truncate(limit as usize);
        Ok(filtered)
    }
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
    let (store, cookie) = store.seed_test_session();
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
        id: Uuid::new_v4(),
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
        .host_events
        .lock()
        .unwrap()
        .push(fleet_core::HostEvent {
            id: Uuid::new_v4(),
            host_id,
            event_type: "heartbeat".into(),
            severity: fleet_core::EventSeverity::Info,
            message: Some("heartbeat received".into()),
            payload: HashMap::new(),
            created_at: chrono::Utc::now(),
        });
    store
        .host_events
        .lock()
        .unwrap()
        .push(fleet_core::HostEvent {
            id: Uuid::new_v4(),
            host_id,
            event_type: "provision_ok".into(),
            severity: fleet_core::EventSeverity::Info,
            message: Some("provisioned successfully".into()),
            payload: HashMap::new(),
            created_at: chrono::Utc::now() - chrono::Duration::minutes(5),
        });

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
    let (store, cookie) = MemStore::new().seed_test_session_with_perms(&[PermissionKind::TaskList]);
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
    let (store, cookie) = MemStore::new().seed_test_session();
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

#[tokio::test]
async fn submit_task_rejects_empty_prompt() {
    let (store, cookie) = MemStore::new().seed_test_session();
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
async fn submit_task_without_dispatcher_returns_unavailable_and_creates_no_task() {
    // 이 하네스의 DashboardState.dispatcher는 항상 None — "기능 미구성" 경로.
    let (store, cookie) = MemStore::new().seed_test_session();
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
