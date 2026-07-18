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
