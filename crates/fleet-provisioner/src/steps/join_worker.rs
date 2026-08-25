//! Step 5.5: 발급받은 bootstrap token으로 원격 `fleet-worker join`을 실행해
//! `operational_token` 기반 worker.toml을 원격에 직접 기록 (로드맵 `#82`).
//!
//! ## 배경
//!
//! `InstallFleetWorker`는 원래 `templates::render_worker_config`로 로컬에서
//! worker.toml을 렌더링했으나, 그 렌더러는 `fleet-worker`가 fail-closed로
//! 거부하는 legacy `[worker] bootstrap_token`을 썼다 — 이렇게 만들어진 워커는
//! 기동하지 못했다(`docs/deployment/worker-provisioning.md` "전환 전" 절 참고).
//! `/v1/workers/join` 응답을 올바르게 렌더링하는 쪽은 오케스트레이터의
//! `fleet-api::handlers::render_worker_config_toml` 뿐이므로, 프로비저너가
//! 그 로직을 로컬에서 중복 구현하는 대신 원격 호스트 자신이
//! `fleet-worker join`을 실행해 그 응답을 직접 디스크에 쓰게 한다.
//!
//! ## 흐름
//!
//! 1. 오케스트레이터에 `max_uses: 1`, 짧은 TTL의 bootstrap token 발급 요청
//!    (`POST /v1/bootstrap-tokens`, `ctx.orchestrator_api_token`으로 인증 —
//!    `token:issue` capability 필요).
//! 2. 원격 채널에서 `fleet-worker join --token-file - ...`를 exec하고,
//!    발급받은 토큰을 stdin으로만 전달한다(`exec_with_stdin`) — 커맨드라인
//!    인자·환경변수·디스크 파일 어디에도 원문이 남지 않는다.
//! 3. 성공하면 원격 `/etc/fleet/worker.toml`에 `operational_token`이 오케스트레이터
//!    응답 그대로 기록된 상태가 되고, 이 스텝은 `chmod 600`만 보정한다.
//!
//! `operational_token`은 대상 호스트가 orchestrator 응답으로 스스로 기록하며,
//! 프로비저너(이 스텝을 실행하는 CLI 프로세스)는 그 값을 보지도 저장하지도 않는다.
//!
//! ## 멱등성
//!
//! `is_applied()`는 원격 `/etc/fleet/worker.toml`에 `operational_token`이 이미
//! 있는지로 판단한다 — 있으면 재실행하지 않는다. `/v1/workers/join`은 항상
//! 신규 `worker_id`를 발급하고 재등록을 지원하지 않으므로(`register`와 다름),
//! 이미 성공한 join을 재실행하면 orchestrator에 중복 워커가 생긴다.
//!
//! ## 실패 시 원칙
//!
//! `/v1/workers/join`은 동일 이름의 워커가 이미 존재하면 `409 Conflict`로
//! 거부한다. 이 스텝은 그 경우를 조용히 성공으로 처리하거나 기존 워커를
//! 지우지 않는다 — 항상 명확한 에러로 전파해 운영자가 직접 판단하게 한다
//! (`agent.md`의 "실패 보상은 항상 전진" 원칙 — 워커 삭제로 후퇴하지 않는다).

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{Step, StepContext, StepOutput};

/// 원격 worker.toml 기본 경로. `InstallFleetWorker`가 준비한 `/etc/fleet`
/// 디렉토리와 동일 경로를 공유한다.
/// `ConfigureMtls`(로드맵 `#85`)가 join 뒤에 이 경로에 `[mtls]` 섹션을
/// 덧붙이므로 `pub(crate)`로 공유한다 — 두 스텝이 서로 다른 경로 상수를
/// 들고 있다가 어긋나는 사고를 구조적으로 막는다.
pub(crate) const REMOTE_CONFIG_PATH: &str = "/etc/fleet/worker.toml";
/// 원격 fleet-worker 바이너리 경로. `InstallFleetWorker`가 이 경로에 배포.
const REMOTE_BIN_PATH: &str = "/usr/local/bin/fleet-worker";
/// 발급 bootstrap token의 기본 TTL(초). exec 직후 곧바로 소비되므로 짧게
/// 잡아도 충분하지만, 느린 SSH 세션(패키지 설치 지연 등)을 감안해 5분 여유.
const DEFAULT_TOKEN_TTL_SECS: u64 = 300;

