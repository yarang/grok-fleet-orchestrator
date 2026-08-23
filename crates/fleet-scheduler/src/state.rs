//! 오케스트레이터 전체 상태를 캡슐화.
//!
//! `FleetState`는 모든 크레이트가 공유하는 중앙 의존성 컨테이너로,
//! Store + Transport + BreakerRegistry + Selector를 함께 들고 있습니다.
//! MCP 핸들러와 Dispatcher가 이를 참조합니다.

use std::sync::Arc;

use crate::breaker::BreakerRegistry;
use crate::lease::LeaseObserver;
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
    /// Control plane lease 관측 (로드맵 #63 2단계). `None`이면 이 배포가
    /// HA lease를 켜지 않은 것 — 기존 단일 인스턴스 배포와 동일하게 항상
    /// control 동작을 허용한다(`lease_allows_control`이 `true`).
    pub lease: Option<LeaseObserver>,
}

impl FleetState {
    /// 모든 구성 요소를 주입받아 생성. lease는 기본 미설정(`with_lease`로
    /// 나중에 추가) — 기존 호출부를 깨지 않기 위한 builder 패턴.
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
            lease: None,
        }
    }

    /// Control plane lease 관측을 연결한다(로드맵 #63 2단계). HA 배포에서
    /// `fleet-cli::run_serve`가 `LeaseManager`를 spawn한 뒤 호출한다.
    pub fn with_lease(mut self, lease: LeaseObserver) -> Self {
        self.lease = Some(lease);
        self
    }

    /// 지금 신규 control-plane 동작(dispatch/cancel/breaker 변경)을
    /// 수행해도 되는지 (로드맵 #63 불변식 2). `lease`가 설정되지 않은
    /// 배포는 항상 `true` — HA lease를 켜지 않은 기존 단일 인스턴스
    /// 배포와 호환.
    pub fn lease_allows_control(&self) -> bool {
        match &self.lease {
            Some(lease) => lease.allows_control(),
            None => true,
        }
    }
}
