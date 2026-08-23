//! Step 1: 사전 검증. OS, arch, 디스크/메모리, Rust, systemd 여부 확인.

use std::collections::HashMap;

use async_trait::async_trait;

use crate::error::StepError;
use crate::ssh::RemoteExecutor;
use crate::steps::{PrereqReport, Step, StepContext, StepOutput};

/// 원격 호스트에서 `CheckPrereqs`를 실제로 실행해 `PrereqReport`를 얻는다
/// (로드맵 `#81`).
///
/// `Playbook::standard`/`Playbook::dry_run`을 구성하기 **전에** 호출해야
/// 한다 — 그러지 않으면 OS/아키텍처 감지 없이 하드코딩된 가정(예:
/// `ubuntu`/`x86_64`)으로 이기종 fleet에 잘못된 패키지 매니저·바이너리를
/// 선택하게 된다. `check_prereqs` 자체가 부작용 없는 조회이므로(모듈 doc),
/// 이 함수가 Playbook 안에서 같은 스텝을 다시 실행하는 것과 중복 호출돼도
/// 안전하다 — 그 재실행은 정상 흐름의 일부다(Playbook의 첫 스텝으로
/// 남겨 실행 이력에 온전히 기록되도록 함).
///
/// 검증 실패(지원되지 않는 OS, 리소스 부족 등)는 그대로 전파한다. 여기서
/// 조용히 가정값으로 대체하지 않는다 — 실 호스트에 대한 판정을 흐리는 것은
/// `#79`가 막으려는 것과 같은 종류의 결함이다.
///
/// `StepContext::default()`(`dry_run: false`)로 호출하므로, 대상이
/// `MockExecutor`라도 실제 검증 로직을 그대로 탄다 — dry-run 미리보기가
/// 필요하면 [`assumed_prereq_from_labels`]를 대신 쓴다.
pub async fn detect_prereq(exec: &dyn RemoteExecutor) -> Result<PrereqReport, StepError> {
    let step = CheckPrereqs::default();
    let ctx = StepContext::default();
    let output = step.apply(exec, &ctx).await?;
    let payload = output
        .payload
        .ok_or_else(|| StepError::Parse("check_prereqs produced no payload".into()))?;
    serde_json::from_value(payload)
        .map_err(|e| StepError::Parse(format!("decoding PrereqReport: {e}")))
}

/// 실제 연결 없이 인벤토리 라벨(`arch`/`os`)에서 미리보기용 가정값을
/// 구성한다 (로드맵 `#81`).
///
/// 라벨이 없으면 기존 기본값(`ubuntu`/`x86_64`)으로 폴백한다 — 이전 동작과의
/// 하위 호환. `CheckPrereqs::apply`가 `ctx.dry_run == true`일 때 바로 이
/// 함수를 쓴다(아래) — 인벤토리/단일 호스트 CLI dry-run은 아무 것도
/// 프로그래밍되지 않은 `MockExecutor`를 쓰므로, 실제 조회를 시도하면
/// `os`/`arch`가 빈 문자열로 돌아와 "OS를 감지할 수 없음"으로 매번
/// 실패했었다(로드맵 #81에서 실측으로 발견 — 이 함수가 없었을 때는 이
/// 문서가 설명하는 정확히 그 실패가 재현됐다).
pub fn assumed_prereq_from_labels(labels: &HashMap<String, String>) -> PrereqReport {
    PrereqReport {
        os: labels
            .get("os")
            .cloned()
            .unwrap_or_else(|| "ubuntu".to_string()),
        arch: labels
            .get("arch")
            .cloned()
            .unwrap_or_else(|| "x86_64".to_string()),
        mem_mb: 16384,
        disk_gb: 100,
        has_rust: false,
        has_systemd: true,
    }
}

/// 사전 검증 스텝. 항상 실행 (멱등 — 부작용 없음).
pub struct CheckPrereqs {
    /// 최소 디스크 여유 (GB).
    pub min_disk_gb: u64,
    /// 최소 메모리 (MB).
    pub min_mem_mb: u64,
}

