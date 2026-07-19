//! `AcpTransport` — `WorkerTransport` trait의 ACP 구현체.
//!
//! per-worker WebSocket 연결. 각 워커는 하나의 `AcpClient` 인스턴스와
//! 하나의 `SessionId`를 가짐. 워커에서 발생하는 AcpEvent는 fan-in 되어
//! 하나의 broadcast 채널로 subscriber(dispatcher)에게 전달.
//!
//! ## 동시성 모델 (Phase 8.4)
//!
//! 워커당 동시에 실행 중인 프롬프트는 `max_concurrent_tasks`개 (기본 1, 상한은
//! `register()` 호출 시 결정). dispatch는 다음 규칙을 따릅니다:
//!
//! - `in_flight.len() >= max_concurrent`인 경우 즉시
//!   `TransportError::WorkerAtCapacity` 반환 (큐잉 없음).
//! - 그 외에는 `in_flight`에 `(task_id, InFlightTask { prompt_id: None, ... })`로
//!   즉시 슬롯을 확보하고 백그라운드에서 `session/prompt`를 보냄.
//! - `prompt()` 응답이 도착하면 `prompt_id`를 채우고 역색인 `prompt_index`에
//!   `(prompt_id, task_id)`를 등록 — 이후 들어오는 `Output`/`Completed`/
//!   `Failed` notification이 정확한 task로 라우팅됨.
//! - `Output` 이벤트의 `prompt_id`가 `None`인 경우 (드문 레이스) 는 drop.
//!
//! ## 재연결 (Phase 8.2)
//!
//! 각 워커는 supervisor 태스크를 가짐. supervisor는:
//!
//! 1. `AcpClient::connect()` + `open_session()` 으로 초기 연결 확립.
//! 2. reader 루프가 종료되면 (WebSocket close / I/O 에러):
//!    - 진행 중인 **모든** `in_flight` 태스크를 `WorkerEvent::Failed`로 emit.
//!    - 상태를 `Disconnected`로 표시.
//!    - 지수 백오프 (1s → 2s → ... → 최대 30s) 후 재연결 시도.
//! 3. unregister 시 `shutdown_rx`로 supervisor를 종료.
//!
//! ```text
//! [AcpTransport]
//!   │
//!   ├── register(worker_id, endpoint, max_concurrent)
//!   │     └─► spawn supervisor(worker_id, endpoint)
//!   │           │
//!   │           ├── loop {
//!   │           │     connect + open_session
//!   │           │       └─► reader_loop (event_rx → WorkerEvent)
//!   │           │     on exit: emit Failed for ALL in_flight tasks, sleep(backoff)
//!   │           │   }
//!   │           │
//!   │           └── shutdown_rx → break
//!   │
//!   ├── dispatch(req)
//!   │     └─► if state != Connected → Err(Connection)
//!   │     └─► if in_flight.len() >= max_concurrent → Err(WorkerAtCapacity)
//!   │     └─► insert in_flight entry, spawn prompt
//!   │
//!   └── cancel(task_id)
//!         └─► lookup in_flight, send session/cancel for known prompt_id
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use fleet_core::{TaskId, TaskResult, TokenUsage, WorkerId};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::acp::{AcpClient, AcpEvent, PromptId, SessionId};
use crate::{DispatchRequest, TransportError, WorkerEvent, WorkerTransport};

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
    /// 첫 재연결 대기 시간.
    pub initial: Duration,
    /// 백오프 상한.
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

/// supervisor 명령.
#[derive(Debug, Clone, Copy)]
enum SupervisorCmd {
    /// 정상 종료.
    Shutdown,
    /// dispatch/cancel 등에서 보내는 ping (현재는 사용 안 함 — 확장용).
    #[allow(dead_code)]
    Ping,
}

/// 워커 연결 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// supervisor가 초기 연결 또는 재연결을 시도 중.
    Connecting,
    /// WebSocket이 열려 있고 세션이 활성.
    Connected,
    /// reader가 종료됨 — supervisor가 재연결 루프에 진입한 상태.
    Disconnected,
}

/// 연결된 AcpClient + SessionId 묶음.
/// supervisor가 소유하며, dispatch/cancel에서 빌려 씀.
struct ActiveSession {
    client: Arc<AcpClient>,
    session_id: SessionId,
}

/// 워커에서 진행 중인 단일 task의 메타데이터.
struct InFlightTask {
    /// `session/prompt` 응답이 도착하기 전에는 `None`.
    /// 응답 도착 후 `set_prompt_id`로 채워짐 — 이후 Output 이벤트 라우팅에 사용.
    prompt_id: Option<PromptId>,
    /// dispatch 시각 (로그/진단용).
    started: Instant,
}

impl InFlightTask {
    fn new() -> Self {
        Self {
            prompt_id: None,
            started: Instant::now(),
        }
    }
}

