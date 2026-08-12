//! 작업 디스패치 루프.
//!
//! `Dispatcher`는 작업을 비동기로 실행하고, 상태 변화를 Store에 반영하며,
//! CircuitBreaker에 결과를 기록합니다. grok-build의 PendingGuard RAII 패턴과
//! sync_running_gauge 패턴을 차용했습니다.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::mpsc;

use fleet_core::{
    CircuitState, FailureKind, FleetEvent, Task, TaskFailure, TaskId, TaskStatus, WorkerId,
};
use fleet_transport::{DispatchRequest, TransportError, WorkerEvent};
use tracing::{info, warn};

use crate::breaker::{BreakerState, Outcome};
use crate::state::FleetState;

/// 활성 작업 게이지 (pending + active). 모니터링용.
static RUNNING_GAUGE: AtomicUsize = AtomicUsize::new(0);

fn inc_running() {
    RUNNING_GAUGE.fetch_add(1, Ordering::Relaxed);
}
fn dec_running() {
    RUNNING_GAUGE.fetch_sub(1, Ordering::Relaxed);
}

/// 현재 실행 중인 작업 수 (pending + active).
pub fn running_count() -> usize {
    RUNNING_GAUGE.load(Ordering::Relaxed)
}

/// 작업 디스패처. submit()으로 작업을 받고 백그라운드에서 실행.
pub struct Dispatcher {
    state: Arc<FleetState>,
    /// 워커 이벤트 수신 (transport → dispatcher)
    event_rx: tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<WorkerEvent>>>,
}

impl Dispatcher {
    pub fn new(state: Arc<FleetState>) -> Self {
        Self {
            state,
            event_rx: tokio::sync::Mutex::new(None),
        }
    }

    /// MockTransport 등과 이벤트 채널 연결.
    pub async fn attach_event_receiver(&self, rx: mpsc::UnboundedReceiver<WorkerEvent>) {
        *self.event_rx.lock().await = Some(rx);
    }

    /// 이벤트 소비 루프 시작. transport에서 발생한 WorkerEvent를
    /// Store의 task status 업데이트로 변환.
    pub async fn run_event_loop(self: Arc<Self>) {
        let mut rx_guard = self.event_rx.lock().await;
        let Some(mut rx) = rx_guard.take() else {
            warn!("no event receiver attached, event loop idle");
            return;
        };
        drop(rx_guard);

        while let Some(event) = rx.recv().await {
            self.handle_worker_event(event).await;
        }
    }

    #[tracing::instrument(skip(self, event))]
    async fn handle_worker_event(&self, event: WorkerEvent) {
        match event {
            WorkerEvent::Completed { task_id, result } => {
                let worker_id = result.worker_id;
                let initial_state = self.get_worker_circuit_state(worker_id).await;
                let cb = self.state.breakers.get(worker_id, initial_state);
                let old_state = cb.state();
                cb.record(Outcome::Success);
                let new_state = cb.state();

                if old_state != new_state {
                    let from_state = match old_state {
                        BreakerState::Closed => CircuitState::Closed,
                        BreakerState::Open => CircuitState::Open,
                        BreakerState::HalfOpen => CircuitState::HalfOpen,
                    };
                    let to_state = match new_state {
                        BreakerState::Closed => CircuitState::Closed,
                        BreakerState::Open => CircuitState::Open,
                        BreakerState::HalfOpen => CircuitState::HalfOpen,
                    };
                    let _ = self
                        .state
                        .store
                        .update_worker_circuit_state(worker_id, to_state)
                        .await;
                    let _ = self
                        .state
                        .store
                        .append_event(&FleetEvent::worker_circuit_changed(
                            worker_id, from_state, to_state,
                        ))
                        .await;
                }

                self.state
                    .store
                    .update_task_status(task_id, &TaskStatus::Completed(result.clone()))
                    .await
                    .ok();

                let _ = self
                    .state
                    .store
                    .append_event(&FleetEvent::task_completed(task_id, worker_id, result))
                    .await;

                dec_running();
                info!(%task_id, %worker_id, "task completed");
            }
            WorkerEvent::Failed { task_id, error } => {
                // 현재 상태에서 worker_id 추출
                let worker_id = self.current_worker_of(task_id).await;

                if let Some(wid) = worker_id {
                    let initial_state = self.get_worker_circuit_state(wid).await;
                    let cb = self.state.breakers.get(wid, initial_state);
                    let old_state = cb.state();
                    cb.record(Outcome::Failure);
                    let new_state = cb.state();

                    if old_state != new_state {
                        let from_state = match old_state {
                            BreakerState::Closed => CircuitState::Closed,
                            BreakerState::Open => CircuitState::Open,
                            BreakerState::HalfOpen => CircuitState::HalfOpen,
                        };
                        let to_state = match new_state {
                            BreakerState::Closed => CircuitState::Closed,
                            BreakerState::Open => CircuitState::Open,
                            BreakerState::HalfOpen => CircuitState::HalfOpen,
                        };
                        let _ = self
                            .state
                            .store
                            .update_worker_circuit_state(wid, to_state)
                            .await;
                        let _ = self
                            .state
                            .store
                            .append_event(&FleetEvent::worker_circuit_changed(
                                wid, from_state, to_state,
                            ))
                            .await;
                    }
                }

                let failure = TaskFailure {
                    error,
                    kind: FailureKind::WorkerError,
                    worker_id,
                    attempts: 1,
                };
                self.state
                    .store
                    .update_task_status(task_id, &TaskStatus::Failed(failure.clone()))
                    .await
                    .ok();
                let _ = self
                    .state
                    .store
                    .append_event(&FleetEvent::task_failed(task_id, failure))
                    .await;

                dec_running();
                warn!(%task_id, "task failed");
            }
            WorkerEvent::Output {
                task_id,
                seq,
                chunk,
            } => {
                let _ = self.state.store.append_output(task_id, &chunk).await;
                tracing::debug!(%task_id, seq, "output chunk buffered");
            }
        }
    }

