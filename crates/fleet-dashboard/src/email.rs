//! 이메일 발송 서비스.
//!
//! SMTP 서버가 설정된 경우 실제 이메일을 발송하고,
//! 설정되지 않은 경우 토큰을 로그에 출력 (개발/테스트용).

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// SMTP 설정.
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from_email: String,
    pub from_name: String,
}

impl SmtpConfig {
    /// 환경변수에서 SMTP 설정 로드.
    /// 필수: `FLEET_SMTP_HOST`, `FLEET_SMTP_USER`, `FLEET_SMTP_PASS`
    /// 옵션: `FLEET_SMTP_PORT` (기본 587), `FLEET_SMTP_FROM` (기본 noreply@host)
    pub fn from_env() -> Option<Self> {
        let host = std::env::var("FLEET_SMTP_HOST").ok()?;
        let username = std::env::var("FLEET_SMTP_USER")
            .unwrap_or_else(|_| "noreply".into());
        let password = std::env::var("FLEET_SMTP_PASS").ok()?;
        let port = std::env::var("FLEET_SMTP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(587);
        let from_email = std::env::var("FLEET_SMTP_FROM")
            .unwrap_or_else(|_| format!("noreply@{}", &host));
        let from_name = std::env::var("FLEET_SMTP_FROM_NAME")
            .unwrap_or_else(|_| "Fleet Orchestrator".into());

        Some(Self {
            host,
            port,
            username,
            password,
            from_email,
            from_name,
        })
    }
}

/// 이메일 발송.
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

    match config {
        Some(cfg) => {
            let email = Message::builder()
                .from(
                    format!("{} <{}>", cfg.from_name, cfg.from_email)
                        .parse()
                        .map_err(|e| format!("invalid from address: {e}"))?,
                )
                .to(to_email
                    .parse()
                    .map_err(|e| format!("invalid to address: {e}"))?)
                .subject(subject)
                .header(ContentType::TEXT_PLAIN)
                .body(body)
                .map_err(|e| format!("email build failed: {e}"))?;

            let transport = AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)
                .map_err(|e| format!("SMTP relay build failed: {e}"))?
                .port(cfg.port)
                .credentials(Credentials::new(
                    cfg.username.clone(),
                    cfg.password.clone(),
                ))
                .build();

            transport
                .send(email)
                .await
                .map_err(|e| format!("SMTP send failed: {e}"))?;

            tracing::info!(to = to_email, "verification email sent via SMTP");
        }
        None => {
            // SMTP 미설정 — 로그에만 출력.
            tracing::warn!(
                to = to_email,
                verify_url = verify_url,
                "SMTP not configured — verification email logged instead of sent. \
                 Set FLEET_SMTP_HOST/USER/PASS to enable email delivery."
            );
        }
    }

    Ok(())
}
