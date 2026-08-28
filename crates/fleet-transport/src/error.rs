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

    /// 요청 자체가 유효하지 않아 워커로 보내지 않았다 (로드맵 #69).
    ///
    /// 위의 다른 variant들과 성격이 다르다: 워커·연결·용량은 모두 **워커의
    /// 상태**를 말하지만 이것은 **요청의 상태**를 말한다. 그래서 호출자는 이
    /// 실패를 워커 건강도의 근거로 삼으면 안 된다 — 특히 circuit breaker에
    /// `Failure`로 기록하면, 클라이언트가 잘못된 경로를 반복 제출하는 것만으로
    /// 멀쩡한 워커의 회로를 열 수 있다.
    #[error("dispatch request is invalid and was not sent to the worker: {0}")]
    InvalidRequest(String),
}
