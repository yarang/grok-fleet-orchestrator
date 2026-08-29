//! `AcpTransport` — `WorkerTransport` trait의 ACP 구현체.
//!
//! 2026-08-11: 손으로 짠 JSON-RPC/WebSocket 클라이언트(옛 `acp/` 모듈) 대신
//! 공식 [`agent-client-protocol`](https://github.com/agentclientprotocol/rust-sdk)
//! Rust SDK를 사용합니다. 배경: 실제 grok의 ACP wire-format을 잘못 추측한 버그
//! 3건(`session/update`의 태그 키가 `type`가 아니라 `sessionUpdate`, `promptId`가
//! 톱레벨이 아니라 `update._meta`에 문자열로 존재 등)을 하루 동안 실측으로 고친
//! 뒤, grok에 번들된 공식 문서가 이미 정확한 스펙을 담고 있었고 공식 SDK도
//! 존재한다는 것을 발견해 전환을 결정했습니다. SDK는
//! `vendor/agent-client-protocol-rust-sdk/`에 로컬 vendor되어 있으며, mTLS
//! WebSocket connector 주입을 위한 소규모 패치가 적용되어 있습니다
//! (`vendor/agent-client-protocol-rust-sdk/FLEET_PATCHES.md` 참고).
//!
//! ## 설계: task당 ACP 세션 하나
//!
//! 기존 구현은 워커당 세션 하나를 공유하고, 여러 태스크가 동시에 그 세션 위에서
//! `session/prompt`를 병렬 호출하며 `promptId`로 서로 구분했습니다. 하지만 실제
//! grok의 `session/update` notification은 (a) 신뢰할 수 있는 톱레벨 `promptId`를
//! 보내지 않고, (b) 있어도 문자열 UUID라 SDK의 `u64` 기반이 아닌 별도 상관관계
//! 체계가 필요했습니다.
//!
//! 이 구현은 대신 **디스패치된 태스크마다 새 ACP 세션을 만듭니다**
//! (`session/new`를 태스크당 1회 호출). `SessionNotification`은 ACP 스펙상
//! 원래 `session_id` 필드를 갖고 있으므로, 세션을 태스크 단위로 분리하면
//! 스트리밍 출력이 **항상, 모호함 없이** 올바른 태스크로 라우팅됩니다 — 예전의
//! "동시에 2개 이상 진행 중이면 라우팅 불가" 제약이 구조적으로 사라집니다.
//! 대가는 태스크당 `session/new` 왕복 1회 추가뿐입니다.
//!
//! ## 동시성 모델
//!
//! 워커당 WebSocket 연결은 하나([`ConnectionTo`]는 저비용 clone 가능 — SDK가
//! 명시적으로 보장). `max_concurrent_tasks`는 `tokio::sync::Semaphore`로 강제.
//! `dispatch()`는 permit을 즉시(non-blocking) 획득 시도하고, 실패 시
//! [`TransportError::WorkerAtCapacity`]를 반환합니다 (큐잉 없음).
//!
//! ## 재연결
//!
//! 각 워커는 supervisor 태스크를 가짐. supervisor는 연결 실패/종료 시
//! 진행 중인 모든 태스크를 `WorkerEvent::Failed`로 정리하고, 지수 백오프
//! (1s → 2s → ... → 최대 30s) 후 재연결을 시도합니다. `unregister()` 호출 시엔
//! 재연결하지 않고 깔끔하게 종료합니다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest,
    SessionId, SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{Agent, ConnectionTo};
use agent_client_protocol_http::HttpClient;
use async_trait::async_trait;
use chrono::Utc;
use fleet_core::{mask_server_key, split_server_key, TaskId, TaskResult, TokenUsage, WorkerId};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, RwLock, Semaphore};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

#[cfg(feature = "mtls")]
use crate::tls::ClientTlsConfig;
use crate::{DispatchRequest, FailureObservation, TransportError, WorkerEvent, WorkerTransport};

/// 브로드캐스트 채널 용량.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// prompt 완료 대기 기본 타임아웃 (10분). 초과 시 Failed 이벤트.
const DEFAULT_PROMPT_TIMEOUT: Duration = Duration::from_secs(600);

/// 재연결 백오프 시퀀스의 첫 간격.
pub const RECONNECT_INITIAL: Duration = Duration::from_secs(1);

/// 재연결 백오프 상한.
pub const RECONNECT_MAX: Duration = Duration::from_secs(30);

/// 재연결 백오프 설정. 테스트 주입을 위해 `AcpTransport`에 보관.
#[derive(Debug, Clone, Copy)]
pub struct ReconnectConfig {
    pub initial: Duration,
    pub max: Duration,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial: RECONNECT_INITIAL,
            max: RECONNECT_MAX,
        }
    }
}

/// 워커 연결 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Connected,
    Disconnected,
}

/// 진행 중인 단일 태스크(=단일 ACP 세션)의 메타데이터.
/// `on_receive_notification` 콜백이 `session_id`로 이 맵을 찾아 스트리밍
/// 출력을 올바른 task로 라우팅한다.
struct InFlightSession {
    task_id: TaskId,
    /// Head-of-Line Blocking 방지용 세션 전용 알림 큐 (로드맵 #41).
    ///
    /// 워커당 WebSocket 연결은 하나뿐이라, SDK의 `on_receive_notification`
    /// 핸들러는 연결 전체에 대해 **단일 순차 루프**(`incoming_protocol_actor`,
    /// vendor SDK 내부)에서 인라인으로 `.await`된다 — 즉 한 세션의
    /// notification 처리가 느려지면 같은 연결을 공유하는 다른 세션의
    /// notification 배달까지 지연된다(vendor SDK 소스로 확인, 프로젝트 코드가
    /// 아니라 패치 대상이 아님). 실제 처리(버퍼 append + seq 증가 + 이벤트
    /// 브로드캐스트)는 세션마다 별도로 spawn된 워커 태스크로 위임하고, 여기
    /// 큐에는 non-blocking send만 하도록 해 SDK의 공유 actor 루프가 즉시
    /// 반환하게 한다 — 세션 간 동시성을 확보하면서, 세션 내부의 순서(단일
    /// 컨슈머 = FIFO)는 그대로 보존한다.
    notify_tx: mpsc::UnboundedSender<SessionMsg>,
}

