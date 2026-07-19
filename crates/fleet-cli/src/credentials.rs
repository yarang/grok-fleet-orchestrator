//! `fleet credentials` 명령 — 워커 API 키 중앙 관리.
//!
//! ## 명령
//!
//! - `fleet credentials init-key` — 마스터 키 생성 (초기 1회).
//! - `fleet credentials set` — 워커에 API 키 저장/회전.
//! - `fleet credentials list` — 워커의 자격 증명 목록 (메타데이터만).
//! - `fleet credentials export` — 복호화된 자격 증명 출력 (프로비저닝용).
//! - `fleet credentials delete` — 자격 증명 제거.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::CredentialsAction;

/// `credentials` 명령 디스패치.
pub async fn run_credentials(action: CredentialsAction) -> Result<()> {
    match action {
        CredentialsAction::InitKey { out } => run_init_key(out.as_deref()).await,
        CredentialsAction::Set {
            api_url,
            api_token,
            worker,
            model_id,
            base_url,
            api_key,
            api_backend,
            context_window,
            model_name,
        } => {
            run_set(SetArgs {
                api_url,
                api_token,
                worker,
                model_id,
                base_url,
                api_key,
                api_backend,
                context_window,
                model_name,
            })
            .await
        }
        CredentialsAction::List {
            api_url,
            api_token,
            worker,
            json,
        } => run_list(&api_url, &api_token, &worker, json).await,
        CredentialsAction::Export {
            api_url,
            api_token,
            worker,
            model_id,
            json,
        } => run_export(&api_url, &api_token, &worker, &model_id, json).await,
        CredentialsAction::Delete {
            api_url,
            api_token,
            worker,
            model_id,
        } => run_delete(&api_url, &api_token, &worker, &model_id).await,
    }
}

/// `credentials init-key` — 마스터 키 생성.
async fn run_init_key(out: Option<&std::path::Path>) -> Result<()> {
    let key = fleet_credentials::MasterKey::generate();
    let hex = key.to_hex();
    match out {
        Some(path) => {
            // 부모 디렉토리 생성.
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("creating {}", parent.display()))?;
                }
            }
            // 0600 권한으로 저장.
            std::fs::write(path, &hex)
                .with_context(|| format!("writing master key to {}", path.display()))?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(path, perms)?;
            eprintln!("master key written to {} (mode 0600)", path.display());
            eprintln!(
                "configure orchestrator: export FLEET_MASTER_KEY=$(cat {})",
                path.display()
            );
        }
        None => {
            // stdout으로 hex 출력.
            println!("{hex}");
            eprintln!("# Set this as FLEET_MASTER_KEY env var or save to /etc/fleet/master.key");
        }
    }
    Ok(())
}

// ── Set ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct SetArgs {
    api_url: String,
    api_token: String,
    worker: String,
    model_id: String,
    base_url: String,
    api_key: Option<String>,
    api_backend: String,
    context_window: u32,
    model_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct PutCredentialRequest {
    model_id: String,
    base_url: String,
    api_key: String,
    api_backend: String,
    context_window: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PutCredentialResponse {
    #[allow(dead_code)]
    status: String,
    worker_name: String,
    model_id: String,
    rotated_at: String,
}

/// `credentials set` — 워커에 자격 증명 저장/회전.
async fn run_set(args: SetArgs) -> Result<()> {
    // API 키 확보: 인자 → 환경변수 → stdin 순서.
    let api_key = match args.api_key {
        Some(k) if !k.is_empty() => k,
        _ => {
            // stdin에서 읽기 (파이프 입력 허용).
            let mut buf = String::new();
            eprintln!("Reading API key from stdin (Ctrl+D to end)...");
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                .context("reading api_key from stdin")?;
            buf.trim().to_string()
        }
    };
    if api_key.is_empty() {
        return Err(anyhow!(
            "api_key is required (pass --api-key, set FLEET_CRED_API_KEY, or pipe via stdin)"
        ));
    }

    let http = build_http_client()?;
    let url = format!(
        "{}/v1/workers/{}/credentials",
        args.api_url.trim_end_matches('/'),
        urlencoding::encode_or_self(&args.worker)
    );
    let body = PutCredentialRequest {
        model_id: args.model_id.clone(),
        base_url: args.base_url.clone(),
        api_key,
        api_backend: args.api_backend.clone(),
        context_window: args.context_window,
        model_name: args.model_name.clone(),
    };
    let resp = http
        .put(&url)
        .bearer_auth(&args.api_token)
        .json(&body)
        .send()
        .await
        .context("credentials set request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("credentials set failed: {status} — {text}"));
    }
    let parsed: PutCredentialResponse =
        resp.json().await.context("parsing set response")?;
    println!(
        "rotated: worker={} model={} rotated_at={}",
        parsed.worker_name, parsed.model_id, parsed.rotated_at
    );
    Ok(())
}

// ── List ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
struct CredentialListItem {
    worker_name: String,
    model_id: String,
    base_url: String,
    api_backend: String,
    context_window: u32,
    model_name: Option<String>,
    created_at: String,
    rotated_at: String,
}

/// `credentials list` — 메타데이터만 출력 (api_key 절대 노출 안 함).
async fn run_list(api_url: &str, api_token: &str, worker: &str, json: bool) -> Result<()> {
    let http = build_http_client()?;
    let url = format!(
        "{}/v1/workers/{}/credentials",
        api_url.trim_end_matches('/'),
        urlencoding::encode_or_self(worker)
    );
    let resp = http
        .get(&url)
        .bearer_auth(api_token)
        .send()
        .await
        .context("credentials list request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("credentials list failed: {status} — {text}"));
    }
    let items: Vec<CredentialListItem> = resp.json().await.context("parsing list response")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }
    if items.is_empty() {
        println!("No credentials set for worker '{worker}'.");
        return Ok(());
    }
    println!(
        "{:<20} {:<15} {:<45} {:<18} {:<23}",
        "MODEL_ID", "API_BACKEND", "BASE_URL", "MODEL_NAME", "ROTATED_AT"
    );
    println!("{}", "-".repeat(125));
    for c in items {
        println!(
            "{:<20} {:<15} {:<45} {:<18} {:<23}",
            c.model_id,
            c.api_backend,
            truncate(&c.base_url, 45),
            truncate(&c.model_name.unwrap_or_else(|| "-".into()), 18),
            &c.rotated_at[..23.min(c.rotated_at.len())],
        );
    }
    Ok(())
}

