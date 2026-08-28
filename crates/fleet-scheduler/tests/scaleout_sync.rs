//! 다중 오케스트레이터 노드(Scale-Out) 간의 서킷 브레이커 동기화 통합 테스트.
//!
//! 이 테스트는 동일한 PostgreSQL DB를 공유하는 두 개의 독립적인 FleetState(노드 A, 노드 B)를 생성하고,
//! 한 노드에서 서킷 브레이커 트립이 발생했을 때 Postgres LISTEN/NOTIFY 채널을 통해
//! 다른 노드의 인메모리 브레이커 상태로 정상 동기화되는지 검증합니다.
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-scheduler --test scaleout_sync -- --test-threads=1
//! ```

use std::sync::Arc;
use std::time::Duration;

use fleet_core::{CircuitBreakerConfig, CircuitState, FleetEvent, Worker, WorkerId};
use fleet_scheduler::breaker::BreakerState;
use fleet_scheduler::sync::MultiAdminSync;
use fleet_scheduler::FleetState;
use fleet_store::{PgStore, Store};
use sqlx::postgres::PgPoolOptions;

fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

async fn try_connect() -> Option<(PgStore, sqlx::PgPool)> {
    let url = database_url()?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but connection failed: {e}"));
    let store = PgStore::from_pool(pool.clone());
    store
        .migrate()
        .await
        .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but migration failed: {e}"));
    Some((store, pool))
}

macro_rules! require_db {
    ($store:ident, $pool:ident) => {
        let ($store, $pool) = match try_connect().await {
            Some(pair) => pair,
            None => return,
        };
        // 각 테스트 전 테이블 비움
        let _ = sqlx::query("TRUNCATE task_outputs, events, tasks, workers CASCADE")
            .execute($store.pool())
            .await;
    };
}

/// 노드의 인메모리 브레이커가 `expected`에 도달할 때까지 데드라인까지 폴링한다.
///
/// 고정 `sleep`을 쓰지 않는 이유: 노드 B의 동기화는 도달 경로가 둘이고 지연이
/// 크게 다르다. (1) Postgres LISTEN/NOTIFY — 즉시. (2) NOTIFY를 놓쳤을 때의
/// 폴백 폴링 — `fleet-store`의 `FALLBACK_POLL_INTERVAL`, 현재 5초.
///
/// 이 테스트는 `tokio::spawn(sync_b.run())` 직후 곧바로 이벤트를 발행하는데,
/// `tokio::spawn`은 태스크를 큐에 넣을 뿐 실행을 보장하지 않는다. spawn된
/// 쪽이 `LISTEN`을 걸기 전에 NOTIFY가 나가면 그 알림은 유실되고(Postgres는
/// 구독 이전에 발행된 알림을 전달하지 않는다) 수렴은 (2)의 주기까지 늦춰진다.
/// 고정 200ms 예산은 그 경로에서 반드시 깨지며, 실제로 CI에서 간헐적으로
/// 깨졌다. 제품은 (2)를 갖고 있어 어느 쪽으로든 수렴하므로 이건 제품 결함이
/// 아니라 테스트의 대기 방식 문제다 — 두 경로 중 어느 쪽으로 도달하든
/// 통과하도록 조건 폴링으로 기다린다.
async fn await_breaker_state(
    state: &FleetState,
    worker_id: WorkerId,
    expected: BreakerState,
    what: &str,
) {
    // 폴백 폴링 주기(5초)보다 넉넉해야 두 경로를 모두 덮는다.
    const DEADLINE: Duration = Duration::from_secs(15);
    const POLL: Duration = Duration::from_millis(20);

    let start = std::time::Instant::now();
    loop {
        let actual = state.breakers.state_of(worker_id);
        if actual == expected {
            return;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "{what}: breaker did not reach {expected:?} within {DEADLINE:?} \
             (last seen {actual:?})"
        );
        tokio::time::sleep(POLL).await;
    }
}

