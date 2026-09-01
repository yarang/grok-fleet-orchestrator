//! 워커 선택 알고리즘.
//!
//! 선택 순서:
//! 0. liveness 필터 (로드맵 #70 — liveness가 확인되지 않은 워커 제외.
//!    아래 "on_demand 워커를 후보에서 빼는 이유" 설명 참고)
//! 1. 라벨 매칭 필터 (`required_labels`)
//! 2. 모델 매칭 필터 (`task.model` — 지정된 경우 `labels["model"]`이 정확히
//!    일치하는 워커만 후보로 남긴다. 지정하지 않으면 기존과 동일)
//! 3. Credential 매칭 필터 (로드맵 #71 — `task.model`이 지정된 경우
//!    `Store::get_worker_credential(worker.name, model)`이 `Some`을 반환하는
//!    워커만 후보로 남긴다. 아래 "credential 필터 기준 필드" 설명 참고)
//! 4. 회로 차단된 워커 제외
//! 5. 용량 필터 (동시 상한에 도달한 워커 제외 — 아래 "부하의 출처" 설명 참고)
//! 6. `server_hint`가 있으면 해당 워커 (없거나 사용 불가면 에러, 폴백 안 함)
//! 7. 없으면 least-loaded (부하 최소)
//!
//! ## 부하의 출처: store 원장 (`Worker::active_tasks` 아님) — 로드맵 #67 3단계
//!
//! 5번과 7번은 **같은 숫자**를 쓴다:
//! `Store::count_dispatched_tasks_by_worker()`가 반환하는, 오케스트레이터가
//! 직접 기록한 `Dispatched` 행의 워커별 개수다.
//!
//! 예전에는 둘 다 `Worker::active_tasks`를 읽었다. 그 필드는 워커의 하트비트
//! 요청 본문(`fleet-api`의 `handlers.rs`)으로만 갱신되며 dispatch 시점에는
//! 오케스트레이터가 건드리지 않는다. 결과적으로 (a) 하트비트 주기(기본 15초)
//! 만큼 낡은 값을 읽어 과다 dispatch가 나고, (b) 워커가 신고하는 값이라
//! 위조할 수 있었다. 특히 0을 신고하는 워커는 5번 필터를 통과하는 데 그치지
//! 않고 7번 정렬에서 **가장 먼저 선택**됐다 — 그래서 필터만 고치는 것으로는
//! 부족하고 정렬 키까지 같이 옮겨야 했다.
//!
//! 이 교체의 선행 조건은 #67 2단계(worker incarnation)였다. 그 전에는 같은
//! `--name`으로 재시작한 워커가 `worker_id`를 재사용해서 이전 화신의 in-flight
//! 작업이 영원히 `Dispatched`로 남았고, 그러면 이 카운트가 영구히 과대계상되어
//! 그 워커는 다시는 선택되지 않았을 것이다.
//!
//! **남아 있는 한계**: 카운트를 읽고 dispatch하기까지가 원자적이지 않다.
//! 동시에 도는 `select()` 둘이 같은 N을 읽고 둘 다 보낼 수 있다. 최종
//! 방어선은 transport의 세마포어(`acp_transport.rs`)로, 상한을 넘긴 dispatch는
//! `TransportError::WorkerAtCapacity`로 거부된다.
//!
//! 예전에는 이 자리에 "슬롯을 CAS로 선점하는 것은 #67의 나머지
//! 절반(`worker_execution_lease`)이며 아직이다"라고 적혀 있었다. **그 테이블은
//! 만들지 않기로 했다**(2026-09-01, `#67` 게이트 ①-B — 근거는
//! `docs/architecture/control-plane-authority-and-failover.md`의 범위 정정).
//! 그러므로 이 한계는 "아직 오지 않은 구현을 기다리는 것"이 아니라 **현재
//! 설계가 받아들인 것**이다. 여기서 CAS가 필요한 대상은 Task dispatch의 슬롯인데,
//! ①-B가 술어를 건 것은 Agent **명령 발행**이라 같은 창을 닫지 않는다.
//!
//! ## Credential 필터 기준 필드: `task.model` (`task.resolved_model` 아님)
//!
//! 로드맵 #71 설계 노트는 `task.resolved_model` 기준 필터링을 제안했으나,
//! 실제 코드를 대조해보면 두 가지 이유로 `task.model`이 맞는 기준이다:
//! 1. `dispatcher.rs`의 `DispatchRequest.model`(워커에 실제로 전달되는 필드)은
//!    `task.model.clone()`이지 `task.resolved_model`이 아니다 — 즉
//!    "실행에 실제로 쓰이는" 필드는 `task.model`이다.
//! 2. `HeuristicTaskRouter::resolve_routing`은 사용자가 `model`을 지정하지
//!    않아도 프로파일 휴리스틱으로 항상 `resolved_model`을 채운다
//!    (`Dispatcher::submit`의 0단계에서 무조건 호출됨). 따라서
//!    `resolved_model`을 기준으로 삼으면 사용자가 model을 지정하지 않은
//!    일반 태스크까지 credential 보유 워커로 강제 제한하게 되어(모든
//!    fleet가 사실상 전 모델에 credential을 프로비저닝해야 함) 문제
//!    배경(사용자가 명시적으로 model을 지정한 경우)의 범위를 크게 벗어나고
//!    기존 동작(및 기존 테스트 스위트 대부분)을 깨뜨린다.
//!
//! ## on_demand 워커를 후보에서 빼는 이유 (로드맵 #70 게이트 5)
//!
//! `WorkerLivenessMode::OnDemand`는 idle 시 heartbeat을 보내지 않는 모드이고,
//! `HealthChecker`는 그 사실을 알기 때문에 이런 워커를 **강등하지 않는다**
//! (`health.rs`의 on_demand skip). 한편 워커 등록 시점의
//! `fleet-api`의 `build_worker`는 liveness_mode와 무관하게 `WorkerStatus::Online`을 쓴다.
//!
//! 두 사실을 합치면, on_demand 워커는 **한 번 등록되면 프로세스가 죽어 있어도
//! 영구히 `Online`으로 남는다**. 그 상태에서 이 selector가 `Online`만 보고
//! 후보로 삼으면, liveness를 확인할 수단이 없는 워커에 계속 dispatch하게 된다
//! — `docs/architecture/observability-and-reconciliation.md`가 "on-demand
//! Worker는 probe 성공 전까지 `Unchecked`이며 dispatch되지 않는다"로 요구하는
//! 것과 정반대다. `worker.rs`의 `WorkerLivenessMode::OnDemand` 문서와
//! `handlers.rs`가 렌더링하는 worker.toml의 "아직 프로덕션에서 쓰지 말 것"
//! 경고도 이 배정이 범위 밖이라고 적어 왔지만, 강제하는 코드는 없었다.
//!
//! 그래서 여기서 후보에서 뺀다. **이것은 영구 규칙이 아니라 상태 기계의 안전한
//! 절반이다** — dispatch 직전 ACP probe(로드맵 #67 의존)가 들어오면 이 필터는
//! "probe 성공한 on_demand 워커는 후보에 포함"으로 바뀌어 `unchecked → probe →
//! dispatch` 흐름이 완성된다. probe가 없는 지금 선택지는 "확인 없이 보낸다"와
//! "보내지 않는다" 둘뿐이고, 후자가 안전한 쪽이다.
//!
//! 별도의 `Unchecked` 워커 상태를 새로 만들지는 않았다. probe가 없는 한 그
//! 상태에서 빠져나올 방법이 없어 도달했다가 영영 못 나오는 상태가 되기
//! 때문이다. 상태를 늘리는 대신 이미 있는 `liveness_mode`를 판단 근거로 쓴다.

