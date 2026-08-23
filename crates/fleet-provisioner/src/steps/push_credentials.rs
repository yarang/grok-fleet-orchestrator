//! Step 5.5 (신규): orchestrator 의 credentials 저장소에서 복호화된 API 키를
//! 받아와 워커의 `/root/.grok/config.toml` 에 병합.
//!
//! ## 동작
//!
//! 1. orchestrator API 호출: `GET /v1/workers/:name/credentials` → 모델 목록.
//! 2. 각 모델에 대해 `GET /v1/workers/:name/credentials/:model/export` 호출 →
//!    복호화된 TOML 섹션 (`[model.<id>]` 블록) 획득.
//! 3. 원격 `/root/.grok/config.toml` 기존 내용을 SSH 로 읽어옴.
//! 4. `[model.*]` 섹션만 교체, `[cli]`, `[ui]` 등 다른 섹션은 보존.
//! 5. 결과 파일을 원격에 atomic write (`/tmp/...` → `mv`).
//!
//! ## 설계 근거
//!
//! - `is_applied()` 는 항상 `false` — credentials 회전이 일어났을 때 재실행만으로
//!   동기화되도록 보장. apply 자체가 cheap (수 회 HTTP + 파일 1회 쓰기).
//! - 빈 credentials 목록은 no-op (기존 파일을 덮어쓰지 않음).
//! - 병합 로직은 line-based parser 로 TOML 파서 의존성 없이 구현.

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{Step, StepContext, StepOutput};

/// grok config.toml 의 기본 원격 경로. fleet-worker.service 가 User=root 이므로
/// `/root/.grok/config.toml` 사용. 향후 worker.toml `[grok] config_path` 로
/// 오버라이드 가능하도록 확장 예정.
const REMOTE_CONFIG_PATH: &str = "/root/.grok/config.toml";

/// PushCredentials 스텝 — orchestrator credentials 저장소를 워커로 동기화.
#[derive(Default)]
pub struct PushCredentials {
    /// HTTP 요청 타임아웃. 기본 10s.
    pub http_timeout: Option<Duration>,
}

#[async_trait]
impl Step for PushCredentials {
    fn name(&self) -> &'static str {
        "push_credentials"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["credentials", "config"]
    }

    async fn is_applied(&self, _exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        // 항상 재실행 — credentials 회전 자동 동기화 보장.
        Ok(false)
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        let api_url = ctx.orchestrator_url.trim_end_matches('/').to_string();

        // dry_run 은 검증 전에 단락 — dry-run 시나리오에서 api_token/url 이
        // 설정되지 않은 경우에도 playbook 전체 시뮬레이션이 가능해야 함.
        if ctx.dry_run {
            let target = if api_url.is_empty() {
                "(orchestrator_url unset)".to_string()
            } else {
                api_url.clone()
            };
            return Ok(StepOutput::message(format!(
                "dry-run: would fetch credentials from {target} and merge into {REMOTE_CONFIG_PATH}"
            )));
        }

        if api_url.is_empty() {
            return Err(StepError::PrereqFailed(
                "orchestrator_url is empty — required for push_credentials step".into(),
            ));
        }
        let api_token = ctx
            .orchestrator_api_token
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                StepError::PrereqFailed(
                    "orchestrator_api_token is required for push_credentials step".into(),
                )
            })?;

        let http = build_http_client(self.http_timeout)?;

        // 1. 모델 목록 조회.
        let summaries = list_credentials(&http, &api_url, api_token, &ctx.worker_name).await?;
        if summaries.is_empty() {
            return Ok(StepOutput::message(format!(
                "no credentials configured for worker '{}'; skipping",
                ctx.worker_name
            )));
        }

        // 2. 각 모델의 복호화된 TOML 섹션 획득.
        let mut sections = Vec::with_capacity(summaries.len());
        for s in &summaries {
            let section =
                export_section(&http, &api_url, api_token, &ctx.worker_name, &s.model_id).await?;
            sections.push(section);
        }

        // 3. 기존 원격 config.toml 읽기 (없으면 빈 문자열).
        // sudo 사용 — config.toml 은 root 소유 0600 이므로 일반 사용자는 읽기 불가.
        let existing = exec
            .exec(&format!(
                "sudo cat {REMOTE_CONFIG_PATH} 2>/dev/null || true"
            ))
            .await?;

        // 4. 병합 — [model.*] 섹션만 교체.
        let merged = merge_config(&existing, &sections);

        // 5. atomic write via /tmp + sudo mv (로드맵 #79 — 실패를 삼키지 않는다).
        exec.write_file("/tmp/grok-config.toml", &merged).await?;
        exec.exec_checked(&format!(
            "sudo mkdir -p $(dirname {REMOTE_CONFIG_PATH}) \
             && sudo mv /tmp/grok-config.toml {REMOTE_CONFIG_PATH} \
             && sudo chmod 600 {REMOTE_CONFIG_PATH}"
        ))
        .await?;

        Ok(StepOutput::message(format!(
            "pushed {} credential section(s) → {REMOTE_CONFIG_PATH}",
            sections.len()
        )))
    }
}

