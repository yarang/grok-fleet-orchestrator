//! stale `Pending`/`Dispatched` 작업 재조정(reconciliation) 루프.
//!
//! [`Dispatcher::submit`](crate::dispatcher::Dispatcher::submit)은 작업 제출
//! 시점에 딱 한 번, 동기적으로 워커 선택 + dispatch를 시도한다. 이 시도가
//! 터미널 상태(`Dispatched`/`Failed`)에 도달하기 전에 중단되면 — 예를 들어
//! orchestrator 프로세스가 `insert_task()`로 `Pending`을 기록한 직후, 후속
//! 상태 갱신 전에 크래시/재시작하면 — 해당 작업은 영구히 `Pending`으로 고아가
//! 된다. 이후 온라인·유휴 워커가 나타나도 아무도 그 작업을 다시 들여다보지
//! 않는다 (프로덕션에서 실제로 관측됨: 워커 3대가 모두 온라인·유휴 상태였는데
//! 작업 하나가 `pending`으로 약 2일간 방치됨).
//!
//! `Dispatched`로 넘어간 뒤에도 별도의 고아 경로가 있다: 담당 워커가 재시작해
//! **새 `worker_id`로 재등록**되면(예: 워커 바이너리 재배포), 그 워커가
//! 실행 중이던 작업이 참조하던 옛 `worker_id`는 `workers` 테이블에서 완전히
//! 사라진다. 워커 자신은 그 작업의 존재를 전혀 모르므로(재시작 시 진행 중이던
//! 세션 상태가 날아감) 하트비트에도 `active_tasks`에 잡히지 않고, 작업은
//! `Dispatched`에 영원히 멈춘 채 아무 이벤트도 받지 못한다 — 대시보드
//! Overview의 "Active tasks" 카운트는 이 죽은 작업을 계속 세지만, 워커
//! 목록의 실제 active 카운트는 0으로 어긋난다 (프로덕션에서 실제로 관측됨:
//! 워커 3대 재배포 직후 재배포 이전에 dispatch된 작업 2건이 16시간 넘게
//! `dispatched`로 고정).
//!
//! [`Reconciler`]는 [`HealthChecker`](crate::health::HealthChecker) /
//! [`SessionCleanup`](crate::cleanup::SessionCleanup)과 동일한 "설정 + spawn +
//! JoinHandle 기반 abort" 패턴을 따르는 백그라운드 루프로, 매 tick마다 두 가지를
//! 스윕한다: `stale_after`보다 오래 `Pending`으로 머문 작업의 재dispatch,
//! `dispatched_worker_check_after`보다 오래됐는데 담당 워커가 더 이상 존재하지
//! 않는 `Dispatched` 작업의 `Failed` 전이.
//!
//! ## 설계 노트
//!
//! - 워커 선택/CircuitBreaker 확인/transport dispatch 로직은
//!   [`Dispatcher::dispatch_existing`](crate::dispatcher::Dispatcher::dispatch_existing)을
//!   그대로 재사용한다 — `submit()`의 `insert_task`/`task_created` 이벤트
//!   단계(작업이 이미 Store에 존재하므로 불필요)만 건너뛴다.
//! - 이번 라운드에도 사용 가능한 워커가 없으면(워커 선택 실패 또는
//!   CircuitOpen) `Pending` 상태를 그대로 유지한다 — 진짜 dispatch 에러
//!   (transport 연결 실패 등)만 `Failed`로 전이된다. "아직 용량 없음"은
//!   정상적인 정상 상태이므로 `warn`이 아니라 `debug`/`info` 레벨로만
//!   기록한다.
//! - `stale_after`는 정상적으로 진행 중인 `submit()` 호출(보통 수십~수백ms)과
//!   재조정 루프가 서로 경합하지 않도록, dispatch 왕복 시간보다 충분히 크게
//!   잡아야 한다 (기본값 60초).
//! - `dispatched_worker_check_after`는 "담당 워커가 store에서 완전히
//!   사라졌다"는 훨씬 강한 신호에 대한 최소 유예 시간이므로 `stale_after`보다
//!   짧게 잡아도 안전하다 (기본값 30초) — 정상적인 `dispatch_existing()`
//!   호출이 `update_task_status`를 커밋하는 사이의 아주 짧은 순간과만
//!   경합하면 되기 때문이다. 워커가 존재하되 단순히 응답이 느린 경우는 이
//!   경로가 건드리지 않는다 — 워커 자체의 헬스체크/CircuitBreaker가 그 경우를
//!   담당한다.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use fleet_core::{FailureKind, TaskFailure, TaskFilter, TaskStatus, TaskStatusFilter};

