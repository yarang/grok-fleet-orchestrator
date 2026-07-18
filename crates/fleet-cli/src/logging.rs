//! tracing 초기화. stderr로 로깅 — stdout은 MCP JSON-RPC가 독점.

use tracing_subscriber::EnvFilter;

/// 로깅 초기화. `log_level`은 기본 필터이나 `RUST_LOG`가 있으면 덮어씀.
pub fn init(log_level: &str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        // stdout은 MCP JSON-RPC가 사용 — 로그는 반드시 stderr.
        .with_writer(std::io::stderr)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();
}
