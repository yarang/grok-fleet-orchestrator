//! Agent → Worker 배정 (로드맵 `#67` 4a).
//!
//! [Agent provisioning](../../../docs/architecture/agents/provisioning.md)이
//! 정본이다. 이 모듈이 존재하는 이유는 정본의 한 문장에 있다: 명령 봉투는
//! 명령을 받을 *프로세스*를 만들지만 명령이 갈 *방향*은 만들지 못한다.
//! `agents.worker_id`가 그 방향이고, 이 모듈이 그 값을 고른다.
//!
//! # 왜 백그라운드 루프가 아닌가
//!
//! 처음 설계는 [`crate::health::HealthChecker`]를 본뜬 주기 루프였다. 그
//! 모양을 버린 이유는 배정이 **상태가 아니라 결정**이기 때문이다. 헬스체커의
//! 루프가 정당한 것은 그것이 관측한 사실(하트비트 부재)을 계속 반영하기
//! 때문이고, 어느 인스턴스가 돌든 같은 결론에 이른다. 배정은 다르다 —
//! 후보가 여럿이면 두 인스턴스가 다른 답을 내며, 4a에는 옮길 프로세스가
//! 없으므로 "답이 바뀌었으면 다시 정한다"에 대한 정직한 답은 "다시 정하지
//! 않는다"뿐이다. 정상 상태가 "아무것도 하지 않음"인 루프는 할 일이 없는
//! 기계다.
//!
//! 그래서 배정은 **Agent를 만드는 요청 안에서** 한 번 일어난다. 부수 효과가
//! 둘 있고 둘 다 이 선택의 근거다:
//!
//! - 감사 actor가 실재한다. 루프였다면 합성 시스템 주체를 만들어야 했다.
//! - lease 게이트가 필요 없다. 두 오케스트레이터 인스턴스가 같은 Agent 행을
//!   두고 경합할 경로가 없다 — 생성은 유일한 id를 만들고, 재배정은 운영자가
//!   특정 Agent를 지목하는 단일 요청이다. [`crate::reconcile`]과
//!   [`crate::dispatcher`]가 `lease_allows_control()`을 확인하는 것은 그들이
//!   **주기적으로** 남의 행을 고치는 writer이기 때문이며, 여기에는 그 조건이
//!   성립하지 않는다.
//!
//! # 배정 가능한 Worker가 없으면 생성은 그대로 성공한다
//!
//! [`choose_worker`]가 실패해도 Agent는 만들어지고 `worker_id`는 `None`으로
//! 남는다. Agent 정의가 Worker 가용성에 인질로 잡히면 안 되기 때문이다 —
//! Worker가 한 대도 없는 저장소에 Project와 Agent를 미리 정의하는 것은 정상
//! 사용이다. `None`이 영구 상태가 되지 않도록 운영자가 나중에 지목해 배정하는
//! 경로(Dashboard `POST /api/agents/{id}/place`, MCP `fleet_place_agent`)를
//! 같은 증분에 함께 넣었다.
//!
//! # 부하의 출처: 오케스트레이터 원장 (`Worker::active_tasks` 아님)
//!
//! least-loaded 정렬은 [`fleet_store::Store::count_agents_by_worker`]가 세는
//! 배정 Agent 수를 쓴다. Worker 자기보고를 쓰지 않는 이유는 Task 선택이
//! `#67` 3단계에서 같은 판단을 한 것과 같다([`crate::selector`]의 모듈 문서):
//! 자기보고는 하트비트 주기만큼 낡고, 위조 가능하며, 0을 신고하는 Worker는
//! 필터를 통과하는 데 그치지 않고 정렬에서 **가장 먼저 선택**된다.
//!
//! **남아 있는 한계**: 카운트를 읽고 INSERT하기까지가 원자적이지 않다.
//! 동시에 들어온 두 생성 요청이 같은 Worker를 고를 수 있고, 아래의 상한
//! 필터는 **읽은 시점의** 카운트로 판정하므로 둘 다 통과할 수 있다. 슬롯을
//! `workers` 행 잠금 아래에서 선점하는 것은 `#67` 구현 게이트 ①-A-2이며,
//! 그때까지 상한은 정렬을 편향시키는 필터이지 집행되는 불변식이 아니다.
//!
//! # 하드 상한은 Worker가 보고한 값으로만 건다
//!
//! [`crate::selector`]는 `w.max_concurrent`로 용량 필터를 걸지만 그 값은
//! 여기서 쓰지 않는다. `max_concurrent`는 **Task** 동시 실행 상한이고 Agent
//! 프로세스 상한이 아니다. 후자는 `max_agent_processes`이고, Worker가 등록
//! 시점에 자기 설정(`grok.max_agent_processes`)에서 실어 보낸다.
//!
//! 이 값은 **nullable이고 기본값이 없다**. `None`은 "상한이 없다"가 아니라
//! "이 Worker의 상한을 **모른다**"이며, 모르는 상한은 필터하지 않는다. 기본값을
//! 두면 이 필드를 보내지 않는 구버전 Worker에 대해 아는 값을 날조하게 되고,
//! 그 날조는 실제 상한이 더 작은 Worker를 과배정하는 방향으로 틀린다.
//!
//! 모르는 Worker를 배제하지 **않는** 쪽을 고른 근거는 방어선의 순서다. 여기의
//! 상한은 유일한 방어선이 아니라 두 번째다 — Worker의 프로세스 매니저(4c)가
//! 자기 상한을 직접 집행하고 초과분을 `observed_reason='cap_reached'`로
//! 되돌려 보낸다. 따라서 여기서 수를 모르거나 틀리게 알면 결과는 거절된 관측
//! 하나이지 상한 초과가 아니다. 반대로 모르는 Worker를 배제하면 구버전 Worker
//! 하나만 남은 플릿에서 배정이 통째로 멈춘다 — 훨씬 나쁜 실패다.

