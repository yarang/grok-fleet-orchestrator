//! 대시보드 앱 조립 + 서버 실행.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, Router};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use fleet_store::Store;

use crate::handlers;

/// 대시보드 서버의 공유 상태.
pub struct DashboardState {
    /// Store trait 구현체.
    pub store: Arc<dyn Store>,
    /// LISTEN/NOTIFY용 Postgres 풀 (SSE 스트리밍에서 사용).
    pub pool: sqlx::PgPool,
    /// Bearer token 인증 활성화.
    /// `None`이면 인증 없이 통과 (로컬 전용, 개발 모드).
    /// `Some(tokens)`면 `/health`만 제외하고 `Authorization: Bearer <token>` 필수.
    pub valid_tokens: Option<Arc<Vec<String>>>,
}

impl DashboardState {
    pub fn new(store: Arc<dyn Store>, pool: sqlx::PgPool) -> Self {
        Self {
            store,
            pool,
            valid_tokens: None,
        }
    }

    /// 인증 활성화. 빈 벡터면 인증 없이 통과 (allow-all 과 동일).
    pub fn with_tokens(mut self, tokens: Vec<String>) -> Self {
        if tokens.is_empty() {
            self.valid_tokens = None;
        } else {
            self.valid_tokens = Some(Arc::new(tokens));
        }
        self
    }
}

/// 전체 라우터 조립.
pub fn build_dashboard_app(state: Arc<DashboardState>) -> Router {
    // 인증 미들웨어 (가장 바깥). valid_tokens == None 이면 통과.
    let state_for_auth = state.clone();
    let app = Router::new()
        .route("/", get(handlers::index))
        .route("/health", get(handlers::health))
        .route("/api/overview", get(handlers::overview))
        .route("/api/workers", get(handlers::list_workers))
        .route("/api/tasks", get(handlers::list_tasks))
        .route("/api/events", get(handlers::list_events))
        .route("/api/events/stream", get(crate::sse::events_stream))
        .route("/static/*path", get(handlers::static_asset))
        .layer(middleware::from_fn(move |req, next| {
            let state = state_for_auth.clone();
            async move { auth_middleware(state, req, next).await }
        }))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);
    app
}

