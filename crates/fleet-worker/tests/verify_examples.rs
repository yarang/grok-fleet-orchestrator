//! examples/ 디렉토리의 샘플 설정 파일이 실제 Rust 파서로 검증되는지 확인.
//!
//! 이 테스트는 `examples/worker.toml` 이 현재 스키마와 동기화되어 있는지
//! 지속적으로 검증한다. 새 필드를 config.rs 에 추가하거나 기본값을 바꾸면
//! 이 테스트가 실패하므로 examples/ 도 함께 업데이트해야 함.

use std::path::PathBuf;

#[test]
fn examples_worker_toml_parses_and_validates() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("worker.toml");
    assert!(path.exists(), "examples/worker.toml not found at {}", path.display());

    let cfg = fleet_worker::config::WorkerConfig::from_file(&path)
        .expect("examples/worker.toml must parse and pass validation");

    // 핵심 필드가 채워있는지 확인 (주석 처리된 placeholder 가 아닌 실제 값).
    assert!(!cfg.worker.name.is_empty());
    assert!(
        cfg.worker.orchestrator_url.starts_with("https://"),
        "example should use https:// for orchestrator_url"
    );
    assert!(cfg.grok.bin.starts_with('/'), "grok.bin should be absolute path");
    assert!(
        cfg.grok.secret.len() >= 16,
        "example secret should be at least 16 chars (placeholder is fine, empty is not)"
    );
    assert!(cfg.grok.max_concurrent_tasks >= 1);

    // mTLS 섹션은 명시적으로 활성화되어 있어야 한다 (예시 목적).
    let mtls = cfg
        .mtls
        .expect("examples/worker.toml should demonstrate mTLS section");
    assert!(mtls.enabled);
    assert!(!mtls.listen_addr.is_empty());
}