    /// 작업 상태에서 worker_id 추출 (Failed 이벤트 처리용).
    async fn current_worker_of(&self, task_id: TaskId) -> Option<WorkerId> {
        self.state
            .store
            .get_task(task_id)
            .await
            .ok()
            .flatten()
            .and_then(|t| match t.status {
                TaskStatus::Dispatched { worker_id, .. } => Some(worker_id),
                _ => None,
            })
    }

    /// 작업을 제출. 워커 선택 → dispatch → 백그라운드 실행.
    #[tracing::instrument(skip(self, task), fields(task_id = %task.id))]
    pub async fn submit(&self, task: Task) -> Result<TaskId, DispatchError> {
        let task_id = task.id;

        // 1. Store에 작업 저장
        self.state
            .store
            .insert_task(&task)
            .await
            .map_err(|e| DispatchError::Store(e.to_string()))?;

        // 2. TaskCreated 이벤트
        let _ = self
            .state
            .store
            .append_event(&FleetEvent::task_created(
                task_id,
                task.server_hint.clone(),
                task.created_by.clone(),
            ))
            .await;

        // 3-6. 워커 선택 → CircuitBreaker 확인 → dispatch. `submit()`은 최초
        // 제출 시도이므로 선택/회로 실패도 기존과 동일하게 즉시 `Failed`로
        // 마킹한다 (`mark_unavailable_as_failed = true`). 재조정 루프
        // ([`crate::reconcile::Reconciler`])는 같은 경로를 재사용하되 그 경우
        // `false`를 전달해 "지금 당장 용량이 없음"을 실패로 취급하지 않는다.
        self.dispatch_existing(task, true).await?;

        Ok(task_id)
    }

