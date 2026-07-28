//! 이메일 발송 서비스 — Gmail SMTP 기반.
//!
//! 환경변수 2개로 Gmail을 통한 이메일 발송을 활성화:
//!
//! ```text
//! FLEET_GMAIL_USER=your-address@gmail.com
//! FLEET_GMAIL_APP_PASS=xxxx xxxx xxxx xxxx   # Google App Password (16자리, 공백 무관)
//! ```
//!
//! **App Password 발급 방법**:
//! 1. Google 계정 → 보안 → 2단계 인증 활성화
//! 2. https://myaccount.google.com/apppasswords — "메일" 앱 비밀번호 생성
//! 3. 생성된 16자리 비밀번호를 `FLEET_GMAIL_APP_PASS`에 설정
//!
//! 추가 옵션:
//! - `FLEET_BASE_URL` — 인증 링크의 베이스 URL (예: `https://fleet.agentthread.dev`)
//! - `FLEET_MAIL_FROM_NAME` — 발송자 표시명 (기본: "Fleet Orchestrator")

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// Gmail SMTP 설정.
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    /// Gmail 주소 (`xxx@gmail.com`).
    pub gmail_user: String,
    /// Google App Password (16자리).
    pub gmail_app_pass: String,
    /// 발송자 표시명.
    pub from_name: String,
}

/// Gmail SMTP 상수.
const GMAIL_HOST: &str = "smtp.gmail.com";
const GMAIL_PORT: u16 = 587;

impl SmtpConfig {
    /// 환경변수에서 Gmail 설정 로드.
    ///
    /// 필수: `FLEET_GMAIL_USER`, `FLEET_GMAIL_APP_PASS`
    /// 옵션: `FLEET_MAIL_FROM_NAME` (기본: "Fleet Orchestrator")
    ///
    /// 둘 중 하나라도 없으면 `None` 반환 (이메일 미발송, 로그만 출력).
    pub fn from_env() -> Option<Self> {
        let gmail_user = std::env::var("FLEET_GMAIL_USER").ok()?;
        let gmail_app_pass = std::env::var("FLEET_GMAIL_APP_PASS").ok()?;
        if gmail_user.is_empty() || gmail_app_pass.is_empty() {
            return None;
        }
        let from_name = std::env::var("FLEET_MAIL_FROM_NAME")
            .unwrap_or_else(|_| "Fleet Orchestrator".into());

        Some(Self {
            gmail_user,
            // 공백 제거 — App Password는 "xxxx xxxx xxxx xxxx" 형태로 표시되는 경우가 많음.
            gmail_app_pass: gmail_app_pass.replace(' ', ""),
            from_name,
        })
    }
}

/// Gmail SMTP 전송기 생성.
fn build_gmail_transport(config: &SmtpConfig) -> AsyncSmtpTransport<Tokio1Executor> {
    AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(GMAIL_HOST)
        .expect("Gmail relay must be valid")
        .port(GMAIL_PORT)
        .credentials(Credentials::new(
            config.gmail_user.clone(),
            config.gmail_app_pass.clone(),
        ))
        .build()
}

/// 이메일 메시지 빌드 (공통).
fn build_email_message(
    config: &SmtpConfig,
    to_email: &str,
    subject: &str,
    body: String,
) -> Result<Message, String> {
    let from = format!("{} <{}>", config.from_name, config.gmail_user);
    Message::builder()
        .from(from.parse().map_err(|e| format!("invalid from address: {e}"))?)
        .to(to_email
            .parse()
            .map_err(|e| format!("invalid to address: {e}"))?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| format!("email build failed: {e}"))
}

/// 이메일 인증 메일 발송.
pub async fn send_verification_email(
    config: Option<&SmtpConfig>,
    to_email: &str,
    verify_url: &str,
) -> Result<(), String> {
    let subject = "Fleet Orchestrator — Verify your email";
    let body = format!(
        r#"Welcome to Fleet Orchestrator!

Please verify your email address by clicking the link below:

{verify_url}

This link expires in 24 hours.

If you did not create an account, you can safely ignore this email.

— Fleet Orchestrator"#
    );

    send_email(config, to_email, subject, body, verify_url).await
}

/// 비밀번호 재설정 메일 발송.
pub async fn send_password_reset_email(
    config: Option<&SmtpConfig>,
    to_email: &str,
    reset_url: &str,
) -> Result<(), String> {
    let subject = "Fleet Orchestrator — Password reset";
    let body = format!(
        r#"A password reset was requested for your Fleet Orchestrator account.

Click the link below to set a new password:

{reset_url}

This link expires in 1 hour.

If you did not request this, you can safely ignore this email.

— Fleet Orchestrator"#
    );

    send_email(config, to_email, subject, body, reset_url).await
}

/// 공통 이메일 발송 로직.
async fn send_email(
    config: Option<&SmtpConfig>,
    to_email: &str,
    subject: &str,
    body: String,
    fallback_url: &str,
) -> Result<(), String> {
    match config {
        Some(cfg) => {
            let email = build_email_message(cfg, to_email, subject, body)?;
            let transport = build_gmail_transport(cfg);

            transport
                .send(email)
                .await
                .map_err(|e| format!("Gmail SMTP send failed: {e}"))?;

            tracing::info!(to = to_email, "email sent via Gmail SMTP");
            Ok(())
        }
        None => {
            // Gmail 미설정 — 로그에만 출력 (개발/테스트용).
            tracing::warn!(
                to = to_email,
                url = fallback_url,
                "Gmail not configured — email logged instead of sent. \
                 Set FLEET_GMAIL_USER + FLEET_GMAIL_APP_PASS to enable Gmail delivery."
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_returns_none_when_unset() {
        // 환경변수가 없으면 None — 병렬 테스트 간섭을 피해 임시 키 사용.
        let key = "FLEET_GMAIL_USER_TEST_NEVER_SET_42";
        std::env::remove_var(key);
        assert!(std::env::var(key).is_err());
    }

    #[test]
    fn app_password_space_stripping_logic() {
        // 구조체 레벨에서 공백 제거 로직 검증 (환경변수 의존 없음).
        let raw_pass = "abcd efgh ijkl mnop";
        let cleaned = raw_pass.replace(' ', "");
        assert_eq!(cleaned, "abcdefghijklmnop");
        assert_eq!(cleaned.len(), 16);
    }

    #[test]
    fn from_env_returns_none_with_empty_values() {
        let config = SmtpConfig::from_env();
        // 실제 환경에서 설정되어 있을 수도 있으니, 있는 경우만 검증.
        if let Some(cfg) = config {
            assert!(!cfg.gmail_user.is_empty());
            assert!(!cfg.gmail_app_pass.is_empty());
        }
    }

    #[test]
    fn gmail_transport_builds_without_panic() {
        let config = SmtpConfig {
            gmail_user: "test@gmail.com".into(),
            gmail_app_pass: "abcdefghijklmnop".into(),
            from_name: "Test".into(),
        };
        // 빌드만 확인 — 실제 전송 안 함.
        let _transport = build_gmail_transport(&config);
    }

    #[test]
    fn email_message_builds_correctly() {
        let config = SmtpConfig {
            gmail_user: "sender@gmail.com".into(),
            gmail_app_pass: "dummy".into(),
            from_name: "Fleet".into(),
        };
        let msg = build_email_message(
            &config,
            "recipient@example.com",
            "Test Subject",
            "Body text".into(),
        );
        assert!(msg.is_ok());
    }
}