use std::sync::Arc;

use thiserror::Error;

use fleet_core::{
    Agent, AgentDesiredStatus, AgentObservedStatus, Task, WorkerId, WorkerLivenessMode,
    WorkerStatus,
};
use fleet_store::Store;

use crate::breaker::{BreakerRegistry, BreakerState};

/// 워커 선택 실패.
#[derive(Debug, Error)]
pub enum SelectionError {
    #[error("no online worker matches the required labels")]
    NoMatchingLabels,

    #[error("no worker is currently online")]
    AllOffline,

    #[error("hinted worker '{0}' not found")]
    HintedNotFound(String),

    #[error("hinted worker '{0}' is offline or circuit-open (not falling back, per user intent)")]
    HintedUnavailable(String),

    #[error("no online worker is labeled for model '{0}'")]
    NoWorkerForModel(String),

    #[error("no online worker holds a credential for model '{0}'")]
    NoWorkerForCredential(String),

    /// liveness가 확인되지 않은 워커만 남아 후보가 소진됨 (로드맵 #70).
    ///
    /// 현재로서는 `on_demand` 워커가 여기에 해당한다 — heartbeat을 보내지
    /// 않아 `Online` 표시를 신뢰할 수 없고, 이를 확인할 probe는 아직 없다
    /// (로드맵 #67 의존). `AllOffline`과 구분하는 이유는 운영자가 볼 원인이
    /// 다르기 때문이다: 저쪽은 "워커를 켜라"이고, 이쪽은 "probe가 구현될
    /// 때까지 이 워커는 배정 대상이 아니다"이다.
    #[error("no dispatchable worker: all candidates are on-demand and unprobed")]
    AllUnprobed,

    /// 살아 있고 자격도 맞는 워커가 있었지만 전부 동시 상한에 도달 (로드맵 #67 3단계).
    ///
    /// 3단계 전에는 이 갈래가 사실상 도달 불가였다. 용량 판단이 워커 자기보고
    /// `active_tasks`를 읽었고 그 값은 하트비트 사이에 0 쪽으로 낡아 있어서
    /// 필터가 거의 걸리지 않았기 때문이다. store 파생 카운트는 dispatch 즉시
    /// 올라가므로 **포화가 후보를 비우는 흔한 경로**가 된다. 이 변형이 없으면
    /// 운영자는 그때마다 `AllOffline`("no worker is currently online")을 보게
    /// 되고, 멀쩡히 돌고 있는 워커를 껐다 켜는 잘못된 대응을 하게 된다.
    #[error("all candidate workers are at their concurrency limit")]
    AllAtCapacity,

    /// 힌트된 워커가 살아 있지만 동시 상한에 도달 (로드맵 #67 3단계).
    ///
    /// `HintedUnavailable`("offline or circuit-open")과 나누는 이유는 위와 같다 —
    /// 운영자가 할 일이 다르다. 저쪽은 워커를 살리는 것이고, 이쪽은 기다리거나
    /// `max_concurrent`를 올리는 것이다.
    #[error("hinted worker '{0}' is at its concurrency limit (not falling back, per user intent)")]
    HintedAtCapacity(String),

    /// `tasks.agent_id`가 지목한 Agent 행이 없다 (로드맵 #49 2단계).
    ///
    /// 없는 Agent는 **제출에서** 거절되므로 정상 경로로는 도달하지 않는다
    /// (현재 코드베이스에 `DELETE FROM agents`가 없어 제출 후 사라지지도
    /// 않는다). 그래도 분기를 두는 것은 selector의 계약이 "제출 검증을
    /// 통과한 입력"에만 성립하게 두지 않기 위해서다 — 조회 실패와 달리 이쪽은
    /// 진짜로 행이 없다는 뜻이므로 둘을 같은 이름으로 보고하면 안 된다.
    #[error("task's agent '{0}' no longer exists")]
    AgentNotFound(String),

    /// Agent가 아직 어떤 Worker에도 놓이지 않았다 (`worker_id IS NULL`).
    ///
    /// `7009e4b` 이후 이것은 **회복 가능한 정상 상태**이지 손상이 아니다.
    /// 그래서 운영자가 할 일은 Agent를 고치는 것이 아니라 배정 회복을
    /// 기다리는 것이며, 이 구분이 `AgentNotFound`와 나누는 이유다.
    #[error("task's agent '{0}' is not placed on any worker yet")]
    AgentUnplaced(String),

    /// Agent가 돌 의도가 없거나(`desired = Stopped`) 못 떴다(`observed = Failed`).
    ///
    /// 둘을 한 이름으로 묶은 것은 운영자의 다음 동작이 같기 때문이다 — 그
    /// Agent를 다시 Running으로 만드는 것. 어느 쪽이었는지는 Agent 화면이
    /// `observed_reason`과 함께 보여 준다.
    #[error("task's agent '{0}' is not running")]
    AgentNotRunning(String),

    /// 시작을 지시했지만 Worker가 아직 아무것도 보고하지 않았다
    /// (`Agent::start_pending`).
    ///
    /// `AllUnprobed`와 같은 계열이다 — **확인되지 않은 것은 배정 대상이
    /// 아니다**. `AgentNotRunning`과 나누는 이유는 운영자가 할 일이 다르기
    /// 때문이다: 저쪽은 Agent를 다시 띄우는 것이고, 이쪽은 ACK가 오기를
    /// 기다리는 것(또는 Worker가 명령을 집어가지 못하는 이유를 보는 것)이다.
    #[error("task's agent '{0}' has been told to start but has not reported yet")]
    AgentNotObserved(String),

    /// Agent가 놓인 Worker가 필터를 통과하지 못했다 (오프라인·차단·라벨 불일치 등).
    ///
    /// 폴백하지 않는 이유는 `HintedUnavailable`과 같다 — 지목은 폴백을 막을 뿐
    /// 필터를 무시하는 권한이 아니고, 다른 Worker로 보내면 그 Agent가 없는
    /// 곳으로 Task를 보내는 것이 된다.
    #[error(
        "the worker hosting agent '{0}' is not dispatchable (not falling back, per user intent)"
    )]
    AgentWorkerUnavailable(String),

    /// Agent가 놓인 Worker가 동시 상한에 도달했다.
    ///
    /// `AgentWorkerUnavailable`과 나누는 이유는 `HintedAtCapacity`가
    /// `HintedUnavailable`과 나뉘는 이유와 같다 — 저쪽은 Worker를 살리는
    /// 것이고, 이쪽은 기다리거나 `max_concurrent`를 올리는 것이다.
    #[error("the worker hosting agent '{0}' is at its concurrency limit (not falling back, per user intent)")]
    AgentWorkerAtCapacity(String),
}

/// 워커 선택기.
pub struct WorkerSelector {
    store: Arc<dyn Store>,
    breakers: Arc<BreakerRegistry>,
}

impl WorkerSelector {
    pub fn new(store: Arc<dyn Store>, breakers: Arc<BreakerRegistry>) -> Self {
        Self { store, breakers }
    }

