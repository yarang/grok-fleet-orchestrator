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

/// 더미 검증용 유효한 Argon2id PHC 문자열.
///
/// 사용자가 존재하지 않거나 비활성일 때, 실제 검증과 **동일한 CPU 시간**을
/// 소모하도록 이 PHC에 대해 Argon2 연산을 수행. 그래야 응답 시간만 측정해서
/// 사용자 존재 여부를 추론하는 타이밍 공격을 차단할 수 있음.
///
/// `hash_password("dummy-timing-equalizer-9f2a")`로 생성한 고정값 —
/// 이 값 자체는 어떤 실제 비밀번호와도 매칭될 필요가 없으며, 오직
/// `verify_password`가 PHC 파싱에서 즉시 실패하지 않고 전체 Argon2
/// 연산(m=19456, t=2)을 실행하도록 보장하는 것이 목적.
const DUMMY_PASSWORD_PHC: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$E76aMSRpoa8HmBR/GWu6IQ$09lh/eBSSkI+jGTVukwyu428MoJGlLn8TFhAWmcRoXM";

/// 더미 비밀번호 검증 — 사용자가 없을 때 호출하여 타이밍을 평등화.
///
/// 반환값은 항상 `false` (실제 인증에 사용하지 않음).
/// 오직 실제 로그인 경로와 동일한 시간 소모를 만들기 위해 존재.
pub fn verify_password_dummy(plain: &str) -> bool {
    verify_password(plain, DUMMY_PASSWORD_PHC).unwrap_or(false)
}

/// 평문 비밀번호를 PHC 문자열과 상수시간으로 비교.
///
/// 내부적으로 `Argon2::verify_password`가 상수시간 비교를 수행.
/// PHC 문자열 파싱 실패(잘못된 형식) 시 에러 반환 — 호출자는
/// 더미 경로가 필요하면 `verify_password_dummy`를 사용할 것.
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

/// 비밀번호 정책 검증 (M7+M8).
///
/// - **최소 8자**: 기본 보안 요구사항.
/// - **최대 128자**: Argon2 DoS 방지 (초장문 비밀번호로 메모리/CPU 고갈).
/// - **zxcvbn 강도 점수 ≥ 3**: 사전 공격에 취약한 비밀번호 차단.
///
/// `user_inputs`에는 사용자명, 이메일 등 비밀번호에 포함되면 안 되는
/// 개인 식별 정보를 전달하면 zxcvbn이 추가로 검사.
pub fn validate_password(password: &str, user_inputs: &[&str]) -> Result<(), AuthError> {
    // 길이 검증.
    if password.len() < 8 || password.len() > 128 {
        return Err(AuthError::WeakPassword);
    }
    // zxcvbn 강도 검증 (score 0-4, 3 이상 요구 = "강력").
    let estimate = zxcvbn::zxcvbn(password, user_inputs);
    let score: u8 = estimate.score().into();
    if score < 3 {
        return Err(AuthError::WeakPassword);
    }
    Ok(())
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

    #[test]
    fn validate_password_rejects_short() {
        // 7자 — 최소 8자 미만.
        assert!(validate_password("Ab1!xyz", &[]).is_err());
    }

    #[test]
    fn validate_password_rejects_weak() {
        // 12자지만 사전 단어 + 숫자 패턴 — zxcvbn score < 3.
        assert!(validate_password("password12345", &[]).is_err());
    }

    #[test]
    fn validate_password_accepts_strong() {
        // 충분한 엔트로피.
        assert!(validate_password("K7$mRq!vN2pL#wZx", &[]).is_ok());
    }

    #[test]
    fn validate_password_rejects_too_long() {
        // 129자 — 최대 128자 초과.
        let long = "a".repeat(129);
        assert!(validate_password(&long, &[]).is_err());
    }

    #[test]
    fn validate_password_checks_user_inputs() {
        // 비밀번호에 사용자명 포함 → zxcvbn이 user_input으로 감지.
        assert!(validate_password("admin_admin_99", &["admin"]).is_err());
    }
}
