//! Step 5: fleet-worker 바이너리 배포 + 설정 파일 작성 + systemd 유닛 설치.

use async_trait::async_trait;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{Step, StepContext, StepOutput};
use crate::templates::TemplateContext;

/// fleet-worker 바이너리 배포 + 설정 파일 작성 + systemd 유닛 설치 스텝.
#[derive(Default)]
pub struct InstallFleetWorker {
    /// 로컬에 빌드된 fleet-worker 바이너리 경로.
    /// None인 경우 ctx.fleet_worker_bin 사용.
    pub local_bin: Option<String>,
}

#[async_trait]
impl Step for InstallFleetWorker {
    fn name(&self) -> &'static str {
        "install_fleet_worker"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["worker", "fleet-worker", "setup"]
    }

    async fn is_applied(&self, exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        let bin = exec.exec("test -x /usr/local/bin/fleet-worker && echo yes").await?;
        let unit = exec
            .exec("test -f /etc/systemd/system/fleet-worker.service && echo yes")
            .await?;
        Ok(bin.trim() == "yes" && unit.trim() == "yes")
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        if ctx.dry_run {
            let bin = self
                .local_bin
                .as_deref()
                .or(ctx.fleet_worker_bin.as_deref())
                .unwrap_or("(unspecified)");
            return Ok(StepOutput::message(format!(
                "dry-run: deploy {bin} → /usr/local/bin/fleet-worker"
            )));
        }

        let local_bin = self
            .local_bin
            .as_deref()
            .or(ctx.fleet_worker_bin.as_deref())
            .ok_or_else(|| {
                StepError::PrereqFailed(
                    "fleet_worker_bin path not provided (set ctx.fleet_worker_bin or local_bin)"
                        .into(),
                )
            })?;

        // 1. 디렉토리 준비
        let _ = exec.exec("sudo mkdir -p /etc/fleet").await;

        // 2. 바이너리 업로드 (base64 trick 또는 SFTP).
        exec.upload_file(local_bin, "/usr/local/bin/fleet-worker", 0o755)
            .await?;

        // 3. 설정 파일 작성 (템플릿).
        let config_toml = crate::templates::render_worker_config(&TemplateContext {
            tunnel_name: ctx.worker_name.clone(),
            hostname: ctx.orchestrator_url.clone(),
            credentials_path: "/etc/cloudflared/creds.json".into(),
        })?;
        exec.write_file("/tmp/fleet-worker.toml", &config_toml).await?;
        let _ = exec
            .exec("sudo mv /tmp/fleet-worker.toml /etc/fleet/worker.toml && sudo chmod 600 /etc/fleet/worker.toml")
            .await;

        // 4. systemd 유닛 작성.
        let unit = crate::templates::FLEET_WORKER_UNIT;
        exec.write_file("/tmp/fleet-worker.service", unit).await?;
        let _ = exec
            .exec("sudo mv /tmp/fleet-worker.service /etc/systemd/system/fleet-worker.service")
            .await;
        let _ = exec.exec("sudo systemctl daemon-reload").await;

        Ok(StepOutput::message(format!(
            "fleet-worker deployed from {local_bin}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;

    #[tokio::test]
    async fn is_applied_when_binary_and_unit_exist() {
        let exec = MockExecutor::new();
        exec.expect_exec("test -x /usr/local/bin/fleet-worker", "yes\n");
        exec.expect_exec("test -f /etc/systemd/system/fleet-worker.service", "yes\n");
        let step = InstallFleetWorker::default();
        assert!(step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn is_not_applied_when_binary_missing() {
        let exec = MockExecutor::new();
        exec.expect_exec("test -x /usr/local/bin/fleet-worker", "");
        let step = InstallFleetWorker::default();
        assert!(!step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn apply_requires_bin_path() {
        let exec = MockExecutor::new();
        let step = InstallFleetWorker::default();
        let result = step
            .apply(&exec, &StepContext::default())
            .await;
        assert!(matches!(result, Err(StepError::PrereqFailed(_))));
        let err = result.unwrap_err();
        assert!(format!("{err}").contains("fleet_worker_bin"));
    }

    #[tokio::test]
    async fn apply_uploads_binary_and_writes_config() {
        let exec = MockExecutor::new();
        let step = InstallFleetWorker {
            local_bin: Some("/tmp/test-worker".into()),
        };
        let ctx = StepContext {
            worker_name: "build-1".into(),
            orchestrator_url: "https://orch.fleet.example.com".into(),
            ..Default::default()
        };
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("/tmp/test-worker"));
        let calls = exec.recorded_calls();
        assert!(calls.iter().any(|c| c.contains("upload") && c.contains("fleet-worker")));
        assert!(calls.iter().any(|c| c.contains("write /tmp/fleet-worker.toml")));
        assert!(calls.iter().any(|c| c.contains("write /tmp/fleet-worker.service")));
    }

    #[tokio::test]
    async fn dry_run_skips_uploads() {
        let exec = MockExecutor::new();
        let step = InstallFleetWorker {
            local_bin: Some("/tmp/x".into()),
        };
        let ctx = StepContext {
            dry_run: true,
            ..Default::default()
        };
        step.apply(&exec, &ctx).await.unwrap();
        assert!(exec.recorded_calls().is_empty());
    }
}
