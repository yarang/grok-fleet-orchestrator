//! MCP 표면의 감사 범위 테스트 (로드맵 `#95` 2단계).
//!
//! 정본([인가·감사](../../../docs/security/authorization-and-audit.md))은 "모든
//! mutation은 append-only audit event를 남긴다"를 요구한다. 이 표면은 그때까지
//! **한 건도 남기지 않았다** — 막고 있던 것은 `ToolContext`에 호출자 신원이
//! 없다는 것이었고, `#58`이 런처 주장 신원(`FLEET_MCP_PRINCIPAL`)을 넣으면서
//! 해소됐다.
//!
//! ## 왜 도구 카탈로그에서 시작하는가
//!
//! 감사 호출을 손으로 센 목록으로 검사하면, 나중에 누가 **새 mutation 도구를
//! 더할 때** 그 도구는 목록에 없으므로 테스트가 통과한다 — 즉 목록형 테스트는
//! 자기가 알고 있는 것만 지킨다. 그래서 판정의 출발점을 `schema::all_tools()`로
//! 두고, 모든 도구가 둘 중 하나로 **분류돼 있는지** 먼저 확인한다. 새 도구는
//! 어느 목록에도 없어 [`every_tool_is_classified`]가 실패하고, 저자는 그 도구가
//! mutation인지 아닌지를 **결정해야** 한다.

use std::sync::Arc;

use fleet_core::{AuditEvent, AuditFilter, PermissionKind};
use fleet_mcp::handlers::{dispatch_tool, ToolContext};
use fleet_mcp::schema;
use fleet_scheduler::{Dispatcher, FleetState};
use fleet_store::mem::MemStore;
use fleet_store::Store;
use fleet_transport::WorkerTransport;
use serde_json::{json, Value};

/// 상태를 바꾸는 도구. 여기 있는 이름은 전부 감사 이벤트를 남겨야 한다.
const MUTATING_TOOLS: &[&str] = &[
    schema::TOOL_DISPATCH_TASK,
    schema::TOOL_CANCEL_TASK,
    schema::TOOL_RESET_WORKER_BREAKER,
    schema::TOOL_REVOKE_BOOTSTRAP_TOKEN,
    schema::TOOL_CREATE_PROJECT,
    schema::TOOL_DELETE_PROJECT,
    schema::TOOL_CREATE_AGENT,
    schema::TOOL_START_AGENT,
    schema::TOOL_STOP_AGENT,
    schema::TOOL_PLACE_AGENT,
    schema::TOOL_CREATE_ISSUE,
    schema::TOOL_TRANSITION_ISSUE,
    schema::TOOL_COMMENT_ISSUE,
];

/// 읽기 전용 도구. 감사 대상이 아니다 — 정본이 요구하는 것은 mutation의
/// 기록이고, 조회까지 남기면 감사 테이블이 트래픽 로그가 된다.
const READ_ONLY_TOOLS: &[&str] = &[
    schema::TOOL_GET_TASK_STATUS,
    schema::TOOL_LIST_TASKS,
    schema::TOOL_WAIT_FOR_TASK,
    schema::TOOL_STREAM_TASK_OUTPUT,
    schema::TOOL_COLLECT_RESULTS,
    schema::TOOL_LIST_WORKERS,
    schema::TOOL_LIST_HOSTS,
    schema::TOOL_LIST_BOOTSTRAP_TOKENS,
    schema::TOOL_LIST_PROJECTS,
    schema::TOOL_LIST_AGENTS,
    schema::TOOL_LIST_ISSUES,
];

/// 런처가 신원을 주장한 배포를 흉내 낸다. 감사의 actor가 이 값과 **같은
/// 문자열**이어야 한다(`fleet_mcp::audit` 모듈 문서).
const PRINCIPAL: &str = "mcp:ci-runner";

