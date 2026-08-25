//! Step: mTLS 서버 인증서/키/CA를 원격 호스트로 배포 (로드맵 `#85`).
//!
//! 인증서 발급(rcgen 서명 로직) 자체는 이 크레이트의 책임이 아니다 —
//! `fleet-cli`가 `fleet mtls issue-server`(같은 크레이트의
//! `mtls::run_issue_server`)로 이 프로세스를 실행하는 로컬 머신에서 미리
//! 발급해, 그 결과 파일의 **로컬** 경로를 `StepContext`의
//! `mtls_server_cert_path`/`mtls_server_key_path`/`mtls_client_ca_path`에
//! 채워 넣는다. 이 스텝은 그 3개 로컬 파일을 [`REMOTE_MTLS_DIR`] 하위 고정
//! 경로로 원자적으로 업로드하기만 한다 — 목적지 경로가 고정이라
//! `JoinWorker` 뒤에 오는 `ConfigureMtls`가 worker.toml에 채울 값을 별도
//! 조율 없이 알 수 있다.
//!
//! `upload_file`(로드맵 `#84`)을 직접 쓰지 않고 `write_file`로 `/tmp`에
//! 스테이징한 뒤 `sudo mv`/`sudo chmod`하는 이유: `upload_file`은 SSH 로그인
//! 사용자 권한으로 곧바로 목적지에 쓴다(`InstallFleetWorker`가 그렇게
//! `/usr/local/bin`에 쓰는 것과 동일 가정) — 하지만 `/etc/fleet/mtls`는
//! `sudo mkdir -p`로 root 소유로 만들어지므로, 비root SSH 사용자는 직접 쓸 수
//! 없다. 이 크레이트의 다른 특권 파일 쓰기(`push_credentials.rs`,
//! `install_cloudflared.rs`, `JoinWorker`가 만드는 `/etc/fleet/worker.toml`)가
//! 전부 이 스테이징+sudo mv 패턴을 쓰므로 그대로 따른다.

use async_trait::async_trait;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{Step, StepContext, StepOutput};

/// 원격 mTLS 자산 디렉토리. 인벤토리/CLI로 바꿀 수 없는 고정 규약이다 —
/// 이 스텝이 쓰는 경로와 `ConfigureMtls`가 worker.toml에 채우는 경로가
/// 항상 정확히 일치해야 하므로, 설정 가능하게 만들면 그 둘이 어긋날 여지가
/// 생긴다.
pub const REMOTE_MTLS_DIR: &str = "/etc/fleet/mtls";
pub const REMOTE_SERVER_CERT_PATH: &str = "/etc/fleet/mtls/server.pem";
pub const REMOTE_SERVER_KEY_PATH: &str = "/etc/fleet/mtls/server.key";
pub const REMOTE_CLIENT_CA_PATH: &str = "/etc/fleet/mtls/ca.pem";

const TMP_CERT: &str = "/tmp/fleet-mtls-server.pem";
const TMP_KEY: &str = "/tmp/fleet-mtls-server.key";
const TMP_CA: &str = "/tmp/fleet-mtls-ca.pem";

/// IssueMtlsAssets 스텝.
#[derive(Default)]
pub struct IssueMtlsAssets;

#[async_trait]
impl Step for IssueMtlsAssets {
    fn name(&self) -> &'static str {
        "issue_mtls_assets"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["mtls", "worker"]
    }

    async fn is_applied(&self, exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        let out = exec
            .exec(&format!(
                "test -f {REMOTE_SERVER_CERT_PATH} && test -f {REMOTE_SERVER_KEY_PATH} \
                 && test -f {REMOTE_CLIENT_CA_PATH} && echo yes"
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
                "mtls disabled; skipping mTLS asset upload",
            ));
        }
        if ctx.dry_run {
            return Ok(StepOutput::message(format!(
                "dry-run: would upload mTLS server cert/key + client CA → {REMOTE_MTLS_DIR}"
            )));
        }

        let cert_path = require_local_path(
            ctx.mtls_server_cert_path.as_deref(),
            "mtls_server_cert_path",
        )?;
        let key_path =
            require_local_path(ctx.mtls_server_key_path.as_deref(), "mtls_server_key_path")?;
        let ca_path =
            require_local_path(ctx.mtls_client_ca_path.as_deref(), "mtls_client_ca_path")?;

        let cert = read_local_pem(cert_path).await?;
        let key = read_local_pem(key_path).await?;
        let ca = read_local_pem(ca_path).await?;

        // /tmp 스테이징 — write_file은 생성 시점부터 0600이라(로드맵 #84)
        // 이 단계에서 이미 비밀(server.key)이 world-readable로 노출되지 않는다.
        exec.write_file(TMP_CERT, &cert).await?;
        exec.write_file(TMP_KEY, &key).await?;
        exec.write_file(TMP_CA, &ca).await?;

        exec.exec_checked(&format!("sudo mkdir -p {REMOTE_MTLS_DIR}"))
            .await?;
        exec.exec_checked(&format!(
            "sudo mv {TMP_CERT} {REMOTE_SERVER_CERT_PATH} \
             && sudo mv {TMP_KEY} {REMOTE_SERVER_KEY_PATH} \
             && sudo mv {TMP_CA} {REMOTE_CLIENT_CA_PATH} \
             && sudo chmod 644 {REMOTE_SERVER_CERT_PATH} {REMOTE_CLIENT_CA_PATH} \
             && sudo chmod 600 {REMOTE_SERVER_KEY_PATH} \
             && sudo chown root:root {REMOTE_SERVER_CERT_PATH} {REMOTE_SERVER_KEY_PATH} \
                {REMOTE_CLIENT_CA_PATH}"
        ))
        .await?;

        Ok(StepOutput::message(format!(
            "mTLS server cert/key + client CA uploaded → {REMOTE_MTLS_DIR}"
        )))
    }
}