/// 워커별 세션. supervisor와 dispatch/cancel 양쪽에서 공유.
struct WorkerSession {
    worker_id: WorkerId,
    /// 원본 endpoint (재연결용).
    endpoint: String,
    /// 현재 연결 상태 (supervisor가 갱신).
    state: RwLock<ConnState>,
    /// 활성 AcpClient / SessionId — Connected 상태에서만 Some.
    /// supervisor가 spawn한 reader와 수명을 같이 함.
    inner: Mutex<Option<ActiveSession>>,
    /// 이 워커의 동시 작업 상한 (register 시 고정).
    max_concurrent: u32,
    /// 현재 진행 중인 태스크 맵 (task_id → 메타데이터).
    /// Phase 8.4 이전에는 단일 `active_task: Option<TaskId>`였으나,
    /// 이제 N개까지 동시에 추적 가능.
    in_flight: Mutex<HashMap<TaskId, InFlightTask>>,
    /// 역색인: prompt_id → task_id. Output/Completed/Failed 이벤트 라우팅용.
    /// `prompt_id`가 결정된 시점(prompt() 응답 도착)에 삽입됨.
    prompt_index: Mutex<HashMap<PromptId, TaskId>>,
    /// prompt_id가 아직 dispatch에 의해 등록되기 전에 도착한 이벤트 버퍼.
    /// session/update (Output)가 session/prompt 응답보다 먼저 도착하는
    /// 레이스 윈도우를 커버. set_prompt_id 호출 시 drain되어 emit됨.
    pending_events: Mutex<HashMap<PromptId, Vec<BufferedEvent>>>,
    /// supervisor 태스크로 보내는 명령 채널.
    cmd_tx: mpsc::UnboundedSender<SupervisorCmd>,
    /// supervisor 태스크 핸들 (Drop에서 abort).
    supervisor: Mutex<Option<JoinHandle<()>>>,
}

/// prompt_id 등록 전에 버퍼링되는 이벤트.
#[derive(Debug, Clone)]
enum BufferedEvent {
    Output { seq: u64, chunk: String },
    Failed { error: String },
}

impl WorkerSession {
    /// 빈 in-flight 상태로 새 세션 생성 (register 호출 시).
    fn new(
        worker_id: WorkerId,
        endpoint: String,
        max_concurrent: u32,
        cmd_tx: mpsc::UnboundedSender<SupervisorCmd>,
    ) -> Arc<Self> {
        Arc::new(Self {
            worker_id,
            endpoint,
            state: RwLock::new(ConnState::Connecting),
            inner: Mutex::new(None),
            max_concurrent: max_concurrent.max(1),
            in_flight: Mutex::new(HashMap::new()),
            prompt_index: Mutex::new(HashMap::new()),
            pending_events: Mutex::new(HashMap::new()),
            cmd_tx,
            supervisor: Mutex::new(None),
        })
    }

    /// 현재 in-flight 카운트.
    async fn in_flight_count(&self) -> usize {
        self.in_flight.lock().await.len()
    }

    /// dispatch 시 슬롯 확보. 용량 초과 시 `WorkerAtCapacity` 에러.
    async fn try_acquire(&self, task_id: TaskId) -> Result<(), TransportError> {
        let mut guard = self.in_flight.lock().await;
        if guard.len() >= self.max_concurrent as usize {
            return Err(TransportError::WorkerAtCapacity(self.worker_id.to_string()));
        }
        guard.insert(task_id, InFlightTask::new());
        Ok(())
    }

    /// `prompt()` 응답 도착 후 prompt_id 등록.
    /// in_flight와 prompt_index 양쪽을 갱신하고, prompt_id가 알려지기 전에
    /// 버퍼링된 이벤트를 drain하여 반환 — 호출자(dispatch)가 emit.
    async fn set_prompt_id(&self, task_id: TaskId, prompt_id: PromptId) -> Vec<BufferedEvent> {
        let mut in_flight = self.in_flight.lock().await;
        if let Some(task) = in_flight.get_mut(&task_id) {
            task.prompt_id = Some(prompt_id);
        } else {
            debug!(
                %task_id, prompt_id = prompt_id.0,
                "set_prompt_id: task not in in_flight — likely already completed"
            );
            return Vec::new();
        }
        drop(in_flight);
        self.prompt_index.lock().await.insert(prompt_id, task_id);
        self.pending_events
            .lock()
            .await
            .remove(&prompt_id)
            .unwrap_or_default()
    }

    /// prompt_id가 아직 등록되지 않은 경우 이벤트를 버퍼에 추가.
    /// reader_loop에서 Output/Failed 처리 시 사용.
    async fn buffer_event(&self, prompt_id: PromptId, event: BufferedEvent) {
        self.pending_events
            .lock()
            .await
            .entry(prompt_id)
            .or_default()
            .push(event);
    }

