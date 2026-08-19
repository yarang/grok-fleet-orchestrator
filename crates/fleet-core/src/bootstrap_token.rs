//! 부트스트랩 토큰 도메인 모델 (Phase 8.3).
//!
//! `fleet-worker join` 흐름에서 워커가 최초 등록 시 사용하는 일회성 (또는
//! 제한적 다회용) 토큰. 어드민이 `fleet token issue` 로 발급하고, 워커는
//! 발급받은 토큰을 `POST /v1/workers/join` 에 포함하여 자신을 등록합니다.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 부트스트랩 토큰 엔티티.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapToken {
    /// 원문 토큰의 SHA-256 digest. 원문은 발급 응답에서만 한 번 반환되며 저장하지 않는다.
    pub token_digest: String,
    pub created_at: DateTime<Utc>,
    /// 발급한 어드민 식별자 (옵션).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// 만료 시각. `None`이면 무기한.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// 최대 사용 가능 횟수. 기본 1 (일회성).
    pub max_uses: u32,
    /// 현재까지 사용된 횟수.
    pub use_count: u32,
    /// 어드민 메모 (옵션).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// 마지막으로 사용한 워커 이름.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_by: Option<String>,
    /// 마지막 사용 시각.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
}

impl BootstrapToken {
    /// 원문 token을 대체하는 공개 식별자.
    ///
    /// 목록·감사·회수 API에는 이 값만 노출한다.
    pub fn public_id(&self) -> String {
        format!("bt_{}", self.token_digest)
    }

    /// 원문 token의 공개 식별자를 계산한다.
    pub fn public_id_for(token: &str) -> String {
        format!("bt_{}", Self::digest_for(token))
    }

    /// 원문 토큰으로부터 저장·검색에 사용할 digest를 계산한다.
    pub fn digest_for(token: &str) -> String {
        hex::encode(Sha256::digest(token.as_bytes()))
    }

    /// 공개 식별자에서 digest를 추출한다. 잘못된 식별자는 거부한다.
    pub fn digest_from_public_id(token_id: &str) -> Option<String> {
        let digest = token_id.strip_prefix("bt_")?;
        (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| digest.to_ascii_lowercase())
    }

    /// 현재 사용 가능한지 (만료 안 됨 + use_count < max_uses).
    pub fn is_usable(&self) -> bool {
        if self.use_count >= self.max_uses {
            return false;
        }
        if let Some(exp) = self.expires_at {
            if Utc::now() > exp {
                return false;
            }
        }
        true
    }

    /// 남은 사용 횟수.
    pub fn remaining_uses(&self) -> u32 {
        self.max_uses.saturating_sub(self.use_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(max_uses: u32, use_count: u32, expires_at: Option<DateTime<Utc>>) -> BootstrapToken {
        BootstrapToken {
            token_digest: BootstrapToken::digest_for("fleet_test"),
            created_at: Utc::now(),
            created_by: None,
            expires_at,
            max_uses,
            use_count,
            notes: None,
            last_used_by: None,
            last_used_at: None,
        }
    }

    #[test]
    fn fresh_single_use_is_usable() {
        let t = token(1, 0, None);
        assert!(t.is_usable());
        assert_eq!(t.remaining_uses(), 1);
    }

    #[test]
    fn exhausted_single_use_not_usable() {
        let t = token(1, 1, None);
        assert!(!t.is_usable());
        assert_eq!(t.remaining_uses(), 0);
    }

    #[test]
    fn multi_use_token_usable_until_exhausted() {
        let mut t = token(5, 0, None);
        assert!(t.is_usable());
        assert_eq!(t.remaining_uses(), 5);
        t.use_count = 4;
        assert!(t.is_usable());
        assert_eq!(t.remaining_uses(), 1);
        t.use_count = 5;
        assert!(!t.is_usable());
    }

    #[test]
    fn expired_token_not_usable() {
        let past = Utc::now() - chrono::Duration::seconds(60);
        let t = token(10, 0, Some(past));
        assert!(!t.is_usable());
    }

    #[test]
    fn future_expiration_usable() {
        let future = Utc::now() + chrono::Duration::days(7);
        let t = token(1, 0, Some(future));
        assert!(t.is_usable());
    }

    #[test]
    fn public_id_round_trip_uses_digest_not_raw_token() {
        let raw = "fleet_secret_token";
        let public_id = BootstrapToken::public_id_for(raw);
        assert!(!public_id.contains(raw));
        assert_eq!(
            BootstrapToken::digest_from_public_id(&public_id).as_deref(),
            Some(BootstrapToken::digest_for(raw).as_str())
        );
        assert!(BootstrapToken::digest_from_public_id("bt_not-a-digest").is_none());
    }
}
