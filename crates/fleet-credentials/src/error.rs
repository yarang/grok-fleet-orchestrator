//! Credentials 계층 에러 타입.

use thiserror::Error;

/// `fleet-credentials` 작업 중 발생하는 에러.
#[derive(Debug, Error)]
pub enum CredentialsError {
    /// 암호화 실패 (AES-GCM).
    #[error("encryption failed: {0}")]
    Encryption(String),

    /// 복호화 실패 — nonce/tag/키 불일치.
    #[error("decryption failed: {0}")]
    Decryption(String),

    /// 직렬화/역직렬화 실패.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// base64/hex 디코딩 실패.
    #[error("encoding error: {0}")]
    Encoding(String),
}

/// 마스터 키 로딩 관련 에러.
#[derive(Debug, Error)]
pub enum MasterKeyError {
    /// `FLEET_MASTER_KEY` env도 없고 `/etc/fleet/master.key` 파일도 없음.
    #[error(
        "master key not found — set FLEET_MASTER_KEY env or create /etc/fleet/master.key (32 bytes hex or base64url)"
    )]
    Missing,

    /// 파일은 존재하지만 읽기 실패.
    #[error("failed to read master key file {path}: {source}")]
    FileRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// 키가 32바이트가 아님.
    #[error("master key must be exactly 32 bytes (got {got} bytes)")]
    InvalidLength { got: usize },

    /// 디코딩 실패.
    #[error("master key is not valid hex or base64url: {0}")]
    Decode(String),

    /// I/O 에러 래핑.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
