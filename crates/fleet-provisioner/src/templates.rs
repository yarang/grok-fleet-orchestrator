//! 설정 파일 템플릿.
//!
//! Handlebars를 사용하지 않고 단순 문자열 치환으로 구현 — 의존성 최소화.
//! 템플릿이 복잡해지면 handlebars 크레이트로 전환.

use std::collections::HashMap;

use crate::error::StepError;

/// 템플릿 렌더링에 필요한 변수 모음.
///
/// `tunnel_name`/`hostname`/`credentials_path`는 cloudflared config.yml과
/// worker.toml 모두에 사용. 나머지 필드는 worker.toml에만 반영되며
/// `Option`이므로 cloudflared 호출처럼 일부만 채운 컨텍스트도 안전.
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    /// 터널 이름 (예: `fleet-build-1`).
    pub tunnel_name: String,
    /// DNS 호스트명 또는 orchestrator URL.
    pub hostname: String,
    /// cloudflared 자격증명 파일 경로.
    pub credentials_path: String,
    // ── worker.toml 전용 (선택) ─────────────────────────────────────────
    /// grok 바이너리 절대경로. 미설정 시 `/usr/local/bin/grok`.
    pub grok_bin: Option<String>,
    /// `grok agent serve --bind` 가 listen할 로컬 주소.
    /// 미설정 시 `127.0.0.1:2419`.
    pub grok_bind_addr: Option<String>,
    /// grok 서버 키 시크릿. worker.toml에서 필수 — None이면 에러.
    pub grok_secret: Option<String>,
    /// 동시 작업 수. 미설정 시 4.
    pub max_concurrent_tasks: Option<u32>,
    /// 재시작 간격(초). 미설정 시 5.
    pub restart_delay_secs: Option<u64>,
    /// grok 서브프로세스 작업 디렉토리. None이면 worker.toml에서 생략.
    pub grok_cwd: Option<String>,
    /// 오케스트레이터 등록용 bootstrap bearer 토큰. None이면 생략.
    pub bootstrap_token: Option<String>,
    /// worker 라벨. TOML inline table로 정렬해서 출력.
    pub labels: Option<HashMap<String, String>>,
}

/// cloudflared config.yml 렌더링.
///
/// ```yaml
/// tunnel: <tunnel_name>
/// credentials-file: <credentials_path>
/// ingress:
///   - hostname: <hostname>
///     service: http://localhost:8081
///   - service: http_status:404
/// ```
pub fn render_cloudflared_config(ctx: &TemplateContext) -> Result<String, StepError> {
    if ctx.tunnel_name.is_empty() {
        return Err(StepError::Template("tunnel_name is empty".into()));
    }
    if ctx.credentials_path.is_empty() {
        return Err(StepError::Template("credentials_path is empty".into()));
    }
    Ok(format!(
        r#"tunnel: {tunnel}
credentials-file: {creds}
ingress:
  - hostname: {host}
    service: http://localhost:8081
  - service: http_status:404
"#,
        tunnel = ctx.tunnel_name,
        creds = ctx.credentials_path,
        host = ctx.hostname,
    ))
}

