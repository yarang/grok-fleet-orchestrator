//! axum 앱 조립 + 서버 실행.
//!
//! `AppState`는 모든 핸들러가 공유하는 의존성(Store, 인증 설정 등)을 캡슐화.
//! `build_app`는 라우터를 조립하고, `run_http_server`는 바인딩 후 serve.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    middleware::{self, Next},
    extract::Request,
    routing::{get, post},
    Router,
    http::StatusCode,
    response::Response,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use fleet_store::Store;

use crate::handlers;

/// HTTP API 서버의 공유 상태.
pub struct AppState {
    /// Store trait 구현체 (보통 `Arc<PgStore>`).
    pub store: Arc<dyn Store>,
    /// 워커에게 권장할 하트비트 주기 (초).
    pub heartbeat_interval_secs: u32,
    /// 인증 생략 여부 (개발 모드).
    pub allow_no_auth: bool,
    /// 허용된 bearer token 목록 (Phase 3 임시; Phase 4에서 OIDC로 대체).
    /// `None`이면 bearer 헤더 없이도 통과 (allow_no_auth와 동일 효과).
    pub valid_tokens: Option<Arc<Vec<String>>>,
    /// Cloudflare Access Application AUD (Phase 4).
    /// 설정된 경우 CF-Access-Jwt-Assertion 헤더의 aud 클레임과 비교.
    pub cf_audience: Option<String>,
}

impl AppState {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            heartbeat_interval_secs: 15,
            allow_no_auth: true,
            valid_tokens: None,
            cf_audience: None,
        }
    }

    pub fn with_heartbeat_interval(mut self, secs: u32) -> Self {
        self.heartbeat_interval_secs = secs;
        self
    }

    pub fn with_tokens(mut self, tokens: Vec<String>) -> Self {
        self.valid_tokens = Some(Arc::new(tokens));
        self.allow_no_auth = false;
        self
    }

    /// Cloudflare Access AUD 설정. 이후 모든 보호된 엔드포인트는
    /// 유효한 CF-Access-Jwt-Assertion 헤더를 요구.
    pub fn with_cf_audience(mut self, aud: impl Into<String>) -> Self {
        self.cf_audience = Some(aud.into());
        self.allow_no_auth = false;
        self
    }
}