    /// task_id를 in_flight와 prompt_index에서 제거. 완료/실패 시 호출.
    /// 제거된 task의 prompt_id를 반환 (사용처에서 필요시 index 정리용).
    async fn complete(&self, task_id: TaskId) -> Option<InFlightTask> {
        let removed = self.in_flight.lock().await.remove(&task_id);
        if let Some(ref task) = removed {
            debug!(
                worker_id = %self.worker_id,
                %task_id,
                elapsed_secs = task.started.elapsed().as_secs_f64(),
                "in-flight task removed"
            );
            if let Some(pid) = task.prompt_id {
                self.prompt_index.lock().await.remove(&pid);
                // 버퍼링된 이벤트도 함께 정리 — emit 주체가 정해지지 않은 상태에서
                // task가 종료되었으므로 드롭.
                self.pending_events.lock().await.remove(&pid);
            }
        }
        removed
    }

    /// prompt_id로 task_id 역조회 (reader_loop에서 Output/Completed/Failed 처리용).
    async fn task_for_prompt(&self, prompt_id: PromptId) -> Option<TaskId> {
        self.prompt_index.lock().await.get(&prompt_id).copied()
    }

    /// 진행 중인 모든 task를 한 번에 실패 처리 (연결 끊김 시).
    /// 각 task마다 `WorkerEvent::Failed`를 emit하고 맵 정리.
    async fn fail_all(
        self: &Arc<Self>,
        broadcaster: &broadcast::Sender<WorkerEvent>,
        reason: &str,
    ) {
        let drained: Vec<TaskId> = {
            let guard = self.in_flight.lock().await;
            guard.keys().copied().collect()
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
        for task_id in drained {
            self.complete(task_id).await;
            let _ = broadcaster.send(WorkerEvent::Failed {
                task_id,
                error: reason.to_string(),
            });
        }
    }

    /// 활성 세션 정리 (dispatch/cancel이 빌려간 Arc 참조가 떨어지면 자동 close).
    /// supervisor에서만 호출.
    async fn clear_active(&self) {
        // 단순히 Mutex에서 take. Arc<AcpClient>가 drop되면 WebSocket이 닫힘.
        // 여전히 다른 곳에서 빌려쓰는 경우가 있다면 자연스럽게 close됨.
        if let Some(active) = self.inner.lock().await.take() {
            // 명시적 close는 어렵지만 (AcpClient::close가 by-value),
            // drop에 맡김. Arc strong count가 0이 되면 close 처리.
            debug!(
                worker_id = %self.worker_id,
                session_id = %active.session_id,
                "clearing active session (WebSocket will close on Arc drop)"
            );
            // reader는 supervisor가 별도로 관리하므로 여기서 abort하지 않음.
        }
    }
}

impl Drop for WorkerSession {
    fn drop(&mut self) {
        // supervisor에게 shutdown 신호 (채널이 닫혀도 무시).
        let _ = self.cmd_tx.send(SupervisorCmd::Shutdown);
        if let Ok(mut guard) = self.supervisor.try_lock() {
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }
    }
}

/// ACP transport 구현체.
pub struct AcpTransport {
    /// worker_id → 워커 세션.
    clients: Arc<RwLock<HashMap<WorkerId, Arc<WorkerSession>>>>,
    /// 모든 워커의 WorkerEvent를 fan-out.
    event_broadcaster: broadcast::Sender<WorkerEvent>,
    /// 재연결 백오프 설정.
    reconnect: ReconnectConfig,
    /// orchestrator 클라이언트 mTLS 구성 (Phase 8.5).
    /// `Some` 인 경우, `wss://` endpoint 에 대해 `WsConn::connect_mtls` 사용.
    /// `ws://` endpoint 는 이 값의 유무와 무관하게 일반 TCP 로 연결.
    #[cfg(feature = "mtls")]
    client_tls: Option<Arc<crate::tls::ClientTlsConfig>>,
}

impl AcpTransport {
    /// 새 transport 생성 (기본 재연결 설정).
    pub fn new() -> Self {
        Self::with_reconnect(ReconnectConfig::default())
    }

    /// 재연결 백오프 설정을 지정하여 생성. 테스트용 — 짧은 백오프로 검증 가능.
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

    /// orchestrator 클라이언트 mTLS 구성을 지정 (Phase 8.5).
    ///
    /// 이후 `register()` 되는 모든 워커에 대해, `wss://` endpoint 인 경우
    /// `ClientTlsConfig` 로 mTLS 핸드셰이크를 수행. 이미 등록된 워커는
    /// 재연결 시점부터 새 구성을 사용.
    #[cfg(feature = "mtls")]
    pub fn with_client_tls(mut self, tls: crate::tls::ClientTlsConfig) -> Self {
        self.client_tls = Some(Arc::new(tls));
        self
    }

    /// 특정 워커의 연결 상태 조회. 미등록 워커면 None.
    pub async fn conn_state(&self, worker_id: WorkerId) -> Option<ConnState> {
        let clients = self.clients.read().await;
        let session = clients.get(&worker_id).cloned()?;
        drop(clients);
        let state = *session.state.read().await;
        Some(state)
    }