/// fleet-worker용 worker.toml 렌더링.
///
/// ```toml
/// [worker]
/// name = "<tunnel_name>"
/// orchestrator_url = "<hostname>"
/// heartbeat_interval_secs = 15
/// labels = { ... }     # 비어있으면 {}
/// bootstrap_token = "..."  # ctx.bootstrap_token이 Some일 때만
///
/// [grok]
/// bin = "/usr/local/bin/grok"
/// bind_addr = "127.0.0.1:2419"
/// secret = "<secret>"
/// max_concurrent_tasks = 4
/// restart_delay_secs = 5
/// cwd = "..."           # ctx.grok_cwd가 Some일 때만
/// ```
///
/// `grok_secret`은 필수. None이거나 빈 문자열이면 에러.
pub fn render_worker_config(ctx: &TemplateContext) -> Result<String, StepError> {
    if ctx.tunnel_name.is_empty() {
        return Err(StepError::Template("tunnel_name (worker name) is empty".into()));
    }
    if ctx.hostname.is_empty() {
        return Err(StepError::Template("hostname (orchestrator_url) is empty".into()));
    }
    let secret = ctx
        .grok_secret
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| StepError::Template("grok_secret is required for worker.toml".into()))?;

    let grok_bin = ctx
        .grok_bin
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("/usr/local/bin/grok");
    let bind_addr = ctx
        .grok_bind_addr
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("127.0.0.1:2419");
    let max_tasks = ctx.max_concurrent_tasks.unwrap_or(4);
    let restart_delay = ctx.restart_delay_secs.unwrap_or(5);

    let labels_inline = format_labels(ctx.labels.as_ref());
    let mut out = String::with_capacity(512);
    out.push_str("[worker]\n");
    out.push_str(&format!("name = \"{}\"\n", ctx.tunnel_name));
    out.push_str(&format!("orchestrator_url = \"{}\"\n", ctx.hostname));
    out.push_str("heartbeat_interval_secs = 15\n");
    out.push_str(&format!("labels = {labels_inline}\n"));
    if let Some(tok) = ctx.bootstrap_token.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("bootstrap_token = \"{tok}\"\n"));
    }
    out.push('\n');
    out.push_str("[grok]\n");
    out.push_str(&format!("bin = \"{grok_bin}\"\n"));
    out.push_str(&format!("bind_addr = \"{bind_addr}\"\n"));
    out.push_str(&format!("secret = \"{secret}\"\n"));
    out.push_str(&format!("max_concurrent_tasks = {max_tasks}\n"));
    out.push_str(&format!("restart_delay_secs = {restart_delay}\n"));
    if let Some(cwd) = ctx.grok_cwd.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("cwd = \"{cwd}\"\n"));
    }
    out.push('\n');
    Ok(out)
}

/// 라벨 맵을 TOML inline table 형태로 직렬화.
///
/// - `None` → `{}`
/// - 빈 맵 → `{}`
/// - 그 외 → `{ key1 = "val1", key2 = "val2" }` (key 기준 정렬, 결정론적 출력)
fn format_labels(labels: Option<&HashMap<String, String>>) -> String {
    let Some(map) = labels else {
        return "{}".into();
    };
    if map.is_empty() {
        return "{}".into();
    }
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    let parts: Vec<String> = keys
        .into_iter()
        .map(|k| format!("{k} = \"{}\"", map[k]))
        .collect();
    format!("{{ {} }}", parts.join(", "))
}

