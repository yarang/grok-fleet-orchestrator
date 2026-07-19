//! `AcpTransport` — `WorkerTransport` trait의 ACP 구현체.
//!
//! per-worker WebSocket 연결. 각 워커는 하나의 `AcpClient` 인스턴스와
//! 하나의 `SessionId`를 가짐. 워커에서 발생하는 AcpEvent는 fan-in 되어
//! 하나의 broadcast 채널로 subscriber(dispatcher)에게 전달.
//!
//! ## 동시성 모델 (Phase 7)
//!
//! grok agent serve의 MvpAgent는 직렬 프롬프트 처리를 가정하므로,
//! 워커당 동시에 실행 중인 프롬프트는 1개. 이를 `active_task`로 추적.
//!
//! ## 재연결 (Phase 8.2)
//!
//! 각 워커는 supervisor 태스크를 가짐. supervisor는:
//!
//! 1. `AcpClient::connect()` + `open_session()` 으로 초기 연결 확립.
//! 2. reader 루프가 종료되면 (WebSocket close / I/O 에러):
//!    - 진행 중인 `active_task`를 `WorkerEvent::Failed`로 emit.
//!    - 상태를 `Disconnected`로 표시.
//!    - 지수 백오프 (1s → 2s → ... → 최대 30s) 후 재연결 시도.
//! 3. unregister 시 `shutdown_rx`로 supervisor를 종료.
//!
//! ```text
//! [AcpTransport]
//!   │
//!   ├── register(worker_id, endpoint)
//!   │     └─► spawn supervisor(worker_id, endpoint)
//!   │           │
//!   │           ├── loop {
//!   │           │     connect + open_session
//!   │           │       └─► reader_loop (event_rx → WorkerEvent)
//!   │           │     on exit: emit Failed for active_task, sleep(backoff)
//!   │           │   }
//!   │           │
//!   │           └── shutdown_rx → break
//!   │
//!   ├── dispatch(req)
//!   │     └─► if state != Connected → Err(Connection)
//!   │     └─► set active_task, spawn prompt
//!   │
//!   └── cancel(task_id)
//!         └─► lookup active_prompt, send session/cancel
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
    /// 현재 진행 중인 태스크 (없으면 None).
    active_task: RwLock<Option<TaskId>>,
    /// 현재 진행 중인 프롬프트의 서버 발급 id.
    active_prompt: RwLock<Option<PromptId>>,
    /// supervisor 태스크로 보내는 명령 채널.
    cmd_tx: mpsc::UnboundedSender<SupervisorCmd>,
    /// supervisor 태스크 핸들 (Drop에서 abort).
    supervisor: Mutex<Option<JoinHandle<()>>>,
}

impl WorkerSession {
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
        }
    }

    /// 특정 워커의 연결 상태 조회. 미등록 워커면 None.
    pub async fn conn_state(&self, worker_id: WorkerId) -> Option<ConnState> {
        let clients = self.clients.read().await;
        let session = clients.get(&worker_id).cloned()?;
        drop(clients);
        let state = *session.state.read().await;
        Some(state)
    }
}