use crate::dispatcher::{DispatchError, Dispatcher};
use crate::state::FleetState;

/// 한 사이클에서 스캔할 최대 pending/dispatched 작업 수.
///
/// `TaskFilter`의 기본 limit(100)보다 넉넉하게 잡아, 대량의 stale 작업이
/// 쌓인 상황에서도 재조정 루프가 일부를 놓치지 않게 한다.
const MAX_PENDING_SCAN: usize = 1000;

/// 재조정 루프 설정.
#[derive(Debug, Clone)]
pub struct ReconcileConfig {
    /// 폴링 주기.
    pub interval: Duration,
    /// 이보다 오래 `Pending`으로 머문 작업만 재dispatch 대상으로 삼는다.
    /// 정상적으로 진행 중인 `submit()` 호출과 경합하지 않도록 dispatch
    /// 왕복 시간보다 충분히 크게 잡아야 한다.
    pub stale_after: Duration,
    /// 이보다 오래 `Dispatched`로 머문 작업 중 담당 워커가 store에서 완전히
    /// 사라진 것만 `Failed`로 전이한다. "워커 존재 여부"라는 강한 신호에
    /// 대한 최소 유예 시간이므로 `stale_after`보다 짧게 잡아도 안전하다.
    pub dispatched_worker_check_after: Duration,
}

impl Default for ReconcileConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            stale_after: Duration::from_secs(60),
            dispatched_worker_check_after: Duration::from_secs(30),
        }
    }
}

/// 단일 재조정 사이클 결과. 로깅/테스트에서 사용.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReconcileSummary {
    /// 재조정 대상으로 발견된 stale pending 작업 수.
    pub stale_found: u64,
    /// 이번 라운드에 성공적으로 재dispatch된 작업 수.
    pub redispatched: u64,
    /// 담당 워커가 사라진 것으로 발견된 stale dispatched 작업 수.
    pub orphaned_found: u64,
    /// 이번 라운드에 Failed로 전이시킨 orphaned dispatched 작업 수.
    pub orphaned_failed: u64,
}

/// stale `Pending` 작업 재조정기. spawn하면 백그라운드 태스크를 반환.
pub struct Reconciler {
    state: Arc<FleetState>,
    dispatcher: Arc<Dispatcher>,
    config: ReconcileConfig,
}

/// 백그라운드 재조정 루프 핸들. `abort()`로 종료.
pub struct ReconcilerHandle {
    inner: JoinHandle<()>,
}

impl ReconcilerHandle {
    /// 백그라운드 루프를 취소하고 종료 대기.
    pub async fn abort(self) {
        self.inner.abort();
        let _ = self.inner.await;
    }
}

impl Reconciler {
    pub fn new(
        state: Arc<FleetState>,
        dispatcher: Arc<Dispatcher>,
        config: ReconcileConfig,
    ) -> Self {
        Self {
            state,
            dispatcher,
            config,
        }
    }

    /// 백그라운드 루프 시작. `HealthChecker`와 동일하게 첫 틱을 기다린 뒤
    /// 시작한다 — 기동 직후 워커 등록/헬스체크가 아직 안정화되지 않은
    /// 상태에서 곧바로 재dispatch를 시도해 불필요한 "아직 용량 없음" 로그를
    /// 만들지 않기 위함이다.
    pub fn spawn(self) -> ReconcilerHandle {
        let handle = tokio::spawn(async move {
            self.run().await;
        });
        ReconcilerHandle { inner: handle }
    }

