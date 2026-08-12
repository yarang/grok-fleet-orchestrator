//! # fleet-scheduler
//!
//! 작업 스케줄링, 워커 선택, 장애 격리를 담당합니다.
//!
//! ## 핵심 컴포넌트
//!
//! - [`WorkerSelector`] — 라벨 매칭 + `server_hint` 우선 + least-loaded 폴백
//! - [`BreakerRegistry`] — 워커별 CircuitBreaker (3상태 머신)
//! - [`Dispatcher`] — 작업을 비동기로 실행하고 상태를 Store에 반영
//! - [`FleetState`] — 오케스트레이터 전체 상태 (Store + Transport + Breakers)

#![forbid(unsafe_code)]
#![allow(missing_docs)]

// FLEET NOTE (2026-08-12): `autonomic` 모듈은 커밋되지 않은 미완성 상태로
// 발견됨 — `Worker.metrics`(존재하지 않는 필드), `FleetEvent::WorkerLeft`의
// `id`/`name`(실제 변형은 `worker_id`/`at`만 가짐), `BreakerRegistry::get`
// 시그니처(`&str` 하나가 아니라 `WorkerId, CircuitState` 두 인자)가 모두
// 현재 타입과 어긋나 컴파일이 안 된다. 이 세션의 스레드 기능과는 무관하고
// 의도를 알 수 없어 직접 고치지 않고, 빌드를 막지 않도록 모듈 연결만
// 잠시 해제해둔다 — 파일 자체(`autonomic.rs`)는 지우지 않았다.
// pub mod autonomic;
pub mod breaker;
pub mod cleanup;
pub mod dispatcher;
pub mod health;
pub mod reconcile;
pub mod selector;
pub mod state;
pub mod sync;

// pub use autonomic::{AutonomicConfig, AutonomicEngine, AutonomicEngineHandle};
pub use breaker::{BreakerRegistry, BreakerState};
pub use cleanup::{CleanupConfig, CleanupSummary, SessionCleanup, SessionCleanupHandle};
pub use dispatcher::{CancelError, DispatchError, Dispatcher, WaitError};
pub use health::{HealthChecker, HealthCheckerHandle, HealthConfig};
pub use reconcile::{ReconcileConfig, ReconcileSummary, Reconciler, ReconcilerHandle};
pub use selector::{SelectionError, WorkerSelector};
pub use state::FleetState;
pub use sync::MultiAdminSync;