    /// 이미 Store에 존재하는 작업(= `insert_task`/`task_created` 이벤트가 이미
    /// 처리된 작업)에 대해 워커 선택 → CircuitBreaker 확인 → `Dispatched` 상태
    /// 전이 → transport dispatch를 수행한다.
    ///
    /// [`submit`](Self::submit)(최초 제출)과
    /// [`Reconciler`](crate::reconcile::Reconciler)(stale `Pending` 작업 재시도)가
    /// 공유하는 경로다 — 두 경로 모두 "이미 Store에 존재하는 작업을 실제로
    /// 워커에 붙이는" 동일한 ~80줄짜리 선택/회로/transport 로직이 필요하므로
    /// 중복 구현을 피하기 위해 여기로 추출했다.
    ///
    /// `mark_unavailable_as_failed`:
    /// - `true` — 워커 선택 실패 또는 CircuitOpen도 즉시 작업을 `Failed`로
    ///   마킹한다. `submit()`의 기존 동작을 그대로 보존하기 위함.
    /// - `false` — 위 두 경우 작업 상태를 건드리지 않고 `Pending`인 채로
    ///   둔다. 재조정 루프에서 사용 — "지금 당장 쓸 수 있는 워커가 없음"은
    ///   실패가 아니라 다음 tick에 재시도할 정상적인 정상 상태이기 때문이다.
    ///
    /// transport dispatch 자체의 실패(연결 오류 등, step 6)는 이 플래그와
    /// 무관하게 항상 `Failed`로 마킹한다 — 이건 재시도 여부와 상관없이
    /// "진짜" 에러이기 때문이다.
    pub(crate) async fn dispatch_existing(
        &self,
        mut task: Task,
        mark_unavailable_as_failed: bool,
    ) -> Result<(), DispatchError> {
        let task_id = task.id;

        // 3. 워커 선택
        let worker_id = match self.state.selector.select(&task).await {
            Ok(id) => id,
            Err(e) => {
                if mark_unavailable_as_failed {
                    // 선택 실패 → 작업을 Failed로 표시
                    let failure = TaskFailure {
                        error: e.to_string(),
                        kind: FailureKind::WorkerUnavailable,
                        worker_id: None,
                        attempts: 0,
                    };
                    self.mark_failed(task_id, failure).await;
                }
                return Err(DispatchError::NoWorker(e.to_string()));
            }
        };

        // 4. CircuitBreaker 체크
        let initial_state = self.get_worker_circuit_state(worker_id).await;
        let cb = self.state.breakers.get(worker_id, initial_state);
        let old_state = cb.state();
        let check_res = cb.check();
        let new_state = cb.state();

        if old_state != new_state {
            let from_state = match old_state {
                BreakerState::Closed => CircuitState::Closed,
                BreakerState::Open => CircuitState::Open,
                BreakerState::HalfOpen => CircuitState::HalfOpen,
            };
            let to_state = match new_state {
                BreakerState::Closed => CircuitState::Closed,
                BreakerState::Open => CircuitState::Open,
                BreakerState::HalfOpen => CircuitState::HalfOpen,
            };
            let _ = self
                .state
                .store
                .update_worker_circuit_state(worker_id, to_state)
                .await;
            let _ = self
                .state
                .store
                .append_event(&FleetEvent::worker_circuit_changed(
                    worker_id, from_state, to_state,
                ))
                .await;
        }

        if let Err(e) = check_res {
            if mark_unavailable_as_failed {
                let failure = TaskFailure {
                    error: format!("circuit open: {e}"),
                    kind: FailureKind::CircuitOpen,
                    worker_id: Some(worker_id),
                    attempts: 0,
                };
                self.mark_failed(task_id, failure).await;
            }
            return Err(DispatchError::CircuitOpen(worker_id));
        }

        // 5. Dispatched 상태로 전이
        task.status = TaskStatus::Dispatched {
            worker_id,
            started_at: Utc::now(),
        };
        self.state
            .store
            .update_task_status(task_id, &task.status)
            .await
            .map_err(|e| DispatchError::Store(e.to_string()))?;

        let _ = self
            .state
            .store
            .append_event(&FleetEvent::task_dispatched(task_id, worker_id))
            .await;

        // 6. Transport로 dispatch
        let prompt = if task.parent_task_id.is_some() {
            self.build_threaded_prompt(&task).await
        } else {
            task.prompt.clone()
        };
        let req = DispatchRequest {
            task_id,
            worker_id,
            prompt,
            cwd: task.cwd.clone(),
            model: task.model.clone(),
            max_turns: task.max_turns,
            timeout_secs: task.timeout_secs,
        };

        inc_running();

        if let Err(e) = self.state.transport.dispatch(req).await {
            // dispatch 자체 실패 (연결 등) — 재조정 여부와 무관하게 항상 실제
            // 실패로 취급한다.
            let old_state = cb.state();
            cb.record(Outcome::Failure);
            let new_state = cb.state();

            if old_state != new_state {
                let from_state = match old_state {
                    BreakerState::Closed => CircuitState::Closed,
                    BreakerState::Open => CircuitState::Open,
                    BreakerState::HalfOpen => CircuitState::HalfOpen,
                };
                let to_state = match new_state {
                    BreakerState::Closed => CircuitState::Closed,
                    BreakerState::Open => CircuitState::Open,
                    BreakerState::HalfOpen => CircuitState::HalfOpen,
                };
                let _ = self
                    .state
                    .store
                    .update_worker_circuit_state(worker_id, to_state)
                    .await;
                let _ = self
                    .state
                    .store
                    .append_event(&FleetEvent::worker_circuit_changed(
                        worker_id, from_state, to_state,
                    ))
                    .await;
            }

            let failure = TaskFailure {
                error: e.to_string(),
                kind: FailureKind::WorkerError,
                worker_id: Some(worker_id),
                attempts: 1,
            };
            self.mark_failed(task_id, failure).await;
            dec_running();
            return Err(DispatchError::Transport(e.to_string()));
        }

        info!(%task_id, %worker_id, "task dispatched");
        Ok(())
    }