fn require_local_path<'a>(value: Option<&'a str>, field: &str) -> Result<&'a str, StepError> {
    value.filter(|s| !s.is_empty()).ok_or_else(|| {
        StepError::PrereqFailed(format!(
            "mtls_enabled=true requires {field} (local file path — see fleet mtls issue-server)"
        ))
    })
}

async fn read_local_pem(path: &str) -> Result<String, StepError> {
    tokio::fs::read_to_string(path)
        .await
        .map_err(|e| StepError::PrereqFailed(format!("reading mTLS asset '{path}' failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;
    use std::io::Write;

    fn ctx_with_local_paths(cert: &str, key: &str, ca: &str) -> StepContext {
        StepContext {
            worker_name: "w1".into(),
            mtls_enabled: true,
            mtls_server_cert_path: Some(cert.into()),
            mtls_server_key_path: Some(key.into()),
            mtls_client_ca_path: Some(ca.into()),
            ..Default::default()
        }
    }

    fn write_temp_pem(dir: &tempfile::TempDir, name: &str, content: &str) -> String {
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path.to_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn apply_disabled_is_noop() {
        let exec = MockExecutor::new();
        let step = IssueMtlsAssets;
        let ctx = StepContext::default(); // mtls_enabled: false
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("disabled"));
        assert!(exec.recorded_calls().is_empty());
    }

    #[tokio::test]
    async fn dry_run_skips_uploads() {
        let exec = MockExecutor::new();
        let step = IssueMtlsAssets;
        let mut ctx = ctx_with_local_paths("/x/server.pem", "/x/server.key", "/x/ca.pem");
        ctx.dry_run = true;
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("dry-run"));
        assert!(exec.recorded_calls().is_empty());
    }

    #[tokio::test]
    async fn apply_requires_local_cert_path() {
        let exec = MockExecutor::new();
        let step = IssueMtlsAssets;
        let ctx = StepContext {
            mtls_enabled: true,
            ..Default::default()
        };
        let err = step.apply(&exec, &ctx).await.unwrap_err();
        assert!(matches!(err, StepError::PrereqFailed(_)));
        assert!(format!("{err}").contains("mtls_server_cert_path"));
    }

    #[tokio::test]
    async fn apply_uploads_all_three_files_to_fixed_remote_paths() {
        let exec = MockExecutor::new();
        let step = IssueMtlsAssets;
        let tmp = tempfile::tempdir().unwrap();
        let cert = write_temp_pem(&tmp, "server.pem", "CERT-CONTENT");
        let key = write_temp_pem(&tmp, "server.key", "KEY-CONTENT");
        let ca = write_temp_pem(&tmp, "ca.pem", "CA-CONTENT");
        let ctx = ctx_with_local_paths(&cert, &key, &ca);

        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains(REMOTE_MTLS_DIR));

        let calls = exec.recorded_calls();
        assert!(calls
            .iter()
            .any(|c| c.contains("mkdir -p") && c.contains(REMOTE_MTLS_DIR)));
        assert!(calls
            .iter()
            .any(|c| c.contains(&format!("write {TMP_CERT}"))));
        assert!(calls
            .iter()
            .any(|c| c.contains(&format!("write {TMP_KEY}"))));
        assert!(calls.iter().any(|c| c.contains(&format!("write {TMP_CA}"))));
        assert!(calls.iter().any(|c| c.contains(REMOTE_SERVER_CERT_PATH)
            && c.contains(REMOTE_SERVER_KEY_PATH)
            && c.contains(REMOTE_CLIENT_CA_PATH)
            && c.contains("chmod 600")
            && c.contains("chown root:root")));
    }

    #[tokio::test]
    async fn is_applied_when_all_three_remote_files_exist() {
        let exec = MockExecutor::new();
        exec.expect_exec(
            format!(
                "test -f {REMOTE_SERVER_CERT_PATH} && test -f {REMOTE_SERVER_KEY_PATH} \
                 && test -f {REMOTE_CLIENT_CA_PATH}"
            ),
            "yes\n",
        );
        let step = IssueMtlsAssets;
        assert!(step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn is_not_applied_when_files_missing() {
        let exec = MockExecutor::new();
        let step = IssueMtlsAssets;
        assert!(!step.is_applied(&exec).await.unwrap());
    }

    #[test]
    fn step_has_mtls_tag() {
        assert!(IssueMtlsAssets.tags().contains(&"mtls"));
    }
}
