//! 프로비저닝 스텝 정의. 각 스텝은 `Step` 트레이트를 구현하고
//! `is_applied()`로 멱등성을 검사합니다.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::StepError;
use crate::ssh::RemoteExecutor;

pub mod check_prereqs;
pub mod install_cloudflared;
pub mod install_deps;
pub mod install_fleet_worker;
pub mod install_grok;
pub mod start_services;

// 하위 모듈의 공개 타입을 steps:: 직접 노출.
pub use check_prereqs::CheckPrereqs;
pub use install_cloudflared::InstallCloudflared;
pub use install_deps::InstallDeps;
pub use install_fleet_worker::InstallFleetWorker;
pub use install_grok::InstallGrok;
pub use start_services::StartServices;

// ── 공통 타입 ──────────────────────────────────────────────────────────

/// 사전 검증 결과. 후속 스텝이 이 값을 참조.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrereqReport {
    pub os: String,
    pub arch: String,
    pub mem_mb: u64,
    pub disk_gb: u64,
    pub has_rust: bool,
    pub has_systemd: bool,
}

/// Cloudflare 터널 정보 (install_cloudflared 스텝 결과).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelInfo {
    pub tunnel_name: String,
    pub hostname: String,
    pub credentials_path: String,
}

/// 스텝 실행 결과. payload는 스텝 종류에 따라 다름 (예: PrereqReport).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepOutput {
    pub message: String,
    /// 직렬화된 페이로드 (PrereqReport, TunnelInfo 등).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl StepOutput {
    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            payload: None,
        }
    }

    pub fn with_payload<T: Serialize>(msg: impl Into<String>, payload: &T) -> Self {
        Self {
            message: msg.into(),
            payload: serde_json::to_value(payload).ok(),
        }
    }
}

/// 멱등성 보장 프로비저닝 스텝.
///
/// 스텝은 다음을 보장해야 함:
/// 1. `is_applied()`가 `true`면 `apply()`는 아무 부작용 없이 즉시 반환.
/// 2. `apply()`는 실패 시 재시도해도 안전 (부분 상태 허용).
/// 3. `tags()`로 특정 스텝만 실행 (`--tags tunnel` 등).
#[async_trait]
pub trait Step: Send + Sync {
    /// 스텝 이름 (로깅/진단용).
    fn name(&self) -> &'static str;

    /// 태그 목록. `--tags` 옵션으로 필터링.
    fn tags(&self) -> &'static [&'static str] {
        &[]
    }

    /// 이미 적용되었는지 검사 (멱등성).
    async fn is_applied(&self, exec: &dyn RemoteExecutor) -> Result<bool, StepError>;

    /// 스텝 실행. `is_applied == false`일 때만 호출됨.
    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError>;
}

/// 스텝에 전달되는 읽기 전용 컨텍스트. 전체 Playbook 진행 상황 공유.
#[derive(Debug, Clone, Default)]
pub struct StepContext {
    /// `--name`으로 지정된 워커 이름.
    pub worker_name: String,
    /// `--labels` (key=value 리스트).
    pub labels: std::collections::HashMap<String, String>,
    /// 오케스트레이터 URL (예: `https://orch.fleet.example.com`).
    pub orchestrator_url: String,
    /// Cloudflare 인증 토큰 (오리진 CA 또는 API 토큰).
    pub cf_token: Option<String>,
    /// fleet-worker 바이너리 로컬 경로 (cargo build 결과).
    pub fleet_worker_bin: Option<String>,
    /// grok 서브프로세스가 listen할 로컬 호스트:포트.
    /// 미설정 시 템플릿 기본값 `127.0.0.1:2419` 사용.
    pub grok_bind_addr: Option<String>,
    /// grok 서버 키 시크릿. worker.toml `[grok] secret` 필드에 필수.
    /// 프로비저닝 호출자가 난수로 생성해서 전달.
    pub grok_secret: Option<String>,
    /// 동시 작업 수 (worker.toml `[grok] max_concurrent_tasks`).
    /// 미설정 시 템플릿 기본값 4 사용.
    pub max_concurrent_tasks: Option<u32>,
    /// 오케스트레이터 등록용 bootstrap bearer 토큰. None이면 worker.toml에서 생략.
    pub bootstrap_token: Option<String>,
    /// Dry-run 모드: 실제 변경 없이 무엇을 할지 로깅만.
    pub dry_run: bool,
}

impl StepContext {
    pub fn for_worker(worker_name: impl Into<String>) -> Self {
        Self {
            worker_name: worker_name.into(),
            ..Default::default()
        }
    }
}