/// JoinWorker 스텝.
#[derive(Default)]
pub struct JoinWorker {
    /// bootstrap token 발급 요청 HTTP 타임아웃. 기본 10s.
    pub http_timeout: Option<Duration>,
    /// 발급할 bootstrap token의 TTL(초). 기본 [`DEFAULT_TOKEN_TTL_SECS`].
    pub token_ttl_secs: Option<u64>,
}

#[async_trait]
impl Step for JoinWorker {
    fn name(&self) -> &'static str {
        "join_worker"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["worker", "fleet-worker", "join"]
    }

    async fn is_applied(&self, exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        let out = exec
            .exec(&format!(
                "grep -q '^operational_token' {REMOTE_CONFIG_PATH} 2>/dev/null && echo yes"
            ))
            .await?;
        Ok(out.trim() == "yes")
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        if ctx.dry_run {
            return Ok(StepOutput::message(format!(
                "dry-run: would mint a single-use bootstrap token and run `fleet-worker join` \
                 on the remote host to register '{}' → {REMOTE_CONFIG_PATH}",
                ctx.worker_name
            )));
        }

        validate_prereqs(ctx)?;
        let api_token = ctx
            .orchestrator_api_token
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                StepError::PrereqFailed(
                    "orchestrator_api_token is required for join_worker step (needs \
                     token:issue capability)"
                        .into(),
                )
            })?;

        let api_url = ctx.orchestrator_url.trim_end_matches('/').to_string();
        let http = build_http_client(self.http_timeout)?;

        // 1. 이 워커 전용 1회용 bootstrap token 발급.
        let token = mint_bootstrap_token(
            &http,
            &api_url,
            api_token,
            &ctx.worker_name,
            self.token_ttl_secs.unwrap_or(DEFAULT_TOKEN_TTL_SECS),
        )
        .await?;

        // mTLS 워커는 --agent-endpoint의 server-key와 --grok-secret이 항상
        // 일치해야 하므로(로드맵 #85), 미지정이면 fleet-worker join의 자체
        // 무작위 생성에 맡기지 않고 여기서 먼저 확정한다.
        let grok_secret = resolve_grok_secret(ctx);

        // 2~3. 원격에서 join 실행 + 권한 보정.
        perform_join(exec, ctx, &token, grok_secret.as_deref()).await
    }
}

/// `ctx.grok_secret`이 있으면 그대로, 없고 mTLS가 켜져 있으면 새로 생성한다.
/// mTLS가 꺼져 있으면 `None`을 유지해 `fleet-worker join`이 스스로 무작위
/// 생성하도록 맡긴다(로드맵 `#82`부터의 기존 동작, 변경 없음).
fn resolve_grok_secret(ctx: &StepContext) -> Option<String> {
    if let Some(s) = ctx.grok_secret.as_deref().filter(|s| !s.is_empty()) {
        return Some(s.to_string());
    }
    if ctx.mtls_enabled {
        let bytes: [u8; 32] = rand::random();
        return Some(hex::encode(bytes));
    }
    None
}

/// mTLS 활성화 시 워커가 광고할 `wss://` agent endpoint를 구성한다.
/// SAN은 `IssueMtlsAssets`가 인증서를 발급할 때 쓰는 값과 항상 같은
/// `ctx.mtls_advertised_host`에서 나온다 — 로드맵 `#85`가 요구하는
/// "advertised_host를 SAN과 같은 값으로 강제"가 이 한 곳에서 자연히
/// 성립한다(두 값의 출처가 애초에 하나이므로 어긋날 수 없다).
fn mtls_agent_endpoint(ctx: &StepContext, grok_secret: &str) -> String {
    let host = ctx
        .mtls_advertised_host
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&ctx.worker_name);
    let port = ctx.mtls_advertised_port.unwrap_or_else(|| {
        ctx.mtls_listen_addr
            .as_deref()
            .and_then(|addr| addr.rsplit(':').next())
            .and_then(|p| p.parse().ok())
            .unwrap_or(2420)
    });
    format!("wss://{host}:{port}/ws?server-key={grok_secret}")
}

