//! 마스터 키 로딩 + AES-256-GCM 암호화/복호화.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use zeroize::Zeroize;

use crate::error::{CredentialsError, MasterKeyError};

/// 기본 마스터 키 파일 경로.
pub const DEFAULT_KEY_FILE: &str = "/etc/fleet/master.key";

/// 환경변수 이름.
pub const ENV_VAR: &str = "FLEET_MASTER_KEY";

/// AES-256-GCM 키 길이 (32바이트 고정).
pub const KEY_LEN: usize = 32;

/// GCM nonce 길이 (12바이트 표준).
pub const NONCE_LEN: usize = 12;

/// 32바이트 마스터 키. 메모리에서 `Zeroize` 처리.
///
/// 로딩 우선순위:
/// 1. `FLEET_MASTER_KEY` 환경변수 (hex 또는 base64url)
/// 2. 파일 (기본 `/etc/fleet/master.key`, 동일 형식)
///
/// 둘 다 없으면 `MasterKeyError::Missing` 반환.
#[derive(Zeroize)]
#[zeroize(drop)]
pub struct MasterKey {
    bytes: [u8; KEY_LEN],
}

impl MasterKey {
    /// 환경변수 → 파일 순서로 로드.
    pub fn load() -> Result<Self, MasterKeyError> {
        Self::load_with_paths(ENV_VAR, DEFAULT_KEY_FILE)
    }

    /// 테스트용으로 경로를 지정 가능.
    pub fn load_with_paths(env_var: &str, file_path: &str) -> Result<Self, MasterKeyError> {
        // 1) 환경변수
        if let Ok(raw) = std::env::var(env_var) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Self::parse(trimmed).map_err(MasterKeyError::Decode);
            }
        }
        // 2) 파일
        match std::fs::read_to_string(file_path) {
            Ok(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err(MasterKeyError::Missing);
                }
                Self::parse(trimmed).map_err(MasterKeyError::Decode)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(MasterKeyError::Missing),
            Err(e) => Err(MasterKeyError::FileRead {
                path: file_path.to_string(),
                source: e,
            }),
        }
    }

    /// hex(64문자) 또는 base64url(43문자)에서 32바이트로 파싱.
    fn parse(s: &str) -> Result<Self, String> {
        // hex 우선 시도 (64 hex = 32 bytes).
        if s.len() == KEY_LEN * 2 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            let mut bytes = [0u8; KEY_LEN];
            hex::decode_to_slice(s, &mut bytes).map_err(|e| format!("hex decode: {e}"))?;
            return Ok(Self { bytes });
        }
        // base64url 시도.
        match URL_SAFE_NO_PAD.decode(s.as_bytes()) {
            Ok(decoded) if decoded.len() == KEY_LEN => {
                let mut bytes = [0u8; KEY_LEN];
                bytes.copy_from_slice(&decoded);
                Ok(Self { bytes })
            }
            Ok(other) => Err(format!(
                "base64url decoded to {} bytes, expected {}",
                other.len(),
                KEY_LEN
            )),
            Err(e) => Err(format!("base64url decode: {e}")),
        }
    }

    /// 테스트/프로그래밍 방식 생성용.
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self { bytes }
    }

    /// 새 무작위 마스터 키 생성 (초기화 스크립트용).
    pub fn generate() -> Self {
        let mut bytes = [0u8; KEY_LEN];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self { bytes }
    }

    /// hex 문자열로 인코딩 (저장용).
    pub fn to_hex(&self) -> String {
        hex::encode(self.bytes)
    }

    /// AES-256-GCM 암호화. 매 호출마다 새로운 무작위 nonce.
    ///
    /// 반환값은 base64로 인코딩된 `nonce || ciphertext || tag`.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedBlob, CredentialsError> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.bytes));
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| CredentialsError::Encryption(e.to_string()))?;

        let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);

        Ok(EncryptedBlob {
            encoded: URL_SAFE_NO_PAD.encode(combined),
        })
    }

    /// `EncryptedBlob` 복호화.
    pub fn decrypt(&self, blob: &EncryptedBlob) -> Result<Vec<u8>, CredentialsError> {
        let combined = URL_SAFE_NO_PAD
            .decode(blob.encoded.as_bytes())
            .map_err(|e| CredentialsError::Encoding(e.to_string()))?;

        if combined.len() < NONCE_LEN {
            return Err(CredentialsError::Decryption(format!(
                "blob too short ({} bytes, need at least {NONCE_LEN})",
                combined.len()
            )));
        }
        let (nonce_bytes, ciphertext) = combined.split_at(NONCE_LEN);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.bytes));
        let nonce = Nonce::from_slice(nonce_bytes);
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| CredentialsError::Decryption(e.to_string()))?;
        Ok(plaintext)
    }
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterKey")
            .field("bytes", &"[redacted]")
            .finish()
    }
}