/// 세션 전용 알림 큐에 들어가는 메시지 (로드맵 #41).
enum SessionMsg {
    /// `session/update`에서 추출한 텍스트 청크.
    Chunk(String),
    /// 배리어. 이 메시지가 컨슈머(세션 워커)에 도달했다는 것은 그 이전에
    /// 큐잉된 모든 `Chunk`가 이미 처리(버퍼 append + 브로드캐스트) 완료됐다는
    /// 뜻이다 — `dispatch()`가 `PromptResponse` 수신 후 `output_buf`를 읽기
    /// 전에 이 배리어를 통해 "지금까지 도착한 모든 청크가 반영됐음"을
    /// 보장한다. 큐잉 자체(채널에 들어간 순서)는 이미 SDK의 단일 actor
    /// 루프가 보장하므로, 이 배리어는 오직 "컨슈머가 실제로 다 처리했는지"만
    /// 확인한다.
    Flush(oneshot::Sender<()>),
}

/// 워커별 세션. supervisor와 dispatch/cancel 양쪽에서 공유.
struct WorkerSession {
    worker_id: WorkerId,
    endpoint: String,
    state: RwLock<ConnState>,
    /// 활성 연결 핸들 — Connected 상태에서만 Some. `ConnectionTo`는 저비용
    /// clone 가능(SDK 설계) — 여러 dispatch가 동시에 빌려 써도 안전.
    connection: Mutex<Option<ConnectionTo<Agent>>>,
    /// 이 워커의 동시 작업 상한을 강제하는 세마포어.
    capacity: Arc<Semaphore>,
    max_concurrent: u32,
    /// session_id -> in-flight 메타데이터. task당 세션 하나이므로 이 맵의
    /// 키만으로 스트리밍 출력을 완전히 모호함 없이 라우팅할 수 있다.
    sessions: Arc<Mutex<HashMap<SessionId, InFlightSession>>>,
    /// supervisor 종료 신호. `register()` 시 1회만 생성되고 `WorkerSession`의
    /// 수명 동안 유지된다(oneshot이 아니라 watch를 쓰는 이유 — 2026-08-11
    /// 버그 수정: 예전엔 supervisor 루프가 돌 때마다 새 oneshot을 만들어서,
    /// 재연결 백오프로 sleep 중일 때 만들어진 oneshot이 `connect_with` 클로저
    /// 안에서 이미 소비된 상태라 `unregister()`가 새로 신호를 보낼 채널
    /// 자체가 없어 최대 backoff 시간만큼 무조건 기다려야 했다 —
    /// `unregister_during_backoff_exits_cleanly` 테스트로 발견).
    shutdown: tokio::sync::watch::Sender<bool>,
    supervisor: Mutex<Option<JoinHandle<()>>>,
    #[cfg(feature = "mtls")]
    tls_connector: Option<tokio_rustls::TlsConnector>,
}

impl WorkerSession {
    fn new(
        worker_id: WorkerId,
        endpoint: String,
        max_concurrent: u32,
        #[cfg(feature = "mtls")] tls_connector: Option<tokio_rustls::TlsConnector>,
    ) -> Arc<Self> {
        let cap = max_concurrent.max(1);
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Arc::new(Self {
            worker_id,
            endpoint,
            state: RwLock::new(ConnState::Connecting),
            connection: Mutex::new(None),
            capacity: Arc::new(Semaphore::new(cap as usize)),
            max_concurrent: cap,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            shutdown,
            supervisor: Mutex::new(None),
            #[cfg(feature = "mtls")]
            tls_connector,
        })
    }

    /// shutdown이 이미 요청됐거나(즉시 true), 아니면 요청될 때까지 또는
    /// `backoff`가 지날 때까지 대기. 반환값 true = shutdown이 원인.
    async fn shutdown_requested_or_backoff_elapsed(&self, backoff: Duration) -> bool {
        let mut rx = self.shutdown.subscribe();
        if *rx.borrow() {
            return true;
        }
        tokio::select! {
            _ = rx.changed() => true,
            _ = tokio::time::sleep(backoff) => false,
        }
    }

    async fn in_flight_count(&self) -> usize {
        self.sessions.lock().await.len()
    }

    /// 진행 중인 모든 태스크를 한 번에 실패 처리 (연결 끊김 시).
    async fn fail_all(&self, broadcaster: &broadcast::Sender<WorkerEvent>, reason: &str) {
        let drained: Vec<(SessionId, TaskId)> = {
            let sessions = self.sessions.lock().await;
            sessions
                .iter()
                .map(|(sid, s)| (sid.clone(), s.task_id))
                .collect()
        };
        if drained.is_empty() {
            return;
        }
        warn!(
            worker_id = %self.worker_id,
            count = drained.len(),
            reason,
            "failing all in-flight tasks due to connection loss"
        );
        let mut sessions = self.sessions.lock().await;
        for (session_id, task_id) in drained {
            sessions.remove(&session_id);
            let _ = broadcaster.send(WorkerEvent::Failed {
                task_id,
                error: reason.to_string(),
                // 여기 있는 세션은 전부 프롬프트가 이미 전달된 것들이다
                // (session/new가 실패한 태스크는 세션 맵에 들어오지 못한다).
                // 워커는 지금도 그 작업을 돌리고 있을 수 있다.
                observation: FailureObservation::ResultLost,
            });
        }
    }
}

