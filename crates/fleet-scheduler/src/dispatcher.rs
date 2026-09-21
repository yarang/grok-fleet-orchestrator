//! 작업 디스패치 루프.
//!
//! `Dispatcher`는 작업을 비동기로 실행하고, 상태 변화를 Store에 반영하며,
//! CircuitBreaker에 결과를 기록합니다. grok-build의 PendingGuard RAII 패턴과
//! sync_running_gauge 패턴을 차용했습니다.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::mpsc;

use fleet_core::{
    CircuitState, FailureKind, FleetEvent, IdempotentInsert, Task, TaskFailure, TaskId, TaskPhase,
    TaskStatus, TransitionOrigin, TransitionOutcome, WorkerId,
};
use fleet_store::ControlFence;
use fleet_transport::{DispatchRequest, FailureObservation, TransportError, WorkerEvent};
use tracing::{info, warn};

use crate::breaker::{BreakerState, Outcome};
use crate::router::TaskRouter;
use crate::selector::SelectionError;
use crate::skill_loader::inject_skills;
use crate::state::FleetState;

/// dispatch 전 인벤토리 조회의 응답 대기 시간 (로드맵 `#70` 게이트 2).
///
/// 플래그로 열지 않는다. 이 값이 하는 일은 "묻지 못했다"를 언제 선언할지
/// 하나뿐이고, 늦으면 그 워커로의 dispatch가 한 번 거절되며 다음 시도에서
/// 다시 묻는다 — 운영자가 조율할 이유가 아직 없다. `ReconcileConfig::
/// session_list_timeout`과 같은 값으로 맞춰 둔 것은 같은 왕복이기 때문이다.
const WORKER_RECOVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

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
    /// `submit()`이 `WorkerUnavailable`/`CircuitOpen`을 몇 번까지 "재시도
    /// 가능한 일시적 실패"로 취급할지 (로드맵 #38). 기본값 0 = 재시도 없음
    /// (이 필드 도입 이전과 동일한 즉시 `Failed` 동작). `fleet serve`는
    /// [`with_max_dispatch_retries`](Self::with_max_dispatch_retries)로
    /// `ReconcileConfig::max_dispatch_retries`(기본 20)와 동일한 값을
    /// 명시적으로 설정한다 — Dispatcher와 Reconciler가 같은 기준으로 재시도
    /// 소진 여부를 판단해야 하기 때문이다.
    max_dispatch_retries: u32,
    /// pin된 AgentTemplate revision의 tool 허용 목록 캐시 (로드맵 `#64`·`#86` 선행).
    ///
    /// **revision id로 캐싱해도 안전한 이유**는 `#86`의 요지가 revision
    /// immutability이기 때문이다 — pin된 revision의 본문은 바뀌지 않으므로
    /// 무효화 문제가 없다. Agent id로 캐싱했다면 pin이 옮겨갈 때 낡은 목록을
    /// 쓰게 된다.
    ///
    /// 캐시를 두는 이유는 이 판정이 **스트리밍 경로**에서 돌기 때문이다.
    /// 도구 호출마다 Agent와 revision을 조회하면 왕복 둘이 붙는다.
    tool_allowlist_cache: tokio::sync::Mutex<
        std::collections::HashMap<fleet_core::AgentTemplateRevisionId, Arc<HashSet<String>>>,
    >,
    /// 이 제어 세대에서 **인벤토리를 이미 물어본** 워커들 (로드맵 `#70` 게이트 2).
    ///
    /// 게이트 문언은 "control failover 뒤 inventory-first recovery가 신규
    /// dispatch보다 **먼저** 수행"을 요구한다. 그 순서를 배경 루프의 타이밍에
    /// 맡기면 순서가 아니라 경합이 된다 — `Reconciler`는 첫 tick을 30초 뒤에
    /// 돌고, 그 사이의 dispatch는 아무것도 묻지 않은 채 나간다. 그래서 여기서
    /// **dispatch의 전제조건**으로 만든다: 구조적으로 어길 수 없다.
    ///
    /// **전역 배리어가 아니라 워커별인 이유**는 "모든 워커가 재연결했다"는
    /// 시점이 존재하지 않기 때문이다. 워커는 자기가 join할 때만 transport에
    /// 등록되고, 영영 돌아오지 않는 워커도 있다. 전역 배리어를 두면 그런 워커
    /// 하나가 fleet 전체의 dispatch를 영구히 막는다. 물어볼 수 있는 시점은
    /// **그 워커에게 실제로 일을 주려는 순간**뿐이고, 그때가 곧 "먼저"가
    /// 성립해야 하는 유일한 자리다.
    ///
    /// 인메모리인 것이 의도다. 프로세스가 죽으면 비워지고, 그것이 정확히 이
    /// 복구가 다시 필요해지는 조건이다. `i64`는 lease epoch이며(lease를 켜지
    /// 않은 배포는 `None`), 세대가 바뀌면 집합을 통째로 버린다 — 새 제어
    /// 기관은 이전 기관이 물어본 것을 물려받지 않는다.
    recovered_workers: tokio::sync::Mutex<(Option<i64>, HashSet<WorkerId>)>,
}

impl Dispatcher {
    pub fn new(state: Arc<FleetState>) -> Self {
        Self {
            state,
            event_rx: tokio::sync::Mutex::new(None),
            max_dispatch_retries: 0,
            tool_allowlist_cache: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            recovered_workers: tokio::sync::Mutex::new((None, HashSet::new())),
        }
    }

    /// 이 인스턴스가 지금 쓰기에 걸어야 할 control-plane 술어
    /// (로드맵 `#67` 구현 게이트 ①-B).
    ///
    /// Dashboard는 `FleetState`를 갖지 않는다 — `DashboardState`가 `store`와
    /// `Option<Arc<Dispatcher>>`만 든다. 그런데 Agent 명령
    /// 발행(`assign_agent_worker`·`set_agent_desired_status`)은 dispatch와
    /// **같은** 제어권 아래 있어야 하므로 그 표면도 같은 fence를 얻어야 한다.
    /// `state`를 공개하는 대신 이 값 하나만 내보낸다 — 표면이 store를 직접
    /// 부르는 것은 이미 하고 있는 일이고, 넓히는 것은 그 호출에 붙는 술어뿐이다.
    /// MCP는 `ToolContext`가 `Arc<FleetState>`를 들고 있어 이 통로가 필요 없고,
    /// [`FleetState::control_fence`](crate::state::FleetState::control_fence)를
    /// 직접 부른다.
    ///
    /// lease를 켜지 않은 배포는 `None`이며, 그 경우 store 쪽 술어도 붙지
    /// 않는다(`FleetState::control_fence`와 같은 이유).
    pub fn control_fence(&self) -> Option<ControlFence> {
        self.state.control_fence()
    }

    /// 회로 상태가 바뀌었으면 저장소와 이벤트 로그에 반영한다.
    ///
    /// 이 블록은 원래 dispatch 경로에 두 번 인라인돼 있었다. 세 번째 사본을
    /// 만드는 대신 여기로 뽑았다 — 기존 둘은 그대로 두었다(같은 커밋에서
    /// 건드리면 복구 경로의 변경과 리팩터가 한 diff에 섞인다).
    async fn persist_breaker_transition(
        &self,
        worker_id: WorkerId,
        old_state: BreakerState,
        new_state: BreakerState,
    ) {
        if old_state == new_state {
            return;
        }
        let to_circuit = |s: BreakerState| match s {
            BreakerState::Closed => CircuitState::Closed,
            BreakerState::Open => CircuitState::Open,
            BreakerState::HalfOpen => CircuitState::HalfOpen,
        };
        let _ = self
            .state
            .store
            .update_worker_circuit_state(worker_id, to_circuit(new_state))
            .await;
        let _ = self
            .state
            .store
            .append_event(&FleetEvent::worker_circuit_changed(
                worker_id,
                to_circuit(old_state),
                to_circuit(new_state),
            ))
            .await;
    }

    /// 이 워커에게 일을 주기 **전에** 무엇을 들고 있는지 묻는다
    /// (로드맵 `#70` 게이트 2 — inventory-first recovery).
    ///
    /// 이 제어 세대에서 이미 물어본 워커면 즉시 통과한다. 그렇지 않으면
    /// 지금 묻고, 답에 따라 이 워커의 어긋난 Task를 먼저 정리한 뒤에야
    /// dispatch를 허용한다.
    ///
    /// **"물어봤다"가 통과 조건이지 "답을 받았다"가 아니다.** 인벤토리를
    /// 광고하지 않는 Agent(`Undeclared`)와 선언과 어긋난 Agent(`Refused`)도
    /// 통과시킨다 — 그러지 않으면 오늘의 거의 모든 배포에서 dispatch가
    /// 영구히 막힌다. 막는 것은 **묻지 못한** 경우뿐이고, 그때는 어차피 그
    /// 워커로 일을 보낼 수도 없다.
    ///
    /// 되돌려주는 값은 이 호출이 실제로 복구를 수행했는지다. 시험이 "두 번째
    /// dispatch는 다시 묻지 않는다"를 보기 위한 것이고, 운영에서는 로그로만
    /// 쓰인다.
    async fn ensure_worker_recovered(&self, worker_id: WorkerId) -> Result<bool, DispatchError> {
        let generation = self.state.lease.as_ref().and_then(|l| l.status().epoch());

        {
            let mut ledger = self.recovered_workers.lock().await;
            if ledger.0 != generation {
                // 제어 세대가 바뀌었다 — 이전 기관이 물어본 것은 이 기관의
                // 근거가 아니다. 통째로 버린다.
                ledger.0 = generation;
                ledger.1.clear();
            }
            if ledger.1.contains(&worker_id) {
                return Ok(false);
            }
        }

        let inventory = match self
            .state
            .transport
            .list_sessions(worker_id, WORKER_RECOVERY_TIMEOUT)
            .await
        {
            Ok(inv) => inv,
            Err(e) => {
                // **여기서 통과시키면 게이트가 사라진다.** 묻지 못한 것을
                // "물어본 것"으로 적으면 이 워커는 이 세대 내내 다시는
                // 조회되지 않는다.
                warn!(
                    %worker_id, error = %e,
                    "dispatch refused — could not read the worker's session inventory"
                );
                return Err(DispatchError::RecoveryUnavailable(worker_id));
            }
        };

        if let fleet_transport::SessionInventory::Reported(live) = &inventory {
            self.reap_vanished_on_worker(worker_id, live).await;
        }

        // 무엇을 근거로 이 워커를 dispatch 가능하다고 판정했는지 남긴다.
        // `Undeclared`가 대부분일 것이고, **그 사실 자체가 기록할 값어치가
        // 있다** — 이 fleet에서 게이트가 실질적으로 비어 있다는 뜻이기 때문이다.
        self.state
            .audit_decision(
                fleet_core::audit::action::CONTROL_WORKER_RECOVERED,
                ("worker", worker_id.to_string()),
                serde_json::json!({
                    "inventory": inventory.label(),
                    "authoritative": inventory.is_authoritative(),
                }),
            )
            .await;

        self.recovered_workers.lock().await.1.insert(worker_id);
        Ok(true)
    }

