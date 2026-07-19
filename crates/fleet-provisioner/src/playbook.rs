//! Playbook — 스텝 시퀀스를 실행하며 멱등성 보장과 진단 로그 제공.
//!
//! ```rust,ignore
//! use fleet_provisioner::prelude::*;
//!
//! let playbook = Playbook::standard(&prereq);
//! let report = playbook.run(&executor, &ctx).await?;
//! ```

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::PlaybookError;
use crate::ssh::RemoteExecutor;
use crate::steps::StepContext;
use crate::steps::{
    CheckPrereqs, InstallCloudflared, InstallDeps, InstallFleetWorker, InstallGrok, PrereqReport,
    StartServices, Step,
};

/// 개별 스텝 실행 결과 (report에 포함).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepStatus {
    /// `is_applied == true`로 건너뜀.
    Skipped,
    /// 정상 실행.
    Applied { message: String },
    /// 실패 (다음 스텝 진행 안 함).
    Failed { error: String },
}

/// Playbook 실행 최종 보고서.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookReport {
    pub worker_name: String,
    pub steps: Vec<StepReport>,
    pub succeeded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepReport {
    pub name: String,
    pub status: StepStatus,
}

/// Playbook 컨텍스트. StepContext를 감싸고 Playbook 실행 중 부가 정보를 전달.
#[derive(Debug, Clone, Default)]
pub struct PlaybookContext {
    /// Playbook 전체에 적용되는 기본 컨텍스트.
    pub base: StepContext,
    /// `--tags` 옵션으로 지정된 태그. None이면 모든 스텝 실행.
    pub only_tags: Option<Vec<String>>,
}

impl PlaybookContext {
    pub fn new(base: StepContext) -> Self {
        Self {
            base,
            only_tags: None,
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.only_tags = Some(tags);
        self
    }
}

/// Playbook — 스텝 리스트를 보유하고 run()으로 순차 실행.
pub struct Playbook {
    steps: Vec<Arc<dyn Step>>,
}

impl Playbook {
    pub fn new(steps: Vec<Arc<dyn Step>>) -> Self {
        Self { steps }
    }

    /// 표준 Playbook (6개 스텝). `prereq`는 check_prereqs 이후 스텝들이 사용.
    pub fn standard(prereq: &PrereqReport) -> Self {
        let steps: Vec<Arc<dyn Step>> = vec![
            Arc::new(CheckPrereqs::default()),
            Arc::new(InstallDeps {
                prereq: prereq.clone(),
            }),
            Arc::new(InstallGrok::default()),
            Arc::new(InstallCloudflared::default()),
            Arc::new(InstallFleetWorker::default()),
            Arc::new(StartServices::default()),
        ];
        Self::new(steps)
    }

    /// Dry-run 전용 Playbook (변경 없이 무엇을 할지 로깅).
    pub fn dry_run(prereq: &PrereqReport) -> Self {
        // dry_run은 ctx로 전달되므로 Playbook 자체는 standard와 동일.
        Self::standard(prereq)
    }

    /// 스텝 수.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Playbook 실행.
    ///
    /// - `only_tags`가 지정된 경우, 해당 태그 중 하나라도 포함된 스텝만 실행.
    /// - 각 스텝은 `is_applied()`를 먼저 검사. true면 Skipped.
    /// - 실패 시 즉시 중단하고 Failed 상태로 보고.
    pub async fn run(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &PlaybookContext,
    ) -> Result<PlaybookReport, PlaybookError> {
        let mut reports = Vec::with_capacity(self.steps.len());
        let host = ctx.base.worker_name.clone();

        for step in &self.steps {
            // 태그 필터링
            if let Some(tags) = &ctx.only_tags {
                let step_tags = step.tags();
                let matched = step_tags.iter().any(|t| tags.iter().any(|f| f == t));
                if !matched {
                    continue;
                }
            }

            let name = step.name();
            tracing::info!(step = name, host = %host, "running step");

            // 멱등성 검사
            let already_applied =
                step.is_applied(exec)
                    .await
                    .map_err(|e| PlaybookError::StepFailed {
                        step: name.into(),
                        host: host.clone(),
                        source: e,
                    })?;

            if already_applied {
                tracing::info!(step = name, "already applied, skipping");
                reports.push(StepReport {
                    name: name.into(),
                    status: StepStatus::Skipped,
                });
                continue;
            }

            // 실행
            match step.apply(exec, &ctx.base).await {
                Ok(output) => {
                    tracing::info!(step = name, message = %output.message, "step done");
                    reports.push(StepReport {
                        name: name.into(),
                        status: StepStatus::Applied {
                            message: output.message,
                        },
                    });
                }
                Err(e) => {
                    let err_msg = format!("{e}");
                    tracing::error!(step = name, error = %err_msg, "step failed");
                    reports.push(StepReport {
                        name: name.into(),
                        status: StepStatus::Failed {
                            error: err_msg.clone(),
                        },
                    });
                    return Err(PlaybookError::StepFailed {
                        step: name.into(),
                        host: host.clone(),
                        source: e,
                    });
                }
            }
        }

        Ok(PlaybookReport {
            worker_name: host,
            steps: reports,
            succeeded: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::StepError;
    use crate::ssh::MockExecutor;
    use crate::steps::StepOutput;
    use async_trait::async_trait;

    struct AlwaysApplied;
    #[async_trait]
    impl Step for AlwaysApplied {
        fn name(&self) -> &'static str {
            "always_applied"
        }
        async fn is_applied(&self, _exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
            Ok(true)
        }
        async fn apply(
            &self,
            _exec: &dyn RemoteExecutor,
            _ctx: &StepContext,
        ) -> Result<StepOutput, StepError> {
            unreachable!("is_applied is always true")
        }
    }

    struct NeverApplied;
    #[async_trait]
    impl Step for NeverApplied {
        fn name(&self) -> &'static str {
            "never_applied"
        }
        async fn is_applied(&self, _exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
            Ok(false)
        }
        async fn apply(
            &self,
            _exec: &dyn RemoteExecutor,
            _ctx: &StepContext,
        ) -> Result<StepOutput, StepError> {
            Ok(StepOutput::message("applied"))
        }
    }

    struct Failing;
    #[async_trait]
    impl Step for Failing {
        fn name(&self) -> &'static str {
            "failing"
        }
        async fn is_applied(&self, _exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
            Ok(false)
        }
        async fn apply(
            &self,
            _exec: &dyn RemoteExecutor,
            _ctx: &StepContext,
        ) -> Result<StepOutput, StepError> {
            Err(StepError::UnsupportedOs("unknown".into()))
        }
    }