/// ACP transport 구현체.
pub struct AcpTransport {
    clients: Arc<RwLock<HashMap<WorkerId, Arc<WorkerSession>>>>,
    event_broadcaster: broadcast::Sender<WorkerEvent>,
    reconnect: ReconnectConfig,
    /// mTLS 클라이언트 구성 (Phase 8.5). `Some`인 경우 `wss://` endpoint에
    /// `HttpClient::with_tls_connector`로 사설 CA + 클라이언트 인증서를 적용.
    #[cfg(feature = "mtls")]
    client_tls: Option<Arc<ClientTlsConfig>>,
}

impl AcpTransport {
    pub fn new() -> Self {
        Self::with_reconnect(ReconnectConfig::default())
    }

    pub fn with_reconnect(reconnect: ReconnectConfig) -> Self {
        let (event_broadcaster, _) = broadcast::channel::<WorkerEvent>(EVENT_CHANNEL_CAPACITY);
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            event_broadcaster,
            reconnect,
            #[cfg(feature = "mtls")]
            client_tls: None,
        }
    }

    /// orchestrator 클라이언트 mTLS 구성을 지정 (Phase 8.5). 이후 `register()`
    /// 되는 모든 워커에 대해 `wss://` endpoint인 경우 mTLS 핸드셰이크를 수행.
    #[cfg(feature = "mtls")]
    #[must_use]
    pub fn with_client_tls(mut self, tls: ClientTlsConfig) -> Self {
        self.client_tls = Some(Arc::new(tls));
        self
    }

    pub async fn conn_state(&self, worker_id: WorkerId) -> Option<ConnState> {
        let clients = self.clients.read().await;
        let session = clients.get(&worker_id).cloned()?;
        drop(clients);
        let state = *session.state.read().await;
        Some(state)
    }

    pub async fn in_flight_count(&self, worker_id: WorkerId) -> Option<usize> {
        let clients = self.clients.read().await;
        let session = clients.get(&worker_id).cloned()?;
        drop(clients);
        Some(session.in_flight_count().await)
    }

    pub async fn max_concurrent(&self, worker_id: WorkerId) -> Option<u32> {
        let clients = self.clients.read().await;
        clients.get(&worker_id).map(|s| s.max_concurrent)
    }
}