    /// 스레드(연속 대화)의 이전 turn들을 새 prompt 앞에 이어붙인다.
    ///
    /// ACP에는 세션을 넘나드는 대화 연속 기능이 없다(태스크마다 새
    /// `session/new`) — 그래서 매 dispatch마다 스레드 히스토리를 텍스트로
    /// 재구성해 보낸다. `list_thread_tasks`는 조상뿐 아니라 스레드 전체를
    /// 반환하므로, 이 태스크보다 먼저 생성된 것만, 그리고 실제로 출력이 있는
    /// (`Completed`) 것만 골라 문맥으로 사용한다 — 실패한 turn은 이어붙일
    /// 유의미한 출력이 없으므로 건너뛴다.
    ///
    /// 저장된 `task.prompt` 자체는 건드리지 않는다 — 태스크 목록/상세 화면에는
    /// 사용자가 실제로 입력한 새 메시지만 보여야 한다. 재구성된 전체 문맥은
    /// dispatch 시점에만 조립되어 워커로 전송된다.
    async fn build_threaded_prompt(&self, task: &Task) -> String {
        let history = self
            .state
            .store
            .list_thread_tasks(task.thread_id)
            .await
            .unwrap_or_default();

        let mut context = String::new();
        for ancestor in &history {
            if ancestor.id == task.id || ancestor.created_at >= task.created_at {
                continue;
            }
            if let TaskStatus::Completed(result) = &ancestor.status {
                context.push_str(&format!("Q: {}\nA: {}\n\n", ancestor.prompt, result.output));
            }
        }

        if context.is_empty() {
            task.prompt.clone()
        } else {
            format!("{context}Q: {}", task.prompt)
        }
    }

    /// 작업을 실패로 마킹하고 이벤트 발행.
    ///
    /// `pub(crate)` — `Reconciler`가 orphaned `Dispatched` 작업(담당 워커가
    /// 재등록으로 사라진 경우)을 Failed로 전이시킬 때도 재사용한다.
    pub(crate) async fn mark_failed(&self, task_id: TaskId, failure: TaskFailure) {
        let _ = self
            .state
            .store
            .update_task_status(task_id, &TaskStatus::Failed(failure.clone()))
            .await;
        let _ = self
            .state
            .store
            .append_event(&FleetEvent::task_failed(task_id, failure))
            .await;
    }

