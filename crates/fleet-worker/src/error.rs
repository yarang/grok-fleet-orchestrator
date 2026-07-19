//! `fleet-worker` 데몬 에러 타입.

use thiserror::Error;

/// 워커 데몬 실행 중 발생하는 에러.
#[derive(Debug, Error)]
pub enum WorkerError {
    /// 설정 파일 읽기/파싱 실패.
    #[error("config error: {0}")]
    Config(String),

    /// 설정 파일 I/O 에러.
    #[error("config io: {0}")]
    ConfigIo(#[from] std::io::Error),

    /// grok 서브프로세스 시작 실패.
    #[error("grok subprocess failed: {0}")]
    GrokSubprocess(String),

    /// orchestrator API 호출 실패.
    #[error("orchestrator api: {0}")]
    OrchestratorApi(String),

    /// HTTP 요청 에러.
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),

    /// TOML 파싱 에러.
    #[error("toml parse: {0}")]
    Toml(#[from] toml::de::Error),

    /// 기타.
    #[error("{0}")]
    Other(String),
}