fn validate_prereqs(ctx: &StepContext) -> Result<(), StepError> {
    if ctx.orchestrator_url.is_empty() {
        return Err(StepError::PrereqFailed(
            "orchestrator_url is empty — required for join_worker step".into(),
        ));
    }
    if ctx.worker_name.is_empty() {
        return Err(StepError::PrereqFailed(
            "worker_name is empty — required for join_worker step".into(),
        ));
    }
    Ok(())
}

/// 이미 발급된 `token`으로 원격 `fleet-worker join`을 실행하고 결과를 처리한다.
/// `apply()`의 HTTP 발급 단계와 분리해 두어, 발급된 토큰이 주어졌다는
/// 전제하의 exec/에러 분류 로직을 HTTP 서버 없이 단위 테스트할 수 있다.
/// `grok_secret`은 `apply()`가 `resolve_grok_secret`으로 미리 확정한 값 —
/// mTLS 활성화 시 `--agent-endpoint`의 `server-key`와 반드시 일치해야
/// 하므로 이 함수 내부에서 새로 만들지 않는다.
async fn perform_join(
    exec: &dyn RemoteExecutor,
    ctx: &StepContext,
    token: &str,
    grok_secret: Option<&str>,
) -> Result<StepOutput, StepError> {
    let join_cmd = build_join_command(ctx, grok_secret);
    let (output, code) = exec
        .exec_with_stdin(&join_cmd, token.as_bytes())
        .await
        .map_err(StepError::from)?;
    if code != 0 {
        if is_name_conflict(&output) {
            return Err(StepError::RemoteExit {
                code,
                stderr: format!(
                    "worker '{}' already exists on the orchestrator — join does not \
                     re-register (register happens once per name; resolve the name \
                     conflict or reuse the existing worker's credentials instead): {}",
                    ctx.worker_name,
                    output.trim()
                ),
            });
        }
        return Err(StepError::RemoteExit {
            code,
            stderr: output,
        });
    }

    // 원격에서 기록한 worker.toml 권한 보정 (root 소유, 0600) — 로드맵 #79,
    // 실패를 삼키지 않는다.
    exec.exec_checked(&format!("sudo chmod 600 {REMOTE_CONFIG_PATH}"))
        .await?;

    Ok(StepOutput::message(format!(
        "worker '{}' joined via bootstrap token — config written to {REMOTE_CONFIG_PATH}",
        ctx.worker_name
    )))
}

/// 원격 `fleet-worker join` 커맨드라인을 구성한다.
///
/// 토큰은 절대 이 문자열에 포함하지 않는다 — `--token-file -`로 stdin에서
/// 읽도록 지시하고, 실제 값은 `exec_with_stdin`의 별도 채널로 전달한다.
/// `worker_name`/`orchestrator_url`/`labels`/`grok_secret`은 신뢰할 수
/// 없는 입력(인벤토리 YAML)일 수 있으므로 셸 인젝션 방지를 위해 항상
/// 작은따옴표로 quote한다.
///
/// `grok_secret`은 `resolve_grok_secret`이 미리 확정한 값(호출자가 지정했거나
/// mTLS용으로 새로 생성됨)을 그대로 받는다 — `None`이면 `fleet-worker join`이
/// 스스로 무작위 생성하도록 `--grok-secret`을 아예 생략한다. `ctx.mtls_enabled`면
/// `--agent-endpoint`를 명시적으로 계산해 넘긴다 — 그러지 않으면
/// `fleet-worker join`이 리버스 SSH 터널 시절의 `{scheme}://{host}/ws/{name}`
/// 형태로 자동 유도해(`derive_agent_endpoint`), 지금은 지원하지 않는
/// 토폴로지를 보고하게 된다(`docs/deployment/topology.md`).
fn build_join_command(ctx: &StepContext, grok_secret: Option<&str>) -> String {
    let mut cmd = format!(
        "sudo {REMOTE_BIN_PATH} join --token-file - --orchestrator-url {} --name {} \
         --config-out {}",
        shell_quote(&ctx.orchestrator_url),
        shell_quote(&ctx.worker_name),
        shell_quote(REMOTE_CONFIG_PATH),
    );
    if let Some(secret) = grok_secret.filter(|s| !s.is_empty()) {
        cmd.push_str(&format!(" --grok-secret {}", shell_quote(secret)));
    }
    if ctx.mtls_enabled {
        let secret = grok_secret.unwrap_or_default();
        let endpoint = mtls_agent_endpoint(ctx, secret);
        cmd.push_str(&format!(" --agent-endpoint {}", shell_quote(&endpoint)));
    }
    if let Some(max) = ctx.max_concurrent_tasks {
        cmd.push_str(&format!(" --max-concurrent-tasks {max}"));
    }
    if !ctx.labels.is_empty() {
        let mut keys: Vec<&String> = ctx.labels.keys().collect();
        keys.sort();
        let joined = keys
            .iter()
            .map(|k| format!("{k}={}", ctx.labels[*k]))
            .collect::<Vec<_>>()
            .join(",");
        cmd.push_str(&format!(" --labels {}", shell_quote(&joined)));
    }
    cmd
}

