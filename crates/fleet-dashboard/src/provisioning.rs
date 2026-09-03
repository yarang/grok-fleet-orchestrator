//! SSH 키 관리 + 원격 호스트 프로비저닝 API 핸들러.
//!
//! 대시보드에서 직접 원격 호스트를 프로비저닝할 수 있게 한다.
//! SSH 비밀키는 `MasterKey` (AES-256-GCM)로 암호화하여 DB에 저장.

use crate::error::ApiError;
use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use fleet_core::{HostStatus, SshKey, SshKeySummary};
use fleet_provisioner::playbook::{detect_prereq, Playbook, PlaybookContext, StepStatus};
use fleet_provisioner::ssh::{HostKeyConfig, SshClient, SshConnectInfo};
use fleet_provisioner::steps::StepContext;

use crate::app::DashboardState;
use crate::auth::{require_permission, AuthPrincipal};

// ── SSH 키 관리 ─────────────────────────────────────────────────────────

/// POST /api/ssh-keys — SSH 비밀키 업로드 (암호화 저장).
pub async fn create_ssh_key_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Json(req): Json<CreateSshKeyRequest>,
) -> Result<StatusCode, ApiError> {
    require_permission(
        &state,
        &principal,
        fleet_core::PermissionKind::HostProvision,
    )
    .await
    .map_err(|_| ApiError::Forbidden("Insufficient permissions".into()))?;

    // MasterKey 필수.
    let master_key = state
        .master_key
        .as_ref()
        .ok_or_else(|| ApiError::Unavailable("Master key not configured".into()))?;

    // 입력 검증.
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name must not be empty".into()));
    }
    if !req.private_key.contains("BEGIN OPENSSH PRIVATE KEY")
        && !req.private_key.contains("BEGIN RSA PRIVATE KEY")
        && !req.private_key.contains("BEGIN EC PRIVATE KEY")
    {
        return Err(ApiError::BadRequest(
            "private_key does not look like a valid PEM key".into(),
        ));
    }

    // 키 타입 감지.
    let key_type = detect_key_type(&req.private_key);

    // fingerprint 계산 — 공개키의 SHA-256 fingerprint.
    let fingerprint = compute_fingerprint(&req.private_key);

    // 암호화.
    let encrypted_blob = master_key
        .encrypt(req.private_key.as_bytes())
        .map_err(|e| {
            tracing::error!(error = %e, "failed to encrypt SSH key");
            ApiError::Internal("Encryption failed".into())
        })?;

    let key = SshKey {
        id: Uuid::new_v4(),
        name: req.name.trim().to_string(),
        encrypted_blob: encrypted_blob.as_str().to_string(),
        fingerprint,
        key_type,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    state.store.create_ssh_key(&key).await.map_err(|e| {
        tracing::error!(error = %e, "create_ssh_key failed");
        ApiError::Internal("DB error".into())
    })?;

    tracing::info!(
        key_name = %key.name,
        key_type = %key.key_type,
        username = %principal.user.username,
        "SSH key uploaded"
    );

    Ok(StatusCode::CREATED)
}

