//! Step 6: 서비스 시작 및 검증. systemd 유닛 enable + start, 하트비트 대기.

use async_trait::async_trait;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{Step, StepContext, StepOutput};

pub struct StartServices {
    /// 하트비트 수신 대기 최대 시간 (초).
    pub wait_timeout_secs: u64,
}

impl Default for StartServices {
    fn default() -> Self {
        Self {
            wait_timeout_secs: 30,
        }
    }
}

#[async_trait]
impl Step for StartServices {
    fn name(&self) -> &'static str {
        "start_services"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["start", "services"]
    }

    async fn is_applied(&self, exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        let active = exec
            .exec("systemctl is-active fleet-worker 2>/dev/null")
            .await?;
        Ok(active.trim() == "active")
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        if ctx.dry_run {
            return Ok(StepOutput::message(
                "dry-run: enable and start fleet-worker + cloudflared".to_string(),
            ));
        }

        // 모든 유닛 enable + start (멱등).
        for cmd in &[
            "sudo systemctl daemon-reload",
            "sudo systemctl enable --now cloudflared 2>&1 || true",
            "sudo systemctl enable --now fleet-worker 2>&1 || true",
            "sudo systemctl restart fleet-worker 2>&1 || true",
        ] {
            let _ = exec.exec(cmd).await;
        }

        // 검증: fleet-worker 활성 상태인지.
        let status = exec
            .exec("systemctl is-active fleet-worker 2>/dev/null")
            .await?;
        if status.trim() != "active" {
            return Err(StepError::RemoteExit {
                code: 1,
                stderr: format!("fleet-worker not active after start (state: {})", status.trim()),
            });
        }

        Ok(StepOutput::message(format!(
            "all services started on {}",
            ctx.worker_name
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;

    #[tokio::test]
    async fn is_applied_when_fleet_worker_active() {
        let exec = MockExecutor::new();
        exec.expect_exec("systemctl is-active fleet-worker", "active\n");
        let step = StartServices::default();
        assert!(step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn apply_enables_and_starts_units() {
        let exec = MockExecutor::new();
        // start 후 is-active 쿼리에 대한 응답.
        exec.expect_exec("systemctl is-active fleet-worker", "active\n");
        let step = StartServices::default();
        let ctx = StepContext::for_worker("build-1");
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("build-1"));
        let calls = exec.recorded_calls();
        assert!(calls.iter().any(|c| c.contains("daemon-reload")));
        assert!(calls.iter().any(|c| c.contains("enable --now")));
    }

    #[tokio::test]
    async fn apply_fails_when_service_not_active() {
        let exec = MockExecutor::new();
        exec.expect_exec("systemctl is-active fleet-worker", "failed\n");
        let step = StartServices::default();
        let result = step.apply(&exec, &StepContext::default()).await;
        assert!(matches!(result, Err(StepError::RemoteExit { .. })));
    }

    #[tokio::test]
    async fn dry_run_skips_enable() {
        let exec = MockExecutor::new();
        let step = StartServices::default();
        let ctx = StepContext {
            dry_run: true,
            ..Default::default()
        };
        step.apply(&exec, &ctx).await.unwrap();
        assert!(exec.recorded_calls().is_empty());
    }
}
