//! Step 1: 사전 검증. OS, arch, 디스크/메모리, Rust, systemd 여부 확인.

use async_trait::async_trait;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{PrereqReport, Step, StepContext, StepOutput};

/// 사전 검증 스텝. 항상 실행 (멱등 — 부작용 없음).
pub struct CheckPrereqs {
    /// 최소 디스크 여유 (GB).
    pub min_disk_gb: u64,
    /// 최소 메모리 (MB).
    pub min_mem_mb: u64,
}

impl Default for CheckPrereqs {
    fn default() -> Self {
        Self {
            min_disk_gb: 10,
            min_mem_mb: 4096,
        }
    }
}

#[async_trait]
impl Step for CheckPrereqs {
    fn name(&self) -> &'static str {
        "check_prereqs"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["check", "prereqs"]
    }

    async fn is_applied(&self, _exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        // 항상 실행 — 사전 검증은 부작용이 없으므로 반복 수행해도 안전.
        Ok(false)
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        _ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        let os_raw = exec.exec("cat /etc/os-release | grep '^ID=' | cut -d= -f2").await?;
        let os = os_raw.trim().trim_matches('"').to_lowercase();

        let arch = exec.exec("uname -m").await?;
        let arch = arch.trim().to_string();

        let mem_str = exec.exec("free -m | awk '/^Mem:/{print $2}'").await?;
        let mem_mb: u64 = mem_str.trim().parse().unwrap_or(0);

        let disk_str = exec.exec("df -BG / | awk 'NR==2{print $4}'").await?;
        let disk_clean = disk_str.trim().trim_end_matches('G');
        let disk_gb: u64 = disk_clean.parse().unwrap_or(0);

        let rust_path = exec.exec("which cargo 2>/dev/null").await?;
        let has_rust = !rust_path.trim().is_empty();

        let systemd_pid = exec.exec("pidof systemd 2>/dev/null").await?;
        let has_systemd = !systemd_pid.trim().is_empty();

        let report = PrereqReport {
            os,
            arch,
            mem_mb,
            disk_gb,
            has_rust,
            has_systemd,
        };

        // 검증
        if !has_systemd {
            return Err(StepError::PrereqFailed(
                "systemd is required for service management".into(),
            ));
        }
        if report.os.is_empty() {
            return Err(StepError::PrereqFailed(
                "could not detect OS (empty /etc/os-release ID)".into(),
            ));
        }
        if mem_mb > 0 && mem_mb < self.min_mem_mb {
            return Err(StepError::PrereqFailed(format!(
                "insufficient memory: {} MB < {} MB minimum",
                mem_mb, self.min_mem_mb
            )));
        }
        if disk_gb > 0 && disk_gb < self.min_disk_gb {
            return Err(StepError::PrereqFailed(format!(
                "insufficient disk: {} GB < {} GB minimum",
                disk_gb, self.min_disk_gb
            )));
        }

        tracing::info!(
            os = %report.os,
            arch = %report.arch,
            mem_mb = report.mem_mb,
            disk_gb = report.disk_gb,
            has_rust = report.has_rust,
            "prereqs verified"
        );

        Ok(StepOutput::with_payload(
            format!(
                "prereqs ok: {} on {}, {}MB RAM, {}GB disk",
                report.os, report.arch, report.mem_mb, report.disk_gb
            ),
            &report,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;

    fn healthy_responses() -> MockExecutor {
        let exec = MockExecutor::new();
        exec.expect_exec("cat /etc/os-release", "ubuntu\n");
        exec.expect_exec("uname -m", "x86_64\n");
        exec.expect_exec("free -m", "16384\n");
        exec.expect_exec("df -BG", "200G\n");
        exec.expect_exec("which cargo", "/home/user/.cargo/bin/cargo\n");
        exec.expect_exec("pidof systemd", "1\n");
        exec
    }

    #[tokio::test]
    async fn detects_ubuntu_x86_with_sufficient_resources() {
        let exec = healthy_responses();
        let step = CheckPrereqs::default();
        let ctx = StepContext::default();
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("ubuntu"));
        assert!(out.message.contains("x86_64"));
        let report: PrereqReport =
            serde_json::from_value(out.payload.unwrap()).unwrap();
        assert_eq!(report.os, "ubuntu");
        assert_eq!(report.arch, "x86_64");
        assert_eq!(report.mem_mb, 16384);
        assert_eq!(report.disk_gb, 200);
        assert!(report.has_rust);
        assert!(report.has_systemd);
    }

    #[tokio::test]
    async fn fails_when_systemd_missing() {
        let exec = MockExecutor::new();
        exec.expect_exec("cat /etc/os-release", "ubuntu\n");
        exec.expect_exec("uname -m", "x86_64\n");
        exec.expect_exec("free -m", "16384\n");
        exec.expect_exec("df -BG", "200G\n");
        exec.expect_exec("which cargo", "");
        exec.expect_exec("pidof systemd", "");
        let step = CheckPrereqs::default();
        let result = step.apply(&exec, &StepContext::default()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, StepError::PrereqFailed(_)));
        assert!(format!("{err}").contains("systemd"));
    }

    #[tokio::test]
    async fn fails_when_disk_insufficient() {
        let exec = MockExecutor::new();
        exec.expect_exec("cat /etc/os-release", "ubuntu\n");
        exec.expect_exec("uname -m", "x86_64\n");
        exec.expect_exec("free -m", "16384\n");
        exec.expect_exec("df -BG", "5G\n");
        exec.expect_exec("which cargo", "");
        exec.expect_exec("pidof systemd", "1\n");
        let step = CheckPrereqs::default();
        let result = step.apply(&exec, &StepContext::default()).await;
        assert!(matches!(result, Err(StepError::PrereqFailed(_))));
    }

    #[tokio::test]
    async fn is_applied_always_false() {
        let exec = MockExecutor::new();
        let step = CheckPrereqs::default();
        assert!(!step.is_applied(&exec).await.unwrap());
    }
}
