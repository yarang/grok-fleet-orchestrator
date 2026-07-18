//! 오케스트레이터 전역 에러 타입.
//!
//! `fleet-core`는 leaf 크레이트이므로 외부 크레이트 의존성이 있는 에러
//! (예: `sqlx::Error`, `russh::Error`)는 여기서 다루지 않습니다. 각 크레이트는
//! 자신의 로컬 에러 타입에서 `FleetError`로 변환(`From`)을 제공합니다.

use thiserror::Error;

use crate::ids::{TaskId, WorkerId};

/// `fleet-core` 도메인 계층에서 발생하는 에러.
#[derive(Debug, Error)]
pub enum FleetError {
    #[error("task not found: {0}")]
    TaskNotFound(TaskId),

    #[error("worker not found: {0}")]
    WorkerNotFound(WorkerId),

    #[error("worker already registered with name: {0}")]
    DuplicateWorkerName(String),

    #[error("no worker matches the requested labels/hint")]
    NoMatchingWorker,

    #[error("hinted worker '{0}' exists but is unavailable (offline or circuit open)")]
    HintedWorkerUnavailable(String),

    #[error("worker circuit is open: {0}")]
    CircuitOpen(WorkerId),

    #[error("task already in terminal state: {0}")]
    TaskAlreadyTerminal(TaskId),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// 워커 선택 단계에서 발생하는 에러. `FleetError`의 부분집합이지만
/// 호출자가 "선택 실패"와 "런타임 실패"를 구분할 수 있게 별도 타입으로 둡니다.
#[derive(Debug, Error)]
pub enum SelectionError {
    #[error("no online worker matches required labels")]
    NoMatchingWorker,

    #[error("hinted worker '{0}' is offline or circuit-open (not falling back, per user intent)")]
    HintedWorkerUnavailable(String),

    #[error("no worker is currently online")]
    AllWorkersOffline,
}

impl From<SelectionError> for FleetError {
    fn from(e: SelectionError) -> Self {
        match e {
            SelectionError::NoMatchingWorker => FleetError::NoMatchingWorker,
            SelectionError::HintedWorkerUnavailable(n) => FleetError::HintedWorkerUnavailable(n),
            SelectionError::AllWorkersOffline => FleetError::NoMatchingWorker,
        }
    }
}

/// `Result` alias for fleet-core operations.
pub type Result<T, E = FleetError> = std::result::Result<T, E>;