/// fleet-worker systemd 유닛 파일 (정적).
pub const FLEET_WORKER_UNIT: &str = r#"[Unit]
Description=Fleet Worker Agent
After=network-online.target cloudflared.service
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/fleet-worker --config /etc/fleet/worker.toml
Restart=on-failure
RestartSec=5
User=root
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ctx() -> TemplateContext {
        TemplateContext {
            tunnel_name: "fleet-build-1".into(),
            hostname: "build-1.fleet.example.com".into(),
            credentials_path: "/etc/cloudflared/creds.json".into(),
            grok_secret: Some("server-key-abc".into()),
            ..Default::default()
        }
    }

    #[test]
    fn cloudflared_config_contains_tunnel_and_hostname() {
        let cfg = render_cloudflared_config(&sample_ctx()).unwrap();
        assert!(cfg.contains("tunnel: fleet-build-1"));
        assert!(cfg.contains("credentials-file: /etc/cloudflared/creds.json"));
        assert!(cfg.contains("hostname: build-1.fleet.example.com"));
        assert!(cfg.contains("service: http://localhost:8081"));
    }

    #[test]
    fn worker_config_emits_worker_and_grok_sections() {
        let cfg = render_worker_config(&sample_ctx()).unwrap();
        assert!(cfg.contains("[worker]"));
        assert!(cfg.contains("[grok]"));
        assert!(cfg.contains("name = \"fleet-build-1\""));
        assert!(cfg.contains("orchestrator_url = \"build-1.fleet.example.com\""));
        assert!(cfg.contains("heartbeat_interval_secs = 15"));
        // grok section defaults
        assert!(cfg.contains("bin = \"/usr/local/bin/grok\""));
        assert!(cfg.contains("bind_addr = \"127.0.0.1:2419\""));
        assert!(cfg.contains("secret = \"server-key-abc\""));
        assert!(cfg.contains("max_concurrent_tasks = 4"));
        assert!(cfg.contains("restart_delay_secs = 5"));
        // cloudflared 섹션은 worker.toml에서 제거되어야 함.
        assert!(!cfg.contains("[cloudflared]"));
    }

    #[test]
    fn worker_config_requires_grok_secret() {
        let mut ctx = sample_ctx();
        ctx.grok_secret = None;
        let err = render_worker_config(&ctx).unwrap_err();
        assert!(format!("{err}").contains("grok_secret"));
        ctx.grok_secret = Some("".into());
        assert!(render_worker_config(&ctx).is_err());
    }

    #[test]
    fn worker_config_requires_hostname() {
        let mut ctx = sample_ctx();
        ctx.hostname = "".into();
        assert!(render_worker_config(&ctx).is_err());
    }

    #[test]
    fn worker_config_overrides_and_labels() {
        let mut labels = HashMap::new();
        labels.insert("arch".into(), "arm64".into());
        labels.insert("region".into(), "us-east".into());
        let ctx = TemplateContext {
            tunnel_name: "w1".into(),
            hostname: "https://fleet.example.com".into(),
            grok_secret: Some("s3cr3t".into()),
            grok_bin: Some("/opt/grok".into()),
            grok_bind_addr: Some("0.0.0.0:3100".into()),
            max_concurrent_tasks: Some(8),
            restart_delay_secs: Some(15),
            grok_cwd: Some("/var/lib/fleet-worker".into()),
            bootstrap_token: Some("fleet-tok".into()),
            labels: Some(labels),
            ..Default::default()
        };
        let cfg = render_worker_config(&ctx).unwrap();
        // 정렬된 키 — arch가 region보다 먼저.
        assert!(cfg.contains("labels = { arch = \"arm64\", region = \"us-east\" }"));
        assert!(cfg.contains("bin = \"/opt/grok\""));
        assert!(cfg.contains("bind_addr = \"0.0.0.0:3100\""));
        assert!(cfg.contains("max_concurrent_tasks = 8"));
        assert!(cfg.contains("restart_delay_secs = 15"));
        assert!(cfg.contains("cwd = \"/var/lib/fleet-worker\""));
        assert!(cfg.contains("bootstrap_token = \"fleet-tok\""));
    }

    #[test]
    fn format_labels_handles_empty_and_sorted() {
        assert_eq!(format_labels(None), "{}");
        let empty: HashMap<String, String> = HashMap::new();
        assert_eq!(format_labels(Some(&empty)), "{}");
        let mut m = HashMap::new();
        m.insert("z".into(), "1".into());
        m.insert("a".into(), "2".into());
        assert_eq!(format_labels(Some(&m)), "{ a = \"2\", z = \"1\" }");
    }

    #[test]
    fn empty_tunnel_name_errors() {
        let ctx = TemplateContext {
            tunnel_name: "".into(),
            hostname: "x".into(),
            credentials_path: "/x".into(),
            ..Default::default()
        };
        assert!(render_cloudflared_config(&ctx).is_err());
    }

    #[test]
    fn fleet_worker_unit_has_systemd_directives() {
        assert!(FLEET_WORKER_UNIT.contains("[Unit]"));
        assert!(FLEET_WORKER_UNIT.contains("ExecStart=/usr/local/bin/fleet-worker"));
        assert!(FLEET_WORKER_UNIT.contains("Restart=on-failure"));
    }
}
