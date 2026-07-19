//! examples/workers.yaml 인벤토리가 실제 파서로 검증되는지 확인.
//!
//! `crates/fleet-provisioner/src/inventory.rs::Inventory` 스키마가
//! 바뀌면 examples/workers.yaml 도 함께 업데이트해야 한다.

use std::path::PathBuf;

#[test]
fn examples_workers_yaml_parses() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("workers.yaml");
    assert!(
        path.exists(),
        "examples/workers.yaml not found at {}",
        path.display()
    );

    let inv = fleet_provisioner::inventory::Inventory::from_file(&path)
        .expect("examples/workers.yaml must parse");

    // 공통 기본값.
    assert!(!inv.defaults.user.is_empty(), "defaults.user must be set");
    assert!(inv.defaults.ssh_port > 0);

    // 각 워커 엔트리의 필수 필드.
    assert!(
        !inv.workers.is_empty(),
        "example should include at least 1 worker"
    );
    for w in &inv.workers {
        assert!(!w.host.is_empty(), "worker.host must be set");
        assert!(!w.name.is_empty(), "worker.name must be set");
        assert!(
            w.grok_secret.is_some(),
            "worker {} must have grok_secret (examples use placeholder)",
            w.name
        );
    }

    // 옵션 섹션.
    assert!(
        inv.options.orchestrator_url.is_some(),
        "options.orchestrator_url should be set in the example"
    );
    assert!(inv.options.parallel >= 1);
}