    /// 이 워커에 배정된 `Dispatched` Task 중 인벤토리에 세션이 없는 것을
    /// `Failed(ExecutionVanished)`로 회수한다.
    ///
    /// `Reconciler`의 주기 스윕과 **같은 규칙**이고 고르는 대상만 다르다 —
    /// 저쪽은 모든 워커를 유예 시간과 함께 훑고, 이쪽은 한 워커를 유예 없이
    /// 본다. 유예가 필요 없는 이유는 이 경로가 **제어 세대마다 한 번**만
    /// 돌기 때문이다: 이 세대에서 아직 아무것도 dispatch하지 않았으므로
    /// "방금 dispatch돼서 아직 세션이 열리지 않은 Task"가 존재할 수 없다.
    async fn reap_vanished_on_worker(&self, worker_id: WorkerId, live: &[String]) {
        let dispatched = match self
            .state
            .store
            .list_tasks(&fleet_core::TaskFilter {
                status: Some(fleet_core::TaskStatusFilter::Dispatched),
                worker_id: Some(worker_id),
                limit: 1000,
                ..Default::default()
            })
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!(%worker_id, error = %e, "recovery: failed to list this worker's dispatched tasks");
                return;
            }
        };

        for task in dispatched {
            let Some(session_id) = task.acp_session_id.as_deref() else {
                continue; // 지목할 세션이 없으면 부재가 아니라 무지다.
            };
            if live.iter().any(|s| s == session_id) {
                continue;
            }
            let failure = vanished_session_failure(worker_id, session_id);
            if self
                .mark_failed(
                    task.id,
                    &[TaskPhase::Dispatched],
                    failure,
                    TransitionOrigin::ControlDecision,
                )
                .await
            {
                warn!(
                    task_id = %task.id, %worker_id, %session_id,
                    "recovery: this worker no longer holds the session for a task we believed was \
                     running — marked failed before dispatching new work here"
                );
            }
        }
    }

    /// dispatch 실패 시 최대 재시도 횟수를 설정한다 (로드맵 #38). `n == 0`
    /// (기본값)이면 `submit()`은 이 필드 도입 이전과 동일하게 워커 선택
    /// 실패나 CircuitOpen에서도 즉시 작업을 `Failed`로 마킹한다.
    pub fn with_max_dispatch_retries(mut self, n: u32) -> Self {
        self.max_dispatch_retries = n;
        self
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
        // control plane lease 확인 (로드맵 #63 불변식 2). `Completed`/`Failed`만
        // 대상이다 — 이 둘만 breaker 상태와 task의 최종 상태를 바꾸는 "제어"
        // 결정이다. `Output`(stdout/stderr 버퍼링)은 권위 있는 결정이 아니라
        // 순수 append-only 관측 데이터 전달이므로 lease 여부와 무관하게
        // 계속 흘려보낸다 — 막으면 사용자가 보고 있는 실시간 출력만 끊기고
        // 얻는 안전 이득은 없다. `ToolCall`(로드맵 #70 게이트 ④ 선행)도 같은
        // 부류이며, 여기서 막으면 **오히려 손해가 크다** — 리스를 잃은 창은
        // 새 소유자가 재조정해야 하는 구간이고, 그 재조정이 물을 "이 Task가
        // 무엇을 했는가"의 증거가 정확히 이 이벤트다.
        if matches!(
            event,
            WorkerEvent::Completed { .. } | WorkerEvent::Failed { .. }
        ) && !self.state.lease_allows_control()
        {
            // `event`를 통째로 로깅하지 않는다 — `Completed.result.output`은
            // 워커 실행 결과 원문이라 로그에 남기면 안 되는 데이터다.
            let task_id = match &event {
                WorkerEvent::Completed { task_id, .. } | WorkerEvent::Failed { task_id, .. } => {
                    *task_id
                }
                WorkerEvent::Output { .. }
                | WorkerEvent::ToolCall { .. }
                | WorkerEvent::SessionOpened { .. } => {
                    unreachable!("filtered by the matches! guard above")
                }
            };
            warn!(
                %task_id,
                "worker event dropped — this instance does not hold the control plane lease \
                 (breaker/task state left untouched; the new lease owner must reconcile it)"
            );
            // 로드맵 `#70` 게이트 6 선행. 그 Task의 최종 상태를 **아무도
            // 확정하지 않은 채** 창이 닫혔다는 뜻이라, 새 리스 소유자의
            // 재조정이 반드시 다시 봐야 하는 항목이다.
            self.state
                .audit_control(
                    fleet_core::audit::action::CONTROL_OUTCOME_ABANDONED,
                    ("task", task_id.to_string()),
                    serde_json::json!({ "lease_status": self.state.lease_status_label() }),
                )
                .await;
            return;
        }

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

                // 워커 이벤트는 dispatch된 작업에만 도착하므로 기대 위상은
                // `Dispatched` 하나뿐이다. reconciler가 이 작업을 이미
                // `Failed`로 확정한 뒤 워커가 뒤늦게 재연결해 완료를 보고하는
                // 경합(reconcile.rs의 orphan/offline 스윕)이 여기서 거절된다.
                let fence = self.state.control_fence();
                let outcome = self
                    .state
                    .store
                    .compare_and_set_task_status(
                        task_id,
                        &[TaskPhase::Dispatched],
                        &TaskStatus::Completed(result.clone()),
                        fence.as_ref(),
                        TransitionOrigin::WorkerOutcome,
                    )
                    .await;

                match outcome {
                    Ok(TransitionOutcome::Applied) => {
                        let _ = self
                            .state
                            .store
                            .append_event(&FleetEvent::task_completed(task_id, worker_id, result))
                            .await;
                        info!(%task_id, %worker_id, "task completed");
                    }
                    Ok(TransitionOutcome::Rejected { current }) => {
                        // 이벤트를 발행하지 않는다 — 상태가 바뀌지 않았는데
                        // `task_completed`를 남기면 이벤트 로그가 일어나지 않은
                        // 일을 주장하게 된다.
                        warn!(
                            %task_id, %worker_id, current = current.as_str(),
                            "late completion ignored — task already left the dispatched phase"
                        );
                    }
                    Ok(TransitionOutcome::Fenced) => {
                        // `Rejected`와 나눠 로그를 남긴다. 이건 이 작업 하나의
                        // 경합이 아니라 **이 인스턴스가 제어 기관이 아니라는**
                        // 신호이므로, 같은 이유로 뒤따르는 모든 쓰기도 거절된다.
                        warn!(
                            %task_id, %worker_id,
                            "completion dropped — this instance no longer holds the control \
                             plane lease epoch"
                        );
                    }
                    Ok(TransitionOutcome::StaleDispatchEpoch { dispatched_under }) => {
                        // 이 인스턴스는 제어 기관이 맞지만, 이 완료는 **다른
                        // 세대가 디스패치한** 작업의 것이다 (로드맵 #67 1단계).
                        //
                        // 도달 경로: epoch N으로 디스패치 → 리스를 잃음(N+1이
                        // 이 작업을 재디스패치할 수 있음) → 다시 획득(N+2).
                        // 그 사이에 떠 있던 N의 dispatch가 지금 완료를 보고한
                        // 것이고, 이 결과를 적으면 N+1이 만든 진행을 덮어쓴다.
                        //
                        // 위 두 arm과 달리 **이 작업 하나만** 포기한다. 리스는
                        // 정상 보유 중이므로 뒤따르는 쓰기는 성공한다.
                        warn!(
                            %task_id, %worker_id, dispatched_under,
                            current_epoch = ?fence.as_ref().map(|f| f.epoch),
                            "completion dropped — dispatched under a different control plane \
                             epoch; another epoch may have redispatched this task"
                        );
                    }
                    Err(e) => {
                        warn!(%task_id, %worker_id, error = %e, "failed to record task completion");
                    }
                }

                // `dec_running()`은 전이 결과와 무관하게 실행한다. 이 게이지는
                // 스토어 상태가 아니라 **디스패처 자신의 `inc_running()`**과
                // 짝지어져 있고, reconciler는 게이지를 전혀 건드리지 않는다.
                // `Applied`일 때만 감소시키면 늦은 완료가 거절될 때마다
                // 증가분이 영구히 남는다.
                dec_running();
                self.dispatch_ready_tasks().await;
            }
            WorkerEvent::Failed {
                task_id,
                error,
                observation,
            } => {
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

                // 관측의 범위를 그대로 옮긴다. 예전에는 여섯 생성 지점 전부가
                // `WorkerError`로 확정됐는데, 그 이름의 doc은 "워커에서 **실행 중**
                // 발생한 에러"라고 주장한다 — 연결 상실과 prompt 타임아웃에서는
                // 거짓이고, 운영자가 있지도 않은 워커 실행 실패 로그를 뒤지게
                // 만든다(`control-plane-authority-and-failover.md`의 인접 결함 1).
                //
                // breaker에는 셋 다 `Outcome::Failure`로 남긴다. 관측을 잃은 것도
                // 그 워커의 건강도에 대한 진짜 신호이기 때문이다 — 분류가 갈리는
                // 것은 **작업의 결말**이지 워커의 상태가 아니다.
                let kind = match observation {
                    FailureObservation::Reported => FailureKind::WorkerError,
                    FailureObservation::NotDelivered => FailureKind::WorkerUnavailable,
                    FailureObservation::ResultLost => FailureKind::ResultLost,
                };
                let failure = TaskFailure {
                    error,
                    kind,
                    worker_id,
                    attempts: 1,
                };
                // Completed 경로와 같은 이유로 `Dispatched`만 기대한다.
                //
                // `mark_failed`의 유일한 `WorkerOutcome` 호출 지점이다. 나머지는
                // 전부 현재 보유자가 지금 내리는 결정이고, 그쪽에 dispatch 세대
                // 술어를 걸면 낡은 세대가 디스패치한 고아를 회수할 수 없게 된다.
                self.mark_failed(
                    task_id,
                    &[TaskPhase::Dispatched],
                    failure,
                    TransitionOrigin::WorkerOutcome,
                )
                .await;

                // 전이 결과와 무관하게 감소 — 위 Completed 핸들러의 주석 참조.
                dec_running();
            }
            WorkerEvent::SessionOpened {
                task_id,
                session_id,
            } => {
                // **fence를 건다.** 세션을 연 것은 dispatch한 인스턴스이고,
                // 그 인스턴스가 이미 제어권을 잃었다면 이 기록도 그
                // 인스턴스의 것이 아니다. `Output`/`ToolCall`을 lease와 무관하게
                // 흘려보내는 것과 갈리는 자리인데, 그 둘은 관측이고 이것은
                // **신원**이기 때문이다 — 관측은 틀려도 나중에 버리면 되지만
                // 신원이 틀리면 재시작 뒤의 cleanup이 남의 세션을 지목한다.
                let fence = self.state.control_fence();
                match self
                    .state
                    .store
                    .record_task_acp_session(task_id, &session_id, fence.as_ref())
                    .await
                {
                    Ok(true) => {
                        tracing::debug!(%task_id, "acp session recorded");
                    }
                    // 셋 중 어느 이유든(이미 값이 있음 · fenced · 없는 id)
                    // 재시도가 아니라 기록이 옳다. 특히 "이미 값이 있음"은
                    // 한 Task에 실행이 둘이라는 뜻이라 흡수 판정(`#97`)의
                    // 불변식 위반이고, 조용히 지나가면 안 된다.
                    Ok(false) => {
                        tracing::warn!(
                            %task_id,
                            "acp session not recorded — already set, fenced, or unknown task"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(%task_id, error = %e, "failed to record acp session");
                    }
                }
            }
            WorkerEvent::Output {
                task_id,
                seq,
                chunk,
            } => {
                let _ = self.state.store.append_output(task_id, &chunk).await;
                tracing::debug!(%task_id, seq, "output chunk buffered");
            }
            WorkerEvent::ToolCall {
                task_id,
                worker_id,
                invocation,
            } => {
                // **축을 Task 상태에서 되찾지 않는다** (로드맵 `#70` 게이트 4).
                //
                // 예전에는 `current_worker_of(task_id)`로 찾았고, 그 함수는
                // `TaskStatus::Dispatched`일 때만 `Some`을 준다. `Failed`가
                // `worker_id`를 들고 있어도 보지 않았다. 결과는 조용한 증거
                // 인멸이었다 — **Task가 terminal로
                // 확정된 순간부터 도착하는 도구 호출이 전부 버려졌다.** 하필
                // 그 구간이 게이트 4가 존재하는 이유다: `Failed(ResultLost)`의
                // 정의가 "워커에서 아직 돌고 있을 수도 있다"인데, 정말로 돌고
                // 있다는 증거가 정확히 그때부터 기록되지 않았다.
                //
                // 이제 축은 그 관측을 배달한 연결 자신이 싣는다. "틀린 축은 없는
                // 축보다 나쁘다"는 기존 판단은 유효하고, 이 값은 가변적인 Task
                // 상태에서 추론한 것이 아니라 가장 권위 있는 출처의 것이다.
                //
                // **"terminal 뒤에 왔는가"를 여기서 판정하지 않는다.** 그러려면
                // 도구 호출마다 store를 한 번 읽어야 하고, 그것은 스트리밍
                // 경로에 왕복을 하나 심는 것이다. 이 이벤트와 Task의 terminal
                // 시각이 둘 다 남으므로 그 판정은 나중에 사후로 도출된다.
                // `invocation`을 로그 메시지에 통째로 넣지 않는다 — 필드 단위로
                // 넘겨야 관측성 정본의 금지 목록을 코드에서 눈으로 확인할 수
                // 있다. (그 넷은 전부 안전하지만, 형태를 지키는 것이 나중에
                // 필드가 늘 때의 방어다.)
                tracing::debug!(
                    %task_id, %worker_id,
                    tool_call_id = %invocation.tool_call_id,
                    kind = ?invocation.kind,
                    status = ?invocation.status,
                    "tool call observed"
                );
                // 대조가 store 조회를 하므로 **관측을 먼저 durable하게 남긴다.**
                // 조회가 실패하거나 느려도 "이 도구가 호출됐다"는 사실 자체는
                // 남아 있어야 한다 — 그것이 원장의 입력이고, 허용 목록 판정은
                // 그 위에 얹히는 부가 정보다.
                let tool_name_check = invocation.clone();
                let _ = self
                    .state
                    .store
                    .append_event(&FleetEvent::task_tool_call(task_id, worker_id, invocation))
                    .await;
                self.check_tool_against_allowlist(task_id, worker_id, &tool_name_check)
                    .await;
            }
        }
    }

    /// 관측된 도구 호출이 Agent 템플릿의 허용 목록 안에 있는지 보고, 밖이면
    /// 감사에 남긴다 (로드맵 `#64`·`#86` 선행).
    ///
    /// **탐지이지 예방이 아니다.** 도구는 이미 워커의 grok 프로세스 안에서
    /// 실행된 뒤이고 우리는 ACP 알림으로 사후에 볼 뿐이다. 그래도 남기는
    /// 이유는 `AgentTemplateBody.tools`가 그동안 저장·표시만 되고 **아무도
    /// 읽지 않는 필드**였기 때문이다 — 강제되지 않는 허용 목록은 없는 것보다
    /// 위험하다.
    ///
    /// **판정하지 않는 경우가 셋이고, 셋 다 "위반 아님"이 아니라 "판정 불가"다.**
    /// 그 구분이 이 함수에서 가장 틀리기 쉬운 자리다:
    ///
    /// 1. Task에 `agent_id`가 없다 — 어떤 템플릿을 적용할지 자체가 없다.
    /// 2. Agent에 `template_pin`이 없거나 목록이 **비어 있다** — 비어 있는
    ///    `tools`는 "아무 도구도 허용하지 않는다"가 아니라 **"선언하지 않았다"**이다.
    ///    `#[serde(default)]`로 기본이 빈 벡터라 기존 템플릿 대부분이 여기 해당하며,
    ///    이것을 deny-all로 읽으면 모든 호출이 위반으로 쏟아져 신호가 죽는다.
    ///    **미선언 자체가 공백이라는 사실은 별개로 다뤄야 하고, 호출마다 남길
    ///    일이 아니다.**
    /// 3. `invocation.name`이 `None` — SDK의 `unstable_tool_call_name`이 주지
    ///    않은 경우다. 이름을 모르면 목록과 대조할 수 없다. 모르는 것을 위반으로
    ///    접으면 거짓 양성이 되고, 반대로 통과로 세면 거짓 음성이 된다. 어느 쪽도
    ///    아니므로 아무것도 적지 않는다.
    async fn check_tool_against_allowlist(
        &self,
        task_id: TaskId,
        worker_id: WorkerId,
        invocation: &fleet_core::ToolInvocation,
    ) {
        // (3) 이름이 없으면 대조 자체가 불가능하다.
        let Some(name) = invocation.name.as_deref() else {
            return;
        };

        // (1) Agent에 묶이지 않은 Task.
        let Ok(Some(task)) = self.state.store.get_task(task_id).await else {
            return;
        };
        let Some(agent_id) = task.agent_id else {
            return;
        };
        let Ok(Some(agent)) = self.state.store.get_agent(agent_id).await else {
            return;
        };
        // (2) pin이 없으면 적용할 revision이 없다.
        let Some(pin) = agent.template_pin else {
            return;
        };

        let allowed = {
            let mut cache = self.tool_allowlist_cache.lock().await;
            match cache.get(&pin.revision_id) {
                Some(hit) => hit.clone(),
                None => {
                    let Ok(Some(revision)) = self
                        .state
                        .store
                        .get_agent_template_revision(pin.revision_id)
                        .await
                    else {
                        return;
                    };
                    let set: Arc<HashSet<String>> =
                        Arc::new(revision.body.tools.iter().cloned().collect());
                    cache.insert(pin.revision_id, set.clone());
                    set
                }
            }
        };

        // (2) 빈 목록은 미선언이다 — deny-all이 아니다.
        if allowed.is_empty() || allowed.contains(name) {
            return;
        }

        tracing::warn!(
            %task_id, %worker_id, %agent_id, tool = %name,
            "agent called a tool outside its template allow-list"
        );
        // `audit_control`이 아니라 `audit_decision`이다 — 이 사실은 리스 경합과
        // 무관하고 단일 인스턴스 배포에서도 참이다.
        self.state
            .audit_decision(
                fleet_core::audit::action::AGENT_TOOL_OUTSIDE_ALLOWLIST,
                ("task", task_id.to_string()),
                serde_json::json!({
                    "agent_id": agent_id.to_string(),
                    "worker_id": worker_id.to_string(),
                    "tool": name,
                    "template_revision_id": pin.revision_id.to_string(),
                }),
            )
            .await;
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
    pub async fn submit(&self, mut task: Task) -> Result<TaskId, DispatchError> {
        let task_id = task.id;

        // 0-a. 입국 심사 — 저장 **전에** 판정한다 (로드맵 #69).
        //
        // 거절이 `insert_task_idempotent` 앞에 있어야 Task 행 자체가 생기지
        // 않는다. 뒤에 두면 영원히 dispatch될 수 없는 행이 Pending으로 남아
        // 재조정 루프가 매 tick마다 같은 실패를 반복한다.
        //
        // 호출자가 `inherit_from_parent`로 부모의 `cwd`를 물려준 뒤라는 점이
        // 중요하다 — 상속된 값도 명시 입력과 똑같이 검증 대상이며(`project_id`가
        // 이미 같은 규칙을 따른다), 그래서 이 게이트는 상속 **뒤**에 있는
        // 이 지점에 있어야 한다.
        //
        // 이것이 유일한 관문은 아니다. `fleet tasks submit`은 `submit()`을
        // 지나지 않고 store에 직접 쓰고, 이 검증 이전에 저장된 행도 남아 있다.
        // 최종 관문은 `AcpTransport::dispatch()`다.
        fleet_core::validate_workspace_cwd(task.cwd.as_deref())
            .map_err(|e| DispatchError::InvalidRequest(format!("cwd: {e}")))?;

        // 0-b. 지능형 라우터를 통해 프로파일/모델/예산 결정
        let router = crate::router::HeuristicTaskRouter::new();
        let decision = router.resolve_routing(&task);
        if task.routing_profile.is_none() {
            task.routing_profile = Some(decision.profile.as_str().to_string());
        }
        if task.resolved_model.is_none() {
            task.resolved_model = Some(decision.resolved_model);
        }
        if task.token_budget.is_none() {
            task.token_budget = Some(decision.token_budget);
        }

        // 1. Store에 작업 저장 — 클라이언트 멱등성 키를 존중한다 (로드맵 #62 2단계).
        //
        // `Duplicate`면 **여기서 즉시 반환한다.** 아래 어느 것도 실행하면 안
        // 된다: TaskCreated 이벤트(일어나지 않은 일을 주장하게 된다), 의존성
        // 검사, 워커 선택, dispatch, RUNNING 게이지 증가. 불리언을 만들어
        // 나머지 흐름에 실어 보내면 그중 하나가 반드시 빠진다.
        let outcome = self
            .state
            .store
            .insert_task_idempotent(&task)
            .await
            .map_err(|e| DispatchError::Store(e.to_string()))?;
        match outcome {
            IdempotentInsert::Inserted => {}
            IdempotentInsert::Duplicate(existing) => {
                tracing::info!(
                    task_id = %existing.id,
                    key = %task.idempotency_key.as_deref().unwrap_or("?"),
                    "duplicate submit — returning the existing task without dispatching"
                );
                // 반환되는 Task는 이미 Completed/Failed일 수 있다. "timeout 후
                // 같은 키로 재호출"이 정확히 이 게이트가 막으려는 시나리오이고,
                // 그때 최초 제출은 대개 이미 끝나 있다.
                return Ok(existing.id);
            }
            IdempotentInsert::Conflict { existing_task_id } => {
                return Err(DispatchError::IdempotencyConflict {
                    key: task.idempotency_key.clone().unwrap_or_default(),
                    existing_task_id,
                });
            }
        }

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
        // 제출 시도이므로 기본(재시도 비활성, `max_dispatch_retries == 0`)
        // 상태에서는 선택/회로 실패도 기존과 동일하게 즉시 `Failed`로
        // 마킹한다 (`mark_unavailable_as_failed = true`).
        //
        // 로드맵 #38: `max_dispatch_retries > 0`이면 `WorkerUnavailable`/
        // `CircuitOpen`을 일시적 실패로 간주해 즉시 확정하지 않는다 —
        // `dispatch_existing`에 `false`를 전달해 작업을 `Pending`인 채로 두고
        // (재조정 루프가 재사용하는 것과 동일한 경로), `retry_count`만 1
        // 올린 뒤 `Ok(task_id)`를 반환한다. 실제 재시도는
        // [`Reconciler`](crate::reconcile::Reconciler)의 stale-Pending
        // 스윕이 백그라운드에서 수행하고, 소진되면 dead-letter(`Failed`)로
        // 전이시킨다. transport dispatch 자체의 실패(연결 오류 등)는 이
        // 플래그와 무관하게 `dispatch_existing` 내부에서 항상 즉시 `Failed`로
        // 마킹되므로 아래에서 별도 처리가 필요 없다.
        // DAG 체이닝: 미완료 선행 작업이 하나라도 있다면 dispatch를 수행하지 않고 Pending으로 대기
        let mut has_unresolved_dependencies = false;
        for dep_id in &task.dependency_ids {
            if let Ok(Some(dep_task)) = self.state.store.get_task(*dep_id).await {
                if !matches!(dep_task.status, TaskStatus::Completed(_)) {
                    has_unresolved_dependencies = true;
                    break;
                }
            } else {
                has_unresolved_dependencies = true;
                break;
            }
        }

        if has_unresolved_dependencies {
            info!(%task_id, "Task has unresolved dependencies, leaving Pending");
            return Ok(task_id);
        }

        let retries_enabled = self.max_dispatch_retries > 0;
        match self.dispatch_existing(task, !retries_enabled).await {
            Ok(()) => {}
            Err(DispatchError::NoWorker(_) | DispatchError::CircuitOpen(_)) if retries_enabled => {
                // 첫 시도가 일시적으로 실패 — Pending인 채로 두고 재시도
                // 횟수만 기록한다. 카운트 증가 자체가 실패해도(드문 경합)
                // 제출은 성공한 것으로 취급한다 — 다음 Reconciler tick이
                // 재시도한다.
                let _ = self.state.store.increment_task_retry_count(task_id).await;
            }
            Err(e) => return Err(e),
        }

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

        // 0. control plane lease 확인 (로드맵 #63 불변식 2). 워커 선택·breaker
        // 변경·transport dispatch 전부 여기서 막는다 — task 상태는 건드리지
        // 않고 `Pending`인 채로 둔다(재시도 대상). `mark_unavailable_as_failed`
        // 여부와 무관하게 항상 거절한다 — lease 없이 "즉시 실패로 확정"하는
        // 것조차 이 인스턴스가 하면 안 되는 상태 변경이다.
        if !self.state.lease_allows_control() {
            warn!(%task_id, "dispatch refused — this instance does not hold the control plane lease");
            // 로드맵 `#70` 게이트 6 선행. `lease_status`를 함께 남기는 이유는
            // 여기서 `control_epoch`가 반드시 `None`이기 때문이다 — 세대가
            // 없다는 것이 이 기록의 내용이고, 그것이 `Fenced`(다른 owner가
            // 있다)인지 `Stopped`(종료 중이다)인지는 조사의 방향을 가른다.
            self.state
                .audit_control(
                    fleet_core::audit::action::CONTROL_DISPATCH_REFUSED,
                    ("task", task_id.to_string()),
                    serde_json::json!({ "lease_status": self.state.lease_status_label() }),
                )
                .await;
            return Err(DispatchError::ControlPlaneFenced);
        }

        // 3. 워커 선택
        let worker_id = match self.state.selector.select(&task).await {
            Ok(id) => id,
            Err(e) => {
                if mark_unavailable_as_failed {
                    // 선택 실패 → 작업을 Failed로 표시. credential 부재로 인한
                    // 후보 소진(로드맵 #71)은 일반 워커 가용성 문제와 원인이
                    // 다르므로 별도 FailureKind로 구분한다 — 재시도로 해소되지
                    // 않고 credential 프로비저닝이 필요함을 명확히 하기 위함.
                    let kind = match &e {
                        SelectionError::NoWorkerForCredential(_) => FailureKind::CredentialMissing,
                        _ => FailureKind::WorkerUnavailable,
                    };
                    let failure = TaskFailure {
                        error: e.to_string(),
                        kind,
                        worker_id: None,
                        attempts: 0,
                    };
                    // 워커를 배정하지 못했으므로 작업은 아직 Pending이다.
                    self.mark_failed(
                        task_id,
                        &[TaskPhase::Pending],
                        failure,
                        TransitionOrigin::ControlDecision,
                    )
                    .await;
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
                // 브레이커가 열려 dispatch를 시도조차 못 했으므로 Pending이다.
                self.mark_failed(
                    task_id,
                    &[TaskPhase::Pending],
                    failure,
                    TransitionOrigin::ControlDecision,
                )
                .await;
            }
            return Err(DispatchError::CircuitOpen(worker_id));
        }

        // 4.5. inventory-first recovery (로드맵 `#70` 게이트 2).
        //
        // **이 워커에게 이 제어 세대의 첫 일을 주기 직전이다.** 게이트 문언은
        // 복구가 신규 dispatch보다 "먼저"일 것을 요구하는데, 그 순서를 배경
        // 루프에 맡기면 순서가 아니라 경합이 된다 — `Reconciler`의 첫 tick은
        // 30초 뒤이고 그 사이의 dispatch는 아무것도 묻지 않은 채 나간다.
        // 전제조건으로 만들면 구조적으로 어길 수 없다.
        //
        // **브레이커 뒤인 이유**는 못 닿는 워커가 회로를 열 수 있어야 하기
        // 때문이다. 앞에 두면 이 경로가 transport 실패보다 먼저 반환해 브레이커
        // 기록을 건너뛰고, 그러면 selector가 그 워커를 영영 계속 고른다. 회로가
        // 열린 워커는 어차피 여기 오지 않으므로 복구할 필요도 없다 — 그 워커의
        // 어긋난 Task는 `Reconciler`의 주기 스윕이 본다.
        //
        // **CAS 앞인 이유**는 반대다. 이 Task를 `Dispatched`로 차지한 뒤에
        // 복구가 실패하면 상태만 옮겨 두고 보내지 못한 실행이 생긴다.
        if let Err(e) = self.ensure_worker_recovered(worker_id).await {
            // 못 닿은 것은 **워커 결함**이다. transport dispatch 실패와 같은
            // 자리에 기록해야 회로가 열리고, 그래야 selector가 이 워커를
            // 그만 고른다.
            let old_state = cb.state();
            cb.record(Outcome::Failure);
            self.persist_breaker_transition(worker_id, old_state, cb.state())
                .await;

            if mark_unavailable_as_failed {
                let failure = TaskFailure {
                    error: e.to_string(),
                    kind: FailureKind::WorkerUnavailable,
                    worker_id: Some(worker_id),
                    attempts: 0,
                };
                // 아직 `Pending`이다 — 워커를 배정하지 못한 것과 같은 자리다.
                self.mark_failed(
                    task_id,
                    &[TaskPhase::Pending],
                    failure,
                    TransitionOrigin::ControlDecision,
                )
                .await;
            }
            return Err(e);
        }

        // 4.7. 하네스 조립 — 프롬프트 + 필수 Skill (로드맵 `#65`).
        //
        // **CAS 앞이다.** 정본([하네스 구성](../../docs/architecture/agents/
        // harness-composition.md))은 "필수 Skill은 시작 전에 고정하고 누락 시
        // 실행을 거절한다"고 적는데, 예전에는 조립이 CAS **뒤**에 있었고
        // 누락은 `warn!` 한 줄이었다. 그래서 선언된 Skill 없이 실행이 나갔고,
        // 거절하려 해도 Task는 이미 `Dispatched`였다.
        //
        // 조립을 한 번만 하는 것도 의도다. "먼저 존재를 확인하고 나중에
        // 읽는다"로 나누면 그 사이에 파일이 사라질 수 있고, 그러면 확인을
        // 통과한 Task가 스킬 없이 나간다 — 확인이 무의미해지는 창이다.
        let base_prompt = if task.parent_task_id.is_some() {
            self.build_threaded_prompt(&task).await
        } else {
            task.prompt.clone()
        };
        let harness = inject_skills(&base_prompt, &task.skills_required);
        if !harness.is_complete() {
            warn!(
                %task_id, missing = ?harness.missing,
                "dispatch refused — a required skill could not be loaded"
            );
            if mark_unavailable_as_failed {
                let failure = TaskFailure {
                    error: format!(
                        "required skill(s) not available on the orchestrator: {}",
                        harness.missing.join(", ")
                    ),
                    kind: FailureKind::SkillMissing,
                    worker_id: Some(worker_id),
                    attempts: 0,
                };
                // 아직 `Pending`이다 — 워커에 보내지 않았다.
                self.mark_failed(
                    task_id,
                    &[TaskPhase::Pending],
                    failure,
                    TransitionOrigin::ControlDecision,
                )
                .await;
            }
            // **브레이커에 기록하지 않는다.** 워커는 이 요청을 본 적이 없고,
            // 스킬이 없는 것은 오케스트레이터 쪽 사실이다. 여기서 기록하면
            // 스킬 하나가 빠진 것만으로 멀쩡한 워커의 회로가 열린다 —
            // `#69`가 `InvalidRequest`를 브레이커에서 면제한 것과 같은 이유다.
            return Err(DispatchError::SkillMissing {
                task_id,
                missing: harness.missing,
            });
        }

        // 5. Dispatched 상태로 전이
        task.status = TaskStatus::Dispatched {
            worker_id,
            started_at: Utc::now(),
        };
        // 같은 작업을 두 인스턴스가 동시에 집어가는 경합, 그리고 dispatch 도중
        // 도착한 취소를 여기서 거른다. 기대 위상은 `Pending` 하나뿐이다 —
        // 이미 `Dispatched`인 작업을 다시 dispatch하면 워커 두 곳에서 같은
        // 작업이 실행된다.
        match self
            .state
            .store
            .compare_and_set_task_status(
                task_id,
                &[TaskPhase::Pending],
                &task.status,
                self.state.control_fence().as_ref(),
                TransitionOrigin::ControlDecision,
            )
            .await
            .map_err(|e| DispatchError::Store(e.to_string()))?
        {
            TransitionOutcome::Applied => {}
            TransitionOutcome::Fenced => {
                // dispatch **전에** 반환하는 이유는 `Rejected`와 같다 — 상태를
                // 차지하지 못한 채 transport로 보내면 아무도 소유하지 않는
                // 실행이 생긴다. 다만 원인이 다르므로 에러도 나눈다: 이건 이
                // 인스턴스가 제어 기관이 아니라는 뜻이고, `lease_allows_control`
                // 검사를 통과한 **뒤에** fenced됐을 때만 도달한다.
                //
                // 로드맵 `#70` 게이트 6 선행 — **이것이 "둘 이상의 owner"의
                // 직접 증거다.** 위 거절과 달리 이 인스턴스는 리스가 있다고
                // 믿고 쓰러 갔고, 저장소가 세대 불일치로 막았다. 그래서 이
                // 기록에는 그 순간 들고 있던 epoch가 실린다.
                self.state
                    .audit_control(
                        fleet_core::audit::action::CONTROL_WRITE_FENCED,
                        ("task", task_id.to_string()),
                        serde_json::json!({ "worker_id": worker_id.to_string() }),
                    )
                    .await;
                return Err(DispatchError::ControlPlaneFenced);
            }
            TransitionOutcome::StaleDispatchEpoch { dispatched_under } => {
                // 저장소는 `ControlDecision`에 dispatch 세대 술어를 걸지 않으므로
                // 이 값을 만들 수 없다. `unreachable!()` 대신 에러로 돌려주는
                // 이유는 이 경로가 스케줄러 루프 안이기 때문이다 — 가정이
                // 깨졌을 때 프로세스를 죽이는 것보다 이 작업 하나를 포기하고
                // 근거를 남기는 편이 낫다.
                return Err(DispatchError::Store(format!(
                    "dispatch CAS reported a stale dispatch epoch ({dispatched_under}) for a \
                     ControlDecision transition — this indicates a bug in the store"
                )));
            }
            TransitionOutcome::Rejected { current } => {
                // transport로 보내기 **전에** 반환한다. 상태를 차지하지 못한
                // 채로 dispatch하면 아무도 소유하지 않는 실행이 생긴다.
                warn!(
                    %task_id, %worker_id, current = current.as_str(),
                    "dispatch aborted — task is no longer pending"
                );
                return Err(DispatchError::NotPending {
                    task_id,
                    current: current.as_str(),
                });
            }
        }

        let _ = self
            .state
            .store
            .append_event(&FleetEvent::task_dispatched(task_id, worker_id))
            .await;

        // 6. Transport로 dispatch. 프롬프트는 위 4.7에서 이미 조립됐다.
        let prompt = harness.prompt;
        let req = DispatchRequest {
            task_id,
            worker_id,
            prompt,
            cwd: task.cwd.clone(),
            model: task.model.clone(),
            max_turns: task.max_turns,
            timeout_secs: task.timeout_secs,
            checkpoint_branch: task.checkpoint_branch.clone(),
            skills_required: task.skills_required.clone(),
        };

        inc_running();

        if let Err(e) = self.state.transport.dispatch(req).await {
            // 요청 결함인가, 워커 결함인가 (로드맵 #69).
            //
            // 회로 차단기는 **워커의 건강도**를 재는 장치다. 요청이 규칙을
            // 어겨 워커에 보내지지도 않은 실패를 여기에 기록하면, 클라이언트가
            // 잘못된 `cwd`를 반복 제출하는 것만으로 멀쩡한 워커의 회로를 열 수
            // 있다 — 검증을 추가하면서 새 DoS 경로를 만드는 셈이다. 그래서
            // 이 갈래만 차단기와 상태 전이 기록 전체를 건너뛴다.
            let invalid_request = matches!(e, TransportError::InvalidRequest(_));

            // dispatch 자체 실패 (연결 등) — 재조정 여부와 무관하게 항상 실제
            // 실패로 취급한다.
            let old_state = cb.state();
            if !invalid_request {
                cb.record(Outcome::Failure);
            }
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
                kind: if invalid_request {
                    // `WorkerError`로 적으면 운영자가 워커 로그를 뒤진다 —
                    // 워커는 이 요청을 본 적이 없다.
                    FailureKind::InvalidRequest
                } else {
                    FailureKind::WorkerError
                },
                worker_id: Some(worker_id),
                attempts: 1,
            };
            // 5단계에서 이미 Dispatched로 옮긴 뒤 transport가 실패한 경로다.
            self.mark_failed(
                task_id,
                &[TaskPhase::Dispatched],
                failure,
                TransitionOrigin::ControlDecision,
            )
            .await;
            // 위 `inc_running()`과 짝을 이루므로 전이 결과와 무관하게 감소.
            dec_running();
            // `From<TransportError>`가 `InvalidRequest`를 보존하므로, 여기서
            // `e.to_string()`으로 접지 않고 그대로 변환한다.
            return Err(DispatchError::from(e));
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

    /// 작업을 실패로 마킹하고 이벤트 발행. 전이가 실제로 적용됐으면 `true`.
    ///
    /// `pub(crate)` — `Reconciler`가 orphaned `Dispatched` 작업(담당 워커가
    /// 재등록으로 사라진 경우)을 Failed로 전이시킬 때도 재사용한다.
    ///
    /// `expected`를 **호출자가 넘기는 이유**: 이 함수는 두 종류의 선행 상태에
    /// 걸쳐 공유된다. 워커를 배정하지 못했거나 브레이커가 열려 dispatch가
    /// 끝난 경로와 reconciler의 dead-letter는 작업이 아직 `Pending`이고,
    /// transport dispatch 실패와 reconciler의 orphan/offline 스윕은 이미
    /// `Dispatched`다. 여기서 `[Pending, Dispatched]`를 하드코딩하면
    /// (즉 [`TaskPhase::allowed_predecessors`]의 기본값을 쓰면) orphan 스윕이
    /// 방금 `Pending` → `Dispatched`로 넘어간 작업을 죽일 수 있다 — 닫으려던
    /// 경합을 그대로 남기는 셈이다.
    ///
    /// 반환값은 호출자가 카운터를 정확히 세기 위한 것이다. 거절됐거나 스토어
    /// 접근이 실패했으면 `false` — 두 경우 모두 "이 호출은 아무것도 쓰지
    /// 않았다"가 참이므로 집계에서 동일하게 취급하는 것이 옳다. 구분은 로그에
    /// 남긴다.
    pub(crate) async fn mark_failed(
        &self,
        task_id: TaskId,
        expected: &[TaskPhase],
        failure: TaskFailure,
        origin: TransitionOrigin,
    ) -> bool {
        let fence = self.state.control_fence();
        let outcome = self
            .state
            .store
            .compare_and_set_task_status(
                task_id,
                expected,
                &TaskStatus::Failed(failure.clone()),
                fence.as_ref(),
                origin,
            )
            .await;

        match outcome {
            Ok(TransitionOutcome::Applied) => {
                let _ = self
                    .state
                    .store
                    .append_event(&FleetEvent::task_failed(task_id, failure))
                    .await;
                warn!(%task_id, "task failed");
                true
            }
            Ok(TransitionOutcome::Rejected { current }) => {
                warn!(
                    %task_id, current = current.as_str(),
                    "failure not recorded — task already left the expected phase"
                );
                false
            }
            Ok(TransitionOutcome::Fenced) => {
                warn!(
                    %task_id,
                    "failure not recorded — this instance no longer holds the control plane \
                     lease epoch"
                );
                false
            }
            Ok(TransitionOutcome::StaleDispatchEpoch { dispatched_under }) => {
                // `WorkerOutcome`으로 호출된 경우에만 도달한다 — 즉 워커가 보고한
                // 실패이고, 그 dispatch는 다른 세대의 것이다. Completed 쪽과 같은
                // 이유로 이 보고 하나만 버린다.
                //
                // `ControlDecision` 호출자(reconciler의 스윕, dispatch 실패 확정)는
                // 저장소가 술어를 걸지 않으므로 여기 오지 않는다. 온다면 저장소
                // 버그이지만, 동작은 같아도 무해하다 — 어느 쪽이든 아무것도
                // 쓰이지 않았고 `false`가 그 사실을 정확히 보고한다.
                warn!(
                    %task_id, dispatched_under,
                    "failure not recorded — reported for a dispatch made under a different \
                     control plane epoch"
                );
                false
            }
            Err(e) => {
                warn!(%task_id, error = %e, "failed to record task failure");
                false
            }
        }
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
        // control plane lease 확인 (로드맵 #63 불변식 2 — "cancel"이 명시
        // 대상이다). 존재하지 않는 task_id로 취소를 시도한 경우와 구분하기
        // 위해 조회보다 먼저 거절한다 — lease 없는 인스턴스는 어차피 어떤
        // task도 취소할 권한이 없으므로, 조회 결과를 보여주는 것 자체가
        // 의미 없다(불변식 1의 "조회는 허용" 예외는 순수 read-only 경로에
        // 대한 것이지, mutation의 전제 조사에는 적용하지 않는다).
        if !self.state.lease_allows_control() {
            warn!(%task_id, "cancel refused — this instance does not hold the control plane lease");
            return Err(CancelError::ControlPlaneFenced);
        }

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
            // **저장소가 기억하는 세션을 함께 싣는다** (로드맵 `#70` 게이트 7).
            // 전송 계층의 인메모리 맵은 프로세스와 함께 비워지므로, 이 단서가
            // 없으면 오케스트레이터가 한 번이라도 재시작한 뒤의 모든 취소가
            // 대상 없이 "성공"한다.
            let req = fleet_transport::CancelRequest {
                task_id,
                known_session: task.acp_session_id.clone(),
                worker_id: Some(wid),
            };
            // transport.cancel은 best-effort — 워커가 이미 끝났을 수 있음.
            // 에러가 나도 상태 전이는 진행.
            match self.state.transport.cancel(req).await {
                Ok(fleet_transport::CancelDelivery::Sent) => {}
                // **전달되지 않았다.** 저쪽은 계속 돌고 있을 수 있는데 아래에서
                // `Cancelled`를 적는다. 그 어긋남을 로그로만 남기면 재시작
                // 뒤에는 아무 흔적도 없으므로 감사에 싣는다 — 이 경로가
                // 조용했던 것이 게이트 7이 존재하는 이유다.
                Ok(fleet_transport::CancelDelivery::Unreachable) => {
                    warn!(
                        %task_id, %wid,
                        "cancel was not delivered — the worker session may still be running"
                    );
                    // `audit_control`이 아니라 `audit_decision`이다 — 저쪽은
                    // 리스가 없으면 아무것도 남기지 않는데, 미전달 취소는
                    // 단일 인스턴스 배포에서 **더** 흔하다.
                    self.state
                        .audit_decision(
                            fleet_core::audit::action::CONTROL_CANCEL_UNDELIVERED,
                            ("task", task_id.to_string()),
                            serde_json::json!({
                                "worker_id": wid.to_string(),
                                "session_known": task.acp_session_id.is_some(),
                            }),
                        )
                        .await;
                }
                // 지목할 세션이 없다. `Dispatched`인데 이 값이 나오면 세션이
                // 열린 뒤 신원이 기록되기 전에 크래시한 Task다 — 드물지만
                // 정상 경로와 구분해서 남긴다.
                Ok(fleet_transport::CancelDelivery::NoSession) => {
                    tracing::debug!(%task_id, %wid, "cancel: no session known for a dispatched task");
                }
                Err(e) => {
                    warn!(%task_id, %wid, error = %e, "transport.cancel failed, proceeding with status update");
                }
            }
        }

        let cancelled = TaskStatus::Cancelled {
            reason,
            cancelled_at: Utc::now(),
        };

        // 위의 `get_task` → `is_terminal()` 검사와 이 쓰기 사이에는 창이 있다.
        // 그 사이에 워커가 완료를 보고하거나 reconciler가 실패로 확정하면
        // 조건 없는 쓰기는 확정된 종료 상태를 취소로 덮어썼다. 같은 조건을
        // `WHERE`에 다시 걸어 그 창을 닫는다 — 위의 검사는 이제 빠른 경로이자
        // 더 나은 에러 메시지를 위한 것이고, 정합성의 근거는 이 CAS다.
        match self
            .state
            .store
            .compare_and_set_task_status(
                task_id,
                &[TaskPhase::Pending, TaskPhase::Dispatched],
                &cancelled,
                self.state.control_fence().as_ref(),
                TransitionOrigin::ControlDecision,
            )
            .await
            .map_err(|e| CancelError::Store(e.to_string()))?
        {
            TransitionOutcome::Applied => {}
            TransitionOutcome::Fenced => {
                // 위의 `lease_allows_control` 검사를 통과한 뒤 fenced된 경우다.
                // 같은 에러를 돌려주지만 도달 경로가 다르다 — 그쪽은 관측
                // 시점의 bool이고 이쪽은 저장소가 쓰기를 실제로 거절한 결과다.
                //
                // 이 시점에는 이미 `transport.cancel()`을 보낸 뒤일 수 있다.
                // 그 요청을 되돌리지는 않는다 — cancel은 process 중단 요청이지
                // effect rollback이 아니고(정본 "취소·timeout·redrive"),
                // 되돌릴 대상을 이 저장소가 아직 기록하지 않는다.
                return Err(CancelError::ControlPlaneFenced);
            }
            TransitionOutcome::StaleDispatchEpoch { dispatched_under } => {
                // dispatch CAS 쪽과 같은 이유로 도달 불가이고, 같은 이유로
                // panic 대신 에러다. 취소는 operator가 부르는 경로이므로
                // 더더욱 프로세스를 죽여선 안 된다.
                return Err(CancelError::Store(format!(
                    "cancel CAS reported a stale dispatch epoch ({dispatched_under}) for a \
                     ControlDecision transition — this indicates a bug in the store"
                )));
            }
            TransitionOutcome::Rejected { current } => {
                // 검사 이후에 종료 상태에 도달한 것이므로, 처음부터 종료
                // 상태였을 때와 같은 에러를 돌려준다 — 호출자 입장에서 두
                // 경우는 구분할 수 없고, 구분할 이유도 없다.
                return Err(CancelError::AlreadyTerminal {
                    task_id,
                    phase: current.as_str(),
                });
            }
        }

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

    async fn dispatch_ready_tasks(&self) {
        use fleet_core::TaskFilter;
        use fleet_core::TaskStatusFilter;

        let filter = TaskFilter {
            status: Some(TaskStatusFilter::Pending),
            limit: 1000,
            ..Default::default()
        };

        if let Ok(pending_tasks) = self.state.store.list_tasks(&filter).await {
            for task in pending_tasks {
                let mut ready = true;
                for dep_id in &task.dependency_ids {
                    if let Ok(Some(dep_task)) = self.state.store.get_task(*dep_id).await {
                        if !matches!(dep_task.status, TaskStatus::Completed(_)) {
                            ready = false;
                            break;
                        }
                    } else {
                        ready = false;
                        break;
                    }
                }
                if ready {
                    let task_id = task.id;
                    info!(%task_id, "Dependency resolved, dispatching task from DAG chain");
                    let _ = self.dispatch_existing(task, false).await;
                }
            }
        }
    }
}

/// `TaskStatus`의 위상 라벨 (에러 메시지용).
/// 인벤토리에서 사라진 세션에 대한 실패 기록 (로드맵 `#70` 게이트 2).
///
/// `Reconciler`의 주기 스윕과 dispatch 전 복구가 **같은 판정을 두 자리에서**
/// 내리므로, 그 판정의 이름(`ExecutionVanished`)과 운영자가 읽을 문장은 한
/// 곳에만 둔다. 두 곳에 적으면 한쪽만 고쳐져 같은 사실이 두 문장으로 남는다.
pub(crate) fn vanished_session_failure(worker_id: WorkerId, session_id: &str) -> TaskFailure {
    TaskFailure {
        error: format!(
            "worker {worker_id} no longer holds ACP session {session_id} for this task — \
             the execution ended without reporting a result"
        ),
        kind: FailureKind::ExecutionVanished,
        worker_id: Some(worker_id),
        attempts: 0,
    }
}

fn phase_label(status: &TaskStatus) -> &'static str {
    // 같은 매핑을 손으로 두 번 적지 않는다 — CAS의 SQL 조건절이 이 문자열
    // 집합에 의존하므로, 사본이 늘어날수록 조용히 어긋날 자리가 늘어난다.
    status.phase().as_str()
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

    /// 이 제어 세대에서 그 워커에게 **아직 무엇을 들고 있는지 묻지 못했다**
    /// (로드맵 `#70` 게이트 2).
    ///
    /// 재시도는 안전하다 — 워커가 다시 닿으면 다음 시도에서 조회가 성공하고
    /// 그때 dispatch가 진행된다. [`CircuitOpen`](Self::CircuitOpen)과 다른
    /// 사실이다: 저쪽은 "이 워커가 최근에 실패를 쌓았다"이고 이쪽은 "이
    /// 워커에 대해 아직 아무것도 모른다"이다.
    #[error("worker {0} has not been recovered yet — its session inventory could not be read")]
    RecoveryUnavailable(WorkerId),

    /// Task가 필수로 선언한 Skill을 조립 시점에 로드하지 못했다
    /// (로드맵 `#65`).
    ///
    /// 재시도해도 같은 결과다 — 오케스트레이터에 그 Skill 파일을 배포해야
    /// 해소된다. [`NoWorker`](Self::NoWorker)와 달리 워커 가용성 문제가
    /// 아니므로 다른 워커로 옮겨도 소용이 없다.
    #[error("task {task_id} requires skill(s) that could not be loaded: {}", missing.join(", "))]
    SkillMissing {
        task_id: TaskId,
        missing: Vec<String>,
    },

    #[error("transport error: {0}")]
    Transport(String),

    /// dispatch 직전의 compare-and-set이 거절됐다 — 그 사이에 다른 writer가
    /// 작업을 `Pending`에서 옮겼다는 뜻이다(취소, 또는 다른 인스턴스의 dispatch).
    /// 재시도해서는 안 된다: 작업은 이미 누군가의 소유다.
    #[error("task {task_id} is no longer pending (now '{current}') — dispatch aborted")]
    NotPending {
        task_id: TaskId,
        current: &'static str,
    },

    /// 같은 멱등성 키가 **다른 페이로드**로 이미 쓰였다 (로드맵 #62 2단계).
    /// 재시도해서는 안 된다 — 키를 바꾸거나 원래 페이로드로 보내야 한다.
    /// HTTP 표면은 이 에러를 409 Conflict로 매핑한다.
    ///
    /// 기존 Task의 프롬프트나 결과는 담지 않는다. 같은 `created_by` 버킷을
    /// 여러 호출자가 공유할 수 있어(마이그레이션 024 참고) 키를 재사용한 쪽이
    /// 그 Task의 소유자라는 보장이 없다.
    #[error(
        "idempotency key '{key}' already used with a different payload (task {existing_task_id})"
    )]
    IdempotencyConflict {
        key: String,
        existing_task_id: TaskId,
    },

    /// 이 인스턴스가 지금 control plane lease를 쥐고 있지 않다(로드맵
    /// `#63` 불변식 2). Cold Standby거나 갱신에 실패해 fenced됐다 —
    /// 재시도는 안전하지만(Reconciler가 다음 tick에 다시 시도), 지금 이
    /// 인스턴스가 dispatch를 대신 수행하면 안 된다.
    #[error("this instance does not currently hold the control plane lease")]
    ControlPlaneFenced,

    /// 제출된 요청 자체가 규칙을 어겨 거절됐다 (로드맵 #69).
    ///
    /// 재시도해서는 안 된다 — 같은 페이로드는 항상 같은 판정을 받는다.
    /// [`DispatchError::NoWorker`]/[`DispatchError::CircuitOpen`]과 달리
    /// 시간이 지난다고 해소되지 않으므로 `#38`의 재시도 대상이 아니다.
    #[error("invalid task request: {0}")]
    InvalidRequest(String),
}