    /// 특정 워커의 현재 in-flight task 수. 미등록 워커면 None.
    /// 관측/디버그/테스트용 — dispatch 결정은 selector가 미리 수행.
    pub async fn in_flight_count(&self, worker_id: WorkerId) -> Option<usize> {
        let clients = self.clients.read().await;
        let session = clients.get(&worker_id).cloned()?;
        drop(clients);
        Some(session.in_flight_count().await)
    }

    /// 특정 워커의 동시 작업 상한. 미등록 워커면 None.
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
        // 중복 등록 체크.
        if self.clients.read().await.contains_key(&worker_id) {
            return Err(TransportError::AlreadyRegistered(worker_id.to_string()));
        }

        let cap = max_concurrent_tasks.max(1);
        info!(
            %worker_id,
            endpoint = %sanitize_endpoint(endpoint),
            max_concurrent = cap,
            "registering ACP worker"
        );

        // supervisor 명령 채널.
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<SupervisorCmd>();
        // 초기 연결 결과 전달 채널.
        let (first_result_tx, first_result_rx) =
            tokio::sync::oneshot::channel::<Result<(), String>>();

        let session = WorkerSession::new(worker_id, endpoint.to_string(), cap, cmd_tx.clone());

        // supervisor spawn.
        let supervisor_handle = spawn_supervisor(
            session.clone(),
            self.event_broadcaster.clone(),
            cmd_rx,
            Some(first_result_tx),
            self.reconnect,
            #[cfg(feature = "mtls")]
            self.client_tls.clone(),
        );
        *session.supervisor.lock().await = Some(supervisor_handle);

        // 초기 연결 결과 대기 — register()는 첫 연결이 성공해야 Ok 반환.
        // 실패해도 supervisor는 백그라운드에서 재시도 중이지만, 호출자에게 명확한 에러 전달.
        match first_result_rx.await {
            Ok(Ok(())) => {
                self.clients.write().await.insert(worker_id, session);
                Ok(())
            }
            Ok(Err(e)) => {
                // 첫 연결 실패 — supervisor가 계속 재시도 중이지만 register()는 에러 반환.
                // (사용자가 unregister 없이 다시 register 시도하지 않도록 주의해야 함.)
                Err(TransportError::Connection(format!(
                    "initial ACP connect failed for {worker_id}: {e}"
                )))
            }
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
        // WorkerSession::drop이 shutdown 전송 + supervisor abort 처리.
        drop(session);
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

        let session = {
            let clients = self.clients.read().await;
            clients
                .get(&worker_id)
                .cloned()
                .ok_or_else(|| TransportError::WorkerNotRegistered(worker_id.to_string()))?
        };

        // 연결 상태 확인 — Disconnected/Connecting인 경우 명확한 에러.
        let state = *session.state.read().await;
        if state != ConnState::Connected {
            return Err(TransportError::Connection(format!(
                "worker {worker_id} not connected (state={state:?}); cannot dispatch task {task_id}"
            )));
        }

        // 용량 검증 후 슬롯 확보 (in_flight에 등록).
        session.try_acquire(task_id).await?;

        // 백그라운드에서 prompt 실행.
        let session_clone = session.clone();
        let broadcaster = self.event_broadcaster.clone();
        let timeout_secs = req.timeout_secs;
        tokio::spawn(async move {
            let started = Instant::now();
            let prompt_str = req.prompt.clone();

            // (client, session_id)를 락 밖에서 빼냄 — await 중 락 유지 방지.
            let client_session: Option<(Arc<AcpClient>, SessionId)> = {
                let guard = session_clone.inner.lock().await;
                guard
                    .as_ref()
                    .map(|a| (a.client.clone(), a.session_id.clone()))
            };

            let result = match client_session {
                Some((client, session_id)) => {
                    run_prompt_with_timeout(&client, &session_id, &prompt_str, timeout_secs).await
                }
                None => Err("worker session disappeared mid-dispatch".to_string()),
            };

            match result {
                Ok(prompt_id) => {
                    // prompt_id를 in_flight와 prompt_index에 등록.
                    // 동시에, prompt_id가 알려지기 전에 도착해 버퍼링된
                    // 이벤트들을 drain하여 emit.
                    let buffered = session_clone.set_prompt_id(task_id, prompt_id).await;
                    for event in buffered {
                        match event {
                            BufferedEvent::Output { seq, chunk } => {
                                let _ = broadcaster.send(WorkerEvent::Output {
                                    task_id,
                                    seq,
                                    chunk,
                                });
                            }
                            BufferedEvent::Failed { error } => {
                                let _ = broadcaster.send(WorkerEvent::Failed { task_id, error });
                            }
                        }
                    }
                    debug!(
                        %task_id, %worker_id,
                        elapsed_secs = started.elapsed().as_secs_f64(),
                        "acp prompt accepted by server"
                    );
                    // Completed는 background reader가 AcpEvent::Completed에서 emit.
                }
                Err(e) => {
                    // prompt() 실패. 두 가지 시나리오를 구분:
                    // (a) 일반적인 prompt 실패 (timeout, malformed response, 서버 에러):
                    //     즉시 complete + Failed emit.
                    // (b) ACP 연결 종료로 인한 실패 (에러 메시지에 "ACP connection
                    //     closed" 포함): supervisor의 fail_all이 "reader exited"
                    //     에러로 emit할 것이므로 여기서는 in_flight를 그대로 둠.
                    if e.contains("ACP connection closed") {
                        debug!(
                            %task_id, %worker_id, error = %e,
                            "prompt failed due to ACP close — deferring to supervisor fail_all"
                        );
                        // in_flight에 그대로 두어 supervisor가 emit.
                    } else if session_clone.complete(task_id).await.is_some() {
                        warn!(%task_id, %worker_id, error = %e, "acp prompt failed");
                        let _ = broadcaster.send(WorkerEvent::Failed { task_id, error: e });
                    } else {
                        // 다른 스레드가 이미 complete — 중복 emit 방지.
                        debug!(
                            %task_id, %worker_id, error = %e,
                            "prompt failure overlapped with concurrent cleanup — skipping emit"
                        );
                    }
                }
            }
        });

        Ok(())
    }

