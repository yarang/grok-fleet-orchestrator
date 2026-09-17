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
//! 워커가 실제로는 재연결에 성공했는데 뒤늦게 `WorkerEvent::Completed`가 도착해
//! 이미 `Failed`로 마킹된 작업을 다시 덮어쓰는 경쟁 상태는, 유예 시간이 아니라
//! [`Store::compare_and_set_task_status`](fleet_store::Store::compare_and_set_task_status)가
//! 막는다(로드맵 `#62`). 이 스윕은 `[Dispatched]`를 기대 위상으로 넘기고, 늦게
//! 도착한 완료 이벤트도 마찬가지로 `[Dispatched]`를 기대하므로, 둘 중 먼저
//! 도착한 쪽만 상태를 옮기고 나머지는 거절된다. 5분의 유예는 이제 정합성의
//! 근거가 아니라 **되돌릴 수 있는 `Offline` 상태에서 성급하게 실패 처리하지
//! 않기 위한 정책**으로만 남는다.
//!
//! 다만 CAS가 닫는 것은 "확정된 상태를 덮어쓰는 것"까지다. 워커에서 실제로
//! 완료된 작업이 여기서 `Failed`로 확정됐다면 그 결과물은 여전히 버려진다 —
//! 상태는 일관되지만 일은 낭비된다. 이를 줄이는 것은 유예 시간 조정의 몫이다.
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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use fleet_core::{
    FailureKind, TaskFailure, TaskFilter, TaskPhase, TaskStatus, TaskStatusFilter,
    TransitionOrigin, WorkerId, WorkerStatus,
};
use fleet_transport::SessionInventory;

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
    /// 워커에게 `session/list`를 물을 때의 응답 대기 시간
    /// (로드맵 `#70` 게이트 2).
    ///
    /// 답이 늦으면 인벤토리 없이 그 워커를 지나친다 — 인벤토리는 회수를
    /// **더 하기** 위한 근거이지 덜 하기 위한 것이 아니므로, 못 물어봤다고
    /// 다른 판정을 미루지 않는다.
    pub session_list_timeout: Duration,
    /// 명령이 이 시간을 넘도록 확인되지 않은 Agent를 감사에 남긴다
    /// (로드맵 `#70` 게이트 3 — ACK 유실).
    ///
    /// heartbeat 주기보다 충분히 길게 잡아야 한다 — 정상적으로 다음 beat을
    /// 기다리는 중인 Agent를 미확인으로 부르면 그 기록이 소음이 된다.
    /// 기본값 5분은 `offline_worker_grace`와 같은 값이며, 같은 질문("이
    /// Worker가 살아 있다고 볼 수 있는가")을 다른 각도에서 재기 때문이다.
    pub command_ack_timeout: Duration,
    /// 워커가 들고 있는 세션 중 이미 끝난 Task의 것을 찾아 취소할지
    /// (로드맵 `#70` 게이트 2·7). 기본값 `true`.
    ///
    /// 끌 수 있게 둔 이유는 이것이 워커의 실행을 **멈추는** 유일한 자동
    /// 경로이기 때문이다. 인벤토리를 광고하지 않는 배포에서는 어차피 한 건도
    /// 일어나지 않지만, 광고하는 배포에서 예상 밖의 취소가 보이면 운영자가
    /// 원인을 찾는 동안 이것부터 끌 수 있어야 한다.
    pub reap_orphan_sessions: bool,
}

impl Default for ReconcileConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            stale_after: Duration::from_secs(60),
            dispatched_worker_check_after: Duration::from_secs(30),
            offline_worker_grace: Duration::from_secs(300),
            max_dispatch_retries: 20,
            session_list_timeout: Duration::from_secs(5),
            command_ack_timeout: Duration::from_secs(300),
            reap_orphan_sessions: true,
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
    /// 담당 워커가 재시작해(새 incarnation) 이전 프로세스의 고아로 발견된
    /// dispatched 작업 수.
    pub restarted_worker_found: u64,
    /// 이번 라운드에 Failed로 전이시킨, 재시작한 워커 배정 작업 수.
    pub restarted_worker_failed: u64,
    /// 담당 워커가 `Offline`으로 장기간 남아있어 발견된 stale dispatched 작업 수.
    pub offline_worker_found: u64,
    /// 이번 라운드에 Failed로 전이시킨, offline 워커 배정 작업 수.
    pub offline_worker_failed: u64,
    /// `retry_count`가 `max_dispatch_retries`에 도달해 재시도를 포기하고
    /// dead-letter(`Failed`)로 전이시킨 작업 수 (로드맵 #38).
    pub dead_lettered: u64,
    /// 워커가 권위 있는 인벤토리로 답했고 그 안에 이 Task의 세션이 없어
    /// 실행이 사라진 것으로 발견된 `Dispatched` 작업 수
    /// (로드맵 `#70` 게이트 2).
    pub vanished_session_found: u64,
    /// 이번 라운드에 `Failed(ExecutionVanished)`로 전이시킨 작업 수.
    pub vanished_session_failed: u64,
    /// 워커가 아직 들고 있지만 그 Task는 이미 끝나 있어 취소를 보낸 세션 수
    /// (로드맵 `#70` 게이트 2·7).
    pub orphan_sessions_cancelled: u64,
    /// 워커가 들고 있는데 **어느 Task도 지목하지 않는** 세션 수. 손대지 않고
    /// 감사에만 남긴다.
    pub unclaimed_sessions_found: u64,
    /// 명령이 임계 시간을 넘도록 확인되지 않아 이번 라운드에 처음 보고한
    /// Agent 수 (로드맵 `#70` 게이트 3).
    pub unacked_commands_found: u64,
}

