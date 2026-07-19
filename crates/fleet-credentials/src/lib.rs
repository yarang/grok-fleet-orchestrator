//! # fleet-credentials
//!
//! 워커 API 키(credentials)의 암호화/복호화 + 마스터 키 관리.
//!
//! ## 목적
//!
//! 오케스트레이터가 모든 워커의 API 키를 중앙 집중적으로 관리.
//! Postgres `worker_credentials` 테이블에는 암호화된 바이트를 저장하고,
//! 실제 사용(프로비저닝, 회전) 시에만 이 크레이트로 복호화.
//!
//! ## 암호화 스킴
//!
//! * 알고리즘: AES-256-GCM (96-bit nonce, 128-bit tag)
//! * 키 도출: 마스터 키(32바이트)를 그대로 AES 키로 사용
//! * 저장 형식: `nonce(12B) || ciphertext || tag(16B)` (base64 인코딩)
//! * 각 credential마다 새 nonce 사용 → 동일 키여도 동일 plaintext가 다르게 암호화됨
//!
//! ## 마스터 키 로딩 우선순위
//!
//! 1. 환경변수 `FLEET_MASTER_KEY` (hex 또는 base64url, 32바이트)
//! 2. 파일 `/etc/fleet/master.key` (동일 형식)
//! 3. 둘 다 없으면 `MasterKeyError::Missing` → 오케스트레이터는 시작 거부
//!
//! ## 예시
//!
//! ```no_run
//! # use fleet_credentials::{MasterKey, EncryptedBlob};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let key = MasterKey::load()?;
//! let blob = key.encrypt(b"sk-api-12345")?;
//! let plaintext = key.decrypt(&blob)?;
//! assert_eq!(plaintext, b"sk-api-12345");
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![allow(missing_docs)]

mod error;
mod master_key;

pub use error::{CredentialsError, MasterKeyError};
pub use master_key::{EncryptedBlob, MasterKey, DEFAULT_KEY_FILE, ENV_VAR, KEY_LEN, NONCE_LEN};

use serde::{Deserialize, Serialize};

/// 워커별 credentials 레코드. DB에서 로드된 후 복호화하여 사용.
///
/// `api_key` 외에 base_url, model, api_backend, context_window 등
/// grok config.toml의 `[model.X]` 섹션 전체를 캡처.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerCredentials {
    /// 워커 이름 (DB FK).
    pub worker_name: String,
    /// 모델 ID (예: `grok-build`, `gllm-5`).
    /// grok config의 `[model.<id>]` 키가 됨.
    pub model_id: String,
    /// API 엔드포인트 base URL.
    pub base_url: String,
    /// API 키 (평문 — 이미 복호화된 상태).
    #[serde(skip_serializing)]
    pub api_key: String,
    /// `chat_completions` 또는 `responses`.
    #[serde(default = "default_api_backend")]
    pub api_backend: String,
    /// 컨텍스트 윈도우 (토큰 수).
    #[serde(default = "default_context_window")]
    pub context_window: u32,
    /// 모델 이름 (예: `GLM-5.1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 마지막 회전 일시 (감사용).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotated_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn default_api_backend() -> String {
    "chat_completions".to_string()
}

fn default_context_window() -> u32 {
    200_000
}

impl WorkerCredentials {
    /// `~/.grok/config.toml`에 들어갈 TOML 섹션 생성.
    ///
    /// ```toml
    /// [model.grok-build]
    /// base_url = "https://api.z.ai/..."
    /// api_key = "..."
    /// model = "GLM-5.1"
    /// api_backend = "chat_completions"
    /// context_window = 200000
    /// ```
    pub fn render_grok_config_section(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("[model.{}]\n", self.model_id));
        s.push_str(&format!("base_url = {}\n", toml_escape(&self.base_url)));
        s.push_str(&format!("api_key = {}\n", toml_escape(&self.api_key)));
        if let Some(m) = &self.model {
            s.push_str(&format!("model = {}\n", toml_escape(m)));
        }
        s.push_str(&format!(
            "api_backend = {}\n",
            toml_escape(&self.api_backend)
        ));
        s.push_str(&format!("context_window = {}\n", self.context_window));
        s
    }
}

/// TOML basic string escape (따옴표 escape만 처리).
fn toml_escape(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_section_basic() {
        let cred = WorkerCredentials {
            worker_name: "worker-arm1".into(),
            model_id: "grok-build".into(),
            base_url: "https://api.z.ai/api/coding/paas/v4".into(),
            api_key: "sk-12345".into(),
            api_backend: "chat_completions".into(),
            context_window: 200_000,
            model: Some("GLM-5.1".into()),
            rotated_at: None,
        };
        let toml = cred.render_grok_config_section();
        assert!(toml.contains("[model.grok-build]"));
        assert!(toml.contains("base_url = \"https://api.z.ai/api/coding/paas/v4\""));
        assert!(toml.contains("api_key = \"sk-12345\""));
        assert!(toml.contains("api_backend = \"chat_completions\""));
        assert!(toml.contains("context_window = 200000"));
    }

    #[test]
    fn api_key_is_skipped_in_serialize() {
        let cred = WorkerCredentials {
            worker_name: "w".into(),
            model_id: "m".into(),
            base_url: "u".into(),
            api_key: "secret-value".into(),
            api_backend: "chat_completions".into(),
            context_window: 1,
            model: None,
            rotated_at: None,
        };
        let json = serde_json::to_string(&cred).unwrap();
        assert!(!json.contains("secret-value"));
        assert!(!json.contains("api_key"));
    }

    #[test]
    fn toml_escape_quotes() {
        assert_eq!(toml_escape("a\"b"), "\"a\\\"b\"");
        assert_eq!(toml_escape("a\\b"), "\"a\\\\b\"");
        assert_eq!(toml_escape("plain"), "\"plain\"");
    }
}
