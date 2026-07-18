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

pub mod breaker;
pub mod dispatcher;
pub mod health;
pub mod selector;
pub mod state;
pub mod sync;

pub use breaker::{BreakerRegistry, BreakerState};
pub use dispatcher::{CancelError, DispatchError, Dispatcher, WaitError};
pub use health::{HealthChecker, HealthCheckerHandle, HealthConfig};
pub use selector::{SelectionError, WorkerSelector};
pub use state::FleetState;
pub use sync::MultiAdminSync;
