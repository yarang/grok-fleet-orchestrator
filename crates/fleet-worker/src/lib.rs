//! # fleet-worker
//!
//! 워커 노드에서 실행되는 데몬. 두 가지 핵심 책임:
//!
//! 1. **grok 서브프로세스 관리** — `grok agent serve`를 백그라운드에서 실행하고
//!    비정상 종료 시 재시작. 헬스체크는 포트 점검으로 수행.
//!
//! 2. **오케스트레이터 등록/하트비트** — 시작 시 `POST /v1/workers/register`로
//!    자신을 등록하고, 주기적으로 `POST /v1/workers/heartbeat`로 상태 전송.
//!
//! ## 아키텍처
//!
//! ```text
//! [fleet-worker 프로세스]
//!   │
//!   ├── GrokRunner (백그라운드 태스크)
//!   │     └── grok agent serve --bind 127.0.0.1:2419 --secret ...
//!   │           (비정상 종료 시 5초 후 재시작)
//!   │
//!   ├── RegistrationClient (백그라운드 태스크)
//!   │     ├── register (1회, 재시도 포함)
//!   │     └── heartbeat 루프 (15초 간격)
//!   │
//!   └── 신호 핸들러 (SIGTERM/SIGINT)
//!         └── grok 종료 + 등록 해제 (best-effort)
//! ```

#![forbid(unsafe_code)]

pub mod config;
pub mod error;
pub mod grok_process;
pub mod join;
pub mod registration;
pub mod runner;

pub use config::WorkerConfig;
pub use error::WorkerError;
pub use join::JoinArgs;
pub use registration::RegistrationClient;
pub use runner::WorkerRunner;

/// tracing-subscriber 초기화. 환경변수 `RUST_LOG`가 없으면 `info` 레벨 적용.
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,fleet_worker=debug"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();
}