    /// 작업에 적합한 워커를 선택.
    pub async fn select(&self, task: &Task) -> Result<WorkerId, SelectionError> {
        // 1. 온라인 워커 목록 조회
        let mut candidates = self
            .store
            .list_workers(&fleet_core::WorkerFilter {
                status: Some(WorkerStatus::Online),
                ..Default::default()
            })
            .await
            .map_err(|e| {
                tracing::error!(target: "fleet::selector", error = %e, "store error");
                SelectionError::AllOffline
            })?;

        if candidates.is_empty() {
            return Err(SelectionError::AllOffline);
        }

        // 1.5. liveness 필터 (로드맵 #70) — `on_demand` 워커는 heartbeat을 보내지
        // 않으므로 `Online` 표시가 실제 생존을 뜻하지 않는다. probe(로드맵 #67)가
        // 들어오기 전까지 후보에서 제외한다. 모듈 최상단 "on_demand 워커를
        // 후보에서 빼는 이유" 참고.
        //
        // 라벨/모델 필터보다 **먼저** 건다: liveness는 워커가 이 작업에 적합한지
        // 이전에 애초에 배정 가능한 대상인지의 문제이고, 순서를 뒤로 미루면
        // "라벨이 안 맞아서 실패"처럼 원인이 잘못 보고된다.
        candidates.retain(|w| w.liveness_mode != WorkerLivenessMode::OnDemand);

        if candidates.is_empty() {
            return Err(SelectionError::AllUnprobed);
        }

        // 2. 라벨 매칭 필터
        candidates.retain(|w| {
            task.required_labels
                .iter()
                .all(|lbl| w.labels.contains_key(lbl))
        });

        if candidates.is_empty() {
            return Err(SelectionError::NoMatchingLabels);
        }

        // 2.5. 모델 매칭 필터 — task.model이 지정된 경우에만 적용. `labels["model"]`
        // 값이 정확히 일치하는 워커만 후보로 남긴다. task.model이 None이면 이 단계는
        // 완전히 건너뛰어 기존(모델 필터 도입 이전) 동작과 동일하게 유지한다.
        if let Some(model) = &task.model {
            candidates.retain(|w| w.labels.get("model") == Some(model));

            if candidates.is_empty() {
                return Err(SelectionError::NoWorkerForModel(model.clone()));
            }
        }

        // 2.6. Credential 매칭 필터 (로드맵 #71) — task.model이 지정된 경우에만
        // 적용. `Store::get_worker_credential(worker.name, model)`이 `Some`을
        // 반환하는(= 해당 model의 credential을 실제로 보유한) 워커만 후보로
        // 남긴다. task.model이 None이면 어떤 credential이 필요한지 알 수
        // 없으므로 이 단계 전체를 건너뛴다 — 모델 라벨 필터와 동일한 조건.
        //
        // Store 조회 자체가 실패하면(예: credential을 지원하지 않는 Store
        // 구현) 해당 워커를 "credential 미보유"와 동일하게 후보에서 제외한다
        // — credential 보유 여부를 확인할 수 없는 워커에 dispatch하는 것보다
        // 안전한 방향(fail-safe)이다.
        if let Some(model) = &task.model {
            let mut with_credential = Vec::with_capacity(candidates.len());
            for w in candidates {
                match self.store.get_worker_credential(&w.name, model).await {
                    Ok(Some(_)) => with_credential.push(w),
                    Ok(None) => {}
                    Err(e) => {
                        tracing::error!(
                            target: "fleet::selector",
                            error = %e, worker = %w.name, model = %model,
                            "store error checking worker credential — excluding worker from candidates"
                        );
                    }
                }
            }
            candidates = with_credential;

            if candidates.is_empty() {
                return Err(SelectionError::NoWorkerForCredential(model.clone()));
            }
        }

        // 3. 회로 차단된 워커 제외
        candidates.retain(|w| !self.breakers.state_of(w.id).is_open());

        // 3.5. (Phase 8.4, 로드맵 #67 3단계) 용량이 없는 워커 제외.
        //
        // 부하의 근거는 워커 자기보고 `Worker::active_tasks`가 아니라
        // 오케스트레이터 자신의 원장(`Dispatched` 행)이다. 자기보고는
        //   (a) 하트비트로만 갱신되어 최대 health interval(기본 15초)만큼 낡고,
        //   (b) 워커가 위조할 수 있으며,
        //   (c) 0을 신고하면 이 필터를 통과할 뿐 아니라 아래 5번의 least-loaded
        //       정렬에서 **가장 먼저 선택**된다 — 필터만 고쳐서는 부족하다.
        // 그래서 이 카운트는 필터와 정렬 **양쪽**에 쓰인다.
        //
        // 여기서 실패하면 fail-open하지 않는다. 카운트를 못 읽으면 누구에 대해서도
        // 용량을 판정할 수 없으므로, 1번의 `list_workers` 실패와 같은 취급을 한다.
        let load = self
            .store
            .count_dispatched_tasks_by_worker()
            .await
            .map_err(|e| {
                tracing::error!(target: "fleet::selector", error = %e, "dispatched-count store error");
                SelectionError::AllOffline
            })?;

        let mut at_capacity: Vec<String> = Vec::new();
        let mut at_capacity_ids: Vec<WorkerId> = Vec::new();
        candidates.retain(|w| {
            if load.get(&w.id).copied().unwrap_or(0) < w.max_concurrent {
                true
            } else {
                at_capacity.push(w.name.clone());
                at_capacity_ids.push(w.id);
                false
            }
        });

        // 4. agent_id 라우팅 (폴백 없음, 로드맵 #49 2단계).
        //
        // `server_hint`와 **같은 자리·같은 의미**다: 필터를 전부 통과한 뒤에
        // 좁히고, 좁힌 결과가 비면 폴백하지 않는다. 지목이 필터를 무시하는
        // 권한이 되면 오프라인·포화·차단된 Worker로 Task를 보내게 된다
        // (`on_demand_worker_cannot_be_forced_by_server_hint` 참조).
        //
        // 둘을 동시에 준 요청은 제출에서 거절되므로 여기서 순서는 무의미하다.
        if let Some(agent_id) = task.agent_id {
            let agent = match self.store.get_agent(agent_id).await {
                Ok(Some(a)) => a,
                Ok(None) => return Err(SelectionError::AgentNotFound(agent_id.to_string())),
                Err(e) => {
                    // fail-open하지 않는다(`count_dispatched_tasks_by_worker`와
                    // 같은 관례). 조회 실패를 "그런 Agent 없음"으로 보고하면
                    // 운영자가 있지도 않은 삭제를 쫓는다.
                    tracing::error!(
                        target: "fleet::selector",
                        error = %e, agent = %agent_id,
                        "store error loading the task's agent — refusing to dispatch"
                    );
                    return Err(SelectionError::AllOffline);
                }
            };
            agent_dispatchable(&agent)?;
            let Some(worker_id) = agent.worker_id else {
                return Err(SelectionError::AgentUnplaced(agent.name));
            };
            return match candidates.iter().find(|w| w.id == worker_id) {
                Some(w) => Ok(w.id),
                None if at_capacity_ids.contains(&worker_id) => {
                    Err(SelectionError::AgentWorkerAtCapacity(agent.name))
                }
                None => Err(SelectionError::AgentWorkerUnavailable(agent.name)),
            };
        }

        // 4.2. server_hint 처리 (폴백 없음)
        if let Some(hint) = &task.server_hint {
            let hinted = candidates.iter().find(|w| &w.name == hint);
            return match hinted {
                Some(w) => Ok(w.id),
                None if at_capacity.iter().any(|n| n == hint) => {
                    Err(SelectionError::HintedAtCapacity(hint.clone()))
                }
                None => {
                    // 힌트 워커가 아예 존재하는지 확인 (에러 메시지 정확도)
                    let exists = self
                        .store
                        .get_worker_by_name(hint)
                        .await
                        .ok()
                        .flatten()
                        .is_some();
                    if exists {
                        Err(SelectionError::HintedUnavailable(hint.clone()))
                    } else {
                        Err(SelectionError::HintedNotFound(hint.clone()))
                    }
                }
            };
        }

        // 4.5. 용량 때문에 후보가 비었으면 그 사실을 그대로 보고한다.
        // 힌트 갈래보다 뒤에 두는 이유는 힌트가 있을 때는 위에서 이미
        // `HintedAtCapacity`로 더 구체적인 답을 냈기 때문이다.
        if candidates.is_empty() && !at_capacity.is_empty() {
            return Err(SelectionError::AllAtCapacity);
        }

        // 5. least-loaded 정렬 (store 파생 부하, 그 다음 이름)
        candidates.sort_by(|a, b| {
            load.get(&a.id)
                .copied()
                .unwrap_or(0)
                .cmp(&load.get(&b.id).copied().unwrap_or(0))
                .then_with(|| a.name.cmp(&b.name))
        });

        candidates
            .first()
            .map(|w| w.id)
            .ok_or(SelectionError::AllOffline)
    }
}

