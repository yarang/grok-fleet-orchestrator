//! Step 6: 서비스 시작 및 검증. systemd 유닛 enable + start, 하트비트 도달 폴링.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Deserialize;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{Step, StepContext, StepOutput};

pub struct StartServices {
    /// 로컬 systemctl 활성화와 오케스트레이터 하트비트 도달을 각각 기다리는
    /// 최대 시간 (초).
    pub wait_timeout_secs: u64,
    /// 위 대기 동안의 폴링 간격. 테스트에서는 짧게 줄여 실시간 대기를 없앤다.
    pub poll_interval: Duration,
}

impl Default for StartServices {
    fn default() -> Self {
        Self {
            wait_timeout_secs: 30,
            poll_interval: Duration::from_secs(1),
        }
    }
}

/// `GET /v1/workers` 응답에서 필요한 필드만 취한다 — `fleet-api`의
/// `WorkerSummary` 전체를 끌어오지 않기 위해 (`push_credentials.rs`의
/// `CredentialSummary`와 같은 관례).
#[derive(Debug, Deserialize)]
struct WorkerStatusEntry {
    name: String,
    status: String,
}

/// 오케스트레이터 폴링 1회의 결과 분류. 재시도 가능한 상태와 즉시 실패해야
/// 하는 상태를 명시적으로 구분한다 (로드맵 #79).
enum Probe {
    Found(String),
    NotFound,
    Unauthorized(u16),
    TransientError,
}

/// HTTP status code만으로 판정 가능한 경우 `Some`을 반환한다 — 성공 상태는
/// 본문을 파싱해야 `Found`/`NotFound`를 가릴 수 있으므로 `None`.
///
/// 순수 함수로 분리한 이유: 401/403을 "worker not found" 타임아웃과
/// 구분하는 것이 이 로직의 핵심 안전장치인데, 실제 HTTP round-trip 없이
/// 이 분기 자체를 테스트로 고정하기 위해서다.
fn classify_status(status: reqwest::StatusCode) -> Option<Probe> {
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Some(Probe::Unauthorized(status.as_u16()));
    }
    if !status.is_success() {
        return Some(Probe::TransientError);
    }
    None
}

#[async_trait]
impl Step for StartServices {
    fn name(&self) -> &'static str {
        "start_services"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["start", "services"]
    }

    async fn is_applied(&self, exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        let active = exec
            .exec("systemctl is-active fleet-worker 2>/dev/null")
            .await?;
        Ok(active.trim() == "active")
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        if ctx.dry_run {
            return Ok(StepOutput::message(
                "dry-run: enable and start fleet-worker + cloudflared".to_string(),
            ));
        }

        // 유닛 enable + start (멱등). 로드맵 #79 — 이전에는 이 4개 명령을 전부
        // `let _ =`로 버려서, 예를 들어 `daemon-reload`가 잘못된 유닛 파일
        // 문법으로 실패해도 스텝이 계속 진행되고 원인이 뒤섞였다.
        //
        // fleet-worker 관련 3개는 load-bearing이라 실패 시 즉시 중단한다.
        // cloudflared enable은 과도기 스텝(#85에서 표준 playbook 제거 예정)
        // 이므로 best-effort로 남기되, 실패를 조용히 삼키지 않고 로그로
        // 관측 가능하게 한다.
        exec.exec_checked("sudo systemctl daemon-reload").await?;
        if let Err(e) = exec
            .exec_checked("sudo systemctl enable --now cloudflared")
            .await
        {
            tracing::warn!(error = %e, "cloudflared enable failed (best-effort, continuing)");
        }
        exec.exec_checked("sudo systemctl enable --now fleet-worker")
            .await?;
        exec.exec_checked("sudo systemctl restart fleet-worker")
            .await?;

        // 로컬 검증: 즉시 1회 확인 대신 `wait_timeout_secs` 동안 폴링한다 —
        // 재시작 직후에는 서비스가 아직 뜨는 중일 수 있다.
        self.poll_until_active(exec).await?;

        // 오케스트레이터 하트비트 도달 확인. `wait_timeout_secs`의 doc이
        // 약속한 대로 실제로 대기한다(로드맵 #79 — 이전에는 이 필드가
        // 어디서도 읽히지 않는 죽은 필드였다). API token이 없는 배포(테스트,
        // 오케스트레이터 미연동 dry 환경)에서는 로컬 상태만 확인했다고
        // 명시적으로 경고하고 넘어간다 — fail-closed로 막지 않는다, 기존
        // 호출부와의 하위 호환을 위해서다.
        match ctx.orchestrator_api_token.as_deref() {
            Some(token) if !token.is_empty() && !ctx.orchestrator_url.is_empty() => {
                self.wait_for_heartbeat(&ctx.orchestrator_url, token, &ctx.worker_name)
                    .await?;
            }
            _ => {
                tracing::warn!(
                    worker = %ctx.worker_name,
                    "orchestrator_api_token/orchestrator_url not set — skipping heartbeat \
                     confirmation, only local systemctl state was verified"
                );
            }
        }

        Ok(StepOutput::message(format!(
            "all services started on {}",
            ctx.worker_name
        )))
    }
}