    async fn cancel(&self, task_id: TaskId) -> Result<(), TransportError> {
        // task_id → (worker_id, prompt_id, session_id, client) 역조회.
        // 어느 워커에 있는지 모르므로 모든 워커를 순회. 동시에 여러 워커에
        // 같은 task_id가 있을 수는 없으므로 첫 번째 발견에서 return.
        let clients = self.clients.read().await;
        for (worker_id, session) in clients.iter() {
            let prompt_id = {
                let in_flight = session.in_flight.lock().await;
                match in_flight.get(&task_id) {
                    Some(t) => t.prompt_id,
                    None => continue, // 이 워커에 없음 — 다음 워커 시도.
                }
            };

            // in_flight에서 제거하지 않음 — Completed/Failed 이벤트가 정리.
            // 여기서 제거하면 서버가 이미 응답을 보낸 후 orphan 상태가 됨.
            let (client, session_id) = {
                let guard = session.inner.lock().await;
                match guard.as_ref() {
                    Some(a) => (a.client.clone(), a.session_id.clone()),
                    None => {
                        debug!(
                            %task_id, %worker_id,
                            "cancel: active session missing — likely disconnected, treating as idempotent success"
                        );
                        return Ok(());
                    }
                }
            };

            info!(%task_id, %worker_id, ?prompt_id, "sending ACP cancel");
            if let Some(pid) = prompt_id {
                client
                    .cancel(&session_id, pid)
                    .await
                    .map_err(|e| TransportError::Connection(format!("acp cancel: {e}")))?;
            } else {
                debug!(
                    %task_id,
                    "prompt_id not yet known — cancel will be applied once prompt is registered"
                );
            }
            return Ok(());
        }

        debug!(%task_id, "cancel: no active worker session found — task already terminal?");
        Ok(())
    }

