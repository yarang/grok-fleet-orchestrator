//! 제어면 단절 → self-fencing → 재연결 → 관측 소거 → 재배정 (로드맵 `#67` 게이트 ⑥).
//!
//! 이 파일이 있는 이유는 게이트 ⑥의 증명이 그동안 **두 조각으로 나뉘어** 있었기
//! 때문이다. 워커 쪽 시험(`fleet-worker::registration`)은 유예를 넘긴 단절에서
//! 프로세스가 멈추는 것까지 보고, 저장소 쪽 시험(`fleet-store::agents`)은 관측이
//! 없는 Agent가 다른 Worker로 옮겨지는 것을 본다. 그 사이 — 워커가 멈춘 사실이
//! heartbeat으로 오케스트레이터에 닿아 관측을 지우고, 그래서 게이트 ②의 술어가
//! 풀린다 — 를 잇는 시험이 없었다.
//!
//! **그 사이가 이 기능의 값 전부다.** 게이트 ②는 `running`으로 보고된 Agent를
//! 옮기지 못하게 막는데, 관측을 지우는 경로는 그 Worker의 다음 heartbeat뿐이다.
//! 그러므로 "프로세스를 멈춘다"와 "다시 연결된다" 둘 중 하나만으로는 Agent가
//! 풀리지 않는다. 이 시험은 그 인과를 한 줄기로 세운다.
//!
//! **단절을 흉내내지 않는다.** 오케스트레이터 태스크를 실제로 죽여 포트를 닫고
//! (워커의 heartbeat은 연결 거부로 실패한다), 같은 포트에 같은 Store로 다시
//! 띄운다. mock 서버에 플래그를 세우는 방식이면 워커의 HTTP 경로가 시험에서
//! 빠지는데, 펜싱을 발동시키는 것이 바로 그 경로의 실패다.
//!
//! **검증 한계**: Store는 `MemStore`다. 관측을 지우는 코드(`apply_agent_observations`)와
//! 재배정 술어는 두 Store 모두에 있고 각각 `fleet-store`에서 따로 증명되므로, 이
//! 시험이 맡은 것은 그 둘을 잇는 **순서**이지 Store의 구현이 아니다. 덕분에
//! `DATABASE_URL` 없이 모든 잡에서 돈다 — 조건부 게이트는 그 자체로 CI보다 약하다는
//! `agent.md` §4.3의 판단을 따른다.

use std::net::SocketAddr;
use std::sync::Arc;

use fleet_api::{build_app, AppState};
use fleet_core::{Agent, AgentDesiredStatus, AgentId, AgentObservedStatus, Project, WorkerId};
use fleet_store::mem::MemStore;
use fleet_store::{SlotClaim, Store};
use fleet_worker::{RegistrationClient, WorkerConfig};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// 인자를 무시하고 오래 자는 가짜 grok.
fn fake_grok(dir: &std::path::Path) -> String {
    use std::io::Write;
    let path = dir.join("fake-grok.sh");
    let mut f = std::fs::File::create(&path).unwrap();
    // `--version`에 **즉시** 답해야 한다. `heartbeat_once`가 첫 beat에서
    // `grok --version`을 블로킹 `std::process::Command::output()`으로 부르므로
    // (`detect_grok_version`), 인자를 무시하고 자는 스크립트를 주면 heartbeat이
    // 자식 수명만큼 런타임 스레드를 붙잡는다 — 실제 grok은 즉시 답한다.
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

/// 주어진 주소에 오케스트레이터를 띄운다. 같은 주소로 다시 부를 수 있어야
/// 하므로 listener를 함수 안에서 bind한다 — 회복이 "같은 자리에 다시 나타남"
/// 이어야 워커의 설정을 건드리지 않는다.
async fn serve_at(addr: SocketAddr, store: Arc<dyn Store>) -> JoinHandle<()> {
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let app = build_app(Arc::new(AppState::new(store)));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    })
}

/// 워커와 오케스트레이터 **사이**의 네트워크. 파티션은 이것을 끊어 만든다.
///
/// 오케스트레이터 태스크를 `abort`하는 방식은 파티션이 되지 못한다 — 그것은
/// accept 루프만 끊고, 이미 맺어진 연결을 서빙하는 태스크는 그대로 살아남는다.
/// 워커의 `reqwest`는 연결을 풀에 넣어 재사용하므로(로그의 `pooling idle
/// connection`) 서버를 "죽인" 뒤에도 heartbeat이 계속 성공한다.
///
/// 그래서 연결 태스크들을 `JoinSet`에 담아 이 태스크가 **소유**한다. `abort`가
/// 그 `JoinSet`을 떨어뜨리면 소켓이 함께 닫혀 풀에 남은 연결도 끊긴다.
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

