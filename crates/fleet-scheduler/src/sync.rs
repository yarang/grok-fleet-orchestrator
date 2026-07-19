//! 다중 admin 동기화 코디네이터.
//!
//! Postgres LISTEN/NOTIFY로 수신한 이벤트를 해석하여, 로컬 인메모리 상태
//! (CircuitBreakerRegistry 등)를 다른 admin이 만든 변경에 맞춥니다.
//!
//! ## 동기화 대상
//!
//! | 이벤트 | 동기화 동작 |
//! |--------|-------------|
//! | `WorkerCircuitChanged` (to=Open) | 로컬 브레이커를 Open으로 강제 |
//! | `WorkerCircuitChanged` (to=Closed) | 로컬 브레이커를 reset |
//! | `WorkerLeft` | 로컬 브레이커 reset (워커가 사라졌으니 정리) |
//! | `WorkerJoined` | 새 워커 — 아무 동작 X (BreakerRegistry는 지연 생성) |
//!
//! ## 설계 노트
//!
//! - Store 자체는 이미 다중 admin에서 일관적 (동일한 Postgres).
//! - selector는 매 `select()` 호출마다 Store를 조회하므로 워커 목록은 자동 동기화됨.
//! - 유일한 인메모리 상태가 `BreakerRegistry`이며, 여기만 동기화하면 됨.
//! - Dispatch/Cancel 자체는 stateless: Store에 저장된 task status가 진실.

use std::sync::Arc;

use futures::StreamExt;
use tracing::{debug, info, warn};

use fleet_core::{CircuitState, EventEntry, FleetEvent};
use fleet_store::listen_events;
use sqlx::PgPool;

use crate::state::FleetState;

/// 다중 admin 동기화 코디네이터. spawn하면 백그라운드 루프를 실행.
pub struct MultiAdminSync {
    state: Arc<FleetState>,
    pool: PgPool,
}

impl MultiAdminSync {
    pub fn new(state: Arc<FleetState>, pool: PgPool) -> Self {
        Self { state, pool }
    }

