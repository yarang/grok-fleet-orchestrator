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
#[derive(Debug, Error)]
pub enum PlaybookError {
    #[error("step '{step}' failed on '{host}': {source}")]
    StepFailed {
        step: String,
        host: String,
        #[source]
        source: StepError,
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
}
