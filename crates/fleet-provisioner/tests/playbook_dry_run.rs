//! Playbook dry-run 통합 테스트.
//!
//! `MockExecutor`를 사용해 Playbook의 dry-run 시나리오를 검증합니다.
//! 실제 SSH 연결 없이 Playbook 로직과 스텝 시퀀스를 확인.

use std::sync::Arc;

use fleet_provisioner::{
    Inventory, MockExecutor, Playbook, PlaybookContext, PrereqReport, RemoteExecutor, StepContext,
};

fn ubuntu_prereq() -> PrereqReport {
    PrereqReport {
        os: "ubuntu".into(),
        arch: "x86_64".into(),
        mem_mb: 16384,
        disk_gb: 100,
        has_rust: false,
        has_systemd: true,
    }
}

/// check_prereqs 통과를 위한 MockExecutor 설정.
fn healthy_executor() -> MockExecutor {
    let exec = MockExecutor::new();
    exec.expect_exec("cat /etc/os-release", "ubuntu\n");
    exec.expect_exec("uname -m", "x86_64\n");
    exec.expect_exec("free -m", "16384\n");
    exec.expect_exec("df -BG", "100G\n");
    exec.expect_exec("which cargo", "/home/user/.cargo/bin/cargo\n");
    exec.expect_exec("pidof systemd", "1\n");
    exec
}

#[tokio::test]
async fn dry_run_playbook_completes_without_network_calls() {
    let exec: Arc<dyn RemoteExecutor> = Arc::new(healthy_executor());
    let pb = Playbook::dry_run(&ubuntu_prereq());
    let ctx = PlaybookContext::new(StepContext {
        worker_name: "build-1".into(),
        orchestrator_url: "https://orch.example.com".into(),
        cf_token: Some("tok".into()),
        fleet_worker_bin: Some("/tmp/fleet-worker".into()),
        dry_run: true,
        ..Default::default()
    });
    let report = pb.run(exec.as_ref(), &ctx).await.expect("playbook failed");
    // dry-run에서 check_prereqs는 항상 실행되므로 실제 exec 호출이 있음.
    // 하지만 deps, cloudflared, fleet_worker는 dry_run 분기로 호출 없음.
    assert!(report.succeeded);
}

#[tokio::test]
async fn standard_playbook_has_six_steps_in_order() {
    let exec: Arc<dyn RemoteExecutor> = Arc::new(healthy_executor());
    let pb = Playbook::standard(&ubuntu_prereq());
    let ctx = PlaybookContext::new(StepContext {
        dry_run: true,
        cf_token: Some("t".into()),
        fleet_worker_bin: Some("/tmp/x".into()),
        orchestrator_url: "https://x".into(),
        worker_name: "w".into(),
        ..Default::default()
    });
    let report = pb.run(exec.as_ref(), &ctx).await.unwrap();
    let names: Vec<&str> = report.steps.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "check_prereqs",
            "install_deps",
            "install_grok",
            "install_cloudflared",
            "install_fleet_worker",
            "start_services",
        ]
    );
}

#[tokio::test]
async fn tags_filter_limits_steps() {
    let exec = healthy_executor();
    let pb = Playbook::standard(&ubuntu_prereq());
    let ctx = PlaybookContext::new(StepContext {
        worker_name: "w".into(),
        dry_run: true,
        ..Default::default()
    })
    .with_tags(vec!["check".into()]);
    let report = pb.run(&exec, &ctx).await.unwrap();
    // check 태그를 가진 스텝만 실행되어야 함.
    assert!(report.steps.iter().all(|s| s.name == "check_prereqs"));
    assert_eq!(report.steps.len(), 1);
}

#[tokio::test]
async fn inventory_dry_run_parses_sample() {
    let yaml = r#"
defaults:
  user: ubuntu
  ssh_key: ~/.ssh/test_key
workers:
  - host: 10.0.0.1
    name: build-1
    labels:
      arch: arm64
  - host: 10.0.0.2
    name: build-2
options:
  orchestrator_url: https://orch.example.com
  dry_run: true
"#;
    let inv = Inventory::parse(yaml).unwrap();
    assert_eq!(inv.workers.len(), 2);
    assert!(inv.options.dry_run);

    // 각 워커에 대해 dry-run playbook 실행 가능 확인.
    for w in &inv.workers {
        let exec: Arc<dyn RemoteExecutor> = Arc::new(healthy_executor());
        let ctx = PlaybookContext::new(StepContext {
            worker_name: w.name.clone(),
            labels: w.labels.clone(),
            dry_run: true,
            ..Default::default()
        });
        let pb = Playbook::dry_run(&ubuntu_prereq());
        let report = pb.run(exec.as_ref(), &ctx).await.unwrap();
        assert!(report.succeeded, "worker {} failed", w.name);
    }
}

#[tokio::test]
async fn failing_prereq_aborts_playbook() {
    // systemd가 없는 환경 시뮬레이션.
    let exec = MockExecutor::new();
    // check_prereqs가 응답하지 않는 명령들에 대해 빈 응답 반환.
    // pidof systemd → "" → 실패.
    let pb = Playbook::standard(&PrereqReport {
        os: "ubuntu".into(),
        arch: "x86_64".into(),
        mem_mb: 16384,
        disk_gb: 100,
        has_rust: false,
        has_systemd: false, // 이 값이 step에 영향을 주지 않음 (step이 직접 검사).
    });
    let ctx = PlaybookContext::new(StepContext::default());
    let result = pb.run(&exec, &ctx).await;
    // systemd가 빈 응답이므로 check_prereqs 실패.
    assert!(result.is_err());
}