/// GET /api/ssh-keys — SSH 키 목록 (메타데이터만).
pub async fn list_ssh_keys_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<Vec<SshKeySummary>>, StatusCode> {
    require_permission(
        &state,
        &principal,
        fleet_core::PermissionKind::HostProvision,
    )
    .await?;

    let keys = state.store.list_ssh_keys().await.map_err(|e| {
        tracing::error!(error = %e, "list_ssh_keys failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let summaries = keys
        .into_iter()
        .map(|k| SshKeySummary {
            name: k.name,
            fingerprint: k.fingerprint,
            key_type: k.key_type,
            created_at: k.created_at,
        })
        .collect();

    Ok(Json(summaries))
}

/// DELETE /api/ssh-keys/:name — SSH 키 삭제.
pub async fn delete_ssh_key_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_permission(
        &state,
        &principal,
        fleet_core::PermissionKind::HostProvision,
    )
    .await
    .map_err(|_| ApiError::Forbidden("Insufficient permissions".into()))?;

    let deleted = state.store.delete_ssh_key(&name).await.map_err(|e| {
        tracing::error!(error = %e, "delete_ssh_key failed");
        ApiError::Internal("DB error".into())
    })?;

    if !deleted {
        return Err(ApiError::NotFound("SSH key not found".into()));
    }

    tracing::info!(
        key_name = %name,
        username = %principal.user.username,
        "SSH key deleted"
    );

    Ok(StatusCode::NO_CONTENT)
}

// ── 프로비저닝 ──────────────────────────────────────────────────────────

/// POST /api/hosts/provision — 원격 호스트 프로비저닝 트리거.
pub async fn provision_host_api(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Json(req): Json<ProvisionRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_permission(
        &state,
        &principal,
        fleet_core::PermissionKind::HostProvision,
    )
    .await
    .map_err(|_| ApiError::Forbidden("Insufficient permissions".into()))?;

    // MasterKey 필수.
    let master_key = state
        .master_key
        .as_ref()
        .ok_or_else(|| ApiError::Unavailable("Master key not configured".into()))?;

    // SSH 키 조회.
    let ssh_key = state
        .store
        .get_ssh_key(&req.ssh_key_name)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "get_ssh_key failed");
            ApiError::Internal("DB error".into())
        })?
        .ok_or_else(|| ApiError::NotFound(format!("SSH key '{}' not found", req.ssh_key_name)))?;

    // 비밀키 복호화.
    let decrypted_key = {
        let blob = fleet_credentials::EncryptedBlob::from_string(&ssh_key.encrypted_blob);
        master_key.decrypt(&blob).map_err(|e| {
            tracing::error!(error = %e, "failed to decrypt SSH key");
            ApiError::Internal("Decryption failed".into())
        })?
    };

    // 임시 파일에 쓰기.
    let temp_key_path = format!("/tmp/.fleet-ssh-key-{}", uuid::Uuid::new_v4().simple());
    std::fs::write(&temp_key_path, &decrypted_key).map_err(|e| {
        tracing::error!(error = %e, "failed to write temp key file");
        ApiError::Internal("Failed to write key file".into())
    })?;
    // 0600 권한 설정.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        let _ = std::fs::set_permissions(&temp_key_path, perms);
    }

    // 프로비저닝 실행 — ensure temp key is cleaned up.
    let result = run_provisioning(&state, &req, &temp_key_path, &principal).await;

    // 임시 키 파일 삭제 (성공/실패와 무관).
    let _ = std::fs::remove_file(&temp_key_path);

    result
}