#[test]
fn every_tool_is_classified() {
    let catalog: Vec<&str> = schema::all_tools().iter().map(|t| t.name).collect();
    let mut unclassified: Vec<&str> = catalog
        .iter()
        .copied()
        .filter(|n| !MUTATING_TOOLS.contains(n) && !READ_ONLY_TOOLS.contains(n))
        .collect();
    unclassified.sort_unstable();
    assert!(
        unclassified.is_empty(),
        "새 도구가 mutation인지 조회인지 분류되지 않았다: {unclassified:?}"
    );
    // 반대 방향도 본다 — 도구가 사라졌는데 목록에 남으면 아래 라운드트립
    // 테스트가 "존재하지 않는 도구를 감사한다"고 주장하게 된다.
    for name in MUTATING_TOOLS.iter().chain(READ_ONLY_TOOLS) {
        assert!(
            catalog.contains(name),
            "목록에 있으나 카탈로그에 없는 도구: {name}"
        );
    }
    assert_eq!(catalog.len(), MUTATING_TOOLS.len() + READ_ONLY_TOOLS.len());
}

/// 13개 mutation 도구를 실제로 한 번씩 호출하고, **각각이** 감사 이벤트를
/// 남겼는지를 도구 이름 단위로 판정한다.
///
/// 총 건수만 세지 않는 이유는 그것이 구별력을 잃기 때문이다 — 한 도구가 두
/// 줄을 남기고 다른 도구가 한 줄도 남기지 않아도 합계는 맞는다.
#[tokio::test]
async fn every_mutating_tool_records_an_audit_event() {
    let h = harness();

    let mut missing: Vec<&str> = Vec::new();
    for tool in MUTATING_TOOLS {
        let produced = exercise(&h, tool).await;
        if produced.is_empty() {
            missing.push(tool);
            continue;
        }
        // 이 표면의 actor는 언제나 런처가 주장한 신원이다. `actor_user_id`는
        // `users` 행에 대응하는 주체가 없으므로 항상 `None`이며, 그것이
        // 문자열이 아니라 사람 행위와 이 표면을 DB에서 가르는 필드다.
        for e in &produced {
            assert_eq!(e.actor_label, PRINCIPAL, "{tool}의 actor");
            assert!(e.actor_user_id.is_none(), "{tool}의 actor_user_id");
        }
    }

    assert!(
        missing.is_empty(),
        "감사 이벤트를 남기지 않은 mutation 도구: {missing:?}"
    );
}

/// 조회 도구는 아무것도 남기지 않는다.
#[tokio::test]
async fn read_only_tools_record_nothing() {
    let h = harness();
    // 조회 대상이 존재하는 상태에서 돌린다 — 전부 "없음"으로 끝나면 이
    // 테스트는 조기 반환 경로만 보게 된다. 세계를 만드는 것 자체가 mutation
    // 도구를 쓰므로, 기준선은 seed **뒤에** 잡는다.
    let world = seed(&h).await;
    let baseline = event_ids(&h.store).await;

    for tool in READ_ONLY_TOOLS {
        let args = read_args(tool, &world);
        let _ = dispatch_tool(&h.ctx, tool, &args).await;
    }

    let added = new_events_since(&h.store, &baseline).await;
    assert!(
        added.is_empty(),
        "조회 도구가 감사 행을 남겼다: {:?}",
        added.iter().map(|e| e.action.clone()).collect::<Vec<_>>()
    );
}

/// 런처가 신원을 주지 않으면 actor는 기존 배포와 같은 `"mcp"` 한 버킷이다.
/// 이것이 리소스의 `created_by`와 같은 값이라는 것이 이 테스트의 요지다 —
/// 둘이 갈라지면 "누가 만들었나"와 "누가 그 행위를 했나"를 맞대 볼 수 없다.
#[tokio::test]
async fn without_a_launcher_principal_the_actor_is_the_default_bucket() {
    let h = harness_with_principal(None);

    let created = dispatch_tool(
        &h.ctx,
        schema::TOOL_CREATE_PROJECT,
        &json!({"name": "defaulted"}),
    )
    .await
    .unwrap();
    let project_id = tool_body(&created)["id"].as_str().unwrap().to_string();

    let issue = dispatch_tool(
        &h.ctx,
        schema::TOOL_CREATE_ISSUE,
        &json!({"project_id": project_id, "title": "who wrote this"}),
    )
    .await
    .unwrap();

    let events = all_events(&h.store).await;
    assert!(!events.is_empty());
    for e in &events {
        assert_eq!(e.actor_label, "mcp");
    }
    // 리소스의 author와 감사의 actor가 같은 문자열이다.
    assert_eq!(tool_body(&issue)["created_by"], "mcp");
}

