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
//! JoinHandle 기반 abort" 패턴을 따르는 백그라운드 루프로, 매 tick마다 세 가지를
//! 스윕한다: `stale_after`보다 오래 `Pending`으로 머문 작업의 재dispatch,
//! `dispatched_worker_check_after`보다 오래됐는데 담당 워커가 더 이상 존재하지
//! 않는 `Dispatched` 작업의 `Failed` 전이, 그리고 담당 워커가 `offline_worker_grace`
//! 이상 `Offline` 상태로 남아있는 `Dispatched` 작업의 `Failed` 전이.
//!
//! ## 세 번째 스윕: HealthChecker↔Task 연동 (2026-08-13 추가)
//!
//! [`HealthChecker`](crate::health::HealthChecker)는 워커가 45초(3회 하트비트
//! 누락) 동안 응답이 없으면 `Worker.status`를 `Offline`으로 바꾸지만, **Task
//! 테이블은 전혀 건드리지 않는다.** 그 결과, 워커가 하트비트만 끊기고(예: 헬스체크
//! 경로만 막힌 네트워크 파티션) ACP WebSocket 연결 자체는 살아있는 애매한 상태라면
//! — `fail_all()`(연결 끊김 감지)도, 프롬프트 타임아웃(기본 10분)도, 아래 첫 번째
//! 스윕(워커 row 자체가 사라진 경우)도 발동하지 않아 — 그 워커에 배정된
//! `Dispatched` 작업이 **영원히 끝나지 않을 수 있었다.**
//!
//! 이 스윕은 그 빈틈을 메운다: 담당 워커가 여전히 `workers` 테이블에 존재하되
//! `status == Offline`이고, 마지막 하트비트(`last_seen`)로부터
//! `offline_worker_grace`(기본 5분) 이상 지났다면 `Failed(WorkerUnavailable)`로
//! 전이한다. **의도적으로 45초(HealthChecker의 Offline 판정 기준)보다 훨씬 긴
//! 유예를 둔다** — `Offline`은 (row 삭제와 달리) 되돌릴 수 있는 상태라, 워커가
//! 곧 재연결될 수 있는 상황에서 성급하게 작업을 실패 처리하고 싶지 않기 때문이다.
//! 이 스윕도 워커가 실제로는 재연결에 성공했는데 뒤늦게 `WorkerEvent::Completed`가
//! 도착해 이미 `Failed`로 마킹된 작업의 상태를 다시 덮어쓰는 이론적 경쟁 상태를
//! 완전히 막지는 못한다 — `update_task_status`가 현재 상태를 조건으로 거는
//! 낙관적 잠금을 하지 않기 때문. 다만 5분이라는 유예 자체가 이 경쟁이 실제로
//! 발생할 창을 매우 좁게 만든다.
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
//!   경합하면 되기 때문이다.
//! - `offline_worker_grace`(기본 300초 = 5분)는 위 세 번째 스윕 전용이며,
//!   워커가 여전히 등록돼 있고 단순히 응답이 느릴 뿐인 흔한 경우(대부분은 수 초
//!   ~수십 초 내 회복)와, 정말로 죽었거나 네트워크가 갈라진 경우를 구분하기
//!   위한 훨씬 보수적인 유예 시간이다.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use fleet_core::{
    FailureKind, TaskFailure, TaskFilter, TaskStatus, TaskStatusFilter, WorkerStatus,
};

use crate::dispatcher::{DispatchError, Dispatcher};
use crate::selector::SelectionError;
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
    /// 담당 워커가 `workers` 테이블에는 여전히 존재하지만 `status == Offline`이고
    /// 마지막 하트비트(`last_seen`)로부터 이 시간 이상 지난 `Dispatched` 작업을
    /// `Failed`로 전이한다. `Offline`은 되돌릴 수 있는 상태라 `dispatched_worker_
    /// check_after`보다 훨씬 보수적으로(길게) 잡는다 — 기본값 5분.
    pub offline_worker_grace: Duration,
    /// stale `Pending` 작업을 최대 몇 번까지 재dispatch 시도할지 (로드맵 #38).
    /// `Task.retry_count`가 이 값에 도달하면 더 이상 재시도하지 않고
    /// `Failed(WorkerUnavailable)`로 전이시킨다(dead-letter). `0`이면 재시도
    /// 없이 기존 동작과 동일 — stale해질 때마다 무기한 재시도(이 필드 도입
    /// 이전의 기존 동작).
    ///
    /// 기본값 20 x 기본 interval(30초) 는 최초 stale_after(60초) 유예 이후
    /// 약 10분간 재시도하다가 포기한다는 뜻이다 - "네트워크 일시 순단"을
    /// 흡수하기엔 충분하고, 영구적으로 워커가 없는 상황을 무기한 Pending으로
    /// 방치하지도 않는 절충값.
    pub max_dispatch_retries: u32,
}