/// stale `Pending` 작업 재조정기. spawn하면 백그라운드 태스크를 반환.
pub struct Reconciler {
    state: Arc<FleetState>,
    dispatcher: Arc<Dispatcher>,
    config: ReconcileConfig,
    /// 이미 감사에 남긴 (Agent, 명령 세대) 쌍 (로드맵 `#70` 게이트 3).
    ///
    /// 매 tick 남기면 같은 사실이 감사를 채워 다른 기록을 덮는다. 인메모리인
    /// 것이 의도다 — 재시작하면 한 번 더 남고, 그것은 **옳다**: 새 프로세스는
    /// 그 사실을 아직 보고한 적이 없다.
    unacked_reported: tokio::sync::Mutex<HashSet<(fleet_core::AgentId, i64)>>,
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
            unacked_reported: tokio::sync::Mutex::new(HashSet::new()),
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
        // control plane lease 확인 (로드맵 #63 불변식 2·"Reconciler는 Active
        // Orchestrator epoch에서만 동작한다"). 개별 dispatch 시도가 아니라
        // sweep 전체를 건너뛴다 — dead-letter 확정, dispatch, stale
        // Dispatched reap이 전부 이 인스턴스가 지금 제어권을 갖고 있다는
        // 전제 위에 있다. 조회(list_tasks)는 lease 없이도 허용되지만
        // (불변식 1), 이 함수는 조회로 끝나지 않고 항상 mutation까지
        // 이어지므로 여기서는 조회 전에 미리 막는다.
        if !self.state.lease_allows_control() {
            debug!(
                "reconcile: skipping sweep — this instance does not hold the control plane lease"
            );
            return ReconcileSummary::default();
        }

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
                // 이 루프는 `Pending` 작업만 돈다. `[Pending]`으로 좁혀야
                // 스캔 도중 다른 인스턴스가 dispatch에 성공한 작업을
                // dead-letter로 죽이지 않는다.
                if self
                    .dispatcher
                    .mark_failed(
                        task_id,
                        &[TaskPhase::Pending],
                        failure,
                        // reconciler의 스윕은 **현재 보유자가 지금 내리는 결정**이다.
                        // 대상 작업을 디스패치한 세대가 지금과 달라도 회수해야 한다 —
                        // 여기에 dispatch 세대 술어를 걸면 리스가 한 번 넘어간 뒤
                        // 남겨진 고아를 아무도 회수하지 못하는 라이브락이 된다.
                        TransitionOrigin::ControlDecision,
                    )
                    .await
                {
                    summary.dead_lettered += 1;
                    warn!(
                        %task_id, retry_count, ?kind,
                        "reconcile: dispatch retries exhausted, dead-lettering task"
                    );
                }
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
                Err(DispatchError::ControlPlaneFenced) => {
                    // sweep 시작 시점엔 lease가 유효했지만 그 사이(다른 task
                    // 처리 중) fenced된 드문 경합 — task 상태는 건드려지지
                    // 않았으므로(Pending 유지) 로깅만 하고 이번 sweep의
                    // 나머지도 계속 스킵되게 둔다.
                    warn!(%task_id, "reconcile: lost the control plane lease mid-sweep, leaving pending");
                }
                Err(e) => {
                    // 두 가지가 여기로 모인다. (a) transport 실패 등 진짜 dispatch
                    // 에러는 dispatch_existing이 이미 Failed로 마킹했다. (b) NotPending은
                    // dispatch 직전 compare-and-set이 거절된 경우로, 상태를 전혀
                    // 건들지 않았다 — 다른 writer가 이미 그 작업을 가져갔다는 뜻이므로
                    // 여기서 실패로 마킹하면 남의 소유물을 죽이게 된다. 둘 다 로깅만 한다.
                    warn!(%task_id, error = %e, "reconcile: dispatch attempt failed");
                }
            }
        }

        let inventories = self.reap_stale_dispatched(&mut summary).await;
        self.reap_orphan_sessions(inventories, &mut summary).await;
        self.report_unacked_commands(&mut summary).await;

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
    /// 반환값은 이 sweep에서 실제로 물어본 워커별 인벤토리다. 고아 세션
    /// 스윕이 같은 답을 다시 묻지 않도록 넘겨준다 — 한 sweep 안에서 두 번
    /// 물으면 서로 다른 시점의 목록으로 두 판정을 내리게 되고, 그 둘이
    /// 모순될 수 있다.
    async fn reap_stale_dispatched(
        &self,
        summary: &mut ReconcileSummary,
    ) -> HashMap<WorkerId, SessionInventory> {
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
                return HashMap::new();
            }
        };

        let now = Utc::now();
        let check_after = chrono::Duration::from_std(self.config.dispatched_worker_check_after)
            .unwrap_or_else(|_| chrono::Duration::seconds(30));
        let offline_grace = chrono::Duration::from_std(self.config.offline_worker_grace)
            .unwrap_or_else(|_| chrono::Duration::seconds(300));
        // 워커당 **한 번만** 묻는다 (로드맵 `#70` 게이트 2). Task마다 물으면
        // 같은 워커에 붙은 작업 수만큼 왕복이 나가고, 더 나쁘게는 한 sweep
        // 안에서 서로 다른 시점의 인벤토리로 판정하게 된다 — 그러면 같은
        // 라운드의 두 판정이 모순될 수 있다.
        let mut inventories: HashMap<WorkerId, SessionInventory> = HashMap::new();

        for task in dispatched {
            let TaskStatus::Dispatched {
                worker_id,
                started_at,
            } = task.status
            else {
                continue; // list_tasks 필터가 이미 보장하지만 방어적으로 스킵.
            };

            // 회수 판정의 기준 시각. `dispatched_at`은 Store가 `NOW()`로 찍으므로
            // `incarnation_started_at`과 같은 시계에서 나온다 — 오케스트레이터가
            // 여러 대여도 호스트 간 시계 오차가 판정에 들어오지 않는다. 012 이전에
            // 만들어진 행만 `None`이며, 그때는 오케스트레이터 시계인 `started_at`로
            // 접는다.
            let dispatched_at = task.dispatched_at.unwrap_or(started_at);

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
                    // (a) 워커 row 자체가 사라짐 — 운영자의 `delete_worker`나
                    // 새 이름으로 다시 조인한 경우다. 강한 신호이므로 짧은
                    // 유예(check_after)만 둔다.
                    //
                    // 여기 있던 "재시작으로 새 worker_id를 받은 경우가 대표적"이라는
                    // 설명은 사실이 아니었다. `register_worker`는 같은 `--name`이면
                    // 기존 `worker_id`를 그대로 재사용하므로 재시작의 정상 경로는
                    // 이 분기에 오지 않는다. 그 오해 때문에 재시작 고아를 어느
                    // 분기도 회수하지 않는 창이 열려 있었고, 아래 (c)가 그 창이다.
                    summary.orphaned_found += 1;
                    let failure = TaskFailure {
                        error: format!(
                            "assigned worker {worker_id} no longer registered (row deleted or rejoined under a new name)"
                        ),
                        kind: FailureKind::WorkerUnavailable,
                        worker_id: Some(worker_id),
                        attempts: 0,
                    };
                    // `[Dispatched]`로 좁힌다. 넓은 기본값을 쓰면 방금
                    // `Pending` → `Dispatched`로 넘어간 작업까지 orphan으로
                    // 오인한다.
                    if self
                        .dispatcher
                        .mark_failed(
                            task_id,
                            &[TaskPhase::Dispatched],
                            failure,
                            // reconciler의 스윕은 **현재 보유자가 지금 내리는 결정**이다.
                            // 대상 작업을 디스패치한 세대가 지금과 달라도 회수해야 한다 —
                            // 여기에 dispatch 세대 술어를 걸면 리스가 한 번 넘어간 뒤
                            // 남겨진 고아를 아무도 회수하지 못하는 라이브락이 된다.
                            TransitionOrigin::ControlDecision,
                        )
                        .await
                    {
                        summary.orphaned_failed += 1;
                        warn!(
                            %task_id, %worker_id,
                            "reconciliation: dispatched task's worker no longer exists, marked failed"
                        );
                    }
                }
                // (c) 워커는 존재하지만 이 작업이 배정된 뒤 **재시작**했다.
                // 재시작은 되돌릴 수 없는 사실이므로 (b)의 Offline 유예보다
                // 앞에 둔다 — 뒤에 두면 재시작 후 Online으로 복귀한 워커의
                // 고아가 맨 아래 `Some(_) => continue`로 다시 빠져나간다.
                //
                // 이 분기가 없는 동안 같은 이름으로 재시작한 워커의 진행 중
                // 작업은 회수 경로가 **하나도** 없었다: row는 남아 있으니 (a)가
                // 아니고, 재등록으로 Online에 하트비트도 새것이라 (b)도 아니다.
                // 그 작업들은 완료되지도 실패하지도 않은 채 `Dispatched`로
                // 영구히 남아 운영자에게 아무 신호도 주지 않는다.
                Some(ref w) if dispatched_at < w.incarnation_started_at => {
                    summary.restarted_worker_found += 1;
                    let restarted_at = w.incarnation_started_at;
                    let failure = TaskFailure {
                        error: format!(
                            "assigned worker {worker_id} restarted at {restarted_at} after this task \
                             was dispatched at {dispatched_at} — the process running it is gone"
                        ),
                        kind: FailureKind::WorkerUnavailable,
                        worker_id: Some(worker_id),
                        attempts: 0,
                    };
                    // (a)와 같은 이유로 `[Dispatched]`, 그리고 같은 이유로
                    // `ControlDecision` — 이 판정은 현재 보유자가 지금 내리는
                    // 결정이지 워커가 보고한 결과가 아니다.
                    if self
                        .dispatcher
                        .mark_failed(
                            task_id,
                            &[TaskPhase::Dispatched],
                            failure,
                            TransitionOrigin::ControlDecision,
                        )
                        .await
                    {
                        summary.restarted_worker_failed += 1;
                        warn!(
                            %task_id, %worker_id, %restarted_at, %dispatched_at,
                            "reconciliation: dispatched task's worker restarted, marked failed"
                        );
                    }
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
                    // (b)와 같은 이유로 `[Dispatched]`.
                    if self
                        .dispatcher
                        .mark_failed(
                            task_id,
                            &[TaskPhase::Dispatched],
                            failure,
                            // reconciler의 스윕은 **현재 보유자가 지금 내리는 결정**이다.
                            // 대상 작업을 디스패치한 세대가 지금과 달라도 회수해야 한다 —
                            // 여기에 dispatch 세대 술어를 걸면 리스가 한 번 넘어간 뒤
                            // 남겨진 고아를 아무도 회수하지 못하는 라이브락이 된다.
                            TransitionOrigin::ControlDecision,
                        )
                        .await
                    {
                        summary.offline_worker_failed += 1;
                        warn!(
                            %task_id, %worker_id, offline_for = %offline_for_desc,
                            "reconciliation: dispatched task's worker offline too long, marked failed"
                        );
                    }
                }
                Some(_) => {
                    // 워커는 존재하고 Offline이 아님(Online/Degraded/CircuitOpen).
                    // 워커의 **건강도**로는 더 할 말이 없다 — 응답이 느릴 뿐이면
                    // 헬스체크/CircuitBreaker의 영역이다.
                    //
                    // 여기가 게이트 2가 지목하던 구멍이다 (로드맵 `#70`). 워커는
                    // 멀쩡한데 **그 위의 실행**이 사라진 경우 — 오케스트레이터가
                    // 재시작해 완료 이벤트를 놓쳤거나, Agent가 세션을 잃었거나 —
                    // 어느 분기도 그것을 보지 않았고, 그 Task는 완료되지도 실패하지도
                    // 않은 채 `Dispatched`로 영구히 남았다. 이제 워커에게 **지금
                    // 무엇을 들고 있는지** 직접 묻는다.
                    let Some(session_id) = task.acp_session_id.as_deref() else {
                        // 세션 이름이 없으면 인벤토리에서 찾을 대상이 없다.
                        // `041` 이전에 만들어진 행, 그리고 `session/new` 전에
                        // 멈춘 행이 여기 온다 — **부재가 아니라 무지**이므로
                        // 아무 결론도 내리지 않는다.
                        continue;
                    };

                    let inventory = match inventories.get(&worker_id) {
                        Some(cached) => cached,
                        None => {
                            let fetched = match self
                                .state
                                .transport
                                .list_sessions(worker_id, self.config.session_list_timeout)
                                .await
                            {
                                Ok(inv) => inv,
                                Err(e) => {
                                    // 못 물어본 것을 "세션이 없다"로 읽으면 살아 있는
                                    // 실행을 전부 회수한다. 권위 없음으로 접는다.
                                    debug!(
                                        %worker_id, error = %e,
                                        "reconcile: could not list sessions; skipping inventory check"
                                    );
                                    SessionInventory::Undeclared
                                }
                            };
                            inventories.entry(worker_id).or_insert(fetched)
                        }
                    };

                    let SessionInventory::Reported(live) = inventory else {
                        // `Undeclared`/`Refused`에서는 부재를 판정할 수 없다 —
                        // 그 둘에서 같은 부재는 "저쪽이 말해 주지 않았다"와
                        // 구분되지 않는다(`SessionInventory` 문서).
                        continue;
                    };
                    if live.iter().any(|s| s == session_id) {
                        continue; // 여전히 살아 있다.
                    }

                    summary.vanished_session_found += 1;
                    // 같은 판정을 dispatch 전 복구(`Dispatcher::
                    // ensure_worker_recovered`)도 내리므로 실패 기록의 이름과
                    // 문장은 한 곳에서만 만든다.
                    let failure =
                        crate::dispatcher::vanished_session_failure(worker_id, session_id);
                    // 위 세 분기와 같은 이유로 `[Dispatched]`와 `ControlDecision`.
                    if self
                        .dispatcher
                        .mark_failed(
                            task_id,
                            &[TaskPhase::Dispatched],
                            failure,
                            TransitionOrigin::ControlDecision,
                        )
                        .await
                    {
                        summary.vanished_session_failed += 1;
                        warn!(
                            %task_id, %worker_id, %session_id,
                            "reconciliation: the worker's session inventory no longer lists this \
                             task's session, marked failed"
                        );
                    }
                }
            }
        }

        inventories
    }

    /// 워커가 아직 들고 있는데 **살아 있는 Task가 아무도 지목하지 않는**
    /// 세션을 정리한다 (로드맵 `#70` 게이트 2·7).
    ///
    /// [`reap_stale_dispatched`](Self::reap_stale_dispatched)의 **거울상**이다.
    /// 저쪽은 우리가 들고 있는 Task에서 출발해 워커에게 그 실행이 아직
    /// 있는지 묻고, 이쪽은 워커가 들고 있는 세션에서 출발해 그것이 아직
    /// 누구의 것인지 묻는다. 두 방향이 모두 필요한 이유는 어긋남이 양쪽으로
    /// 생기기 때문이다 — Task는 남았는데 실행이 사라지는 쪽은 저쪽이 보고,
    /// **실행은 남았는데 Task가 끝난** 쪽은 이쪽만 본다.
    ///
    /// 그 두 번째가 실제로 새는 자리다. `Dispatcher::cancel`이
    /// `CancelDelivery::Unreachable`을 받으면 저장소에는 `Cancelled`를 적지만
    /// 워커는 그 통지를 받은 적이 없다. 그 Task는 이제 **종료 상태**라
    /// `reap_stale_dispatched`의 `Dispatched` 필터에 걸리지 않고, 전송 계층의
    /// 세션 맵은 프로세스와 함께 비워지며, 워커의 세션은 계속 돌면서 토큰을
    /// 쓴다. 그것을 지목할 수 있는 코드가 한 곳도 없었다.
    ///
    /// 세션 하나에 대한 처분은 셋이고, **셋을 가르는 것이 이 함수의 전부다**:
    ///
    /// | `find_task_by_acp_session` | 뜻 | 처분 |
    /// | --- | --- | --- |
    /// | 살아 있는 Task | 정상 실행 | 건드리지 않는다 |
    /// | 종료된 Task | 고아 — 상태와 실제가 어긋났다 | 취소를 보낸다 |
    /// | `None` | 누구의 것인지 모른다 | **손대지 않고** 감사에만 남긴다 |
    ///
    /// 세 번째를 두 번째로 접으면 안 된다. `None`인 경우가 셋이고 그중 둘은
    /// 죽이면 안 되는 것이다 — 지금 dispatch가 진행 중이라 `acp_session_id`가
    /// 아직 커밋되지 않았거나(경합 창), 아예 다른 제어면이 연 세션이거나,
    /// 우리가 기록을 잃었거나. 첫째와 둘째에서 취소를 보내면 살아 있는 남의
    /// 실행을 죽인다. 관측만 남기는 것이 여기서 가능한 가장 강한 처분이다.
    async fn reap_orphan_sessions(
        &self,
        mut inventories: HashMap<WorkerId, SessionInventory>,
        summary: &mut ReconcileSummary,
    ) {
        if !self.config.reap_orphan_sessions {
            return;
        }

        // `reap_stale_dispatched`가 물어본 워커는 **자기 Task가 있는 워커뿐**
        // 이다. 고아 세션은 그 반대쪽에 있다 — Task가 이미 끝났으므로 그
        // 워커에는 `Dispatched`가 한 건도 없을 수 있고, 실제로 그쪽이 더 흔한
        // 경우다. 그래서 여기서는 워커를 따로 전수한다.
        let workers = match self
            .state
            .store
            .list_workers(&fleet_core::WorkerFilter {
                limit: MAX_PENDING_SCAN,
                ..Default::default()
            })
            .await
        {
            Ok(w) => w,
            Err(e) => {
                warn!(error = %e, "reconcile: failed to list workers for the orphan session sweep");
                return;
            }
        };

        for worker in workers {
            let worker_id = worker.id;
            let inventory = match inventories.remove(&worker_id) {
                Some(cached) => cached,
                None => match self
                    .state
                    .transport
                    .list_sessions(worker_id, self.config.session_list_timeout)
                    .await
                {
                    Ok(inv) => inv,
                    Err(e) => {
                        debug!(
                            %worker_id, error = %e,
                            "reconcile: could not list sessions; skipping the orphan sweep for this worker"
                        );
                        continue;
                    }
                },
            };

            let SessionInventory::Reported(sessions) = inventory else {
                // 권위 없는 답으로는 "이 세션이 고아다"를 말할 수 없다. 더
                // 중요하게는, 여기서 접으면 **아무 목록도 없는 것**과 같아져
                // 애초에 순회할 대상이 없다.
                continue;
            };

            for session_id in sessions {
                let owner = match self.state.store.find_task_by_acp_session(&session_id).await {
                    Ok(t) => t,
                    Err(e) => {
                        // 조회가 실패한 것을 "주인이 없다"로 읽으면 살아 있는
                        // 실행을 고아로 만든다. 다음 tick에서 다시 본다.
                        warn!(
                            %worker_id, %session_id, error = %e,
                            "reconcile: could not look up the owner of a session; leaving it alone"
                        );
                        continue;
                    }
                };

                let Some(task) = owner else {
                    // 누구의 것인지 모른다 — 위 표의 셋째 줄. 손대지 않는다.
                    summary.unclaimed_sessions_found += 1;
                    self.state
                        .audit_decision(
                            fleet_core::audit::action::CONTROL_UNCLAIMED_SESSION,
                            ("worker", worker_id.to_string()),
                            serde_json::json!({ "session_id": session_id }),
                        )
                        .await;
                    warn!(
                        %worker_id, %session_id,
                        "reconciliation: the worker holds a session that no task claims — left running"
                    );
                    continue;
                };

                if !task.is_terminal() {
                    continue; // 정상 실행.
                }

                // 종료된 Task의 실행이 아직 살아 있다. **여기서 `task.id`가
                // 진짜인 것이 중요하다** — 합성한 id로 취소를 보내면 전송
                // 계층의 1단계 조회가 엉뚱한 세션을 집을 수 있고, 감사에는
                // 존재하지 않는 Task가 남는다.
                let req = fleet_transport::CancelRequest {
                    task_id: task.id,
                    known_session: Some(session_id.clone()),
                    worker_id: Some(worker_id),
                };
                match self.state.transport.cancel(req).await {
                    Ok(fleet_transport::CancelDelivery::Sent) => {
                        summary.orphan_sessions_cancelled += 1;
                        self.state
                            .audit_decision(
                                fleet_core::audit::action::CONTROL_ORPHAN_SESSION_CANCELLED,
                                ("task", task.id.to_string()),
                                serde_json::json!({
                                    "worker_id": worker_id.to_string(),
                                    "session_id": session_id,
                                    "task_phase": task.status.phase().as_str(),
                                }),
                            )
                            .await;
                        warn!(
                            task_id = %task.id, %worker_id, %session_id,
                            "reconciliation: cancelled a session whose task had already ended"
                        );
                    }
                    // 워커에 닿지 않았다. 인벤토리는 방금 답했는데 취소는 못
                    // 보낸 경우이므로(그 사이에 연결이 끊겼다) 다음 tick에서
                    // 다시 본다 — 그때는 인벤토리 조회부터 실패할 것이다.
                    Ok(other) => {
                        debug!(
                            task_id = %task.id, %worker_id, %session_id, delivery = ?other,
                            "reconcile: orphan session cancel was not delivered"
                        );
                    }
                    Err(e) => {
                        warn!(
                            task_id = %task.id, %worker_id, %session_id, error = %e,
                            "reconcile: orphan session cancel failed"
                        );
                    }
                }
            }
        }
    }

    /// 명령이 확인되지 않은 채 임계 시간을 넘긴 Agent를 감사에 남긴다
    /// (로드맵 `#70` 게이트 3 — ACK 유실).
    ///
    /// **다시 보내지 않는다.** 게이트 문언의 "자동 중복 실행 없이"가 요구하는
    /// 자리가 정확히 여기다: 확인되지 않은 명령을 자동으로 재발행하면 Worker가
    /// 첫 명령을 늦게 집어갔을 때 같은 Agent가 두 번 뜬다. 확인이 없다는 것은
    /// "도달하지 않았다"가 아니라 **"모른다"**이므로, 여기서 할 수 있는 가장
    /// 강한 처분은 운영자에게 보이게 만드는 것이다.
    ///
    /// `031`이 `command_generation`/`last_acked_generation`을 만든 뒤로
    /// "확인됐는가"는 알 수 있었지만 **그 값으로 판정하는 코드가 한 곳도
    /// 없었다** — `command_delivered()`/`start_pending()`의 호출부는 MCP 응답을
    /// 조립하는 두 줄뿐이었다. 운영자가 그 필드를 직접 조회하지 않는 한 Worker가
    /// 명령을 영영 집어가지 않아도 아무 신호가 나지 않았다.
    async fn report_unacked_commands(&self, summary: &mut ReconcileSummary) {
        let threshold = match chrono::Duration::from_std(self.config.command_ack_timeout) {
            Ok(d) => d,
            Err(_) => chrono::Duration::seconds(300),
        };
        let agents = match self
            .state
            .store
            .list_agents(&fleet_core::AgentFilter {
                project_id: None,
                status: None,
                worker_id: None,
                limit: MAX_PENDING_SCAN,
                offset: 0,
            })
            .await
        {
            Ok(a) => a,
            Err(e) => {
                warn!(error = %e, "reconcile: failed to list agents for the unacked-command sweep");
                return;
            }
        };

        let now = Utc::now();
        let mut reported = self.unacked_reported.lock().await;
        for agent in agents {
            // 배정된 Worker가 없으면 확인할 상대가 없다 — 미확인이 아니라
            // **보낸 적이 없는** 것이다.
            let Some(worker_id) = agent.worker_id else {
                continue;
            };
            let Some(unacked_for) = agent.command_unacked_for(now) else {
                continue; // 확인이 끝났거나 발행 시각을 모른다.
            };
            if unacked_for < threshold {
                continue;
            }
            let key = (agent.id, agent.command_generation);
            if !reported.insert(key) {
                continue; // 이 세대는 이미 보고했다.
            }

            summary.unacked_commands_found += 1;
            self.state
                .audit_decision(
                    fleet_core::audit::action::AGENT_COMMAND_UNACKED,
                    ("agent", agent.id.to_string()),
                    serde_json::json!({
                        "worker_id": worker_id.to_string(),
                        "command_generation": agent.command_generation,
                        "last_acked_generation": agent.last_acked_generation,
                        "unacked_for_secs": unacked_for.num_seconds(),
                        "desired_status": agent.desired_status.as_str(),
                    }),
                )
                .await;
            warn!(
                agent_id = %agent.id, %worker_id,
                command_generation = agent.command_generation,
                unacked_for_secs = unacked_for.num_seconds(),
                "reconciliation: the assigned worker has not acknowledged this agent command — \
                 not reissuing it (that would risk a duplicate start)"
            );
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
        let (state, dispatcher, _) = setup_with_transport(store, mock_workers).await;
        (state, dispatcher)
    }

    /// `setup`과 같되 transport 핸들도 함께 돌려준다 — 인벤토리를 설정해야
    /// 하는 시험(로드맵 `#70` 게이트 2)에 필요하다.
    async fn setup_with_transport(
        store: Arc<dyn Store>,
        mock_workers: Vec<MockWorker>,
    ) -> (Arc<FleetState>, Arc<Dispatcher>, Arc<MockTransport>) {
        let transport = MockTransport::new();
        for mw in mock_workers {
            transport.add_worker(mw).await;
        }
        let event_rx = fleet_transport::WorkerTransport::subscribe(&transport)
            .await
            .unwrap();
        let mock = Arc::new(transport);
        let transport: Arc<dyn fleet_transport::WorkerTransport> = mock.clone();

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

        (state, dispatcher, mock)
    }

    /// 온라인 워커. `incarnation_started_at`을 충분히 과거로 밀어 둔다 —
    /// `Worker::new`의 기본값(지금)을 그대로 쓰면 "존재하지도 않던 워커에
    /// 120초 전에 배정된 작업"이라는 물리적으로 불가능한 픽스처가 되고,
    /// 재시작 회수 분기가 그 관계를 정확히 보기 때문에 의도치 않게 발동한다.
    fn make_worker(name: &str) -> Worker {
        let mut w = Worker::new(name, format!("wss://{name}/ws"));
        w.status = WorkerStatus::Online;
        w.incarnation_started_at = chrono::Utc::now() - chrono::Duration::hours(1);
        w
    }

    /// `Pending` 상태의 작업을 지정된 나이(age)로 생성.
    fn make_pending_task(prompt: &str, age: chrono::Duration) -> Task {
        let mut task = Task::from_request(TaskRequest {
            prompt: prompt.into(),
            created_by: "test".into(),
            // 로드맵 #69 — dispatch 경로가 `cwd`를 요구한다.
            cwd: Some("/srv/fleet/workspaces/test".into()),
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
            // 로드맵 #69 — dispatch 경로가 `cwd`를 요구한다.
            cwd: Some("/srv/fleet/workspaces/test".into()),
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
                session_list_timeout: Duration::from_secs(5),
                command_ack_timeout: Duration::from_secs(300),
                reap_orphan_sessions: true,
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
    async fn reconcile_once_skips_the_whole_sweep_when_control_plane_lease_is_fenced() {
        // 로드맵 #63 2단계 — "Reconciler는 Active Orchestrator epoch에서만
        // 동작한다"(docs/architecture/control-plane-authority-and-failover.md).
        // dispatch_existing 레벨의 개별 거절이 아니라, sweep 자체가 아예
        // 시작되지 않아야 한다.
        let worker = make_worker("idle-1");
        let worker_id = worker.id;

        let store: Arc<dyn Store> = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_pending_task("stale work", chrono::Duration::seconds(120));
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(MockTransport::new());
        let state = Arc::new(
            FleetState::new(store.clone(), transport, CircuitBreakerConfig::default()).with_lease(
                crate::lease::LeaseObserver::with_status(
                    "test-cluster",
                    "inst-a",
                    crate::lease::LeaseStatus::Fenced,
                ),
            ),
        );
        let dispatcher = Arc::new(Dispatcher::new(state.clone()));

        let reconciler = Reconciler::new(
            state.clone(),
            dispatcher,
            ReconcileConfig {
                interval: Duration::from_secs(3600),
                stale_after: Duration::from_secs(60),
                dispatched_worker_check_after: Duration::from_secs(30),
                offline_worker_grace: Duration::from_secs(300),
                max_dispatch_retries: 20,
                session_list_timeout: Duration::from_secs(5),
                command_ack_timeout: Duration::from_secs(300),
                reap_orphan_sessions: true,
            },
        );

        let summary = reconciler.reconcile_once().await;
        assert_eq!(
            summary,
            ReconcileSummary::default(),
            "the entire sweep must be skipped while fenced, not just individual dispatch attempts"
        );

        // 워커도 있고 stale하기까지 한 task인데도 손대지 않았어야 한다.
        let _ = worker_id;
        let stored = store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(stored.status, TaskStatus::Pending),
            "expected Pending (untouched), got {:?}",
            stored.status
        );
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
                session_list_timeout: Duration::from_secs(5),
                command_ack_timeout: Duration::from_secs(300),
                reap_orphan_sessions: true,
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
                session_list_timeout: Duration::from_secs(5),
                command_ack_timeout: Duration::from_secs(300),
                reap_orphan_sessions: true,
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
                session_list_timeout: Duration::from_secs(5),
                command_ack_timeout: Duration::from_secs(300),
                reap_orphan_sessions: true,
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
                session_list_timeout: Duration::from_secs(5),
                command_ack_timeout: Duration::from_secs(300),
                reap_orphan_sessions: true,
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
        // 배정된 워커의 row 자체가 사라진 경우 — 운영자의 삭제나 새 이름으로의
        // 재조인이다. (재시작은 같은 `worker_id`를 유지하므로 이 분기가 아니라
        // 아래 `..._on_restarted_worker_...`가 담당한다.)
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
                session_list_timeout: Duration::from_secs(5),
                command_ack_timeout: Duration::from_secs(300),
                reap_orphan_sessions: true,
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
        // `make_worker`와 같은 이유 — Offline 유예 경로를 재현하려면 재시작
        // 분기가 먼저 발동하지 않아야 한다.
        w.incarnation_started_at = chrono::Utc::now() - chrono::Duration::hours(1);
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
                session_list_timeout: Duration::from_secs(5),
                command_ack_timeout: Duration::from_secs(300),
                reap_orphan_sessions: true,
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
    async fn dispatched_task_on_restarted_worker_is_marked_failed() {
        // 이 증분 전까지 회수 경로가 **하나도 없던** 창의 회귀 테스트.
        //
        // 워커가 같은 `--name`으로 재시작하면 `register_worker`가 기존
        // `worker_id`를 재사용하므로(fleet-api `handlers.rs`), row는 그대로
        // 남고 상태는 다시 `Online`, 하트비트도 새것이다. 그래서 (a) 워커
        // 부재에도, (b) Offline 300초 유예에도 걸리지 않는다. 그 결과 이전
        // 프로세스에 디스패치됐던 작업은 완료되지도 실패하지도 않은 채
        // `Dispatched`로 영구히 남았다.
        let mut worker = make_worker("restarted-in-place");
        // 태스크가 디스패치된 뒤(120초 전보다 나중)에 재시작했다.
        worker.incarnation_started_at = chrono::Utc::now() - chrono::Duration::seconds(30);
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task(
            "was running on the old process",
            worker_id,
            chrono::Duration::seconds(120),
        );
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;
        let reconciler = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default());

        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary.restarted_worker_found, 1);
        assert_eq!(summary.restarted_worker_failed, 1);
        assert_eq!(
            summary.orphaned_found, 0,
            "워커 row는 남아 있으므로 (a) 경로가 아니다"
        );
        assert_eq!(
            summary.offline_worker_found, 0,
            "워커는 Online이므로 (b) 경로가 아니다 — 이것이 창이 열려 있던 이유다"
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
    async fn task_dispatched_after_the_restart_is_left_alone() {
        // 재시작 **뒤에** 디스패치된 작업은 현재 프로세스의 것이다. 술어가
        // 방향을 잃으면 정상 작업을 매 tick마다 죽이므로, 반대 방향도 고정한다.
        let mut worker = make_worker("restarted-then-got-work");
        worker.incarnation_started_at = chrono::Utc::now() - chrono::Duration::seconds(600);
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task(
            "belongs to the current process",
            worker_id,
            chrono::Duration::seconds(120),
        );
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;
        let reconciler = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default());

        let summary = reconciler.reconcile_once().await;
        assert_eq!(summary.restarted_worker_found, 0);

        let still_dispatched = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(
            still_dispatched.status,
            TaskStatus::Dispatched { .. }
        ));
    }

    #[tokio::test]
    async fn restart_check_prefers_dispatched_at_over_started_at() {
        // 판정 기준은 Store가 `NOW()`로 찍는 `dispatched_at`이고, 그것이 없는
        // 구 행(migration 012 이전)에서만 오케스트레이터 시계인 `started_at`로
        // 접는다. 두 값을 어긋나게 두어 어느 쪽을 보는지 고정한다.
        let mut worker = make_worker("skewed");
        worker.incarnation_started_at = chrono::Utc::now() - chrono::Duration::seconds(300);
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        // `started_at`은 재시작보다 앞서지만(회수 대상처럼 보인다),
        // `dispatched_at`은 재시작보다 나중이다(실제로는 현재 프로세스의 작업).
        let mut task = make_dispatched_task("skewed", worker_id, chrono::Duration::seconds(600));
        task.dispatched_at = Some(chrono::Utc::now() - chrono::Duration::seconds(120));
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;
        let reconciler = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default());

        let summary = reconciler.reconcile_once().await;
        assert_eq!(
            summary.restarted_worker_found, 0,
            "dispatched_at이 아니라 started_at으로 판정하고 있다"
        );
        assert!(matches!(
            state.store.get_task(task_id).await.unwrap().unwrap().status,
            TaskStatus::Dispatched { .. }
        ));
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
                session_list_timeout: Duration::from_secs(5),
                command_ack_timeout: Duration::from_secs(300),
                reap_orphan_sessions: true,
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

    // ── 인벤토리 기반 회수 (로드맵 `#70` 게이트 2) ──────────────────────
    //
    // 이 네 시험이 함께 단정하는 것은 **회수의 근거가 워커의 건강도가 아니라
    // 워커가 답한 인벤토리**라는 것이다. 앞의 세 분기(row 삭제·재시작·Offline)는
    // 전부 워커에 관한 사실로 판정하고, 워커가 멀쩡한데 그 위의 실행만 사라진
    // 경우를 하나도 보지 못했다.

    /// 세션 픽스처를 갖춘 `Dispatched` 작업.
    fn make_dispatched_task_with_session(
        prompt: &str,
        worker_id: WorkerId,
        age: chrono::Duration,
        session_id: &str,
    ) -> Task {
        let mut task = make_dispatched_task(prompt, worker_id, age);
        task.acp_session_id = Some(session_id.to_string());
        task
    }

    #[tokio::test]
    async fn a_dispatched_task_whose_session_vanished_from_the_inventory_is_marked_failed() {
        let worker = make_worker("healthy-but-empty");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task_with_session(
            "its session is gone",
            worker_id,
            chrono::Duration::seconds(120),
            "sess-gone",
        );
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        // transport에도 등록한다 — `list_sessions`는 등록된 워커만 안다.
        // 등록하지 않으면 `WorkerNotRegistered`가 나고 구현이 그것을 권위 없음으로
        // 접으므로, 이 시험이 **인벤토리를 읽지 않고도** 통과해 버린다.
        let (state, dispatcher, transport) = setup_with_transport(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, worker.endpoint.clone())],
        )
        .await;
        // 워커는 **다른** 세션 하나를 들고 있다. 빈 목록으로 두면 "목록을
        // 읽었는가"와 "목록이 비어 있었는가"가 구분되지 않는다.
        transport
            .set_session_inventory(
                worker_id,
                fleet_transport::SessionInventory::Reported(vec!["sess-other".into()]),
            )
            .await;

        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.vanished_session_found, 1);
        assert_eq!(summary.vanished_session_failed, 1);
        let failed = state.store.get_task(task_id).await.unwrap().unwrap();
        match failed.status {
            TaskStatus::Failed(f) => assert_eq!(f.kind, FailureKind::ExecutionVanished),
            other => panic!("기대와 다름: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_task_whose_session_is_still_listed_is_left_alone() {
        let worker = make_worker("still-running");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task_with_session(
            "still running",
            worker_id,
            chrono::Duration::seconds(120),
            "sess-live",
        );
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        // transport에도 등록한다 — `list_sessions`는 등록된 워커만 안다.
        // 등록하지 않으면 `WorkerNotRegistered`가 나고 구현이 그것을 권위 없음으로
        // 접으므로, 이 시험이 **인벤토리를 읽지 않고도** 통과해 버린다.
        let (state, dispatcher, transport) = setup_with_transport(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, worker.endpoint.clone())],
        )
        .await;
        transport
            .set_session_inventory(
                worker_id,
                fleet_transport::SessionInventory::Reported(vec![
                    "sess-other".into(),
                    "sess-live".into(),
                ]),
            )
            .await;

        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.vanished_session_found, 0);
        let task = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(task.status, TaskStatus::Dispatched { .. }));
    }

    /// **인벤토리를 주지 않는 워커에서는 아무 결론도 내리지 않는다.**
    ///
    /// 이것이 이 변경에서 가장 중요한 부정 단정이다. `Undeclared`를 빈 목록으로
    /// 읽는 구현은 오늘의 모든 배포에서 **진행 중인 모든 작업을 회수한다** —
    /// 실제 Agent 다수가 `session/list`를 광고하지 않기 때문이다.
    #[tokio::test]
    async fn a_worker_that_declares_no_inventory_never_triggers_a_reap() {
        let worker = make_worker("silent-about-sessions");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task_with_session(
            "nobody can say whether this is alive",
            worker_id,
            chrono::Duration::seconds(120),
            "sess-unknowable",
        );
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        // 워커는 transport에 **등록돼 있다** — 그래야 `Undeclared`가 조회
        // 실패의 부산물이 아니라 실제로 읽은 답이 된다.
        // MockTransport의 기본값이 `Undeclared`라 일부러 설정하지 않는다.
        let (state, dispatcher, _) = setup_with_transport(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, worker.endpoint.clone())],
        )
        .await;

        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.vanished_session_found, 0);
        let task = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(
            matches!(task.status, TaskStatus::Dispatched { .. }),
            "인벤토리를 주지 않는 워커의 작업을 회수했다 — 오늘의 모든 배포가 그 워커다"
        );
    }

    /// 광고했다가 거절한 워커도 마찬가지다. `Refused`는 "이 Agent 구현이
    /// 선언과 어긋난다"는 진단이지 "세션이 없다"가 아니다.
    #[tokio::test]
    async fn a_refused_inventory_never_triggers_a_reap() {
        let worker = make_worker("declares-then-refuses");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task_with_session(
            "the agent contradicted itself",
            worker_id,
            chrono::Duration::seconds(120),
            "sess-x",
        );
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        // transport에도 등록한다 — `list_sessions`는 등록된 워커만 안다.
        // 등록하지 않으면 `WorkerNotRegistered`가 나고 구현이 그것을 권위 없음으로
        // 접으므로, 이 시험이 **인벤토리를 읽지 않고도** 통과해 버린다.
        let (state, dispatcher, transport) = setup_with_transport(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, worker.endpoint.clone())],
        )
        .await;
        transport
            .set_session_inventory(
                worker_id,
                fleet_transport::SessionInventory::Refused {
                    message: "method not found".into(),
                },
            )
            .await;

        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.vanished_session_found, 0);
        let task = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(task.status, TaskStatus::Dispatched { .. }));
    }

    /// 세션 이름이 없는 작업은 인벤토리에서 찾을 대상이 없다 — **부재가 아니라
    /// 무지**다. `041` 이전 행과 `session/new` 전에 멈춘 행이 여기 온다.
    #[tokio::test]
    async fn a_task_without_a_session_name_is_not_judged_by_the_inventory() {
        let worker = make_worker("empty-inventory");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        // `acp_session_id`가 없다.
        let task =
            make_dispatched_task("no session name", worker_id, chrono::Duration::seconds(120));
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        // transport에도 등록한다 — `list_sessions`는 등록된 워커만 안다.
        // 등록하지 않으면 `WorkerNotRegistered`가 나고 구현이 그것을 권위 없음으로
        // 접으므로, 이 시험이 **인벤토리를 읽지 않고도** 통과해 버린다.
        let (state, dispatcher, transport) = setup_with_transport(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, worker.endpoint.clone())],
        )
        .await;
        transport
            .set_session_inventory(
                worker_id,
                fleet_transport::SessionInventory::Reported(Vec::new()),
            )
            .await;

        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.vanished_session_found, 0);
        let task = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(task.status, TaskStatus::Dispatched { .. }));
    }

    /// 유예 시간 안의 작업은 인벤토리를 묻기도 전에 건너뛴다 — 방금
    /// `Pending → Dispatched`로 넘어간 작업은 아직 `session/new` 왕복 중이라
    /// 워커의 목록에 없는 것이 정상이다.
    #[tokio::test]
    async fn a_freshly_dispatched_task_is_not_judged_by_the_inventory() {
        let worker = make_worker("fresh");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task_with_session(
            "just dispatched",
            worker_id,
            chrono::Duration::seconds(1),
            "sess-not-yet-open",
        );
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        // transport에도 등록한다 — `list_sessions`는 등록된 워커만 안다.
        // 등록하지 않으면 `WorkerNotRegistered`가 나고 구현이 그것을 권위 없음으로
        // 접으므로, 이 시험이 **인벤토리를 읽지 않고도** 통과해 버린다.
        let (state, dispatcher, transport) = setup_with_transport(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, worker.endpoint.clone())],
        )
        .await;
        transport
            .set_session_inventory(
                worker_id,
                fleet_transport::SessionInventory::Reported(Vec::new()),
            )
            .await;

        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.vanished_session_found, 0);
        let task = state.store.get_task(task_id).await.unwrap().unwrap();
        assert!(matches!(task.status, TaskStatus::Dispatched { .. }));
    }

    // ── 고아 세션 회수 (로드맵 `#70` 게이트 2·7) ────────────────────────
    //
    // 위 블록이 "Task는 남았는데 실행이 사라진" 쪽을 고정했다면, 이 블록은
    // **거울상**을 고정한다 — 실행은 남았는데 Task가 끝난 쪽이다. 그쪽은
    // `Dispatched`만 훑어서는 원리적으로 볼 수 없다(대상이 이미 종료 상태라
    // 그 필터에 걸리지 않는다).

    /// 종료된 Task의 세션을 워커가 아직 들고 있으면 취소를 보낸다.
    ///
    /// **이것이 새던 자리다.** `cancel`이 `Unreachable`을 받아도 저장소에는
    /// `Cancelled`가 적히고, 그 Task는 종료 상태라 `reap_stale_dispatched`의
    /// `Dispatched` 필터에 걸리지 않으며, 전송 계층의 세션 맵은 프로세스와
    /// 함께 비워진다. 그 실행을 지목할 수 있는 코드가 한 곳도 없었다.
    #[tokio::test]
    async fn a_session_whose_task_already_ended_is_cancelled() {
        let worker = make_worker("still-holds-a-dead-task");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        // 취소가 전달되지 않은 채 종료 상태가 된 Task를 재현한다.
        let mut task = make_dispatched_task_with_session(
            "cancelled but never told",
            worker_id,
            chrono::Duration::seconds(120),
            "sess-zombie",
        );
        task.status = TaskStatus::Cancelled {
            reason: "operator asked".into(),
            cancelled_at: chrono::Utc::now(),
        };
        let task_id = task.id;
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher, transport) = setup_with_transport(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, worker.endpoint.clone())],
        )
        .await;
        transport
            .set_session_inventory(
                worker_id,
                fleet_transport::SessionInventory::Reported(vec!["sess-zombie".into()]),
            )
            .await;

        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.orphan_sessions_cancelled, 1);
        assert_eq!(summary.unclaimed_sessions_found, 0);

        let sent = transport.cancel_requests().await;
        assert_eq!(sent.len(), 1, "취소는 정확히 한 번 나가야 한다");
        assert_eq!(sent[0].known_session.as_deref(), Some("sess-zombie"));
        assert_eq!(sent[0].worker_id, Some(worker_id));
        assert_eq!(
            sent[0].task_id, task_id,
            "합성한 id가 아니라 그 세션을 연 Task의 진짜 id여야 한다 — \
             전송 계층의 1단계 조회가 task_id로 세션을 찾기 때문이다"
        );
    }

    /// 살아 있는 Task의 세션은 건드리지 않는다.
    #[tokio::test]
    async fn a_session_of_a_running_task_is_never_cancelled() {
        let worker = make_worker("running");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let task = make_dispatched_task_with_session(
            "still running",
            worker_id,
            chrono::Duration::seconds(120),
            "sess-live",
        );
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher, transport) = setup_with_transport(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, worker.endpoint.clone())],
        )
        .await;
        transport
            .set_session_inventory(
                worker_id,
                fleet_transport::SessionInventory::Reported(vec!["sess-live".into()]),
            )
            .await;

        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.orphan_sessions_cancelled, 0);
        assert!(
            transport.cancel_requests().await.is_empty(),
            "살아 있는 실행에 취소를 보냈다"
        );
    }

    /// **어느 Task도 지목하지 않는 세션은 죽이지 않는다.**
    ///
    /// 이 부정 단정이 이 스윕에서 가장 중요하다. `None`이 나오는 경우가 셋이고
    /// 그중 둘은 죽이면 안 되는 것이다 — dispatch가 진행 중이라
    /// `acp_session_id`가 아직 커밋되지 않았거나, 다른 제어면이 연 세션이거나.
    /// "주인이 없으니 고아"로 접는 구현은 그 둘을 죽인다.
    #[tokio::test]
    async fn a_session_no_task_claims_is_reported_but_left_running() {
        let worker = make_worker("holds-something-unknown");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let (state, dispatcher, transport) = setup_with_transport(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, worker.endpoint.clone())],
        )
        .await;
        transport
            .set_session_inventory(
                worker_id,
                fleet_transport::SessionInventory::Reported(vec!["sess-nobodys".into()]),
            )
            .await;

        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.unclaimed_sessions_found, 1);
        assert_eq!(summary.orphan_sessions_cancelled, 0);
        assert!(
            transport.cancel_requests().await.is_empty(),
            "누구의 것인지 모르는 실행을 죽였다 — 진행 중인 dispatch가 그 모양이다"
        );

        // 감사에 남아야 한다. 손대지 않는 처분에서 **유일한** 산출물이므로,
        // 이것이 없으면 운영자는 그런 세션이 있다는 사실조차 알 수 없다.
        let events = state
            .store
            .list_audit_events(&fleet_core::AuditFilter {
                action: Some(fleet_core::audit::action::CONTROL_UNCLAIMED_SESSION.to_string()),
                ..Default::default()
            })
            .await
            .expect("list audit");
        assert_eq!(events.len(), 1, "받은 값 {events:?}");
    }

    /// 인벤토리를 주지 않는 워커에서는 이 스윕도 아무것도 하지 않는다.
    /// `Undeclared`를 빈 목록으로 읽는 구현은 순회할 대상이 없어 조용히
    /// 통과하지만, `Reported(vec![])`로 읽는 구현과 구분하기 위해 고정한다.
    #[tokio::test]
    async fn an_undeclared_worker_contributes_no_orphans() {
        let worker = make_worker("silent");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let mut task = make_dispatched_task_with_session(
            "ended",
            worker_id,
            chrono::Duration::seconds(120),
            "sess-x",
        );
        task.status = TaskStatus::Cancelled {
            reason: "r".into(),
            cancelled_at: chrono::Utc::now(),
        };
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher, transport) = setup_with_transport(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, worker.endpoint.clone())],
        )
        .await;

        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.orphan_sessions_cancelled, 0);
        assert_eq!(summary.unclaimed_sessions_found, 0);
        assert!(transport.cancel_requests().await.is_empty());
    }

    /// 설정으로 끌 수 있다. 워커의 실행을 멈추는 유일한 자동 경로이므로
    /// 운영자가 원인을 찾는 동안 이것부터 끌 수 있어야 한다.
    #[tokio::test]
    async fn the_orphan_sweep_can_be_turned_off() {
        let worker = make_worker("opt-out");
        let worker_id = worker.id;
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();

        let mut task = make_dispatched_task_with_session(
            "ended",
            worker_id,
            chrono::Duration::seconds(120),
            "sess-zombie",
        );
        task.status = TaskStatus::Cancelled {
            reason: "r".into(),
            cancelled_at: chrono::Utc::now(),
        };
        store.insert_task(&task).await.unwrap();

        let (state, dispatcher, transport) = setup_with_transport(
            store.clone() as Arc<dyn Store>,
            vec![MockWorker::new(worker_id, worker.endpoint.clone())],
        )
        .await;
        transport
            .set_session_inventory(
                worker_id,
                fleet_transport::SessionInventory::Reported(vec!["sess-zombie".into()]),
            )
            .await;

        let cfg = ReconcileConfig {
            reap_orphan_sessions: false,
            ..ReconcileConfig::default()
        };
        let summary = Reconciler::new(state.clone(), dispatcher, cfg)
            .reconcile_once()
            .await;

        assert_eq!(summary.orphan_sessions_cancelled, 0);
        assert!(transport.cancel_requests().await.is_empty());
    }

    // ── 확인되지 않은 Agent 명령 (로드맵 `#70` 게이트 3 — ACK 유실) ──────
    //
    // `031`이 만든 `command_generation`/`last_acked_generation`으로 "확인됐는가"는
    // 알 수 있었지만, 그 값으로 **판정하는 코드가 한 곳도 없었다**. 아래 넷이
    // 그 판정과, 그 판정이 **해서는 안 되는 일**을 고정한다.

    /// 미확인 명령을 들고 배정된 Agent를 만든다.
    async fn seed_unacked_agent(
        store: &Arc<MemStore>,
        worker_id: WorkerId,
        unacked_for: chrono::Duration,
    ) -> fleet_core::AgentId {
        let mut agent = fleet_core::Agent::new(fleet_core::ProjectId::new(), "stuck");
        agent.worker_id = Some(worker_id);
        agent.assigned_at = Some(chrono::Utc::now());
        agent.desired_status = fleet_core::AgentDesiredStatus::Running;
        agent.command_generation = 1;
        agent.last_acked_generation = 0;
        agent.command_issued_at = Some(chrono::Utc::now() - unacked_for);
        let id = agent.id;
        store.create_agent(&agent).await.unwrap();
        id
    }

    async fn unacked_audits(store: &dyn Store) -> Vec<fleet_core::AuditEvent> {
        store
            .list_audit_events(&fleet_core::AuditFilter {
                action: Some(fleet_core::audit::action::AGENT_COMMAND_UNACKED.to_string()),
                ..Default::default()
            })
            .await
            .expect("list audit")
    }

    #[tokio::test]
    async fn a_command_unacked_past_the_threshold_is_reported() {
        let worker = make_worker("never-picks-up");
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();
        let agent_id = seed_unacked_agent(&store, worker.id, chrono::Duration::seconds(600)).await;

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;
        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.unacked_commands_found, 1);
        let events = unacked_audits(state.store.as_ref()).await;
        assert_eq!(events.len(), 1, "받은 값 {events:?}");
        assert_eq!(
            events[0].target_id.as_deref(),
            Some(agent_id.to_string().as_str())
        );
    }

    /// **재발행하지 않는다.** 게이트 문언의 "자동 중복 실행 없이"가 요구하는
    /// 자리가 여기다 — 확인되지 않은 명령을 자동으로 다시 보내면 Worker가 첫
    /// 명령을 늦게 집어갔을 때 같은 Agent가 두 번 뜬다. 세대가 오르면 그것이
    /// 곧 재발행이므로, 세대가 그대로인지를 본다.
    #[tokio::test]
    async fn an_unacked_command_is_never_reissued() {
        let worker = make_worker("silent");
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();
        let agent_id = seed_unacked_agent(&store, worker.id, chrono::Duration::seconds(600)).await;

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;
        Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        let after = state.store.get_agent(agent_id).await.unwrap().unwrap();
        assert_eq!(
            after.command_generation, 1,
            "세대가 올랐다면 명령을 다시 보냈다는 뜻이고, 그것이 중복 실행의 경로다"
        );
        assert_eq!(
            after.last_acked_generation, 0,
            "확인을 가짜로 만들면 안 된다"
        );
    }

    /// 같은 세대는 한 번만 보고한다. 매 tick 남기면 같은 사실이 감사를 채워
    /// 다른 기록을 덮는다.
    #[tokio::test]
    async fn the_same_generation_is_reported_once() {
        let worker = make_worker("silent-twice");
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();
        seed_unacked_agent(&store, worker.id, chrono::Duration::seconds(600)).await;

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;
        let reconciler = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default());
        let first = reconciler.reconcile_once().await;
        let second = reconciler.reconcile_once().await;

        assert_eq!(first.unacked_commands_found, 1);
        assert_eq!(
            second.unacked_commands_found, 0,
            "두 번째 tick이 또 보고했다"
        );
        assert_eq!(unacked_audits(state.store.as_ref()).await.len(), 1);
    }

    /// 임계 시간 안이면 보고하지 않는다 — 다음 heartbeat을 기다리는 중인
    /// 정상 Agent가 여기 걸리면 그 기록은 소음이 된다.
    ///
    /// **발행 시각을 모르는 Agent도 보고하지 않는다**(042 이전 행). 이쪽이
    /// 더 중요한 단정이다: `None`을 "오래됐다"로 읽는 구현은 마이그레이션
    /// 직후에 모든 미확인 Agent를 한꺼번에 쏟아낸다.
    #[tokio::test]
    async fn a_fresh_or_undated_command_is_not_reported() {
        let worker = make_worker("recent");
        let store = Arc::new(MemStore::new());
        store.upsert_worker(&worker).await.unwrap();
        seed_unacked_agent(&store, worker.id, chrono::Duration::seconds(10)).await;

        // 발행 시각을 모르는 Agent.
        let mut undated = fleet_core::Agent::new(fleet_core::ProjectId::new(), "undated");
        undated.worker_id = Some(worker.id);
        undated.assigned_at = Some(chrono::Utc::now());
        undated.desired_status = fleet_core::AgentDesiredStatus::Running;
        undated.command_generation = 1;
        undated.last_acked_generation = 0;
        undated.command_issued_at = None;
        store.create_agent(&undated).await.unwrap();

        let (state, dispatcher) = setup(store.clone() as Arc<dyn Store>, vec![]).await;
        let summary = Reconciler::new(state.clone(), dispatcher, ReconcileConfig::default())
            .reconcile_once()
            .await;

        assert_eq!(summary.unacked_commands_found, 0);
        assert!(unacked_audits(state.store.as_ref()).await.is_empty());
    }
}