impl StartServices {
    async fn poll_until_active(&self, exec: &dyn RemoteExecutor) -> Result<(), StepError> {
        let deadline = Instant::now() + Duration::from_secs(self.wait_timeout_secs);
        loop {
            let status = exec
                .exec("systemctl is-active fleet-worker 2>/dev/null")
                .await?;
            let status = status.trim().to_string();
            if status == "active" {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(StepError::RemoteExit {
                    code: 1,
                    stderr: format!(
                        "fleet-worker not active after {}s (state: {status})",
                        self.wait_timeout_secs
                    ),
                });
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn wait_for_heartbeat(
        &self,
        orchestrator_url: &str,
        token: &str,
        worker_name: &str,
    ) -> Result<(), StepError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| StepError::RemoteExit {
                code: 0,
                stderr: format!("building http client for heartbeat check: {e}"),
            })?;
        let url = format!(
            "{}/v1/workers?limit=500",
            orchestrator_url.trim_end_matches('/')
        );
        let deadline = Instant::now() + Duration::from_secs(self.wait_timeout_secs);
        loop {
            match self.probe_heartbeat(&http, &url, token, worker_name).await {
                // 인증/권한 오류는 재시도해도 해소되지 않는다 — 남은 시간
                // 동안 계속 폴링하다 "worker not found" 타임아웃으로
                // 오인시키지 않고 즉시, 정확한 원인으로 실패한다 (로드맵
                // #79의 요지 그 자체: 실패는 관측 가능해야 한다).
                Probe::Unauthorized(status) => {
                    return Err(StepError::RemoteExit {
                        code: status as i32,
                        stderr: format!(
                            "orchestrator rejected heartbeat check with {status} — the \
                             provisioning API token likely lacks 'worker:list' capability \
                             (see docs/deployment/worker-provisioning.md)"
                        ),
                    });
                }
                Probe::Found(status) if status == "online" => return Ok(()),
                Probe::Found(_) | Probe::NotFound | Probe::TransientError => {}
            }
            if Instant::now() >= deadline {
                return Err(StepError::RemoteExit {
                    code: 1,
                    stderr: format!(
                        "worker '{worker_name}' did not report a heartbeat to the \
                         orchestrator within {}s",
                        self.wait_timeout_secs
                    ),
                });
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    /// 워커 목록을 1회 조회해 `worker_name`의 현재 status를 반환한다.
    async fn probe_heartbeat(
        &self,
        http: &reqwest::Client,
        url: &str,
        token: &str,
        worker_name: &str,
    ) -> Probe {
        let resp = match http.get(url).bearer_auth(token).send().await {
            Ok(r) => r,
            // 네트워크 오류(오케스트레이터 재시작 중 등)는 일시적일 수
            // 있으므로 폴링을 중단시키지 않는다.
            Err(_) => return Probe::TransientError,
        };
        if let Some(probe) = classify_status(resp.status()) {
            return probe;
        }
        let Ok(workers) = resp.json::<Vec<WorkerStatusEntry>>().await else {
            return Probe::TransientError;
        };
        match workers.into_iter().find(|w| w.name == worker_name) {
            Some(w) => Probe::Found(w.status),
            None => Probe::NotFound,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;

    /// 테스트 전용 — 실시간 대기를 없애기 위해 poll_interval을 최소화한다.
    fn fast_step() -> StartServices {
        StartServices {
            wait_timeout_secs: 1,
            poll_interval: Duration::from_millis(1),
        }
    }

    // ── classify_status (로드맵 #79) ─────────────────────────────────────

    #[test]
    fn classify_status_marks_401_and_403_as_unauthorized_not_transient() {
        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
        ] {
            match classify_status(status) {
                Some(Probe::Unauthorized(code)) => assert_eq!(code, status.as_u16()),
                _ => panic!("expected Probe::Unauthorized for status {status}"),
            }
        }
    }

    #[test]
    fn classify_status_marks_other_non_success_as_transient() {
        for status in [
            reqwest::StatusCode::BAD_GATEWAY,
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            reqwest::StatusCode::NOT_FOUND,
        ] {
            assert!(matches!(
                classify_status(status),
                Some(Probe::TransientError)
            ));
        }
    }

    #[test]
    fn classify_status_defers_success_to_body_parsing() {
        assert!(classify_status(reqwest::StatusCode::OK).is_none());
    }

    #[tokio::test]
    async fn is_applied_when_fleet_worker_active() {
        let exec = MockExecutor::new();
        exec.expect_exec("systemctl is-active fleet-worker", "active\n");
        let step = StartServices::default();
        assert!(step.is_applied(&exec).await.unwrap());
    }

    #[tokio::test]
    async fn apply_enables_and_starts_units() {
        let exec = MockExecutor::new();
        // start 후 is-active 쿼리에 대한 응답.
        exec.expect_exec("systemctl is-active fleet-worker", "active\n");
        let step = fast_step();
        let ctx = StepContext::for_worker("build-1");
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("build-1"));
        let calls = exec.recorded_calls();
        assert!(calls.iter().any(|c| c.contains("daemon-reload")));
        assert!(calls.iter().any(|c| c.contains("enable --now")));
    }

    #[tokio::test]
    async fn apply_fails_when_service_never_becomes_active() {
        let exec = MockExecutor::new();
        exec.expect_exec("systemctl is-active fleet-worker", "failed\n");
        let step = fast_step();
        let result = step.apply(&exec, &StepContext::default()).await;
        assert!(matches!(result, Err(StepError::RemoteExit { .. })));
    }

    #[tokio::test]
    async fn apply_fails_when_daemon_reload_fails() {
        let exec = MockExecutor::new();
        exec.expect_exec("sudo systemctl daemon-reload", "bad unit file\n");
        exec.expect_exit("sudo systemctl daemon-reload", 1);
        let step = fast_step();
        let result = step.apply(&exec, &StepContext::default()).await;
        match result {
            Err(StepError::RemoteExit { code, stderr }) => {
                assert_eq!(code, 1);
                assert!(stderr.contains("bad unit file"));
            }
            other => panic!("expected RemoteExit with daemon-reload output, got {other:?}"),
        }
        // fleet-worker enable/restart는 실행되지 않아야 한다 (즉시 중단).
        let calls = exec.recorded_calls();
        assert!(!calls
            .iter()
            .any(|c| c.contains("enable --now fleet-worker")));
    }

    #[tokio::test]
    async fn apply_continues_when_only_cloudflared_enable_fails() {
        let exec = MockExecutor::new();
        exec.expect_exit("sudo systemctl enable --now cloudflared", 1);
        exec.expect_exec("systemctl is-active fleet-worker", "active\n");
        let step = fast_step();
        let out = step.apply(&exec, &StepContext::default()).await.unwrap();
        assert!(out.message.contains("all services started"));
    }

    #[tokio::test]
    async fn apply_without_orchestrator_token_skips_heartbeat_check_but_succeeds() {
        let exec = MockExecutor::new();
        exec.expect_exec("systemctl is-active fleet-worker", "active\n");
        let step = fast_step();
        let ctx = StepContext {
            orchestrator_api_token: None,
            ..StepContext::default()
        };
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("all services started"));
    }

    #[tokio::test]
    async fn dry_run_skips_enable() {
        let exec = MockExecutor::new();
        let step = StartServices::default();
        let ctx = StepContext {
            dry_run: true,
            ..Default::default()
        };
        step.apply(&exec, &ctx).await.unwrap();
        assert!(exec.recorded_calls().is_empty());
    }
}