    async fn run(&self) {
        let mut interval = tokio::time::interval(self.config.interval);
        interval.tick().await;

        info!(
            interval = ?self.config.interval,
            stale_after = ?self.config.stale_after,
            dispatched_worker_check_after = ?self.config.dispatched_worker_check_after,
            "task reconciliation loop started (pending redispatch + orphaned dispatched reap)"
        );

        loop {
            interval.tick().await;
            self.reconcile_once().await;
        }
    }

    /// 단일 재조정 사이클. 테스트에서 직접 호출 가능.
    ///
    /// 스토어 조회가 실패해도 panic하지 않고 빈 요약을 반환한다 — 다음
    /// tick에서 재시도할 수 있도록 루프가 죽지 않아야 하기 때문
    /// (`HealthChecker`/`SessionCleanup`과 동일한 회복성 패턴).
    pub async fn reconcile_once(&self) -> ReconcileSummary {
        let pending = match self
            .state
            .store
            .list_tasks(&TaskFilter {
                status: Some(TaskStatusFilter::Pending),
                limit: MAX_PENDING_SCAN,
                ..Default::default()
            })
            .await
        {
            Ok(tasks) => tasks,
            Err(e) => {
                warn!(error = %e, "reconcile: failed to list pending tasks");
                return ReconcileSummary::default();
            }
        };

        let now = Utc::now();
        let stale_after = chrono::Duration::from_std(self.config.stale_after)
            .unwrap_or_else(|_| chrono::Duration::seconds(60));

        let mut summary = ReconcileSummary::default();

        for task in pending {
            let age = now - task.created_at;
            if age < stale_after {
                // 아직 신선함 — 정상 submit() 호출이 진행 중일 수 있으므로 건드리지 않음.
                continue;
            }

            summary.stale_found += 1;
            let task_id = task.id;

            // `false` — 선택 실패/CircuitOpen을 실패로 마킹하지 않고 Pending
            // 상태를 유지, 다음 tick에서 재시도한다.
            match self.dispatcher.dispatch_existing(task, false).await {
                Ok(()) => {
                    summary.redispatched += 1;
                    info!(%task_id, "reconciliation redispatched a stale pending task");
                }
                Err(DispatchError::NoWorker(reason)) => {
                    debug!(%task_id, %reason, "reconcile: still no capacity, leaving pending");
                }
                Err(DispatchError::CircuitOpen(worker_id)) => {
                    debug!(
                        %task_id, %worker_id,
                        "reconcile: selected worker's circuit is open, leaving pending"
                    );
                }
                Err(e) => {
                    // 진짜 dispatch 에러(transport 실패 등) — dispatch_existing이
                    // 이미 task를 Failed로 마킹했으므로 여기서는 로깅만 한다.
                    warn!(%task_id, error = %e, "reconcile: dispatch attempt failed");
                }
            }
        }

        self.reap_orphaned_dispatched(&mut summary).await;

        if summary.stale_found > 0 || summary.orphaned_found > 0 {
            info!(
                stale_found = summary.stale_found,
                redispatched = summary.redispatched,
                orphaned_found = summary.orphaned_found,
                orphaned_failed = summary.orphaned_failed,
                "reconciliation sweep completed"
            );
        }

        summary
    }