    /// 작업을 취소.
    ///
    /// 허용 상태: `Pending` 또는 `Dispatched`. 이미 종료 상태(Completed/Failed/Cancelled)면
    /// 에러를 반환합니다. `Dispatched`인 경우 transport.cancel()로 워커 측에 취소를 전파하고,
    /// 그 후 Store 상태를 `Cancelled`로 전이합니다.
    ///
    /// **CircuitBreaker 고려**: 취소는 사용자 의도이므로 실패로 간주하지 않습니다.
    /// 따라서 브레이커에는 어떤 outcome도 기록하지 않습니다.
    #[tracing::instrument(skip(self, reason), fields(task_id = %task_id))]
    pub async fn cancel(
        &self,
        task_id: TaskId,
        reason: impl Into<String>,
    ) -> Result<(), CancelError> {
        let reason = reason.into();

        let task = self
            .state
            .store
            .get_task(task_id)
            .await
            .map_err(|e| CancelError::Store(e.to_string()))?
            .ok_or(CancelError::NotFound(task_id))?;

        // 이미 종료 상태인지 검사
        if task.is_terminal() {
            return Err(CancelError::AlreadyTerminal {
                task_id,
                phase: phase_label(&task.status),
            });
        }

        // Dispatched 상태면 워커에게 취소 통지
        let worker_id = match &task.status {
            TaskStatus::Dispatched { worker_id, .. } => Some(*worker_id),
            _ => None,
        };
        if let Some(wid) = worker_id {
            // transport.cancel은 best-effort — 워커가 이미 끝났을 수 있음.
            // 에러가 나도 상태 전이는 진행.
            if let Err(e) = self.state.transport.cancel(task_id).await {
                warn!(%task_id, %wid, error = %e, "transport.cancel failed, proceeding with status update");
            }
        }

        let cancelled = TaskStatus::Cancelled {
            reason,
            cancelled_at: Utc::now(),
        };

        self.state
            .store
            .update_task_status(task_id, &cancelled)
            .await
            .map_err(|e| CancelError::Store(e.to_string()))?;

        let _ = self
            .state
            .store
            .append_event(&FleetEvent::task_cancelled(
                task_id,
                // FleetEvent의 reason 필드에 들어감; cancelled 상태의 reason과 일치
                match &cancelled {
                    TaskStatus::Cancelled { reason, .. } => reason.clone(),
                    _ => unreachable!(),
                },
            ))
            .await;

        dec_running();
        info!(%task_id, "task cancelled");
        Ok(())
    }

    /// 작업이 종료 상태(Completed/Failed/Cancelled)에 도달할 때까지 대기.
    ///
    /// `timeout`이 지나면 `Err(WaitTimeout)` 반환. 종료 시 해당 `Task` 반환.
    /// 폴링 주기는 50ms (MCP 클라이언트의 동기적 호출 패턴에 적합).
    pub async fn wait_for_task(
        &self,
        task_id: TaskId,
        timeout: std::time::Duration,
    ) -> Result<Task, WaitError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let task = self
                .state
                .store
                .get_task(task_id)
                .await
                .map_err(|e| WaitError::Store(e.to_string()))?
                .ok_or(WaitError::NotFound(task_id))?;

            if task.is_terminal() {
                return Ok(task);
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(WaitError::Timeout { task_id, timeout });
            }

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    async fn get_worker_circuit_state(&self, id: WorkerId) -> CircuitState {
        self.state
            .store
            .get_worker(id)
            .await
            .ok()
            .flatten()
            .map(|w| w.circuit_state)
            .unwrap_or(CircuitState::Closed)
    }
}

/// `TaskStatus`의 위상 라벨 (에러 메시지용).
fn phase_label(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::Dispatched { .. } => "dispatched",
        TaskStatus::Completed(_) => "completed",
        TaskStatus::Failed(_) => "failed",
        TaskStatus::Cancelled { .. } => "cancelled",
    }
}

/// 디스패치 에러.
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("store error: {0}")]
    Store(String),

    #[error("no worker available: {0}")]
    NoWorker(String),

    #[error("circuit breaker open for worker {0}")]
    CircuitOpen(WorkerId),

    #[error("transport error: {0}")]
    Transport(String),
}

impl From<TransportError> for DispatchError {
    fn from(e: TransportError) -> Self {
        DispatchError::Transport(e.to_string())
    }
}

/// 작업 취소 에러.
#[derive(Debug, thiserror::Error)]
pub enum CancelError {
    #[error("store error: {0}")]
    Store(String),

    #[error("task not found: {0}")]
    NotFound(TaskId),

    #[error("task {task_id} already in terminal state '{phase}' — cannot cancel")]
    AlreadyTerminal {
        task_id: TaskId,
        phase: &'static str,
    },
}

/// 작업 대기 에러.
#[derive(Debug, thiserror::Error)]
pub enum WaitError {
    #[error("store error: {0}")]
    Store(String),

    #[error("task not found: {0}")]
    NotFound(TaskId),

    #[error("timed out waiting for task {task_id} after {timeout:?}")]
    Timeout {
        task_id: TaskId,
        timeout: std::time::Duration,
    },
}