/// Issue 작성자도 런처가 주장한 신원을 따른다 (`#58`).
#[tokio::test]
async fn issue_author_matches_the_audit_actor() {
    let h = harness();
    let world = seed(&h).await;

    let issue = tool_body(
        &dispatch_tool(
            &h.ctx,
            schema::TOOL_CREATE_ISSUE,
            &json!({"project_id": world.project_id, "title": "authored"}),
        )
        .await
        .unwrap(),
    );
    assert_eq!(issue["created_by"], PRINCIPAL);

    let comment = tool_body(
        &dispatch_tool(
            &h.ctx,
            schema::TOOL_COMMENT_ISSUE,
            &json!({"issue_id": issue["id"], "body": "same author"}),
        )
        .await
        .unwrap(),
    );
    assert_eq!(comment["author"], PRINCIPAL);

    // seed도 Issue를 하나 만들므로 건수가 아니라 **이 Issue를 가리키는 줄**을
    // 집어 본다.
    let issue_id = issue["id"].as_str().unwrap();
    for action in [
        fleet_core::audit::action::ISSUE_CREATE,
        fleet_core::audit::action::ISSUE_COMMENT,
    ] {
        let rows: Vec<_> = events_for(&h.store, action)
            .await
            .into_iter()
            .filter(|e| e.target_id.as_deref() == Some(issue_id))
            .collect();
        assert_eq!(rows.len(), 1, "{action}");
        assert_eq!(rows[0].actor_label, PRINCIPAL);
        // Project 범위 감사 질의가 이 줄을 찾을 수 있어야 한다 (`#95` 1단계가
        // `project_id`를 자유 형식 JSON이 아니라 컬럼으로 둔 이유).
        assert_eq!(
            rows[0].project_id.map(|p| p.to_string()).as_deref(),
            Some(world.project_id.as_str())
        );
    }
}

/// 멱등 흡수는 Task 행을 만들지 않으므로 감사 줄도 만들지 않는다 — Dashboard
/// `POST /api/tasks`와 같은 규칙이다. 기록하면 감사 행 수가 "몇 개가
/// 제출됐는가"가 아니라 "몇 번 요청했는가"를 센다.
#[tokio::test]
async fn an_idempotent_resubmit_does_not_add_a_second_audit_row() {
    let h = harness();
    register_worker(&h).await;

    let args = json!({
        "prompt": "build",
        "cwd": "/tmp/fleet-audit-test",
        "idempotency_key": "same-key",
    });
    let first = tool_body(
        &dispatch_tool(&h.ctx, schema::TOOL_DISPATCH_TASK, &args)
            .await
            .unwrap(),
    );
    let second = tool_body(
        &dispatch_tool(&h.ctx, schema::TOOL_DISPATCH_TASK, &args)
            .await
            .unwrap(),
    );
    assert_eq!(second["deduplicated"], true);
    assert_eq!(first["task_id"], second["task_id"]);

    let rows = events_for(&h.store, fleet_core::audit::action::TASK_SUBMIT).await;
    assert_eq!(rows.len(), 1, "중복 제출이 감사 줄을 하나 더 만들었다");
}