// ── orchestrator API 클라이언트 ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CredentialSummary {
    #[allow(dead_code)]
    worker_name: String,
    model_id: String,
    #[allow(dead_code)]
    base_url: String,
    #[allow(dead_code)]
    api_backend: String,
    #[allow(dead_code)]
    context_window: u32,
    #[allow(dead_code)]
    model_name: Option<String>,
    #[allow(dead_code)]
    created_at: String,
    #[allow(dead_code)]
    rotated_at: String,
}

#[derive(Debug, Deserialize)]
struct ExportResponse {
    #[serde(alias = "grok_config_section")]
    section: String,
}

async fn list_credentials(
    http: &reqwest::Client,
    api_url: &str,
    token: &str,
    worker: &str,
) -> Result<Vec<CredentialSummary>, StepError> {
    let url = format!("{api_url}/v1/workers/{}/credentials", urlencode(worker));
    let resp = http
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| StepError::RemoteExit {
            code: 0,
            stderr: format!("credentials list request failed: {e}"),
        })?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(StepError::RemoteExit {
            code: status.as_u16() as i32,
            stderr: format!("credentials list failed: {status} — {body}"),
        });
    }
    resp.json::<Vec<CredentialSummary>>()
        .await
        .map_err(|e| StepError::RemoteExit {
            code: 0,
            stderr: format!("parsing credentials list response: {e}"),
        })
}

async fn export_section(
    http: &reqwest::Client,
    api_url: &str,
    token: &str,
    worker: &str,
    model_id: &str,
) -> Result<String, StepError> {
    let url = format!(
        "{api_url}/v1/workers/{}/credentials/{}/export",
        urlencode(worker),
        urlencode(model_id)
    );
    let resp = http
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| StepError::RemoteExit {
            code: 0,
            stderr: format!("credentials export request failed ({model_id}): {e}"),
        })?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(StepError::RemoteExit {
            code: status.as_u16() as i32,
            stderr: format!("credentials export failed ({model_id}): {status} — {body}"),
        });
    }
    // ExportResponse 의 어떤 alias 와도 매칭되도록 시도; 실패 시 raw 텍스트로 폴백.
    let text = resp.text().await.map_err(|e| StepError::RemoteExit {
        code: 0,
        stderr: format!("reading export body ({model_id}): {e}"),
    })?;
    if let Ok(parsed) = serde_json::from_str::<ExportResponse>(&text) {
        Ok(parsed.section)
    } else {
        // API가 이미 TOML 섹션 텍스트만 리턴하는 경우 (raw 모드).
        Ok(text)
    }
}

fn build_http_client(timeout: Option<Duration>) -> Result<reqwest::Client, StepError> {
    let mut builder = reqwest::Client::builder();
    if let Some(t) = timeout {
        builder = builder.timeout(t);
    } else {
        builder = builder.timeout(Duration::from_secs(10));
    }
    builder.build().map_err(|e| StepError::RemoteExit {
        code: 0,
        stderr: format!("building http client: {e}"),
    })
}

// ── config.toml 병합 로직 ───────────────────────────────────────────────

