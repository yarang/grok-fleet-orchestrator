//! 인증 보조 유틸 — SHA-256 hex 계산 + CSRF 토큰 생성.

use sha2::{Digest, Sha256};

/// 바이트 → SHA-256 hex (64 chars).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// CSRF 토큰 생성 (32바이트 난수, base64url-no-pad).
///
/// 더블 서밋 쿠키 패턴: 동일한 값을 쿠키와 폼 필드/헤더에 각각 전송하여
/// 서버에서 일치 여부를 상수시간으로 검증.
pub fn generate_csrf_token() -> String {
    let mut bytes = [0u8; 32];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    URL_SAFE_NO_PAD.encode(bytes)
}

/// CSRF 토큰 상수시간 비교.
pub fn csrf_tokens_match(a: &str, b: &str) -> bool {
    subtle::ConstantTimeEq::ct_eq(a.as_bytes(), b.as_bytes()).into()
}