    /// 백그라운드 동기화 루프 진입. 보통 `tokio::spawn`으로 감쌈.
    ///
    /// 에러가 나도 재시도하며 영구 실행.
    pub async fn run(self) {
        info!("multi-admin sync coordinator starting");

        loop {
            match self.run_once().await {
                Ok(()) => {
                    // 스트림이 종료되는 경우는 거의 없지만, 안전하게 재연결
                    warn!("event stream ended, reconnecting in 1s");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                Err(e) => {
                    warn!(error = %e, "event stream error, reconnecting in 2s");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }

    async fn run_once(&self) -> Result<(), fleet_store::StoreError> {
        // store 참조를 빌려 스트림 생성. 스트림은 'static이 아니므로
        // 이 함수 안에서 소비해야 함.
        let stream = listen_events(self.state.store.as_ref(), &self.pool).await?;

        futures::pin_mut!(stream);
        while let Some(events) = stream.next().await {
            self.apply_events(events).await;
        }
        Ok(())
    }

    /// 수신한 이벤트 배치를 로컬 상태에 반영.
    pub async fn apply_events(&self, events: Vec<EventEntry>) {
        for entry in events {
            let EventEntry { seq, event } = entry;
            let applied = Self::apply_one_to(&self.state, &event).await;
            if applied {
                debug!(seq, event_type = event.event_type(), "synced event");
            }
        }
    }

    /// 단일 이벤트 적용 (정적 버전).
    /// `apply_events`에서 호출됨. PgPool 없이 FleetState만으로 테스트 가능.
    pub async fn apply_one_to(state: &FleetState, event: &FleetEvent) -> bool {
        match event {
            FleetEvent::WorkerCircuitChanged {
                worker_id,
                from: _,
                to,
                ..
            } => {
                let cb = state.breakers.get(*worker_id);
                match to {
                    CircuitState::Open => {
                        // 다른 admin이 Open시킨 것을 로컬에 반영
                        cb.force_open();
                        debug!(%worker_id, "breaker synced to Open");
                    }
                    CircuitState::Closed => {
                        cb.reset();
                        debug!(%worker_id, "breaker synced to Closed");
                    }
                    CircuitState::HalfOpen => {
                        // HalfOpen은 자동 전이 상태 — 동기화하지 않음 (각 admin이 check() 시 자체 판단)
                    }
                }
                true
            }
            FleetEvent::WorkerLeft { worker_id, .. } => {
                // 워커가 등록 해제되었으면 로컬 브레이커도 초기화
                state.breakers.reset(*worker_id);
                debug!(%worker_id, "breaker reset on worker left");
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::breaker::BreakerState;
    use fleet_core::{
        CircuitBreakerConfig, CircuitState, TaskId, WorkerId,
    };

    fn make_state() -> Arc<FleetState> {
        let transport = fleet_transport::MockTransport::new();
        let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(transport);
        Arc::new(FleetState::new(
            // Store는 동기화 로직에서 사용되지 않음 — breaker만.
            // 하지만 FleetState::new는 Store를 요구하므로 dummy 사용.
            // 테스트에서는 sync.apply_one_to만 직접 호출.
            std::sync::Arc::new(crate::sync::tests::NoopStore) as Arc<dyn fleet_store::Store>,
            transport,
            CircuitBreakerConfig::default(),
        ))
    }

    /// 빈 Store 구현 (sync 테스트용).
    pub struct NoopStore;
    #[async_trait::async_trait]
    impl fleet_store::Store for NoopStore {
        async fn insert_task(&self, _: &fleet_core::Task) -> Result<(), fleet_store::StoreError> {
            unimplemented!()
        }
        async fn get_task(&self, _: TaskId) -> Result<Option<fleet_core::Task>, fleet_store::StoreError> {
            unimplemented!()
        }
        async fn update_task_status(
            &self,
            _: TaskId,
            _: &fleet_core::TaskStatus,
        ) -> Result<(), fleet_store::StoreError> {
            unimplemented!()
        }
        async fn list_tasks(
            &self,
            _: &fleet_core::TaskFilter,
        ) -> Result<Vec<fleet_core::Task>, fleet_store::StoreError> {
            unimplemented!()
        }
        async fn upsert_worker(&self, _: &fleet_core::Worker) -> Result<(), fleet_store::StoreError> {
            unimplemented!()
        }
        async fn get_worker(&self, _: WorkerId) -> Result<Option<fleet_core::Worker>, fleet_store::StoreError> {
            unimplemented!()
        }
        async fn get_worker_by_name(
            &self,
            _: &str,
        ) -> Result<Option<fleet_core::Worker>, fleet_store::StoreError> {
            unimplemented!()
        }
        async fn list_workers(
            &self,
            _: &fleet_core::WorkerFilter,
        ) -> Result<Vec<fleet_core::Worker>, fleet_store::StoreError> {
            Ok(vec![])
        }
        async fn delete_worker(&self, _: WorkerId) -> Result<(), fleet_store::StoreError> {
            unimplemented!()
        }
        async fn update_worker_heartbeat(
            &self,
            _: WorkerId,
            _: &fleet_core::WorkerHeartbeat,
        ) -> Result<(), fleet_store::StoreError> {
            unimplemented!()
        }
        async fn append_event(
            &self,
            _: &FleetEvent,
        ) -> Result<u64, fleet_store::StoreError> {
            unimplemented!()
        }
        async fn list_events(
            &self,
            _: u64,
            _: u32,
        ) -> Result<Vec<EventEntry>, fleet_store::StoreError> {
            Ok(vec![])
        }
        async fn append_output(&self, _: TaskId, _: &str) -> Result<u64, fleet_store::StoreError> {
            unimplemented!()
        }
        async fn get_output(
            &self,
            _: TaskId,
            _: u64,
        ) -> Result<fleet_core::TaskOutput, fleet_store::StoreError> {
            unimplemented!()
        }
        async fn migrate(&self) -> Result<(), fleet_store::StoreError> {
            Ok(())
        }
        async fn create_bootstrap_token(
            &self,
            _: &fleet_core::BootstrapToken,
        ) -> Result<(), fleet_store::StoreError> {
            unimplemented!()
        }
        async fn consume_bootstrap_token(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(), fleet_store::StoreError> {
            unimplemented!()
        }
        async fn list_bootstrap_tokens(
            &self,
        ) -> Result<Vec<fleet_core::BootstrapToken>, fleet_store::StoreError> {
            unimplemented!()
        }
        async fn revoke_bootstrap_token(
            &self,
            _: &str,
        ) -> Result<bool, fleet_store::StoreError> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn circuit_open_event_forces_local_breaker_open() {
        let state = make_state();
        let worker_id = WorkerId::new();

        // 로컬 브레이커는 기본 Closed
        assert_eq!(state.breakers.state_of(worker_id), BreakerState::Closed);

        let event = FleetEvent::WorkerCircuitChanged {
            worker_id,
            from: CircuitState::Closed,
            to: CircuitState::Open,
            at: chrono::Utc::now(),
        };
        let applied = MultiAdminSync::apply_one_to(&state, &event).await;
        assert!(applied);

        // 로컬 브레이커가 Open으로 동기화되어야 함
        assert_eq!(state.breakers.state_of(worker_id), BreakerState::Open);
    }

    #[tokio::test]
    async fn circuit_close_event_resets_local_breaker() {
        let state = make_state();
        let worker_id = WorkerId::new();

        // 먼저 Open 설정
        state.breakers.get(worker_id).force_open();
        assert_eq!(state.breakers.state_of(worker_id), BreakerState::Open);

        let event = FleetEvent::WorkerCircuitChanged {
            worker_id,
            from: CircuitState::Open,
            to: CircuitState::Closed,
            at: chrono::Utc::now(),
        };
        MultiAdminSync::apply_one_to(&state, &event).await;

        assert_eq!(state.breakers.state_of(worker_id), BreakerState::Closed);
    }

    #[tokio::test]
    async fn worker_left_resets_breaker() {
        let state = make_state();
        let worker_id = WorkerId::new();
        state.breakers.get(worker_id).force_open();

        let event = FleetEvent::worker_left(worker_id, "test");
        let applied = MultiAdminSync::apply_one_to(&state, &event).await;
        assert!(applied);

        assert_eq!(state.breakers.state_of(worker_id), BreakerState::Closed);
    }

    #[tokio::test]
    async fn non_sync_events_return_false() {
        let state = make_state();
        // TaskCreated, WorkerJoined 등은 동기화 대상 아님
        let event = FleetEvent::task_created(TaskId::new(), None, "test");
        let applied = MultiAdminSync::apply_one_to(&state, &event).await;
        assert!(!applied);
    }
}