impl Default for ReconcileConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            stale_after: Duration::from_secs(60),
            dispatched_worker_check_after: Duration::from_secs(30),
            offline_worker_grace: Duration::from_secs(300),
            max_dispatch_retries: 20,
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
    /// 담당 워커가 `Offline`으로 장기간 남아있어 발견된 stale dispatched 작업 수.
    pub offline_worker_found: u64,
    /// 이번 라운드에 Failed로 전이시킨, offline 워커 배정 작업 수.
    pub offline_worker_failed: u64,
    /// `retry_count`가 `max_dispatch_retries`에 도달해 재시도를 포기하고
    /// dead-letter(`Failed`)로 전이시킨 작업 수 (로드맵 #38).
    pub dead_lettered: u64,
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
            offline_worker_grace = ?self.config.offline_worker_grace,
            "task reconciliation loop started (pending redispatch + orphaned/offline dispatched reap)"
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

            // DAG 체이닝: 미완료 선행 작업이 하나라도 있다면 Reconciler도 재배포하지 않고 건너뜀
            let mut has_unresolved_dependencies = false;
            for dep_id in &task.dependency_ids {
                if let Ok(Some(dep_task)) = self.state.store.get_task(*dep_id).await {
                    if !matches!(dep_task.status, fleet_core::TaskStatus::Completed(_)) {
                        has_unresolved_dependencies = true;
                        break;
                    }
                } else {
                    has_unresolved_dependencies = true;
                    break;
                }
            }
            if has_unresolved_dependencies {
                continue;
            }

            summary.stale_found += 1;
            let task_id = task.id;

            // 로드맵 #38: `max_dispatch_retries > 0`이고 이미 그만큼 재시도
            // (submit()의 최초 시도 포함)했다면 더 이상 dispatch를 시도하지
            // 않고 dead-letter(`Failed`)로 전이시킨다 — 무기한 Pending 방치를
            // 방지한다. `max_dispatch_retries == 0`이면 이 필드 도입 이전과
            // 동일하게 무제한 재시도한다.
            if self.config.max_dispatch_retries > 0
                && task.retry_count >= self.config.max_dispatch_retries
            {
                let retry_count = task.retry_count;
                // dead-letter 원인을 분류하기 위해 선택 로직을 한 번 더
                // (부작용 없는 순수 조회로) 돌려본다 — 로드맵 #71. 이 시점까지
                // 재시도가 소진됐다는 건 이전 사이클들에서도 계속 같은 이유로
                // 실패해왔다는 뜻이므로, 지금 다시 물어봐도 같은 분류가 나올
                // 가능성이 매우 높다. credential 부재가 지속적인 원인이면
                // `CredentialMissing`으로 구분해 재프로비저닝이 필요함을
                // 모니터링/대시보드에서 바로 알 수 있게 한다.
                let kind = match self.state.selector.select(&task).await {
                    Err(SelectionError::NoWorkerForCredential(_)) => FailureKind::CredentialMissing,
                    _ => FailureKind::WorkerUnavailable,
                };
                let failure = TaskFailure {
                    error: format!("dispatch retries exhausted ({retry_count} attempts)"),
                    kind,
                    worker_id: None,
                    attempts: retry_count,
                };
                self.dispatcher.mark_failed(task_id, failure).await;
                summary.dead_lettered += 1;
                warn!(
                    %task_id, retry_count, ?kind,
                    "reconcile: dispatch retries exhausted, dead-lettering task"
                );
                continue;
            }

            // `false` — 선택 실패/CircuitOpen을 실패로 마킹하지 않고 Pending
            // 상태를 유지, 다음 tick에서 재시도한다.
            match self.dispatcher.dispatch_existing(task, false).await {
                Ok(()) => {
                    summary.redispatched += 1;
                    info!(%task_id, "reconciliation redispatched a stale pending task");
                }
                Err(DispatchError::NoWorker(reason)) => {
                    debug!(%task_id, %reason, "reconcile: still no capacity, leaving pending");
                    if self.config.max_dispatch_retries > 0 {
                        let _ = self.state.store.increment_task_retry_count(task_id).await;
                    }
                }
                Err(DispatchError::CircuitOpen(worker_id)) => {
                    debug!(
                        %task_id, %worker_id,
                        "reconcile: selected worker's circuit is open, leaving pending"
                    );
                    if self.config.max_dispatch_retries > 0 {
                        let _ = self.state.store.increment_task_retry_count(task_id).await;
                    }
                }
                Err(e) => {
                    // 진짜 dispatch 에러(transport 실패 등) — dispatch_existing이
                    // 이미 task를 Failed로 마킹했으므로 여기서는 로깅만 한다.
                    warn!(%task_id, error = %e, "reconcile: dispatch attempt failed");
                }
            }
        }

        self.reap_stale_dispatched(&mut summary).await;

        if summary.stale_found > 0 || summary.orphaned_found > 0 || summary.offline_worker_found > 0
        {
            info!(
                stale_found = summary.stale_found,
                redispatched = summary.redispatched,
                orphaned_found = summary.orphaned_found,
                orphaned_failed = summary.orphaned_failed,
                offline_worker_found = summary.offline_worker_found,
                offline_worker_failed = summary.offline_worker_failed,
                dead_lettered = summary.dead_lettered,
                "reconciliation sweep completed"
            );
        }

        summary
    }

    /// `Dispatched` 작업 중 (a) 담당 워커가 store에서 완전히 사라졌거나,
    /// (b) 담당 워커가 여전히 존재하지만 `Offline`으로 `offline_worker_grace`
    /// 이상 남아있는 것을 찾아 `Failed(WorkerUnavailable)`로 전이한다.
    /// `summary`에 결과를 누적한다.
    async fn reap_stale_dispatched(&self, summary: &mut ReconcileSummary) {
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
        let offline_grace = chrono::Duration::from_std(self.config.offline_worker_grace)
            .unwrap_or_else(|_| chrono::Duration::seconds(300));

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

            let worker = match self.state.store.get_worker(worker_id).await {
                Ok(w) => w,
                Err(e) => {
                    // 조회 자체가 실패하면 판단할 수 없으므로 건드리지 않고 다음 tick에서 재시도.
                    warn!(%worker_id, error = %e, "reconcile: failed to check worker existence, skipping");
                    continue;
                }
            };

            let task_id = task.id;

            match worker {
                None => {
                    // (a) 워커 row 자체가 사라짐 — 재시작으로 새 worker_id를 받은
                    // 경우가 대표적. 강한 신호이므로 짧은 유예(check_after)만 둔다.
                    summary.orphaned_found += 1;
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
                Some(w) if w.status == WorkerStatus::Offline => {
                    // (b) 워커는 존재하지만 Offline — 되돌릴 수 있는 상태이므로
                    // 훨씬 긴 유예(offline_worker_grace)를 마지막 하트비트 기준으로 적용.
                    // `last_seen`이 `None`(한 번도 heartbeat를 받은 적 없음)이면 유예를
                    // 줄 근거가 없으므로 즉시 대상으로 취급한다.
                    let past_grace = match w.last_seen {
                        Some(ls) => now - ls >= offline_grace,
                        None => true,
                    };
                    if !past_grace {
                        continue; // 아직 유예 기간 내 — 재연결을 기다린다.
                    }
                    let offline_for_desc = match w.last_seen {
                        Some(ls) => format!("{}s", (now - ls).num_seconds()),
                        None => "never (no heartbeat ever received)".to_string(),
                    };

                    summary.offline_worker_found += 1;
                    let failure = TaskFailure {
                        error: format!(
                            "assigned worker {worker_id} has been offline for {offline_for_desc} \
                             (no heartbeat) — assuming the task is lost"
                        ),
                        kind: FailureKind::WorkerUnavailable,
                        worker_id: Some(worker_id),
                        attempts: 0,
                    };
                    self.dispatcher.mark_failed(task_id, failure).await;
                    summary.offline_worker_failed += 1;
                    warn!(
                        %task_id, %worker_id, offline_for = %offline_for_desc,
                        "reconciliation: dispatched task's worker offline too long, marked failed"
                    );
                }
                Some(_) => {
                    // 워커는 존재하고 Offline이 아님(Online/Degraded/CircuitOpen) —
                    // 응답이 느릴 뿐이면 헬스체크/CircuitBreaker가 담당하는 영역이므로
                    // 건드리지 않는다.
                    continue;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::Dispatcher;
    use crate::state::FleetState;
    use fleet_core::{
        CircuitBreakerConfig, Task, TaskId, TaskRequest, TaskStatus, Worker, WorkerId, WorkerStatus,
    };
    use fleet_store::mem::MemStore;
    use fleet_store::Store;
    use fleet_transport::{MockTransport, MockWorker};

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
                offline_worker_grace: Duration::from_secs(300),
                max_dispatch_retries: 20,
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
                offline_worker_grace: Duration::from_secs(300),
                max_dispatch_retries: 20,
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
    async fn stale_pending_task_dead_letters_after_max_retries_exhausted() {
        // 로드맵 #38 — retry_count가 max_dispatch_retries에 도달한 stale
        // Pending 작업은 더 이상 재시도하지 않고 Failed(dead-letter)로 전이.
        let store = Arc::new(MemStore::new());
        let mut task = make_pending_task("retries exhausted", chrono::Duration::seconds(120));
        let task_id = task.id;
        task.retry_count = 3;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;

        let reconciler = Reconciler::new(
            state.clone(),
            dispatcher,
            ReconcileConfig {
                interval: Duration::from_secs(3600),
                stale_after: Duration::from_secs(60),
                dispatched_worker_check_after: Duration::from_secs(30),
                offline_worker_grace: Duration::from_secs(300),
                max_dispatch_retries: 3,
            },
        );

        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary.stale_found, 1);
        assert_eq!(summary.redispatched, 0);
        assert_eq!(summary.dead_lettered, 1);

        let failed = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(failed.status, TaskStatus::Failed(_)),
            "task should be dead-lettered as Failed: {:?}",
            failed.status
        );
    }

    #[tokio::test]
    async fn stale_pending_task_dead_letters_as_credential_missing_when_no_worker_has_credential() {
        // 로드맵 #71 — worker는 온라인이고 model 라벨도 일치하지만 그 model의
        // credential을 아무도 보유하지 않은 경우: 재시도를 계속 소진해도
        // 해소되지 않으므로(정적인 원인), dead-letter는 일반
        // `WorkerUnavailable`이 아니라 `FailureKind::CredentialMissing`으로
        // 구분되어야 한다.
        let store = Arc::new(MemStore::new());
        let mut worker = Worker::new("gemini-1", "wss://gemini-1/ws");
        worker.status = WorkerStatus::Online;
        worker.labels.insert("model".into(), "gemini".into());
        store.upsert_worker(&worker).await.unwrap();
        // 의도적으로 credential을 프로비저닝하지 않는다.

        let mut task = make_pending_task("credential-less work", chrono::Duration::seconds(120));
        task.model = Some("gemini".into());
        task.retry_count = 3;
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
                offline_worker_grace: Duration::from_secs(300),
                max_dispatch_retries: 3,
            },
        );

        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary.stale_found, 1);
        assert_eq!(summary.dead_lettered, 1);

        let failed = state.store.get_task(task_id).await.unwrap().unwrap();
        match failed.status {
            TaskStatus::Failed(f) => assert_eq!(
                f.kind,
                fleet_core::FailureKind::CredentialMissing,
                "expected CredentialMissing, got {:?}",
                f.kind
            ),
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn stale_pending_task_under_retry_limit_still_gets_redispatch_attempt() {
        // retry_count가 max_dispatch_retries 미만이면 여전히 정상적으로
        // dispatch_existing()을 시도한다 (dead-letter 분기를 타지 않음).
        let worker = make_worker("idle-1");
        let worker_id = worker.id;

        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let mut task = make_pending_task("still retrying", chrono::Duration::seconds(120));
        let task_id = task.id;
        task.retry_count = 2;
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
                offline_worker_grace: Duration::from_secs(300),
                max_dispatch_retries: 3,
            },
        );

        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary.stale_found, 1);
        assert_eq!(summary.redispatched, 1);
        assert_eq!(summary.dead_lettered, 0);

        let completed = wait_until_terminal(state.store.as_ref(), task_id).await;
        match completed.status {
            TaskStatus::Completed(result) => assert_eq!(result.worker_id, worker_id),
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reconcile_once_tolerates_store_errors_without_panicking() {
        let store = Arc::new(MemStore::new().with_failing(&["list_tasks"]));
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
                offline_worker_grace: Duration::from_secs(300),
                max_dispatch_retries: 20,
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

    /// `Offline` 워커를 생성하며 `last_seen`을 지정된 나이(age)로 설정.
    fn make_offline_worker(name: &str, last_seen_age: chrono::Duration) -> Worker {
        let mut w = Worker::new(name, format!("wss://{name}/ws"));
        w.status = WorkerStatus::Offline;
        w.last_seen = Some(chrono::Utc::now() - last_seen_age);
        w
    }

    #[tokio::test]
    async fn dispatched_task_on_long_offline_worker_is_marked_failed() {
        // HealthChecker↔Task 연동 빈틈 재현: 워커가 존재하고 Offline 상태이며
        // 마지막 하트비트로부터 offline_worker_grace(여기선 5초로 줄임) 이상
        // 지났다면, 담당 Dispatched 작업은 Failed로 전이돼야 한다.
        let worker = make_offline_worker("ghost-but-registered", chrono::Duration::seconds(10));
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task("stuck task", worker_id, chrono::Duration::seconds(120));
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
                offline_worker_grace: Duration::from_secs(5), // 짧게 — 테스트용
                max_dispatch_retries: 20,
            },
        );

        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary.offline_worker_found, 1);
        assert_eq!(summary.offline_worker_failed, 1);
        assert_eq!(
            summary.orphaned_found, 0,
            "worker still exists — not the orphan path"
        );

        let failed = state.store.get_task(task_id).await.unwrap().unwrap();
        match failed.status {
            TaskStatus::Failed(failure) => {
                assert_eq!(failure.kind, FailureKind::WorkerUnavailable);
                assert_eq!(failure.worker_id, Some(worker_id));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatched_task_on_recently_offline_worker_stays_dispatched_within_grace() {
        // 워커가 Offline이 된 지 얼마 안 됐다면(offline_worker_grace 이내) —
        // 곧 재연결될 수 있으므로 성급하게 Failed로 전이하면 안 된다.
        let worker = make_offline_worker("just-went-offline", chrono::Duration::seconds(2));
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task(
            "still maybe running",
            worker_id,
            chrono::Duration::seconds(120),
        );
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
                offline_worker_grace: Duration::from_secs(300), // 기본값 — 2초는 한참 못 미침
                max_dispatch_retries: 20,
            },
        );

        let summary = reconciler.reconcile_once().await;
        assert_eq!(
            summary.offline_worker_found, 0,
            "worker offline for only 2s should still be within the 300s grace period"
        );

        let still_dispatched = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(
            still_dispatched.status,
            TaskStatus::Dispatched { .. }
        ));
    }

    #[tokio::test]
    async fn dispatched_task_on_degraded_worker_is_left_alone() {
        // Degraded(온라인이지만 저하됨)는 Offline이 아니므로 이 스윕이 건드리면
        // 안 된다 — 헬스체크/CircuitBreaker의 영역.
        let mut worker = make_worker("degraded-1");
        worker.status = WorkerStatus::Degraded;
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task(
            "degraded but alive",
            worker_id,
            chrono::Duration::seconds(120),
        );
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;
        let reconciler = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default());

        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary.offline_worker_found, 0);
        assert_eq!(summary.orphaned_found, 0);

        let still_dispatched = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(
            still_dispatched.status,
            TaskStatus::Dispatched { .. }
        ));
    }
}