/// 실제 프로비저닝 실행.
async fn run_provisioning(
    state: &DashboardState,
    req: &ProvisionRequest,
    key_path: &str,
    principal: &AuthPrincipal,
) -> Result<Json<serde_json::Value>, ApiError> {
    // grok_secret 자동 생성 (비어 있으면).
    let grok_secret = if req.grok_secret.is_empty() {
        hex::encode(rand::random::<[u8; 32]>())
    } else {
        req.grok_secret.clone()
    };

    // fleet-worker 바이너리 경로 기본값.
    let fleet_worker_bin = if req.fleet_worker_bin.is_empty() {
        "/usr/local/bin/fleet-worker".to_string()
    } else {
        req.fleet_worker_bin.clone()
    };

    // StepContext 구성.
    let mut labels = HashMap::new();
    for (k, v) in &req.labels {
        labels.insert(k.clone(), v.clone());
    }

    let step_ctx = StepContext {
        worker_name: req.worker_name.clone(),
        labels,
        orchestrator_url: req.orchestrator_url.clone(),
        cf_token: None,
        fleet_worker_bin: Some(fleet_worker_bin),
        // 대시보드에서 트리거하는 프로비저닝은 아직 아키텍처별 바이너리
        // 소스를 입력받지 않는다 — 단일 경로(`fleet_worker_bin`)로만 동작한다
        // (메커니즘 자체는 로드맵 `#81`이 만들었다). `#83`은 YAML 인벤토리
        // 모드(`fleet provision --inventory`)만 배선했다 — `ProvisionRequest`는
        // 인벤토리가 아닌 단건 JSON 요청이라 범위 밖이다. 이 호출부에 맵을
        // 채우는 배선은 아직 별도 항목이 없다.
        fleet_worker_bin_by_arch: std::collections::HashMap::new(),
        grok_bind_addr: None,
        grok_secret: Some(grok_secret.clone()),
        max_concurrent_tasks: req.max_concurrent_tasks,
        orchestrator_api_token: req.api_token.clone(),
        dry_run: req.dry_run,
        mtls_enabled: false,
        mtls_listen_addr: None,
        mtls_server_cert_path: None,
        mtls_server_key_path: None,
        mtls_client_ca_path: None,
        mtls_advertised_host: None,
        mtls_advertised_port: None,
    };

    // dry_run: MockExecutor 사용.
    if req.dry_run {
        return Ok(Json(serde_json::json!({
            "worker_name": &req.worker_name,
            "dry_run": true,
            "message": "Dry run — no changes applied."
        })));
    }

    // SSH 연결.
    let connect_info = SshConnectInfo::new(
        req.host.clone(),
        req.ssh_user.clone(),
        std::path::PathBuf::from(key_path),
    )
    .with_port(req.ssh_port);

    let host_key_config = HostKeyConfig::new(fleet_provisioner::ssh::HostKeyPolicy::Tofu);

    let ssh = SshClient::connect(connect_info, host_key_config)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, host = %req.host, "SSH connect failed");
            ApiError::Unavailable(format!("SSH connection failed: {e}"))
        })?;

    // Playbook 실행. 로드맵 #81 — 이전에는 os/arch를 항상 ubuntu/x86_64로
    // 가정했다. 이 시점에는 이미 실 SSH 연결이 있으므로(dry_run은 위에서
    // 먼저 반환) check_prereqs를 실제로 실행해 얻은 값을 쓴다.
    let prereq = detect_prereq(&ssh).await.map_err(|e| {
        tracing::error!(error = %e, host = %req.host, "check_prereqs failed");
        ApiError::Unavailable(format!("host prerequisite check failed: {e}"))
    })?;
    let playbook = Playbook::standard(&prereq);
    let pb_ctx = PlaybookContext {
        base: step_ctx,
        only_tags: if req.tags.is_empty() {
            None
        } else {
            Some(req.tags.clone())
        },
    };

    let report = playbook.run(&ssh, &pb_ctx).await;

    // 결과 처리.
    match report {
        Ok(report) => {
            let succeeded = report.succeeded;

            // Host upsert + 이벤트 기록.
            let host = fleet_core::Host {
                id: Uuid::new_v4(),
                hostname: req.worker_name.clone(),
                worker_id: None,
                status: if succeeded {
                    HostStatus::Provisioned
                } else {
                    HostStatus::Failed
                },
                ssh_host: Some(req.host.clone()),
                ssh_port: req.ssh_port as i32,
                ssh_user: Some(req.ssh_user.clone()),
                grok_version: None,
                fleet_worker_version: None,
                os_info: None,
                metrics: Default::default(),
                last_heartbeat_at: None,
                provisioned_at: if succeeded {
                    Some(chrono::Utc::now())
                } else {
                    None
                },
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            let _ = state.store.upsert_host(&host).await;

            // 이벤트 기록.
            let event_type = if succeeded {
                "provision_ok"
            } else {
                "provision_fail"
            };
            let event = fleet_core::HostEvent {
                id: Uuid::new_v4(),
                host_id: host.id,
                event_type: event_type.into(),
                severity: if succeeded {
                    fleet_core::EventSeverity::Info
                } else {
                    fleet_core::EventSeverity::Error
                },
                message: Some(format!(
                    "Provisioning by {} — {} steps",
                    principal.user.username,
                    report.steps.len()
                )),
                payload: HashMap::new(),
                created_at: chrono::Utc::now(),
            };
            let _ = state.store.append_host_event(&event).await;

            // 응답.
            let steps_json: Vec<serde_json::Value> = report
                .steps
                .iter()
                .map(|s| {
                    let status_str = match &s.status {
                        StepStatus::Skipped => "skipped",
                        StepStatus::Applied { .. } => "applied",
                        StepStatus::Failed { .. } => "failed",
                    };
                    serde_json::json!({
                        "name": s.name,
                        "status": status_str,
                    })
                })
                .collect();

            tracing::info!(
                host = %req.host,
                worker_name = %req.worker_name,
                succeeded,
                username = %principal.user.username,
                "provisioning completed"
            );

            Ok(Json(serde_json::json!({
                "worker_name": &req.worker_name,
                "host": &req.host,
                "succeeded": succeeded,
                "steps": steps_json,
            })))
        }
        Err(e) => {
            tracing::error!(error = %e, host = %req.host, "provisioning failed");
            Err(ApiError::Internal(format!("Provisioning failed: {e}")))
        }
    }
}

