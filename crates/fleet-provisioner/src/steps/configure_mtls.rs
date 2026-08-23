//! Step: `JoinWorker`가 만든 worker.toml에 `[mtls]` 섹션을 덧붙인다
//! (로드맵 `#85`).
//!
//! `/v1/workers/join` 응답을 렌더링하는 `fleet-api::handlers::render_worker_config_toml`은
//! `[mtls]`를 전혀 모른다(오케스트레이터 스키마에 mTLS 필드가 아예 없다) —
//! 그 렌더러를 확장하는 대신, `JoinWorker`가 만든 파일에 이 스텝이 원격에서
//! 직접 섹션을 덧붙인다. `push_credentials.rs`가 `/root/.grok/config.toml`을
//! 다루는 것과 같은 read-append-atomic write 패턴이다.

use async_trait::async_trait;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::issue_mtls_assets::{
    REMOTE_CLIENT_CA_PATH, REMOTE_SERVER_CERT_PATH, REMOTE_SERVER_KEY_PATH,
};
use crate::steps::join_worker::REMOTE_CONFIG_PATH;
use crate::steps::{Step, StepContext, StepOutput};

const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:2420";
const DEFAULT_ADVERTISED_PORT: u16 = 2420;

const TMP_PATCHED_CONFIG: &str = "/tmp/fleet-worker-mtls-patch.toml";

/// ConfigureMtls 스텝.
#[derive(Default)]
pub struct ConfigureMtls;

