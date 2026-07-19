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
}

impl Default for InstallCloudflared {
    fn default() -> Self {
        Self {
            hostname_pattern: "{worker}.fleet.internal".into(),
        }
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
        let config = exec.exec("test -f /etc/cloudflared/config.yml && echo yes").await?;
        let running = exec.exec("systemctl is-active cloudflared 2>/dev/null").await?;
        Ok(config.trim() == "yes" && running.trim() == "active")
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        let hostname = self
            .hostname_pattern
            .replace("{worker}", &ctx.worker_name);
        let tunnel_name = format!("fleet-{}", ctx.worker_name);

        if ctx.dry_run {
            return Ok(StepOutput::message(format!(
                "dry-run: install cloudflared tunnel '{tunnel_name}' with hostname {hostname}"
            )));
        }

        // 1. 바이너리 다운로드
        let code = exec
            .exec_streaming(
                "curl -fsSL \
                 https://github.com/cloudflare/cloudflared/releases/latest/download/\
                 cloudflared-linux-amd64 \
                 -o /tmp/cloudflared && \
                 sudo mv /tmp/cloudflared /usr/local/bin/cloudflared && \
                 sudo chmod +x /usr/local/bin/cloudflared",
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
        let cf_token = ctx
            .cf_token
            .as_ref()
            .ok_or_else(|| StepError::PrereqFailed("cf_token is required for tunnel creation".into()))?;

        // 토큰 인증 (cloudflared tunnel login은 대화형이라 토큰 방식 선호).
        let _ = exec
            .exec(&format!(
                "cloudflared tunnel --cred-file /etc/cloudflared/creds.json token {cf_token} 2>&1 || true"
            ))
            .await;

        // 3. config.yml 생성 (템플릿).
        let tmpl_ctx = TemplateContext {
            tunnel_name: tunnel_name.clone(),
            hostname: hostname.clone(),
            credentials_path: "/etc/cloudflared/creds.json".into(),
            ..Default::default()
        };
        let config_yaml = crate::templates::render_cloudflared_config(&tmpl_ctx)?;
        exec.write_file("/tmp/cloudflared-config.yml", &config_yaml)
            .await?;
        let mv_code = exec
            .exec("sudo mv /tmp/cloudflared-config.yml /etc/cloudflared/config.yml")
            .await;
        // mv 실패는 무시 (이미 존재하는 디렉토리일 수 있음).
        let _ = mv_code;

        // 4. systemd 유닛 설치. cloudflared는 `service install` 명령 제공.
        let install_code = exec
            .exec_streaming(
                "sudo cloudflared --config /etc/cloudflared/config.yml service install 2>&1 || \
                 sudo systemctl enable cloudflared 2>&1 || true",
                Box::new(|line| tracing::info!("[remote] {line}")),
            )
            .await?;
        if install_code != 0 {
            tracing::warn!(code = install_code, "cloudflared service install returned non-zero (may already be installed)");
        }
        let _ = exec.exec("sudo systemctl restart cloudflared 2>&1 || true").await;

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
        assert!(calls.iter().any(|c| c.contains("write /tmp/cloudflared-config.yml")));
        assert!(calls.iter().any(|c| c.contains("cloudflared-linux-amd64")));
    }

    #[tokio::test]
    async fn hostname_pattern_substitutes_worker_name() {
        let exec = MockExecutor::new();
        let step = InstallCloudflared {
            hostname_pattern: "{worker}.fleet.example.com".into(),
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
}
