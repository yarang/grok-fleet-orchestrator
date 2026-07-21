//! 인증 보조 유틸 — SHA-256 hex 계산.

use sha2::{Digest, Sha256};

/// 바이트 → SHA-256 hex (64 chars).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