    /// `Dispatched` 작업 중 담당 워커가 store에서 완전히 사라진 것을 찾아
    /// `Failed(WorkerUnavailable)`로 전이한다. `summary`에 결과를 누적한다.
    async fn reap_orphaned_dispatched(&self, summary: &mut ReconcileSummary) {
        let dispatched = match self
            .state
            .store
            .list_tasks(&TaskFilter {
                status: Some(TaskStatusFilter::Dispatched),
                limit: MAX_PENDING_SCAN,
                ..Default::default()
            })
            .await
        {
            Ok(tasks) => tasks,
            Err(e) => {
                warn!(error = %e, "reconcile: failed to list dispatched tasks");
                return;
            }
        };

        let now = Utc::now();
        let check_after = chrono::Duration::from_std(self.config.dispatched_worker_check_after)
            .unwrap_or_else(|_| chrono::Duration::seconds(30));

        for task in dispatched {
            let TaskStatus::Dispatched {
                worker_id,
                started_at,
            } = task.status
            else {
                continue; // list_tasks 필터가 이미 보장하지만 방어적으로 스킵.
            };

            if now - started_at < check_after {
                // 방금 dispatch된 작업 — dispatch_existing()의 update_task_status
                // 커밋과 경합하지 않도록 최소 유예 시간을 둔다.
                continue;
            }

            let worker_exists = match self.state.store.get_worker(worker_id).await {
                Ok(w) => w.is_some(),
                Err(e) => {
                    // 조회 자체가 실패하면 판단할 수 없으므로 건드리지 않고 다음 tick에서 재시도.
                    warn!(%worker_id, error = %e, "reconcile: failed to check worker existence, skipping");
                    continue;
                }
            };

            if worker_exists {
                continue; // 워커는 존재 — 응답이 느릴 뿐이면 헬스체크/CircuitBreaker가 담당.
            }

            summary.orphaned_found += 1;
            let task_id = task.id;

            let failure = TaskFailure {
                error: format!(
                    "assigned worker {worker_id} no longer registered (likely restarted with a new worker id)"
                ),
                kind: FailureKind::WorkerUnavailable,
                worker_id: Some(worker_id),
                attempts: 0,
            };
            self.dispatcher.mark_failed(task_id, failure).await;
            summary.orphaned_failed += 1;
            warn!(
                %task_id, %worker_id,
                "reconciliation: dispatched task's worker no longer exists, marked failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::Dispatcher;
    use crate::state::FleetState;
    use async_trait::async_trait;
    use fleet_core::{
        BootstrapToken, CircuitBreakerConfig, EventEntry, FleetEvent, Task, TaskId, TaskOutput,
        TaskRequest, TaskStatus, Worker, WorkerFilter, WorkerHeartbeat, WorkerId, WorkerStatus,
    };
    use fleet_store::{Store, StoreError};
    use fleet_transport::{MockTransport, MockWorker};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// 인메모리 Store — reconcile 테스트 전용. 실제로 필요한 메서드만 동작하고
    /// 나머지는 이 테스트에서 호출되지 않으므로 `unimplemented!()`.
    struct MemStore {
        tasks: Mutex<HashMap<TaskId, Task>>,
        workers: Mutex<HashMap<WorkerId, Worker>>,
        events: Mutex<Vec<EventEntry>>,
        /// `true`이면 `list_tasks`가 항상 에러를 반환 (회복성 테스트용).
        fail_list_tasks: bool,
    }

    impl MemStore {
        fn new() -> Self {
            Self {
                tasks: Mutex::new(HashMap::new()),
                workers: Mutex::new(HashMap::new()),
                events: Mutex::new(Vec::new()),
                fail_list_tasks: false,
            }
        }

        fn failing_list_tasks() -> Self {
            Self {
                fail_list_tasks: true,
                ..Self::new()
            }
        }
    }

    #[async_trait]
    impl Store for MemStore {
        async fn insert_task(&self, t: &Task) -> Result<(), StoreError> {
            self.tasks.lock().unwrap().insert(t.id, t.clone());
            Ok(())
        }
        async fn get_task(&self, id: TaskId) -> Result<Option<Task>, StoreError> {
            Ok(self.tasks.lock().unwrap().get(&id).cloned())
        }
        async fn update_task_status(
            &self,
            id: TaskId,
            status: &TaskStatus,
        ) -> Result<(), StoreError> {
            let mut tasks = self.tasks.lock().unwrap();
            let Some(task) = tasks.get_mut(&id) else {
                return Err(StoreError::NotFound);
            };
            task.status = status.clone();
            if matches!(status, TaskStatus::Dispatched { .. }) {
                task.dispatched_at = Some(chrono::Utc::now());
            }
            Ok(())
        }
        async fn list_tasks(
            &self,
            filter: &fleet_core::TaskFilter,
        ) -> Result<Vec<Task>, StoreError> {
            if self.fail_list_tasks {
                return Err(StoreError::Unsupported("list_tasks forced failure"));
            }
            let tasks = self.tasks.lock().unwrap();
            let mut out: Vec<Task> = tasks
                .values()
                .filter(|t| match &filter.status {
                    Some(TaskStatusFilter::Pending) => matches!(t.status, TaskStatus::Pending),
                    Some(TaskStatusFilter::Dispatched) => {
                        matches!(t.status, TaskStatus::Dispatched { .. })
                    }
                    Some(TaskStatusFilter::Completed) => {
                        matches!(t.status, TaskStatus::Completed(_))
                    }
                    Some(TaskStatusFilter::Failed) => matches!(t.status, TaskStatus::Failed(_)),
                    Some(TaskStatusFilter::Cancelled) => {
                        matches!(t.status, TaskStatus::Cancelled { .. })
                    }
                    Some(TaskStatusFilter::Terminal) => t.is_terminal(),
                    Some(TaskStatusFilter::Active) => !t.is_terminal(),
                    None => true,
                })
                .cloned()
                .collect();
            out.sort_by_key(|t| t.created_at);
            out.truncate(filter.limit);
            Ok(out)
        }
        async fn upsert_worker(&self, w: &Worker) -> Result<(), StoreError> {
            self.workers.lock().unwrap().insert(w.id, w.clone());
            Ok(())
        }
        async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
            Ok(self.workers.lock().unwrap().get(&id).cloned())
        }
        async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError> {
            Ok(self
                .workers
                .lock()
                .unwrap()
                .values()
                .find(|w| w.name == name)
                .cloned())
        }
        async fn list_workers(&self, f: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
            let workers = self.workers.lock().unwrap();
            let out: Vec<Worker> = workers
                .values()
                .filter(|w| f.status.is_none_or(|s| w.status == s))
                .filter(|w| f.labels.iter().all(|(k, v)| w.labels.get(k) == Some(v)))
                .cloned()
                .collect();
            Ok(out)
        }
        async fn delete_worker(&self, id: WorkerId) -> Result<(), StoreError> {
            self.workers.lock().unwrap().remove(&id);
            Ok(())
        }
        async fn update_worker_heartbeat(
            &self,
            _: WorkerId,
            _: &WorkerHeartbeat,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn append_event(&self, e: &FleetEvent) -> Result<u64, StoreError> {
            let mut events = self.events.lock().unwrap();
            let seq = (events.len() + 1) as u64;
            events.push(EventEntry {
                seq,
                event: e.clone(),
            });
            Ok(seq)
        }
        async fn list_events(&self, _: u64, _: u32) -> Result<Vec<EventEntry>, StoreError> {
            Ok(self.events.lock().unwrap().clone())
        }
        async fn append_output(&self, _: TaskId, _: &str) -> Result<u64, StoreError> {
            unimplemented!()
        }
        async fn get_output(&self, _: TaskId, _: u64) -> Result<TaskOutput, StoreError> {
            unimplemented!()
        }
        async fn migrate(&self) -> Result<(), StoreError> {
            Ok(())
        }
        async fn create_bootstrap_token(&self, _: &BootstrapToken) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn consume_bootstrap_token(&self, _: &str, _: &str) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn list_bootstrap_tokens(&self) -> Result<Vec<BootstrapToken>, StoreError> {
            unimplemented!()
        }
        async fn revoke_bootstrap_token(&self, _: &str) -> Result<bool, StoreError> {
            unimplemented!()
        }
        async fn upsert_worker_credential(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: u32,
            _: Option<&str>,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_worker_credential(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<fleet_store::StoredCredential>, StoreError> {
            unimplemented!()
        }
        async fn list_worker_credentials(
            &self,
            _: &str,
        ) -> Result<Vec<fleet_store::StoredCredential>, StoreError> {
            unimplemented!()
        }
        async fn delete_worker_credential(&self, _: &str, _: &str) -> Result<bool, StoreError> {
            unimplemented!()
        }
    }

    /// FleetState + Dispatcher를 함께 조립. `mock_workers`가 있으면 transport에
    /// 등록하고 이벤트 루프를 백그라운드에서 실행한다.
    async fn setup(
        store: Arc<dyn Store>,
        mock_workers: Vec<MockWorker>,
    ) -> (Arc<FleetState>, Arc<Dispatcher>) {
        let transport = MockTransport::new();
        for mw in mock_workers {
            transport.add_worker(mw).await;
        }
        let event_rx = fleet_transport::WorkerTransport::subscribe(&transport)
            .await
            .unwrap();
        let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(transport);

        let state = Arc::new(FleetState::new(
            store,
            transport,
            CircuitBreakerConfig::default(),
        ));

        let dispatcher = Arc::new(Dispatcher::new(state.clone()));
        dispatcher.attach_event_receiver(event_rx).await;

        let bg = dispatcher.clone();
        tokio::spawn(async move {
            bg.run_event_loop().await;
        });

        (state, dispatcher)
    }

    fn make_worker(name: &str) -> Worker {
        let mut w = Worker::new(name, format!("wss://{name}/ws"));
        w.status = WorkerStatus::Online;
        w
    }

    /// `Pending` 상태의 작업을 지정된 나이(age)로 생성.
    fn make_pending_task(prompt: &str, age: chrono::Duration) -> Task {
        let mut task = Task::from_request(TaskRequest {
            prompt: prompt.into(),
            created_by: "test".into(),
            ..Default::default()
        });
        task.created_at = chrono::Utc::now() - age;
        task
    }

    /// `Dispatched { worker_id }` 상태의 작업을 지정된 경과 시간(started_at 기준)으로 생성.
    fn make_dispatched_task(prompt: &str, worker_id: WorkerId, age: chrono::Duration) -> Task {
        let mut task = Task::from_request(TaskRequest {
            prompt: prompt.into(),
            created_by: "test".into(),
            ..Default::default()
        });
        task.status = TaskStatus::Dispatched {
            worker_id,
            started_at: chrono::Utc::now() - age,
        };
        task
    }

    async fn wait_until_terminal(store: &dyn Store, task_id: TaskId) -> Task {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Ok(Some(task)) = store.get_task(task_id).await {
                if task.is_terminal() {
                    return task;
                }
            }
            if std::time::Instant::now() > deadline {
                panic!("task {task_id} did not reach terminal state within 2s");
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn stale_pending_task_is_redispatched_when_worker_available() {
        let worker = make_worker("idle-1");
        let worker_id = worker.id;

        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_pending_task("stale work", chrono::Duration::seconds(120));
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher) = setup(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, "wss://idle-1/ws")],
        )
        .await;

        let reconciler = Reconciler::new(
            state.clone(),
            dispatcher,
            ReconcileConfig {
                interval: Duration::from_secs(3600),
                stale_after: Duration::from_secs(60),
                dispatched_worker_check_after: Duration::from_secs(30),
            },
        );

        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary.stale_found, 1);
        assert_eq!(summary.redispatched, 1);

        // transport가 실제로 dispatch를 받아 처리를 완료했는지 확인.
        let completed = wait_until_terminal(state.store.as_ref(), task_id).await;
        match completed.status {
            TaskStatus::Completed(result) => assert_eq!(result.worker_id, worker_id),
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fresh_pending_task_is_left_untouched() {
        let worker = make_worker("idle-1");
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        // 5초 전에 생성됨 — stale_after(60s)보다 훨씬 신선함.
        let task = make_pending_task("fresh work", chrono::Duration::seconds(5));
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;

        let reconciler = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default());
        let summary = reconciler.reconcile_once().await;

        assert_eq!(summary.stale_found, 0, "fresh task should not be touched");
        assert_eq!(summary.redispatched, 0);

        let still_pending = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(still_pending.status, TaskStatus::Pending));
    }

    #[tokio::test]
    async fn stale_pending_task_without_capacity_stays_pending() {
        // 워커가 아예 없음 → selection 실패 → Pending 유지, Failed로 전이되면 안 됨.
        let store = Arc::new(MemStore::new());
        let task = make_pending_task("no capacity", chrono::Duration::seconds(120));
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;

        let reconciler = Reconciler::new(
            state.clone(),
            dispatcher,
            ReconcileConfig {
                interval: Duration::from_secs(3600),
                stale_after: Duration::from_secs(60),
                dispatched_worker_check_after: Duration::from_secs(30),
            },
        );

        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary.stale_found, 1);
        assert_eq!(
            summary.redispatched, 0,
            "no worker available — should not count as redispatched"
        );

        let still_pending = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(still_pending.status, TaskStatus::Pending),
            "task should remain Pending, not Failed: {:?}",
            still_pending.status
        );
    }

    #[tokio::test]
    async fn reconcile_once_tolerates_store_errors_without_panicking() {
        let store = Arc::new(MemStore::failing_list_tasks());
        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;

        let reconciler = Reconciler::new(state, dispatcher, ReconcileConfig::default());

        // panic 없이 빈 요약을 반환해야 함 — 다음 tick에서 재시도 가능하도록
        // 루프 자체가 죽지 않아야 하기 때문.
        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary, ReconcileSummary::default());
    }