use chrono::{DateTime, Utc};
use fleet_core::worker::{CircuitState, WorkerLivenessMode, WorkerStatus};
use fleet_core::WorkerId;
use fleet_store::{Store, StoreError};

/// 배정할 Worker를 찾지 못한 이유.
///
/// [`crate::selector::SelectionError`]와 같은 이유로 원인을 구분한다 —
/// 운영자가 취할 행동이 다르다. `AllOffline`은 "Worker를 켜라",
/// `AllUnprobed`는 "probe(`#70`)가 들어올 때까지 이 Worker는 배정 대상이
/// 아니다", `AllCircuitOpen`은 "회로를 닫아라(`fleet_reset_worker_breaker`)"다.
#[derive(Debug, thiserror::Error)]
pub enum PlacementError {
    #[error("no worker is currently online")]
    AllOffline,

    /// `on_demand` Worker는 idle 시 heartbeat을 보내지 않아 `Online` 표시가
    /// 생존을 뜻하지 않는다. 4b의 desired state는 heartbeat에 실려 가므로
    /// 이 Worker에는 **원리적으로 도달하지 않는다** — 명령 전달을 고쳐서
    /// 풀 수 있는 문제가 아니고, dispatch 직전 ACP probe(`#70`)가 푼다.
    ///
    /// 메시지의 "unprobed"를 probed/unprobed 분할로 읽지 말 것. probe가 아직
    /// 없으므로 **모든** on_demand Worker가 unprobed이고, 필터는 probe 여부를
    /// 보지 않고 `liveness_mode`만 본다. `#70`이 probe를 들여오면 그때 두 개념이
    /// 갈라지고 이 필터도 "probe 성공한 on_demand는 포함"으로 바뀐다.
    /// 문구를 [`crate::selector::SelectionError::AllUnprobed`]와 같게 둔 것은
    /// **같은 조건**이기 때문이다 — 같은 사실에 다른 이름을 붙이면 운영자가
    /// 두 표면의 로그를 맞춰 볼 때 서로 다른 원인으로 읽는다.
    #[error("no placeable worker: all candidates are on-demand and unprobed")]
    AllUnprobed,

    #[error("no placeable worker: every online worker has an open circuit")]
    AllCircuitOpen,

