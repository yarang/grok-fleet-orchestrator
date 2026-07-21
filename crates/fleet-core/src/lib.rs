//! # fleet-core
//!
//! Grok Fleet Orchestrator의 도메인 모델 크레이트.
//!
//! 의존성이 없는 leaf 크레이트로, 오케스트레이터의 모든 크레이트가 공유하는
//! 타입 정의만 포함합니다:
//!
//! - [`task`] — 작업(Task), 상태, 결과, 필터
//! - [`worker`] — 워커(Worker), 상태, CircuitBreaker 상태
//! - [`events`] — 상태 변화 이벤트 (append-only 로그용)
//! - [`ids`] — 타입 안전 식별자 (`TaskId`, `WorkerId`)
//! - [`config`] — 오케스트레이터/워커 설정 타입
//! - [`error`] — 도메인 에러 타입
//!
//! ## 설계 원칙
//!
//! 1. **의존성 없음**: 이 크레이트는 외부 인프라(sqlx, russh, tokio 등)에
//!    의존하지 않습니다. 다른 fleet-* 크레이트가 이 타입들을 공유합니다.
//! 2. **직렬화 친화적**: 모든 타입은 `serde` 직렬화를 지원하며, JSON 객체는
//!    내부적으로 `tag` 필드로 변별 가능합니다 (이벤트 로그, Postgres JSONB와 호환).
//! 3. **상태머신 명확성**: `TaskStatus`는 허용된 상태 전이만 표현합니다.

#![forbid(unsafe_code)]
// TODO(0.1.0): 출시 전 모든 public 필드/variant에 doc comment 추가 후
// `#![warn(missing_docs)]`로 전환. 현재는 API가 변동 중이므로 임시 허용.
#![allow(missing_docs)]

pub mod auth;
pub mod bootstrap_token;
pub mod config;
pub mod error;
pub mod events;
pub mod ids;
pub mod task;
pub mod worker;

// 주요 타입 re-export (fleet_core::Task 등으로 접근 가능)
pub use auth::{
    password, AuthError, BootstrapPurpose, BuiltinRole, LoginAttempt, Permission, PermissionId,
    PermissionKind, Role, RoleId, RolePermission, Session, SessionId, User, UserId, UserRole,
};
pub use bootstrap_token::BootstrapToken;
pub use config::{
    ApiConfig, CircuitBreakerConfig, OidcConfig, OrchestratorConfig, SchedulerConfig,
    StaticWorkerConfig, StoreConfig, WorkerSidecarConfig,
};
pub use error::{FleetError, Result, SelectionError};
pub use events::{EventEntry, FleetEvent};
pub use ids::{TaskId, WorkerId};
pub use task::{
    FailureKind, Labels, Task, TaskFailure, TaskFilter, TaskOutput, TaskOutputChunk, TaskPriority,
    TaskRequest, TaskResult, TaskStatus, TaskStatusFilter, TokenUsage,
};
pub use worker::{CircuitState, Worker, WorkerFilter, WorkerHeartbeat, WorkerStatus};