// ── Export ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
struct ExportedCredentialResponse {
    worker_name: String,
    model_id: String,
    base_url: String,
    api_key: String,
    api_backend: String,
    context_window: u32,
    model_name: Option<String>,
    rotated_at: String,
    grok_config_section: String,
}

/// `credentials export` — 복호화된 자격 증명 + TOML 섹션.
///
/// 기본 모드에서는 grok config 섹션만 stdout으로 출력 (파이프 친화적).
/// `--json`을 주면 전체 객체를 JSON으로 출력.
async fn run_export(
    api_url: &str,
    api_token: &str,
    worker: &str,
    model_id: &str,
    json: bool,
) -> Result<()> {
    let http = build_http_client()?;
    let url = format!(
        "{}/v1/workers/{}/credentials/{}/export",
        api_url.trim_end_matches('/'),
        urlencoding::encode_or_self(worker),
        urlencoding::encode_or_self(model_id)
    );
    let resp = http
        .get(&url)
        .bearer_auth(api_token)
        .send()
        .await
        .context("credentials export request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("credentials export failed: {status} — {text}"));
    }
    let parsed: ExportedCredentialResponse =
        resp.json().await.context("parsing export response")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&parsed)?);
    } else {
        // TOML 섹션만 출력 (grok config에 append 가능).
        print!("{}", parsed.grok_config_section);
    }
    Ok(())
}

// ── Delete ──────────────────────────────────────────────────────────────

/// `credentials delete` — 자격 증명 제거.
async fn run_delete(api_url: &str, api_token: &str, worker: &str, model_id: &str) -> Result<()> {
    let http = build_http_client()?;
    let url = format!(
        "{}/v1/workers/{}/credentials/{}",
        api_url.trim_end_matches('/'),
        urlencoding::encode_or_self(worker),
        urlencoding::encode_or_self(model_id)
    );
    let resp = http
        .delete(&url)
        .bearer_auth(api_token)
        .send()
        .await
        .context("credentials delete request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("credentials delete failed: {status} — {text}"));
    }
    println!("deleted: worker={worker} model={model_id}");
    Ok(())
}

// ── 공용 헬퍼 ──────────────────────────────────────────────────────────

/// 공유 HTTP 클라이언트 (타임아웃 10s).
fn build_http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

// token.rs와 동일한 인라인 urlencoding 모듈 (의존성 추가 방지).
mod urlencoding {
    pub fn encode_or_self(s: &str) -> String {
        if s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_long_string() {
        // max 이상이면 말줄임. s.len() > max 일 때 앞 (max-1) 문자 + "…".
        assert_eq!(truncate("hello world", 5), "hell…"); // 11 > 5 → 앞 4문자 + "…"
        assert_eq!(truncate("ok", 10), "ok"); // 2 ≤ 10 → 그대로
        // "exactly5" 길이 8 > 5 → 앞 4문자 + "…"
        assert_eq!(truncate("exactly5", 5), "exac…");
    }

    #[test]
    fn urlencoding_passes_safe() {
        assert_eq!(urlencoding::encode_or_self("worker-arm1"), "worker-arm1");
        assert_eq!(urlencoding::encode_or_self("grok-build"), "grok-build");
    }

    #[test]
    fn urlencoding_escapes_unsafe() {
        assert_eq!(urlencoding::encode_or_self("a b"), "a%20b");
        assert_eq!(urlencoding::encode_or_self("a/b"), "a%2Fb");
    }
}