    /// 상한을 **보고한** Worker가 모두 가득 찼다. 상한을 보고하지 않은
    /// (`max_agent_processes IS NULL`) Worker는 필터를 통과하므로, 이 오류는
    /// 그런 Worker가 하나도 없을 때만 나온다.
    #[error("no placeable worker: every online worker is at its agent-process cap")]
    AllAtCapacity,

    #[error("store error: {0}")]
    Store(#[from] StoreError),
}

/// Agent 하나를 배정할 Worker를 고른다.
///
/// 순서는 [`crate::selector::WorkerSelector::select`]와 같다(online →
/// liveness → circuit → least-loaded). 라벨·모델·credential 단계가 없는 것은
/// [`fleet_core::Agent`]에 `required_labels`도 `model`도 없기 때문이다 —
/// 그래서 저 함수를 재사용하지 않고 순서만 물려받는다.
pub async fn choose_worker(store: &dyn Store) -> Result<WorkerId, PlacementError> {
    let mut candidates = store
        .list_workers(&fleet_core::WorkerFilter {
            status: Some(WorkerStatus::Online),
            ..Default::default()
        })
        .await?;

    if candidates.is_empty() {
        return Err(PlacementError::AllOffline);
    }

    candidates.retain(|w| w.liveness_mode != WorkerLivenessMode::OnDemand);
    if candidates.is_empty() {
        return Err(PlacementError::AllUnprobed);
    }

    // 회로 상태의 출처가 in-memory `BreakerRegistry`가 아니라 저장된
    // `workers.circuit_state`인 이유: 이 함수의 호출자가 둘(MCP와 Dashboard)
    // 인데 `DashboardState`에는 registry가 없다. 두 표면이 같은 규칙을
    // 집행해야 하므로 둘 다 읽을 수 있는 출처를 쓴다. 그 컬럼은
    // `Dispatcher`가 전이마다 `update_worker_circuit_state`로 반영하며,
    // 여러 오케스트레이터 인스턴스가 공유하는 유일한 회로 관측이기도 하다.
    //
    // `HalfOpen`은 제외하지 않는다 — 탐색 중이라는 뜻이고, dispatch가
    // `is_open()`으로 판정해 HalfOpen을 통과시키는 것과 같은 취급이다.
    //
    // **한계**: 이 컬럼은 in-memory registry보다 쓰기 한 번만큼 늦고,
    // `MemStore`의 `update_worker_circuit_state`는 트레이트 기본 구현
    // (no-op)이라 메모리 Store 기반 테스트에서는 회로가 열리지 않는다.
    candidates.retain(|w| w.circuit_state != CircuitState::Open);
    if candidates.is_empty() {
        return Err(PlacementError::AllCircuitOpen);
    }

    let load = store.count_agents_by_worker().await?;

    // 프로세스 상한 필터 (`#67` 게이트 ①-A). `None`은 통과시킨다 — 모듈 문서의
    // "모르는 상한은 필터하지 않는다"를 집행하는 자리다.
    //
    // 세는 것이 Worker의 실제 프로세스 수가 아니라 **배정된 `agents` 행 수**라는
    // 점이 중요하다. 두 수는 갈린다 — 배정만 되고 아직 뜨지 않은 Agent, 종료
    // 중이지만 아직 사라지지 않은 프로세스가 각각 한쪽에만 잡힌다. 그래서 이
    // 필터가 있어도 Worker 쪽 `cap_reached`는 죽은 코드가 되지 않는다.
    candidates.retain(|w| match w.max_agent_processes {
        Some(cap) => load.get(&w.id).copied().unwrap_or(0) < cap,
        None => true,
    });
    if candidates.is_empty() {
        return Err(PlacementError::AllAtCapacity);
    }

    // 동점은 `registered_at`이 이른 쪽 — `WorkerSelector`와 같은 tiebreak이다.
    // 이 필드가 재시작 시각이 아니라 최초 등록 시각이어야 하는 이유가 여기에도
    // 걸린다: 재시작마다 갱신되면 방금 재시작한 Worker가 동점에서 항상 지거나
    // 항상 이겨 배정이 한쪽으로 쏠린다.
    candidates.sort_by_key(|w| (load.get(&w.id).copied().unwrap_or(0), w.registered_at));

    // `retain` 네 번을 통과했으므로 비어 있지 않다.
    Ok(candidates[0].id)
}

/// 생성 경로용 최선 노력 배정 — 실패해도 Agent 생성을 막지 않는다.
///
/// 실패 사유는 로그로만 남긴다. 호출자에게 돌려주지 않는 이유는 그것으로
/// 할 수 있는 일이 없기 때문이다 — 생성은 계속되어야 하고, 운영자는 응답의
/// `worker_id`가 `null`인 것으로 배정되지 않았음을 본다.
///
/// `WorkerId`만이 아니라 **쌍**을 돌려주는 이유: `worker_id`와 `assigned_at`은
/// DB의 `agents_placement_complete` CHECK가 강제하는 both-or-neither 쌍이고,
/// 한쪽만 돌려주면 호출자가 나머지 절반을 스스로 만들어야 한다 — 그 자리가
/// 곧 반쪽 배정을 만들 수 있는 자리다. 쌍으로 묶으면 그 실수가 타입 수준에서
/// 불가능해지고, 덤으로 두 표면 중 chrono 의존이 없는 `fleet-mcp`도 이 함수를
/// 그대로 쓸 수 있다.
pub async fn place_on_create(store: &dyn Store) -> Option<(WorkerId, DateTime<Utc>)> {
    match choose_worker(store).await {
        Ok(worker_id) => Some((worker_id, Utc::now())),
        Err(e) => {
            tracing::info!(
                target: "fleet::placement",
                reason = %e,
                "creating agent without a worker placement"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_core::{Agent, ProjectId};
    use fleet_store::mem::MemStore;

    /// `Online` + `periodic` + 회로 닫힘 — 배정 가능한 기본형.
    fn placeable(name: &str) -> fleet_core::Worker {
        fleet_core::Worker::new(name, format!("http://{name}"))
    }

    /// 배정 원장을 채운다. `count_agents_by_worker`가 세는 것은 Agent 행이지
    /// `Worker::active_tasks`가 아니므로, 부하를 만들려면 실제로 Agent를
    /// 만들어 배정 상태로 넣어야 한다 — 이 사실 자체가 fixture로 고정된다.
    async fn seed_agents(store: &MemStore, worker: &fleet_core::Worker, n: usize) {
        let project = ProjectId::new();
        for i in 0..n {
            let agent = Agent::new(project, format!("{}-a{i}", worker.name))
                .with_placement(worker.id, chrono::Utc::now());
            store.create_agent(&agent).await.unwrap();
        }
    }

    #[tokio::test]
    async fn no_workers_at_all_is_all_offline() {
        let store = MemStore::new();
        let err = choose_worker(&store).await.unwrap_err();
        assert!(
            matches!(err, PlacementError::AllOffline),
            "Worker가 한 대도 없는 저장소는 `AllOffline`이다: {err}"
        );
    }

    #[tokio::test]
    async fn offline_worker_is_not_a_candidate() {
        let store = MemStore::new();
        let mut w = placeable("down");
        w.status = WorkerStatus::Offline;
        store.upsert_worker(&w).await.unwrap();

        let err = choose_worker(&store).await.unwrap_err();
        assert!(matches!(err, PlacementError::AllOffline), "{err}");
    }

    #[tokio::test]
    async fn on_demand_worker_is_not_a_candidate() {
        // `fleet-api`의 등록 경로가 liveness_mode와 무관하게 `Online`을 쓰고
        // `HealthChecker`는 on_demand를 강등하지 않으므로, on_demand Worker는
        // 등록 이후 **영구히 Online**이다. fixture를 Offline으로 두면 1단계
        // 필터에 먼저 걸려 정작 검증하려는 2단계가 실행되지 않는다
        // (`selector.rs`의 같은 fixture와 같은 이유).
        let store = MemStore::new();
        let mut w = placeable("lazy");
        w.liveness_mode = WorkerLivenessMode::OnDemand;
        assert_eq!(w.status, WorkerStatus::Online);
        store.upsert_worker(&w).await.unwrap();

        let err = choose_worker(&store).await.unwrap_err();
        assert!(
            matches!(err, PlacementError::AllUnprobed),
            "on_demand만 남으면 `AllOffline`이 아니라 `AllUnprobed`여야 한다 \
             — 운영자가 취할 행동이 다르다: {err}"
        );
    }

    #[tokio::test]
    async fn open_circuit_worker_is_not_a_candidate() {
        // `update_worker_circuit_state`가 MemStore에서 no-op 기본 구현이므로
        // 전이 API로는 회로를 열 수 없다. 대신 저장된 값을 직접 넣는다 —
        // `choose_worker`가 읽는 것이 registry가 아니라 **컬럼**이라는 것이
        // 바로 이 테스트가 성립하는 이유다.
        let store = MemStore::new();
        let mut w = placeable("tripped");
        w.circuit_state = CircuitState::Open;
        store.upsert_worker(&w).await.unwrap();

        let err = choose_worker(&store).await.unwrap_err();
        assert!(matches!(err, PlacementError::AllCircuitOpen), "{err}");
    }

    #[tokio::test]
    async fn half_open_worker_stays_a_candidate() {
        // dispatch가 `is_open()`으로 판정해 HalfOpen을 통과시키는 것과 같은
        // 취급. 여기서 HalfOpen을 빼면 탐색 중인 Worker가 영원히 배정 대상에서
        // 빠지고, 회로를 닫을 트래픽이 오지 않는다.
        let store = MemStore::new();
        let mut w = placeable("probing");
        w.circuit_state = CircuitState::HalfOpen;
        let id = w.id;
        store.upsert_worker(&w).await.unwrap();

        assert_eq!(choose_worker(&store).await.unwrap(), id);
    }

    #[tokio::test]
    async fn least_loaded_wins() {
        let store = MemStore::new();
        let busy = placeable("busy");
        let idle = placeable("idle");
        store.upsert_worker(&busy).await.unwrap();
        store.upsert_worker(&idle).await.unwrap();
        seed_agents(&store, &busy, 3).await;
        seed_agents(&store, &idle, 1).await;

        assert_eq!(
            choose_worker(&store).await.unwrap(),
            idle.id,
            "부하의 출처는 배정된 Agent 수다"
        );
    }

    #[tokio::test]
    async fn stopped_agents_do_not_hold_a_slot() {
        // 원장이 `status <> 'stopped'`로 거르므로 회수만으로 슬롯이 풀린다.
        // 이것이 `unassign_agent_worker`를 만들지 않은 근거다 — 슬롯을
        // 되돌리는 별도 경로가 필요 없다.
        let store = MemStore::new();
        let a = placeable("a");
        let b = placeable("b");
        store.upsert_worker(&a).await.unwrap();
        store.upsert_worker(&b).await.unwrap();
        seed_agents(&store, &a, 2).await;
        seed_agents(&store, &b, 1).await;
        assert_eq!(choose_worker(&store).await.unwrap(), b.id);

        // a에 배정된 Agent를 전부 중지 → a의 부하가 0이 되어 다시 이긴다.
        let on_a = store
            .list_agents(&fleet_core::AgentFilter {
                worker_id: Some(a.id),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(on_a.len(), 2);
        for agent in on_a {
            store
                .update_agent_status(agent.id, fleet_core::AgentStatus::Stopped)
                .await
                .unwrap();
        }

        assert_eq!(
            choose_worker(&store).await.unwrap(),
            a.id,
            "중지된 Agent는 슬롯을 잡지 않는다"
        );
    }

    #[tokio::test]
    async fn ties_break_on_earliest_registration() {
        let store = MemStore::new();
        let mut old = placeable("old");
        let mut new = placeable("new");
        old.registered_at = chrono::Utc::now() - chrono::Duration::hours(2);
        new.registered_at = chrono::Utc::now();
        store.upsert_worker(&old).await.unwrap();
        store.upsert_worker(&new).await.unwrap();

        // 둘 다 부하 0 — 동점.
        assert_eq!(choose_worker(&store).await.unwrap(), old.id);
    }

    #[tokio::test]
    async fn place_on_create_swallows_the_error() {
        // 생성 경로의 계약: 실패는 `None`이지 `Err`가 아니다. Agent 정의가
        // Worker 가용성에 인질로 잡히면 안 되기 때문이다.
        let store = MemStore::new();
        assert!(place_on_create(&store).await.is_none());

        let w = placeable("up");
        store.upsert_worker(&w).await.unwrap();
        let (id, _at) = place_on_create(&store).await.expect("placed");
        assert_eq!(id, w.id);
    }

    // ── 프로세스 상한 (`#67` 게이트 ①-A) ───────────────────────────────

    #[tokio::test]
    async fn unknown_cap_never_filters() {
        // `max_agent_processes`가 `None`인 Worker는 부하가 아무리 높아도
        // 후보에 남는다. `None`이 "상한 0"이나 "상한 미보고이므로 배제"로
        // 읽히면 구버전 Worker만 남은 플릿에서 배정이 통째로 멈춘다.
        let store = MemStore::new();
        let w = placeable("legacy");
        assert!(
            w.max_agent_processes.is_none(),
            "생성자의 기본은 '모른다'다"
        );
        store.upsert_worker(&w).await.unwrap();
        seed_agents(&store, &w, 50).await;

        assert_eq!(choose_worker(&store).await.unwrap(), w.id);
    }

    #[tokio::test]
    async fn a_full_worker_is_excluded_even_when_it_is_least_loaded() {
        // 상한 필터가 실제로 발동했는지를 정렬과 분리해서 본다. 가득 찬 쪽이
        // **부하는 더 낮게** 되도록 만들었으므로, 필터가 없으면 least-loaded
        // 정렬이 가득 찬 쪽을 고른다. 즉 이 단정은 정렬로는 통과할 수 없다.
        let store = MemStore::new();
        let mut full = placeable("full");
        full.max_agent_processes = Some(1);
        let roomy = placeable("roomy");
        store.upsert_worker(&full).await.unwrap();
        store.upsert_worker(&roomy).await.unwrap();
        seed_agents(&store, &full, 1).await;
        seed_agents(&store, &roomy, 5).await;

        assert_eq!(choose_worker(&store).await.unwrap(), roomy.id);
    }

    #[tokio::test]
    async fn all_reported_caps_full_is_all_at_capacity() {
        let store = MemStore::new();
        let mut w = placeable("solo");
        w.max_agent_processes = Some(2);
        store.upsert_worker(&w).await.unwrap();
        seed_agents(&store, &w, 2).await;

        let err = choose_worker(&store).await.unwrap_err();
        assert!(
            matches!(err, PlacementError::AllAtCapacity),
            "상한을 보고한 Worker가 전부 가득 차면 `AllAtCapacity`다: {err}"
        );

        // 한 자리가 비면 즉시 다시 배정 가능해진다 — 상한이 영구 배제가
        // 아니라 카운트 비교임을 고정한다.
        let mut idle = placeable("idle");
        idle.max_agent_processes = Some(1);
        store.upsert_worker(&idle).await.unwrap();
        assert_eq!(choose_worker(&store).await.unwrap(), idle.id);
    }

    #[tokio::test]
    async fn stopped_agents_free_a_capped_slot() {
        // `count_agents_by_worker`가 `stopped`를 세지 않는다는 사실이 상한
        // 필터에도 그대로 걸린다는 것. 이 둘이 어긋나면 회수된 Agent가 슬롯을
        // 영구히 점유해 Worker가 서서히 배정 불가가 된다.
        let store = MemStore::new();
        let mut w = placeable("recycler");
        w.max_agent_processes = Some(1);
        store.upsert_worker(&w).await.unwrap();

        let project = ProjectId::new();
        let a = Agent::new(project, "spent").with_placement(w.id, chrono::Utc::now());
        store.create_agent(&a).await.unwrap();
        assert!(matches!(
            choose_worker(&store).await.unwrap_err(),
            PlacementError::AllAtCapacity
        ));

        store
            .update_agent_status(a.id, fleet_core::AgentStatus::Stopped)
            .await
            .unwrap();
        assert_eq!(choose_worker(&store).await.unwrap(), w.id);
    }
}
