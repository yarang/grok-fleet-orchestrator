//! `fleet token` 명령 — 부트스트랩 토큰 생성.
//!
//! 워커가 오케스트레이터에 처음 등록할 때 `--api-tokens`에 추가할 수 있는
//! 무작위 bearer 토큰을 생성합니다. CSPRNG 난수를 base64url로 인코딩하여
//! 출력합니다.

use anyhow::{Context, Result};

use crate::TokenAction;

/// `token` 명령 디스패치.
pub async fn run_token(action: TokenAction) -> Result<()> {
    match action {
        TokenAction::New { prefix, bytes } => run_token_new(&prefix, bytes).await,
    }
}

/// `token new` — 무작위 토큰 생성 후 stdout 출력.
async fn run_token_new(prefix: &str, bytes: usize) -> Result<()> {
    if !(8..=256).contains(&bytes) {
        return Err(anyhow::anyhow!(
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
        }
    }
}