impl Default for AcpTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WorkerTransport for AcpTransport {
    async fn register(
        &self,
        worker_id: WorkerId,
        endpoint: &str,
        max_concurrent_tasks: u32,
    ) -> Result<(), TransportError> {
        if self.clients.read().await.contains_key(&worker_id) {
            return Err(TransportError::AlreadyRegistered(worker_id.to_string()));
        }

        let cap = max_concurrent_tasks.max(1);
        info!(
            %worker_id,
            endpoint = %mask_server_key(endpoint),
            max_concurrent = cap,
            "registering ACP worker"
        );

        #[cfg(feature = "mtls")]
        let tls_connector = self
            .client_tls
            .as_ref()
            .filter(|_| endpoint.starts_with("wss://"))
            .map(|tls| {
                tls.build_connector().map_err(|e| {
                    TransportError::Connection(format!("mTLS connector build failed: {e}"))
                })
            })
            .transpose()?;

        let session = WorkerSession::new(
            worker_id,
            endpoint.to_string(),
            cap,
            #[cfg(feature = "mtls")]
            tls_connector,
        );

        let (first_result_tx, first_result_rx) =
            tokio::sync::oneshot::channel::<Result<(), String>>();
        let supervisor_handle = spawn_supervisor(
            session.clone(),
            self.event_broadcaster.clone(),
            self.reconnect,
            Some(first_result_tx),
        );
        *session.supervisor.lock().await = Some(supervisor_handle);

        // supervisor를 즉시 clients에 등록 — 최초 연결 시도의 성공 여부와 무관하게
        // (일시적 실패 후 백그라운드 재연결이 성공했을 때 dispatch가 세션을 찾을
        // 수 있어야 하므로 — 과거 프로덕션 버그: 이 순서를 지키지 않아 워커가
        // "online"으로 보이는데도 태스크가 영원히 dispatch 안 되는 상태가 있었다).
        self.clients.write().await.insert(worker_id, session);

        match first_result_rx.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(TransportError::Connection(format!(
                "initial ACP connect failed for {worker_id}: {e} (still tracked — supervisor keeps retrying in the background)"
            ))),
            Err(_) => Err(TransportError::Connection(format!(
                "supervisor task dropped first-result channel for {worker_id}"
            ))),
        }
    }

    async fn unregister(&self, worker_id: WorkerId) -> Result<(), TransportError> {
        let session = self
            .clients
            .write()
            .await
            .remove(&worker_id)
            .ok_or_else(|| TransportError::WorkerNotRegistered(worker_id.to_string()))?;

        info!(%worker_id, "unregistering ACP worker");
        let _ = session.shutdown.send(true);
        if let Some(handle) = session.supervisor.lock().await.take() {
            // graceful shutdown 신호를 보냈으니 자연 종료를 기다린다.
            // 무한 대기하지 않도록 상한을 둔다.
            let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        }
        Ok(())
    }

    async fn is_connected(&self, worker_id: WorkerId) -> bool {
        let clients = self.clients.read().await;
        match clients.get(&worker_id) {
            Some(session) => *session.state.read().await == ConnState::Connected,
            None => false,
        }
    }

    async fn dispatch(&self, req: DispatchRequest) -> Result<(), TransportError> {
        let worker_id = req.worker_id;
        let task_id = req.task_id;

        // 로드맵 #69 — 워커에 도달하는 **마지막 관문**. 제출 시점(Dispatcher,
        // CLI)에도 같은 규칙을 걸지만 그것만으로는 부족하다: 이 검증이 생기기
        // 전에 저장된 Task 행과, 저장소에 직접 쓰는 경로(`fleet tasks submit`은
        // `Dispatcher::submit`을 지나지 않는다)가 재조정 루프를 타고 여기로
        // 들어온다. 워커 상태를 보기 **전에** 판정하는 이유는, 무효한 요청이
        // 워커 연결 상태에 따라 다른 에러를 받으면 안 되기 때문이다 — 원인은
        // 요청에 있지 워커에 있지 않다.
        let cwd = match fleet_core::validate_workspace_cwd(req.cwd.as_deref()) {
            Ok(validated) => PathBuf::from(validated),
            Err(e) => {
                warn!(%task_id, %worker_id, error = %e, "refusing to dispatch: invalid cwd");
                return Err(TransportError::InvalidRequest(format!(
                    "task {task_id}: {e}"
                )));
            }
        };

        let session = {
            let clients = self.clients.read().await;
            clients
                .get(&worker_id)
                .cloned()
                .ok_or_else(|| TransportError::WorkerNotRegistered(worker_id.to_string()))?
        };

        let state = *session.state.read().await;
        if state != ConnState::Connected {
            return Err(TransportError::Connection(format!(
                "worker {worker_id} not connected (state={state:?}); cannot dispatch task {task_id}"
            )));
        }

        // 용량 검증 — 즉시(non-blocking) 획득, 큐잉 없음.
        let permit = Arc::clone(&session.capacity)
            .try_acquire_owned()
            .map_err(|_| TransportError::WorkerAtCapacity(worker_id.to_string()))?;

        let connection = {
            let guard = session.connection.lock().await;
            guard.clone().ok_or_else(|| {
                TransportError::Connection(format!(
                    "worker {worker_id} session disappeared mid-dispatch"
                ))
            })?
        };

        let broadcaster = self.event_broadcaster.clone();
        let sessions_map = session.sessions.clone();
        let timeout = req
            .timeout_secs
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_PROMPT_TIMEOUT);
        // req.model / req.max_turns: ACP session/new·session/prompt 스펙에 직접
        // 대응하는 필드가 없다 (모델은 워커의 grok 프로세스 시작 시점에 고정,
        // max_turns는 grok 자체 세션 옵션 밖) — 예전 구현도 이 두 필드를
        // 사용하지 않았으므로 회귀는 아니다. cwd는 이번에 처음으로 실제
        // session/new에 연결된다(예전엔 워커당 공유 세션이라 무시됐음).
        //
        // `cwd`는 이 함수 최상단에서 이미 검증·확정됐다. 예전에는 여기서
        // `unwrap_or_else(|| PathBuf::from("/"))`로 기본값을 지어냈고, 그 결과
        // `cwd`를 생략한 모든 태스크의 에이전트 세션이 파일시스템 루트에서
        // 열렸다.
        let prompt_text = req.prompt.clone();

        tokio::spawn(async move {
            let _permit = permit; // 완료/실패 시 drop되며 슬롯 반환.
            let started = Instant::now();

            let new_session = tokio::time::timeout(
                timeout,
                connection
                    .send_request(NewSessionRequest::new(cwd))
                    .block_task(),
            )
            .await;

            let session_id = match new_session {
                Ok(Ok(resp)) => resp.session_id,
                Ok(Err(e)) => {
                    warn!(%task_id, %worker_id, error = %e, "session/new failed");
                    let _ = broadcaster.send(WorkerEvent::Failed {
                        task_id,
                        error: format!("session/new: {e}"),
                        // 워커가 답을 줬다. 프롬프트가 나가기 전 단계에서
                        // 거부당했지만 그것은 분류를 바꾸지 않는다 —
                        // `FailureObservation`의 질문 1이 질문 2보다 먼저다.
                        observation: FailureObservation::Reported,
                    });
                    return;
                }
                Err(_) => {
                    warn!(%task_id, %worker_id, "session/new timed out");
                    let _ = broadcaster.send(WorkerEvent::Failed {
                        task_id,
                        error: format!("session/new timed out after {timeout:?}"),
                        // 세션이 열리지 않았으니 프롬프트는 전달되지 않았다.
                        // 워커가 응답하지 않는다는 사실 자체는 확정이다.
                        observation: FailureObservation::NotDelivered,
                    });
                    return;
                }
            };

            let output_buf = Arc::new(Mutex::new(String::new()));
            let seq = Arc::new(AtomicU64::new(0));
            let (notify_tx, mut notify_rx) = mpsc::unbounded_channel::<SessionMsg>();
            sessions_map
                .lock()
                .await
                .insert(session_id.clone(), InFlightSession { task_id, notify_tx });

            // 로드맵 #41 — 이 세션 전용 워커. FIFO 단일 컨슈머라 세션 내부
            // 순서는 보존되고, 세션마다 독립된 태스크로 돌기 때문에 한
            // 세션의 처리가 같은 연결을 공유하는 다른 세션을 지연시키지
            // 않는다(`InFlightSession::notify_tx` 문서 참고).
            let worker_output_buf = output_buf.clone();
            let worker_broadcaster = broadcaster.clone();
            tokio::spawn(async move {
                while let Some(msg) = notify_rx.recv().await {
                    match msg {
                        SessionMsg::Chunk(text) => {
                            worker_output_buf.lock().await.push_str(&text);
                            let out_seq = seq.fetch_add(1, Ordering::Relaxed);
                            let _ = worker_broadcaster.send(WorkerEvent::Output {
                                task_id,
                                seq: out_seq,
                                chunk: text,
                            });
                        }
                        SessionMsg::Flush(ack) => {
                            let _ = ack.send(());
                        }
                    }
                }
            });

            let prompt_result = tokio::time::timeout(
                timeout,
                connection
                    .send_request(PromptRequest::new(
                        session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new(prompt_text))],
                    ))
                    .block_task(),
            )
            .await;

            // 로드맵 #41 — `output_buf`를 읽기 전에, 이 세션 워커가 지금까지
            // 큐잉된 모든 청크를 실제로 처리(append) 완료했는지 배리어로
            // 확인한다. PromptResponse는 같은 연결의 이전 notification들보다
            // 항상 나중에 도착/디스패치되므로(SDK의 단일 incoming actor
            // 루프가 순서를 보존) 여기 도달한 시점엔 마지막 청크까지 이미
            // `notify_tx` 채널에 들어가 있음이 보장된다 — 다만 "채널에
            // 들어가 있음"과 "워커가 다 처리함"은 다르므로 별도 배리어가
            // 필요하다. 세션이 없거나(비정상 경로) send가 실패하면 워커가
            // 이미 죽은 것이므로 조용히 건너뛴다.
            let flush_tx = sessions_map
                .lock()
                .await
                .get(&session_id)
                .map(|entry| entry.notify_tx.clone());
            if let Some(tx) = flush_tx {
                let (ack_tx, ack_rx) = oneshot::channel();
                if tx.send(SessionMsg::Flush(ack_tx)).is_ok() {
                    let _ = ack_rx.await;
                }
            }

            // 이 턴은 끝났으므로 더 이상 이 session_id로 오는 notification을
            // 라우팅할 필요가 없다 — 제거해 맵이 무한정 자라는 것을 방지.
            // 반환값(제거된 엔트리 유무)로 중복 emit을 막는다: 연결이 끊기면
            // supervisor의 fail_all()도 "같은" session_id를 동시에 정리하며
            // Failed를 emit할 수 있다 — 이미 fail_all()이 먼저 제거했다면
            // (반환값 None) 여기서는 emit하지 않는다(2026-08-11 발견 —
            // 연결 종료 시 Failed가 태스크당 최대 1번만 나가야 함). 맵에서
            // 제거되면 이 세션의 마지막 `notify_tx` 소유자가 사라지므로
            // 위 세션 워커 태스크도 채널 종료로 자연스럽게 끝난다.
            let already_handled_elsewhere = sessions_map.lock().await.remove(&session_id).is_none();

            let duration_secs = started.elapsed().as_secs_f64();
            let output = output_buf.lock().await.clone();

            match prompt_result {
                Ok(Ok(resp)) => {
                    debug!(
                        %task_id, %worker_id,
                        elapsed_secs = duration_secs,
                        stop_reason = ?resp.stop_reason,
                        "acp prompt completed"
                    );
                    let token_usage = extract_token_usage(&resp);
                    let result = TaskResult {
                        output,
                        exit_code: 0,
                        duration_secs,
                        token_usage,
                        worker_id,
                        finished_at: Utc::now(),
                    };
                    let _ = broadcaster.send(WorkerEvent::Completed { task_id, result });
                }
                Ok(Err(e)) if !already_handled_elsewhere => {
                    warn!(%task_id, %worker_id, error = %e, "acp prompt failed");
                    let _ = broadcaster.send(WorkerEvent::Failed {
                        task_id,
                        error: format!("session/prompt: {e}"),
                        observation: FailureObservation::Reported,
                    });
                }
                Err(_) if !already_handled_elsewhere => {
                    warn!(%task_id, %worker_id, "acp prompt timed out");

                    // 프롬프트는 워커에 갔다. 타임아웃은 우리 쪽 대기가 끝났다는
                    // 뜻일 뿐, 저쪽 실행이 끝났다는 뜻이 아니다 — 그래서 여기서
                    // 세션을 끊어 준다. 이 자리에서 보내야만 하는 이유가 있다:
                    // 바로 위에서 `sessions_map`의 엔트리를 이미 제거했고,
                    // `cancel()`은 그 맵을 `s.task_id == task_id`로 훑어 세션을
                    // 찾는다. 즉 나중에 외부에서 `cancel(task_id)`를 불러도 찾을
                    // 게 없어 조용히 `Ok(())`를 반환한다(그 debug 로그의
                    // "task already terminal?"은 이 경로에 대해 거짓이다 —
                    // 워커 쪽 실행은 살아 있고 라우팅 엔트리만 사라진 것이다).
                    // `connection`과 `session_id`가 스코프에 남아 있는 여기가
                    // 워커에 닿을 수 있는 마지막 지점이다.
                    //
                    // permit은 여전히 이 arm이 끝나면서 놓는다. `session/cancel`은
                    // ack 없는 notification이라 워커가 실제로 멈췄는지 확인할
                    // 방법이 없고, 확인될 때까지 슬롯을 붙들면 영영 돌아오지
                    // 않는 permit이 된다 — 채울 방법이 없는 상태는 만들지
                    // 않는다. 초과 점유 창은 닫히는 게 아니라 좁아진다.
                    info!(%task_id, %worker_id, session_id = %session_id, "sending ACP cancel after prompt timeout");
                    let _ = connection.send_notification(CancelNotification::new(session_id));

                    let _ = broadcaster.send(WorkerEvent::Failed {
                        task_id,
                        error: format!("session/prompt timed out after {timeout:?}"),
                        // cancel을 보냈다고 결과가 확정되는 것은 아니다. 워커가
                        // 그것을 받았는지, 받고 멈췄는지, 멈추기 전에 무엇을
                        // 했는지 우리는 모른다 — 관측은 여전히 유실이다.
                        observation: FailureObservation::ResultLost,
                    });
                }
                Ok(Err(_)) | Err(_) => {
                    // fail_all()이 이미 이 태스크를 Failed로 emit했음 — 중복 방지.
                    debug!(
                        %task_id, %worker_id,
                        "prompt failure overlapped with concurrent connection-loss cleanup — skipping duplicate emit"
                    );
                }
            }
        });

        Ok(())
    }

    async fn cancel(&self, task_id: TaskId) -> Result<(), TransportError> {
        let clients = self.clients.read().await;
        for (worker_id, session) in clients.iter() {
            let session_id = {
                let sessions = session.sessions.lock().await;
                sessions
                    .iter()
                    .find(|(_, s)| s.task_id == task_id)
                    .map(|(sid, _)| sid.clone())
            };
            let Some(session_id) = session_id else {
                continue;
            };

            let connection = session.connection.lock().await.clone();
            let Some(connection) = connection else {
                debug!(
                    %task_id, %worker_id,
                    "cancel: active connection missing — likely disconnected, treating as idempotent success"
                );
                return Ok(());
            };

            info!(%task_id, %worker_id, session_id = %session_id, "sending ACP cancel");
            let _ = connection.send_notification(CancelNotification::new(session_id));
            return Ok(());
        }

        debug!(%task_id, "cancel: no active worker session found — task already terminal?");
        Ok(())
    }

    async fn ping(&self, worker_id: WorkerId) -> Result<Duration, TransportError> {
        let clients = self.clients.read().await;
        let session = clients
            .get(&worker_id)
            .cloned()
            .ok_or_else(|| TransportError::WorkerNotRegistered(worker_id.to_string()))?;
        drop(clients);
        let state = *session.state.read().await;
        if state != ConnState::Connected {
            return Err(TransportError::Connection(format!(
                "worker {worker_id} not connected (state={state:?})"
            )));
        }
        Ok(Duration::from_millis(1))
    }

    async fn subscribe(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<WorkerEvent>, TransportError> {
        let mut bcast_rx = self.event_broadcaster.subscribe();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<WorkerEvent>();
        tokio::spawn(async move {
            loop {
                match bcast_rx.recv().await {
                    Ok(event) => {
                        if tx.send(event).is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(n, "acp transport subscriber lagged");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        Ok(rx)
    }
}

// ─── supervisor ──────────────────────────────────────────────────────

/// supervisor 태스크 spawn. 반환된 핸들을 abort하면 supervisor 종료.
fn spawn_supervisor(
    session: Arc<WorkerSession>,
    broadcaster: broadcast::Sender<WorkerEvent>,
    reconnect: ReconnectConfig,
    first_result: Option<oneshot::Sender<Result<(), String>>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let worker_id = session.worker_id;
        let mut backoff = reconnect.initial;
        let mut first_result = first_result;

        loop {
            *session.state.write().await = ConnState::Connecting;

            let (ready_tx, mut ready_rx) = oneshot::channel::<Result<(), String>>();

            let ws_client = match build_ws_client(&session) {
                Ok(c) => c,
                Err(e) => {
                    warn!(%worker_id, error = %e, "failed to build ACP WebSocket client");
                    *session.state.write().await = ConnState::Disconnected;
                    if let Some(tx) = first_result.take() {
                        let _ = tx.send(Err(e));
                    }
                    if session.shutdown_requested_or_backoff_elapsed(backoff).await {
                        return;
                    }
                    backoff = (backoff * 2).min(reconnect.max);
                    continue;
                }
            };

            let sessions_map = session.sessions.clone();
            let session_for_conn = session.clone();

            let connect_future = agent_client_protocol::Client
                .builder()
                .on_receive_notification(
                    move |notification: SessionNotification, _cx| {
                        let sessions_map = sessions_map.clone();
                        // 로드맵 #41 — 여기서는 세션 전용 큐에 non-blocking
                        // send만 하고 즉시 반환한다. 실제 처리는 세션 워커
                        // 태스크에서 일어난다 — `handle_session_notification`
                        // 문서 참고.
                        async move {
                            handle_session_notification(&sessions_map, notification).await;
                            Ok(())
                        }
                    },
                    agent_client_protocol::on_receive_notification!(),
                )
                .connect_with(ws_client, move |connection: ConnectionTo<Agent>| {
                    let session = session_for_conn.clone();
                    async move {
                        let init = connection
                            .send_request(InitializeRequest::new(ProtocolVersion::V1))
                            .block_task()
                            .await;
                        if let Err(e) = init {
                            let _ = ready_tx.send(Err(format!("initialize: {e}")));
                            return Err(e);
                        }

                        *session.connection.lock().await = Some(connection);
                        *session.state.write().await = ConnState::Connected;
                        let _ = ready_tx.send(Ok(()));

                        // shutdown 신호가 올 때까지 대기 — transport(WS I/O 루프)가
                        // 먼저 죽으면 이 connect_with 전체가 그쪽 에러로 먼저
                        // 반환된다(SDK가 select로 경쟁시킴). 우리 쪽이 먼저 끝나면
                        // graceful shutdown 경로. watch 사용 이유는
                        // `WorkerSession.shutdown` 필드 문서 참고.
                        let mut shutdown_rx = session.shutdown.subscribe();
                        if !*shutdown_rx.borrow() {
                            let _ = shutdown_rx.changed().await;
                        }
                        Ok(())
                    }
                });

            // 버그 수정(2026-08-11): `connect_with(...).await`는 연결이 끊길
            // 때까지(=shutdown_rx가 resolve될 때까지) 절대 안 끝난다 — 최초
            // handshake 결과만 기다리면 되는 register()가 연결 수명 전체
            // 동안 블록되는 데드락이었다. ready_rx(빠른 handshake 신호)와
            // connect_future(연결 전체 수명)를 select로 분리해, handshake
            // 결과가 나오는 즉시 register() 호출자에게 전달하고, 연결
            // 자체는 계속 백그라운드에서 돈다.
            tokio::pin!(connect_future);
            let connect_result = tokio::select! {
                ready = &mut ready_rx => {
                    let outcome = ready.unwrap_or_else(|_| Err("ready channel closed before handshake completed".to_string()));
                    if let Some(tx) = first_result.take() {
                        let _ = tx.send(outcome);
                    }
                    // handshake는 끝났지만 연결 자체는 계속 산다 — 끊길 때까지 대기.
                    connect_future.await
                }
                result = &mut connect_future => {
                    // handshake가 채 끝나기도 전에 연결 자체가 죽은 경우(예: WS
                    // 핸드셰이크 실패) — first_result는 아직 못 받았을 것.
                    if let Some(tx) = first_result.take() {
                        let _ = tx.send(result.as_ref().map(|_| ()).map_err(|e| e.to_string()));
                    }
                    result
                }
            };

            *session.connection.lock().await = None;
            *session.state.write().await = ConnState::Disconnected;
            session
                .fail_all(&broadcaster, "ACP connection lost — will reconnect")
                .await;

            // shutdown이 true면 unregister()가 이미 요청한 것 — 재연결하지 않음.
            let should_stop = *session.shutdown.borrow();

            match &connect_result {
                Ok(()) if should_stop => {
                    info!(%worker_id, "supervisor shutting down cleanly");
                    return;
                }
                Ok(()) => {
                    // shutdown 요청 없이 정상 종료된 드문 케이스 — 안전하게 재연결 취급.
                    warn!(%worker_id, "ACP connection ended unexpectedly (clean) — reconnecting");
                }
                Err(e) => {
                    if should_stop {
                        info!(%worker_id, "supervisor shutting down (connection errored during shutdown, ignoring)");
                        return;
                    }
                    warn!(%worker_id, error = %e, "ACP connection error — reconnecting");
                }
            }

            if session.shutdown_requested_or_backoff_elapsed(backoff).await {
                return;
            }
            backoff = (backoff * 2).min(reconnect.max);
        }
    })
}

/// `session/update` notification 처리 — session_id로 해당 세션의 전용 알림
/// 큐를 찾아 텍스트 청크를 non-blocking send한다. 등록 안 된 session_id(이미
/// 종료됐거나 이 워커 소관이 아님)는 조용히 무시.
///
/// 로드맵 #41 — 실제 버퍼 append/`WorkerEvent::Output` emit은 여기서 하지
/// 않는다. 이 함수는 SDK의 공유 `incoming_protocol_actor` 루프에서 인라인
/// `.await`로 호출되므로(`InFlightSession::notify_tx` 문서 참고), 여기서
/// 느려질 수 있는 작업을 하면 같은 연결의 다른 세션 notification 배달까지
/// 지연시킨다 — 그래서 세션 조회 + non-blocking channel send만 하고 즉시
/// 반환하고, 실제 처리는 세션마다 별도로 spawn된 워커 태스크(`dispatch()`
/// 참고)로 넘긴다.
async fn handle_session_notification(
    sessions_map: &Arc<Mutex<HashMap<SessionId, InFlightSession>>>,
    notification: SessionNotification,
) {
    let text = match &notification.update {
        SessionUpdate::AgentMessageChunk(chunk) => extract_chunk_text(chunk),
        _ => None,
    };
    let Some(text) = text else { return };

    let notify_tx = {
        let sessions = sessions_map.lock().await;
        sessions
            .get(&notification.session_id)
            .map(|entry| entry.notify_tx.clone())
    };
    let Some(notify_tx) = notify_tx else { return };
    let _ = notify_tx.send(SessionMsg::Chunk(text));
}

/// `PromptResponse`에서 토큰 사용량 추출 (`unstable_end_turn_token_usage`
/// feature가 켜진 경우에만 필드가 존재 — 이 크레이트는 켜서 빌드한다).
fn extract_token_usage(
    resp: &agent_client_protocol::schema::v1::PromptResponse,
) -> Option<TokenUsage> {
    resp.usage.as_ref().map(|u| TokenUsage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        cache_read_tokens: u.cached_read_tokens.unwrap_or(0),
    })
}

/// `ContentChunk`(AgentMessageChunk)에서 텍스트 추출. 텍스트가 아닌 콘텐츠
/// (이미지 등)는 무시.
fn extract_chunk_text(chunk: &agent_client_protocol::schema::v1::ContentChunk) -> Option<String> {
    match &chunk.content {
        ContentBlock::Text(text_content) => Some(text_content.text.clone()),
        _ => None,
    }
}

/// ACP WebSocket 인증 자격을 어디에 싣는가 (로드맵 `#94`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcpAuthMode {
    /// `Authorization: Bearer <secret>` 헤더. 기본값.
    Header,
    /// `?server-key=<secret>` 쿼리 파라미터. `#94` 이전의 동작.
    Query,
}

impl AcpAuthMode {
    /// `FLEET_ACP_AUTH=query`일 때만 예전 동작으로 되돌린다.
    ///
    /// 기본을 헤더로 두는 이유: 쿼리 문자열은 중간 프록시(nginx 등)의 access
    /// log에 **평문으로** 남고, 그 로그는 우리가 통제하지 않는다. 헤더는 남지
    /// 않는다. 이것이 `#94`가 실제로 닫는 유일한 구멍이다 — secret을 없애는
    /// 것은 불가능하다(아래 `ws_auth_parts` 문서 참고).
    ///
    /// 탈출구를 남기는 이유: 헤더 인증은 grok `0.2.112`에서 실측했고 그보다
    /// 오래된 grok이 이를 지원하는지는 확인하지 않았다. 그런 워커가 섞여
    /// 있으면 다이얼이 401로 거절되므로, 운영자가 grok을 올릴 때까지 이
    /// 변수로 되돌릴 수 있어야 한다. 워커별 자동 협상은 만들지 않는다 —
    /// "그런 워커가 실제로 있다"는 증거가 없는 상태에서 협상 기계를 먼저
    /// 만드는 것은 채울 방법이 없는 것을 미리 만드는 것과 같다.
    fn from_env() -> Self {
        match std::env::var("FLEET_ACP_AUTH").as_deref() {
            Ok("query") => Self::Query,
            _ => Self::Header,
        }
    }
}

/// 저장된 endpoint 문자열에서 **실제로 다이얼할 URL**과 **`Authorization`
/// 헤더 값**을 만든다 (로드맵 `#94`).
///
/// `Worker.endpoint`의 저장 형태는 바뀌지 않는다 — 여전히 `?server-key=`를
/// 담은 원문이고, 외부로 나갈 때는 여전히 `mask_server_key`가 가린다(`#75`).
/// 바뀌는 것은 **wire에 나가는 형태**뿐이다.
///
/// secret을 **없애지는 못한다.** `MtlsProxy`가 TLS를 종단하고 grok에게는
/// 평문을 넘기므로 grok은 클라이언트 인증서를 볼 수 없고, 따라서 "mTLS만으로
/// 충분"은 성립하지 않는다(grok `0.2.112` 실측: 인증 없는 `/ws`는 401).
/// `#94`가 할 수 있는 것은 secret을 **URL 밖으로 옮기는 것**뿐이다.
///
/// `Query` 모드이거나 endpoint에 `server-key`가 없으면 원문을 그대로 쓴다.
fn ws_auth_parts(endpoint: &str, mode: AcpAuthMode) -> (String, Option<String>) {
    if mode == AcpAuthMode::Query {
        return (endpoint.to_string(), None);
    }
    match split_server_key(endpoint) {
        (stripped, Some(secret)) => (stripped, Some(format!("Bearer {secret}"))),
        (original, None) => (original, None),
    }
}

/// mTLS 여부와 인증 모드에 따라 `HttpClient`를 구성.
fn build_ws_client(session: &Arc<WorkerSession>) -> Result<HttpClient, String> {
    let (dial_url, auth_header) = ws_auth_parts(&session.endpoint, AcpAuthMode::from_env());
    let client = HttpClient::with_endpoint(&dial_url).map_err(|e| {
        format!(
            "invalid endpoint {}: {e}",
            mask_server_key(&session.endpoint)
        )
    })?;
    let client = match auth_header {
        Some(value) => client.with_ws_auth_header(value),
        None => client,
    };
    #[cfg(feature = "mtls")]
    let client = match &session.tls_connector {
        Some(connector) => client.with_tls_connector(connector.clone()),
        None => client,
    };
    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ws_auth_parts (로드맵 #94) ──────────────────────────────────────

    #[test]
    fn header_mode_moves_secret_out_of_the_url() {
        let (url, header) = ws_auth_parts(
            "wss://worker-1.fleet.internal:2420/ws?server-key=topsecret",
            AcpAuthMode::Header,
        );
        assert_eq!(url, "wss://worker-1.fleet.internal:2420/ws");
        assert_eq!(header.as_deref(), Some("Bearer topsecret"));
        // #94의 실질: 다이얼되는 URL 어디에도 secret이 없어야 한다.
        assert!(!url.contains("topsecret"));
        assert!(!url.contains("server-key"));
    }

    #[test]
    fn header_value_uses_the_exact_format_grok_accepts() {
        // grok 0.2.112 실측: 스킴은 대소문자를 구분하고(`bearer`는 401),
        // 공백은 정확히 하나여야 한다(둘이면 401). 이 테스트는 그 계약을
        // 문자열 수준에서 고정한다.
        let (_, header) = ws_auth_parts("ws://h/ws?server-key=s3cr3t", AcpAuthMode::Header);
        assert_eq!(header.as_deref(), Some("Bearer s3cr3t"));
    }

    #[test]
    fn query_mode_leaves_the_endpoint_exactly_as_stored() {
        let raw = "wss://h:2420/ws?server-key=topsecret";
        let (url, header) = ws_auth_parts(raw, AcpAuthMode::Query);
        assert_eq!(url, raw, "예전 동작으로의 탈출구는 원문을 그대로 써야 한다");
        assert!(header.is_none());
    }

    #[test]
    fn endpoint_without_secret_is_untouched_in_both_modes() {
        let raw = "wss://h:2420/ws";
        for mode in [AcpAuthMode::Header, AcpAuthMode::Query] {
            let (url, header) = ws_auth_parts(raw, mode);
            assert_eq!(url, raw, "{mode:?}");
            assert!(header.is_none(), "{mode:?}");
        }
    }

    #[test]
    fn non_mtls_named_path_keeps_its_path_while_shedding_the_secret() {
        // 비mTLS 경로는 `/ws/{name}`을 쓴다(리버스 SSH 터널 뒤에서 워커를
        // 구분하기 위해 — config.rs의 agent_endpoint 문서 참고). 경로는
        // 반드시 보존돼야 한다.
        let (url, header) = ws_auth_parts(
            "ws://tunnel-host/ws/worker-7?server-key=topsecret",
            AcpAuthMode::Header,
        );
        assert_eq!(url, "ws://tunnel-host/ws/worker-7");
        assert_eq!(header.as_deref(), Some("Bearer topsecret"));
    }

    #[tokio::test]
    async fn new_transport_has_no_clients() {
        let t = AcpTransport::new();
        assert!(t.clients.read().await.is_empty());
    }

    #[tokio::test]
    async fn subscribe_returns_receiver() {
        let t = AcpTransport::new();
        let _rx = t.subscribe().await.unwrap();
    }

    #[tokio::test]
    async fn unregister_unknown_returns_error() {
        let t = AcpTransport::new();
        let result = t.unregister(WorkerId::new()).await;
        assert!(matches!(
            result,
            Err(TransportError::WorkerNotRegistered(_))
        ));
    }

    #[tokio::test]
    async fn is_connected_unknown_worker_false() {
        let t = AcpTransport::new();
        assert!(!t.is_connected(WorkerId::new()).await);
    }

    #[tokio::test]
    async fn conn_state_unknown_worker_none() {
        let t = AcpTransport::new();
        assert!(t.conn_state(WorkerId::new()).await.is_none());
    }

    #[tokio::test]
    async fn ping_unknown_worker_errors() {
        let t = AcpTransport::new();
        let result = t.ping(WorkerId::new()).await;
        assert!(matches!(
            result,
            Err(TransportError::WorkerNotRegistered(_))
        ));
    }
}
