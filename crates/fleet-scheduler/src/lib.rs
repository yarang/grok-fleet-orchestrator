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

// FLEET NOTE (2026-08-13): `autonomic` 모듈(MAPE-K 자가치유 엔진)은 삭제되었다.
// 커밋되지 않은 미완성 상태였고(`Worker.metrics` 존재하지 않는 필드,
// `FleetEvent::WorkerLeft`/`BreakerRegistry::get` 시그니처 불일치로 컴파일 불가),
// 재연결에는 하드웨어 메트릭 저장 위치(`hosts` 테이블 join 또는 `Worker`
// 필드 추가)부터 설계해야 하는 별도 기능 개발이 필요해 단순 타입 수정 범위를
// 넘어섰다. 설계 의도는 `docs/architecture/overview.md`의 "Autonomic
// Self-Healing Engine" 절에 보존되어 있으며, git 이력(이 커밋 이전)에서 원본
// 코드를 그대로 복원할 수 있다. 재구현 시 참고: `docs/roadmap/roadmap.md` #43.
pub mod breaker;
pub mod cleanup;
pub mod dispatcher;
pub mod health;
pub mod reconcile;
pub mod router;
pub mod selector;
pub mod skill_loader;
pub mod state;
pub mod sync;

pub use breaker::{BreakerRegistry, BreakerState};
pub use cleanup::{CleanupConfig, CleanupSummary, SessionCleanup, SessionCleanupHandle};
pub use dispatcher::{CancelError, DispatchError, Dispatcher, WaitError};
pub use health::{HealthChecker, HealthCheckerHandle, HealthConfig};
pub use reconcile::{ReconcileConfig, ReconcileSummary, Reconciler, ReconcilerHandle};
pub use router::{HeuristicTaskRouter, RoutingDecision, RoutingProfile, TaskRouter};
pub use selector::{SelectionError, WorkerSelector};
pub use state::FleetState;
pub use sync::MultiAdminSync;
