//! Transport 계층 에러 타입.

use thiserror::Error;

/// `fleet-transport` 작업 중 발생하는 에러.
#[derive(Debug, Error)]
pub enum TransportError {
    /// 워커가 풀에 등록되어 있지 않음.
    #[error("worker {0} is not registered")]
    WorkerNotRegistered(String),

    /// 워커 연결 실패 또는 끊김.
    #[error("worker connection error: {0}")]
    Connection(String),

    /// 워커가 시간 내 응답하지 않음.
    #[error("worker timeout after {0:?}")]
    Timeout(std::time::Duration),

    /// 워커 측 에러 (exit ≠ 0, panic 등).
    #[error("worker error: {0}")]
    WorkerError(String),

    /// 중복 등록.
    #[error("worker {0} already registered")]
    AlreadyRegistered(String),

    /// 워커가 동시 작업 상한에 도달해 추가 dispatch 불가.
    #[error("worker {0} is at capacity (max_concurrent_tasks reached)")]
    WorkerAtCapacity(String),

    /// 인증 실패.
    #[error("authentication failed: {0}")]
    Auth(String),
}