impl From<TransportError> for DispatchError {
    fn from(e: TransportError) -> Self {
        match e {
            // 요청 결함은 워커 결함이 아니다 — 뭉뜽그려 `Transport`로 접으면
            // 호출자가 둘을 구분할 수 없고, 재시도·회로 판단이 전부 틀린다.
            TransportError::InvalidRequest(msg) => DispatchError::InvalidRequest(msg),
            other => DispatchError::Transport(other.to_string()),
        }
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

    /// 로드맵 `#63` 불변식 2 — [`DispatchError::ControlPlaneFenced`] 참고.
    #[error("this instance does not currently hold the control plane lease")]
    ControlPlaneFenced,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::FleetState;
    use fleet_core::{CircuitBreakerConfig, TaskRequest};
    use fleet_store::mem::MemStore;
    use fleet_store::Store;

    /// 워커가 하나도 없는(선택 실패가 보장되는) `FleetState` + `Dispatcher` 조립.
    fn setup_no_workers(max_dispatch_retries: u32) -> (Arc<FleetState>, Dispatcher) {
        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        let transport: Arc<dyn fleet_transport::WorkerTransport> =
            Arc::new(fleet_transport::MockTransport::new());
        let state = Arc::new(FleetState::new(
            store,
            transport,
            CircuitBreakerConfig::default(),
        ));
        let dispatcher =
            Dispatcher::new(state.clone()).with_max_dispatch_retries(max_dispatch_retries);
        (state, dispatcher)
    }

    fn sample_task() -> Task {
        Task::from_request(TaskRequest {
            prompt: "hello".into(),
            created_by: "test".into(),
            // 로드맵 #69 — `cwd`는 더 이상 생략할 수 없다. 이 헬퍼가 값을
            // 채우므로 아래 테스트 대부분은 이 규칙과 무관하게 예전 의미를
            // 그대로 유지한다. 생략 자체를 검사하는 것은 아래
            // `submit_rejects_a_task_without_cwd_before_it_is_stored`뿐이다.
            cwd: Some("/srv/fleet/workspaces/test".into()),
            ..Default::default()
        })
    }

    // 로드맵 #69 — 입국 심사는 저장 **전에** 걸려야 한다. 뒤에 있으면 영원히
    // dispatch될 수 없는 행이 Pending으로 남아 재조정 루프가 매 tick마다 같은
    // 실패를 반복한다. 그래서 "Err를 받았다"만으로는 부족하고 "행이 없다"까지
    // 확인한다.
    #[tokio::test]
    async fn submit_rejects_a_task_without_cwd_before_it_is_stored() {
        let (state, dispatcher) = setup_no_workers(0);
        let mut task = sample_task();
        task.cwd = None;
        let task_id = task.id;

        let err = dispatcher
            .submit(task)
            .await
            .expect_err("cwd is required — submit() must fail");
        assert!(
            matches!(err, DispatchError::InvalidRequest(_)),
            "expected InvalidRequest, got {err:?}"
        );

        assert!(
            state.store.get_task(task_id).await.unwrap().is_none(),
            "a rejected submit must not leave a Task row behind"
        );
    }

    // 파일시스템 루트는 명시해도 거절된다. 이 변경이 없애려는 상태(루트에서
    // 열린 에이전트 세션)를 클라이언트가 이름을 대어 되살릴 수 있으면 수정이
    // 반쪽이 된다.
    #[tokio::test]
    async fn submit_rejects_filesystem_root_as_cwd() {
        let (state, dispatcher) = setup_no_workers(0);
        let mut task = sample_task();
        task.cwd = Some("/".into());
        let task_id = task.id;

        let err = dispatcher.submit(task).await.expect_err("'/' is rejected");
        assert!(
            matches!(err, DispatchError::InvalidRequest(_)),
            "expected InvalidRequest, got {err:?}"
        );
        assert!(state.store.get_task(task_id).await.unwrap().is_none());
    }

    // 로드맵 #38: 재시도 비활성(기본값, max_dispatch_retries == 0)일 때는 이
    // 필드 도입 이전과 동일하게 워커 선택 실패에서 즉시 Err + Failed 확정.
    #[tokio::test]
    async fn submit_marks_failed_immediately_when_retries_disabled() {
        let (state, dispatcher) = setup_no_workers(0);
        let task = sample_task();
        let task_id = task.id;

        let err = dispatcher
            .submit(task)
            .await
            .expect_err("no worker available — submit() must fail");
        assert!(matches!(err, DispatchError::NoWorker(_)));

        let stored = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(stored.status, TaskStatus::Failed(_)),
            "expected Failed, got {:?}",
            stored.status
        );
    }

