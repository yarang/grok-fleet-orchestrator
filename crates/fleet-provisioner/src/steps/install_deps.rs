//! Step 2: OS 종속성 설치. apt/dnf 감지 후 build-essential/openssl-devel 등 설치.

use async_trait::async_trait;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{PrereqReport, Step, StepContext, StepOutput};

pub struct InstallDeps {
    /// 사전 검증 결과. Playbook이 check_prereqs 결과를 주입.
    pub prereq: PrereqReport,
}

#[async_trait]
impl Step for InstallDeps {
    fn name(&self) -> &'static str {
        "install_deps"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["deps", "setup"]
    }

    async fn is_applied(&self, exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        // build-essential이 설치되어 있으면 적용됨으로 간주.
        let gcc = exec.exec("which gcc 2>/dev/null").await?;
        let pkg_config = exec.exec("which pkg-config 2>/dev/null").await?;
        Ok(!gcc.trim().is_empty() && !pkg_config.trim().is_empty())
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        if ctx.dry_run {
            tracing::info!("[dry-run] would install deps for {}", self.prereq.os);
            return Ok(StepOutput::message(format!(
                "dry-run: deps for {}",
                self.prereq.os
            )));
        }

        match self.prereq.os.as_str() {
            "ubuntu" | "debian" => {
                run_with_streaming(
                    exec,
                    "sudo apt-get update -qq && \
                     sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
                     build-essential pkg-config libssl-dev ca-certificates",
                )
                .await?;
            }
            "rhel" | "fedora" | "amzn" | "centos" | "rocky" | "alma" => {
                run_with_streaming(
                    exec,
                    "sudo dnf install -y -q gcc gcc-c++ make pkgconfig openssl-devel \
                     ca-certificates tar",
                )
                .await?;
            }
            other => {
                return Err(StepError::UnsupportedOs(other.into()));
            }
        }

        Ok(StepOutput::message(format!(
            "deps installed on {}",
            self.prereq.os
        )))
    }
}

async fn run_with_streaming(exec: &dyn RemoteExecutor, command: &str) -> Result<(), StepError> {
    let code = exec
        .exec_streaming(
            command,
            Box::new(|line| {
                tracing::info!("[remote] {line}");
            }),
        )
        .await?;
    if code != 0 {
        return Err(StepError::RemoteExit {
            code,
            stderr: format!("deps install exited {code}"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;

    fn ubuntu_prereq() -> PrereqReport {
        PrereqReport {
            os: "ubuntu".into(),
            arch: "x86_64".into(),
            mem_mb: 16384,
            disk_gb: 100,
            has_rust: false,
            has_systemd: true,
        }
    }

    #[tokio::test]
    async fn is_applied_when_gcc_and_pkgconfig_present() {
        let exec = MockExecutor::new();
        exec.expect_exec("which gcc", "/usr/bin/gcc\n");
        exec.expect_exec("which pkg-config", "/usr/bin/pkg-config\n");
        let step = InstallDeps {
            prereq: ubuntu_prereq(),
        };
        assert!(step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn is_not_applied_when_gcc_missing() {
        let exec = MockExecutor::new();
        exec.expect_exec("which gcc", "");
        exec.expect_exec("which pkg-config", "/usr/bin/pkg-config\n");
        let step = InstallDeps {
            prereq: ubuntu_prereq(),
        };
        assert!(!step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn apply_ubuntu_runs_apt_get() {
        let exec = MockExecutor::new();
        let step = InstallDeps {
            prereq: ubuntu_prereq(),
        };
        let out = step.apply(&exec, &StepContext::default()).await.unwrap();
        assert!(out.message.contains("ubuntu"));
        let calls = exec.recorded_calls();
        assert!(calls.iter().any(|c| c.contains("apt-get install")));
    }

    #[tokio::test]
    async fn apply_rhel_runs_dnf() {
        let exec = MockExecutor::new();
        let step = InstallDeps {
            prereq: PrereqReport {
                os: "rhel".into(),
                arch: "x86_64".into(),
                mem_mb: 16384,
                disk_gb: 100,
                has_rust: false,
                has_systemd: true,
            },
        };
        step.apply(&exec, &StepContext::default()).await.unwrap();
        let calls = exec.recorded_calls();
        assert!(calls.iter().any(|c| c.contains("dnf install")));
    }

    #[tokio::test]
    async fn apply_unsupported_os_errors() {
        let exec = MockExecutor::new();
        let step = InstallDeps {
            prereq: PrereqReport {
                os: "freebsd".into(),
                arch: "x86_64".into(),
                mem_mb: 16384,
                disk_gb: 100,
                has_rust: false,
                has_systemd: true,
            },
        };
        let result = step.apply(&exec, &StepContext::default()).await;
        assert!(matches!(result, Err(StepError::UnsupportedOs(_))));
    }

    #[tokio::test]
    async fn dry_run_skips_actual_install() {
        let exec = MockExecutor::new();
        let step = InstallDeps {
            prereq: ubuntu_prereq(),
        };
        let ctx = StepContext {
            dry_run: true,
            ..Default::default()
        };
        step.apply(&exec, &ctx).await.unwrap();
        let calls = exec.recorded_calls();
        assert!(calls.is_empty(), "dry-run should not exec anything");
    }
}
