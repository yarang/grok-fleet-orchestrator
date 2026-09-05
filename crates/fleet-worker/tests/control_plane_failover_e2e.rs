//! Primary 종료 → 수동 승격 → **Worker 재연결** → 밀린 일의 재조정
//! (로드맵 `#67` 게이트 ④).
//!
//! 게이트가 요구하는 셋 중 앞의 둘은 `fleet-scheduler`의 `lease_failover`가
//! 스케줄러만으로 덮었다. 남은 것이 가운데 항목이고, 그것은 스케줄러만으로는
//! 만들 수 없다 — 워커의 heartbeat이 닿는 곳은 리스를 쥔 스케줄러가 아니라
//! **API 표면**이기 때문이다. 그래서 이 파일은 셋을 한 프로세스 안에 세운다:
//! 같은 Store 위의 API 인스턴스 둘, 그 둘 사이에서 옮겨 가는 리스, 그리고
//! 주소를 하나도 바꾸지 않은 채 재연결하는 진짜 워커.
//!
//! **주소를 유지하는 것이 이 시험의 요점 하나다.** 워커와 오케스트레이터 사이에
//! TCP 링크를 두고, 링크의 **뒤쪽만** A에서 B로 바꾼다. 워커의 `worker.toml`은
//! 손대지 않는다 — VIP나 로드밸런서 뒤에서 인스턴스가 교체되는 실제 모양이고,
//! "워커가 재연결했다"가 설정 변경이 아니라 관측이 되게 한다.
//!
//! **검증 한계**: 밀린 Task를 그 워커에게 실제로 dispatch하지는 않는다. 그러려면
//! ACP transport를 워커의 종단에 물려야 하는데, 이 시험이 묻는 것은 "새 Primary가
//! 밀린 일을 집는가"이지 그것이 어디로 가는가가 아니다. dispatch 경로는
//! `fleet-scheduler`의 `dispatch_e2e`가 따로 덮는다.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use fleet_api::{build_app, AppState};
use fleet_core::{CircuitBreakerConfig, Task, TaskRequest, TaskStatus, WorkerId};
use fleet_scheduler::{
    Dispatcher, FleetState, LeaseManager, LeaseManagerConfig, ReconcileConfig, Reconciler,
};
use fleet_store::mem::MemStore;
use fleet_store::Store;
use fleet_transport::MockTransport;
use fleet_worker::{RegistrationClient, WorkerConfig};
use tokio::sync::watch;
use tokio::task::JoinHandle;

fn fake_grok(dir: &std::path::Path) -> String {
    use std::io::Write;
    let path = dir.join("fake-grok.sh");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(
        f,
        "#!/bin/sh\ncase \"$1\" in --version) echo 'grok 0.0.0-test'; exit 0;; esac\nexec sleep 300"
    )
    .unwrap();
    drop(f);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path.to_string_lossy().into_owned()
}

async fn serve_at(addr: SocketAddr, store: Arc<dyn Store>) -> JoinHandle<()> {
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let app = build_app(Arc::new(AppState::new(store)));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    })
}

/// 워커가 보는 주소와 실제 인스턴스를 잇는 링크. 연결 태스크를 `JoinSet`으로
/// 소유하므로 `abort`가 이미 맺어진 연결까지 끊는다 — 그렇게 하지 않으면
/// `reqwest`의 연결 풀 때문에 "죽인" 인스턴스로 트래픽이 계속 간다.
async fn link(listen: SocketAddr, backend: SocketAddr) -> JoinHandle<()> {
    let listener = tokio::net::TcpListener::bind(listen).await.unwrap();
    tokio::spawn(async move {
        let mut conns = tokio::task::JoinSet::new();
        while let Ok((mut inbound, _)) = listener.accept().await {
            conns.spawn(async move {
                if let Ok(mut outbound) = tokio::net::TcpStream::connect(backend).await {
                    let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                }
            });
        }
    })
}

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

async fn last_seen(store: &Arc<dyn Store>, id: WorkerId) -> Option<chrono::DateTime<chrono::Utc>> {
    store
        .get_worker(id)
        .await
        .unwrap()
        .expect("worker")
        .last_seen
}

async fn task_status(store: &Arc<dyn Store>, id: fleet_core::TaskId) -> TaskStatus {
    store.get_task(id).await.unwrap().expect("task").status
}

