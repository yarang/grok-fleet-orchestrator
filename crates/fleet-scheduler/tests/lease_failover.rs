//! Primary 종료 → 수동 승격 → 밀린 일의 재조정 (로드맵 `#67` 게이트 ④).
//!
//! **store 수준의 lease 의미론은 이미 증명돼 있다** — `fleet-store`의
//! `control_plane_lease`가 획득·갱신·만료·해제·경합을 11건으로 덮는다. 여기서
//! 묻는 것은 그 위의 질문이다: 리스를 **쥔 인스턴스만** 밀린 일을 건드리는가,
//! 그리고 승격이 실제로 그 권한을 옮기는가.
//!
//! 그 둘이 갈라지면 최악의 형태로 틀린다. 앞의 것이 깨지면 리스를 잃은 옛
//! Primary가 새 Primary와 **동시에** 재dispatch하고, 뒤의 것이 깨지면 아무도
//! 하지 않아 밀린 Task가 영원히 남는다.
//!
//! **가짜 리스를 쓰지 않는다.** `LeaseObserver::with_status`로 상태를 지어내면
//! 승격 자체가 시험에서 빠지는데, 게이트가 묻는 것이 바로 그 승격이다. 실제
//! `LeaseManager` 둘을 같은 Store 위에 띄우고 한쪽을 내린다.
//!
//! **리스 게이팅은 두 겹이고, 그것이 이 시험의 변이 결과를 설명한다.**
//! `Reconciler::sweep`과 `Dispatcher::dispatch`가 각각 `lease_allows_control()`을
//! 보므로 둘은 **직렬**이다 — 한쪽만 지운 트리에서는 다른 쪽이 여전히 막아
//! 관측 가능한 동작이 변하지 않고, 그래서 이 시험은 초록으로 남는다(양쪽을 각각
//! 지워 실측했다). 동작을 보는 시험으로서 그것이 옳다: 한 겹이 사라져도 시스템은
//! 여전히 옳게 행동한다. 둘을 **함께** 지우면 승격 전에 Task가 `Failed`로 떨어져
//! 시험이 붉어진다. 즉 이 시험이 고정하는 것은 "리스 없이는 밀린 일을 건드리지
//! 않는다"는 **성질**이지 특정 `if` 문이 아니다.
//!
//! **재시도 상한을 1로 낮춘 것도 판별력을 위해서다.** 기본값 20이면 소진까지
//! 최소 6초가 걸려 "승격 전에는 아무 일도 없다"는 확인이 그 6초 안에 들어가
//! 버린다 — 양쪽 게이팅을 다 지워도 그 시점에는 아직 `Pending`이라 단정이
//! 통과한다(실측으로 확인했다).
//!
//! **검증 한계**: 게이트가 요구하는 셋 중 "Worker 재연결"은 여기에 없다. 그것은
//! 워커 프로세스와 API 표면이 함께 있어야 하고(`fleet-worker`의
//! `self_fencing_e2e`가 그 골격을 갖고 있다), 이 파일은 스케줄러 쪽 둘만 덮는다.

use std::sync::Arc;
use std::time::Duration;

use fleet_core::{CircuitBreakerConfig, Task, TaskRequest, TaskStatus, Worker, WorkerStatus};
use fleet_scheduler::{
    Dispatcher, FleetState, LeaseManager, LeaseManagerConfig, ReconcileConfig, Reconciler,
};
use fleet_store::Store;
use fleet_transport::MockTransport;

/// 시험용 리스 설정. 기본값(ttl 15초)은 이 시험을 1분 넘게 만든다.
fn fast_lease() -> LeaseManagerConfig {
    LeaseManagerConfig {
        ttl: Duration::from_secs(3),
        renew_interval: Duration::from_secs(1),
        poll_interval: Duration::from_millis(300),
        shutdown_grace: Duration::from_secs(3),
    }
}

/// 밀린 일을 곧바로 집어 드는 재조정 설정.
fn eager_reconcile() -> ReconcileConfig {
    ReconcileConfig {
        interval: Duration::from_millis(300),
        stale_after: Duration::from_secs(0),
        dispatched_worker_check_after: Duration::from_secs(0),
        offline_worker_grace: Duration::from_secs(0),
        // **1회로 낮추는 것이 이 시험의 판별력을 만든다.** 기본값 20이면
        // 소진까지 최소 6초가 걸려, "승격 전에는 아무 일도 없다"는 확인이
        // 그 6초 안에 들어가 버린다 — 리스 게이팅을 지운 트리에서도 아직
        // `Pending`이라 단정이 통과한다(실측으로 확인했다). 1회면 리스가
        // 없을 때 곧바로 상태가 바뀌므로 그 구멍이 사라진다.
        max_dispatch_retries: 1,
    }
}