    #[tokio::test]
    async fn orphaned_dispatched_task_with_missing_worker_is_marked_failed() {
        // 프로덕션에서 실제로 관측된 시나리오 재현: 워커가 재시작해 새
        // worker_id로 재등록되면서, 옛 worker_id로 dispatch됐던 작업이 고아가 됨.
        let ghost_worker_id = WorkerId::new();
        let store = Arc::new(MemStore::new());
        // 주의: ghost_worker_id는 절대 upsert_worker되지 않음 — "존재하지 않는 워커"를 재현.

        let task =
            make_dispatched_task("orphaned", ghost_worker_id, chrono::Duration::seconds(120));
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;

        let reconciler = Reconciler::new(
            state.clone(),
            dispatcher,
            ReconcileConfig {
                interval: Duration::from_secs(3600),
                stale_after: Duration::from_secs(60),
                dispatched_worker_check_after: Duration::from_secs(30),
            },
        );

        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary.orphaned_found, 1);
        assert_eq!(summary.orphaned_failed, 1);

        let failed = state.store.get_task(task_id).await.unwrap().unwrap();
        match failed.status {
            TaskStatus::Failed(failure) => {
                assert_eq!(failure.kind, FailureKind::WorkerUnavailable);
                assert_eq!(failure.worker_id, Some(ghost_worker_id));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatched_task_with_existing_worker_is_left_alone() {
        // 워커가 여전히 존재하면 (응답이 느릴 뿐일 수 있으므로) 건드리지 않는다 —
        // 헬스체크/CircuitBreaker의 책임 영역.
        let worker = make_worker("still-here");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task("still running", worker_id, chrono::Duration::seconds(120));
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;
        let reconciler = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default());

        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary.orphaned_found, 0);
        assert_eq!(summary.orphaned_failed, 0);

        let still_dispatched = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(
            still_dispatched.status,
            TaskStatus::Dispatched { .. }
        ));
    }

    #[tokio::test]
    async fn freshly_dispatched_orphan_is_left_untouched_within_grace_period() {
        // 워커가 없더라도, started_at이 dispatched_worker_check_after보다
        // 신선하면 dispatch_existing()의 커밋과 경합하지 않도록 건드리지 않는다.
        let ghost_worker_id = WorkerId::new();
        let store = Arc::new(MemStore::new());

        let task = make_dispatched_task(
            "just dispatched",
            ghost_worker_id,
            chrono::Duration::seconds(2),
        );
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;
        let reconciler = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default());

        let summary = reconciler.reconcile_once().await;
        assert_eq!(
            summary.orphaned_found, 0,
            "fresh dispatched task should not be touched even without a matching worker"
        );

        let still_dispatched = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(
            still_dispatched.status,
            TaskStatus::Dispatched { .. }
        ));
    }
}
