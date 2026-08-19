//! Worker LLM credential 하위 자원의 capability 인가 + 감사 기록 (로드맵 #66).
//!
//! 배경: `GET /v1/workers/:name/credentials`, `.../credentials/:model/export`,
//! `PUT`/`DELETE`가 capability 행렬에 등록돼 있지 않아, **인증만 통과하면**
//! (capability가 빈 principal이나 워커 자신의 operational 토큰으로도) 어떤
//! 워커의 LLM 프로바이더 API 키든 평문으로 꺼낼 수 있었다. 감사 기록도 없었다.
//!
//! 이 테스트는 "인증 != 인가"를 route 단위로 고정한다.

use std::sync::Arc;

use chrono::Utc;
use fleet_api::{build_app, ApiTokenCredential, AppState};
use fleet_core::{AuditFilter, BootstrapToken, PermissionKind, Worker};
use fleet_credentials::MasterKey;
use fleet_store::mem::MemStore;
use fleet_store::{Store, WorkerOperationalCredential};

const WORKER: &str = "cred-authz-worker";
const MODEL: &str = "grok-4";
const PLAINTEXT_KEY: &str = "sk-super-secret-provider-key";

struct Fixture {
    url: String,
    store: Arc<MemStore>,
    /// 워커 자신을 인증하는 operational 토큰(`fwo_...`). LLM credential과는
    /// 완전히 다른 비밀 — 이 토큰으로 LLM 키를 꺼낼 수 있으면 안 된다.
    worker_token: String,
}

/// 워커 1대 + 암호화된 LLM credential 1건을 심고 서버를 띄운다.
///
/// 토큰별 capability는 최소 권한으로 쪼개 둔다 — 어떤 capability가 어떤
/// route를 여는지 테스트가 직접 증명하도록.
async fn setup() -> Fixture {
    let store = Arc::new(MemStore::new());
    let dyn_store: Arc<dyn Store> = store.clone();

    let worker = Worker::new(WORKER, format!("ws://{WORKER}.local/ws"));
    let worker_id = worker.id;
    dyn_store.upsert_worker(&worker).await.unwrap();

    let worker_token = format!("fwo_test_{WORKER}");
    dyn_store
        .upsert_worker_operational_credential(&WorkerOperationalCredential {
            worker_id,
            credential_digest: BootstrapToken::digest_for(&worker_token),
            issued_at: Utc::now(),
            expires_at: None,
            revoked_at: None,
            rotation_generation: 1,
        })
        .await
        .unwrap();

    let master_key = MasterKey::generate();
    let blob = master_key.encrypt(PLAINTEXT_KEY.as_bytes()).unwrap();
    dyn_store
        .upsert_worker_credential(
            WORKER,
            MODEL,
            blob.as_str(),
            "https://api.x.ai/v1",
            "openai",
            131_072,
            Some("grok-4"),
        )
        .await
        .unwrap();

    let state = AppState::new(dyn_store)
        .with_master_key(master_key)
        .with_tokens(vec![
            // 인증은 되지만 LLM credential 권한이 전혀 없는 주체.
            ApiTokenCredential {
                principal_id: "no-cred-caps".into(),
                token: "token-none".into(),
                capabilities: vec![PermissionKind::WorkerList],
            },
            // 워커 삭제 권한만 있는 주체 — LLM credential 삭제로 번지면 안 된다.
            ApiTokenCredential {
                principal_id: "worker-deleter".into(),
                token: "token-worker-delete".into(),
                capabilities: vec![PermissionKind::WorkerDelete],
            },
            ApiTokenCredential {
                principal_id: "reader".into(),
                token: "token-read".into(),
                capabilities: vec![PermissionKind::WorkerLlmCredentialRead],
            },
            // 프로비저너 역할: 목록 조회 + 평문 export만. 저장/삭제는 불가.
            ApiTokenCredential {
                principal_id: "provisioner".into(),
                token: "token-export".into(),
                capabilities: vec![
                    PermissionKind::WorkerLlmCredentialRead,
                    PermissionKind::WorkerLlmCredentialExport,
                ],
            },
            // 저장/삭제 담당: export는 불가.
            ApiTokenCredential {
                principal_id: "cred-manager".into(),
                token: "token-manage".into(),
                capabilities: vec![PermissionKind::WorkerLlmCredentialManage],
            },
        ]);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(Arc::new(state));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Fixture {
        url: format!("http://{addr}"),
        store,
        worker_token,
    }
}

async fn get_status(url: &str, path: &str, token: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("{url}{path}"))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
}

fn export_path() -> String {
    format!("/v1/workers/{WORKER}/credentials/{MODEL}/export")
}

fn list_path() -> String {
    format!("/v1/workers/{WORKER}/credentials")
}

