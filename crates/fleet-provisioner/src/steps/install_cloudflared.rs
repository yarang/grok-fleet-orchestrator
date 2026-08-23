//! Step 4: cloudflared 설치 및 터널 생성.
//!
//! 1. cloudflared 바이너리 다운로드
//! 2. 터널 자격증명 생성
//! 3. config.yml 작성
//! 4. DNS 라우팅
//! 5. systemd 유닛 활성화

use async_trait::async_trait;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{Step, StepContext, StepOutput, TunnelInfo};
use crate::templates::TemplateContext;

pub struct InstallCloudflared {
    /// 터널 DNS 호스트명 패턴. `{worker}` 자리표시자 포함 가능.
    /// 예: `"{worker}.fleet.example.com"`.
    pub hostname_pattern: String,
    /// 대상 호스트의 아키텍처(`PrereqReport.arch`). `Playbook::standard`가
    /// `check_prereqs` 결과로 채운다 (로드맵 `#81`). cloudflared 릴리스
    /// 바이너리의 arch suffix(`amd64`/`arm64`) 선택에 쓰인다.
    pub target_arch: Option<String>,
}

impl Default for InstallCloudflared {
    fn default() -> Self {
        Self {
            hostname_pattern: "{worker}.fleet.internal".into(),
            target_arch: None,
        }
    }
}

/// `PrereqReport.arch`(`uname -m` 값)를 cloudflared 릴리스 자산의 arch
/// suffix로 변환한다. cloudflared는 Linux 자산을 `amd64`/`arm64`로 이름
/// 붙이지만 `uname -m`은 `x86_64`/`aarch64`를 반환하므로 그대로 쓸 수 없다.
///
/// 알 수 없는 값은 `amd64`로 폴백하고 경고를 남긴다 — 이 스텝은 표준
/// playbook에서 곧 제거될 예정이라(mTLS 직접 다이얼 채택, 로드맵 `#85`)
/// 여기서 하드 실패로 무인 프로비저닝 전체를 막을 정도의 투자를 하지
/// 않는다. 다만 잘못된 선택이 조용히 일어나지는 않게 한다.
fn cloudflared_arch_suffix(arch: Option<&str>) -> &'static str {
    match arch {
        Some("aarch64") | Some("arm64") => "arm64",
        Some("x86_64") | Some("amd64") => "amd64",
        Some(other) => {
            tracing::warn!(
                arch = other,
                "unrecognized architecture for cloudflared release asset — defaulting to amd64"
            );
            "amd64"
        }
        None => "amd64",
    }
}