    fn make_playbook(steps: Vec<Arc<dyn Step>>) -> Playbook {
        Playbook::new(steps)
    }

    #[tokio::test]
    async fn skips_steps_that_are_already_applied() {
        let exec = MockExecutor::new();
        let pb = make_playbook(vec![Arc::new(AlwaysApplied)]);
        let report = pb
            .run(&exec, &PlaybookContext::new(StepContext::default()))
            .await
            .unwrap();
        assert_eq!(report.steps.len(), 1);
        assert!(matches!(report.steps[0].status, StepStatus::Skipped));
    }

    #[tokio::test]
    async fn applies_steps_when_not_applied() {
        let exec = MockExecutor::new();
        let pb = make_playbook(vec![Arc::new(NeverApplied)]);
        let report = pb
            .run(&exec, &PlaybookContext::new(StepContext::default()))
            .await
            .unwrap();
        assert!(matches!(report.steps[0].status, StepStatus::Applied { .. }));
    }

    #[tokio::test]
    async fn stops_on_step_failure() {
        let exec = MockExecutor::new();
        let pb = make_playbook(vec![Arc::new(Failing), Arc::new(NeverApplied)]);
        let result = pb
            .run(&exec, &PlaybookContext::new(StepContext::default()))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            PlaybookError::StepFailed {
                step,
                host: _,
                source,
            } => {
                assert_eq!(step, "failing");
                assert!(matches!(source, StepError::UnsupportedOs(_)));
            }
            _ => panic!("unexpected error variant"),
        }
    }

    #[tokio::test]
    async fn tag_filter_includes_only_matching_steps() {
        struct Tagged(&'static str, &'static [&'static str]);
        #[async_trait]
        impl Step for Tagged {
            fn name(&self) -> &'static str {
                self.0
            }
            fn tags(&self) -> &'static [&'static str] {
                self.1
            }
            async fn is_applied(&self, _: &dyn RemoteExecutor) -> Result<bool, StepError> {
                Ok(false)
            }
            async fn apply(
                &self,
                _: &dyn RemoteExecutor,
                _: &StepContext,
            ) -> Result<StepOutput, StepError> {
                Ok(StepOutput::message(format!("{} applied", self.0)))
            }
        }
        let exec = MockExecutor::new();
        let pb = make_playbook(vec![
            Arc::new(Tagged("a", &["setup"])),
            Arc::new(Tagged("b", &["tunnel"])),
            Arc::new(Tagged("c", &["setup", "verify"])),
        ]);
        let ctx = PlaybookContext::new(StepContext::default()).with_tags(vec!["setup".into()]);
        let report = pb.run(&exec, &ctx).await.unwrap();
        // b는 tunnel만 가지므로 제외.
        assert_eq!(report.steps.len(), 2);
        assert_eq!(report.steps[0].name, "a");
        assert_eq!(report.steps[1].name, "c");
    }

    #[tokio::test]
    async fn standard_playbook_has_six_steps() {
        let prereq = PrereqReport {
            os: "ubuntu".into(),
            arch: "x86_64".into(),
            mem_mb: 16384,
            disk_gb: 100,
            has_rust: false,
            has_systemd: true,
        };
        let pb = Playbook::standard(&prereq);
        assert_eq!(pb.len(), 6);
    }
}