    // 재시도 활성(max_dispatch_retries > 0)일 때는 WorkerUnavailable을 즉시
    // 확정하지 않고 Pending으로 남긴 채 Ok(task_id)를 반환, retry_count를 1
    // 올린다 — Reconciler가 백그라운드에서 재시도를 이어받는다.
    #[tokio::test]
    async fn submit_leaves_task_pending_and_records_retry_when_retries_enabled() {
        let (state, dispatcher) = setup_no_workers(3);
        let task = sample_task();
        let task_id = task.id;

        let returned_id = dispatcher
            .submit(task)
            .await
            .expect("submit() must succeed when retries are enabled");
        assert_eq!(returned_id, task_id);

        let stored = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(stored.status, TaskStatus::Pending),
            "expected Pending, got {:?}",
            stored.status
        );
        assert_eq!(stored.retry_count, 1);
    }

    // 로드맵 #69 — 회로 차단기 오염 방지.
    //
    // 검증을 추가하면 새 DoS 경로가 열릴 수 있다: 클라이언트가 잘못된 `cwd`를
    // 반복 제출하는 것만으로 멀쩡한 워커의 회로를 열 수 있다면, 방어가 공격
    // 표면이 된다. 그래서 **요청 결함**은 차단기에 기록하지 않는다.
    //
    // 대조군을 같은 테스트에 둔다. 대조군이 없으면 "회로가 안 열렸다"가
    // 면제 때문인지 애초에 이 설정에서 회로가 열리지 않기 때문인지 구분되지
    // 않는다 — 통과하지만 아무것도 증명하지 않는 테스트가 된다.
    #[tokio::test]
    async fn invalid_request_does_not_open_the_circuit_but_a_worker_fault_does() {
        use fleet_transport::WorkerTransport as _;

        // 실패 1건으로 곧장 열리는 설정.
        let cb_config = CircuitBreakerConfig {
            enabled: true,
            min_samples: 1,
            error_rate_threshold: 0.1,
            ..CircuitBreakerConfig::default()
        };

        // ── 실험군: 잘못된 cwd ──────────────────────────────────────────
        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        let mut worker = fleet_core::Worker::new("w-invalid", "wss://w/ws");
        worker.status = fleet_core::WorkerStatus::Online;
        store.upsert_worker(&worker).await.unwrap();

        let mock = fleet_transport::MockTransport::new();
        mock.register(worker.id, "wss://w/ws", 4).await.unwrap();
        let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(mock);
        let state = Arc::new(FleetState::new(store.clone(), transport, cb_config.clone()));
        let dispatcher = Dispatcher::new(state.clone());

        // 검증 이전에 저장된 행을 흉내 낸다 — `submit()`의 입국 심사를
        // 우회해 store에 직접 넣는다. 실제로 이 경로로 들어오는 것이
        // `fleet tasks submit` 경유분과 레거시 행이다.
        let mut task = sample_task();
        task.cwd = None;
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let err = dispatcher
            .dispatch_existing(task, true)
            .await
            .expect_err("invalid cwd must fail the dispatch");
        assert!(
            matches!(err, DispatchError::InvalidRequest(_)),
            "expected InvalidRequest (not collapsed into Transport), got {err:?}"
        );

        let stored = store.get_task(task_id).await.unwrap().unwrap();
        match stored.status {
            TaskStatus::Failed(f) => assert_eq!(
                f.kind,
                fleet_core::FailureKind::InvalidRequest,
                "the worker never saw this request — WorkerError would send the \
                 operator to the worker logs"
            ),
            other => panic!("expected Failed, got {other:?}"),
        }
        // 회로 상태는 **이벤트 로그**로 관찰한다. `MemStore`는
        // `update_worker_circuit_state`의 트레이트 기본 구현(no-op)을 그대로
        // 쓰므로 `get_worker().circuit_state`는 무엇을 해도 Closed로 남는다 —
        // 그걸로 판정하면 면제가 동작하든 말든 통과하는 테스트가 된다.
        assert!(
            !circuit_opened(state.store.as_ref(), worker.id).await,
            "a malformed request must not count against the worker's health"
        );

        // ── 대조군: 워커 결함(용량 소진) ────────────────────────────────
        let store2: Arc<dyn Store> = Arc::new(MemStore::new());
        let mut worker2 = fleet_core::Worker::new("w-fault", "wss://w2/ws");
        worker2.status = fleet_core::WorkerStatus::Online;
        store2.upsert_worker(&worker2).await.unwrap();

        // store에는 Online으로 넣되 transport에는 등록하지 않는다 — selector는
        // 이 워커를 고르지만 dispatch는 `WorkerNotRegistered`로 실패한다.
        // 명백히 **워커 쪽** 결함이다.
        let mock2 = fleet_transport::MockTransport::new();
        let transport2: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(mock2);
        let state2 = Arc::new(FleetState::new(store2.clone(), transport2, cb_config));
        let dispatcher2 = Dispatcher::new(state2.clone());

        let task2 = sample_task(); // cwd는 유효하다.
        store2.insert_task(&task2).await.unwrap();
        let err2 = dispatcher2
            .dispatch_existing(task2, true)
            .await
            .expect_err("an unregistered worker must fail the dispatch");
        // `#70` 게이트 2 이후로 **못 닿는 워커는 transport dispatch보다 먼저**
        // 걸린다 — dispatch 전 인벤토리 조회가 같은 연결을 쓰기 때문이다.
        // 에러 이름만 바뀌었고 이 시험의 주장은 그대로다: 아래 회로 단정이
        // 그 주장이며, 복구 실패도 transport 실패와 **같은 자리에** 브레이커
        // 기록을 남기므로 통과한다. 그 기록을 빠뜨리면 selector가 못 닿는
        // 워커를 영영 계속 고른다.
        assert!(
            matches!(err2, DispatchError::RecoveryUnavailable(_)),
            "expected RecoveryUnavailable, got {err2:?}"
        );

        assert!(
            circuit_opened(state2.store.as_ref(), worker2.id).await,
            "control: a genuine worker fault under this config does open the circuit"
        );
    }

    /// 이벤트 로그에서 해당 워커의 회로가 Open으로 전이한 적이 있는지.
    async fn circuit_opened(store: &dyn Store, worker_id: WorkerId) -> bool {
        store
            .list_events(0, 1000)
            .await
            .expect("events")
            .into_iter()
            .any(|e| {
                matches!(
                    e.event,
                    fleet_core::FleetEvent::WorkerCircuitChanged {
                        worker_id: w,
                        to: fleet_core::CircuitState::Open,
                        ..
                    } if w == worker_id
                )
            })
    }

    // 로드맵 #71 — worker는 온라인이고 `model` 라벨도 일치하지만 해당 model의
    // credential을 보유하지 않은 경우: 재시도 비활성(기본값) 상태에서는
    // WorkerUnavailable이 아니라 FailureKind::CredentialMissing으로 즉시
    // Failed 확정되어야 한다 — 원인이 명확히 credential 미프로비저닝임을
    // 나타내기 위함.
    #[tokio::test]
    async fn submit_marks_credential_missing_when_worker_lacks_credential() {
        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        let mut worker = fleet_core::Worker::new("gemini-1", "wss://gemini-1/ws");
        worker.status = fleet_core::WorkerStatus::Online;
        worker.labels.insert("model".into(), "gemini".into());
        store.upsert_worker(&worker).await.unwrap();
        // 의도적으로 credential을 프로비저닝하지 않는다.

        let transport: Arc<dyn fleet_transport::WorkerTransport> =
            Arc::new(fleet_transport::MockTransport::new());
        let state = Arc::new(FleetState::new(
            store.clone(),
            transport,
            CircuitBreakerConfig::default(),
        ));
        let dispatcher = Dispatcher::new(state.clone());

        let mut task = sample_task();
        task.model = Some("gemini".into());
        let task_id = task.id;

        let err = dispatcher
            .submit(task)
            .await
            .expect_err("no credentialed worker — submit() must fail");
        assert!(matches!(err, DispatchError::NoWorker(_)));

        let stored = state.store.get_task(task_id).await.unwrap().unwrap();
        match stored.status {
            TaskStatus::Failed(f) => assert_eq!(
                f.kind,
                fleet_core::FailureKind::CredentialMissing,
                "expected CredentialMissing, got {:?}",
                f.kind
            ),
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    // 지정된 model의 credential을 가진 worker만 있을 때는 정상적으로
    // 즉시(재시도 없이) 워커가 선택되어야 한다 — credential 필터가 정상
    // 경로를 막지 않음을 확인.
    #[tokio::test]
    async fn submit_selects_worker_when_credential_present() {
        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        let mut worker = fleet_core::Worker::new("gemini-1", "wss://gemini-1/ws");
        worker.status = fleet_core::WorkerStatus::Online;
        worker.labels.insert("model".into(), "gemini".into());
        let worker_id = worker.id;
        store.upsert_worker(&worker).await.unwrap();
        store
            .upsert_worker_credential(
                "gemini-1",
                "gemini",
                "encrypted-blob",
                "https://example.test",
                "test-backend",
                128_000,
                None,
            )
            .await
            .unwrap();

        let transport = fleet_transport::MockTransport::new();
        transport
            .add_worker(fleet_transport::MockWorker::new(
                worker_id,
                "wss://gemini-1/ws",
            ))
            .await;
        let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(transport);
        let state = Arc::new(FleetState::new(
            store.clone(),
            transport,
            CircuitBreakerConfig::default(),
        ));
        let dispatcher = Dispatcher::new(state.clone());

        let mut task = sample_task();
        task.model = Some("gemini".into());
        let task_id = task.id;

        dispatcher
            .submit(task)
            .await
            .expect("submit() must succeed — worker holds the required credential");

        let stored = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(stored.status, TaskStatus::Dispatched { .. }),
            "expected Dispatched, got {:?}",
            stored.status
        );
    }

    // ── 로드맵 #63 2단계: control plane lease gating ────────────────────

    use crate::lease::{LeaseObserver, LeaseStatus};

    fn setup_fenced() -> (Arc<FleetState>, Dispatcher) {
        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        let transport: Arc<dyn fleet_transport::WorkerTransport> =
            Arc::new(fleet_transport::MockTransport::new());
        let state = Arc::new(
            FleetState::new(store, transport, CircuitBreakerConfig::default()).with_lease(
                LeaseObserver::with_status("test-cluster", "inst-a", LeaseStatus::Fenced),
            ),
        );
        let dispatcher = Dispatcher::new(state.clone());
        (state, dispatcher)
    }

    /// 감사 기록에서 그 결정을 찾는다. 없으면 목록 전체를 보여 준다 —
    /// "없다"는 실패에서 무엇이 있었는지가 곧 진단이다.
    async fn audited(store: &dyn Store, action: &str) -> fleet_core::AuditEvent {
        let events = store
            .list_audit_events(&fleet_core::AuditFilter {
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        events
            .iter()
            .find(|e| e.action == action)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "{action}이 감사에 남아야 한다 — 받은 것 {:?}",
                    events.iter().map(|e| &e.action).collect::<Vec<_>>()
                )
            })
    }

    /// 리스가 없어 거절한 dispatch가 **감사에 남는다** (로드맵 `#70` 게이트 6 선행).
    ///
    /// 이 시험이 이 증분의 출발점이다. 그 전까지 `fleet-scheduler`는 감사
    /// 기록을 하나도 내지 않았고(실측: emitter 35군데가 전부 API·대시보드·core),
    /// 제어면 결정은 `warn!` 로그로만 남았다 — 관측성 정본의 alert 표가 요구하는
    /// "fencing/epoch 증거 확인"에 조회할 원천이 없었다.
    ///
    /// **`control_epoch`가 `None`인 것이 결함이 아니라 내용이다.** 리스를 갖지
    /// 못한 인스턴스에는 세대가 없고, 그 사실은 `lease_status`가 나른다.
    #[tokio::test]
    async fn a_dispatch_refused_without_the_lease_is_audited() {
        let (state, dispatcher) = setup_fenced();
        let task = sample_task();
        let task_id = task.id;
        state.store.insert_task(&task).await.unwrap();

        let err = dispatcher.dispatch_existing(task, false).await.unwrap_err();
        assert!(matches!(err, DispatchError::ControlPlaneFenced));

        let e = audited(
            &*state.store,
            fleet_core::audit::action::CONTROL_DISPATCH_REFUSED,
        )
        .await;
        assert_eq!(e.target_id.as_deref(), Some(task_id.to_string().as_str()));
        assert_eq!(
            e.actor_label, "orchestrator:inst-a",
            "행위자는 사람이 아니라 이 인스턴스다 — cluster_id로는 둘 이상의 \
             owner를 구분할 수 없다"
        );
        assert_eq!(
            e.control_epoch, None,
            "세대를 갖지 못한 채 내린 거절이므로 epoch가 없는 것이 사실이다"
        );
        assert_eq!(
            e.detail.get("lease_status").and_then(|v| v.as_str()),
            Some("fenced"),
            "epoch가 없는 이유는 detail이 날라야 한다: {:?}",
            e.detail
        );
    }

    /// 리스가 설정되지 않은 배포는 **아무것도 남기지 않는다**.
    ///
    /// 단일 인스턴스 배포에는 세대도 경합도 없으므로 남길 사실이 없다.
    /// 이 단정이 없으면 "모든 경로에서 무조건 감사한다"는 구현이 통과하고,
    /// 그러면 리스를 쓰지 않는 배포의 감사 로그가 의미 없는 행으로 찬다.
    #[tokio::test]
    async fn a_deployment_without_a_lease_records_no_control_audit() {
        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        let transport: Arc<dyn fleet_transport::WorkerTransport> =
            Arc::new(fleet_transport::MockTransport::new());
        let state = Arc::new(FleetState::new(
            store.clone(),
            transport,
            CircuitBreakerConfig::default(),
        ));
        let dispatcher = Dispatcher::new(state.clone());

        let task = sample_task();
        state.store.insert_task(&task).await.unwrap();
        // 리스가 없으면 `lease_allows_control()`이 true라 거절 경로에 닿지
        // 않는다. 그래서 워커 이벤트 경로로 확인한다 — 그쪽도 같은 함수를 쓴다.
        state
            .audit_control(
                fleet_core::audit::action::CONTROL_DISPATCH_REFUSED,
                ("task", task.id.to_string()),
                serde_json::json!({}),
            )
            .await;
        let _ = dispatcher;

        let events = store
            .list_audit_events(&fleet_core::AuditFilter {
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            events.is_empty(),
            "리스 없는 배포에는 남길 제어면 사실이 없다: {:?}",
            events.iter().map(|e| &e.action).collect::<Vec<_>>()
        );
    }

    /// 리스를 잃은 채 도착한 워커 결과가 **버려졌다는 사실**이 남는다
    /// (로드맵 `#70` 게이트 6 선행).
    ///
    /// 그 Task의 최종 상태를 아무도 확정하지 않은 채 창이 닫혔다는 뜻이라,
    /// 새 리스 소유자의 재조정이 반드시 다시 봐야 하는 항목이다. 지금까지
    /// 이 사실은 `warn!` 한 줄로만 남았다.
    #[tokio::test]
    async fn an_abandoned_worker_outcome_is_audited() {
        let (state, dispatcher) = setup_fenced();
        let worker = fleet_core::Worker::new("w1", "wss://w1/ws");
        state.store.upsert_worker(&worker).await.unwrap();
        let mut task = sample_task();
        let task_id = task.id;
        task.status = TaskStatus::Dispatched {
            worker_id: worker.id,
            started_at: Utc::now(),
        };
        state.store.insert_task(&task).await.unwrap();

        dispatcher
            .handle_worker_event(WorkerEvent::Failed {
                task_id,
                error: "boom".into(),
                observation: fleet_transport::FailureObservation::Reported,
            })
            .await;

        let e = audited(
            &*state.store,
            fleet_core::audit::action::CONTROL_OUTCOME_ABANDONED,
        )
        .await;
        assert_eq!(e.target_id.as_deref(), Some(task_id.to_string().as_str()));
        assert_eq!(e.control_epoch, None);
    }

    /// 저장소가 막은 쓰기는 **들고 있던 epoch와 함께** 감사에 남는다
    /// (로드맵 `#70` 게이트 6 선행).
    ///
    /// **이 시험이 `control_epoch` 컬럼의 존재 이유 전부다.** 다른 두 제어면
    /// 감사(거절·유기)에서는 이 값이 항상 `None`이라, 그것들만 있으면 컬럼이
    /// 영원히 비어 있어도 아무도 눈치채지 못한다.
    ///
    /// 그리고 이 경우가 관측성 정본의 alert 표가 말하는 **"둘 이상의 owner
    /// 관측"의 직접 증거**다. 앞의 거절은 이 인스턴스가 스스로 물러선 것이지만,
    /// 여기서는 리스가 있다고 **믿고** 쓰러 갔다가 저장소에 막혔다 — 즉 다른
    /// 누군가가 그 사이에 세대를 가져갔다는 뜻이고, 조사에 필요한 것이 바로
    /// 이 인스턴스가 그때 믿고 있던 세대다.
    #[tokio::test]
    async fn a_write_the_store_fenced_is_audited_with_the_epoch_we_held() {
        let ttl = std::time::Duration::from_secs(60);
        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        let held = store
            .acquire_control_lease("c70", "instance-a", ttl, None)
            .await
            .unwrap();
        store
            .release_control_lease("c70", "instance-a", held.epoch)
            .await
            .unwrap();
        let taken = store
            .acquire_control_lease("c70", "instance-b", ttl, None)
            .await
            .unwrap();
        assert!(taken.epoch > held.epoch, "시험 전제: 세대가 넘어갔다");

        let mock = Arc::new(fleet_transport::MockTransport::new());
        let transport: Arc<dyn fleet_transport::WorkerTransport> = mock.clone();
        let state = Arc::new(
            FleetState::new(store.clone(), transport, CircuitBreakerConfig::default()).with_lease(
                LeaseObserver::with_status(
                    "c70",
                    "instance-a",
                    // 낡았지만 본인은 Active로 믿는다 — bool 게이트는 열려 있다.
                    LeaseStatus::Active { epoch: held.epoch },
                ),
            ),
        );
        let dispatcher = Dispatcher::new(state.clone());
        assert!(
            state.lease_allows_control(),
            "관측이 아직 Active여야 이 시나리오가 성립한다"
        );

        // 후보 워커가 있어야 선택을 지나 CAS까지 간다. **transport에도**
        // 등록한다 — `#70` 게이트 2의 dispatch 전 인벤토리 조회가 등록된
        // 워커만 알기 때문이다. 저장소에만 넣으면 CAS에 닿기도 전에
        // `RecoveryUnavailable`로 끝나 이 시험이 보려는 fenced 경로가 아예
        // 실행되지 않는다.
        let worker = fleet_core::Worker::new("w1", "wss://w1/ws");
        store.upsert_worker(&worker).await.unwrap();
        mock.add_worker(fleet_transport::MockWorker::new(
            worker.id,
            worker.endpoint.clone(),
        ))
        .await;
        let task = sample_task();
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let err = dispatcher
            .dispatch_existing(task, false)
            .await
            .expect_err("저장소가 새 epoch로 옮겨갔으면 dispatch는 거절돼야 한다");
        assert!(matches!(err, DispatchError::ControlPlaneFenced));

        let e = audited(&*store, fleet_core::audit::action::CONTROL_WRITE_FENCED).await;
        assert_eq!(e.target_id.as_deref(), Some(task_id.to_string().as_str()));
        assert_eq!(e.actor_label, "orchestrator:instance-a");
        assert_eq!(
            e.control_epoch,
            Some(held.epoch),
            "막힌 순간 이 인스턴스가 믿고 있던 세대가 실려야 한다 — 저장소가 \
             이미 옮겨간 {}이 아니다",
            taken.epoch
        );
        // 스스로 물러선 것과 저장소에 막힌 것은 **다른 사실**이므로 다른
        // 행으로 남아야 한다. 둘을 한 이름으로 접으면 조사가 갈리지 않는다.
        let all = store
            .list_audit_events(&fleet_core::AuditFilter {
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            !all.iter()
                .any(|a| a.action == fleet_core::audit::action::CONTROL_DISPATCH_REFUSED),
            "bool 게이트는 통과했으므로 거절 기록은 없어야 한다: {:?}",
            all.iter().map(|a| &a.action).collect::<Vec<_>>()
        );
    }

    /// 로드맵 #62 3단계. 위의 `setup_fenced` 계열과 **다른 창**을 다룬다.
    ///
    /// 그쪽은 이미 `Fenced`를 관측한 뒤의 동작이라 `allows_control()`의 bool
    /// 하나로 막힌다. 여기서는 관측이 아직 `Active`인 채로 저장소만 앞서 간
    /// 상태를 만든다 — 갱신 주기(5초) 안에 장애 전환이 일어나면 실제로 이렇게
    /// 된다. bool 검사는 통과하므로, 거절의 근거는 CAS에 실린 epoch 술어뿐이다.
    #[tokio::test]
    async fn cancel_is_fenced_when_the_store_moved_to_a_newer_epoch() {
        let ttl = std::time::Duration::from_secs(60);
        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        let held = store
            .acquire_control_lease("c62", "instance-a", ttl, None)
            .await
            .unwrap();

        // 장애 전환. instance-a의 observer는 이 사실을 아직 모른다.
        store
            .release_control_lease("c62", "instance-a", held.epoch)
            .await
            .unwrap();
        let taken = store
            .acquire_control_lease("c62", "instance-b", ttl, None)
            .await
            .unwrap();
        assert!(taken.epoch > held.epoch);

        let transport: Arc<dyn fleet_transport::WorkerTransport> =
            Arc::new(fleet_transport::MockTransport::new());
        let state = Arc::new(
            FleetState::new(store.clone(), transport, CircuitBreakerConfig::default()).with_lease(
                LeaseObserver::with_status(
                    "c62",
                    "inst-a",
                    LeaseStatus::Active {
                        epoch: held.epoch, // 낡았지만 본인은 Active로 믿는다
                    },
                ),
            ),
        );
        let dispatcher = Dispatcher::new(state.clone());

        // 이 단언이 이 테스트의 요점이다 — bool 게이트는 열려 있다.
        assert!(
            state.lease_allows_control(),
            "관측이 아직 Active여야 이 시나리오가 성립한다"
        );

        let task = sample_task();
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let err = dispatcher
            .cancel(task_id, "operator")
            .await
            .expect_err("저장소가 새 epoch로 옮겨갔으면 취소는 거절돼야 한다");
        assert!(matches!(err, CancelError::ControlPlaneFenced));

        let stored = store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(stored.status, TaskStatus::Pending),
            "거절된 취소는 상태를 바꾸지 않아야 한다, got {:?}",
            stored.status
        );
    }

    #[tokio::test]
    async fn submit_refuses_when_control_plane_lease_is_fenced() {
        let (state, dispatcher) = setup_fenced();
        let task = sample_task();
        let task_id = task.id;

        let err = dispatcher
            .submit(task)
            .await
            .expect_err("submit() must refuse dispatch while fenced");
        assert!(matches!(err, DispatchError::ControlPlaneFenced));

        // task row는 insert_task까지는 진행됐으니 존재하되, dispatch_existing
        // 이전에 거절됐으므로 여전히 Pending이어야 한다 — Failed로 잘못
        // 확정되면 안 된다.
        let stored = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(stored.status, TaskStatus::Pending),
            "expected Pending (never touched by dispatch), got {:?}",
            stored.status
        );
    }

    #[tokio::test]
    async fn cancel_refuses_when_control_plane_lease_is_fenced_even_for_unknown_task() {
        let (_state, dispatcher) = setup_fenced();
        // 존재하지 않는 task_id를 써도 NotFound가 아니라 ControlPlaneFenced가
        // 먼저 나와야 한다 — lease 확인이 조회보다 앞선다.
        let bogus_task_id = TaskId::new();

        let err = dispatcher
            .cancel(bogus_task_id, "test")
            .await
            .expect_err("cancel() must refuse while fenced");
        assert!(matches!(err, CancelError::ControlPlaneFenced));
    }

    #[tokio::test]
    async fn completed_event_is_dropped_when_control_plane_lease_is_fenced() {
        let (state, dispatcher) = setup_fenced();

        let worker = fleet_core::Worker::new("w1", "wss://w1/ws");
        state.store.upsert_worker(&worker).await.unwrap();

        let mut task = sample_task();
        let task_id = task.id;
        task.status = TaskStatus::Dispatched {
            worker_id: worker.id,
            started_at: Utc::now(),
        };
        state.store.insert_task(&task).await.unwrap();

        let result = fleet_core::TaskResult {
            output: "should never be persisted while fenced".into(),
            exit_code: 0,
            duration_secs: 1.0,
            token_usage: None,
            worker_id: worker.id,
            finished_at: Utc::now(),
        };
        dispatcher
            .handle_worker_event(WorkerEvent::Completed { task_id, result })
            .await;

        // fenced 인스턴스는 이 이벤트를 버려야 한다 — task는 Dispatched에서
        // 움직이지 않는다. 새 lease 소유자가 나중에 재조정해야 할 상태다.
        let stored = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(stored.status, TaskStatus::Dispatched { .. }),
            "expected Dispatched (event dropped), got {:?}",
            stored.status
        );
    }

    #[tokio::test]
    async fn failed_event_is_dropped_when_control_plane_lease_is_fenced() {
        let (state, dispatcher) = setup_fenced();

        let worker = fleet_core::Worker::new("w1", "wss://w1/ws");
        state.store.upsert_worker(&worker).await.unwrap();

        let mut task = sample_task();
        let task_id = task.id;
        task.status = TaskStatus::Dispatched {
            worker_id: worker.id,
            started_at: Utc::now(),
        };
        state.store.insert_task(&task).await.unwrap();

        dispatcher
            .handle_worker_event(WorkerEvent::Failed {
                task_id,
                error: "should never be persisted while fenced".into(),
                observation: fleet_transport::FailureObservation::Reported,
            })
            .await;

        let stored = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(stored.status, TaskStatus::Dispatched { .. }),
            "expected Dispatched (event dropped), got {:?}",
            stored.status
        );
    }

    #[tokio::test]
    async fn output_event_is_still_processed_when_control_plane_lease_is_fenced() {
        // Output은 권위 있는 "제어" 결정이 아니라 순수 관측 데이터 전달이라,
        // fenced 상태에서도 계속 흘려보내야 한다 — breaker/task 최종 상태만
        // 막는 것이지 모든 쓰기를 막는 게 아니다.
        let (state, dispatcher) = setup_fenced();

        let worker = fleet_core::Worker::new("w1", "wss://w1/ws");
        state.store.upsert_worker(&worker).await.unwrap();

        let mut task = sample_task();
        let task_id = task.id;
        task.status = TaskStatus::Dispatched {
            worker_id: worker.id,
            started_at: Utc::now(),
        };
        state.store.insert_task(&task).await.unwrap();

        dispatcher
            .handle_worker_event(WorkerEvent::Output {
                task_id,
                seq: 1,
                chunk: "hello from worker".into(),
            })
            .await;

        let output = state.store.get_output(task_id, 0).await.unwrap();
        assert!(
            output
                .chunks
                .iter()
                .any(|c| c.chunk.contains("hello from worker")),
            "output must still be buffered while fenced: {output:?}"
        );
    }

    /// 도구 호출 관측이 **durable 이벤트로 남는다** (로드맵 `#70` 게이트 4 선행).
    ///
    /// transport 쪽 시험은 관측이 오케스트레이터에 **도달**하는 것까지 본다.
    /// 도달한 것이 사라지지 않는지는 여기서 본다 — 둘 사이가 이 증분에서
    /// 새로 생긴 구간이고, effect ledger가 나중에 읽을 곳이 바로 이 로그다.
    ///
    /// **fenced 상태에서 돌린다.** 관측을 리스 뒤에 가두면 안 되는 이유가
    /// `Output`보다 이쪽이 강하다: 리스를 잃은 창은 새 소유자가 재조정해야
    /// 하는 구간이고, 그 재조정이 물을 "이 Task가 무엇을 했는가"의 증거가
    /// 정확히 이 이벤트다.
    #[tokio::test]
    async fn a_tool_call_observation_is_recorded_even_while_fenced() {
        let (state, dispatcher) = setup_fenced();

        let worker = fleet_core::Worker::new("w1", "wss://w1/ws");
        state.store.upsert_worker(&worker).await.unwrap();

        let mut task = sample_task();
        let task_id = task.id;
        task.status = TaskStatus::Dispatched {
            worker_id: worker.id,
            started_at: Utc::now(),
        };
        state.store.insert_task(&task).await.unwrap();

        dispatcher
            .handle_worker_event(WorkerEvent::ToolCall {
                task_id,
                worker_id: worker.id,
                invocation: fleet_core::ToolInvocation {
                    tool_call_id: "tc-9".into(),
                    name: Some("bash".into()),
                    kind: fleet_core::ToolInvocationKind::Execute,
                    status: fleet_core::ToolInvocationStatus::Completed,
                },
            })
            .await;

        let events = state.store.list_events(0, 100).await.unwrap();
        let recorded = events
            .iter()
            .find(|e| e.event.event_type() == "task_tool_call")
            .unwrap_or_else(|| {
                panic!(
                    "도구 호출이 이벤트 로그에 남아야 한다 — 받은 것 {:?}",
                    events
                        .iter()
                        .map(|e| e.event.event_type())
                        .collect::<Vec<_>>()
                )
            });
        let payload = serde_json::to_string(&recorded.event).unwrap();
        assert!(
            payload.contains("tc-9"),
            "어느 호출인지 남아야 한다: {payload}"
        );
        assert!(payload.contains("execute"), "종류가 남아야 한다: {payload}");
        assert!(
            payload.contains(&worker.id.to_string()),
            "나중에 process inventory와 대조할 축이 남아야 한다: {payload}"
        );
    }

    // ── tool 허용 목록 탐지 (로드맵 `#64`·`#86` 선행) ──────────────────
    //
    // `AgentTemplateBody.tools`는 그동안 저장되고 Dashboard에 표시되기만 하고
    // **아무도 읽지 않는 필드**였다. 강제되지 않는 허용 목록은 없는 것보다
    // 위험하다 — 통제처럼 보이기 때문이다. 아래 넷은 그 필드가 이제 실제로
    // 판정에 쓰인다는 것과, **판정하지 않는 경우를 위반으로 접지 않는다는 것**을
    // 함께 지킨다. 하나만 두면 "항상 그 답을 내는" 구현으로도 통과한다.

    /// pin된 템플릿과 그 Agent에 묶인 Task를 만들어 돌려준다.
    async fn agent_with_tools(state: &Arc<FleetState>, tools: Vec<String>) -> Task {
        let project = fleet_core::Project::new(format!("p-{}", uuid::Uuid::new_v4()));
        state.store.create_project(&project).await.unwrap();

        let template = fleet_core::AgentTemplate::new(Some(project.id), "role");
        state.store.create_agent_template(&template).await.unwrap();
        let mut body = fleet_core::AgentTemplateBody::new("do things");
        body.tools = tools;
        let revision = state
            .store
            .create_agent_template_revision(template.id, &body, Some("test"))
            .await
            .unwrap();

        // 템플릿은 draft로 태어나고 draft는 pin을 받지 않는다 — 실제 도메인
        // 규칙이라 시험도 같은 경로를 밟아야 한다.
        state
            .store
            .update_agent_template_status(template.id, fleet_core::AgentTemplateStatus::Published)
            .await
            .unwrap();

        let mut agent = fleet_core::Agent::new(project.id, "a1");
        agent = agent.with_template_pin(fleet_core::AgentTemplatePin {
            template_id: template.id,
            revision_id: revision.id,
        });
        state.store.create_agent(&agent).await.unwrap();

        let mut task = sample_task();
        task.agent_id = Some(agent.id);
        task.project_id = Some(project.id);
        state.store.insert_task(&task).await.unwrap();
        task
    }

    async fn allowlist_audit_count(state: &Arc<FleetState>) -> usize {
        state
            .store
            .list_audit_events(&fleet_core::AuditFilter {
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap()
            .iter()
            .filter(|e| e.action == fleet_core::audit::action::AGENT_TOOL_OUTSIDE_ALLOWLIST)
            .count()
    }

    async fn observe_tool(
        dispatcher: &Dispatcher,
        task_id: TaskId,
        worker_id: WorkerId,
        name: Option<&str>,
    ) {
        dispatcher
            .handle_worker_event(WorkerEvent::ToolCall {
                task_id,
                worker_id,
                invocation: fleet_core::ToolInvocation {
                    tool_call_id: "tc-1".into(),
                    name: name.map(str::to_string),
                    kind: fleet_core::ToolInvocationKind::Execute,
                    status: fleet_core::ToolInvocationStatus::Completed,
                },
            })
            .await;
    }

    /// 목록 **안**의 도구는 아무것도 남기지 않는다.
    ///
    /// 이 케이스가 없으면 아래 위반 테스트가 "항상 위반으로 적는" 구현으로도
    /// 통과한다.
    #[tokio::test]
    async fn a_tool_inside_the_allowlist_is_not_flagged() {
        let (state, dispatcher) = setup_fenced();
        let task = agent_with_tools(&state, vec!["bash".into(), "read".into()]).await;

        observe_tool(&dispatcher, task.id, WorkerId::new(), Some("bash")).await;

        assert_eq!(
            allowlist_audit_count(&state).await,
            0,
            "허용된 도구가 위반으로 남으면 신호가 죽는다"
        );
    }

    /// 목록 **밖**의 도구는 감사에 남는다.
    #[tokio::test]
    async fn a_tool_outside_the_allowlist_is_recorded() {
        let (state, dispatcher) = setup_fenced();
        let task = agent_with_tools(&state, vec!["read".into()]).await;

        observe_tool(&dispatcher, task.id, WorkerId::new(), Some("bash")).await;

        assert_eq!(
            allowlist_audit_count(&state).await,
            1,
            "선언된 목록 밖의 호출은 durable하게 남아야 한다 — \
             막을 수는 없어도 일어났다는 사실은 남는다"
        );
    }

    /// **빈 목록은 미선언이지 deny-all이 아니다.**
    ///
    /// `tools`는 `#[serde(default)]`로 기본이 빈 벡터라 기존 템플릿 대부분이
    /// 여기 해당한다. deny-all로 읽으면 모든 호출이 위반으로 쏟아져 신호가
    /// 죽고, 그러면 이 통제 자체가 무의미해진다.
    #[tokio::test]
    async fn an_empty_allowlist_means_undeclared_not_deny_all() {
        let (state, dispatcher) = setup_fenced();
        let task = agent_with_tools(&state, vec![]).await;

        observe_tool(&dispatcher, task.id, WorkerId::new(), Some("bash")).await;

        assert_eq!(
            allowlist_audit_count(&state).await,
            0,
            "빈 목록을 deny-all로 읽으면 모든 호출이 위반이 된다"
        );
    }

    /// **이름을 모르면 판정하지 않는다.**
    ///
    /// SDK의 `unstable_tool_call_name`이 주지 않는 경우다. 모르는 것을 위반으로
    /// 접으면 거짓 양성이고, 통과로 세면 거짓 음성이다. 어느 쪽도 아니다.
    #[tokio::test]
    async fn a_tool_call_without_a_name_is_not_judged() {
        let (state, dispatcher) = setup_fenced();
        let task = agent_with_tools(&state, vec!["read".into()]).await;

        observe_tool(&dispatcher, task.id, WorkerId::new(), None).await;

        assert_eq!(
            allowlist_audit_count(&state).await,
            0,
            "이름 없는 호출을 위반으로 적으면 거짓 양성이다"
        );
    }

    /// **terminal로 확정된 뒤에 도착한 도구 호출도 기록된다**
    /// (로드맵 `#70` 게이트 4).
    ///
    /// 이 시험은 예전에 정반대를 단정했다(`..._is_dropped`). 그 근거는 "축을
    /// 모르면 지어내지 않는다"였고 그 원칙 자체는 지금도 옳지만, **축을 Task
    /// 상태에서 되찾으려 한 것이 틀렸다.** `current_worker_of`는
    /// `TaskStatus::Dispatched`만 매치했다. `Cancelled`에는 워커가 아예 없지만
    /// `Failed(TaskFailure)`는 `worker_id`를 들고 있는데도 보지 않았다 — 즉 그
    /// 구현은 "축을 모른다"가 아니라 **"Task가 끝났다"를 축의 부재로 오독**하고
    /// 있었고, 축을 아는 경우조차 버렸다.
    ///
    /// 결과는 조용한 증거 인멸이었다. `Failed(ResultLost)`의 정의가 "워커에서
    /// 아직 돌고 있을 수도 있다"인데, 정말로 돌고 있다는 **유일한 증거**가
    /// 정확히 그 순간부터 버려졌다 — 게이트 4가 존재하는 이유가 그 구간이다.
    ///
    /// 이제 축은 그 관측을 배달한 연결이 싣는다. 지어낸 값이 아니라 가장
    /// 권위 있는 출처의 값이므로 원래 원칙과 어긋나지 않는다.
    #[tokio::test]
    async fn a_tool_call_after_the_task_went_terminal_is_still_recorded() {
        let (state, dispatcher) = setup_fenced();

        let worker = fleet_core::Worker::new("w1", "wss://w1/ws");
        state.store.upsert_worker(&worker).await.unwrap();

        // 결과 유실로 **이미 확정된** Task. 이 상태에는 worker_id가 없다.
        let mut task = sample_task();
        let task_id = task.id;
        task.status = TaskStatus::Failed(fleet_core::TaskFailure {
            error: "worker went away".into(),
            kind: fleet_core::FailureKind::ResultLost,
            worker_id: Some(worker.id),
            attempts: 1,
        });
        state.store.insert_task(&task).await.unwrap();

        dispatcher
            .handle_worker_event(WorkerEvent::ToolCall {
                task_id,
                worker_id: worker.id,
                invocation: fleet_core::ToolInvocation {
                    tool_call_id: "tc-after-terminal".into(),
                    name: Some("bash".into()),
                    kind: fleet_core::ToolInvocationKind::Execute,
                    status: fleet_core::ToolInvocationStatus::Completed,
                },
            })
            .await;

        let events = state.store.list_events(0, 100).await.unwrap();
        let recorded = events
            .iter()
            .find(|e| e.event.event_type() == "task_tool_call")
            .unwrap_or_else(|| {
                panic!(
                    "확정 뒤에도 실행이 계속됐다는 증거가 남아야 한다 — 받은 것 {:?}",
                    events
                        .iter()
                        .map(|e| e.event.event_type())
                        .collect::<Vec<_>>()
                )
            });
        let payload = serde_json::to_string(&recorded.event).unwrap();
        assert!(
            payload.contains("tc-after-terminal"),
            "어느 호출인지 남아야 한다: {payload}"
        );
        assert!(
            payload.contains(&worker.id.to_string()),
            "축이 남아야 한다 — 배달한 연결의 것이지 Task 상태에서 추론한 것이 아니다: {payload}"
        );
    }

    // ── 로드맵 #62 2단계 게이트 3: 클라이언트 멱등성 ────────────────────
    //
    // `task_idempotency.rs`(fleet-store)는 **행이 중복되지 않음**을 증명한다.
    // 그러나 게이트가 실제로 요구하는 것은 **실행이 중복되지 않음**이다 —
    // 그 계약은 `submit()`의 조기 반환에만 존재한다. 아래 두 테스트가 없으면
    // 누군가 `IdempotentInsert` match를 `append_event`/워커 선택 **아래로**
    // 옮겨도 store 테스트는 전부 초록으로 남는다.
    //
    // 워커가 없는 상태(`setup_no_workers(0)`)를 쓰는 것이 의도적이다: 중복
    // 경로가 아래로 새면 `submit()`은 `NoWorker`로 실패하므로, 조기 반환이
    // 깨진 사실이 반환값만으로도 드러난다. 이벤트 수 assertion은 그보다 한
    // 단계 더 앞을 지킨다 — `append_event`는 워커 로직보다 먼저 실행되므로,
    // match를 그 사이로 옮기는 변경은 오직 이 assertion만이 잡는다.

    /// 같은 키·같은 payload로 재제출하면 최초 작업 id를 그대로 돌려주고,
    /// 이벤트를 쌓지 않으며, 두 번째 행을 만들지 않는다.
    #[tokio::test]
    async fn duplicate_submit_returns_the_original_without_dispatching() {
        use fleet_core::{TaskFilter, TaskResult, TaskStatus, WorkerId};

        let (state, dispatcher) = setup_no_workers(0);
        let request = TaskRequest {
            prompt: "build the thing".into(),
            created_by: "alice".into(),
            idempotency_key: Some("submit-once".into()),
            cwd: Some("/srv/fleet/workspaces/test".into()),
            ..Default::default()
        };

        // 최초 제출은 이미 성공했고 **완료까지 됐다**고 가정한다 — timeout 후
        // 같은 키로 재호출하는 것이 이 게이트가 막으려는 바로 그 시나리오이고,
        // 그 시점의 최초 제출은 대개 이미 끝나 있다.
        let first = Task::from_request(request.clone());
        let first_id = first.id;
        assert!(
            state
                .store
                .insert_task_idempotent(&first)
                .await
                .expect("first insert")
                .inserted(),
            "fixture 자체가 중복이면 테스트가 무의미하다"
        );
        state
            .store
            .update_task_status(
                first_id,
                &TaskStatus::Completed(TaskResult {
                    output: "done".into(),
                    exit_code: 0,
                    duration_secs: 1.0,
                    token_usage: None,
                    worker_id: WorkerId::new(),
                    finished_at: chrono::Utc::now(),
                }),
            )
            .await
            .expect("update_task_status");

        let events_before = state
            .store
            .list_events(0, 1000)
            .await
            .expect("events")
            .len();

        let retry = Task::from_request(request);
        let retry_id = retry.id;
        assert_ne!(retry_id, first_id, "재제출은 새 id를 들고 온다");

        let returned = dispatcher
            .submit(retry)
            .await
            .expect("중복 제출은 성공으로 반환되어야 한다 (워커가 없어도)");

        assert_eq!(
            returned, first_id,
            "호출자는 최초 작업 id를 받아야 한다 — 새로 만든 id가 아니라"
        );
        let events_after = state
            .store
            .list_events(0, 1000)
            .await
            .expect("events")
            .len();
        assert_eq!(
            events_before, events_after,
            "중복 제출은 이벤트를 하나도 쌓으면 안 된다 (TaskCreated는 일어나지 않은 일을 주장하게 된다)"
        );
        assert!(
            state
                .store
                .get_task(retry_id)
                .await
                .expect("get_task")
                .is_none(),
            "재제출용 id로는 행이 생기면 안 된다"
        );
        assert_eq!(
            state
                .store
                .list_tasks(&TaskFilter::default())
                .await
                .expect("list_tasks")
                .len(),
            1,
            "행은 여전히 하나여야 한다"
        );
    }

    /// 같은 키에 **다른** payload가 오면 거절하고, 행도 이벤트도 남기지 않는다.
    #[tokio::test]
    async fn conflicting_payload_submit_is_rejected_and_creates_nothing() {
        use fleet_core::TaskFilter;

        let (state, dispatcher) = setup_no_workers(0);
        let first = Task::from_request(TaskRequest {
            prompt: "build the thing".into(),
            created_by: "alice".into(),
            idempotency_key: Some("submit-once".into()),
            cwd: Some("/srv/fleet/workspaces/test".into()),
            ..Default::default()
        });
        let first_id = first.id;
        state
            .store
            .insert_task_idempotent(&first)
            .await
            .expect("first insert");

        let events_before = state
            .store
            .list_events(0, 1000)
            .await
            .expect("events")
            .len();

        let conflicting = Task::from_request(TaskRequest {
            prompt: "delete the thing".into(),
            created_by: "alice".into(),
            idempotency_key: Some("submit-once".into()),
            cwd: Some("/srv/fleet/workspaces/test".into()),
            ..Default::default()
        });
        let conflicting_id = conflicting.id;

        let err = dispatcher
            .submit(conflicting)
            .await
            .expect_err("같은 키에 다른 payload는 거절되어야 한다");
        match err {
            DispatchError::IdempotencyConflict {
                ref key,
                existing_task_id,
            } => {
                assert_eq!(key, "submit-once");
                assert_eq!(
                    existing_task_id, first_id,
                    "호출자가 충돌 상대를 찾아갈 수 있어야 한다"
                );
            }
            other => panic!("expected IdempotencyConflict, got {other:?}"),
        }

        assert_eq!(
            state
                .store
                .list_events(0, 1000)
                .await
                .expect("events")
                .len(),
            events_before,
            "거절된 제출은 이벤트를 남기면 안 된다"
        );
        assert!(
            state
                .store
                .get_task(conflicting_id)
                .await
                .expect("get_task")
                .is_none(),
            "거절된 제출은 행을 남기면 안 된다"
        );
        assert_eq!(
            state
                .store
                .list_tasks(&TaskFilter::default())
                .await
                .expect("list_tasks")
                .len(),
            1
        );
    }

    // ── 인접 결함 1: 관측의 범위를 실패 분류로 옮긴다 ──────────────────

    /// `WorkerEvent::Failed`의 `observation`이 저장되는 `FailureKind`를 정한다.
    ///
    /// 예전에는 이 자리가 `FailureKind::WorkerError` 상수였다. 그 이름의 doc은
    /// "워커에서 **실행 중** 발생한 에러"라고 주장하는데, transport의 여섯 생성
    /// 지점 중 둘(연결 상실 시의 `fail_all()`, `session/prompt` 타임아웃)은 워커가
    /// 실패를 보고한 적이 없다 — 작업은 그 순간에도 워커에서 돌고 있을 수 있다.
    /// 셋째(`session/new` 타임아웃)는 반대쪽으로 틀렸다: 프롬프트가 전달되지도
    /// 않았으니 워커 실행 실패가 아니라 워커 무응답이다.
    ///
    /// 표로 고정하는 이유는 생성 지점이 아니라 **매핑**이 계약이기 때문이다.
    /// transport에 새 실패 경로가 생겨도 셋 중 하나를 고르면 되고, 넷째가
    /// 필요해지면 이 테스트가 먼저 깨진다.
    #[tokio::test]
    async fn failure_observation_decides_the_persisted_failure_kind() {
        let cases = [
            (FailureObservation::Reported, FailureKind::WorkerError),
            (
                FailureObservation::NotDelivered,
                FailureKind::WorkerUnavailable,
            ),
            (FailureObservation::ResultLost, FailureKind::ResultLost),
        ];

        for (observation, expected) in cases {
            let (state, dispatcher) = setup_no_workers(0);
            let worker = fleet_core::Worker::new("w1", "wss://w1/ws");
            state.store.upsert_worker(&worker).await.unwrap();

            let mut task = sample_task();
            let task_id = task.id;
            task.status = TaskStatus::Dispatched {
                worker_id: worker.id,
                started_at: Utc::now(),
            };
            state.store.insert_task(&task).await.unwrap();

            dispatcher
                .handle_worker_event(WorkerEvent::Failed {
                    task_id,
                    error: "boom".into(),
                    observation,
                })
                .await;

            let stored = state.store.get_task(task_id).await.unwrap().unwrap();
            match stored.status {
                TaskStatus::Failed(failure) => assert_eq!(
                    failure.kind, expected,
                    "{observation:?} should persist as {expected:?}"
                ),
                other => panic!("{observation:?}: expected Failed, got {other:?}"),
            }
        }
    }

    /// 관측을 잃은 실패도 breaker에는 실패로 남는다.
    ///
    /// 분류를 가른 것은 **작업의 결말**이지 워커의 건강도가 아니다. 연결이
    /// 끊기거나 응답이 오지 않는 것은 그 워커에 대한 진짜 나쁜 신호이므로,
    /// `ResultLost`라고 해서 breaker 카운트에서 빼면 고장 난 워커로 계속
    /// 디스패치하게 된다. 샘플 1건에 회로가 열리도록 설정해 두고 breaker
    /// 레지스트리에서 관측한다 — `CircuitBreaker`에 카운터 접근자가 없고,
    /// 저장소 쪽은 관측 지점이 되지 못한다: `Store::update_worker_circuit_state`는
    /// 트레이트 기본 구현이 `Ok(())`이고 `MemStore`가 이를 재정의하지 않아
    /// 여기서는 쓰기가 조용히 사라진다.
    #[tokio::test]
    async fn result_lost_still_counts_as_a_breaker_failure() {
        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        let transport: Arc<dyn fleet_transport::WorkerTransport> =
            Arc::new(fleet_transport::MockTransport::new());
        let state = Arc::new(FleetState::new(
            store,
            transport,
            CircuitBreakerConfig {
                min_samples: 1,
                error_rate_threshold: 1.0,
                ..Default::default()
            },
        ));
        let dispatcher = Dispatcher::new(state.clone());

        let worker = fleet_core::Worker::new("w1", "wss://w1/ws");
        state.store.upsert_worker(&worker).await.unwrap();

        let mut task = sample_task();
        let task_id = task.id;
        task.status = TaskStatus::Dispatched {
            worker_id: worker.id,
            started_at: Utc::now(),
        };
        state.store.insert_task(&task).await.unwrap();

        dispatcher
            .handle_worker_event(WorkerEvent::Failed {
                task_id,
                error: "ACP connection lost — will reconnect".into(),
                observation: FailureObservation::ResultLost,
            })
            .await;

        assert_eq!(
            state.breakers.state_of(worker.id),
            crate::breaker::BreakerState::Open,
            "관측 상실도 워커 건강도 신호다 — breaker에서 빠지면 안 된다"
        );
    }

    // ── dispatch 전 inventory-first recovery (로드맵 `#70` 게이트 2) ──────
    //
    // 게이트 문언은 복구가 신규 dispatch보다 **먼저**일 것을 요구한다. 아래
    // 넷이 그 "먼저"를 네 방향에서 고정한다: 실제로 먼저 일어나는가, 세대당
    // 한 번인가, 못 물어보면 막히는가, 그리고 **오늘의 배포를 막지 않는가**.

    /// 한 워커를 store와 transport 양쪽에 올리고 dispatcher를 조립한다.
    async fn setup_one_registered_worker() -> (
        Arc<FleetState>,
        Arc<Dispatcher>,
        Arc<fleet_transport::MockTransport>,
        fleet_core::Worker,
    ) {
        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        let mut worker = fleet_core::Worker::new("w-recovery", "wss://w-recovery/ws");
        worker.status = fleet_core::WorkerStatus::Online;
        store.upsert_worker(&worker).await.unwrap();

        let mock = Arc::new(fleet_transport::MockTransport::new());
        mock.add_worker(fleet_transport::MockWorker::new(
            worker.id,
            worker.endpoint.clone(),
        ))
        .await;
        let transport: Arc<dyn fleet_transport::WorkerTransport> = mock.clone();
        let state = Arc::new(FleetState::new(
            store,
            transport,
            CircuitBreakerConfig::default(),
        ));
        let dispatcher = Arc::new(Dispatcher::new(state.clone()));
        (state, dispatcher, mock, worker)
    }

    /// **복구가 dispatch보다 먼저 일어난다.**
    ///
    /// 재시작한 오케스트레이터를 재현한다: 저장소에는 이 워커에 배정된
    /// `Dispatched` Task가 남아 있고, 워커의 인벤토리에는 그 세션이 없다.
    /// 새 Task를 제출하면, 그 dispatch가 나가기 **전에** 먼저 그 어긋남이
    /// 정리돼 있어야 한다.
    #[tokio::test]
    async fn a_worker_is_recovered_before_it_receives_new_work() {
        let (state, dispatcher, mock, worker) = setup_one_registered_worker().await;

        // 재시작 전에 이 워커로 나갔다고 믿는 Task.
        let mut stale = sample_task();
        stale.status = TaskStatus::Dispatched {
            worker_id: worker.id,
            started_at: Utc::now() - chrono::Duration::seconds(300),
        };
        stale.acp_session_id = Some("sess-gone".into());
        let stale_id = stale.id;
        state.store.insert_task(&stale).await.unwrap();

        // 워커는 그 세션을 들고 있지 않다(다른 것 하나를 들고 있다 — 빈
        // 목록으로 두면 "목록을 읽었는가"와 "비어 있었는가"가 섞인다).
        mock.set_session_inventory(
            worker.id,
            fleet_transport::SessionInventory::Reported(vec!["sess-other".into()]),
        )
        .await;

        let fresh = sample_task();
        state.store.insert_task(&fresh).await.unwrap();
        dispatcher
            .dispatch_existing(fresh, false)
            .await
            .expect("dispatch");

        let recovered = state.store.get_task(stale_id).await.unwrap().unwrap();
        match recovered.status {
            TaskStatus::Failed(f) => assert_eq!(f.kind, fleet_core::FailureKind::ExecutionVanished),
            other => panic!(
                "새 dispatch가 나가기 전에 어긋난 Task가 정리됐어야 한다 — 받은 값 {other:?}"
            ),
        }

        // 순서를 사후에 재구성할 수 있는 기록이 남아야 한다.
        let e = audited(
            state.store.as_ref(),
            fleet_core::audit::action::CONTROL_WORKER_RECOVERED,
        )
        .await;
        assert_eq!(e.target_id.as_deref(), Some(worker.id.to_string().as_str()));
    }

    /// 세대당 **한 번만** 묻는다. 매 dispatch마다 물으면 왕복이 하나 붙고,
    /// 그것은 이 게이트가 요구한 적 없는 비용이다.
    #[tokio::test]
    async fn the_inventory_is_read_once_per_control_generation() {
        let (state, dispatcher, mock, worker) = setup_one_registered_worker().await;
        mock.set_session_inventory(
            worker.id,
            fleet_transport::SessionInventory::Reported(Vec::new()),
        )
        .await;

        for _ in 0..3 {
            let t = sample_task();
            state.store.insert_task(&t).await.unwrap();
            dispatcher
                .dispatch_existing(t, false)
                .await
                .expect("dispatch");
        }

        let recoveries = state
            .store
            .list_audit_events(&fleet_core::AuditFilter {
                action: Some(fleet_core::audit::action::CONTROL_WORKER_RECOVERED.to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            recoveries.len(),
            1,
            "세 번 dispatch했는데 복구가 {}번 일어났다",
            recoveries.len()
        );
    }

    /// **물어보지 못하면 보내지 않는다.** 이 단정이 없으면 게이트가 사라진다 —
    /// 조회 실패를 통과시키는 구현은 "먼저 물어본다"를 지키지 않으면서도
    /// 위의 두 시험을 통과한다.
    #[tokio::test]
    async fn a_worker_we_cannot_ask_does_not_receive_work() {
        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        let mut worker = fleet_core::Worker::new("w-unreachable", "wss://w-unreachable/ws");
        worker.status = fleet_core::WorkerStatus::Online;
        store.upsert_worker(&worker).await.unwrap();

        // store에만 있고 transport에는 없다 — 재연결되지 않은 워커의 모양이다.
        let transport: Arc<dyn fleet_transport::WorkerTransport> =
            Arc::new(fleet_transport::MockTransport::new());
        let state = Arc::new(FleetState::new(
            store,
            transport,
            CircuitBreakerConfig::default(),
        ));
        let dispatcher = Dispatcher::new(state.clone());

        let t = sample_task();
        state.store.insert_task(&t).await.unwrap();
        let err = dispatcher
            .dispatch_existing(t, false)
            .await
            .expect_err("물어보지 못한 워커로는 보내면 안 된다");
        assert!(
            matches!(err, DispatchError::RecoveryUnavailable(_)),
            "받은 값 {err:?}"
        );
    }

    /// **인벤토리를 광고하지 않는 워커에게도 일은 나간다.**
    ///
    /// 이 부정 단정이 없으면 게이트가 오늘의 거의 모든 배포에서 dispatch를
    /// 영구히 막는다 — 실제 Agent 다수가 `session/list`를 광고하지 않는다.
    /// 통과 조건은 "물어봤다"이지 "답을 받았다"가 아니다.
    #[tokio::test]
    async fn an_undeclared_worker_is_still_dispatchable() {
        let (state, dispatcher, _mock, _worker) = setup_one_registered_worker().await;
        // 인벤토리를 설정하지 않는다 — MockTransport의 기본값이 `Undeclared`다.

        let t = sample_task();
        let task_id = t.id;
        state.store.insert_task(&t).await.unwrap();
        dispatcher
            .dispatch_existing(t, false)
            .await
            .expect("광고하지 않는 워커에게도 dispatch는 나가야 한다");

        let stored = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(stored.status, TaskStatus::Dispatched { .. }));

        let e = audited(
            state.store.as_ref(),
            fleet_core::audit::action::CONTROL_WORKER_RECOVERED,
        )
        .await;
        assert_eq!(
            e.detail.get("inventory").and_then(|v| v.as_str()),
            Some("undeclared"),
            "무엇을 근거로 통과시켰는지가 기록에 남아야 한다 — 이 fleet에서 \
             게이트가 실질적으로 비어 있다는 사실이 그 기록이다"
        );
    }

    // ── 필수 Skill 누락 거절 (로드맵 `#65` 게이트 1) ──────────────────────
    //
    // 정본([하네스 구성](../../docs/architecture/agents/harness-composition.md))은
    // "필수 Skill은 시작 전에 고정하고 **누락 시 실행을 거절한다**"고 적는다.
    // 예전 구현은 정반대였다 — 없으면 `warn!` 한 줄 찍고 건너뛰어, Task가
    // 선언한 Skill 없이 그대로 실행됐다. `skills_required`라는 이름이 강제되지
    // 않는 장식이었다.
    //
    // 두 시험 모두 `FLEET_SKILLS_DIR`을 쓴다. env var는 프로세스 전역이라
    // 병렬 실행에서 서로를 덮는데, CI와 로컬 게이트가 둘 다
    // `--test-threads=1`을 쓰므로(§agent.md 3.2) 이 파일 안에서는 안전하다.

    #[tokio::test]
    async fn a_task_missing_its_required_skill_is_never_dispatched() {
        let skills = tempfile::TempDir::new().unwrap();
        std::env::set_var("FLEET_SKILLS_DIR", skills.path());

        let (state, dispatcher, mock, _worker) = setup_one_registered_worker().await;
        let mut task = sample_task();
        task.skills_required = vec!["security-audit".into()];
        let task_id = task.id;
        state.store.insert_task(&task).await.unwrap();

        let err = dispatcher
            .dispatch_existing(task, false)
            .await
            .expect_err("선언된 Skill이 없으면 실행이 나가면 안 된다");
        match err {
            DispatchError::SkillMissing { missing, .. } => {
                assert_eq!(missing, vec!["security-audit".to_string()])
            }
            other => panic!("기대와 다름: {other:?}"),
        }

        // **워커에 아무것도 가지 않았다**는 것이 이 시험의 핵심이다.
        assert!(
            mock.dispatch_requests().await.is_empty(),
            "스킬 없이 실행이 나갔다 — 거절이 성립하지 않은 것이다"
        );
        // 그리고 Task는 여전히 `Pending`이다. `Dispatched`로 옮긴 뒤 거절하면
        // 아무도 소유하지 않는 실행 상태가 남는다.
        let stored = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(stored.status, TaskStatus::Pending),
            "받은 값 {:?}",
            stored.status
        );

        std::env::remove_var("FLEET_SKILLS_DIR");
    }

    /// 대조군. 위 시험만 있으면 **모든** dispatch를 거절하는 구현도 통과한다.
    /// 여기서는 스킬이 실제로 프롬프트에 실려 나갔는지까지 본다.
    #[tokio::test]
    async fn a_task_whose_skill_is_present_is_dispatched_with_it() {
        let skills = tempfile::TempDir::new().unwrap();
        std::fs::write(
            skills.path().join("security-audit.md"),
            "You are a security auditor.",
        )
        .unwrap();
        std::env::set_var("FLEET_SKILLS_DIR", skills.path());

        let (state, dispatcher, mock, _worker) = setup_one_registered_worker().await;
        let mut task = sample_task();
        task.skills_required = vec!["security-audit".into()];
        let task_id = task.id;
        state.store.insert_task(&task).await.unwrap();

        dispatcher
            .dispatch_existing(task, false)
            .await
            .expect("스킬이 있으면 dispatch는 나가야 한다");

        let sent = mock.dispatch_requests().await;
        assert_eq!(sent.len(), 1);
        assert!(
            sent[0].prompt.contains("<SKILL: security-audit>")
                && sent[0].prompt.contains("You are a security auditor."),
            "조립된 프롬프트에 스킬 본문이 없다 — 받은 값 {:?}",
            sent[0].prompt
        );

        let stored = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(stored.status, TaskStatus::Dispatched { .. }));

        std::env::remove_var("FLEET_SKILLS_DIR");
    }

    /// 거절이 **워커의 회로를 열지 않는다.**
    ///
    /// 스킬이 없는 것은 오케스트레이터 쪽 사실이고 워커는 이 요청을 본 적이
    /// 없다. 여기서 브레이커에 기록하면 스킬 하나가 빠진 것만으로 멀쩡한
    /// 워커가 후보에서 빠지고, 클라이언트가 그 Task를 반복 제출하는 것만으로
    /// 회로를 열 수 있다 — `#69`가 `InvalidRequest`를 면제한 것과 같은 이유다.
    #[tokio::test]
    async fn a_missing_skill_does_not_open_the_workers_circuit() {
        let skills = tempfile::TempDir::new().unwrap();
        std::env::set_var("FLEET_SKILLS_DIR", skills.path());

        let (state, dispatcher, _mock, worker) = setup_one_registered_worker().await;
        for _ in 0..10 {
            let mut task = sample_task();
            task.skills_required = vec!["absent".into()];
            state.store.insert_task(&task).await.unwrap();
            let _ = dispatcher.dispatch_existing(task, false).await;
        }

        assert!(
            !circuit_opened(state.store.as_ref(), worker.id).await,
            "요청 쪽 결함이 워커의 건강도로 기록됐다"
        );

        std::env::remove_var("FLEET_SKILLS_DIR");
    }
}
