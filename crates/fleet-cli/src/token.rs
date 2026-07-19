//! `fleet token` 명령 — 부트스트랩 토큰 생성/발급/조회/회수.
//!
//! ## 명령
//!
//! - `fleet token new` — 로컬 무작위 토큰 생성 (DB 미사용). `--api-tokens`에
//!   수동으로 추가.
//! - `fleet token issue` — orchestrator DB에 토큰을 영속 발급. Phase 8.3.
//! - `fleet token list` — 발급된 토큰 목록.
//! - `fleet token revoke TOKEN` — 토큰 회수.
//!
//! `token new`는 이전 버전 호환용으로 유지. 신규 배포에서는 `token issue` 권장.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::TokenAction;

/// `token` 명령 디스패치.
pub async fn run_token(action: TokenAction) -> Result<()> {
    match action {
        TokenAction::New { prefix, bytes } => run_token_new(&prefix, bytes).await,
        TokenAction::Issue {
            api_url,
            api_token,
            prefix,
            bytes,
            max_uses,
            expires_in_secs,
            created_by,
            notes,
        } => {
            run_token_issue(IssueArgs {
                api_url,
                api_token,
                prefix,
                bytes,
                max_uses,
                expires_in_secs,
                created_by,
                notes,
            })
            .await
        }
        TokenAction::List { api_url, api_token, json } => {
            run_token_list(&api_url, &api_token, json).await
        }
        TokenAction::Revoke { api_url, api_token, token } => {
            run_token_revoke(&api_url, &api_token, &token).await
        }
    }
}

/// `token new` — 무작위 토큰 생성 후 stdout 출력.
async fn run_token_new(prefix: &str, bytes: usize) -> Result<()> {
    if !(8..=256).contains(&bytes) {
        return Err(anyhow!(
            "--bytes must be between 8 and 256 (got {bytes})"
        ));
    }
    let raw = generate_random_bytes(bytes)?;
    let encoded = base64url(&raw);
    let token = if prefix.is_empty() {
        encoded
    } else {
        format!("{prefix}_{encoded}")
    };
    println!("{token}");
    Ok(())
}

// ── Phase 8.3: stateful token management via orchestrator API ──────────

#[derive(Debug, Serialize)]
struct CreateTokenApiRequest {
    prefix: String,
    bytes: usize,
    max_uses: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_in_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateTokenApiResponse {
    token: String,
    #[allow(dead_code)]
    created_at: String,
    #[allow(dead_code)]
    expires_at: Option<String>,
    #[allow(dead_code)]
    max_uses: u32,
}

#[derive(Debug, Deserialize, Serialize)]
struct TokenListItem {
    token: String,
    created_at: String,
    expires_at: Option<String>,
    max_uses: u32,
    use_count: u32,
    remaining_uses: u32,
    notes: Option<String>,
    last_used_by: Option<String>,
    last_used_at: Option<String>,
}

/// `token issue` 인자.
struct IssueArgs {
    api_url: String,
    api_token: String,
    prefix: String,
    bytes: usize,
    max_uses: u32,
    expires_in_secs: Option<u64>,
    created_by: Option<String>,
    notes: Option<String>,
}

/// `token issue` — orchestrator에 토큰 발급 요청.
async fn run_token_issue(args: IssueArgs) -> Result<()> {
    let http = build_http_client()?;
    let url = format!("{}/v1/bootstrap-tokens", args.api_url.trim_end_matches('/'));
    let body = CreateTokenApiRequest {
        prefix: args.prefix.clone(),
        bytes: args.bytes,
        max_uses: args.max_uses,
        expires_in_secs: args.expires_in_secs,
        created_by: args.created_by.clone(),
        notes: args.notes.clone(),
    };
    let resp = http
        .post(&url)
        .bearer_auth(&args.api_token)
        .json(&body)
        .send()
        .await
        .context("token issue request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("token issue failed: {status} — {text}"));
    }
    let parsed: CreateTokenApiResponse = resp.json().await.context("parsing issue response")?;
    println!("{}", parsed.token);
    Ok(())
}

/// `token list` — 발급된 토큰을 테이블 형태로 출력.
async fn run_token_list(api_url: &str, api_token: &str, json: bool) -> Result<()> {
    let http = build_http_client()?;
    let url = format!("{}/v1/bootstrap-tokens", api_url.trim_end_matches('/'));
    let resp = http.get(&url).bearer_auth(api_token).send().await
        .context("token list request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("token list failed: {status} — {text}"));
    }
    let items: Vec<TokenListItem> = resp.json().await.context("parsing list response")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    if items.is_empty() {
        println!("No bootstrap tokens issued.");
        return Ok(());
    }

    println!(
        "{:<40} {:<23} {:<13} {:<11} {:<20}",
        "TOKEN", "CREATED", "USES", "REMAINING", "LAST_USED_BY"
    );
    println!("{}", "-".repeat(112));
    for t in items {
        let truncated = if t.token.len() > 38 {
            format!("{}…{}", &t.token[..24], &t.token[t.token.len() - 12..])
        } else {
            t.token.clone()
        };
        println!(
            "{:<40} {:<23} {}/{}        {:<11} {:<20}",
            truncated,
            &t.created_at[..23.min(t.created_at.len())],
            t.use_count,
            t.max_uses,
            t.remaining_uses,
            t.last_used_by.unwrap_or_else(|| "-".into()),
        );
    }
    Ok(())
}

/// `token revoke TOKEN` — 토큰 회수.
async fn run_token_revoke(api_url: &str, api_token: &str, token: &str) -> Result<()> {
    let http = build_http_client()?;
    let url = format!(
        "{}/v1/bootstrap-tokens/{}",
        api_url.trim_end_matches('/'),
        urlencoding::encode_or_self(token)
    );
    let resp = http.delete(&url).bearer_auth(api_token).send().await
        .context("token revoke request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("token revoke failed: {status} — {text}"));
    }
    println!("revoked: {token}");
    Ok(())
}

/// 공유 HTTP 클라이언트 (타임아웃 10s).
fn build_http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?)
}

