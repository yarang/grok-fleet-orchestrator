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
//!
//! ## 동시성 모델 (Phase 8.4, 2026-08-11 재설계)
//!
//! 각 워커는 `max_concurrent_tasks`개의 동시 작업을 처리할 수 있습니다.
//! - `dispatch(req)`는 워커의 활성 작업 수가 상한에 도달한 경우 즉시
//!   `TransportError::WorkerAtCapacity`를 반환합니다 (dispatch 큐잉 없음 —
//!   Selector가 사전에 필터링함).
//! - 동시 태스크는 **태스크마다 새 ACP 세션**을 열어 병렬로 실행됩니다(워커당
//!   WebSocket 연결은 하나 공유). 실제 grok가 신뢰할 수 있는 `promptId`
//!   상관관계를 제공하지 않아, 세션 단위 분리로 대체 — ACP 스펙상
//!   `session/update`가 원래 갖고 있는 `session_id`로 완전히 모호함 없이
//!   라우팅된다. 상세는 `acp_transport` 모듈 문서 참고.
//! - `Output` / `Completed` / `Failed` 이벤트는 `session_id`를 통해
//!   올바른 `task_id`로 라우팅됩니다.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

pub mod error;
pub mod mock;

#[cfg(feature = "acp")]
pub mod acp_transport;
#[cfg(feature = "mtls")]
pub mod mtls_proxy;
#[cfg(feature = "mtls")]
pub mod tls;

pub use error::TransportError;
pub use mock::{MockTransport, MockWorker};

#[cfg(feature = "acp")]
pub use acp_transport::{AcpTransport, ConnState, ReconnectConfig};
#[cfg(feature = "mtls")]
pub use mtls_proxy::{MtlsProxy, ProxyError};
#[cfg(feature = "mtls")]
pub use tls::{ClientTlsConfig, RotatingCertResolver, ServerTlsConfig, TlsError};

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
    pub checkpoint_branch: Option<String>,
    /// 에이전트 스킬 로더: 태스크 명세의 `skills_required` 목록.
    /// 비어있으면 스킬 인젝션 없음.
    pub skills_required: Vec<String>,
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
    Completed { task_id: TaskId, result: TaskResult },
    /// 작업이 실패로 처리됨. `observation`이 **그 판단의 근거가 어디까지인지**를
    /// 함께 싣는다 — 이 필드가 없던 동안 dispatcher는 여섯 생성 지점(테스트 double
    /// 포함) 전부를 `FailureKind::WorkerError`로 못 박았고, 그중 **셋**은 워커가
    /// 실패를 보고한 적이 없었다: 연결 상실 시의 `fail_all()`, `session/new`
    /// 타임아웃, `session/prompt` 타임아웃. 그 셋이 전부 같은 곳으로 가지는
    /// 않는다 — `session/new` 타임아웃은 프롬프트가 전달되기 전이라 "실행이
    /// 시작되지 않았다"가 **확정**이고, 나머지 둘만 결과가 불확실하다.
    Failed {
        task_id: TaskId,
        error: String,
        observation: FailureObservation,
    },
}

/// 실패를 **어떻게 관측했는가**. `WorkerEvent::Failed`가 한 덩어리로 싣던 의미를
/// 셋으로 가른다.
///
/// 가르는 기준은 심각도가 아니라 **오케스트레이터가 아는 것의 범위**이며,
/// 두 질문을 이 **순서대로** 묻는다.
///
/// 1. 워커에게서 답이 왔는가? 왔다면 `Reported`다 — 그 답이 세션 생성 거부든
///    실행 중 실패든, 결과는 확정이다.
/// 2. 답이 없다면, 프롬프트가 전달됐는가? 전달 전이면 `NotDelivered`,
///    전달 후면 `ResultLost`다.
///
/// **순서가 중요하다.** 두 질문을 대등한 축으로 읽으면 `session/new`가 에러를
/// 응답한 경우 — 답은 왔고 프롬프트는 가지 않았다 — 가 어디로 가야 할지 정해지지
/// 않는다. 1번이 먼저인 이유는 답이 온 순간 2번이 무의미해지기 때문이다: 결과가
/// 확정된 마당에 프롬프트가 어디까지 갔는지는 오케스트레이터의 판단을 바꾸지
/// 않는다. 2번은 답이 없을 때에만, 그 무지의 범위를 좁히려고 묻는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureObservation {
    /// 워커가 실패를 응답으로 보고했다. 결과는 확정이다. 실행 중 실패든 세션
    /// 생성 거부든 이쪽이다 — 어느 쪽이든 워커는 살아 있고, 답을 줬다.
    Reported,
    /// 워커가 **응답하지 않아** 프롬프트가 전달되지 못했다. 실행은 시작되지
    /// 않았다 — 이것도 확정이다.
    NotDelivered,
    /// 프롬프트는 전달됐으나 응답을 듣지 못했다. 워커에서 아직 실행 중일 수도,
    /// 이미 끝났을 수도 있다. **확정이 아닌 유일한 variant**다.
    ResultLost,
}

/// 워커 통신 trait. 각 워커 엔드포인트당 하나의 인스턴스가 아닌,
/// 풀 전체를 관리하는 구현체를 가정합니다 (`register`/`unregister`).
#[async_trait]
pub trait WorkerTransport: Send + Sync {
    /// 워커를 풀에 등록. 이미 등록된 워커 ID면 에러.
    ///
    /// `max_concurrent_tasks`는 이 워커가 동시에 처리할 수 있는 상한.
    /// 0은 허용하지 않음 (에러). 1은 직렬 처리, N≥2는 동시 다중 세션.
    /// transport는 dispatch 시 이 값을 검사하여 `WorkerAtCapacity` 에러를 반환.
    async fn register(
        &self,
        worker_id: WorkerId,
        endpoint: &str,
        max_concurrent_tasks: u32,
    ) -> Result<(), TransportError>;

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