async fn audit_actions(store: &Arc<MemStore>, action: &str) -> Vec<fleet_core::AuditEvent> {
    store
        .list_audit_events(&AuditFilter {
            action: Some(action.to_string()),
            ..Default::default()
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn export_without_capability_is_forbidden() {
    let fx = setup().await;
    let resp = get_status(&fx.url, &export_path(), "token-none").await;
    assert_eq!(
        resp.status(),
        403,
        "authenticated-but-uncapable principal must not read plaintext API keys"
    );
    let body = resp.text().await.unwrap();
    assert!(
        !body.contains(PLAINTEXT_KEY),
        "denied response must not leak the API key"
    );
    assert!(
        audit_actions(
            &fx.store,
            fleet_core::audit::action::WORKER_LLM_CREDENTIAL_EXPORT
        )
        .await
        .is_empty(),
        "denied export must not record a success event"
    );
}

#[tokio::test]
async fn export_with_capability_succeeds_and_is_audited() {
    let fx = setup().await;
    let resp = get_status(&fx.url, &export_path(), "token-export").await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["api_key"], PLAINTEXT_KEY);

    let events = audit_actions(
        &fx.store,
        fleet_core::audit::action::WORKER_LLM_CREDENTIAL_EXPORT,
    )
    .await;
    assert_eq!(events.len(), 1, "successful export must be audited");
    let event = &events[0];
    assert_eq!(event.actor_label, "provisioner");
    assert_eq!(event.target_type.as_deref(), Some("worker_llm_credential"));
    assert_eq!(
        event.target_id.as_deref(),
        Some(&*format!("{WORKER}/{MODEL}"))
    );
    // 감사 로그에 비밀 값이 실려서는 안 된다.
    assert!(!event.detail.to_string().contains(PLAINTEXT_KEY));
}

#[tokio::test]
async fn manage_capability_alone_cannot_export_plaintext() {
    // 저장/삭제 권한과 평문 열람 권한은 분리돼 있어야 한다.
    let fx = setup().await;
    let resp = get_status(&fx.url, &export_path(), "token-manage").await;
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn worker_operational_token_cannot_export_its_own_llm_credential() {
    // 워커의 operational identity(`fwo_`)는 자신을 control plane에 인증하기
    // 위한 것이지, LLM API 키를 셀프서비스로 꺼내기 위한 것이 아니다.
    let fx = setup().await;
    let resp = get_status(&fx.url, &export_path(), &fx.worker_token).await;
    assert_eq!(
        resp.status(),
        403,
        "worker must not export even its own LLM credential"
    );
    let body = resp.text().await.unwrap();
    assert!(!body.contains(PLAINTEXT_KEY));
}

#[tokio::test]
async fn list_requires_read_capability() {
    let fx = setup().await;
    assert_eq!(
        get_status(&fx.url, &list_path(), "token-none")
            .await
            .status(),
        403
    );
    assert_eq!(
        get_status(&fx.url, &list_path(), &fx.worker_token)
            .await
            .status(),
        403,
        "worker operational token must not enumerate LLM credentials"
    );
    assert_eq!(
        get_status(&fx.url, &list_path(), "token-read")
            .await
            .status(),
        200
    );
}

#[tokio::test]
async fn delete_requires_llm_manage_not_worker_delete() {
    let fx = setup().await;
    let client = reqwest::Client::new();
    let path = format!("{}/v1/workers/{WORKER}/credentials/{MODEL}", fx.url);

    // worker:delete는 워커 엔티티 삭제 권한이지 credential 삭제 권한이 아니다.
    let resp = client
        .delete(&path)
        .header("authorization", "Bearer token-worker-delete")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);

    // 워커 자신의 operational 토큰도 마찬가지(WorkerDelete를 갖고 있다).
    let resp = client
        .delete(&path)
        .header("authorization", format!("Bearer {}", fx.worker_token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);

    let resp = client
        .delete(&path)
        .header("authorization", "Bearer token-manage")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let events = audit_actions(
        &fx.store,
        fleet_core::audit::action::WORKER_LLM_CREDENTIAL_DELETE,
    )
    .await;
    assert_eq!(events.len(), 1, "credential delete must be audited");
    assert_eq!(events[0].actor_label, "cred-manager");
}

#[tokio::test]
async fn put_requires_llm_manage_capability() {
    let fx = setup().await;
    let client = reqwest::Client::new();
    let path = format!("{}/v1/workers/{WORKER}/credentials", fx.url);
    let body = serde_json::json!({
        "model_id": "grok-4-fast",
        "api_key": "sk-another-key",
        "base_url": "https://api.x.ai/v1",
        "api_backend": "openai",
        "context_window": 131072,
    });

    for token in ["token-none", "token-read", "token-export"] {
        let resp = client
            .put(&path)
            .header("authorization", format!("Bearer {token}"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            403,
            "{token} must not be able to overwrite LLM credentials"
        );
    }

    let resp = client
        .put(&path)
        .header("authorization", "Bearer token-manage")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        audit_actions(
            &fx.store,
            fleet_core::audit::action::WORKER_LLM_CREDENTIAL_PUT
        )
        .await
        .len(),
        1
    );
}