#[async_trait]
impl Step for ConfigureMtls {
    fn name(&self) -> &'static str {
        "configure_mtls"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["mtls", "worker"]
    }

    async fn is_applied(&self, exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        let out = exec
            .exec(&format!(
                "sudo grep -q '^\\[mtls\\]' {REMOTE_CONFIG_PATH} 2>/dev/null && echo yes"
            ))
            .await?;
        Ok(out.trim() == "yes")
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        if !ctx.mtls_enabled {
            return Ok(StepOutput::message(
                "mtls disabled; skipping [mtls] section",
            ));
        }
        if ctx.dry_run {
            return Ok(StepOutput::message(format!(
                "dry-run: would append [mtls] section to {REMOTE_CONFIG_PATH}"
            )));
        }

        let listen_addr = ctx
            .mtls_listen_addr
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_LISTEN_ADDR);
        let advertised_host = ctx
            .mtls_advertised_host
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&ctx.worker_name);
        let advertised_port = ctx.mtls_advertised_port.unwrap_or_else(|| {
            listen_addr
                .rsplit(':')
                .next()
                .and_then(|p| p.parse().ok())
                .unwrap_or(DEFAULT_ADVERTISED_PORT)
        });

        // JoinWorker가 성공적으로 만든 파일이라는 sanity check — 태그
        // 필터링(`--tags mtls`)으로 JoinWorker 없이 이 스텝만 단독 실행되는
        // 경로를 조용히 깨진 파일로 만들지 않기 위해서다.
        let existing = exec
            .exec(&format!("sudo cat {REMOTE_CONFIG_PATH} 2>/dev/null || true"))
            .await?;
        if !existing.contains("[worker]") {
            return Err(StepError::PrereqFailed(format!(
                "{REMOTE_CONFIG_PATH} does not look like a worker.toml written by JoinWorker \
                 (missing [worker] section) — run without --tags, or include 'join' so \
                 JoinWorker runs first"
            )));
        }

        let mtls_block = format!(
            "\n[mtls]\n\
             enabled = true\n\
             listen_addr = \"{listen_addr}\"\n\
             server_cert_path = \"{REMOTE_SERVER_CERT_PATH}\"\n\
             server_key_path = \"{REMOTE_SERVER_KEY_PATH}\"\n\
             client_ca_path = \"{REMOTE_CLIENT_CA_PATH}\"\n\
             advertised_host = \"{advertised_host}\"\n\
             advertised_port = {advertised_port}\n"
        );
        let mut merged = existing.trim_end().to_string();
        merged.push('\n');
        merged.push_str(&mtls_block);

        exec.write_file(TMP_PATCHED_CONFIG, &merged).await?;
        exec.exec_checked(&format!(
            "sudo mv {TMP_PATCHED_CONFIG} {REMOTE_CONFIG_PATH} \
             && sudo chmod 600 {REMOTE_CONFIG_PATH}"
        ))
        .await?;

        Ok(StepOutput::message(format!(
            "[mtls] section written → {REMOTE_CONFIG_PATH} (advertised {advertised_host}:{advertised_port})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;

    fn base_ctx() -> StepContext {
        StepContext {
            worker_name: "w1".into(),
            mtls_enabled: true,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn apply_disabled_is_noop() {
        let exec = MockExecutor::new();
        let step = ConfigureMtls;
        let out = step.apply(&exec, &StepContext::default()).await.unwrap();
        assert!(out.message.contains("disabled"));
        assert!(exec.recorded_calls().is_empty());
    }

    #[tokio::test]
    async fn dry_run_skips_everything() {
        let exec = MockExecutor::new();
        let step = ConfigureMtls;
        let mut ctx = base_ctx();
        ctx.dry_run = true;
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("dry-run"));
        assert!(exec.recorded_calls().is_empty());
    }

    #[tokio::test]
    async fn apply_rejects_missing_worker_section() {
        let exec = MockExecutor::new();
        exec.expect_exec(format!("sudo cat {REMOTE_CONFIG_PATH}"), "");
        let step = ConfigureMtls;
        let err = step.apply(&exec, &base_ctx()).await.unwrap_err();
        assert!(matches!(err, StepError::PrereqFailed(_)));
        assert!(format!("{err}").contains("JoinWorker"));
    }

    #[tokio::test]
    async fn apply_appends_mtls_section_preserving_existing_content() {
        let exec = MockExecutor::new();
        exec.expect_exec(
            format!("sudo cat {REMOTE_CONFIG_PATH}"),
            "[worker]\nname = \"w1\"\noperational_token = \"fwo_x\"\n\n[grok]\nbin = \"/usr/local/bin/grok\"\n",
        );
        let step = ConfigureMtls;
        let mut ctx = base_ctx();
        ctx.mtls_listen_addr = Some("0.0.0.0:9999".into());
        ctx.mtls_advertised_host = Some("w1.fleet.internal".into());

        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("w1.fleet.internal:9999"));

        let calls = exec.recorded_calls();
        let write_call = calls
            .iter()
            .find(|c| c.contains(&format!("write {TMP_PATCHED_CONFIG}")))
            .expect("expected a write call for the patched config");
        // MockExecutor의 write_file은 "write <path> (<n> bytes)" 형태로만
        // 기록하므로 실제 병합 내용은 write_file 트레이트 메서드가 받은 인자를
        // 직접 검증해야 한다 — MockExecutor는 그 인자를 보존하지 않으므로,
        // 여기서는 write가 정확히 한 번, 대상 경로로 일어났음과 이어지는
        // sudo mv/chmod가 최종 경로를 향함을 확인하는 선에서 검증한다.
        assert!(write_call.contains(TMP_PATCHED_CONFIG));
        assert!(calls
            .iter()
            .any(|c| c.contains(&format!("mv {TMP_PATCHED_CONFIG} {REMOTE_CONFIG_PATH}"))
                && c.contains("chmod 600")));
    }

    #[tokio::test]
    async fn advertised_port_defaults_from_listen_addr_port() {
        let exec = MockExecutor::new();
        exec.expect_exec(
            format!("sudo cat {REMOTE_CONFIG_PATH}"),
            "[worker]\nname = \"w1\"\n",
        );
        let step = ConfigureMtls;
        let mut ctx = base_ctx();
        ctx.mtls_listen_addr = Some("0.0.0.0:4444".into());
        // advertised_port 미설정 — listen_addr 포트로 폴백해야 함.
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains(":4444"));
    }

    #[tokio::test]
    async fn advertised_host_defaults_to_worker_name() {
        let exec = MockExecutor::new();
        exec.expect_exec(
            format!("sudo cat {REMOTE_CONFIG_PATH}"),
            "[worker]\nname = \"w1\"\n",
        );
        let step = ConfigureMtls;
        // mtls_advertised_host 미설정 — worker_name으로 폴백해야 함.
        let out = step.apply(&exec, &base_ctx()).await.unwrap();
        assert!(out.message.contains("w1:2420"));
    }

    #[tokio::test]
    async fn is_applied_when_mtls_section_present() {
        let exec = MockExecutor::new();
        exec.expect_exec("sudo grep -q '^\\[mtls\\]'", "yes\n");
        let step = ConfigureMtls;
        assert!(step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn is_not_applied_when_mtls_section_absent() {
        let exec = MockExecutor::new();
        let step = ConfigureMtls;
        assert!(!step.is_applied(&exec).await.unwrap());
    }

    #[test]
    fn step_has_mtls_tag() {
        assert!(ConfigureMtls.tags().contains(&"mtls"));
    }
}