/// 취소 사유는 호출자가 넣는 임의 문자열이라 자격증명이 섞일 수 있다.
/// `TASK_SUBMIT`이 prompt를 빼는 것과 같은 이유로 본문을 싣지 않는다.
#[tokio::test]
async fn the_cancel_reason_body_never_reaches_the_audit_row() {
    const REASON: &str = "token=hunter2-should-not-persist";
    let h = harness();
    register_worker(&h).await;

    let submitted = tool_body(
        &dispatch_tool(
            &h.ctx,
            schema::TOOL_DISPATCH_TASK,
            &json!({"prompt": "work", "cwd": "/tmp/fleet-audit-test"}),
        )
        .await
        .unwrap(),
    );
    let task_id = submitted["task_id"].as_str().unwrap().to_string();

    dispatch_tool(
        &h.ctx,
        schema::TOOL_CANCEL_TASK,
        &json!({"task_id": task_id, "reason": REASON}),
    )
    .await
    .unwrap();

    let rows = events_for(&h.store, fleet_core::audit::action::TASK_CANCEL).await;
    assert_eq!(rows.len(), 1);
    let rendered = serde_json::to_string(&rows[0]).unwrap();
    assert!(
        !rendered.contains(REASON),
        "취소 사유 본문이 감사 행에 실렸다: {rendered}"
    );
    assert_eq!(
        rows[0].detail["reason_len"].as_u64(),
        Some(REASON.chars().count() as u64)
    );
}

// ── 하네스 ───────────────────────────────────────────────────────────────

/// 테스트 하네스. transport를 들고 있는 이유는 Worker를 **양쪽에** 등록해야
/// 하기 때문이다 — 저장소에만 넣으면 dispatch가 "worker is not registered"로
/// 끝나고, 그러면 감사 경로가 밟히지 않는다.
struct Harness {
    ctx: ToolContext,
    store: Arc<dyn Store>,
    transport: Arc<fleet_transport::MockTransport>,
}

fn harness() -> Harness {
    harness_with_principal(Some(PRINCIPAL))
}

fn harness_with_principal(principal: Option<&str>) -> Harness {
    let store: Arc<dyn Store> = Arc::new(MemStore::new());
    let transport = Arc::new(fleet_transport::MockTransport::new());
    let state = Arc::new(FleetState::new(
        store.clone(),
        transport.clone() as Arc<dyn fleet_transport::WorkerTransport>,
        fleet_core::CircuitBreakerConfig::default(),
    ));
    let dispatcher = Arc::new(Dispatcher::new(state.clone()));
    let mut ctx =
        ToolContext::new(state, dispatcher).with_capabilities(PermissionKind::all().to_vec());
    if let Some(p) = principal {
        ctx = ctx.with_created_by(p.to_string());
    }
    Harness {
        ctx,
        store,
        transport,
    }
}

async fn all_events(store: &Arc<dyn Store>) -> Vec<AuditEvent> {
    store
        .list_audit_events(&AuditFilter {
            limit: 1000,
            ..Default::default()
        })
        .await
        .expect("list_audit_events")
}

/// 지금까지 남은 감사 행의 id 집합.
///
/// "직전 호출이 무엇을 남겼는가"를 **개수나 정렬 순서가 아니라 id 차집합**으로
/// 본다. 저장소는 `created_at` 역순으로 주는데 같은 마이크로초에 들어온 두 행의
/// 상대 순서는 정해져 있지 않다 — 순서에 기대면 그 순간에만 깨지는 테스트가
/// 된다.
async fn event_ids(store: &Arc<dyn Store>) -> Vec<uuid::Uuid> {
    all_events(store).await.iter().map(|e| e.id).collect()
}

async fn new_events_since(store: &Arc<dyn Store>, baseline: &[uuid::Uuid]) -> Vec<AuditEvent> {
    all_events(store)
        .await
        .into_iter()
        .filter(|e| !baseline.contains(&e.id))
        .collect()
}

async fn events_for(store: &Arc<dyn Store>, action: &str) -> Vec<AuditEvent> {
    store
        .list_audit_events(&AuditFilter {
            action: Some(action.to_string()),
            limit: 1000,
            ..Default::default()
        })
        .await
        .expect("list_audit_events")
}