// `urlencoding` crate 의존성 추가 없이 안전한 경로 인코딩을 제공하기 위한 폴백.
mod urlencoding {
    pub fn encode_or_self(s: &str) -> String {
        // 부트스트랩 토큰은 알파벳/숫자/-/_ 만 포함하므로 인코딩이 불필요.
        // 혹시 몰라 percent-encoding을 적용.
        if s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
            s.to_string()
        } else {
            percent_encode(s)
        }
    }

    fn percent_encode(s: &str) -> String {
        let mut out = String::with_capacity(s.len() * 3);
        for b in s.as_bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                out.push(*b as char);
            } else {
                out.push_str(&format!("%{:02X}", b));
            }
        }
        out
    }
}

/// 운영체제 CSPRNG에서 `n` 바이트 읽기.
fn generate_random_bytes(n: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    #[cfg(unix)]
    {
        let mut f = std::fs::File::open("/dev/urandom")
            .context("failed to open /dev/urandom")?;
        f.read_exact(&mut buf).context("/dev/urandom read failed")?;
    }
    #[cfg(not(unix))]
    {
        // Windows 등 폴백: uuid v4 두 개를 반복 사용.
        let mut filled = 0;
        while filled < n {
            let id = uuid::Uuid::new_v4();
            let b = id.as_bytes();
            let take = (n - filled).min(b.len());
            buf[filled..filled + take].copy_from_slice(&b[..take]);
            filled += take;
        }
    }
    Ok(buf)
}

/// base64url-no-pad 인코딩 (의존성 추가 없이 직접 구현).
fn base64url(input: &[u8]) -> String {
    const ALPHA: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((input.len() * 4).div_ceil(3));
    let mut chunks = input.chunks_exact(3);
    for c in &mut chunks {
        let n = ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | (c[2] as u32);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(n & 0x3F) as usize] as char);
    }
    let rem = chunks.remainder();
    match rem.len() {
        1 => {
            let n = (rem[0] as u32) << 16;
            out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        }
        2 => {
            let n = ((rem[0] as u32) << 16) | ((rem[1] as u32) << 8);
            out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn base64url_no_padding() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64url_alphabet_uses_dash_underscore() {
        // 0xfc 0x00 → 인코딩에 '-' 또는 '_'가 등장해야 함.
        let s = base64url(&[0xfc, 0x00]);
        assert!(s.contains('-') || s.contains('_'), "got: {s}");
        // 표준 base64였으면 '+' 또는 '/'여야 함.
        assert!(!s.contains('+'));
        assert!(!s.contains('/'));
    }

    #[test]
    fn random_bytes_correct_length() {
        let v = generate_random_bytes(32).unwrap();
        assert_eq!(v.len(), 32);
    }

    #[test]
    fn random_bytes_are_not_all_zero() {
        // 매우 드물지만 CSPRNG가 고장나지 않은 이상 0이 아닌 값이 있어야 함.
        let v = generate_random_bytes(32).unwrap();
        let nonzero = v.iter().filter(|b| **b != 0).count();
        assert!(nonzero > 20, "expected mostly non-zero bytes, got {nonzero}/32");
    }

    #[tokio::test]
    async fn token_new_outputs_prefixed_string() {
        let act = TokenAction::New {
            prefix: "fleet".into(),
            bytes: 16,
        };
        // stdout을 직접 캡처하지 않고, 함수 자체가 Ok인지만 검증.
        // (rust nightly의 io::stdout 캡처 없이, 단순 성공 여부로 검증.)
        // 별도 검증은 base64url + random_bytes 테스트로 분리됨.
        let result = run_token(act).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn token_new_rejects_too_short() {
        let act = TokenAction::New {
            prefix: "x".into(),
            bytes: 4,
        };
        let result = run_token(act).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("must be between"), "unexpected error: {msg}");
    }

    /// CLI 파싱이 올바르게 동작하는지 검증.
    #[test]
    fn cli_parses_token_new() {
        #[derive(Debug, Parser)]
        struct T {
            #[command(subcommand)]
            cmd: TokenAction,
        }
        let t: T = Parser::try_parse_from(["token", "new", "--prefix", "p", "--bytes", "24"])
            .unwrap();
        match t.cmd {
            TokenAction::New { prefix, bytes } => {
                assert_eq!(prefix, "p");
                assert_eq!(bytes, 24);
            }
            _ => panic!("expected TokenAction::New"),
        }
    }
}
