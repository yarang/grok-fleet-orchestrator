//! 작업 디스패치 루프.
//!
//! `Dispatcher`는 작업을 비동기로 실행하고, 상태 변화를 Store에 반영하며,
//! CircuitBreaker에 결과를 기록합니다. grok-build의 PendingGuard RAII 패턴과
//! sync_running_gauge 패턴을 차용했습니다.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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

    async fn handle_worker_event(&self, event: WorkerEvent) {
        match event {
            WorkerEvent::Completed { task_id, result } => {
                let worker_id = result.worker_id;
                let cb = self.state.breakers.get(worker_id);
                cb.record(Outcome::Success);

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
                    let cb = self.state.breakers.get(wid);
                    cb.record(Outcome::Failure);

                    let new_state = cb.state();
                    if matches!(new_state, BreakerState::Open) {
                        let _ = self
                            .state
                            .store
                            .append_event(&FleetEvent::worker_circuit_changed(
                                wid,
                                CircuitState::Closed,
                                CircuitState::Open,
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
            WorkerEvent::Output { task_id, seq, chunk } => {
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
    pub async fn submit(&self, mut task: Task) -> Result<TaskId, DispatchError> {
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

        // 3. 워커 선택
        let worker_id = match self.state.selector.select(&task).await {
            Ok(id) => id,
            Err(e) => {
                // 선택 실패 → 작업을 Failed로 표시
                let failure = TaskFailure {
                    error: e.to_string(),
                    kind: FailureKind::WorkerUnavailable,
                    worker_id: None,
                    attempts: 0,
                };
                self.mark_failed(task_id, failure).await;
                return Err(DispatchError::NoWorker(e.to_string()));
            }
        };

        // 4. CircuitBreaker 체크
        let cb = self.state.breakers.get(worker_id);
        if let Err(e) = cb.check() {
            let failure = TaskFailure {
                error: format!("circuit open: {e}"),
                kind: FailureKind::CircuitOpen,
                worker_id: Some(worker_id),
                attempts: 0,
            };
            self.mark_failed(task_id, failure).await;
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
        let req = DispatchRequest {
            task_id,
            worker_id,
            prompt: task.prompt.clone(),
            cwd: task.cwd.clone(),
            model: task.model.clone(),
            max_turns: task.max_turns,
            timeout_secs: task.timeout_secs,
        };

        inc_running();

        if let Err(e) = self.state.transport.dispatch(req).await {
            // dispatch 자체 실패 (연결 등)
            cb.record(Outcome::Failure);
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
        Ok(task_id)
    }

    /// 작업을 실패로 마킹하고 이벤트 발행.
    async fn mark_failed(&self, task_id: TaskId, failure: TaskFailure) {
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