#[tokio::test]
async fn test_circuit_breaker_sync_between_scaleout_nodes() {
    require_db!(store_a, pool_a);

    // 동일 DB를 가리키는 노드 B의 커넥션 풀 및 스토어 생성
    let store_b = PgStore::from_pool(pool_a.clone());

    // 3회 실패 시 트립되도록 브레이커 설정 구성
    let cb_config = CircuitBreakerConfig {
        enabled: true,
        window_duration_secs: 10,
        min_samples: 3,
        error_rate_threshold: 0.5,
        open_duration_secs: 5,
        half_open_max_probes: 1,
        failure_codes: vec![500],
    };

    let transport_a = Arc::new(fleet_transport::MockTransport::new());
    let transport_b = Arc::new(fleet_transport::MockTransport::new());

    // 독립된 노드 A, B의 FleetState 생성
    let state_a = Arc::new(FleetState::new(
        Arc::new(store_a),
        transport_a,
        cb_config.clone(),
    ));
    let state_b = Arc::new(FleetState::new(
        Arc::new(store_b),
        transport_b,
        cb_config.clone(),
    ));

    // 노드 B의 동기화 코디네이터(MultiAdminSync) 백그라운드 기동
    let sync_b = MultiAdminSync::new(state_b.clone(), pool_a.clone());
    let sync_handle = tokio::spawn(sync_b.run());

    // 1. 워커 생성 및 DB 등록
    let worker_id = WorkerId::new();
    let worker = Worker {
        id: worker_id,
        name: "test-worker-1".to_string(),
        endpoint: "wss://localhost:8080/ws".to_string(),
        labels: std::collections::HashMap::new(),
        status: fleet_core::WorkerStatus::Online,
        last_seen: Some(chrono::Utc::now()),
        active_tasks: 0,
        max_concurrent: 2,
        circuit_state: CircuitState::Closed,
        worker_version: None,
        liveness_mode: fleet_core::WorkerLivenessMode::Periodic,
        registered_at: chrono::Utc::now(),
        incarnation_started_at: chrono::Utc::now(),
    };
    state_a.store.upsert_worker(&worker).await.unwrap();

    // 양쪽 노드 브레이커 초기 상태는 Closed여야 함
    assert_eq!(state_a.breakers.state_of(worker_id), BreakerState::Closed);
    assert_eq!(state_b.breakers.state_of(worker_id), BreakerState::Closed);

    // 2. 노드 A에서 실패 누적으로 서킷 브레이커 트립 모사
    let cb_a = state_a.breakers.get(worker_id, CircuitState::Closed);
    cb_a.record(fleet_scheduler::breaker::Outcome::Failure);
    cb_a.record(fleet_scheduler::breaker::Outcome::Failure);
    cb_a.record(fleet_scheduler::breaker::Outcome::Failure);

    assert_eq!(cb_a.state(), BreakerState::Open);

    // 노드 A가 DB에 상태 업데이트 및 이벤트 발행 (Dispatcher의 동작 모사)
    state_a
        .store
        .update_worker_circuit_state(worker_id, CircuitState::Open)
        .await
        .unwrap();
    state_a
        .store
        .append_event(&FleetEvent::worker_circuit_changed(
            worker_id,
            CircuitState::Closed,
            CircuitState::Open,
        ))
        .await
        .unwrap();

    // 3. LISTEN/NOTIFY 또는 폴백 폴링으로 노드 B의 인메모리 브레이커가
    //    Open으로 싱크될 때까지 대기
    await_breaker_state(
        &state_b,
        worker_id,
        BreakerState::Open,
        "node B after node A tripped the breaker",
    )
    .await;

    // 4. 노드 A에서 서킷을 Closed로 복구(리셋) 모사
    cb_a.reset();
    assert_eq!(cb_a.state(), BreakerState::Closed);

    state_a
        .store
        .update_worker_circuit_state(worker_id, CircuitState::Closed)
        .await
        .unwrap();
    state_a
        .store
        .append_event(&FleetEvent::worker_circuit_changed(
            worker_id,
            CircuitState::Open,
            CircuitState::Closed,
        ))
        .await
        .unwrap();

    // 노드 B의 인메모리 브레이커도 Closed로 원복 싱크될 때까지 대기
    await_breaker_state(
        &state_b,
        worker_id,
        BreakerState::Closed,
        "node B after node A reset the breaker",
    )
    .await;

    // 백그라운드 태스크 정리
    sync_handle.abort();
}