/// POSIX 셸 안전 작은따옴표 quoting: `'`를 `'\''`로 치환하고 전체를 감싼다.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r#"'\''"#))
}

/// `fleet-worker join`의 실패 출력에 동일 이름 워커가 이미 존재한다는
/// `409 Conflict` 표시가 있는지 판단한다. `fleet-worker`의 최상위 에러
/// 핸들링(`eprintln!("error: {e:#}")`)이 anyhow 체인을 그대로 이어붙이므로,
/// `join.rs::run_join`이 만드는 `"join failed: {status} — {text}"`가 그대로
/// 담겨 있다(`status`의 Display는 `"409 Conflict"`).
fn is_name_conflict(output: &str) -> bool {
    output.contains("409") || output.contains("already exists")
}

// ── orchestrator API 클라이언트 ─────────────────────────────────────────

#[derive(Debug, Serialize)]
struct MintBootstrapTokenRequest<'a> {
    prefix: &'a str,
    max_uses: u32,
    expires_in_secs: u64,
    notes: String,
    created_by: &'a str,
}

#[derive(Debug, Deserialize)]
struct MintBootstrapTokenResponse {
    token: String,
}

async fn mint_bootstrap_token(
    http: &reqwest::Client,
    api_url: &str,
    api_token: &str,
    worker_name: &str,
    ttl_secs: u64,
) -> Result<String, StepError> {
    let url = format!("{api_url}/v1/bootstrap-tokens");
    let body = MintBootstrapTokenRequest {
        prefix: "fleet",
        max_uses: 1,
        expires_in_secs: ttl_secs,
        notes: format!("fleet provision: {worker_name}"),
        created_by: "fleet-provision",
    };
    let resp = http
        .post(&url)
        .bearer_auth(api_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| StepError::RemoteExit {
            code: 0,
            stderr: format!("bootstrap token mint request failed: {e}"),
        })?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(StepError::RemoteExit {
            code: status.as_u16() as i32,
            stderr: format!("bootstrap token mint failed: {status} — {text}"),
        });
    }
    let parsed: MintBootstrapTokenResponse =
        resp.json().await.map_err(|e| StepError::RemoteExit {
            code: 0,
            stderr: format!("parsing bootstrap token mint response: {e}"),
        })?;
    Ok(parsed.token)
}

