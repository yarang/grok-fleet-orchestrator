//! Step 3: grok CLI 설치. 이미 설치된 경우 버전 비교 후 필요시 업그레이드.

use async_trait::async_trait;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{Step, StepContext, StepOutput};

/// grok CLI 설치 스텝.
#[derive(Default)]
pub struct InstallGrok {
    /// 요구할 최소 버전 (예: "0.1.0"). 미설정 시 CARGO_PKG_VERSION 사용.
    pub required_version: Option<String>,
}

#[async_trait]
impl Step for InstallGrok {
    fn name(&self) -> &'static str {
        "install_grok"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["grok", "setup"]
    }

    async fn is_applied(&self, exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        let existing = exec.exec("grok --version 2>/dev/null").await?;
        if existing.trim().is_empty() {
            return Ok(false);
        }
        if let Some(req) = &self.required_version {
            let installed = parse_version(existing.trim());
            let required = parse_version(req);
            if installed >= required {
                tracing::debug!(installed = %existing.trim(), required = %req, "grok already up-to-date");
                return Ok(true);
            }
            tracing::info!(installed = %existing.trim(), required = %req, "grok upgrade needed");
            return Ok(false);
        }
        Ok(true)
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        if ctx.dry_run {
            return Ok(StepOutput::message("dry-run: install grok".to_string()));
        }

        // 공식 인스톨러. 로깅하며 실행.
        let code = exec
            .exec_streaming(
                "curl -fsSL https://x.ai/cli/install.sh | bash",
                Box::new(|line| tracing::info!("[remote] {line}")),
            )
            .await?;
        if code != 0 {
            return Err(StepError::RemoteExit {
                code,
                stderr: "grok installer failed".into(),
            });
        }

        let version = exec.exec("grok --version").await?;
        let version = version.trim().to_string();
        if version.is_empty() {
            return Err(StepError::PrereqFailed(
                "grok --version returned empty after install".into(),
            ));
        }
        Ok(StepOutput::message(format!("grok installed: {version}")))
    }
}

/// 단순한 semantic version 파서. 문자열에서 첫 번째 `N.N.N` 패턴 추출.
/// 비정상 입력은 (0,0,0) 반환.
pub fn parse_version(s: &str) -> (u32, u32, u32) {
    // 문자열 전체에서 첫 번째 digit으로 시작하는 시퀀스 찾기.
    let start = match s.find(|c: char| c.is_ascii_digit()) {
        Some(idx) => idx,
        None => return (0, 0, 0),
    };
    let rest = &s[start..];
    let cleaned: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if cleaned.is_empty() {
        return (0, 0, 0);
    }
    let parts: Vec<u32> = cleaned.split('.').filter_map(|p| p.parse().ok()).collect();
    (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;

    #[test]
    fn parse_version_handles_common_formats() {
        assert_eq!(parse_version("1.2.3"), (1, 2, 3));
        assert_eq!(parse_version("grok 0.5.1"), (0, 5, 1));
        assert_eq!(parse_version("v2.0.0-rc1"), (2, 0, 0));
        assert_eq!(parse_version("garbage"), (0, 0, 0));
        assert_eq!(parse_version("1"), (1, 0, 0));
    }

    #[tokio::test]
    async fn is_applied_false_when_grok_missing() {
        let exec = MockExecutor::new();
        exec.expect_exec("grok --version", "");
        let step = InstallGrok::default();
        assert!(!step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn is_applied_true_when_version_meets_required() {
        let exec = MockExecutor::new();
        exec.expect_exec("grok --version", "grok 1.5.0\n");
        let step = InstallGrok {
            required_version: Some("1.0.0".into()),
        };
        assert!(step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn is_applied_false_when_version_below_required() {
        let exec = MockExecutor::new();
        exec.expect_exec("grok --version", "grok 0.5.0\n");
        let step = InstallGrok {
            required_version: Some("1.0.0".into()),
        };
        assert!(!step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn apply_invokes_installer_and_verifies() {
        let exec = MockExecutor::new();
        // install_grok 단계는 install 명령 실행 후 --version 호출.
        exec.expect_exec("grok --version", "grok 1.0.0\n");
        let step = InstallGrok::default();
        let out = step.apply(&exec, &StepContext::default()).await.unwrap();
        assert!(out.message.contains("1.0.0"));
        let calls = exec.recorded_calls();
        assert!(calls.iter().any(|c| c.contains("install.sh")));
    }

    #[tokio::test]
    async fn apply_fails_when_version_still_empty_after_install() {
        let exec = MockExecutor::new();
        exec.expect_exec("grok --version", "");
        let step = InstallGrok::default();
        let result = step.apply(&exec, &StepContext::default()).await;
        assert!(matches!(result, Err(StepError::PrereqFailed(_))));
    }
}