// ── 헬퍼 ────────────────────────────────────────────────────────────────

/// PEM 헤더로 키 타입 감지.
fn detect_key_type(private_key: &str) -> String {
    if private_key.contains("BEGIN OPENSSH PRIVATE KEY") {
        // OpenSSH 포맷 — ed25519가 일반적이지만 RSA일 수도 있음.
        // 간단히 "openssh"로 분류.
        "openssh".into()
    } else if private_key.contains("BEGIN RSA PRIVATE KEY") {
        "rsa".into()
    } else if private_key.contains("BEGIN EC PRIVATE KEY") {
        "ecdsa".into()
    } else {
        "unknown".into()
    }
}

/// 비밀키에서 fingerprint 계산.
/// 실제 공개키 추출은 복잡하므로, 비밀키 내용의 SHA-256 해시를 사용.
/// (표시용이므로 보안적으로 민감하지 않음.)
fn compute_fingerprint(private_key: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(private_key.as_bytes());
    // SHA-256 앞 16바이트를 hex로 (32 chars) — 충분한 식별력.
    hex::encode(&hash[..16])
}

// ── 요청 구조체 ──────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct CreateSshKeyRequest {
    pub name: String,
    pub private_key: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct ProvisionRequest {
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    pub ssh_user: String,
    pub ssh_key_name: String,
    pub worker_name: String,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    pub orchestrator_url: String,
    #[serde(default)]
    pub grok_secret: String,
    /// 오케스트레이터 관리 API 용 bearer 토큰. JoinWorker가 `/v1/bootstrap-tokens`
    /// 발급에 사용한다(로드맵 `#82`) — `token:issue` capability 필요.
    #[serde(default)]
    pub api_token: Option<String>,
    #[serde(default)]
    pub fleet_worker_bin: String,
    #[serde(default)]
    pub max_concurrent_tasks: Option<u32>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_ssh_port() -> u16 {
    22
}

/// GET /admin/ssh-keys — SSH 키 관리 HTML 페이지.
///
/// `/api/ssh-keys`와 동일하게 `host:provision` 권한 필요.
pub async fn admin_ssh_keys_page(
    axum::extract::Extension(principal): axum::extract::Extension<AuthPrincipal>,
) -> axum::response::Response {
    super::handlers::serve_page_if_permitted(
        &principal,
        fleet_core::PermissionKind::HostProvision,
        "admin-ssh-keys.html",
    )
}

/// GET /hosts/provision — 프로비저닝 폼 HTML 페이지.
///
/// `/api/hosts/provision`과 동일하게 `host:provision` 권한 필요.
pub async fn provision_page(
    axum::extract::Extension(principal): axum::extract::Extension<AuthPrincipal>,
) -> axum::response::Response {
    super::handlers::serve_page_if_permitted(
        &principal,
        fleet_core::PermissionKind::HostProvision,
        "provision.html",
    )
}
