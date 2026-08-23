//! 에러 타입 모음. 각 하위 시스템별로 전용 에러 타입을 두어 호출자가
//! 세분화된 처리(match)를 할 수 있도록 함.

use thiserror::Error;

/// SSH 연결/실행 실패.
#[derive(Debug, Error)]
pub enum SshError {
    #[error("SSH not connected")]
    NotConnected,
    #[error("authentication failed for user '{0}'")]
    AuthFailed(String),
    #[error("SSH key load failed: {0}")]
    KeyLoad(String),
    #[error("host key verification failed for '{host}': {reason}")]
    HostKeyVerification { host: String, reason: String },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[cfg(feature = "ssh")]
    #[error("russh error: {0}")]
    Russh(#[from] russh::Error),
    #[cfg(feature = "ssh")]
    #[error("russh-keys error: {0}")]
    RusshKeys(#[from] russh_keys::Error),
    #[error("SSH protocol error: {0}")]
    Protocol(String),
}

/// Playbook 스텝 실행 실패.
#[derive(Debug, Error)]
pub enum StepError {
    #[error("SSH error: {0}")]
    Ssh(#[from] SshError),
    #[error("unsupported OS: '{0}' (expected ubuntu, debian, rhel, fedora, amzn)")]
    UnsupportedOs(String),
    #[error("prerequisite not met: {0}")]
    PrereqFailed(String),
    #[error("remote command exited with code {code}: {stderr}")]
    RemoteExit { code: i32, stderr: String },
    #[error("template render failed: {0}")]
    Template(String),
    #[error("parse error: {0}")]
    Parse(String),
}

/// 인벤토리 YAML 파싱 실패.
#[derive(Debug, Error)]
pub enum InventoryError {
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("inventory has no workers")]
    Empty,
    #[error("worker '{0}' missing required field 'host'")]
    MissingHost(String),
    #[error("worker '{0}' missing required field 'name'")]
    MissingName(String),
    #[error("defaults missing required field 'ssh_key'")]
    MissingSshKey,
}

/// Playbook orchestration 실패 (여러 스텝/호스트 실패 집계).
///
/// `StepFailed`의 `source: StepError`를 Box로 감싼 이유: StepError가 SshError를
/// 포함하므로(특히 `ssh` 피처의 russh 관련 variant) PlaybookError 자체 크기가 커져
/// clippy::result_large_err(Err variant가 크면 Result를 값으로 전달할 때 스택 복사
/// 비용이 커진다는 경고)를 유발한다. Box로 힙에 두면 PlaybookError 크기가
/// 포인터 하나로 줄어든다.
#[derive(Debug, Error)]
pub enum PlaybookError {
    #[error("step '{step}' failed on '{host}': {source}")]
    StepFailed {
        step: String,
        host: String,
        #[source]
        source: Box<StepError>,
        /// 실패 이전에 완료(Skipped/Applied)되고, 실패한 스텝 자신의 `Failed`
        /// 항목까지 포함한 부분 실행 이력 (로드맵 #79).
        ///
        /// 이전에는 이 정보가 에러에 실리지 않아, 호출자(`fleet-cli`의
        /// `run_playbook` 실패 처리)가 매번 `steps: vec![]`로 리포트를
        /// 만들었다 — 20대 중 7번째가 어느 스텝에서 실패했는지, 그 전에
        /// 무엇까지 성공했는지를 실패 리포트만으로는 알 수 없었다.
        completed_steps: Vec<crate::playbook::StepReport>,
    },
    #[error("all retries exhausted for step '{step}' on '{host}'")]
    RetriesExhausted { step: String, host: String },
}

/// 프로비저닝 최상위 실패.
#[derive(Debug, Error)]
pub enum ProvisionError {
    #[error("SSH error: {0}")]
    Ssh(#[from] SshError),
    #[error("step error: {0}")]
    Step(#[from] StepError),
    #[error("inventory error: {0}")]
    Inventory(#[from] InventoryError),
    #[error("playbook error: {0}")]
    Playbook(#[from] PlaybookError),
    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let e = StepError::UnsupportedOs("freebsd".into());
        assert!(format!("{e}").contains("freebsd"));
        assert!(format!("{e}").contains("ubuntu"));
    }

    #[test]
    fn ssh_protocol_error_constructs() {
        let e = SshError::Protocol("handshake timeout".into());
        assert!(format!("{e}").contains("handshake"));
    }

    #[test]
    fn host_key_verification_error_is_descriptive() {
        let e = SshError::HostKeyVerification {
            host: "10.0.0.5".into(),
            reason: "key mismatch".into(),
        };
        let msg = format!("{e}");
        assert!(msg.contains("10.0.0.5"));
        assert!(msg.contains("key mismatch"));
        assert!(msg.contains("verification failed"));
    }
}