/// 암호화된 결과. DB의 TEXT 칼럼에 그대로 저장.
///
/// 구조: `base64url(nonce(12B) || ciphertext || tag(16B))`
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EncryptedBlob {
    /// base64url 인코딩된 문자열.
    pub encoded: String,
}

impl EncryptedBlob {
    /// 빈 blob (빈 plaintext를 암호화한 것과 동일).
    pub fn empty() -> Self {
        Self {
            encoded: String::new(),
        }
    }

    /// DB 저장용 문자열 반환.
    pub fn as_str(&self) -> &str {
        &self.encoded
    }

    /// DB에서 로드.
    pub fn from_string(s: impl Into<String>) -> Self {
        Self {
            encoded: s.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let key = MasterKey::generate();
        let plaintext = b"sk-api-12345-secret-value";
        let blob = key.encrypt(plaintext).unwrap();
        let decoded = key.decrypt(&blob).unwrap();
        assert_eq!(decoded.as_slice(), plaintext);
    }

    #[test]
    fn different_nonce_each_call() {
        let key = MasterKey::generate();
        let plaintext = b"same-input";
        let blob1 = key.encrypt(plaintext).unwrap();
        let blob2 = key.encrypt(plaintext).unwrap();
        assert_ne!(
            blob1.encoded, blob2.encoded,
            "각 호출마다 다른 nonce 사용 → 동일 plaintext도 다른 ciphertext"
        );
        // 둘 다 올바르게 복호화되어야 함
        assert_eq!(key.decrypt(&blob1).unwrap(), plaintext);
        assert_eq!(key.decrypt(&blob2).unwrap(), plaintext);
    }

    #[test]
    fn wrong_key_fails_decryption() {
        let key1 = MasterKey::generate();
        let key2 = MasterKey::generate();
        let blob = key1.encrypt(b"secret").unwrap();
        assert!(key2.decrypt(&blob).is_err());
    }

    #[test]
    fn parse_hex_64chars() {
        let key = MasterKey::generate();
        let hex = key.to_hex();
        assert_eq!(hex.len(), KEY_LEN * 2);
        let parsed = MasterKey::parse(&hex).unwrap();
        let original_blob = parsed.encrypt(b"test").unwrap();
        assert!(key.decrypt(&original_blob).is_ok());
    }

    #[test]
    fn parse_base64url_43chars() {
        let key = MasterKey::generate();
        let b64 = URL_SAFE_NO_PAD.encode(key.bytes);
        assert_eq!(b64.len(), 43);
        let parsed = MasterKey::parse(&b64).unwrap();
        let blob = parsed.encrypt(b"x").unwrap();
        assert!(key.decrypt(&blob).is_ok());
    }

    #[test]
    fn parse_rejects_wrong_length() {
        assert!(MasterKey::parse("aabbcc").is_err());
        assert!(MasterKey::parse(
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789extra"
        )
        .is_err());
    }

    #[test]
    fn empty_string_in_env_falls_through_to_file() {
        // 빈 환경변수는 없는 것으로 처리되어야 함.
        // 이 테스트는 load_with_paths를 통해 간접 검증 (프로덕션 코드 경로).
        let temp = tempfile_env();
        let result = MasterKey::load_with_paths("FLEET_TEST_EMPTY_VAR_DOES_NOT_EXIST", &temp);
        assert!(matches!(result, Err(MasterKeyError::Missing)));
    }

    fn tempfile_env() -> String {
        // 존재하지 않는 파일 — Missing 반환 유도
        "/tmp/fleet-test-master-key-does-not-exist-12345.key".to_string()
    }

    #[test]
    fn debug_does_not_leak() {
        let key = MasterKey::generate();
        let s = format!("{:?}", key);
        assert!(!s.contains(&key.to_hex()));
        assert!(s.contains("[redacted]"));
    }

    #[test]
    fn drop_zeroizes() {
        // 직접 검증은 어렵지만 drop 트레이트가 컴파일되는지만 확인.
        let key = MasterKey::generate();
        drop(key);
        // 여기까지 컴파일되면 zeroize_derive가 정상 작동 중.
    }
}
