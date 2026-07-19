//! Store 계층 에러 타입.

use thiserror::Error;

/// `fleet-store` 작업 중 발생하는 에러.
#[derive(Debug, Error)]
pub enum StoreError {
    /// 연결 실패, 풀 고갈, 네트워크 등.
    #[error("database connection error: {0}")]
    Connection(String),

    /// 행을 찾지 못함 (get_* 호출 시 None과 구분하기 위함은 아님 —
    /// None은 `Ok(None)`으로 반환. 이 에러는 예상치 못한 상황용).
    #[error("not found")]
    NotFound,

    /// UUID, JSON 역직렬화 등 디코딩 실패.
    #[error("decode error: {0}")]
    Decode(String),

    /// 고유 제약 위반 (예: 중복 워커 이름).
    #[error("conflict: {0}")]
    Conflict(String),

    /// 마이그레이션 실패.
    #[error("migration error: {0}")]
    Migration(String),

    /// sqlx 에러의 래핑.
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),

    /// serde_json 에러의 래핑.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// 부트스트랩 토큰이 존재하지 않거나 소진/만료됨 (Phase 8.3).
    #[error("bootstrap token invalid or exhausted: {0}")]
    BootstrapTokenInvalid(String),
}

/// Result alias.
pub type Result<T> = std::result::Result<T, StoreError>;
