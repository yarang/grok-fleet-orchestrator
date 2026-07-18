//! API 에러 타입. axum의 `IntoResponse`로 JSON 응답으로 변환됨.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// API 처리 중 발생하는 에러.
#[derive(Debug, Error)]
pub enum ApiError {
    /// 요청 본문 파싱 실패, 필수 필드 누락 등.
    #[error("bad request: {0}")]
    BadRequest(String),

    /// 인증 실패 — 토큰 누락/만료/잘못됨.
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// 리소스를 찾을 수 없음 (예: 없는 worker_id).
    #[error("not found: {0}")]
    NotFound(String),

    /// 충돌 (예: 같은 name의 워커가 이미 online).
    #[error("conflict: {0}")]
    Conflict(String),

    /// Store 계층 에러.
    #[error("store error: {0}")]
    Store(String),

    /// 내부 서버 에러.
    #[error("internal: {0}")]
    Internal(String),
}

impl ApiError {
    /// HTTP 상태 코드 매핑.
    fn status(&self) -> StatusCode {
        use ApiError::*;
        match self {
            BadRequest(_) => StatusCode::BAD_REQUEST,
            Unauthorized(_) => StatusCode::UNAUTHORIZED,
            NotFound(_) => StatusCode::NOT_FOUND,
            Conflict(_) => StatusCode::CONFLICT,
            Store(_) | Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 머신 판독용 에러 코드 (클라이언트 분기용).
    fn code(&self) -> &'static str {
        use ApiError::*;
        match self {
            BadRequest(_) => "bad_request",
            Unauthorized(_) => "unauthorized",
            NotFound(_) => "not_found",
            Conflict(_) => "conflict",
            Store(_) => "store_error",
            Internal(_) => "internal_error",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Json(json!({
            "error": {
                "code": self.code(),
                "message": self.to_string(),
            }
        }));
        (status, body).into_response()
    }
}

impl From<fleet_store::StoreError> for ApiError {
    fn from(e: fleet_store::StoreError) -> Self {
        use fleet_store::StoreError as S;
        match e {
            S::NotFound => ApiError::NotFound("entity not found".into()),
            S::Conflict(msg) => ApiError::Conflict(msg),
            other => ApiError::Store(other.to_string()),
        }
    }
}
