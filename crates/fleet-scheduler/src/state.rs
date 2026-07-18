//! 오케스트레이터 전체 상태를 캡슐화.
//!
//! `FleetState`는 모든 크레이트가 공유하는 중앙 의존성 컨테이너로,
//! Store + Transport + BreakerRegistry + Selector를 함께 들고 있습니다.
//! MCP 핸들러와 Dispatcher가 이를 참조합니다.

use std::sync::Arc;

use crate::breaker::BreakerRegistry;
use crate::selector::WorkerSelector;
use fleet_core::CircuitBreakerConfig;
use fleet_store::Store;
use fleet_transport::WorkerTransport;

/// 오케스트레이터 전역 상태. `Arc<FleetState>`로 모든 핸들러에 공유.
pub struct FleetState {
    pub store: Arc<dyn Store>,
    pub transport: Arc<dyn WorkerTransport>,
    pub breakers: Arc<BreakerRegistry>,
    pub selector: WorkerSelector,
}

impl FleetState {
    /// 모든 구성 요소를 주입받아 생성.
    pub fn new(
        store: Arc<dyn Store>,
        transport: Arc<dyn WorkerTransport>,
        breaker_config: CircuitBreakerConfig,
    ) -> Self {
        let breakers = Arc::new(BreakerRegistry::new(breaker_config));
        let selector = WorkerSelector::new(store.clone(), breakers.clone());
        Self {
            store,
            transport,
            breakers,
            selector,
        }
    }
}