fn tool_body(out: &Value) -> Value {
    let text = out["content"][0]["text"].as_str().expect("tool text");
    serde_json::from_str(text).unwrap_or_else(|e| panic!("도구 결과가 JSON이 아니다 ({e}): {out}"))
}

fn field(out: &Value, key: &str) -> String {
    tool_body(out)[key]
        .as_str()
        .unwrap_or_else(|| panic!("도구 결과에 `{key}`가 없다: {}", tool_body(out)))
        .to_string()
}

/// Worker를 저장소와 transport **양쪽에** 등록한다.
///
/// dispatch는 두 곳을 다 본다 — 저장소에서 후보를 고르고 transport로 보낸다.
/// 한쪽만 채우면 도구가 `isError`로 끝나 감사 경로가 밟히지 않는데, 합계만
/// 보는 테스트는 그것을 구분하지 못한다.
async fn register_worker(h: &Harness) -> fleet_core::WorkerId {
    let worker = fleet_core::Worker::new(
        format!("w-{}", uuid::Uuid::new_v4()),
        "wss://seed.invalid/ws",
    );
    let id = worker.id;
    h.store.upsert_worker(&worker).await.unwrap();
    h.transport
        .register(id, &worker.endpoint, worker.max_concurrent)
        .await
        .expect("mock transport register");
    // `register()`가 기본값으로 MockWorker를 덮어쓰므로 **그 뒤에** 넣는다.
    // 제출된 Task가 실행 중으로 남아 있어야 취소가 의미를 갖는다 — 기본
    // 지연(10ms)이면 취소 시점에 이미 끝나 있다.
    let mut mock = fleet_transport::MockWorker::new(id, worker.endpoint.clone());
    mock.latency = std::time::Duration::from_secs(300);
    mock.max_concurrent = worker.max_concurrent;
    h.transport.add_worker(mock).await;
    id
}

/// 라운드트립에 필요한 최소 세계. 도구로 만들 수 있는 것은 도구로 만든다 —
/// 저장소에 직접 넣으면 그 도구의 감사 경로가 이 테스트에서 한 번도 밟히지
/// 않는다.
struct World {
    project_id: String,
    agent_id: String,
    worker_id: String,
    issue_id: String,
    task_id: String,
    token_id: String,
}

async fn seed(h: &Harness) -> World {
    let worker_id = register_worker(h).await;

    let raw_token = format!("fleet_seed_{}", uuid::Uuid::new_v4());
    h.store
        .create_bootstrap_token(&fleet_core::BootstrapToken {
            token_digest: fleet_core::BootstrapToken::digest_for(&raw_token),
            created_at: chrono::Utc::now(),
            created_by: None,
            expires_at: None,
            max_uses: 1,
            use_count: 0,
            notes: None,
            last_used_by: None,
            last_used_at: None,
        })
        .await
        .unwrap();

    let project_id = field(
        &dispatch_tool(
            &h.ctx,
            schema::TOOL_CREATE_PROJECT,
            &json!({"name": format!("seed-{}", uuid::Uuid::new_v4())}),
        )
        .await
        .unwrap(),
        "id",
    );

    let agent_id = field(
        &dispatch_tool(
            &h.ctx,
            schema::TOOL_CREATE_AGENT,
            &json!({"project_id": project_id, "name": "seed-agent"}),
        )
        .await
        .unwrap(),
        "id",
    );

    let issue_id = field(
        &dispatch_tool(
            &h.ctx,
            schema::TOOL_CREATE_ISSUE,
            &json!({"project_id": project_id, "title": "seed issue"}),
        )
        .await
        .unwrap(),
        "id",
    );

    let task_id = field(
        &dispatch_tool(
            &h.ctx,
            schema::TOOL_DISPATCH_TASK,
            &json!({
                "prompt": "seed task",
                "cwd": "/tmp/fleet-audit-test",
                "project_id": project_id,
            }),
        )
        .await
        .unwrap(),
        "task_id",
    );

    World {
        project_id,
        agent_id,
        worker_id: worker_id.to_string(),
        issue_id,
        token_id: fleet_core::BootstrapToken::public_id_for(&raw_token),
        task_id,
    }
}

