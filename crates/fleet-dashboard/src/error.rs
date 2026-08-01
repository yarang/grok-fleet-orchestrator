//! 대시보드 JSON API 오류 타입.
//!
//! ## 왜 필요한가
//!
//! 대시보드 API 핸들러는 오류를 `StatusCode`로 반환해 왔는데, axum에서
//! `StatusCode`를 그대로 응답하면 **본문이 비어 있다**. 클라이언트는 403인지
//! 500인지만 알 뿐 이유를 알 수 없고, 프론트엔드는 상태 코드별로 문구를
//! 하드코딩해야 했다. 일부 핸들러는 `(StatusCode, String)`으로 평문을
//! 반환해서 같은 API 안에서도 형식이 제각각이었다.
//!
//! ## 형식
//!
//! `fleet-api`의 `ApiError`와 **동일한 와이어 포맷**을 사용한다:
//!
//! ```json
//! { "error": { "code": "not_found", "message": "worker not found" } }
//! ```
//!
//! 두 서비스가 같은 형식을 쓰므로 클라이언트가 오류 처리를 한 벌만 구현하면
//! 된다. 형식이 갈라지지 않도록 테스트로 고정해 두었다.
//!
//! HTML 페이지 핸들러(브라우저 내비게이션)는 이 타입을 쓰지 않는다 — 사람이
//! 읽을 오류 페이지를 반환하는 것이 맞다.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// 대시보드 JSON API 오류.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// 요청 파싱 실패, 잘못된 파라미터 등.
    #[error("bad request: {0}")]
    BadRequest(String),

    /// 인증되지 않음 (세션 없음/만료).
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// 인증되었으나 권한 부족 (RBAC 거부).
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// 리소스 없음.
    #[error("not found: {0}")]
    NotFound(String),

    /// 충돌 (중복 생성 등).
    #[error("conflict: {0}")]
    Conflict(String),

    /// Store 계층 오류.
    #[error("store error: {0}")]
    Store(String),

    /// 그 외 내부 오류.
    #[error("internal: {0}")]
    Internal(String),

    /// 일시적으로 처리 불가 (필수 설정 누락 등). 재시도 가치가 있음을 알린다.
    #[error("service unavailable: {0}")]
    Unavailable(String),
}

impl ApiError {
    /// HTTP 상태 코드 매핑.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Store(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// 머신 판독용 오류 코드. `fleet-api`의 코드값과 일치시킨다.
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Store(_) => "store_error",
            Self::Internal(_) => "internal_error",
            Self::Unavailable(_) => "unavailable",
        }
    }

    /// 권한 거부 오류 (RBAC 메시지 통일용).
    pub fn forbidden() -> Self {
        Self::Forbidden("permission denied".into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // 500번대는 내부 사정이 응답에 새지 않도록 서버 로그에만 상세를 남긴다.
        if self.status().is_server_error() {
            tracing::error!(code = self.code(), detail = %self, "dashboard api error");
        }
        let body = Json(json!({
            "error": {
                "code": self.code(),
                "message": self.to_string(),
            }
        }));
        (self.status(), body).into_response()
    }
}

impl From<fleet_store::StoreError> for ApiError {
    fn from(e: fleet_store::StoreError) -> Self {
        use fleet_store::StoreError as S;
        match e {
            S::NotFound => Self::NotFound("entity not found".into()),
            S::Conflict(msg) => Self::Conflict(msg),
            other => Self::Store(other.to_string()),
        }
    }
}

/// `StatusCode`를 반환하는 기존 헬퍼(`require_permission` 등)와의 호환 변환.
///
/// 이 변환이 있어야 `require_permission(&principal, perm)?` 같은 호출부를
/// 그대로 두고 반환 타입만 바꿀 수 있다.
impl From<StatusCode> for ApiError {
    fn from(status: StatusCode) -> Self {
        match status {
            StatusCode::BAD_REQUEST => Self::BadRequest("invalid request".into()),
            StatusCode::UNAUTHORIZED => Self::Unauthorized("authentication required".into()),
            StatusCode::FORBIDDEN => Self::Forbidden("permission denied".into()),
            StatusCode::NOT_FOUND => Self::NotFound("resource not found".into()),
            StatusCode::CONFLICT => Self::Conflict("conflict".into()),
            other => Self::Internal(format!("unexpected status {other}")),
        }
    }
}

/// 대시보드 API 핸들러의 표준 결과 타입.
pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_json(err: ApiError) -> (StatusCode, serde_json::Value) {
        let resp = err.into_response();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn error_body_shape_matches_fleet_api() {
        let (status, body) = body_json(ApiError::NotFound("worker xyz".into())).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // fleet-api와 동일한 봉투: { "error": { "code", "message" } }
        assert_eq!(body["error"]["code"], "not_found");
        assert_eq!(body["error"]["message"], "not found: worker xyz");
        assert!(
            body.get("error").is_some() && body.as_object().unwrap().len() == 1,
            "최상위에는 error 키만 있어야 한다: {body}"
        );
    }

    #[tokio::test]
    async fn status_and_code_mapping() {
        for (err, want_status, want_code) in [
            (
                ApiError::BadRequest("x".into()),
                StatusCode::BAD_REQUEST,
                "bad_request",
            ),
            (
                ApiError::Unauthorized("x".into()),
                StatusCode::UNAUTHORIZED,
                "unauthorized",
            ),
            (
                ApiError::Forbidden("x".into()),
                StatusCode::FORBIDDEN,
                "forbidden",
            ),
            (
                ApiError::NotFound("x".into()),
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                ApiError::Conflict("x".into()),
                StatusCode::CONFLICT,
                "conflict",
            ),
            (
                ApiError::Store("x".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_error",
            ),
            (
                ApiError::Internal("x".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            ),
        ] {
            let code = err.code();
            let (status, body) = body_json(err).await;
            assert_eq!(status, want_status);
            assert_eq!(code, want_code);
            assert_eq!(body["error"]["code"], want_code);
        }
    }

    /// `require_permission`이 돌려주는 `StatusCode::FORBIDDEN`이 403 + forbidden
    /// 코드로 이어져야 한다 (기존 호출부를 그대로 두기 위한 변환).
    #[tokio::test]
    async fn status_code_conversion_preserves_meaning() {
        let err: ApiError = StatusCode::FORBIDDEN.into();
        let (status, body) = body_json(err).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "forbidden");
    }

    #[test]
    fn store_not_found_maps_to_404() {
        let err: ApiError = fleet_store::StoreError::NotFound.into();
        assert_eq!(err.status(), StatusCode::NOT_FOUND);
        assert_eq!(err.code(), "not_found");
    }
}
