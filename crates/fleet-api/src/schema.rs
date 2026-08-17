//! HTTP 요청/응답 바디 스키마.
//!
//! 클라이언트가 전송하는 JSON 형태와 오케스트레이터가 반환하는 형태를 정의.
//! 도메인 타입(`Worker`, `WorkerHeartbeat`)과는 별개로 API 표면을 관리하여
//! 도메인 변경이 API 계약에 미치는 영향을 격리.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use fleet_core::{BootstrapToken, WorkerStatus};

/// `POST /v1/workers/register` 요청 바디.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegisterRequest {
    /// 워커 고유 이름 (DNS-safe). 충돌 시 거부됨.
    pub name: String,
    /// 워커의 agent endpoint (예: `wss://10.0.1.10:2419/ws`).
    pub agent_endpoint: String,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_tasks: u32,
    /// 사이드카 버전 (예: `0.1.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_version: Option<String>,
    /// 재연결 시 기존 worker_id (없으면 신규 등록).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_worker_id: Option<String>,
}

fn default_max_concurrent() -> u32 {
    4
}

/// `POST /v1/workers/register` 응답.
#[derive(Debug, Clone, Serialize)]
pub struct RegisterResponse {
    pub worker_id: String,
    pub heartbeat_interval_secs: u32,
    pub config_revision: u32,
    pub orchestrator_version: &'static str,
    pub status: &'static str,
}

/// `POST /v1/workers/heartbeat` 요청 바디.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeartbeatRequest {
    pub worker_id: String,
    #[serde(default)]
    pub active_tasks: u32,
    /// Unix load average (1, 5, 15분).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub load_avg: Vec<f32>,
    #[serde(default)]
    pub mem_available_mb: u64,
    #[serde(default)]
    pub disk_free_mb: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_usage: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ram_usage: Option<f32>,
    #[serde(default = "default_true")]
    pub agent_healthy: bool,
    /// grok CLI 버전.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grok_version: Option<String>,
    /// fleet-worker 버전.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_worker_version: Option<String>,
    /// OS 정보.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_info: Option<ApiOsInfo>,
}

/// heartbeat 요청용 OS 정보.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiOsInfo {
    #[serde(default)]
    pub os_type: String,
    #[serde(default)]
    pub distro: String,
    #[serde(default)]
    pub kernel: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub hostname: String,
}

fn default_true() -> bool {
    true
}

/// `POST /v1/hosts/register` 요청 — 프로비저닝 완료 후 CLI가 호출.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HostRegisterRequest {
    pub hostname: String,
    pub ssh_host: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: i32,
    pub ssh_user: String,
    pub succeeded: bool,
    #[serde(default)]
    pub message: Option<String>,
}

fn default_ssh_port() -> i32 {
    22
}

/// `POST /v1/hosts/register` 응답.
#[derive(Debug, Clone, Serialize)]
pub struct HostRegisterResponse {
    pub ok: bool,
    pub host_id: String,
}

/// `POST /v1/workers/heartbeat` 응답.
#[derive(Debug, Clone, Serialize)]
pub struct HeartbeatResponse {
    pub ok: bool,
    /// 오케스트레이터가 워커에게 지시하는 상태 (예: "shutdown" 시그널).
    /// Phase 3에서는 항상 "running".
    pub desired_state: &'static str,
    pub server_time: DateTime<Utc>,
}

/// `GET /v1/workers` / `GET /v1/workers/:id` 응답에 담기는 워커 요약.
#[derive(Debug, Clone, Serialize)]
pub struct WorkerSummary {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub status: String,
    pub labels: HashMap<String, String>,
    pub active_tasks: u32,
    pub max_concurrent: u32,
    pub circuit_state: String,
    pub last_seen: Option<DateTime<Utc>>,
    pub registered_at: DateTime<Utc>,
}

impl WorkerSummary {
    pub fn status_str(s: WorkerStatus) -> &'static str {
        match s {
            WorkerStatus::Online => "online",
            WorkerStatus::Degraded => "degraded",
            WorkerStatus::Draining => "draining",
            WorkerStatus::Offline => "offline",
            WorkerStatus::CircuitOpen => "circuit_open",
        }
    }
}

/// `GET /v1/health` 응답.
#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

/// `DELETE /v1/workers/:id` 요청 바디 (옵션).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DeregisterRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ── Phase 8.3: Bootstrap tokens + worker join flow ─────────────────────

/// `POST /v1/workers/join` 요청 바디. 신규 워커가 부트스트랩 토큰으로
/// 자신을 등록할 때 사용.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JoinRequest {
    /// 어드민이 발급한 부트스트랩 토큰.
    pub token: String,
    /// 워커 이름 (DNS-safe).
    pub name: String,
    /// 워커의 agent endpoint (예: `ws://worker:2419/ws?server-key=...`).
    pub agent_endpoint: String,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_tasks: u32,
    /// 사이드카 버전.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_version: Option<String>,
}

/// `POST /v1/workers/join` 응답. `RegisterResponse`에 더해 클라이언트가
/// 디스크에 기록할 worker.toml 내용을 포함.
#[derive(Debug, Clone, Serialize)]
pub struct JoinResponse {
    pub worker_id: String,
    pub heartbeat_interval_secs: u32,
    pub config_revision: u32,
    pub orchestrator_version: &'static str,
    pub status: &'static str,
    /// 워커가 저장할 worker.toml 전체 내용.
    pub worker_config_toml: String,
}