/// 전체 라우터를 조립. 라우트 구조:
///
/// ```text
/// /
/// ├── health                      → GET /v1/health
/// └── v1
///     └── workers
///         ├── register            → POST
///         ├── heartbeat           → POST
///         ├──                     → GET (list)
///         └── :id                 → GET / DELETE
/// ```
pub fn build_app(state: Arc<AppState>) -> Router {
    let api_routes = Router::new()
        .route("/register", post(handlers::register_worker))
        .route("/heartbeat", post(handlers::heartbeat))
        .route("/", get(handlers::list_workers))
        .route("/:id", get(handlers::get_worker).delete(handlers::deregister_worker));

    let v1 = Router::new()
        .route("/health", get(handlers::health))
        .nest("/workers", api_routes);

    // Cloudflare Access 미들웨어 (가장 바깥).
    // 설정된 경우 모든 요청이 CF-Access-Jwt-Assertion 검증을 받음.
    let state_for_cf = state.clone();
    let v1 = if state.cf_audience.is_some() {
        v1.layer(middleware::from_fn(move |req, next| {
            let state = state_for_cf.clone();
            async move { crate::cloudflare::cloudflare_access_middleware(state, req, next).await }
        }))
    } else {
        v1
    };

    // Bearer token 인증 미들웨어 (CF Access 뒤).
    let state_for_auth = state.clone();
    let v1 = v1.layer(middleware::from_fn(move |req, next| {
        let state = state_for_auth.clone();
        async move { auth_middleware(state, req, next).await }
    }));

    Router::new()
        .nest("/v1", v1)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Bearer token 인증 미들웨어.
///
/// - `allow_no_auth == true`면 통과
/// - `valid_tokens == None`이면 통과
/// - 그 외에는 `Authorization: Bearer <token>` 헤더가 `valid_tokens`에 있어야 함
async fn auth_middleware(
    state: Arc<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if state.allow_no_auth {
        return Ok(next.run(req).await);
    }
    let Some(tokens) = &state.valid_tokens else {
        return Ok(next.run(req).await);
    };

    // health 엔드포인트는 인증 없이 허용 (LB 프로브용)
    if req.uri().path() == "/v1/health" {
        return Ok(next.run(req).await);
    }

    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let Some(header) = auth_header else {
        tracing::warn!(path = %req.uri().path(), "missing Authorization header");
        return Err(StatusCode::UNAUTHORIZED);
    };

    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "));

    let Some(token) = token else {
        tracing::warn!(path = %req.uri().path(), "malformed Authorization header");
        return Err(StatusCode::UNAUTHORIZED);
    };

    if tokens.iter().any(|t| t == token) {
        Ok(next.run(req).await)
    } else {
        tracing::warn!(path = %req.uri().path(), "invalid bearer token");
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// 서버 바인딩 + serve. shutdown 시그널은 호출자가 처리.
pub async fn run_http_server(
    state: Arc<AppState>,
    bind: SocketAddr,
) -> std::io::Result<()> {
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "HTTP API server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fleet_core::{
        CircuitState, EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput, TaskStatus,
        Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
    };
    use fleet_store::{Store, StoreError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// 테스트용 인메모리 Store.
    struct MemStore {
        workers: Mutex<HashMap<WorkerId, Worker>>,
        events: Mutex<Vec<EventEntry>>,
    }
    impl MemStore {
        fn new() -> Self {
            Self {
                workers: Mutex::new(HashMap::new()),
                events: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl Store for MemStore {
        async fn insert_task(&self, _: &Task) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_task(&self, _: TaskId) -> Result<Option<Task>, StoreError> {
            unimplemented!()
        }
        async fn update_task_status(&self, _: TaskId, _: &TaskStatus) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn list_tasks(&self, _: &TaskFilter) -> Result<Vec<Task>, StoreError> {
            unimplemented!()
        }
        async fn upsert_worker(&self, w: &Worker) -> Result<(), StoreError> {
            self.workers.lock().unwrap().insert(w.id, w.clone());
            Ok(())
        }
        async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
            Ok(self.workers.lock().unwrap().get(&id).cloned())
        }
        async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError> {
            Ok(self
                .workers
                .lock()
                .unwrap()
                .values()
                .find(|w| w.name == name)
                .cloned())
        }
        async fn list_workers(&self, f: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
            Ok(self
                .workers
                .lock()
                .unwrap()
                .values()
                .filter(|w| f.status.map_or(true, |s| w.status == s))
                .filter(|w| {
                    f.labels
                        .iter()
                        .all(|(k, v)| w.labels.get(k) == Some(v))
                })
                .cloned()
                .collect())
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
                w.active_tasks = hb.active_tasks;
                w.last_seen = Some(chrono::Utc::now());
            }
            Ok(())
        }
        async fn append_event(&self, e: &FleetEvent) -> Result<u64, StoreError> {
            let mut events = self.events.lock().unwrap();
            let seq = (events.len() + 1) as u64;
            events.push(EventEntry {
                seq,
                event: e.clone(),
            });
            Ok(seq)
        }
        async fn list_events(&self, _: u64, _: u32) -> Result<Vec<EventEntry>, StoreError> {
            Ok(self.events.lock().unwrap().clone())
        }
        async fn append_output(&self, _: TaskId, _: &str) -> Result<u64, StoreError> {
            unimplemented!()
        }
        async fn get_output(&self, _: TaskId, _: u64) -> Result<TaskOutput, StoreError> {
            unimplemented!()
        }
        async fn migrate(&self) -> Result<(), StoreError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn app_state_defaults_to_no_auth() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state = AppState::new(store);
        assert!(state.allow_no_auth);
        assert!(state.valid_tokens.is_none());
        assert_eq!(state.heartbeat_interval_secs, 15);
    }

    #[tokio::test]
    async fn app_state_with_tokens_disables_no_auth() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state = AppState::new(store).with_tokens(vec!["secret".into()]);
        assert!(!state.allow_no_auth);
        assert_eq!(state.valid_tokens.as_ref().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn build_app_smoke() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state = Arc::new(AppState::new(store));
        let _router = build_app(state);
        // 빌드가 성공하면 OK — 라우터 구성 검증.
    }

    #[test]
    fn circuit_state_unused_marker() {
        // CircuitState가 이 모듈에서 미사용이더라도 다른 곳에서 쓰이므로 re-export
        let _ = CircuitState::Closed;
    }
}
