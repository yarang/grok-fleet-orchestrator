//! Step 5: fleet-worker 바이너리 배포 + 설정 파일 작성 + systemd 유닛 설치.

use async_trait::async_trait;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{Step, StepContext, StepOutput};
use crate::templates::TemplateContext;

/// fleet-worker 바이너리 배포 + 설정 파일 작성 + systemd 유닛 설치 스텝.
#[derive(Default)]
pub struct InstallFleetWorker {
    /// 로컬에 빌드된 fleet-worker 바이너리 경로. 명시하면 아래 아키텍처
    /// 매칭보다 우선한다(테스트·강제 오버라이드용).
    pub local_bin: Option<String>,
    /// 대상 호스트의 아키텍처(`PrereqReport.arch`). `Playbook::standard`가
    /// `check_prereqs` 결과로 채운다 (로드맵 `#81`). `ctx.fleet_worker_bin_by_arch`에서
    /// 이 값과 일치하는 바이너리를 우선 찾는 데 쓰인다.
    pub target_arch: Option<String>,
}

impl InstallFleetWorker {
    /// fleet-worker 바이너리 로컬 경로 선택 (로드맵 #81). dry-run 미리보기와
    /// 실제 배포가 같은 결과를 봐야 하므로 한 곳에 모았다.
    ///
    /// 우선순위: 1) 명시적 오버라이드(테스트·강제 지정용) 2) 아키텍처 매칭
    /// 바이너리(`ctx.fleet_worker_bin_by_arch[target_arch]`) 3) 아키텍처
    /// 무관 단일 폴백(`ctx.fleet_worker_bin`, 기존 단일 아키텍처 배포와
    /// 하위 호환).
    fn resolve_local_bin<'a>(&'a self, ctx: &'a StepContext) -> Option<&'a str> {
        let by_arch = self
            .target_arch
            .as_deref()
            .and_then(|arch| ctx.fleet_worker_bin_by_arch.get(arch))
            .map(String::as_str);
        self.local_bin
            .as_deref()
            .or(by_arch)
            .or(ctx.fleet_worker_bin.as_deref())
    }
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
        let bin = exec
            .exec("test -x /usr/local/bin/fleet-worker && echo yes")
            .await?;
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
            // dry-run 미리보기도 실제 경로와 같은 선택 로직을 써야 한다
            // (로드맵 #81) — 그러지 않으면 arm64 인벤토리를 dry-run해도
            // "어떤 바이너리가 선택될지" 미리 볼 수 없다.
            let bin = self.resolve_local_bin(ctx).unwrap_or("(unspecified)");
            return Ok(StepOutput::message(format!(
                "dry-run: deploy {bin} → /usr/local/bin/fleet-worker"
            )));
        }

        let local_bin = self.resolve_local_bin(ctx).ok_or_else(|| match &self.target_arch {
            Some(arch) => StepError::PrereqFailed(format!(
                "no fleet-worker binary available for arch '{arch}' — set \
                 ctx.fleet_worker_bin_by_arch['{arch}'] or ctx.fleet_worker_bin \
                 (or local_bin for a forced override)"
            )),
            None => StepError::PrereqFailed(
                "fleet_worker_bin path not provided (set ctx.fleet_worker_bin or local_bin)"
                    .into(),
            ),
        })?;

        // 1. 디렉토리 준비 (로드맵 #79 — 실패를 삼키지 않는다).
        exec.exec_checked("sudo mkdir -p /etc/fleet").await?;

        // 2. 바이너리 업로드 (base64 trick 또는 SFTP).
        exec.upload_file(local_bin, "/usr/local/bin/fleet-worker", 0o755)
            .await?;

        // 3. 설정 파일 작성 (템플릿).
        let config_toml = crate::templates::render_worker_config(&TemplateContext {
            tunnel_name: ctx.worker_name.clone(),
            hostname: ctx.orchestrator_url.clone(),
            credentials_path: "/etc/cloudflared/creds.json".into(),
            grok_secret: ctx.grok_secret.clone(),
            grok_bind_addr: ctx.grok_bind_addr.clone(),
            max_concurrent_tasks: ctx.max_concurrent_tasks,
            bootstrap_token: ctx.bootstrap_token.clone(),
            labels: Some(ctx.labels.clone()),
            mtls_enabled: ctx.mtls_enabled,
            mtls_listen_addr: ctx.mtls_listen_addr.clone(),
            mtls_server_cert_path: ctx.mtls_server_cert_path.clone(),
            mtls_server_key_path: ctx.mtls_server_key_path.clone(),
            mtls_client_ca_path: ctx.mtls_client_ca_path.clone(),
            mtls_advertised_host: ctx.mtls_advertised_host.clone(),
            mtls_advertised_port: ctx.mtls_advertised_port,
            ..Default::default()
        })?;
        exec.write_file("/tmp/fleet-worker.toml", &config_toml)
            .await?;
        exec.exec_checked(
            "sudo mv /tmp/fleet-worker.toml /etc/fleet/worker.toml && sudo chmod 600 /etc/fleet/worker.toml",
        )
        .await?;

        // 4. systemd 유닛 작성.
        let unit = crate::templates::FLEET_WORKER_UNIT;
        exec.write_file("/tmp/fleet-worker.service", unit).await?;
        exec.exec_checked(
            "sudo mv /tmp/fleet-worker.service /etc/systemd/system/fleet-worker.service",
        )
        .await?;
        exec.exec_checked("sudo systemctl daemon-reload").await?;

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
        let result = step.apply(&exec, &StepContext::default()).await;
        assert!(matches!(result, Err(StepError::PrereqFailed(_))));
        let err = result.unwrap_err();
        assert!(format!("{err}").contains("fleet_worker_bin"));
    }

    #[tokio::test]
    async fn apply_uploads_binary_and_writes_config() {
        let exec = MockExecutor::new();
        let step = InstallFleetWorker {
            local_bin: Some("/tmp/test-worker".into()),
            ..InstallFleetWorker::default()
        };
        let ctx = StepContext {
            worker_name: "build-1".into(),
            orchestrator_url: "https://orch.fleet.example.com".into(),
            grok_secret: Some("server-secret".into()),
            ..Default::default()
        };
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("/tmp/test-worker"));
        let calls = exec.recorded_calls();
        assert!(calls
            .iter()
            .any(|c| c.contains("upload") && c.contains("fleet-worker")));
        assert!(calls
            .iter()
            .any(|c| c.contains("write /tmp/fleet-worker.toml")));
        assert!(calls
            .iter()
            .any(|c| c.contains("write /tmp/fleet-worker.service")));
    }

    #[tokio::test]
    async fn apply_fails_without_grok_secret() {
        let exec = MockExecutor::new();
        let step = InstallFleetWorker {
            local_bin: Some("/tmp/test-worker".into()),
            ..InstallFleetWorker::default()
        };
        let ctx = StepContext {
            worker_name: "build-1".into(),
            orchestrator_url: "https://orch.fleet.example.com".into(),
            ..Default::default()
        };
        let result = step.apply(&exec, &ctx).await;
        assert!(matches!(result, Err(StepError::Template(_))));
        let err = result.unwrap_err();
        assert!(format!("{err}").contains("grok_secret"));
    }

    #[tokio::test]
    async fn dry_run_skips_uploads() {
        let exec = MockExecutor::new();
        let step = InstallFleetWorker {
            local_bin: Some("/tmp/x".into()),
            ..InstallFleetWorker::default()
        };
        let ctx = StepContext {
            dry_run: true,
            ..Default::default()
        };
        step.apply(&exec, &ctx).await.unwrap();
        assert!(exec.recorded_calls().is_empty());
    }

    #[tokio::test]
    async fn dry_run_message_reflects_arch_selected_binary() {
        // dry-run 미리보기가 실제 배포와 같은 바이너리를 "선택할 것"이라고
        // 보여줘야 한다 — 그래야 arm64 인벤토리를 dry-run했을 때 잘못된
        // (또는 미지정) 바이너리가 배포될 것을 사전에 알 수 있다 (로드맵 #81).
        let exec = MockExecutor::new();
        let step = InstallFleetWorker {
            target_arch: Some("aarch64".into()),
            ..InstallFleetWorker::default()
        };
        let mut ctx = StepContext {
            dry_run: true,
            ..Default::default()
        };
        ctx.fleet_worker_bin_by_arch
            .insert("aarch64".into(), "/tmp/fleet-worker-aarch64".into());

        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(
            out.message.contains("fleet-worker-aarch64"),
            "dry-run message should preview the arch-matched binary, got: {}",
            out.message
        );
        assert!(exec.recorded_calls().is_empty());
    }

    // ── 아키텍처별 바이너리 선택 (로드맵 #81) ─────────────────────────────

    fn context_with_secret() -> StepContext {
        StepContext {
            worker_name: "build-1".into(),
            orchestrator_url: "https://orch.fleet.example.com".into(),
            grok_secret: Some("server-secret".into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn uses_arch_specific_binary_when_available() {
        let exec = MockExecutor::new();
        let step = InstallFleetWorker {
            target_arch: Some("aarch64".into()),
            ..InstallFleetWorker::default()
        };
        let mut ctx = context_with_secret();
        ctx.fleet_worker_bin_by_arch
            .insert("aarch64".into(), "/tmp/fleet-worker-aarch64".into());
        ctx.fleet_worker_bin = Some("/tmp/fleet-worker-x86_64".into());

        step.apply(&exec, &ctx).await.unwrap();
        let calls = exec.recorded_calls();
        assert!(
            calls
                .iter()
                .any(|c| c.contains("fleet-worker-aarch64")),
            "expected the aarch64-specific binary to be uploaded, got: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.contains("fleet-worker-x86_64")),
            "must not fall back to the untargeted path when an arch match exists"
        );
    }

    #[tokio::test]
    async fn falls_back_to_untargeted_binary_when_arch_not_in_map() {
        let exec = MockExecutor::new();
        let step = InstallFleetWorker {
            target_arch: Some("aarch64".into()),
            ..InstallFleetWorker::default()
        };
        let mut ctx = context_with_secret();
        // 맵에는 x86_64만 있고 감지된 arch(aarch64)는 없다 — 단일 폴백을 써야 한다.
        ctx.fleet_worker_bin_by_arch
            .insert("x86_64".into(), "/tmp/fleet-worker-x86_64".into());
        ctx.fleet_worker_bin = Some("/tmp/fleet-worker-generic".into());

        step.apply(&exec, &ctx).await.unwrap();
        let calls = exec.recorded_calls();
        assert!(calls.iter().any(|c| c.contains("fleet-worker-generic")));
    }

    #[tokio::test]
    async fn explicit_local_bin_overrides_arch_map() {
        let exec = MockExecutor::new();
        let step = InstallFleetWorker {
            local_bin: Some("/tmp/forced-override".into()),
            target_arch: Some("aarch64".into()),
        };
        let mut ctx = context_with_secret();
        ctx.fleet_worker_bin_by_arch
            .insert("aarch64".into(), "/tmp/fleet-worker-aarch64".into());

        step.apply(&exec, &ctx).await.unwrap();
        let calls = exec.recorded_calls();
        assert!(calls.iter().any(|c| c.contains("forced-override")));
    }

    #[tokio::test]
    async fn error_message_names_the_missing_arch() {
        let exec = MockExecutor::new();
        let step = InstallFleetWorker {
            target_arch: Some("aarch64".into()),
            ..InstallFleetWorker::default()
        };
        let ctx = context_with_secret(); // fleet_worker_bin_by_arch/fleet_worker_bin 둘 다 비어있음.

        let result = step.apply(&exec, &ctx).await;
        let err = result.unwrap_err();
        assert!(matches!(err, StepError::PrereqFailed(_)));
        assert!(format!("{err}").contains("aarch64"));
    }
}