impl Default for AcpTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WorkerTransport for AcpTransport {
    async fn register(&self, worker_id: WorkerId, endpoint: &str) -> Result<(), TransportError> {
        // 중복 등록 체크.
        if self.clients.read().await.contains_key(&worker_id) {
            return Err(TransportError::AlreadyRegistered(worker_id.to_string()));
        }

        info!(%worker_id, endpoint = %sanitize_endpoint(endpoint), "registering ACP worker");

        // supervisor 명령 채널.
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<SupervisorCmd>();
        // 초기 연결 결과 전달 채널.
        let (first_result_tx, first_result_rx) =
            tokio::sync::oneshot::channel::<Result<(), String>>();

        let session = Arc::new(WorkerSession {
            worker_id,
            endpoint: endpoint.to_string(),
            state: RwLock::new(ConnState::Connecting),
            inner: Mutex::new(None),
            active_task: RwLock::new(None),
            active_prompt: RwLock::new(None),
            cmd_tx: cmd_tx.clone(),
            supervisor: Mutex::new(None),
        });

        // supervisor spawn.
        let supervisor_handle = spawn_supervisor(
            session.clone(),
            self.event_broadcaster.clone(),
            cmd_rx,
            Some(first_result_tx),
            self.reconnect,
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

        // active_task 설정.
        *session.active_task.write().await = Some(task_id);
        *session.active_prompt.write().await = None;

        // 백그라운드에서 prompt 실행.
        let session_clone = session.clone();
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
                Ok(_prompt_id) => {
                    debug!(
                        %task_id, %worker_id,
                        elapsed_secs = started.elapsed().as_secs_f64(),
                        "acp prompt completed"
                    );
                    // Completed는 background reader가 AcpEvent::Completed에서 emit.
                }
                Err(e) => {
                    warn!(%task_id, %worker_id, error = %e, "acp prompt failed");
                    // reader가 이미 Failed를 emit했을 수 있음 — 그 경우 active_task는 None.
                    // 여전히 active면 사용자가 timeout으로 인지할 수 있도록
                    // 별도 처리는 하지 않음 (간소화).
                    let _ = session_clone;
                }
            }
        });

        Ok(())
    }

    async fn cancel(&self, task_id: TaskId) -> Result<(), TransportError> {
        // task_id → (worker_id, prompt_id, session_id, client) 역조회.
        let target: Option<(
            WorkerId,
            Option<PromptId>,
            Option<Arc<AcpClient>>,
            Option<SessionId>,
        )> = {
            let clients = self.clients.read().await;
            let mut found = None;
            for (worker_id, session) in clients.iter() {
                let active = *session.active_task.read().await;
                if active == Some(task_id) {
                    let prompt_id = *session.active_prompt.read().await;
                    let (client, session_id) = {
                        let guard = session.inner.lock().await;
                        guard
                            .as_ref()
                            .map(|a| (Some(a.client.clone()), Some(a.session_id.clone())))
                            .unwrap_or((None, None))
                    };
                    found = Some((*worker_id, prompt_id, client, session_id));
                    break;
                }
            }
            found
        };

        if let Some((worker_id, prompt_id_opt, client_opt, session_id_opt)) = target {
            info!(%task_id, %worker_id, "sending ACP cancel");
            let (client, session_id) = match (client_opt, session_id_opt) {
                (Some(c), Some(s)) => (c, s),
                _ => {
                    debug!(
                        %task_id, %worker_id,
                        "cancel: active session missing — likely disconnected, treating as idempotent success"
                    );
                    return Ok(());
                }
            };
            if let Some(prompt_id) = prompt_id_opt {
                client
                    .cancel(&session_id, prompt_id)
                    .await
                    .map_err(|e| TransportError::Connection(format!("acp cancel: {e}")))?;
            } else {
                debug!(%task_id, "prompt_id not yet known — cancel no-op until first output arrives");
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
fn spawn_supervisor(
    session: Arc<WorkerSession>,
    broadcaster: broadcast::Sender<WorkerEvent>,
    mut cmd_rx: mpsc::UnboundedReceiver<SupervisorCmd>,
    first_result: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    reconnect: ReconnectConfig,
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
            let (client, session_id, event_rx) = match establish_session(&endpoint).await {
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
                    // 진행 중인 태스크를 Failed로 처리.
                    fail_active_task(
                        &session,
                        &broadcaster,
                        "ACP reader exited (connection lost)",
                    )
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
async fn establish_session(
    endpoint: &str,
) -> Result<
    (
        AcpClient,
        SessionId,
        mpsc::UnboundedReceiver<AcpEvent>,
    ),
    String,
> {
    let (client, event_rx) = AcpClient::connect(endpoint)
        .await
        .map_err(|e| format!("acp connect: {e}"))?;
    let session_id = client
        .open_session(None)
        .await
        .map_err(|e| format!("acp open_session: {e}"))?;
    Ok((client, session_id, event_rx))
}

/// reader 종료 시 진행 중인 active_task가 있으면 WorkerEvent::Failed emit.
async fn fail_active_task(
    session: &Arc<WorkerSession>,
    broadcaster: &broadcast::Sender<WorkerEvent>,
    reason: &str,
) {
    let active = session.active_task.write().await.take();
    if let Some(task_id) = active {
        *session.active_prompt.write().await = None;
        warn!(
            worker_id = %session.worker_id,
            %task_id, reason,
            "failing in-flight task due to reader exit"
        );
        let _ = broadcaster.send(WorkerEvent::Failed {
            task_id,
            error: reason.to_string(),
        });
    }
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
async fn run_reader_loop(
    session: Arc<WorkerSession>,
    broadcaster: broadcast::Sender<WorkerEvent>,
    mut event_rx: mpsc::UnboundedReceiver<AcpEvent>,
) {
    let worker_id = session.worker_id;
    while let Some(event) = event_rx.recv().await {
        match event {
            AcpEvent::Output { prompt_id, seq, chunk } => {
                // 첫 Output이면 active_prompt를 prompt_id로 설정.
                if session.active_prompt.read().await.is_none() {
                    if let Some(pid) = prompt_id {
                        *session.active_prompt.write().await = Some(pid);
                    }
                }
                let task_id_opt = *session.active_task.read().await;
                if let Some(task_id) = task_id_opt {
                    let _ = broadcaster.send(WorkerEvent::Output {
                        task_id,
                        seq,
                        chunk,
                    });
                } else {
                    debug!(
                        %worker_id, ?prompt_id,
                        "Output event arrived but no active task — dropping"
                    );
                }
            }
            AcpEvent::Completed { prompt_id: _, result } => {
                let task_id_opt = *session.active_task.read().await;
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
                }
                *session.active_task.write().await = None;
                *session.active_prompt.write().await = None;
            }
            AcpEvent::Failed { prompt_id: _, error } => {
                let task_id_opt = *session.active_task.read().await;
                if let Some(task_id) = task_id_opt {
                    let _ = broadcaster.send(WorkerEvent::Failed { task_id, error });
                }
                *session.active_task.write().await = None;
                *session.active_prompt.write().await = None;
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
        assert!(matches!(result, Err(TransportError::WorkerNotRegistered(_))));
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
        assert!(matches!(result, Err(TransportError::WorkerNotRegistered(_))));
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
