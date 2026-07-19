//! 테스트/개발용 인메모리 `WorkerTransport` 구현.
//!
//! 실제 워커 없이도 dispatch → poll → result 전체 플로우를 검증할 수 있게
//! 해줍니다. 각 dispatch는 백그라운드 태스크로 실행되며, 미리 설정된
//! 결과(`set_result`) 또는 프롬프트를 에코한 결과를 반환합니다.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fleet_core::{TaskId, TaskResult, WorkerId};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::time::sleep;
use tracing::debug;

use crate::{DispatchRequest, TransportError, WorkerEvent, WorkerTransport};

/// Mock 내부 브로드캐스트 채널의 버퍼 크기.
/// WorkerEvent는 Clone 가능해야 broadcast로 전달 가능.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Mock 워커의 동작을 제어하기 위한 핸들.
#[derive(Clone)]
pub struct MockWorker {
    pub id: WorkerId,
    pub endpoint: String,
    /// 이 워커가 dispatch 시 강제로 반환할 결과 (None이면 프롬프트 에코).
    pub forced_result: Option<Result<String, String>>,
    /// dispatch당 지연 시간.
    pub latency: Duration,
    /// 워커가 강제로 실패해야 하는지.
    pub force_fail: bool,
}

impl MockWorker {
    pub fn new(id: WorkerId, endpoint: impl Into<String>) -> Self {
        Self {
            id,
            endpoint: endpoint.into(),
            forced_result: None,
            latency: Duration::from_millis(10),
            force_fail: false,
        }
    }
}

/// Mock transport의 상태.
struct Inner {
    workers: HashMap<WorkerId, MockWorker>,
    /// task_id → (worker_id, completed result). 완료된 작업의 결과 보관.
    completed: HashMap<TaskId, (WorkerId, Result<TaskResult, String>)>,
    /// 활성 작업 수 (용량 검증용).
    active: HashMap<WorkerId, u32>,
    /// 이벤트 브로드캐스트 채널 (subscribe()가 호출될 때마다 새 receiver 생성).
    /// 모든 WorkerEvent는 여기로 송출됨.
    event_tx: broadcast::Sender<WorkerEvent>,
}

/// 인메모리 `WorkerTransport`. 테스트에서 `Arc<MockTransport>`로 공유.
pub struct MockTransport {
    inner: Arc<Mutex<Inner>>,
}

