//! 오케스트레이터 전체 상태를 캡슐화.
//!
//! `FleetState`는 모든 크레이트가 공유하는 중앙 의존성 컨테이너로,
//! Store + Transport + BreakerRegistry + Selector를 함께 들고 있습니다.
//! MCP 핸들러와 Dispatcher가 이를 참조합니다.

use std::sync::Arc;

use crate::breaker::BreakerRegistry;
use crate::lease::{LeaseObserver, LeaseStatus};
use crate::selector::WorkerSelector;
use fleet_core::CircuitBreakerConfig;
use fleet_store::{ControlFence, Store};
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
        let selector = WorkerSelector::new(store.clone(), breakers.clone(), transport.clone());
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

    /// Task 상태 쓰기에 함께 걸 control-plane epoch 술어 (로드맵 #62 3단계).
    ///
    /// `lease`가 설정되지 않은 배포는 `None`이다. `lease_allows_control`이
    /// 같은 경우에 `true`를 돌려주는 것과 짝을 이룬다 — HA lease를 켜지 않은
    /// 단일 인스턴스 배포에는 fence로 걸 epoch 자체가 없고, 그 배포에서
    /// 제어권을 다투는 상대도 없다. lease를 켠 배포에서만 술어가 붙는다.
    pub fn control_fence(&self) -> Option<ControlFence> {
        self.lease.as_ref().and_then(|lease| lease.fence())
    }

    /// 지금 리스 상태의 짧은 이름 (로드맵 `#70` 게이트 ⑥ 선행).
    ///
    /// 감사 `detail`에 싣는 값이라 **닫힌 어휘**여야 한다. `Debug` 출력을
    /// 그대로 쓰면 `Active { epoch: 7 }`처럼 값이 섞여 들어가 필드로 질의할
    /// 수 없게 되고, epoch는 이미 전용 컬럼이 있다.
    pub fn lease_status_label(&self) -> &'static str {
        match self.lease.as_ref().map(|l| l.status()) {
            None => "no_lease_configured",
            Some(LeaseStatus::Active { .. }) => "active",
            Some(LeaseStatus::Fenced) => "fenced",
            Some(LeaseStatus::Stopped) => "stopped",
        }
    }

    /// 제어면 결정을 감사에 남긴다 (로드맵 `#70` 게이트 ⑥ 선행).
    ///
    /// **이 함수가 있기 전까지 `fleet-scheduler`는 감사 기록을 하나도 내지
    /// 않았다**(2026-09-06 실측: 감사 emitter 35군데가 전부 `fleet-api`·
    /// `fleet-dashboard`·`fleet-core`). 즉 dispatch·펜싱 같은 제어면 결정은
    /// `warn!` 로그로만 남았고, 관측성 정본의 alert 표가 요구하는
    /// "fencing/epoch 증거 확인"은 조회할 원천이 없었다.
    ///
    /// 행위자는 사람이 아니라 **이 인스턴스**다. `cluster_id`가 아니라
    /// `instance_id`를 쓰는 이유는 그 표가 요구하는 증거가 "둘 이상의 owner
    /// 관측"이고, 같은 cluster의 두 인스턴스는 cluster_id로 구분되지 않기
    /// 때문이다.
    ///
    /// **리스가 설정되지 않은 배포에서는 아무것도 남기지 않는다.** 단일
    /// 인스턴스 배포에는 세대도 경합도 없으므로 남길 사실이 없다 —
    /// `lease_allows_control()`이 그 경우 항상 `true`인 것과 같은 이유다.
    pub async fn audit_control(
        &self,
        action: &str,
        target: (&str, String),
        detail: serde_json::Value,
    ) {
        let Some(lease) = self.lease.as_ref() else {
            return;
        };
        let mut event = fleet_core::AuditEvent::failure(
            format!("orchestrator:{}", lease.instance_id()),
            action,
        )
        .target(target.0, target.1)
        .detail(detail);
        // `None`은 "정보 없음"이 아니라 "이 인스턴스에 세대가 없었다"이며,
        // 그것 자체가 증거다 — `AuditEvent::control_epoch`의 독스트링 참고.
        if let Some(epoch) = lease.status().epoch() {
            event = event.control_epoch(epoch);
        }
        if let Err(e) = self.store.record_audit_event(&event).await {
            // 감사 실패로 제어 흐름을 바꾸지 않는다. 다만 조용히 넘기지도
            // 않는다 — 이 기록이 없으면 위 alert의 조사 자체가 성립하지 않는다.
            tracing::warn!(target: "fleet::control", action, error = %e,
                "failed to record a control-plane audit event");
        }
    }
}