/// 리스를 관측하는 스케줄러 인스턴스 하나.
async fn scheduler(
    store: Arc<dyn Store>,
    lease: fleet_scheduler::LeaseObserver,
) -> Arc<FleetState> {
    let transport = MockTransport::new();
    let event_rx = fleet_transport::WorkerTransport::subscribe(&transport)
        .await
        .unwrap();
    let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(transport);
    let state = Arc::new(
        FleetState::new(store, transport, CircuitBreakerConfig::default()).with_lease(lease),
    );
    Arc::new(Dispatcher::new(state.clone()))
        .attach_event_receiver(event_rx)
        .await;
    state
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_worker_reconnects_to_the_promoted_instance_and_its_pending_work_moves() {
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn Store> = Arc::new(MemStore::new());

    // 같은 Store 위의 두 인스턴스. 워커는 둘의 존재를 모른다.
    let backend_a = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap()
    };
    let backend_b = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap()
    };
    let server_a = serve_at(backend_a, store.clone()).await;
    let _server_b = serve_at(backend_b, store.clone()).await;

    let front = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap()
    };
    let mut net = link(front, backend_a).await;

    // 진짜 워커. 등록도 heartbeat도 실제 HTTP다.
    let config = Arc::new(
        WorkerConfig::for_test()
            .name("failover-worker")
            .orchestrator_url(format!("http://{front}"))
            .grok_bin(fake_grok(dir.path()))
            .agent_port_range("39900-39919")
            .agent_workspace_root(dir.path().to_string_lossy().into_owned())
            .build(),
    );
    let client = Arc::new(RegistrationClient::new(config.clone()).unwrap());
    let worker_id: WorkerId = client
        .register_once()
        .await
        .expect("register")
        .worker_id
        .parse()
        .unwrap();

    let manager = Arc::new(
        fleet_worker::AgentProcessManager::new(config.clone()).expect("agent process manager"),
    );
    let (stop_worker, worker_rx) = watch::channel(false);
    let worker_loop = {
        let (c, m) = (client.clone(), manager.clone());
        tokio::spawn(async move {
            c.run_heartbeat_loop(1, 3600, "127.0.0.1:1".into(), m, worker_rx)
                .await;
        })
    };

    // 옛 Primary가 남긴 밀린 Task.
    let task = Task::from_request(TaskRequest {
        prompt: "left behind across the failover".into(),
        ..Default::default()
    });
    store.insert_task(&task).await.unwrap();

    let lease_cfg = || LeaseManagerConfig {
        ttl: Duration::from_secs(3),
        renew_interval: Duration::from_secs(1),
        poll_interval: Duration::from_millis(300),
        shutdown_grace: Duration::from_secs(3),
    };
    let primary = LeaseManager::new(store.clone(), "c1", "primary", lease_cfg()).spawn();
    until(
        "Primary가 리스를 잡지 못했다",
        Duration::from_secs(10),
        || {
            let s = primary.status();
            async move { s.is_active() }
        },
    )
    .await;
    let standby = LeaseManager::new(store.clone(), "c1", "standby", lease_cfg()).spawn();

    // Standby의 Reconciler는 돌지만 리스가 없으므로 아무것도 건드리지 않는다.
    let standby_state = scheduler(store.clone(), standby.observer()).await;
    let standby_reconciler = Reconciler::new(
        standby_state.clone(),
        Arc::new(Dispatcher::new(standby_state.clone()).with_max_dispatch_retries(1)),
        ReconcileConfig {
            interval: Duration::from_millis(300),
            stale_after: Duration::from_secs(0),
            dispatched_worker_check_after: Duration::from_secs(0),
            offline_worker_grace: Duration::from_secs(0),
            max_dispatch_retries: 1,
        },
    )
    .spawn();

    // 워커가 A를 통해 살아 있다.
    until(
        "워커의 heartbeat이 도달하지 않았다",
        Duration::from_secs(20),
        || {
            let (s, id) = (store.clone(), worker_id);
            async move { last_seen(&s, id).await.is_some() }
        },
    )
    .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        matches!(task_status(&store, task.id).await, TaskStatus::Pending),
        "리스를 쥐지 않은 인스턴스는 밀린 일을 건드리지 않아야 한다"
    );

    // ── Primary 종료 ────────────────────────────────────────────────
    // 링크를 끊어 A로 가는 길을 없애고(워커의 heartbeat이 실패하기 시작한다),
    // 리스도 정상 해제한다.
    net.abort();
    let _ = (&mut net).await;
    server_a.abort();
    primary.shutdown().await;

    let stalled = last_seen(&store, worker_id).await.expect("last_seen");

    // ── 수동 승격 ───────────────────────────────────────────────────
    until(
        "승격이 일어나지 않았다",
        Duration::from_secs(10),
        || {
            let s = standby.status();
            async move { s.is_active() }
        },
    )
    .await;

    // ── Worker 재연결 ───────────────────────────────────────────────
    // **워커의 설정은 그대로다.** 같은 `front`에 다시 링크를 세우되 뒤쪽만
    // B로 바꾼다 — 워커는 자기가 다른 인스턴스와 말하고 있다는 것을 모른다.
    let net = link(front, backend_b).await;

    until(
        "재연결 뒤에도 워커의 heartbeat이 새 인스턴스에 닿지 않았다",
        Duration::from_secs(20),
        || {
            let (s, id) = (store.clone(), worker_id);
            async move {
                last_seen(&s, id)
                    .await
                    .map(|t| t > stalled)
                    .unwrap_or(false)
            }
        },
    )
    .await;

    // ── 밀린 일의 재조정 ────────────────────────────────────────────
    // 코드는 하나도 바뀌지 않았고 바뀐 것은 리스뿐이다.
    until(
        "승격 뒤에도 밀린 일이 재조정되지 않았다",
        Duration::from_secs(20),
        || {
            let (s, id) = (store.clone(), task.id);
            async move { !matches!(task_status(&s, id).await, TaskStatus::Pending) }
        },
    )
    .await;

    let _ = stop_worker.send(true);
    let _ = worker_loop.await;
    manager.shutdown_all().await;
    standby_reconciler.abort().await;
    standby.shutdown().await;
    net.abort();
}