fn build_http_client(timeout: Option<Duration>) -> Result<reqwest::Client, StepError> {
    let mut builder = reqwest::Client::builder();
    builder = builder.timeout(timeout.unwrap_or(Duration::from_secs(10)));
    builder.build().map_err(|e| StepError::RemoteExit {
        code: 0,
        stderr: format!("building http client: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;

    fn ctx_with(worker_name: &str, orchestrator_url: &str, api_token: Option<&str>) -> StepContext {
        StepContext {
            worker_name: worker_name.into(),
            orchestrator_url: orchestrator_url.into(),
            orchestrator_api_token: api_token.map(String::from),
            ..Default::default()
        }
    }

    // ── prereq 검증 ────────────────────────────────────────────────────

    #[tokio::test]
    async fn apply_requires_orchestrator_url() {
        let exec = MockExecutor::new();
        let step = JoinWorker::default();
        let ctx = ctx_with("w1", "", Some("tok"));
        let err = step.apply(&exec, &ctx).await.unwrap_err();
        assert!(matches!(err, StepError::PrereqFailed(_)));
        assert!(format!("{err}").contains("orchestrator_url"));
    }

    #[tokio::test]
    async fn apply_requires_worker_name() {
        let exec = MockExecutor::new();
        let step = JoinWorker::default();
        let ctx = ctx_with("", "https://orch.example.com", Some("tok"));
        let err = step.apply(&exec, &ctx).await.unwrap_err();
        assert!(matches!(err, StepError::PrereqFailed(_)));
        assert!(format!("{err}").contains("worker_name"));
    }

    #[tokio::test]
    async fn apply_requires_api_token() {
        let exec = MockExecutor::new();
        let step = JoinWorker::default();
        let ctx = ctx_with("w1", "https://orch.example.com", None);
        let err = step.apply(&exec, &ctx).await.unwrap_err();
        assert!(matches!(err, StepError::PrereqFailed(_)));
        assert!(format!("{err}").contains("orchestrator_api_token"));
    }

    #[tokio::test]
    async fn dry_run_skips_everything() {
        let exec = MockExecutor::new();
        let step = JoinWorker::default();
        let ctx = StepContext {
            dry_run: true,
            worker_name: "w1".into(),
            ..Default::default()
        };
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("dry-run"));
        assert!(exec.recorded_calls().is_empty());
    }

    // ── is_applied ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn is_applied_when_operational_token_present() {
        let exec = MockExecutor::new();
        exec.expect_exec("grep -q '^operational_token'", "yes\n");
        let step = JoinWorker::default();
        assert!(step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn is_not_applied_when_config_missing_token() {
        let exec = MockExecutor::new();
        exec.expect_exec("grep -q '^operational_token'", "");
        let step = JoinWorker::default();
        assert!(!step.is_applied(&exec).await.unwrap());
    }

    // ── build_join_command / shell_quote ──────────────────────────────

    #[test]
    fn join_command_never_contains_the_token() {
        let ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        let cmd = build_join_command(&ctx, None);
        assert!(!cmd.contains("s3cr3t-token"));
        assert!(cmd.contains("--token-file -"));
        assert!(cmd.contains("--config-out"));
        assert!(cmd.contains(REMOTE_CONFIG_PATH));
    }

    #[test]
    fn join_command_quotes_untrusted_worker_name() {
        let ctx = ctx_with("w1; rm -rf /", "https://orch.example.com", Some("tok"));
        let cmd = build_join_command(&ctx, None);
        // 위험한 이름이 quote 없이 그대로 셸에 들어가면 안 된다.
        assert!(cmd.contains(&shell_quote("w1; rm -rf /")));
    }

    #[test]
    fn join_command_includes_optional_fields() {
        let mut ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        ctx.max_concurrent_tasks = Some(8);
        ctx.labels.insert("arch".into(), "arm64".into());
        ctx.labels.insert("region".into(), "us-east".into());
        let cmd = build_join_command(&ctx, Some("s3cr3t"));
        assert!(cmd.contains("--grok-secret"));
        assert!(cmd.contains("s3cr3t"));
        assert!(cmd.contains("--max-concurrent-tasks 8"));
        // 정렬된 키 — arch가 region보다 먼저.
        assert!(cmd.contains(&shell_quote("arch=arm64,region=us-east")));
    }

    #[test]
    fn join_command_without_secret_omits_grok_secret_flag() {
        let ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        let cmd = build_join_command(&ctx, None);
        assert!(!cmd.contains("--grok-secret"));
    }

    #[test]
    fn shell_quote_escapes_embedded_single_quotes() {
        assert_eq!(shell_quote("it's"), r#"'it'\''s'"#);
        assert_eq!(shell_quote("plain"), "'plain'");
    }

    // ── mTLS agent-endpoint (로드맵 #85) ──────────────────────────────

    #[test]
    fn mtls_disabled_never_adds_agent_endpoint_flag() {
        let mut ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        ctx.mtls_enabled = false;
        let cmd = build_join_command(&ctx, Some("secret"));
        assert!(!cmd.contains("--agent-endpoint"));
    }

    #[test]
    fn mtls_enabled_adds_wss_agent_endpoint_matching_advertised_host() {
        let mut ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        ctx.mtls_enabled = true;
        ctx.mtls_advertised_host = Some("w1.fleet.internal".into());
        ctx.mtls_advertised_port = Some(2420);
        let cmd = build_join_command(&ctx, Some("s3cr3t"));
        assert!(cmd.contains("--agent-endpoint"));
        assert!(cmd.contains(&shell_quote(
            "wss://w1.fleet.internal:2420/ws?server-key=s3cr3t"
        )));
    }

    #[test]
    fn mtls_agent_endpoint_falls_back_to_worker_name_and_listen_addr_port() {
        let mut ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        ctx.mtls_enabled = true;
        ctx.mtls_listen_addr = Some("0.0.0.0:9999".into());
        // advertised_host/port 미설정 — worker_name과 listen_addr 포트로 폴백.
        let endpoint = mtls_agent_endpoint(&ctx, "sec");
        assert_eq!(endpoint, "wss://w1:9999/ws?server-key=sec");
    }

    #[test]
    fn resolve_grok_secret_generates_when_mtls_enabled_and_unset() {
        let mut ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        ctx.mtls_enabled = true;
        let secret = resolve_grok_secret(&ctx).expect("mtls should generate a secret");
        assert_eq!(secret.len(), 64); // 32바이트 hex.
    }

    #[test]
    fn resolve_grok_secret_stays_none_when_mtls_disabled_and_unset() {
        let ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        assert!(resolve_grok_secret(&ctx).is_none());
    }

    #[test]
    fn resolve_grok_secret_prefers_explicit_value_even_with_mtls() {
        let mut ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        ctx.mtls_enabled = true;
        ctx.grok_secret = Some("explicit".into());
        assert_eq!(resolve_grok_secret(&ctx).as_deref(), Some("explicit"));
    }

    // ── is_name_conflict ───────────────────────────────────────────────

    #[test]
    fn detects_409_conflict_output() {
        assert!(is_name_conflict(
            "error: fleet-worker join failed: join failed: 409 Conflict — worker name 'w1' \
             already exists — use POST /v1/workers/register to re-register"
        ));
    }

    #[test]
    fn does_not_flag_unrelated_failures_as_conflict() {
        assert!(!is_name_conflict(
            "error: fleet-worker join failed: join request failed: connection refused"
        ));
    }

    // ── perform_join (토큰 발급을 이미 마친 상태 가정) ─────────────────

    #[tokio::test]
    async fn perform_join_sends_token_only_via_stdin_and_chmods_on_success() {
        let exec = MockExecutor::new();
        let ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        let out = perform_join(&exec, &ctx, "bt_supersecret", None)
            .await
            .unwrap();
        assert!(out.message.contains("joined"));

        let calls = exec.recorded_calls();
        // 커맨드라인 어디에도 토큰 원문이 없어야 한다.
        assert!(!calls.iter().any(|c| c.contains("bt_supersecret")));
        assert!(calls.iter().any(|c| c.contains("chmod 600")));

        let stdin_writes = exec.recorded_stdin_writes();
        assert_eq!(stdin_writes.len(), 1);
        assert_eq!(stdin_writes[0].1, b"bt_supersecret");
    }

    #[tokio::test]
    async fn perform_join_maps_409_to_actionable_conflict_error() {
        let exec = MockExecutor::new();
        let ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        exec.expect_exec(
            "sudo /usr/local/bin/fleet-worker join",
            "error: fleet-worker join failed: join failed: 409 Conflict — worker name 'w1' \
             already exists — use POST /v1/workers/register to re-register",
        );
        exec.expect_exit("sudo /usr/local/bin/fleet-worker join", 1);
        let err = perform_join(&exec, &ctx, "bt_x", None).await.unwrap_err();
        assert!(matches!(err, StepError::RemoteExit { code: 1, .. }));
        let msg = format!("{err}");
        assert!(msg.contains("already exists"));
        assert!(msg.contains("w1"));
    }

    #[tokio::test]
    async fn perform_join_propagates_other_failures_verbatim() {
        let exec = MockExecutor::new();
        let ctx = ctx_with("w1", "https://orch.example.com", Some("tok"));
        exec.expect_exec(
            "sudo /usr/local/bin/fleet-worker join",
            "error: fleet-worker join failed: join request failed: connection refused",
        );
        exec.expect_exit("sudo /usr/local/bin/fleet-worker join", 1);
        let err = perform_join(&exec, &ctx, "bt_x", None).await.unwrap_err();
        assert!(matches!(err, StepError::RemoteExit { code: 1, .. }));
        assert!(format!("{err}").contains("connection refused"));
    }

    #[test]
    fn step_has_join_tag() {
        let step = JoinWorker::default();
        assert!(step.tags().contains(&"join"));
    }
}
