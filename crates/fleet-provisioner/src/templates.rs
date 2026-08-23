//! 설정 파일 템플릿.
//!
//! Handlebars를 사용하지 않고 단순 문자열 치환으로 구현 — 의존성 최소화.
//! 템플릿이 복잡해지면 handlebars 크레이트로 전환.
//!
//! worker.toml 렌더링은 이 모듈이 아니라 `fleet-worker join`(오케스트레이터의
//! `/v1/workers/join` 응답)이 담당한다 — `JoinWorker` 스텝(로드맵 `#82`) 참고.
//! 과거 이 모듈이 로컬에서 worker.toml을 중복 렌더링했으나, 그 결과물은
//! `fleet-worker`가 fail-closed로 거부하는 legacy `bootstrap_token` 필드를
//! 썼기 때문에 기동 불가능한 워커를 만들었다. 지금은 cloudflared config.yml만
//! 이 모듈이 렌더링한다.

use crate::error::StepError;

/// 템플릿 렌더링에 필요한 변수 모음 (cloudflared config.yml 전용).
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    /// 터널 이름 (예: `fleet-build-1`).
    pub tunnel_name: String,
    /// DNS 호스트명.
    pub hostname: String,
    /// cloudflared 자격증명 파일 경로.
    pub credentials_path: String,
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
    fn empty_tunnel_name_errors() {
        let ctx = TemplateContext {
            tunnel_name: "".into(),
            hostname: "x".into(),
            credentials_path: "/x".into(),
        };
        assert!(render_cloudflared_config(&ctx).is_err());
    }

    #[test]
    fn empty_credentials_path_errors() {
        let ctx = TemplateContext {
            tunnel_name: "x".into(),
            hostname: "x".into(),
            credentials_path: "".into(),
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