/// 한 도구를 "성공하는 모양"으로 한 번 호출하고, **그 호출이** 남긴 감사 행만
/// 돌려준다.
///
/// 도구마다 세계를 새로 만든다 — 앞 도구가 남긴 상태(회수된 Agent, archive된
/// Project)가 뒤 도구를 조기 반환시키면 그 도구의 감사 경로가 밟히지 않는데,
/// 합계만 보는 테스트는 그것을 통과로 읽는다. 세계를 만드는 일 자체가 mutation
/// 도구를 쓰므로 기준선은 seed **뒤에** 잡는다.
async fn exercise(h: &Harness, tool: &str) -> Vec<AuditEvent> {
    let w = seed(h).await;
    let args = match tool {
        schema::TOOL_DISPATCH_TASK => json!({
            "prompt": "hello",
            "cwd": "/tmp/fleet-audit-test",
            "project_id": w.project_id,
        }),
        schema::TOOL_CANCEL_TASK => json!({"task_id": w.task_id, "reason": "no longer needed"}),
        schema::TOOL_RESET_WORKER_BREAKER => json!({"worker_id": w.worker_id}),
        schema::TOOL_REVOKE_BOOTSTRAP_TOKEN => json!({"token_id": w.token_id}),
        schema::TOOL_CREATE_PROJECT => json!({"name": format!("p-{}", uuid::Uuid::new_v4())}),
        schema::TOOL_DELETE_PROJECT => json!({"project_id": w.project_id}),
        schema::TOOL_CREATE_AGENT => json!({"project_id": w.project_id, "name": "fresh"}),
        schema::TOOL_START_AGENT | schema::TOOL_STOP_AGENT => json!({"agent_id": w.agent_id}),
        schema::TOOL_PLACE_AGENT => json!({"agent_id": w.agent_id, "worker_id": w.worker_id}),
        schema::TOOL_CREATE_ISSUE => json!({"project_id": w.project_id, "title": "fresh issue"}),
        schema::TOOL_TRANSITION_ISSUE => json!({"issue_id": w.issue_id, "status": "triaged"}),
        schema::TOOL_COMMENT_ISSUE => json!({"issue_id": w.issue_id, "body": "a comment"}),
        other => panic!("분류되었으나 호출 방법이 적히지 않은 도구: {other}"),
    };
    let baseline = event_ids(&h.store).await;
    let out = dispatch_tool(&h.ctx, tool, &args)
        .await
        .unwrap_or_else(|e| panic!("{tool} 호출이 JSON-RPC 에러로 끝났다: {}", e.message));
    assert_ne!(
        out["isError"], true,
        "{tool} 호출이 도구 에러로 끝났다 — 감사 경로가 밟히지 않는다: {out}"
    );
    new_events_since(&h.store, &baseline).await
}

fn read_args(tool: &str, w: &World) -> Value {
    match tool {
        schema::TOOL_GET_TASK_STATUS => json!({"task_id": w.task_id}),
        schema::TOOL_COLLECT_RESULTS => json!({"task_ids": [w.task_id]}),
        // 완료를 기다리지 않는다 — 이 테스트가 보는 것은 감사 부작용이지
        // 완료 여부가 아니다.
        schema::TOOL_WAIT_FOR_TASK => json!({"task_id": w.task_id, "timeout_secs": 1}),
        schema::TOOL_STREAM_TASK_OUTPUT => json!({"task_id": w.task_id, "max_polls": 1}),
        schema::TOOL_LIST_AGENTS | schema::TOOL_LIST_ISSUES => {
            json!({"project_id": w.project_id})
        }
        _ => json!({}),
    }
}