/// `POST /v1/bootstrap-tokens` 요청 바디.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateBootstrapTokenRequest {
    /// 토큰 접두어 (예: "fleet"). 생성 시 `<prefix>_<random>` 형태.
    #[serde(default = "default_token_prefix")]
    pub prefix: String,
    /// 무작위 바이트 길이.
    #[serde(default = "default_token_bytes")]
    pub bytes: usize,
    /// 최대 사용 횟수. 기본 1 (일회성).
    #[serde(default = "default_max_uses")]
    pub max_uses: u32,
    /// 만료까지 초. None이면 무기한.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in_secs: Option<u64>,
    /// 어드민 메모.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// 발급자 식별자 (자동 추출이 어려운 경우 CLI에서 전달).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
}

fn default_token_prefix() -> String {
    "fleet".to_string()
}
fn default_token_bytes() -> usize {
    32
}
fn default_max_uses() -> u32 {
    1
}

/// `POST /v1/bootstrap-tokens` 응답.
#[derive(Debug, Clone, Serialize)]
pub struct CreateBootstrapTokenResponse {
    /// 발급 시에만 반환되는 원문. 이후 목록·회수 API에는 포함되지 않는다.
    pub token: String,
    /// 목록·회수에 사용할 공개 식별자.
    pub token_id: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_uses: u32,
}

/// `GET /v1/bootstrap-tokens` 응답의 개별 항목.
#[derive(Debug, Clone, Serialize)]
pub struct BootstrapTokenSummary {
    /// 원문 대신 노출하는 SHA-256 기반 공개 식별자.
    pub token_id: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_uses: u32,
    pub use_count: u32,
    pub remaining_uses: u32,
    pub notes: Option<String>,
    pub last_used_by: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl From<BootstrapToken> for BootstrapTokenSummary {
    fn from(t: BootstrapToken) -> Self {
        let remaining = t.remaining_uses();
        BootstrapTokenSummary {
            token_id: t.public_id(),
            created_at: t.created_at,
            expires_at: t.expires_at,
            max_uses: t.max_uses,
            use_count: t.use_count,
            remaining_uses: remaining,
            notes: t.notes,
            last_used_by: t.last_used_by,
            last_used_at: t.last_used_at,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Phase 8.6: Worker credentials
// ═══════════════════════════════════════════════════════════════════════

/// `PUT /v1/workers/:name/credentials` 요청 바디.
///
/// 한 번에 하나의 (model_id, api_key) 조합을 설정.
/// `api_key`는 평문으로 전송되며, 서버에서 암호화하여 저장.
#[derive(Debug, Clone, Deserialize)]
pub struct PutCredentialRequest {
    /// grok config의 `[model.<id>]` 키 (예: `grok-build`).
    pub model_id: String,
    /// API 엔드포인트 base URL.
    pub base_url: String,
    /// 평문 API 키. 암호화되어 DB에 저장됨.
    pub api_key: String,
    /// `chat_completions` 또는 `responses`. 기본 `chat_completions`.
    #[serde(default = "default_credential_api_backend")]
    pub api_backend: String,
    /// 컨텍스트 윈도우. 기본 200000.
    #[serde(default = "default_credential_context_window")]
    pub context_window: u32,
    /// 모델 이름 (예: `GLM-5.1`). 선택적.
    #[serde(default)]
    pub model_name: Option<String>,
}

fn default_credential_api_backend() -> String {
    "chat_completions".to_string()
}

fn default_credential_context_window() -> u32 {
    200_000
}

/// `PUT /v1/workers/:name/credentials` 응답.
#[derive(Debug, Clone, Serialize)]
pub struct PutCredentialResponse {
    pub status: &'static str,
    pub worker_name: String,
    pub model_id: String,
    pub rotated_at: DateTime<Utc>,
}

/// `GET /v1/workers/:name/credentials` 응답의 개별 항목.
///
/// **api_key는 절대 반환하지 않음** — 복호화는 worker 측에서만.
/// 이 API는 메타데이터(어떤 모델이 설정되었는지, 회전 시간 등)만 반환.
#[derive(Debug, Clone, Serialize)]
pub struct CredentialSummary {
    pub worker_name: String,
    pub model_id: String,
    pub base_url: String,
    pub api_backend: String,
    pub context_window: u32,
    pub model_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub rotated_at: DateTime<Utc>,
}

impl From<fleet_store::StoredCredential> for CredentialSummary {
    fn from(c: fleet_store::StoredCredential) -> Self {
        Self {
            worker_name: c.worker_name,
            model_id: c.model_id,
            base_url: c.base_url,
            api_backend: c.api_backend,
            context_window: c.context_window,
            model_name: c.model_name,
            created_at: c.created_at,
            rotated_at: c.rotated_at,
        }
    }
}

/// `GET /v1/workers/:name/credentials/:model_id/export` 응답.
///
/// 복호화된 API 키 + 메타데이터를 반환. 이 엔드포인트는 프로비저닝
/// 시에만 사용되며, bearer 토큰 인증을 필수로 받음.
#[derive(Debug, Clone, Serialize)]
pub struct ExportedCredential {
    pub worker_name: String,
    pub model_id: String,
    pub base_url: String,
    pub api_key: String,
    pub api_backend: String,
    pub context_window: u32,
    pub model_name: Option<String>,
    pub rotated_at: DateTime<Utc>,
    /// 워커의 `~/.grok/config.toml`에 추가할 TOML 섹션 (렌더링된 문자열).
    pub grok_config_section: String,
}