/// 대시보드 HTTP 서버 바인딩 + serve.
pub async fn run_dashboard_server(
    state: Arc<DashboardState>,
    bind: SocketAddr,
) -> std::io::Result<()> {
    let app = build_dashboard_app(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "dashboard server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Bearer token 인증 미들웨어.
///
/// - `valid_tokens == None`이면 통과 (로컬/개발 모드).
/// - `Some(tokens)`면 보호 경로(`/api/*`)에 한해 인증 필수.
///   `/`, `/static/*`, `/health`는 공개 — 브라우저가 HTML 과 정적 자원을
///   먼저 로드한 뒤 JS 가 localStorage 의 token 으로 `/api/*` 를 호출.
async fn auth_middleware(
    state: Arc<DashboardState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(tokens) = &state.valid_tokens else {
        return Ok(next.run(req).await);
    };

    let path = req.uri().path();

    // 공개 경로 — 인증 없이 허용.
    // - /health: LB 프로브
    // - /: dashboard HTML 페이지 자체 (JS 가 token 을 입력받아 /api/* 호출)
    // - /static/*: CSS, JS 자원 (민감 정보 없음)
    if path == "/health" || path == "/" || path.starts_with("/static/") {
        return Ok(next.run(req).await);
    }

    // Authorization 헤더 우선, 그 다음 ?token= 쿼리 파라미터 (htmx/SSE 호환).
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let token = auth_header
        .and_then(|h| {
            h.strip_prefix("Bearer ")
                .or_else(|| h.strip_prefix("bearer "))
        })
        .or_else(|| req.uri().query().and_then(|q| url_query_token(q)));

    let Some(token) = token else {
        tracing::warn!(path = %req.uri().path(), "dashboard: missing Authorization header");
        return Err(StatusCode::UNAUTHORIZED);
    };

    if !tokens.iter().any(|t| t == token) {
        tracing::warn!(path = %req.uri().path(), "dashboard: invalid bearer token");
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}

/// URL 쿼리스트링에서 `token=` 파라미터 값 추출.
fn url_query_token(query: &str) -> Option<&str> {
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next() == Some("token") {
            return parts.next();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fleet_core::{
        BootstrapToken, EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput, TaskStatus,
        Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
    };
    use fleet_store::{Store, StoreError};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tower::util::ServiceExt;

    struct MemStore {
        workers: Mutex<HashMap<WorkerId, Worker>>,
    }
    impl MemStore {
        fn new() -> Self {
            Self {
                workers: Mutex::new(HashMap::new()),
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
            Ok(vec![])
        }
        async fn upsert_worker(&self, w: &Worker) -> Result<(), StoreError> {
            self.workers.lock().unwrap().insert(w.id, w.clone());
            Ok(())
        }
        async fn get_worker(&self, _: WorkerId) -> Result<Option<Worker>, StoreError> {
            unimplemented!()
        }
        async fn get_worker_by_name(&self, _: &str) -> Result<Option<Worker>, StoreError> {
            Ok(None)
        }
        async fn list_workers(&self, _: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
            Ok(self.workers.lock().unwrap().values().cloned().collect())
        }
        async fn delete_worker(&self, _: WorkerId) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn update_worker_heartbeat(
            &self,
            _: WorkerId,
            _: &WorkerHeartbeat,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn append_event(&self, _: &FleetEvent) -> Result<u64, StoreError> {
            unimplemented!()
        }
        async fn list_events(&self, _: u64, _: u32) -> Result<Vec<EventEntry>, StoreError> {
            Ok(vec![])
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

        // Phase 8.6: credentials 메서드 — dashboard 테스트에서 미사용.
        async fn upsert_worker_credential(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: u32,
            _: Option<&str>,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_worker_credential(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<fleet_store::StoredCredential>, StoreError> {
            unimplemented!()
        }
        async fn list_worker_credentials(
            &self,
            _: &str,
        ) -> Result<Vec<fleet_store::StoredCredential>, StoreError> {
            unimplemented!()
        }
        async fn delete_worker_credential(&self, _: &str, _: &str) -> Result<bool, StoreError> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn dashboard_app_builds() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        // 실제 PgPool 없이 빌드만 검증. pool은 stub.
        // NOTE: dashboard_app 빌드에는 state가 필요하지만, pool은 테스트에서 생략.
        // 대신 라우터 구조만 확인.
        let _ = store;
        // dashboard_app(state) 호출을 위해 pool 필요 → 이 테스트는 생략.
    }

    // ── bearer auth 통합 테스트 ─────────────────────────────────────────

    fn pg_pool_stub() -> sqlx::PgPool {
        // 실제 DB 없이 Pool 타입만 필요한 테스트에서 사용.
        // PgPool::connect_lazy 가 아니면 빈 Pool 을 만들 방법이 없으므로
        // 더미 DSN 으로 connect_lazy 시도. 테스트에서는 pool 을 실제로 사용하지 않음.
        sqlx::PgPool::connect_lazy("postgres://stub:stub@127.0.0.1:1/stub")
            .expect("connect_lazy should not perform I/O")
    }

    #[tokio::test]
    async fn no_auth_when_valid_tokens_none() {
        // valid_tokens == None 이면 모든 경로 통과 (로컬 개발 모드).
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state = Arc::new(DashboardState::new(store, pg_pool_stub()));
        let app = build_dashboard_app(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_endpoint_bypasses_auth() {
        // /health 는 LB 프로브용 — valid_tokens 가 있어도 통과.
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state = Arc::new(
            DashboardState::new(store, pg_pool_stub()).with_tokens(vec!["secret-token".into()]),
        );
        let app = build_dashboard_app(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn protected_route_rejects_missing_token() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state = Arc::new(
            DashboardState::new(store, pg_pool_stub()).with_tokens(vec!["secret-token".into()]),
        );
        let app = build_dashboard_app(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/overview")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn protected_route_accepts_valid_token() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state = Arc::new(
            DashboardState::new(store, pg_pool_stub()).with_tokens(vec!["secret-token".into()]),
        );
        let app = build_dashboard_app(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/overview")
                    .header("Authorization", "Bearer secret-token")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // 200 또는 500 (store/pool 문제) — 인증은 통과했는지가 중요.
        assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn protected_route_rejects_invalid_token() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state = Arc::new(
            DashboardState::new(store, pg_pool_stub()).with_tokens(vec!["secret-token".into()]),
        );
        let app = build_dashboard_app(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/overview")
                    .header("Authorization", "Bearer wrong-token")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn with_tokens_empty_disables_auth() {
        // 빈 벡터는 valid_tokens = None 과 동일 (allow all).
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state = Arc::new(DashboardState::new(store, pg_pool_stub()).with_tokens(vec![]));
        assert!(state.valid_tokens.is_none());
    }

    #[tokio::test]
    async fn static_assets_are_public() {
        // /static/* 는 브라우저가 페이지 로드 시 먼저 가져오는 자원 — 공개.
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state =
            Arc::new(DashboardState::new(store, pg_pool_stub()).with_tokens(vec!["secret".into()]));
        let app = build_dashboard_app(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/static/styles.css")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn index_page_is_public() {
        // / (HTML) 자체는 공개 — JS 가 로드된 후 /api/* 호출 시 인증.
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state =
            Arc::new(DashboardState::new(store, pg_pool_stub()).with_tokens(vec!["secret".into()]));
        let app = build_dashboard_app(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn query_param_token_accepted() {
        // ?token= 쿼리 파라미터로도 인증 (SSE EventSource 호환).
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let state =
            Arc::new(DashboardState::new(store, pg_pool_stub()).with_tokens(vec!["secret".into()]));
        let app = build_dashboard_app(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/overview?token=secret")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