/// 기존 config.toml 에서 `[model.*]` 섹션을 제거하고, 주어진 섹션들을 append.
///
/// TOML 파서 의존성 없이 line-based 처리. 섹션 헤더 (`[xxx]`)를 만나면
/// 그 다음 섹션 헤더 또는 파일 끝까지를 그 섹션의 body로 간주.
pub fn merge_config(existing: &str, model_sections: &[String]) -> String {
    let preserved = strip_model_sections(existing);
    let mut out = String::with_capacity(preserved.len() + 256);
    out.push_str(&preserved);
    // preserved 끝에 정확히 하나의 newline 이 오도록 정규화.
    if !out.is_empty() {
        while out.ends_with("\n\n") {
            out.pop();
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n'); // 섹션 사이 빈 줄
    }
    for section in model_sections {
        out.push_str(section);
        if !section.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n'); // 다음 섹션과 빈 줄로 구분.
    }
    // trailing newline 정규화.
    while out.ends_with("\n\n\n") {
        out.pop();
    }
    out
}

/// `[model.*]` 섹션과 그 body 를 전부 제거한 결과 반환.
/// 다른 섹션 (`[cli]`, `[ui]` 등) 과 그 body 는 그대로 보존.
fn strip_model_sections(existing: &str) -> String {
    let mut out = String::with_capacity(existing.len());
    let mut in_model_section = false;
    for line in existing.lines() {
        let trimmed = line.trim();
        let is_header = trimmed.starts_with('[') && trimmed.ends_with(']');
        if is_header {
            in_model_section = trimmed.starts_with("[model.");
        }
        if in_model_section {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// ASCII 안전 문자는 그대로, 나머지는 percent-encode.
/// credentials.rs 의 인라인 urlencoding 과 동일 로직 (의존성 추가 방지).
fn urlencode(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return s.to_string();
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;

    // ── merge_config 단위 테스트 ────────────────────────────────────────

    #[test]
    fn merge_replaces_model_section_preserving_cli_ui() {
        let existing = "[cli]\nauto_update = true\n\n[ui]\nyolo = false\n\n[model.grok-build]\nbase_url = \"old\"\napi_key = \"old-key\"\n";
        let new_section =
            "[model.grok-build]\nbase_url = \"new\"\napi_key = \"new-key\"\n".to_string();
        let merged = merge_config(existing, &[new_section]);
        assert!(merged.contains("[cli]"));
        assert!(merged.contains("auto_update = true"));
        assert!(merged.contains("[ui]"));
        assert!(merged.contains("yolo = false"));
        assert!(merged.contains("[model.grok-build]"));
        assert!(merged.contains("\"new\""));
        assert!(merged.contains("\"new-key\""));
        assert!(!merged.contains("\"old-key\""));
    }

    #[test]
    fn merge_handles_empty_existing() {
        let section = "[model.grok-build]\nbase_url = \"x\"\n".to_string();
        let merged = merge_config("", &[section]);
        assert!(merged.starts_with("[model.grok-build]"));
    }

    #[test]
    fn merge_handles_multiple_model_sections() {
        let existing = "[cli]\nauto = true\n\n[model.a]\nx = 1\n[model.b]\ny = 2\n";
        let sections = vec![
            "[model.a]\nx = 10\n".to_string(),
            "[model.c]\nz = 30\n".to_string(),
        ];
        let merged = merge_config(existing, &sections);
        // [model.a], [model.c] 는 새 것만, [model.b] 는 완전 제거.
        assert!(merged.contains("x = 10"));
        assert!(merged.contains("[model.c]"));
        assert!(merged.contains("z = 30"));
        assert!(!merged.contains("y = 2"));
        assert!(!merged.contains("[model.b]"));
    }

    #[test]
    fn merge_preserves_comments_outside_model_sections() {
        let existing = "# top comment\n[cli]\n# inner\nx = 1\n\n[model.x]\nk = 1\n";
        let merged = merge_config(existing, &[]);
        assert!(merged.contains("# top comment"));
        assert!(merged.contains("[cli]"));
        assert!(merged.contains("# inner"));
        assert!(!merged.contains("[model.x]"));
        assert!(!merged.contains("k = 1"));
    }

    #[test]
    fn merge_preserves_table_arrays() {
        // [[servers]] 같은 table-array 도 헤더 로 시작하므로 그대로 보존.
        let existing = "[[servers]]\nname = \"a\"\n\n[model.x]\nk = 1\n";
        let merged = merge_config(existing, &[]);
        assert!(merged.contains("[[servers]]"));
        assert!(merged.contains("name = \"a\""));
        assert!(!merged.contains("[model.x]"));
    }

    // ── urlencode ──────────────────────────────────────────────────────

    #[test]
    fn urlencode_passes_safe_chars() {
        assert_eq!(urlencode("worker-arm1"), "worker-arm1");
        assert_eq!(urlencode("grok-build"), "grok-build");
        assert_eq!(urlencode("model.v1"), "model.v1");
    }

    #[test]
    fn urlencode_escapes_unsafe() {
        assert_eq!(urlencode("a b"), "a%20b");
        assert_eq!(urlencode("a/b"), "a%2Fb");
    }

    // ── Step trait 통합 테스트 ─────────────────────────────────────────

    #[tokio::test]
    async fn apply_requires_orchestrator_url() {
        let exec = MockExecutor::new();
        let step = PushCredentials::default();
        let ctx = StepContext {
            worker_name: "w1".into(),
            orchestrator_api_token: Some("tok".into()),
            ..Default::default()
        };
        let err = step.apply(&exec, &ctx).await.unwrap_err();
        assert!(matches!(err, StepError::PrereqFailed(_)));
        assert!(format!("{err}").contains("orchestrator_url"));
    }

    #[tokio::test]
    async fn apply_requires_api_token() {
        let exec = MockExecutor::new();
        let step = PushCredentials::default();
        let ctx = StepContext {
            worker_name: "w1".into(),
            orchestrator_url: "https://orch.example.com".into(),
            ..Default::default()
        };
        let err = step.apply(&exec, &ctx).await.unwrap_err();
        assert!(matches!(err, StepError::PrereqFailed(_)));
        assert!(format!("{err}").contains("orchestrator_api_token"));
    }

    #[tokio::test]
    async fn dry_run_skips_api_calls() {
        let exec = MockExecutor::new();
        let step = PushCredentials::default();
        let ctx = StepContext {
            worker_name: "w1".into(),
            orchestrator_url: "https://orch.example.com".into(),
            orchestrator_api_token: Some("tok".into()),
            dry_run: true,
            ..Default::default()
        };
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("dry-run"));
        assert!(exec.recorded_calls().is_empty());
    }

    #[tokio::test]
    async fn is_applied_always_false() {
        let exec = MockExecutor::new();
        let step = PushCredentials::default();
        assert!(!step.is_applied(&exec).await.unwrap());
    }

    #[test]
    fn step_has_credentials_tag() {
        let step = PushCredentials::default();
        assert!(step.tags().contains(&"credentials"));
    }
}