#[async_trait]
impl Step for InstallCloudflared {
    fn name(&self) -> &'static str {
        "install_cloudflared"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["tunnel", "cloudflared", "setup"]
    }

    async fn is_applied(&self, exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        // /etc/cloudflared/config.yml이 있고 cloudflared가 동작 중이면 적용됨.
        let config = exec
            .exec("test -f /etc/cloudflared/config.yml && echo yes")
            .await?;
        let running = exec
            .exec("systemctl is-active cloudflared 2>/dev/null")
            .await?;
        Ok(config.trim() == "yes" && running.trim() == "active")
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        let hostname = self.hostname_pattern.replace("{worker}", &ctx.worker_name);
        let tunnel_name = format!("fleet-{}", ctx.worker_name);

        if ctx.dry_run {
            return Ok(StepOutput::message(format!(
                "dry-run: install cloudflared tunnel '{tunnel_name}' with hostname {hostname}"
            )));
        }

        // 1. 바이너리 다운로드 (로드맵 #81 — 아키텍처별 자산 선택).
        let arch_suffix = cloudflared_arch_suffix(self.target_arch.as_deref());
        let code = exec
            .exec_streaming(
                &format!(
                    "curl -fsSL \
                     https://github.com/cloudflare/cloudflared/releases/latest/download/\
                     cloudflared-linux-{arch_suffix} \
                     -o /tmp/cloudflared && \
                     sudo mv /tmp/cloudflared /usr/local/bin/cloudflared && \
                     sudo chmod +x /usr/local/bin/cloudflared"
                ),
                Box::new(|line| tracing::info!("[remote] {line}")),
            )
            .await?;
        if code != 0 {
            return Err(StepError::RemoteExit {
                code,
                stderr: "cloudflared download failed".into(),
            });
        }

        // 2. 터널 자격증명 생성 (cf_token 필요).
        let cf_token = ctx.cf_token.as_ref().ok_or_else(|| {
            StepError::PrereqFailed("cf_token is required for tunnel creation".into())
        })?;

        // 자격증명 디렉토리 준비 (로드맵 #79 — 이전에는 이 mkdir이 없어서
        // 아래 config.yml `mv`가 새 호스트에서 "디렉토리 없음"으로 조용히
        // 실패하고 있었다. 실패해도 config.yml이 /tmp에만 남고 스텝은
        // "Applied"로 보고됐다).
        exec.exec_checked("sudo mkdir -p /etc/cloudflared").await?;

        // 토큰 인증 (cloudflared tunnel login은 대화형이라 토큰 방식 선호).
        // 자격증명 생성은 이후 모든 단계의 전제이므로 실패를 삼키지 않는다.
        exec.exec_checked(&format!(
            "cloudflared tunnel --cred-file /etc/cloudflared/creds.json token {cf_token}"
        ))
        .await?;

        // 3. config.yml 생성 (템플릿).
        let tmpl_ctx = TemplateContext {
            tunnel_name: tunnel_name.clone(),
            hostname: hostname.clone(),
            credentials_path: "/etc/cloudflared/creds.json".into(),
        };
        let config_yaml = crate::templates::render_cloudflared_config(&tmpl_ctx)?;
        exec.write_file("/tmp/cloudflared-config.yml", &config_yaml)
            .await?;
        exec.exec_checked("sudo mv /tmp/cloudflared-config.yml /etc/cloudflared/config.yml")
            .await?;

        // 4. systemd 유닛 설치. cloudflared는 `service install` 명령 제공.
        let install_code = exec
            .exec_streaming(
                "sudo cloudflared --config /etc/cloudflared/config.yml service install 2>&1 || \
                 sudo systemctl enable cloudflared 2>&1 || true",
                Box::new(|line| tracing::info!("[remote] {line}")),
            )
            .await?;
        if install_code != 0 {
            tracing::warn!(
                code = install_code,
                "cloudflared service install returned non-zero (may already be installed)"
            );
        }

        // 재시작은 best-effort로 남긴다 — service install 방식에 따라 이미
        // active 상태일 수 있다. 다만 실패는 더 이상 조용히 삼키지 않고
        // 로그로 관측 가능하게 한다 (로드맵 #79).
        if let Err(e) = exec.exec_checked("sudo systemctl restart cloudflared").await {
            tracing::warn!(error = %e, "cloudflared restart failed (best-effort, continuing)");
        }

        let info = TunnelInfo {
            tunnel_name,
            hostname,
            credentials_path: "/etc/cloudflared/creds.json".into(),
        };
        Ok(StepOutput::with_payload(
            format!("cloudflared tunnel '{}' created", info.tunnel_name),
            &info,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;

    #[tokio::test]
    async fn is_applied_when_config_and_service_active() {
        let exec = MockExecutor::new();
        exec.expect_exec("test -f /etc/cloudflared/config.yml", "yes\n");
        exec.expect_exec("systemctl is-active cloudflared", "active\n");
        let step = InstallCloudflared::default();
        assert!(step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn is_not_applied_when_service_inactive() {
        let exec = MockExecutor::new();
        exec.expect_exec("test -f /etc/cloudflared/config.yml", "yes\n");
        exec.expect_exec("systemctl is-active cloudflared", "inactive\n");
        let step = InstallCloudflared::default();
        assert!(!step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn apply_requires_cf_token() {
        let exec = MockExecutor::new();
        let step = InstallCloudflared::default();
        let ctx = StepContext {
            cf_token: None,
            ..Default::default()
        };
        let result = step.apply(&exec, &ctx).await;
        assert!(matches!(result, Err(StepError::PrereqFailed(_))));
        let err = result.unwrap_err();
        assert!(format!("{err}").contains("cf_token"));
    }

    #[tokio::test]
    async fn apply_writes_config_file() {
        let exec = MockExecutor::new();
        let step = InstallCloudflared::default();
        let ctx = StepContext {
            worker_name: "build-1".into(),
            cf_token: Some("tok-abc".into()),
            ..Default::default()
        };
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("fleet-build-1"));
        let calls = exec.recorded_calls();
        assert!(calls
            .iter()
            .any(|c| c.contains("write /tmp/cloudflared-config.yml")));
        assert!(calls.iter().any(|c| c.contains("cloudflared-linux-amd64")));
    }

    #[tokio::test]
    async fn hostname_pattern_substitutes_worker_name() {
        let exec = MockExecutor::new();
        let step = InstallCloudflared {
            hostname_pattern: "{worker}.fleet.example.com".into(),
            ..InstallCloudflared::default()
        };
        let ctx = StepContext {
            worker_name: "gpu-runner-1".into(),
            cf_token: Some("t".into()),
            ..Default::default()
        };
        let out = step.apply(&exec, &ctx).await.unwrap();
        let info: TunnelInfo = serde_json::from_value(out.payload.unwrap()).unwrap();
        assert_eq!(info.hostname, "gpu-runner-1.fleet.example.com");
    }

    #[tokio::test]
    async fn dry_run_skips_network_calls() {
        let exec = MockExecutor::new();
        let step = InstallCloudflared::default();
        let ctx = StepContext {
            worker_name: "w".into(),
            cf_token: Some("t".into()),
            dry_run: true,
            ..Default::default()
        };
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("dry-run"));
        assert!(exec.recorded_calls().is_empty());
    }

    // ── 아키텍처별 cloudflared 자산 선택 (로드맵 #81) ─────────────────────

    #[test]
    fn cloudflared_arch_suffix_maps_uname_values() {
        assert_eq!(cloudflared_arch_suffix(Some("aarch64")), "arm64");
        assert_eq!(cloudflared_arch_suffix(Some("arm64")), "arm64");
        assert_eq!(cloudflared_arch_suffix(Some("x86_64")), "amd64");
        assert_eq!(cloudflared_arch_suffix(Some("amd64")), "amd64");
    }

    #[test]
    fn cloudflared_arch_suffix_defaults_to_amd64_for_unknown_or_missing() {
        assert_eq!(cloudflared_arch_suffix(Some("riscv64")), "amd64");
        assert_eq!(cloudflared_arch_suffix(None), "amd64");
    }

    #[tokio::test]
    async fn apply_downloads_arm64_binary_for_aarch64_target() {
        let exec = MockExecutor::new();
        let step = InstallCloudflared {
            target_arch: Some("aarch64".into()),
            ..InstallCloudflared::default()
        };
        let ctx = StepContext {
            worker_name: "arm-worker".into(),
            cf_token: Some("tok".into()),
            ..Default::default()
        };
        step.apply(&exec, &ctx).await.unwrap();
        let calls = exec.recorded_calls();
        assert!(
            calls.iter().any(|c| c.contains("cloudflared-linux-arm64")),
            "expected arm64 asset download, got: {calls:?}"
        );
        assert!(!calls.iter().any(|c| c.contains("cloudflared-linux-amd64")));
    }

    #[tokio::test]
    async fn apply_downloads_amd64_binary_by_default() {
        let exec = MockExecutor::new();
        let step = InstallCloudflared::default(); // target_arch: None
        let ctx = StepContext {
            worker_name: "x86-worker".into(),
            cf_token: Some("tok".into()),
            ..Default::default()
        };
        step.apply(&exec, &ctx).await.unwrap();
        let calls = exec.recorded_calls();
        assert!(calls.iter().any(|c| c.contains("cloudflared-linux-amd64")));
    }
}
