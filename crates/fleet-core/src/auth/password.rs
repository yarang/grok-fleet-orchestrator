//! 비밀번호 해싱(Argon2id) 및 세션 토큰 생성.
//!
//! ## 매개변수 (OWASP 권장, 2023)
//!
//! - `m` (memory): 19,456 KiB (~19 MiB)
//! - `t` (iterations): 2
//! - `p` (parallelism): 1
//!
//! PHC($P$) 형식 문자열로 저장되며, 검증 시 상수시간 비교(`subtle`)를 사용.

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::RngCore;
use sha2::{Digest, Sha256};

use super::AuthError;

/// OWASP 권장 매개변수로 Argon2id 인스턴스 생성.
fn argon2_instance() -> Argon2<'static> {
    // Argon2::default()가 이미 OWASP에 근접하지만, 명시적으로 설정.
    // m=19456 KiB, t=2, p=1.
    let params = argon2::Params::new(19_456, 2, 1, None).expect("valid argon2 params");
    Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
}

/// 평문 비밀번호를 Argon2id PHC 문자열로 해싱.
pub fn hash_password(plain: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = argon2_instance();
    let hash = argon2
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| AuthError::HashFailed(e.to_string()))?;
    Ok(hash.to_string())
}

/// 평문 비밀번호를 PHC 문자열과 상수시간으로 비교.
///
/// 내부적으로 `Argon2::verify_password`가 상수시간 비교를 수행.
/// PHC 문자열 파싱 실패(잘못된 형식) 시에도 동일한 타이밍이 되도록
/// 미리 더미 연산을 수행하지는 않음 — PHC 파싱 실패는 DB 손상이므로
/// 명백한 에러로 처리.
pub fn verify_password(plain: &str, phc: &str) -> Result<bool, AuthError> {
    let parsed = PasswordHash::new(phc).map_err(|e| AuthError::HashParseFailed(e.to_string()))?;
    let argon2 = argon2_instance();
    Ok(argon2.verify_password(plain.as_bytes(), &parsed).is_ok())
}

/// 32바이트 난수 세션 토큰 생성 + SHA-256 해시.
///
/// 반환: `(token, hash_hex)`
/// - `token`: base64url-no-pad, 쿠키에 설정
/// - `hash_hex`: SHA-256 of `token` (hex), DB에 저장
///
/// DB 노출 시에도 토큰 재현 불가.
pub fn generate_session_token() -> (String, String) {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let hash = sha256_hex(token.as_bytes());
    (token, hash)
}

/// 바이트 → SHA-256 hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// 상수시간 문자열 비교 (session token hash 검증용).
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    subtle::ConstantTimeEq::ct_eq(a.as_bytes(), b.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let plain = "correct horse battery staple";
        let phc = hash_password(plain).unwrap();
        assert!(phc.starts_with("$argon2id$"));
        assert!(verify_password(plain, &phc).unwrap());
    }

    #[test]
    fn verify_rejects_wrong_password() {
        let phc = hash_password("correct horse battery staple").unwrap();
        assert!(!verify_password("wrong password", &phc).unwrap());
    }

    #[test]
    fn each_hash_is_unique_due_to_salt() {
        let a = hash_password("same password").unwrap();
        let b = hash_password("same password").unwrap();
        assert_ne!(a, b, "salt must differ");
    }

    #[test]
    fn invalid_phc_returns_error() {
        let result = verify_password("x", "not-a-valid-phc");
        assert!(matches!(result, Err(AuthError::HashParseFailed(_))));
    }

    #[test]
    fn session_token_is_43_chars_base64url() {
        let (token, hash) = generate_session_token();
        // 32 bytes base64url-no-pad = 43 chars.
        assert_eq!(token.len(), 43);
        // SHA-256 hex = 64 chars.
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sha256_hex_known_vector() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn constant_time_eq_matching() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd")); // different length
    }
}