    async fn ping(&self, worker_id: WorkerId) -> Result<Duration, TransportError> {
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
///
/// `first_result`가 Some이면 첫 번째 연결 시도의 결과를 전송.
/// None이면 register()가 이미 동기적으로 처리했거나 외부에서 사용하지 않음.
#[allow(clippy::too_many_arguments)]
fn spawn_supervisor(
    session: Arc<WorkerSession>,
    broadcaster: broadcast::Sender<WorkerEvent>,
    mut cmd_rx: mpsc::UnboundedReceiver<SupervisorCmd>,
    first_result: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    reconnect: ReconnectConfig,
    #[cfg(feature = "mtls")] client_tls: Option<Arc<crate::tls::ClientTlsConfig>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let worker_id = session.worker_id;
        let endpoint = session.endpoint.clone();
        let mut backoff = reconnect.initial;
        // 첫 연결 결과 채널 — 한 번만 전송하고 None으로 해제.
        let mut first_result = first_result;

        loop {
            // 1. 상태를 Connecting으로.
            *session.state.write().await = ConnState::Connecting;

            // 2. 연결 + 세션 열기 시도.
            let (client, session_id, event_rx) = match establish_session(
                &endpoint,
                #[cfg(feature = "mtls")]
                client_tls.as_deref(),
            )
            .await
            {
                Ok(t) => t,
                Err(e) => {
                    warn!(%worker_id, error = %e, "supervisor connect failed");
                    *session.state.write().await = ConnState::Disconnected;
                    // 첫 연결 실패 시 register() 호출자에게 에러 전달.
                    if let Some(tx) = first_result.take() {
                        let _ = tx.send(Err(e));
                    }
                    if wait_with_shutdown(&mut cmd_rx, backoff).await {
                        info!(%worker_id, "supervisor received shutdown during backoff");
                        return;
                    }
                    backoff = (backoff * 2).min(reconnect.max);
                    continue;
                }
            };

            info!(%worker_id, session = %session_id, "ACP session established");
            *session.state.write().await = ConnState::Connected;
            backoff = reconnect.initial; // 성공 시 백오프 리셋.
                                         // 첫 연결 성공 시 register() 호출자에게 Ok 전달.
            if let Some(tx) = first_result.take() {
                let _ = tx.send(Ok(()));
            }

            let client_arc = Arc::new(client);

            // 3. 활성 세션 등록 (reader보다 먼저 — dispatch가 즉시 접근 가능해야 함).
            *session.inner.lock().await = Some(ActiveSession {
                client: client_arc.clone(),
                session_id: session_id.clone(),
            });

            // 4. reader 루프 spawn. 이 핸들을 supervisor가 직접 추적.
            let reader_session = session.clone();
            let reader_broadcaster = broadcaster.clone();
            let reader_handle: JoinHandle<()> = tokio::spawn(async move {
                run_reader_loop(reader_session, reader_broadcaster, event_rx).await;
            });

            // 5. reader 종료 또는 shutdown 신호 대기.
            let reader_exit_reason = tokio::select! {
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(SupervisorCmd::Shutdown) => ReaderExit::Shutdown,
                        Some(_) => ReaderExit::Other,
                        None => ReaderExit::Shutdown, // sender drop
                    }
                }
                _ = reader_handle => ReaderExit::ReaderExited,
            };

            match reader_exit_reason {
                ReaderExit::Shutdown => {
                    info!(%worker_id, "supervisor received shutdown — closing");
                    session.clear_active().await;
                    *session.state.write().await = ConnState::Disconnected;
                    return;
                }
                ReaderExit::ReaderExited => {
                    warn!(%worker_id, "ACP reader exited — will reconnect");
                    // 진행 중인 **모든** 태스크를 Failed로 처리 (Phase 8.4).
                    session
                        .fail_all(&broadcaster, "ACP reader exited (connection lost)")
                        .await;
                    // 활성 세션 정리.
                    session.clear_active().await;
                    *session.state.write().await = ConnState::Disconnected;
                    // 백오프 후 재시도.
                    if wait_with_shutdown(&mut cmd_rx, backoff).await {
                        info!(%worker_id, "supervisor received shutdown during reconnect backoff");
                        return;
                    }
                    backoff = (backoff * 2).min(reconnect.max);
                    continue;
                }
                ReaderExit::Other => {
                    // 기타 신호 — shutdown과 동일하게 처리.
                    info!(%worker_id, "supervisor received non-shutdown cmd — treating as shutdown");
                    session.clear_active().await;
                    *session.state.write().await = ConnState::Disconnected;
                    return;
                }
            }
        }
    })
}

/// reader 종료 이유.
enum ReaderExit {
    Shutdown,
    ReaderExited,
    Other,
}

/// endpoint로 AcpClient를 연결하고 초기 세션을 엹.
/// 실패 시 재시도는 호출자(supervisor) 담당.
///
/// Phase 8.5: `client_tls` 가 `Some` 이고 endpoint 가 `wss://` 인 경우 mTLS 핸드셰이크.
/// `client_tls` 가 `Some` 인데 endpoint 가 `ws://` 인 경우 mTLS 없이 일반 TCP (경고 로그).
/// `client_tls` 가 `None` 인데 endpoint 가 `wss://` 인 경우 공용 CA (webpki-roots) 사용.
async fn establish_session(
    endpoint: &str,
    #[cfg(feature = "mtls")] client_tls: Option<&crate::tls::ClientTlsConfig>,
) -> Result<(AcpClient, SessionId, mpsc::UnboundedReceiver<AcpEvent>), String> {
    #[cfg(feature = "mtls")]
    let use_mtls = client_tls.is_some() && endpoint.starts_with("wss://");
    #[cfg(not(feature = "mtls"))]
    let use_mtls = false;

    let (client, event_rx) = if use_mtls {
        #[cfg(feature = "mtls")]
        {
            let tls = client_tls.expect("checked above");
            AcpClient::connect_mtls(endpoint, tls)
                .await
                .map_err(|e| format!("acp connect (mTLS): {e}"))?
        }
        #[cfg(not(feature = "mtls"))]
        {
            unreachable!("use_mtls requires the mtls feature")
        }
    } else {
        #[cfg(feature = "mtls")]
        if client_tls.is_some() && endpoint.starts_with("ws://") {
            warn!(
                endpoint = %sanitize_endpoint(endpoint),
                "client_tls configured but endpoint is ws:// — falling back to plain TCP"
            );
        }
        AcpClient::connect(endpoint)
            .await
            .map_err(|e| format!("acp connect: {e}"))?
    };
    let session_id = client
        .open_session(None)
        .await
        .map_err(|e| format!("acp open_session: {e}"))?;
    Ok((client, session_id, event_rx))
}

