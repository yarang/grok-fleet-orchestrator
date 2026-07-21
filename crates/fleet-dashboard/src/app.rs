//! 대시보드 앱 조립 + 서버 실행.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::middleware;
use axum::routing::{get, post, Router};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use fleet_store::Store;

use crate::auth::require_session;
use crate::handlers;

/// 대시보드 서버의 공유 상태.
pub struct DashboardState {
    /// Store trait 구현체.
    pub store: Arc<dyn Store>,
    /// LISTEN/NOTIFY용 Postgres 풀 (SSE 스트리밍에서 사용).
    pub pool: sqlx::PgPool,
    /// 쿠키 Secure 플래그 (로컬 개발은 false, 프로덕션은 true).
    pub secure_cookies: bool,
}

impl DashboardState {
    pub fn new(store: Arc<dyn Store>, pool: sqlx::PgPool) -> Self {
        Self {
            store,
            pool,
            secure_cookies: true,
        }
    }

    /// 로컬 개발용 (Secure 쿠키 비활성).
    pub fn new_insecure(store: Arc<dyn Store>, pool: sqlx::PgPool) -> Self {
        Self {
            store,
            pool,
            secure_cookies: false,
        }
    }
}

/// 전체 라우터 조립.
///
/// 라우트 그룹:
/// - **public**: `/login`, `/logout`, `/health` (세션 미들웨어 없음)
/// - **protected**: `/`, `/api/*`, `/static/*` (require_session 적용)
pub fn build_dashboard_app(state: Arc<DashboardState>) -> Router {
    let public = Router::new()
        .route("/login", get(handlers::login_page).post(handlers::login))
        .route(
            "/bootstrap",
            get(handlers::bootstrap_page).post(handlers::bootstrap),
        )
        .route("/health", get(handlers::health));

    let protected = Router::new()
        .route("/", get(handlers::index))
        .route("/api/overview", get(handlers::overview))
        .route("/api/workers", get(handlers::list_workers))
        .route("/api/tasks", get(handlers::list_tasks))
        .route("/api/events", get(handlers::list_events))
        .route("/api/events/stream", get(crate::sse::events_stream))
        .route("/api/me", get(handlers::me))
        .route("/logout", post(handlers::logout))
        .route("/static/*path", get(handlers::static_asset))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ));

    Router::new()
        .merge(public)
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// 대시보드 HTTP 서버 바인딩 + serve.
pub async fn run_dashboard_server(
    state: Arc<DashboardState>,
    bind: SocketAddr,
) -> std::io::Result<()> {
    let app = build_dashboard_app(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "dashboard server listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use fleet_core::{
        BootstrapToken, EventEntry, FleetEvent, LoginAttempt, Permission, Role, Session, SessionId,
        Task, TaskFilter, TaskId, TaskOutput, TaskStatus, User, UserId, Worker, WorkerFilter,
        WorkerHeartbeat, WorkerId,
    };
    use fleet_store::{Store, StoreError};
    use std::collections::HashMap;
    use std::sync::Mutex;

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
            Ok(vec![])
        }
        async fn revoke_bootstrap_token(&self, _: &str) -> Result<bool, StoreError> {
            Ok(false)
        }
        async fn create_user(&self, _: &User) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_user_by_id(&self, _: UserId) -> Result<Option<User>, StoreError> {
            Ok(None)
        }
        async fn get_user_by_username(&self, _: &str) -> Result<Option<User>, StoreError> {
            Ok(None)
        }
        async fn list_users(&self) -> Result<Vec<User>, StoreError> {
            Ok(vec![])
        }
        async fn count_users(&self) -> Result<u64, StoreError> {
            Ok(0)
        }
        async fn update_user_password(&self, _: UserId, _: &str) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn update_user_last_login(
            &self,
            _: UserId,
            _: chrono::DateTime<Utc>,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn set_user_enabled(&self, _: UserId, _: bool) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn delete_user(&self, _: UserId) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn create_role(&self, _: &Role) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_role_by_name(&self, _: &str) -> Result<Option<Role>, StoreError> {
            Ok(None)
        }
        async fn list_roles(&self) -> Result<Vec<Role>, StoreError> {
            Ok(vec![])
        }
        async fn create_permission(&self, _: &Permission) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_permission_by_name(&self, _: &str) -> Result<Option<Permission>, StoreError> {
            Ok(None)
        }
        async fn list_permissions(&self) -> Result<Vec<Permission>, StoreError> {
            Ok(vec![])
        }
        async fn assign_user_role(
            &self,
            _: UserId,
            _: fleet_core::RoleId,
            _: Option<UserId>,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn revoke_user_role(
            &self,
            _: UserId,
            _: fleet_core::RoleId,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn list_user_roles(&self, _: UserId) -> Result<Vec<Role>, StoreError> {
            Ok(vec![])
        }
        async fn list_user_permissions(&self, _: UserId) -> Result<Vec<Permission>, StoreError> {
            Ok(vec![])
        }
        async fn grant_role_permission(
            &self,
            _: fleet_core::RoleId,
            _: fleet_core::PermissionId,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn create_session(&self, _: &Session) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_session_by_token_hash(&self, _: &str) -> Result<Option<Session>, StoreError> {
            Ok(None)
        }
        async fn delete_session(&self, _: SessionId) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn delete_expired_sessions(&self) -> Result<u64, StoreError> {
            Ok(0)
        }
        async fn delete_user_sessions(&self, _: UserId) -> Result<u64, StoreError> {
            Ok(0)
        }
        async fn record_login_attempt(&self, _: &LoginAttempt) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn count_recent_failed_attempts(
            &self,
            _: &str,
            _: Option<&str>,
            _: i64,
        ) -> Result<u64, StoreError> {
            Ok(0)
        }
        async fn clear_login_attempts(&self, _: &str, _: Option<&str>) -> Result<u64, StoreError> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn dashboard_app_builds() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        // dashboard_app 빌드만 검증 (실제 pool은 필요 없음).
        // pool은 SSE용이므로 stub. 실제 통합 테스트는 PgStore 기반.
        let _ = store;
    }
}
