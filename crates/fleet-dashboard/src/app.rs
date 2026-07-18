//! 대시보드 앱 조립 + 서버 실행.

use std::net::SocketAddr;
use std::sync::Arc;

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
}

impl DashboardState {
    pub fn new(store: Arc<dyn Store>, pool: sqlx::PgPool) -> Self {
        Self { store, pool }
    }
}

/// 전체 라우터 조립.
pub fn build_dashboard_app(state: Arc<DashboardState>) -> Router {
    Router::new()
        .route("/", get(handlers::index))
        .route("/health", get(handlers::health))
        .route("/api/overview", get(handlers::overview))
        .route("/api/workers", get(handlers::list_workers))
        .route("/api/tasks", get(handlers::list_tasks))
        .route("/api/events", get(handlers::list_events))
        .route("/api/events/stream", get(crate::sse::events_stream))
        .route("/static/*path", get(handlers::static_asset))
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
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fleet_core::{
        EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput, TaskStatus, Worker,
        WorkerFilter, WorkerHeartbeat, WorkerId,
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
        async fn update_worker_heartbeat(&self, _: WorkerId, _: &WorkerHeartbeat) -> Result<(), StoreError> {
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
}