/// reader 종료 시 진행 중인 active_task가 있면 WorkerEvent::Failed emit.
///
/// Phase 8.4부터 이 함수는 사용되지 않습니다 — `WorkerSession::fail_all`이
/// 단일 호출로 모든 in-flight task를 실패 처리합니다. 호환성을 위해
/// #[cfg(test)] 에서만 노출.
#[cfg(test)]
#[allow(dead_code)]
async fn fail_active_task(
    session: &Arc<WorkerSession>,
    broadcaster: &broadcast::Sender<WorkerEvent>,
    reason: &str,
) {
    session.fail_all(broadcaster, reason).await;
}

/// backoff 대기 중 shutdown 신호가 오면 true 반환.
async fn wait_with_shutdown(
    cmd_rx: &mut mpsc::UnboundedReceiver<SupervisorCmd>,
    backoff: Duration,
) -> bool {
    match tokio::time::timeout(backoff, cmd_rx.recv()).await {
        // shutdown 또는 sender drop — 종료로 간주.
        Ok(Some(SupervisorCmd::Shutdown)) | Ok(None) => true,
        // Ping 등 기타 신호 — false.
        Ok(Some(_)) => false,
        // 타임아웃 정상 종료.
        Err(_) => false,
    }
}

/// 백그라운드 reader 루프. AcpEvent → WorkerEvent 변환.
///
/// Phase 8.4부터 다중 동시 task를 지원 — 각 이벤트는 `prompt_id` 기반으로
/// `session.prompt_index`에서 올바른 task_id를 찾아 라우팅됨. 단,
/// session/update notification이 session/prompt 응답보다 먼저 도착하는
/// 레이스 윈도우에서는 prompt_id가 아직 dispatch에 의해 등록되지 않았을
/// 수 있음 — 이 경우 이벤트를 버퍼에 넣고, dispatch의 set_prompt_id가
/// 호출될 때 drain되어 emit.
async fn run_reader_loop(
    session: Arc<WorkerSession>,
    broadcaster: broadcast::Sender<WorkerEvent>,
    mut event_rx: mpsc::UnboundedReceiver<AcpEvent>,
) {
    let worker_id = session.worker_id;
    while let Some(event) = event_rx.recv().await {
        match event {
            AcpEvent::Output {
                prompt_id,
                seq,
                chunk,
            } => {
                // prompt_id로 task_id 역조회. None인 경우 (드문 레이스)
                // 출력을 drop — Phase 8.4 동시성 모델에서는 단일 active_task
                // 가정이 더 이상 유효하지 않음.
                let task_id_opt: Option<TaskId> = match prompt_id {
                    Some(pid) => match session.task_for_prompt(pid).await {
                        Some(tid) => Some(tid),
                        None => {
                            // dispatch가 아직 prompt_id를 등록하지 않음.
                            // 버퍼에 저장 후 set_prompt_id 호출 시 emit.
                            session
                                .buffer_event(
                                    pid,
                                    BufferedEvent::Output {
                                        seq,
                                        chunk: chunk.clone(),
                                    },
                                )
                                .await;
                            debug!(
                                %worker_id, prompt_id = pid.0,
                                "buffered early Output — will flush on prompt_id register"
                            );
                            None
                        }
                    },
                    None => {
                        debug!(
                            %worker_id,
                            "Output event without prompt_id — cannot route in concurrent mode, dropping"
                        );
                        None
                    }
                };
                if let Some(task_id) = task_id_opt {
                    let _ = broadcaster.send(WorkerEvent::Output {
                        task_id,
                        seq,
                        chunk,
                    });
                }
            }
            AcpEvent::Completed { prompt_id, result } => {
                // prompt_id로 task 역조회. unknown인 경우 이미 complete된 task.
                let task_id_opt = match prompt_id {
                    Some(pid) => session.task_for_prompt(pid).await,
                    None => {
                        debug!(
                            %worker_id,
                            "Completed event without prompt_id — cannot route in concurrent mode, dropping"
                        );
                        None
                    }
                };
                if let Some(task_id) = task_id_opt {
                    let output = extract_output_text(&result);
                    let token_usage = result.usage.map(|u| TokenUsage {
                        input_tokens: u.input_tokens,
                        output_tokens: u.output_tokens,
                        cache_read_tokens: u.cache_read_input_tokens.unwrap_or(0),
                    });
                    let task_result = TaskResult {
                        output,
                        exit_code: 0,
                        duration_secs: 0.0,
                        token_usage,
                        worker_id,
                        finished_at: Utc::now(),
                    };
                    let _ = broadcaster.send(WorkerEvent::Completed {
                        task_id,
                        result: task_result,
                    });
                    session.complete(task_id).await;
                } else if let Some(pid) = prompt_id {
                    debug!(
                        %worker_id, prompt_id = pid.0,
                        "Completed event for unregistered prompt_id — likely set_prompt_id hasn't run yet; dropping (rare race)"
                    );
                }
            }
            AcpEvent::Failed { prompt_id, error } => {
                let task_id_opt: Option<TaskId> = match prompt_id {
                    Some(pid) => match session.task_for_prompt(pid).await {
                        Some(tid) => Some(tid),
                        None => {
                            // dispatch가 아직 prompt_id를 등록하지 않은 상태에서
                            // 서버가 Failed update를 보낸 경우 — 버퍼링.
                            session
                                .buffer_event(
                                    pid,
                                    BufferedEvent::Failed {
                                        error: error.clone(),
                                    },
                                )
                                .await;
                            debug!(
                                %worker_id, prompt_id = pid.0,
                                "buffered early Failed event"
                            );
                            None
                        }
                    },
                    None => {
                        debug!(
                            %worker_id,
                            "Failed event without prompt_id — cannot route in concurrent mode, dropping"
                        );
                        None
                    }
                };
                if let Some(task_id) = task_id_opt {
                    let _ = broadcaster.send(WorkerEvent::Failed { task_id, error });
                    session.complete(task_id).await;
                }
            }
        }
    }
    debug!(%worker_id, "ACP reader loop terminated");
}