/// 조건이 참이 될 때까지 짧게 폴링한다. 고정 `sleep`으로 기다리면 느린
/// 머신에서 간헐 실패하고 빠른 머신에서 시간을 버린다.
async fn until<F, Fut>(what: &str, timeout: std::time::Duration, mut cond: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if cond().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("{what} — {timeout:?} 안에 일어나지 않았다");
}

async fn self_fenced_events(store: &Arc<dyn Store>) -> Vec<fleet_core::AuditEvent> {
    store
        .list_audit_events(&fleet_core::AuditFilter {
            actor_user_id: None,
            action: Some(fleet_core::audit::action::AGENT_SELF_FENCED.to_string()),
            limit: 100,
            offset: 0,
        })
        .await
        .expect("list_audit_events")
}

async fn observed(store: &Arc<dyn Store>, agent_id: AgentId) -> Option<AgentObservedStatus> {
    store
        .get_agent(agent_id)
        .await
        .expect("get_agent")
        .expect("agent row")
        .observed_status
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_partition_fences_the_agent_and_reconnecting_frees_it_for_another_worker() {
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn Store> = Arc::new(MemStore::new());

    // 포트를 먼저 잡아 두고 놓아준다 — 회복 때 **같은 주소**에 다시 띄워야
    // 워커가 설정된 URL 그대로 돌아온 제어면을 만난다.
    // 오케스트레이터는 끝까지 살아 있다. 끊는 것은 그 앞의 네트워크다.
    let backend = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap()
    };
    let _server = serve_at(backend, store.clone()).await;
    // 워커가 보는 주소. 회복 때 **같은 자리**에 다시 세워야 설정을 건드리지 않는다.
    let front = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap()
    };
    let mut net = link(front, backend).await;
    let url = format!("http://{front}");

    // 1. 워커 등록 — 실제 HTTP다.
    let config = Arc::new(
        WorkerConfig::for_test()
            .name("e2e-fenced-worker")
            .orchestrator_url(url.clone())
            .grok_bin(fake_grok(dir.path()))
            .agent_port_range("39700-39719")
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
        .expect("worker uuid");

    // 옮겨 갈 자리도 하나 만들어 둔다. 마지막 단정이 "다른 Worker로 갈 수
    // 있는가"이므로 후보가 없으면 그 단정이 성립하지 않는다.
    let other = Arc::new(
        RegistrationClient::new(Arc::new(
            WorkerConfig::for_test()
                .name("e2e-spare-worker")
                .orchestrator_url(url.clone())
                .build(),
        ))
        .unwrap(),
    );
    let other_id: WorkerId = other
        .register_once()
        .await
        .expect("register spare")
        .worker_id
        .parse()
        .unwrap();

    // 2. 그 워커에 배정된, 돌아야 하는 Agent 하나.
    let project = Project::new("e2e");
    store
        .create_project(&project)
        .await
        .expect("create project");
    let mut agent =
        Agent::new(project.id, "fenced-one").with_placement(worker_id, chrono::Utc::now());
    agent.desired_status = AgentDesiredStatus::Running;
    agent.command_generation = 1;
    let placed = store.create_agent(&agent).await.expect("create agent");
    assert_eq!(placed, Some(worker_id), "진단: 배정이 반영되지 않았다");
    let cmds = store
        .list_agent_commands(worker_id)
        .await
        .expect("commands");
    assert_eq!(cmds.len(), 1, "진단: 명령 목록이 비었다 — {cmds:?}");
    assert_eq!(cmds[0].desired_status, AgentDesiredStatus::Running);

    // 3. heartbeat 루프 시작. 유예 1초는 루프의 하한(주기+1초)에 걸려 2초가 된다.
    let manager = Arc::new(
        fleet_worker::AgentProcessManager::new(config.clone()).expect("agent process manager"),
    );
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let loop_handle = {
        let (c, m) = (client.clone(), manager.clone());
        tokio::spawn(async move {
            c.run_heartbeat_loop(1, 1, "127.0.0.1:1".into(), m, shutdown_rx)
                .await;
        })
    };

    // 4. 프로세스가 뜨고 그 사실이 오케스트레이터까지 닿는다.
    until(
        "Agent 프로세스가 뜨지 않았다",
        std::time::Duration::from_secs(20),
        || {
            let m = manager.clone();
            async move { !m.running_agents().await.is_empty() }
        },
    )
    .await;
    until(
        "관측이 오케스트레이터에 도달하지 않았다",
        std::time::Duration::from_secs(20),
        || {
            let (s, id) = (store.clone(), agent.id);
            async move { observed(&s, id).await == Some(AgentObservedStatus::Running) }
        },
    )
    .await;

    // 이 시점에 게이트 ②의 술어는 이 Agent를 붙잡고 있다. 그것이 옳다 —
    // 프로세스가 실제로 돌고 있으므로 옮기면 두 곳에서 도는 창이 열린다.
    assert_eq!(
        store
            .assign_agent_worker(agent.id, other_id, None)
            .await
            .unwrap(),
        SlotClaim::ObservedRunning,
        "돌고 있다고 보고된 Agent는 옮길 수 없어야 한다"
    );

    // 5. 단절 — 오케스트레이터를 죽여 포트를 닫는다. `abort` 뒤 join까지
    //    기다려야 listener가 실제로 drop되고 그 주소를 다시 쓸 수 있다.
    net.abort();
    let _ = (&mut net).await;

    // 6. 유예를 넘기면 워커가 스스로 멈춘다.
    until(
        "유예를 넘겼는데 프로세스가 남아 있다",
        std::time::Duration::from_secs(20),
        || {
            let m = manager.clone();
            async move { m.running_agents().await.is_empty() }
        },
    )
    .await;

    // **여기가 이 시험의 핵심이다.** 프로세스는 멈췄지만 오케스트레이터는
    // 아직 그것을 모르므로 관측은 `running`으로 남아 있고, Agent는 여전히
    // 묶여 있다. 즉 멈추는 것만으로는 풀리지 않는다.
    assert_eq!(
        observed(&store, agent.id).await,
        Some(AgentObservedStatus::Running),
        "단절 중에는 멈춘 사실이 오케스트레이터에 닿을 수 없다"
    );
    assert_eq!(
        store
            .assign_agent_worker(agent.id, other_id, None)
            .await
            .unwrap(),
        SlotClaim::ObservedRunning,
        "그래서 이 시점에도 아직 옮길 수 없다"
    );

    // 7. 회복 — 네트워크가 같은 자리에 돌아온다. 오케스트레이터는 내내 살아 있었다.
    let net = link(front, backend).await;

    // 8. **왜 멈췄는지가 도착한다.** 이것이 펜싱이 나르는 유일한 새 사실이다 —
    //    상태는 아래 9번이 보여주듯 저절로 되돌아가므로, 이 감사 줄이 없으면
    //    운영자에게는 Agent가 잠깐 사라졌다 돌아온 일이 아무 흔적도 남기지
    //    않는다.
    until(
        "재연결 뒤에도 펜싱 사건이 감사에 닿지 않았다",
        std::time::Duration::from_secs(20),
        || {
            let s = store.clone();
            async move { !self_fenced_events(&s).await.is_empty() }
        },
    )
    .await;

    let events = self_fenced_events(&store).await;
    assert_eq!(events.len(), 1, "펜싱 사건이 정확히 한 줄이어야 한다");
    assert_eq!(
        events[0].target_id.as_deref(),
        Some(agent.id.to_string().as_str())
    );
    assert_eq!(events[0].actor_label, format!("worker:{worker_id}"));
    assert!(
        events[0].detail["unreachable_secs"].as_u64().unwrap() >= 2,
        "실제로 잰 경과 시간이 실려야 한다 — {:?}",
        events[0].detail
    );

    // 9. 그리고 시스템은 **원래대로 수렴한다.** 오케스트레이터는 여전히 이
    //    Agent를 이 Worker에서 원하므로, 워커는 연결이 돌아오자마자 다시 띄우고
    //    관측도 `running`으로 되돌아간다.
    //
    //    **여기서 처음 확인한 사실이 하나 있다.** 이 시험을 쓰기 전에는 펜싱이
    //    "게이트 ②의 술어를 풀어 준다"고 적었었다 — 관측을 지우는 경로가 그
    //    Worker의 다음 heartbeat뿐이니, 멈추고 재연결하면 Agent가 자유로워진다는
    //    추론이었다. **틀렸다.** 재연결은 관측을 지우는 것이 아니라 되살린다.
    //    배정이 그대로인 한 그것이 옳은 동작이고, 갇힌 Agent를 실제로 푸는 것은
    //    운영자가 배치를 거두거나(`desired=stopped`) Worker를 지우는 것(036)이다.
    until(
        "재연결 뒤에 Agent가 다시 뜨지 않았다",
        std::time::Duration::from_secs(20),
        || {
            let (s, id) = (store.clone(), agent.id);
            async move { observed(&s, id).await == Some(AgentObservedStatus::Running) }
        },
    )
    .await;
    assert_eq!(
        store
            .assign_agent_worker(agent.id, other_id, None)
            .await
            .unwrap(),
        SlotClaim::ObservedRunning,
        "다시 돌고 있으므로 여전히 옮길 수 없다 — 펜싱은 이 술어를 풀지 않는다"
    );

    let _ = shutdown_tx.send(true);
    let _ = loop_handle.await;
    manager.shutdown_all().await;
    net.abort();
}