/// 한 오케스트레이터 인스턴스. 같은 Store를 보되 `FleetState`는 따로 갖는다 —
/// 그것이 두 인스턴스를 가르는 경계이고, 리스 관측도 거기 붙는다.
async fn instance(store: Arc<dyn Store>, lease: fleet_scheduler::LeaseObserver) -> Arc<FleetState> {
    let transport = MockTransport::new();
    let event_rx = fleet_transport::WorkerTransport::subscribe(&transport)
        .await
        .unwrap();
    let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(transport);

    let state = Arc::new(
        FleetState::new(store, transport, CircuitBreakerConfig::default()).with_lease(lease),
    );
    let dispatcher = Arc::new(Dispatcher::new(state.clone()));
    dispatcher.attach_event_receiver(event_rx).await;
    state
}

/// 조건이 참이 될 때까지 짧게 폴링한다.
async fn until<F, Fut>(what: &str, timeout: Duration, mut cond: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if cond().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{what} — {timeout:?} 안에 일어나지 않았다");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pending_work_waits_for_promotion_and_only_the_new_primary_reconciles_it() {
    let store = Arc::new(fleet_store::mem::MemStore::new()) as Arc<dyn Store>;

    // 어디로도 dispatch되지 않도록 워커를 두지 않는다 — 이 시험이 묻는 것은
    // 재조정이 **일어나는가**이지 어디로 가는가가 아니다.
    let mut offline = Worker::new("gone", "wss://gone/ws");
    offline.status = WorkerStatus::Offline;
    store.upsert_worker(&offline).await.unwrap();

    // 옛 Primary가 남긴 밀린 Task.
    let task = Task::from_request(TaskRequest {
        prompt: "left behind by the old primary".into(),
        ..Default::default()
    });
    store.insert_task(&task).await.unwrap();

    let primary = LeaseManager::new(store.clone(), "c1", "primary", fast_lease());
    let standby = LeaseManager::new(store.clone(), "c1", "standby", fast_lease());

    let primary_handle = primary.spawn();
    until(
        "Primary가 리스를 잡지 못했다",
        Duration::from_secs(10),
        || {
            let h = primary_handle.status();
            async move { h.is_active() }
        },
    )
    .await;

    let standby_handle = standby.spawn();
    // Standby는 유효한 리스를 빼앗지 못한다 — `control_plane_lease`가 이미
    // 증명한 사실이지만, 아래 단정의 전제라서 여기서도 확인한다.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        !standby_handle.status().is_active(),
        "유효한 리스가 있는 동안 Standby가 활성이면 아래 단정이 무의미하다"
    );

    // Standby의 Reconciler는 돌지만 **아무것도 건드리지 않아야 한다.**
    let standby_state = instance(store.clone(), standby_handle.observer()).await;
    let standby_reconciler = Reconciler::new(
        standby_state.clone(),
        Arc::new(Dispatcher::new(standby_state.clone()).with_max_dispatch_retries(1)),
        eager_reconcile(),
    )
    .spawn();

    tokio::time::sleep(Duration::from_secs(2)).await;
    let before = store.get_task(task.id).await.unwrap().expect("task");
    assert!(
        matches!(before.status, TaskStatus::Pending),
        "리스를 쥐지 않은 인스턴스는 밀린 일을 건드리지 않아야 한다 — {:?}",
        before.status
    );

    // Primary 종료. `shutdown()`은 리스를 **정상 해제**한다(정본의 두 만료 원인
    // 중 "graceful release" 쪽) — TTL을 기다리는 `abort()`와 다른 경로다.
    primary_handle.shutdown().await;

    // 수동 승격: Standby의 리스 루프가 빈 자리를 집어 든다.
    until(
        "승격이 일어나지 않았다",
        Duration::from_secs(10),
        || {
            let s = standby_handle.status();
            async move { s.is_active() }
        },
    )
    .await;

    // 그리고 이제 같은 Reconciler가 그 일을 집는다. 코드는 하나도 바뀌지
    // 않았고 바뀐 것은 리스뿐이다 — 그것이 이 시험의 요지다.
    until(
        "승격 뒤에도 밀린 일이 재조정되지 않았다",
        Duration::from_secs(15),
        || {
            let (s, id) = (store.clone(), task.id);
            async move {
                !matches!(
                    s.get_task(id).await.unwrap().expect("task").status,
                    TaskStatus::Pending
                )
            }
        },
    )
    .await;

    standby_reconciler.abort().await;
    standby_handle.shutdown().await;
}
