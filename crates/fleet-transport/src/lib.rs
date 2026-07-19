//! # fleet-transport
//!
//! 워커 통신 추상화 계층.
//!
//! `WorkerTransport` trait은 단일 워커로의 작업 디스패치/취소/출력 스트리밍을
//! 정의합니다. 이 trait에만 의존함으로써:
//!
//! - `fleet-scheduler`와 `fleet-mcp`는 통신 메커니즘(Hub SDK, 직접 SSH 등)을 몰라도 됨
//! - 테스트는 mock 구현으로 전체 플로우를 검증 가능
//! - grok-build의 `xai-computer-hub-sdk` 의존은 feature flag로 격리
//!
//! ## 구현체
//!
//! - [`MockTransport`] — 테스트/개발용 인메모리 구현
//! - (`hub` feature) `HubTransport` — `HubConnectionPool` 래핑 (Phase 3)

#![forbid(unsafe_code)]
#![allow(missing_docs)]

pub mod error;
pub mod mock;

#[cfg(feature = "acp")]
pub mod acp;
#[cfg(feature = "acp")]
pub mod acp_transport;

pub use error::TransportError;
pub use mock::{MockTransport, MockWorker};

#[cfg(feature = "acp")]
pub use acp_transport::AcpTransport;

use async_trait::async_trait;
use fleet_core::{TaskId, TaskResult, WorkerId};
use std::time::Duration;

/// 단일 워커로의 작업 실행 요청.
#[derive(Debug, Clone)]
pub struct DispatchRequest {
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub prompt: String,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub max_turns: Option<u32>,
    pub timeout_secs: Option<u64>,
}

/// 워커에서 발생하는 이벤트 (스트리밍).
#[derive(Debug, Clone)]
pub enum WorkerEvent {
    /// stdout/stderr 청크.
    Output {
        task_id: TaskId,
        seq: u64,
        chunk: String,
    },
    /// 작업 완료.
    Completed {
        task_id: TaskId,
        result: TaskResult,
    },
    /// 워커 측 에러 (작업은 실패로 처리됨).
    Failed {
        task_id: TaskId,
        error: String,
    },
}

/// 워커 통신 trait. 각 워커 엔드포인트당 하나의 인스턴스가 아닌,
/// 풀 전체를 관리하는 구현체를 가정합니다 (`register`/`unregister`).
#[async_trait]
pub trait WorkerTransport: Send + Sync {
    /// 워커를 풀에 등록. 이미 등록된 워커 ID면 에러.
    async fn register(&self, worker_id: WorkerId, endpoint: &str) -> Result<(), TransportError>;

    /// 워커를 풀에서 제거.
    async fn unregister(&self, worker_id: WorkerId) -> Result<(), TransportError>;

    /// 워커 연결 가능 여부 확인.
    async fn is_connected(&self, worker_id: WorkerId) -> bool;

    /// 작업을 워커에 디스패치. 완료를 기다리지 않고 즉시 반환.
    /// 결과는 `subscribe()`로 수신한 이벤트 스트림으로 전달.
    async fn dispatch(&self, req: DispatchRequest) -> Result<(), TransportError>;

    /// 진행 중인 작업을 취소.
    async fn cancel(&self, task_id: TaskId) -> Result<(), TransportError>;

    /// 워커 연결을 테스트 (헬스체크용).
    async fn ping(&self, worker_id: WorkerId) -> Result<Duration, TransportError>;

    /// 워커 이벤트 스트림을 구독.
    ///
    /// Dispatcher는 시작 시 1회 호출하여 receiver를 얻고,
    /// 이후 백그라운드 루프에서 이벤트를 소비합니다.
    /// 구현체는 내부적으로 broadcast 채널을 운영하며, 호출 시마다
    /// 새로운 receiver를 반환합니다 (멀티 구독자 지원).
    async fn subscribe(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<WorkerEvent>, TransportError>;
}