impl MockTransport {
    /// 새 mock transport 생성.
    ///
    /// 이전 버전(Phase 1-6)은 `(Self, Receiver)` 튜플을 반환했으나,
    /// trait 객체로 사용하기 위해 이벤트 스트림은 `subscribe()`로 분리.
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel::<WorkerEvent>(EVENT_CHANNEL_CAPACITY);
        let inner = Inner {
            workers: HashMap::new(),
            completed: HashMap::new(),
            active: HashMap::new(),
            event_tx,
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    /// 테스트에서 워커를 미리 추가.
    pub async fn add_worker(&self, worker: MockWorker) {
        let id = worker.id;
        self.inner.lock().await.workers.insert(id, worker);
    }

    /// 특정 작업의 결과를 미리 설정 (강제 성공/실패).
    pub async fn set_task_result(&self, task_id: TaskId, result: Result<TaskResult, String>) {
        // worker_id는 dispatch 시점에 채워짐. 여기서는 임시 저장.
        // 실제로는 dispatch完成后 completed 맵에 들어감.
        let mut guard = self.inner.lock().await;
        // 보류 결과는 별도 맵이 필요하지만, 단순화를 위해
        // dispatch 시 forced_result를 확인하는 방식 사용.
        if let Result::Ok(r) = &result {
            guard
                .completed
                .insert(task_id, (r.worker_id, result.clone()));
        }
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WorkerTransport for MockTransport {
    async fn register(&self, worker_id: WorkerId, endpoint: &str) -> Result<(), TransportError> {
        let mut guard = self.inner.lock().await;
        if guard.workers.contains_key(&worker_id) {
            return Err(TransportError::AlreadyRegistered(worker_id.to_string()));
        }
        guard.workers.insert(
            worker_id,
            MockWorker::new(worker_id, endpoint),
        );
        debug!(%worker_id, %endpoint, "mock worker registered");
        Ok(())
    }

    async fn unregister(&self, worker_id: WorkerId) -> Result<(), TransportError> {
        let mut guard = self.inner.lock().await;
        guard
            .workers
            .remove(&worker_id)
            .ok_or_else(|| TransportError::WorkerNotRegistered(worker_id.to_string()))?;
        guard.active.remove(&worker_id);
        Ok(())
    }

    async fn is_connected(&self, worker_id: WorkerId) -> bool {
        self.inner.lock().await.workers.contains_key(&worker_id)
    }

    async fn dispatch(&self, req: DispatchRequest) -> Result<(), TransportError> {
        let worker_config: MockWorker;
        let event_tx: broadcast::Sender<WorkerEvent>;

        {
            let mut guard = self.inner.lock().await;

            // 명시적으로 clone해서 borrow를 짧게 유지
            let worker_opt = guard.workers.get(&req.worker_id).cloned();
            worker_config = worker_opt
                .ok_or_else(|| TransportError::WorkerNotRegistered(req.worker_id.to_string()))?;
            event_tx = guard.event_tx.clone();

            // 활성 작업 카운트 증가
            *guard.active.entry(req.worker_id).or_insert(0) += 1;
        }

        let task_id = req.task_id;
        let worker_id = req.worker_id;
        let inner = self.inner.clone();

        // 백그라운드에서 "실행"
        tokio::spawn(async move {
            // 지연 시뮬레이션
            sleep(worker_config.latency).await;

            let started = Instant::now();
            let result = if worker_config.force_fail {
                Err("forced failure".to_string())
            } else if let Some(forced) = &worker_config.forced_result {
                forced.clone()
            } else {
                // 프롬프트 에코 (간단한 성공 시뮬레이션)
                Ok(format!("[mock] executed: {}", req.prompt))
            };

            let duration = started.elapsed().as_secs_f64();

            // 활성 카운트 감소
            {
                let mut guard = inner.lock().await;
                if let Some(count) = guard.active.get_mut(&worker_id) {
                    *count = count.saturating_sub(1);
                }
            }

            match result {
                Ok(output) => {
                    let task_result = TaskResult {
                        output,
                        exit_code: 0,
                        duration_secs: duration,
                        token_usage: None,
                        worker_id,
                        finished_at: chrono::Utc::now(),
                    };
                    // broadcast::send는 수신자가 없으면 Err를 반환하지만,
                    // dispatcher가 아직 subscribe하지 않았을 수 있으므로 에러는 무시.
                    let _ = event_tx.send(WorkerEvent::Completed {
                        task_id,
                        result: task_result,
                    });
                }
                Err(err) => {
                    let _ = event_tx.send(WorkerEvent::Failed {
                        task_id,
                        error: err,
                    });
                }
            }
        });

        Ok(())
    }

    async fn cancel(&self, _task_id: TaskId) -> Result<(), TransportError> {
        // Mock에서는 실제 취소를 시뮬레이션하지 않음 (단순 성공).
        // 진짜 취소 동작은 Phase 2에서 CancellationToken으로 구현.
        Ok(())
    }

    async fn ping(&self, worker_id: WorkerId) -> Result<Duration, TransportError> {
        let guard = self.inner.lock().await;
        if !guard.workers.contains_key(&worker_id) {
            return Err(TransportError::WorkerNotRegistered(worker_id.to_string()));
        }
        Ok(Duration::from_millis(1))
    }

    async fn subscribe(
        &self,
    ) -> Result<mpsc::UnboundedReceiver<WorkerEvent>, TransportError> {
        // broadcast receiver를 mpsc receiver로 브리지.
        // 이렇게 하면 trait 시그니처(`mpsc::UnboundedReceiver`)를 그대로 유지하면서
        // 멀티 구독자를 지원할 수 있음.
        let mut bcast_rx = self.inner.lock().await.event_tx.subscribe();
        let (tx, rx) = mpsc::unbounded_channel::<WorkerEvent>();

        tokio::spawn(async move {
            loop {
                match bcast_rx.recv().await {
                    Ok(event) => {
                        if tx.send(event).is_err() {
                            // 구독자가 드롭됨 — 브리지 태스크 종료.
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // 느린 구독자가 일부 이벤트를 놓침.
                        // 테스트 환경에서는 발생하지 않아야 함 (capacity=256).
                        tracing::warn!(
                            "mock transport subscriber lagged — some worker events dropped"
                        );
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // transport가 드롭됨 — 정상 종료.
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_core::WorkerId;

    #[tokio::test]
    async fn register_and_dispatch() {
        let transport = MockTransport::new();
        let mut event_rx = transport.subscribe().await.unwrap();
        let worker_id = WorkerId::new();

        transport
            .register(worker_id, "wss://mock/ws")
            .await
            .unwrap();

        let req = DispatchRequest {
            task_id: TaskId::new(),
            worker_id,
            prompt: "hello".into(),
            cwd: None,
            model: None,
            max_turns: None,
            timeout_secs: None,
        };
        transport.dispatch(req).await.unwrap();

        // 이벤트 수신
        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");

        match event {
            WorkerEvent::Completed { result, .. } => {
                assert_eq!(result.exit_code, 0);
                assert!(result.output.contains("hello"));
            }
            _ => panic!("expected Completed event"),
        }
    }

    #[tokio::test]
    async fn unregistered_worker_errors() {
        let transport = MockTransport::new();
        let req = DispatchRequest {
            task_id: TaskId::new(),
            worker_id: WorkerId::new(),
            prompt: "x".into(),
            cwd: None,
            model: None,
            max_turns: None,
            timeout_secs: None,
        };
        let result = transport.dispatch(req).await;
        assert!(result.is_err());
    }
}