impl BreakerState {
    /// `Open` 여부 (편의 메서드).
    pub fn is_open(&self) -> bool {
        matches!(self, BreakerState::Open)
    }
}

/// 지목된 Agent가 지금 Task를 받을 수 있는 상태인가 (로드맵 `#49` 2단계).
///
/// **관측되지 않은 Agent는 배정 대상이 아니다.** 이것은 이 파일이 `AllUnprobed`
/// 에서 이미 택한 방향이다 — liveness가 확인되지 않은 `on_demand` 워커를
/// "아마 살아 있을 것"으로 두지 않고 후보에서 뺀다. Agent도 같다:
/// `start_pending()`은 "명령은 냈고 답이 없다"는 뜻이지 "떴다"가 아니며
/// (`fleet_core::Agent::start_pending`의 doc), 뜨지 않은 프로세스로 Task를
/// 보내면 실패가 dispatch 뒤로 밀려 원인이 흐려진다.
///
/// 반대 방향(관측 없음을 통과시키기)의 값은 ACK 경로가 지연되거나 아직
/// 배포되지 않은 환경에서도 Task가 흐른다는 것이다. 그 값을 택하지 않은 것은
/// 위 선례 때문이며, 바꾸려면 이 함수의 `start_pending` 분기 하나만 지우면
/// 된다 — 판정을 여기 한 곳에 모아 둔 이유가 그것이다.
fn agent_dispatchable(agent: &Agent) -> Result<(), SelectionError> {
    if agent.desired_status == AgentDesiredStatus::Stopped
        || agent.observed_status == Some(AgentObservedStatus::Failed)
    {
        return Err(SelectionError::AgentNotRunning(agent.name.clone()));
    }
    if agent.start_pending() {
        return Err(SelectionError::AgentNotObserved(agent.name.clone()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // 명시적 임포트 — fleet_core의 SelectionError를 제외하고 가져옴
    use std::sync::Arc;

    use crate::breaker::BreakerRegistry;
    use crate::selector::{SelectionError, WorkerSelector};
    use async_trait::async_trait;
    use fleet_core::{
        Agent, AgentDesiredStatus, AgentId, AgentObservedStatus, BootstrapToken,
        CircuitBreakerConfig, EventEntry, FleetEvent, ProjectId, Task, TaskFilter, TaskId,
        TaskOutput, TaskPhase, TaskRequest, TaskStatus, TransitionOutcome, Worker, WorkerFilter,
        WorkerHeartbeat, WorkerId, WorkerLivenessMode, WorkerStatus,
    };
    use fleet_store::{Store, StoreError};

    /// 인메모리 mock Store (selector 테스트용).
    struct MockStore {
        workers: std::sync::Mutex<Vec<Worker>>,
        /// (worker_name, model_id) 쌍 — credential 필터 테스트용 fixture.
        /// 로드맵 #71 — 실제 blob 내용은 selector 로직에서 쓰지 않으므로
        /// 존재 여부만 추적한다.
        credentials: std::sync::Mutex<std::collections::HashSet<(String, String)>>,
        /// 워커별 store 파생 부하 fixture (로드맵 #67 3단계).
        ///
        /// 빈 맵을 반환하는 스텁으로 두지 않은 이유: 그러면 모든 테스트가
        /// "부하가 전부 0"이라는 한 경우만 밟게 되고, 그건 교체 이전의
        /// 동작과 구분되지 않는다 — 스위트는 초록인데 새 로직은 한 줄도
        /// 검증되지 않는다. `Worker::active_tasks`와 **따로** 세팅할 수
        /// 있게 해서 selector가 어느 쪽을 읽는지 테스트가 가릴 수 있게 한다.
        dispatched: std::sync::Mutex<std::collections::HashMap<WorkerId, u32>>,
        /// Agent 지목 fixture (로드맵 #49 2단계).
        agents: std::sync::Mutex<Vec<Agent>>,
        /// `true`면 `get_agent`가 조회 자체에 실패한다 — "행이 없다"와
        /// "물어보지 못했다"를 selector가 구분하는지 시험하기 위한 스위치다.
        agent_lookup_fails: std::sync::atomic::AtomicBool,
    }

    impl MockStore {
        fn new(workers: Vec<Worker>) -> Self {
            Self {
                workers: std::sync::Mutex::new(workers),
                credentials: std::sync::Mutex::new(std::collections::HashSet::new()),
                dispatched: std::sync::Mutex::new(std::collections::HashMap::new()),
                agents: std::sync::Mutex::new(Vec::new()),
                agent_lookup_fails: std::sync::atomic::AtomicBool::new(false),
            }
        }

        /// 빌더 헬퍼 — Agent fixture를 추가한다 (로드맵 #49 2단계).
        fn with_agent(self, agent: Agent) -> Self {
            self.agents.lock().unwrap().push(agent);
            self
        }

        /// 빌더 헬퍼 — `get_agent` 조회를 실패시킨다.
        fn with_failing_agent_lookup(self) -> Self {
            self.agent_lookup_fails
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self
        }

        /// 빌더 헬퍼 — 이름으로 지목한 워커의 store 파생 `Dispatched` 건수를 세팅.
        fn with_load(self, worker_name: &str, n: u32) -> Self {
            let id = self
                .workers
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.name == worker_name)
                .unwrap_or_else(|| panic!("no fixture worker named {worker_name}"))
                .id;
            self.dispatched.lock().unwrap().insert(id, n);
            self
        }

        /// 빌더 헬퍼 — 주어진 (worker_name, model_id)에 대한 credential이
        /// 존재하는 것으로 fixture를 채운다.
        fn with_credential(self, worker_name: &str, model_id: &str) -> Self {
            self.credentials
                .lock()
                .unwrap()
                .insert((worker_name.to_string(), model_id.to_string()));
            self
        }
    }

    #[async_trait]
    impl Store for MockStore {
        async fn insert_task_idempotent(
            &self,
            _: &Task,
        ) -> Result<fleet_core::IdempotentInsert, StoreError> {
            Ok(fleet_core::IdempotentInsert::Inserted)
        }

        async fn insert_task(&self, _: &Task) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_task(&self, _: TaskId) -> Result<Option<Task>, StoreError> {
            unimplemented!()
        }
        async fn update_task_status(&self, _: TaskId, _: &TaskStatus) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn compare_and_set_task_status(
            &self,
            _: TaskId,
            _: &[TaskPhase],
            _: &TaskStatus,
            _: Option<&fleet_store::ControlFence>,
            _: fleet_core::TransitionOrigin,
        ) -> Result<TransitionOutcome, StoreError> {
            unimplemented!()
        }
        async fn list_tasks(&self, _: &TaskFilter) -> Result<Vec<Task>, StoreError> {
            unimplemented!()
        }
        async fn count_dispatched_tasks_by_worker(
            &self,
        ) -> Result<std::collections::HashMap<WorkerId, u32>, StoreError> {
            Ok(self.dispatched.lock().unwrap().clone())
        }
        async fn get_agent(&self, id: AgentId) -> Result<Option<Agent>, StoreError> {
            if self
                .agent_lookup_fails
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err(StoreError::Connection("agent lookup exploded".into()));
            }
            Ok(self
                .agents
                .lock()
                .unwrap()
                .iter()
                .find(|a| a.id == id)
                .cloned())
        }
        async fn increment_task_retry_count(&self, _: TaskId) -> Result<u32, StoreError> {
            unimplemented!()
        }
        async fn update_task_checkpoint(
            &self,
            _: TaskId,
            _: Option<&str>,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn delete_task(
            &self,
            _: TaskId,
        ) -> Result<fleet_core::TaskDeleteOutcome, StoreError> {
            unimplemented!()
        }
        async fn bump_worker_incarnation(
            &self,
            _: WorkerId,
        ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StoreError> {
            unimplemented!()
        }
        async fn upsert_worker(&self, _: &Worker) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_worker(&self, _: WorkerId) -> Result<Option<Worker>, StoreError> {
            unimplemented!()
        }
        async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError> {
            let workers = self.workers.lock().unwrap();
            Ok(workers.iter().find(|w| w.name == name).cloned())
        }
        async fn list_workers(&self, filter: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
            let workers = self.workers.lock().unwrap();
            let mut out: Vec<Worker> = workers
                .iter()
                .filter(|w| filter.status.is_none_or(|s| w.status == s))
                .cloned()
                .collect();
            out.sort_by_key(|w| w.registered_at);
            Ok(out)
        }
        async fn delete_worker(&self, _: WorkerId) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn update_worker_heartbeat(
            &self,
            _: WorkerId,
            _: &WorkerHeartbeat,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn append_event(&self, _: &FleetEvent) -> Result<u64, StoreError> {
            unimplemented!()
        }
        async fn list_events(&self, _: u64, _: u32) -> Result<Vec<EventEntry>, StoreError> {
            unimplemented!()
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

        // Phase 8.6: credentials 메서드. `get_worker_credential`은 로드맵 #71
        // credential 필터 테스트에서 `with_credential` fixture와 함께 사용됨.
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
            worker_name: &str,
            model_id: &str,
        ) -> Result<Option<fleet_store::StoredCredential>, StoreError> {
            let has_cred = self
                .credentials
                .lock()
                .unwrap()
                .contains(&(worker_name.to_string(), model_id.to_string()));
            if !has_cred {
                return Ok(None);
            }
            Ok(Some(fleet_store::StoredCredential {
                worker_name: worker_name.to_string(),
                model_id: model_id.to_string(),
                encrypted_blob: "test-encrypted-blob".into(),
                base_url: "https://example.test".into(),
                api_backend: "test-backend".into(),
                context_window: 128_000,
                model_name: None,
                created_at: chrono::Utc::now(),
                rotated_at: chrono::Utc::now(),
            }))
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

    fn make_worker(name: &str, active: u32, labels: &[(&str, &str)]) -> Worker {
        let mut w = Worker::new(name, format!("wss://{name}/ws"));
        w.active_tasks = active;
        for (k, v) in labels {
            w.labels.insert((*k).into(), (*v).into());
        }
        w
    }

    fn make_task(prompt: &str, hint: Option<&str>, labels: &[&str]) -> Task {
        let mut task = Task::from_request(TaskRequest {
            prompt: prompt.into(),
            created_by: "test".into(),
            ..Default::default()
        });
        task.server_hint = hint.map(String::from);
        task.required_labels = labels.iter().map(|s| s.to_string()).collect();
        task
    }

    #[tokio::test]
    async fn select_least_loaded() {
        // 예전 이 테스트는 `assert_ne!(selected, WorkerId::nil())`만 확인해서
        // 정렬이 어떻게 되든 통과했다. store 파생 부하로 옮기면서 실제로
        // 어떤 워커가 뽑히는지를 단정한다.
        let workers = vec![
            make_worker("busy", 0, &[]),
            make_worker("idle", 0, &[]),
            make_worker("medium", 0, &[]),
        ];
        let store = Arc::new(
            MockStore::new(workers)
                .with_load("busy", 5)
                .with_load("medium", 2),
        );
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let task = make_task("work", None, &[]);
        let selected = selector.select(&task).await.unwrap();

        let idle = store.get_worker_by_name("idle").await.unwrap().unwrap();
        assert_eq!(selected, idle.id, "부하가 가장 낮은 워커가 선택되어야 한다");
    }

    /// 로드맵 #67 3단계 — 정렬 키가 자기보고가 아니라 store 원장이어야 한다.
    ///
    /// 두 값을 **정반대로** 세팅한다. 자기보고를 읽으면 `liar`(0을 신고)가,
    /// store를 읽으면 `honest`가 뽑힌다. 교체 전 코드는 이 테스트에서
    /// `liar`를 골랐다 — 거짓말하는 워커가 필터를 통과하는 데 그치지 않고
    /// **우대**받는다는 것이 이 3단계의 핵심 결함이었다.
    #[tokio::test]
    async fn least_loaded_ignores_self_reported_active_tasks() {
        let workers = vec![
            make_worker("honest", 9, &[]), // 자기보고 9, 실제 0
            make_worker("liar", 0, &[]),   // 자기보고 0, 실제 4
        ];
        let store = Arc::new(MockStore::new(workers).with_load("liar", 4));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let selected = selector
            .select(&make_task("work", None, &[]))
            .await
            .unwrap();

        let honest = store.get_worker_by_name("honest").await.unwrap().unwrap();
        assert_eq!(
            selected, honest.id,
            "자기보고 0을 신고한 워커가 아니라 원장상 한가한 워커가 뽑혀야 한다"
        );
    }

    /// 로드맵 #67 3단계 — 용량 필터도 같은 숫자를 읽는다.
    ///
    /// `Worker::new`의 기본 `max_concurrent`만큼 store 부하를 채우면 후보가
    /// 비어야 하고, 그때 `AllOffline`이 아니라 `AllAtCapacity`가 나와야 한다.
    /// 자기보고 값은 0으로 두어, 그 값을 읽는 구현이라면 이 테스트가
    /// `Ok(_)`로 통과해 버리게 만든다.
    #[tokio::test]
    async fn all_at_capacity_is_reported_distinctly() {
        let w = make_worker("solo", 0, &[]);
        let cap = w.max_concurrent;
        let store = Arc::new(MockStore::new(vec![w]).with_load("solo", cap));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let result = selector.select(&make_task("work", None, &[])).await;
        assert!(
            matches!(result, Err(SelectionError::AllAtCapacity)),
            "포화는 오프라인과 구분되어야 한다, got {result:?}"
        );
    }

    /// 로드맵 #67 3단계 — 힌트된 워커의 포화도 `HintedUnavailable`과 구분한다.
    #[tokio::test]
    async fn hinted_at_capacity_is_reported_distinctly() {
        let hinted = make_worker("gpu-1", 0, &[]);
        let cap = hinted.max_concurrent;
        let store = Arc::new(
            MockStore::new(vec![hinted, make_worker("cpu-1", 0, &[])]).with_load("gpu-1", cap),
        );
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let result = selector
            .select(&make_task("work", Some("gpu-1"), &[]))
            .await;
        assert!(
            matches!(result, Err(SelectionError::HintedAtCapacity(ref n)) if n == "gpu-1"),
            "포화된 힌트 워커는 offline/circuit-open과 구분되어야 한다, got {result:?}"
        );
    }

    #[tokio::test]
    async fn select_hint_respected() {
        let workers = vec![make_worker("w1", 0, &[]), make_worker("gpu-1", 0, &[])];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let task = make_task("work", Some("gpu-1"), &[]);
        let selected = selector.select(&task).await.unwrap();
        let chosen = store.get_worker_by_name("gpu-1").await.unwrap().unwrap();
        assert_eq!(selected, chosen.id);
    }

    #[tokio::test]
    async fn select_hint_unavailable_no_fallback() {
        // 힌트 워커가 오프라인인 경우 폴백하지 않고 에러
        let mut offline = make_worker("offline-1", 0, &[]);
        offline.status = WorkerStatus::Offline;
        let workers = vec![offline, make_worker("online-1", 0, &[])];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let task = make_task("work", Some("offline-1"), &[]);
        let result = selector.select(&task).await;
        assert!(matches!(result, Err(SelectionError::HintedUnavailable(_))));
    }

    #[tokio::test]
    async fn select_label_filter() {
        let workers = vec![
            make_worker("cpu-1", 0, &[("arch", "x86_64")]),
            make_worker("gpu-1", 0, &[("gpu", "true"), ("arch", "x86_64")]),
        ];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let task = make_task("train", None, &["gpu"]);
        let result = selector.select(&task).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn select_no_match() {
        let workers = vec![make_worker("cpu-1", 0, &[("arch", "x86_64")])];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let task = make_task("train", None, &["tpu"]);
        let result = selector.select(&task).await;
        assert!(matches!(result, Err(SelectionError::NoMatchingLabels)));
    }

    #[tokio::test]
    async fn select_model_routes_to_matching_worker() {
        let workers = vec![
            make_worker("gemini-1", 0, &[("model", "gemini")]),
            make_worker("glm-1", 0, &[("model", "glm-5")]),
        ];
        // 로드맵 #71 — credential 필터가 이 테스트의 대상 워커를 걸러내지
        // 않도록 gemini-1에 gemini credential을 부여한다.
        let store = Arc::new(MockStore::new(workers).with_credential("gemini-1", "gemini"));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let mut task = make_task("work", None, &[]);
        task.model = Some("gemini".into());

        let selected = selector.select(&task).await.unwrap();
        let gemini_worker = store.get_worker_by_name("gemini-1").await.unwrap().unwrap();
        assert_eq!(
            selected, gemini_worker.id,
            "must route to gemini-labeled worker only"
        );
    }

    #[tokio::test]
    async fn select_no_worker_for_model() {
        let workers = vec![
            make_worker("glm-1", 0, &[("model", "glm-5")]),
            make_worker("plain-1", 0, &[]),
        ];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let mut task = make_task("work", None, &[]);
        task.model = Some("gemini".into());

        let result = selector.select(&task).await;
        match result {
            Err(SelectionError::NoWorkerForModel(m)) => {
                assert!(
                    m.contains("gemini"),
                    "error message should mention model: {m}"
                );
                let msg = SelectionError::NoWorkerForModel(m).to_string();
                assert!(
                    msg.contains("gemini"),
                    "Display impl should mention model: {msg}"
                );
            }
            other => panic!("expected NoWorkerForModel, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn select_model_none_is_backward_compatible() {
        // task.model == None → model 라벨 유무와 무관하게 기존과 동일하게 동작해야 함.
        let workers = vec![
            make_worker("busy", 0, &[("model", "gemini")]),
            make_worker("idle", 0, &[]),
            make_worker("medium", 0, &[("model", "glm-5")]),
        ];
        // 부하는 store 파생 카운트로 심는다 (로드맵 #67 3단계). 예전에는 이
        // 픽스처가 `active_tasks`로 부하를 표현했는데, 정렬 키가 옮겨가면서
        // 그 값은 selector가 읽지 않는다 — 그대로 두면 세 워커가 모두 부하 0
        // 동률이 되어 이름 순으로 "busy"가 뽑힌다.
        let store = Arc::new(
            MockStore::new(workers)
                .with_load("busy", 3)
                .with_load("medium", 2),
        );
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let task = make_task("work", None, &[]); // model: None (default)
        let selected = selector.select(&task).await.unwrap();

        // least-loaded 정책은 그대로 유지 — "idle"이 선택되어야 함
        let idle_worker = store.get_worker_by_name("idle").await.unwrap().unwrap();
        assert_eq!(
            selected, idle_worker.id,
            "model=None must ignore model labels and pick least-loaded, unchanged from prior behavior"
        );
    }

    #[tokio::test]
    async fn select_model_and_required_labels_compose() {
        // required_labels와 model 필터가 AND로 결합되어야 함 (둘 다 만족하는 워커만 남음).
        let workers = vec![
            // gpu 라벨은 있지만 model이 다름 → 제외
            make_worker("gpu-glm", 0, &[("gpu", "true"), ("model", "glm-5")]),
            // model은 맞지만 gpu 라벨이 없음 → 제외
            make_worker("cpu-gemini", 0, &[("model", "gemini")]),
            // 둘 다 만족 → 선택되어야 함
            make_worker("gpu-gemini", 0, &[("gpu", "true"), ("model", "gemini")]),
        ];
        // 로드맵 #71 — credential 필터가 gpu-gemini를 걸러내지 않도록 credential 부여.
        let store = Arc::new(MockStore::new(workers).with_credential("gpu-gemini", "gemini"));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let mut task = make_task("train", None, &["gpu"]);
        task.model = Some("gemini".into());

        let selected = selector.select(&task).await.unwrap();
        let expected = store
            .get_worker_by_name("gpu-gemini")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            selected, expected.id,
            "must satisfy both required_labels AND model filters"
        );
    }

    // ── 로드맵 #71: credential 매칭 필터 ────────────────────────────────

    #[tokio::test]
    async fn select_credential_required_and_present_routes_normally() {
        // 지정된 model의 credential을 가진 worker만 있을 때 정상 dispatch.
        let workers = vec![make_worker("gemini-1", 0, &[("model", "gemini")])];
        let store = Arc::new(MockStore::new(workers).with_credential("gemini-1", "gemini"));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let mut task = make_task("work", None, &[]);
        task.model = Some("gemini".into());

        let selected = selector.select(&task).await.unwrap();
        let gemini_worker = store.get_worker_by_name("gemini-1").await.unwrap().unwrap();
        assert_eq!(selected, gemini_worker.id);
    }

    #[tokio::test]
    async fn select_credential_missing_on_all_candidates_errors() {
        // credential 없는 worker만 있을 때 — 재시도 대상인 NoWorkerForCredential.
        let workers = vec![make_worker("gemini-1", 0, &[("model", "gemini")])];
        let store = Arc::new(MockStore::new(workers)); // credential 미부여
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let mut task = make_task("work", None, &[]);
        task.model = Some("gemini".into());

        let result = selector.select(&task).await;
        match result {
            Err(SelectionError::NoWorkerForCredential(m)) => {
                assert!(m.contains("gemini"), "error should mention model: {m}");
            }
            other => panic!("expected NoWorkerForCredential, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn select_credential_partial_provisioning_routes_to_credentialed_worker() {
        // 일부 worker만 credential을 가진 fleet — credential 있는 worker로만 라우팅.
        let workers = vec![
            make_worker("gemini-1", 0, &[("model", "gemini")]),
            make_worker("gemini-2", 0, &[("model", "gemini")]),
        ];
        // gemini-2만 credential 프로비저닝 완료.
        let store = Arc::new(MockStore::new(workers).with_credential("gemini-2", "gemini"));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let mut task = make_task("work", None, &[]);
        task.model = Some("gemini".into());

        let selected = selector.select(&task).await.unwrap();
        let gemini_2 = store.get_worker_by_name("gemini-2").await.unwrap().unwrap();
        assert_eq!(
            selected, gemini_2.id,
            "must route only to the credentialed worker, ignoring the uncredentialed one"
        );
    }

    #[tokio::test]
    async fn select_no_model_skips_credential_check() {
        // model 미지정 task는 credential 유무와 무관하게 기존처럼 정상 dispatch.
        let workers = vec![make_worker("plain-1", 0, &[])]; // credential 없음
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let task = make_task("work", None, &[]); // model: None

        let selected = selector.select(&task).await.unwrap();
        let plain = store.get_worker_by_name("plain-1").await.unwrap().unwrap();
        assert_eq!(selected, plain.id);
    }

    // ── 로드맵 #70 게이트 5: on_demand 워커는 probe 전까지 dispatch되지 않는다 ──

    /// `make_worker`와 같지만 liveness_mode를 `OnDemand`로 둔다.
    ///
    /// status는 일부러 `Online`으로 남긴다 — `fleet-api`의 `build_worker`가
    /// liveness_mode와 무관하게 `Online`을 쓰므로, 이것이 등록 직후 저장소에
    /// 실제로 들어 있는 모양이다. 이 fixture를 `Offline`으로 만들면 1단계
    /// 필터에 먼저 걸려 정작 검증하려는 1.5단계가 실행되지 않는다.
    fn make_on_demand_worker(name: &str, active: u32, labels: &[(&str, &str)]) -> Worker {
        let mut w = make_worker(name, active, labels);
        w.liveness_mode = WorkerLivenessMode::OnDemand;
        assert_eq!(
            w.status,
            WorkerStatus::Online,
            "fixture must reproduce registration state: on_demand workers are stored as Online"
        );
        w
    }

    #[tokio::test]
    async fn on_demand_worker_is_never_a_dispatch_candidate() {
        // on_demand 워커 하나뿐인 fleet — heartbeat이 없어 Online 표시를
        // 신뢰할 수 없고 probe(로드맵 #67)도 없으므로 배정하지 않는다.
        let workers = vec![make_on_demand_worker("laptop", 0, &[])];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let task = make_task("work", None, &[]);

        match selector.select(&task).await {
            Err(SelectionError::AllUnprobed) => {}
            other => panic!("expected AllUnprobed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn periodic_worker_is_preferred_over_idle_on_demand_worker() {
        // 부하가 더 높아도 heartbeat을 보내는 워커가 선택되어야 한다 —
        // least-loaded 정책보다 liveness 필터가 먼저다. 이 순서가 뒤집히면
        // "가장 한가한 워커"가 사실은 죽어 있는 워커가 된다.
        let workers = vec![
            make_on_demand_worker("idle-laptop", 0, &[]),
            make_worker("busy-server", 0, &[]),
        ];
        // 부하는 store 파생 카운트로 심는다. max_concurrent 기본값이 4이므로
        // 3까지만 — 4 이상이면 용량 필터(3.5단계)에 걸려 liveness 필터가 아닌
        // 다른 이유로 비게 되고, 이 테스트가 검증하려는 순서를 못 보게 된다.
        let store = Arc::new(MockStore::new(workers).with_load("busy-server", 3));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let task = make_task("work", None, &[]);

        let selected = selector.select(&task).await.unwrap();
        let expected = store
            .get_worker_by_name("busy-server")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            selected, expected.id,
            "liveness filter must outrank the least-loaded policy"
        );
    }

    #[tokio::test]
    async fn on_demand_worker_cannot_be_forced_by_server_hint() {
        // server_hint는 폴백을 막을 뿐 필터를 무시하는 권한이 아니다.
        // 여기서는 후보가 통째로 비므로 hint 처리 이전에 AllUnprobed로 끝난다.
        let workers = vec![make_on_demand_worker("laptop", 0, &[])];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let task = make_task("work", Some("laptop"), &[]);

        match selector.select(&task).await {
            Err(SelectionError::AllUnprobed) => {}
            other => panic!(
                "expected AllUnprobed even with an explicit hint, got {:?}",
                other
            ),
        }
    }

    #[tokio::test]
    async fn on_demand_exclusion_is_reported_before_label_mismatch() {
        // 원인 보고의 정확도 검증: on_demand 워커가 라벨도 만족하지 않을 때,
        // 운영자에게 "라벨이 안 맞는다"가 아니라 "probe되지 않았다"가 보여야
        // 한다. 필터 순서를 뒤로 미루면 이 단정이 깨진다.
        let workers = vec![make_on_demand_worker("laptop", 0, &[])];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let task = make_task("work", None, &["gpu"]);

        match selector.select(&task).await {
            Err(SelectionError::AllUnprobed) => {}
            other => panic!("expected AllUnprobed, not a label error, got {:?}", other),
        }
    }

    // ── 로드맵 #49 2단계 — `tasks.agent_id` 라우팅 ──────────────────────────
    //
    // 이 블록이 지키는 계약은 두 가지다: (a) 지목은 필터를 **통과한 뒤에만**
    // 좁힌다(무시하는 권한이 아니다), (b) 실패는 운영자가 할 일이 다른 만큼
    // 서로 다른 이름으로 보고된다. 후자는 컴파일러가 지켜 주지 않으므로 —
    // 여섯 갈래가 전부 `AllOffline` 하나로 접혀도 스위트는 초록이다 —
    // 갈래마다 단정을 남긴다.

    /// 배정된 Worker 위에서 실제로 돌고 있는 Agent fixture.
    fn make_running_agent(name: &str, worker_id: Option<WorkerId>) -> Agent {
        let mut a = Agent::new(ProjectId::new(), name);
        a.worker_id = worker_id;
        a.assigned_at = worker_id.map(|_| chrono::Utc::now());
        a.desired_status = AgentDesiredStatus::Running;
        a.observed_status = Some(AgentObservedStatus::Running);
        a
    }

    fn make_agent_task(agent_id: AgentId) -> Task {
        let mut task = make_task("agent work", None, &[]);
        task.agent_id = Some(agent_id);
        task
    }

    #[tokio::test]
    async fn agent_pin_routes_to_the_agents_worker() {
        // 부하로는 `idle`이 뽑혀야 하는 상황에서 Agent가 `hosting` 위에 있다.
        // 지목이 least-loaded 정렬을 실제로 덮어쓰는지 확인한다 — 두 워커의
        // 부하를 같게 두면 이 테스트는 우연히 통과할 수 있다.
        let workers = vec![make_worker("hosting", 0, &[]), make_worker("idle", 0, &[])];
        let store = Arc::new(MockStore::new(workers).with_load("hosting", 3));
        // `Worker::new`가 id를 새로 뽑으므로 Agent의 배정은 **store 안의**
        // 워커에서 가져와야 한다 — 밖에서 만든 fixture의 id를 쓰면 지목이
        // 아무 워커도 가리키지 않는 채로 테스트가 통과할 수 있다.
        let hosting = store.get_worker_by_name("hosting").await.unwrap().unwrap();
        let agent = make_running_agent("planner", Some(hosting.id));
        let agent_id = agent.id;
        store.agents.lock().unwrap().push(agent);

        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let selected = selector.select(&make_agent_task(agent_id)).await.unwrap();
        assert_eq!(
            selected, hosting.id,
            "지목한 Agent가 놓인 워커로 가야 한다 — 더 한가한 워커가 있어도"
        );
    }

    #[tokio::test]
    async fn agent_pin_rejects_unknown_agent() {
        let store = Arc::new(MockStore::new(vec![make_worker("w1", 0, &[])]));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        match selector.select(&make_agent_task(AgentId::new())).await {
            Err(SelectionError::AgentNotFound(_)) => {}
            other => panic!("expected AgentNotFound, got {:?}", other),
        }
    }

    /// 조회 **실패**는 "행이 없다"가 아니다.
    ///
    /// 둘을 같은 이름으로 접으면 운영자가 일어나지도 않은 삭제를 쫓는다.
    /// `count_dispatched_tasks_by_worker`가 세운 fail-safe 관례와 같은 갈래다.
    #[tokio::test]
    async fn agent_lookup_failure_is_not_reported_as_a_missing_agent() {
        let store =
            Arc::new(MockStore::new(vec![make_worker("w1", 0, &[])]).with_failing_agent_lookup());
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        match selector.select(&make_agent_task(AgentId::new())).await {
            Err(SelectionError::AgentNotFound(_)) => {
                panic!("조회 실패를 'Agent 없음'으로 보고하면 안 된다")
            }
            Err(SelectionError::AllOffline) => {}
            other => panic!("expected AllOffline, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn agent_pin_rejects_stopped_agent() {
        let store = Arc::new(MockStore::new(vec![make_worker("w1", 0, &[])]));
        let w1 = store.get_worker_by_name("w1").await.unwrap().unwrap();
        let mut agent = make_running_agent("planner", Some(w1.id));
        agent.desired_status = AgentDesiredStatus::Stopped;
        let agent_id = agent.id;
        store.agents.lock().unwrap().push(agent);

        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        match selector.select(&make_agent_task(agent_id)).await {
            Err(SelectionError::AgentNotRunning(name)) => assert_eq!(name, "planner"),
            other => panic!("expected AgentNotRunning, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn agent_pin_rejects_failed_agent() {
        let store = Arc::new(MockStore::new(vec![make_worker("w1", 0, &[])]));
        let w1 = store.get_worker_by_name("w1").await.unwrap().unwrap();
        let mut agent = make_running_agent("planner", Some(w1.id));
        agent.observed_status = Some(AgentObservedStatus::Failed);
        let agent_id = agent.id;
        store.agents.lock().unwrap().push(agent);

        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        match selector.select(&make_agent_task(agent_id)).await {
            Err(SelectionError::AgentNotRunning(_)) => {}
            other => panic!("expected AgentNotRunning, got {:?}", other),
        }
    }

    /// `start_pending()` — 시작 명령은 냈는데 아직 아무 보고도 없는 Agent.
    ///
    /// **이 단정이 이 설계의 값 선택 그 자체다.** 여기서 `Ok`를 받게 바꾸는
    /// 것(= `agent_dispatchable`의 `start_pending` 분기를 지우는 것)도 방어
    /// 가능한 선택이지만, 그렇게 하면 아직 존재가 확인되지 않은 프로세스로
    /// Task를 보내게 된다 — `AllUnprobed`가 이미 반대쪽을 택했다.
    #[tokio::test]
    async fn agent_pin_rejects_agent_that_has_not_reported_yet() {
        let store = Arc::new(MockStore::new(vec![make_worker("w1", 0, &[])]));
        let w1 = store.get_worker_by_name("w1").await.unwrap().unwrap();
        let mut agent = make_running_agent("planner", Some(w1.id));
        agent.observed_status = None; // 명령은 냈고(desired=Running) 답이 없다.
        assert!(agent.start_pending(), "fixture가 start_pending이어야 한다");
        let agent_id = agent.id;
        store.agents.lock().unwrap().push(agent);

        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        match selector.select(&make_agent_task(agent_id)).await {
            Err(SelectionError::AgentNotObserved(name)) => assert_eq!(name, "planner"),
            other => panic!("expected AgentNotObserved, got {:?}", other),
        }
    }

    /// 배정되지 않은 Agent는 손상이 아니라 회복 가능한 정상 상태다(`7009e4b`).
    #[tokio::test]
    async fn agent_pin_rejects_unplaced_agent_distinctly() {
        let agent = make_running_agent("planner", None);
        let agent_id = agent.id;
        let store = Arc::new(MockStore::new(vec![make_worker("w1", 0, &[])]).with_agent(agent));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        match selector.select(&make_agent_task(agent_id)).await {
            Err(SelectionError::AgentUnplaced(name)) => assert_eq!(name, "planner"),
            other => panic!("expected AgentUnplaced, got {:?}", other),
        }
    }

    /// 지목은 폴백을 막을 뿐 필터를 무시하는 권한이 아니다.
    ///
    /// `server_hint`의 `on_demand_worker_cannot_be_forced_by_server_hint`와
    /// 같은 계약을 Agent 지목에 대해 다시 세운다. 멀쩡한 워커를 하나 남겨
    /// 두는 것이 핵심이다 — 그게 없으면 후보가 비어 `AllUnprobed`로 빠져
    /// 이 갈래를 밟지 못한다.
    #[tokio::test]
    async fn agent_pin_does_not_fall_back_to_another_worker() {
        let mut on_demand = make_worker("agent-host", 0, &[]);
        on_demand.liveness_mode = WorkerLivenessMode::OnDemand;
        let store = Arc::new(MockStore::new(vec![
            on_demand,
            make_worker("healthy", 0, &[]),
        ]));
        let host = store
            .get_worker_by_name("agent-host")
            .await
            .unwrap()
            .unwrap();
        let agent = make_running_agent("planner", Some(host.id));
        let agent_id = agent.id;
        store.agents.lock().unwrap().push(agent);

        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        match selector.select(&make_agent_task(agent_id)).await {
            Ok(w) => panic!("멀쩡한 워커로 폴백하면 안 된다 (selected {w})"),
            Err(SelectionError::AgentWorkerUnavailable(name)) => assert_eq!(name, "planner"),
            other => panic!("expected AgentWorkerUnavailable, got {:?}", other),
        }
    }

    /// 포화는 오프라인과 다른 이름으로 보고된다 — 운영자가 할 일이 다르다.
    ///
    /// `HintedAtCapacity`/`HintedUnavailable`이 나뉜 이유와 같으며, 이
    /// 갈래는 `AllAtCapacity` 검사보다 **앞**에서 판정돼야 한다(뒤에 두면
    /// 모든 후보가 포화일 때 Agent 이름이 사라진 일반 오류가 나간다).
    #[tokio::test]
    async fn agent_pin_reports_hosting_worker_capacity_distinctly() {
        let host = make_worker("agent-host", 0, &[]);
        let cap = host.max_concurrent;
        let store = Arc::new(MockStore::new(vec![host, make_worker("healthy", 0, &[])]));
        let host_id = store
            .get_worker_by_name("agent-host")
            .await
            .unwrap()
            .unwrap()
            .id;
        store.dispatched.lock().unwrap().insert(host_id, cap);
        let agent = make_running_agent("planner", Some(host_id));
        let agent_id = agent.id;
        store.agents.lock().unwrap().push(agent);

        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        match selector.select(&make_agent_task(agent_id)).await {
            Err(SelectionError::AgentWorkerAtCapacity(name)) => assert_eq!(name, "planner"),
            other => panic!("expected AgentWorkerAtCapacity, got {:?}", other),
        }
    }
}