impl Default for CheckPrereqs {
    fn default() -> Self {
        Self {
            min_disk_gb: 10,
            min_mem_mb: 4096,
        }
    }
}

#[async_trait]
impl Step for CheckPrereqs {
    fn name(&self) -> &'static str {
        "check_prereqs"
    }

    fn tags(&self) -> &'static [&'static str] {
        &["check", "prereqs"]
    }

    async fn is_applied(&self, _exec: &dyn RemoteExecutor) -> Result<bool, StepError> {
        // 항상 실행 — 사전 검증은 부작용이 없으므로 반복 수행해도 안전.
        Ok(false)
    }

    async fn apply(
        &self,
        exec: &dyn RemoteExecutor,
        ctx: &StepContext,
    ) -> Result<StepOutput, StepError> {
        // 로드맵 #81 — dry-run(인벤토리/단일 호스트 CLI 미리보기)은 아무 것도
        // 프로그래밍되지 않은 MockExecutor를 쓰며 실제 연결이 없다. 그런데도
        // 이 스텝은 원래 dry_run과 무관하게 항상 진짜 조회·검증을 시도했다
        // — MockExecutor가 모든 질의에 빈 문자열을 돌려주므로 "OS를 감지할
        // 수 없음"으로 매번 실패해, 인벤토리 dry-run 자체가 성립하지 않았다
        // (실제 CLI로 재현 확인됨). 인벤토리 라벨(arch/os)에서 가정값을
        // 구성해 최소한 이후 스텝이 올바른 아키텍처 분기를 미리 보여줄 수
        // 있게 한다. 실 연결(`dry_run == false`)에서는 이 분기를 타지
        // 않는다 — 검증은 그대로 살아 있다.
        if ctx.dry_run {
            let report = assumed_prereq_from_labels(&ctx.labels);
            return Ok(StepOutput::with_payload(
                format!(
                    "dry-run: assuming {} on {} (from inventory labels, not detected — \
                     no real connection in dry-run mode)",
                    report.os, report.arch
                ),
                &report,
            ));
        }

        let os_raw = exec
            .exec("cat /etc/os-release | grep '^ID=' | cut -d= -f2")
            .await?;
        let os = os_raw.trim().trim_matches('"').to_lowercase();

        let arch = exec.exec("uname -m").await?;
        let arch = arch.trim().to_string();

        let mem_str = exec.exec("free -m | awk '/^Mem:/{print $2}'").await?;
        let mem_mb: u64 = mem_str.trim().parse().unwrap_or(0);

        let disk_str = exec.exec("df -BG / | awk 'NR==2{print $4}'").await?;
        let disk_clean = disk_str.trim().trim_end_matches('G');
        let disk_gb: u64 = disk_clean.parse().unwrap_or(0);

        let rust_path = exec.exec("which cargo 2>/dev/null").await?;
        let has_rust = !rust_path.trim().is_empty();

        let systemd_pid = exec.exec("pidof systemd 2>/dev/null").await?;
        let has_systemd = !systemd_pid.trim().is_empty();

        let report = PrereqReport {
            os,
            arch,
            mem_mb,
            disk_gb,
            has_rust,
            has_systemd,
        };

        // 검증
        if !has_systemd {
            return Err(StepError::PrereqFailed(
                "systemd is required for service management".into(),
            ));
        }
        if report.os.is_empty() {
            return Err(StepError::PrereqFailed(
                "could not detect OS (empty /etc/os-release ID)".into(),
            ));
        }
        if mem_mb > 0 && mem_mb < self.min_mem_mb {
            return Err(StepError::PrereqFailed(format!(
                "insufficient memory: {} MB < {} MB minimum",
                mem_mb, self.min_mem_mb
            )));
        }
        if disk_gb > 0 && disk_gb < self.min_disk_gb {
            return Err(StepError::PrereqFailed(format!(
                "insufficient disk: {} GB < {} GB minimum",
                disk_gb, self.min_disk_gb
            )));
        }

        tracing::info!(
            os = %report.os,
            arch = %report.arch,
            mem_mb = report.mem_mb,
            disk_gb = report.disk_gb,
            has_rust = report.has_rust,
            "prereqs verified"
        );

        Ok(StepOutput::with_payload(
            format!(
                "prereqs ok: {} on {}, {}MB RAM, {}GB disk",
                report.os, report.arch, report.mem_mb, report.disk_gb
            ),
            &report,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockExecutor;

    fn healthy_responses() -> MockExecutor {
        let exec = MockExecutor::new();
        exec.expect_exec("cat /etc/os-release", "ubuntu\n");
        exec.expect_exec("uname -m", "x86_64\n");
        exec.expect_exec("free -m", "16384\n");
        exec.expect_exec("df -BG", "200G\n");
        exec.expect_exec("which cargo", "/home/user/.cargo/bin/cargo\n");
        exec.expect_exec("pidof systemd", "1\n");
        exec
    }

    #[tokio::test]
    async fn detects_ubuntu_x86_with_sufficient_resources() {
        let exec = healthy_responses();
        let step = CheckPrereqs::default();
        let ctx = StepContext::default();
        let out = step.apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("ubuntu"));
        assert!(out.message.contains("x86_64"));
        let report: PrereqReport = serde_json::from_value(out.payload.unwrap()).unwrap();
        assert_eq!(report.os, "ubuntu");
        assert_eq!(report.arch, "x86_64");
        assert_eq!(report.mem_mb, 16384);
        assert_eq!(report.disk_gb, 200);
        assert!(report.has_rust);
        assert!(report.has_systemd);
    }

    #[tokio::test]
    async fn fails_when_systemd_missing() {
        let exec = MockExecutor::new();
        exec.expect_exec("cat /etc/os-release", "ubuntu\n");
        exec.expect_exec("uname -m", "x86_64\n");
        exec.expect_exec("free -m", "16384\n");
        exec.expect_exec("df -BG", "200G\n");
        exec.expect_exec("which cargo", "");
        exec.expect_exec("pidof systemd", "");
        let step = CheckPrereqs::default();
        let result = step.apply(&exec, &StepContext::default()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, StepError::PrereqFailed(_)));
        assert!(format!("{err}").contains("systemd"));
    }

    #[tokio::test]
    async fn fails_when_disk_insufficient() {
        let exec = MockExecutor::new();
        exec.expect_exec("cat /etc/os-release", "ubuntu\n");
        exec.expect_exec("uname -m", "x86_64\n");
        exec.expect_exec("free -m", "16384\n");
        exec.expect_exec("df -BG", "5G\n");
        exec.expect_exec("which cargo", "");
        exec.expect_exec("pidof systemd", "1\n");
        let step = CheckPrereqs::default();
        let result = step.apply(&exec, &StepContext::default()).await;
        assert!(matches!(result, Err(StepError::PrereqFailed(_))));
    }

    #[tokio::test]
    async fn is_applied_always_false() {
        let exec = MockExecutor::new();
        let step = CheckPrereqs::default();
        assert!(!step.is_applied(&exec).await.unwrap());
    }

    // ── dry-run은 실제 조회 대신 라벨 기반 가정값을 쓴다 (로드맵 #81) ────
    //
    // 이전에는 apply()가 ctx.dry_run과 무관하게 항상 실제 조회를 시도했다.
    // 인벤토리/단일 호스트 CLI dry-run은 프로그래밍되지 않은 MockExecutor를
    // 쓰므로, 모든 질의가 빈 문자열로 돌아와 "could not detect OS"로 매번
    // 실패했다 — 실제 `fleet provision --dry-run` 실행으로 재현 확인된
    // 회귀다. 아래 테스트는 그 경로가 더 이상 실패하지 않음을 고정한다.

    #[tokio::test]
    async fn dry_run_against_bare_mock_executor_does_not_fail() {
        // 아무 것도 프로그래밍하지 않은 MockExecutor — 인벤토리 dry-run이
        // 실제로 쓰는 것과 동일한 상태.
        let exec = MockExecutor::new();
        let mut labels = HashMap::new();
        labels.insert("arch".to_string(), "aarch64".to_string());
        let ctx = StepContext {
            dry_run: true,
            labels,
            ..Default::default()
        };
        let out = CheckPrereqs::default().apply(&exec, &ctx).await.unwrap();
        assert!(out.message.contains("dry-run"));
        assert!(out.message.contains("aarch64"));
        let report: PrereqReport = serde_json::from_value(out.payload.unwrap()).unwrap();
        assert_eq!(report.arch, "aarch64");
        // 실제 조회를 시도하지 않았어야 한다.
        assert!(exec.recorded_calls().is_empty());
    }

    #[tokio::test]
    async fn dry_run_without_labels_falls_back_to_default_assumption() {
        let exec = MockExecutor::new();
        let ctx = StepContext {
            dry_run: true,
            ..Default::default()
        };
        let out = CheckPrereqs::default().apply(&exec, &ctx).await.unwrap();
        let report: PrereqReport = serde_json::from_value(out.payload.unwrap()).unwrap();
        assert_eq!(report.arch, "x86_64");
        assert_eq!(report.os, "ubuntu");
    }

    #[tokio::test]
    async fn non_dry_run_against_bare_mock_executor_still_fails_validation() {
        // dry_run이 아니면(실 연결로 취급) 여전히 진짜 검증을 거쳐야 한다 —
        // 위 dry-run 폴백이 실제 실행의 fail-closed 성격을 약화시키지
        // 않았음을 확인.
        let exec = MockExecutor::new();
        let result = CheckPrereqs::default()
            .apply(&exec, &StepContext::default())
            .await;
        assert!(matches!(result, Err(StepError::PrereqFailed(_))));
    }

    // ── detect_prereq / assumed_prereq_from_labels (로드맵 #81) ─────────

    #[tokio::test]
    async fn detect_prereq_returns_real_values_from_executor() {
        let exec = MockExecutor::new();
        exec.expect_exec("cat /etc/os-release", "arch\n");
        exec.expect_exec("uname -m", "aarch64\n");
        exec.expect_exec("free -m", "8192\n");
        exec.expect_exec("df -BG", "50G\n");
        exec.expect_exec("which cargo", "");
        exec.expect_exec("pidof systemd", "1\n");

        let report = detect_prereq(&exec).await.unwrap();
        assert_eq!(report.os, "arch");
        assert_eq!(report.arch, "aarch64");
        assert_eq!(report.mem_mb, 8192);
    }

    #[tokio::test]
    async fn detect_prereq_propagates_check_prereqs_failure() {
        // 조용히 가정값으로 대체하지 않고 그대로 전파해야 한다.
        let exec = MockExecutor::new();
        exec.expect_exec("cat /etc/os-release", "ubuntu\n");
        exec.expect_exec("uname -m", "x86_64\n");
        exec.expect_exec("free -m", "16384\n");
        exec.expect_exec("df -BG", "200G\n");
        exec.expect_exec("which cargo", "");
        exec.expect_exec("pidof systemd", "");

        let result = detect_prereq(&exec).await;
        assert!(matches!(result, Err(StepError::PrereqFailed(_))));
    }

    #[test]
    fn assumed_prereq_from_labels_uses_arch_and_os_labels() {
        let mut labels = HashMap::new();
        labels.insert("arch".to_string(), "aarch64".to_string());
        labels.insert("os".to_string(), "debian".to_string());
        let report = assumed_prereq_from_labels(&labels);
        assert_eq!(report.arch, "aarch64");
        assert_eq!(report.os, "debian");
    }

    #[test]
    fn assumed_prereq_from_labels_falls_back_without_labels() {
        let report = assumed_prereq_from_labels(&HashMap::new());
        assert_eq!(report.arch, "x86_64");
        assert_eq!(report.os, "ubuntu");
    }
}