/// prompt() 호출을 timeout과 함께 래핑.
async fn run_prompt_with_timeout(
    client: &Arc<AcpClient>,
    session_id: &SessionId,
    prompt: &str,
    timeout_secs: Option<u64>,
) -> Result<PromptId, String> {
    let timeout = timeout_secs
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_PROMPT_TIMEOUT);

    match tokio::time::timeout(timeout, client.prompt(session_id, prompt)).await {
        Ok(Ok(pid)) => Ok(pid),
        Ok(Err(e)) => Err(format!("acp prompt: {e}")),
        Err(_) => Err(format!("acp prompt timed out after {timeout:?}")),
    }
}

/// PromptResult.agent_message에서 텍스트를 추출.
fn extract_output_text(result: &crate::acp::PromptResult) -> String {
    let mut out = String::new();
    for block in &result.agent_message {
        if let Some(obj) = block.as_object() {
            if obj.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(t) = obj.get("text").and_then(|v| v.as_str()) {
                    out.push_str(t);
                }
            }
        }
    }
    out
}

/// endpoint에서 server-key 마스킹 (로깅용).
fn sanitize_endpoint(endpoint: &str) -> String {
    if let Some(idx) = endpoint.find("server-key=") {
        let start = idx + "server-key=".len();
        let end = endpoint[start..]
            .find(['&', '#'])
            .map(|e| start + e)
            .unwrap_or(endpoint.len());
        format!("{}<redacted>{}", &endpoint[..start], &endpoint[end..])
    } else {
        endpoint.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_endpoint_masks_server_key() {
        let s = sanitize_endpoint("ws://h:1/ws?server-key=topsecret");
        assert!(!s.contains("topsecret"));
        assert!(s.contains("<redacted>"));
    }

    #[test]
    fn extract_output_text_from_text_blocks() {
        let result = crate::acp::PromptResult {
            prompt_id: Some(1),
            agent_message: vec![
                serde_json::json!({"type": "text", "text": "foo"}),
                serde_json::json!({"type": "text", "text": "bar"}),
            ],
            end_of_turn: true,
            usage: None,
        };
        assert_eq!(extract_output_text(&result), "foobar");
    }

    #[test]
    fn extract_output_text_empty_when_no_text_blocks() {
        let result = crate::acp::PromptResult {
            prompt_id: Some(1),
            agent_message: vec![serde_json::json!({"type": "image", "url": "..."})],
            end_of_turn: true,
            usage: None,
        };
        assert_eq!(extract_output_text(&result), "");
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

    #[tokio::test]
    async fn wait_with_shutdown_returns_false_on_timeout() {
        let (_tx, mut rx) = mpsc::unbounded_channel::<SupervisorCmd>();
        // 10ms 타임아웃 — 즉시 반환.
        let got = wait_with_shutdown(&mut rx, Duration::from_millis(10)).await;
        assert!(!got);
    }

    #[tokio::test]
    async fn wait_with_shutdown_returns_true_on_shutdown() {
        let (tx, mut rx) = mpsc::unbounded_channel::<SupervisorCmd>();
        tx.send(SupervisorCmd::Shutdown).unwrap();
        let got = wait_with_shutdown(&mut rx, Duration::from_secs(60)).await;
        assert!(got);
    }

    #[tokio::test]
    async fn wait_with_shutdown_returns_true_on_sender_drop() {
        let (tx, mut rx) = mpsc::unbounded_channel::<SupervisorCmd>();
        drop(tx);
        let got = wait_with_shutdown(&mut rx, Duration::from_secs(60)).await;
        assert!(got);
    }
}
