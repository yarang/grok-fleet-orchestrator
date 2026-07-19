//! # fleet-api
//!
//! HTTP API 서버. MCP와 별개로 워커 관리(등록/하트비트) 및 admin용 엔드포인트를
//! 노출합니다.
//!
//! ## 엔드포인트
//!
//! | Method | Path                         | 용도                          |
//! |--------|------------------------------|-------------------------------|
//! | POST   | `/v1/workers/register`       | 워커 최초 등록 / 재연결        |
//! | POST   | `/v1/workers/heartbeat`      | 주기적 하트비트                |
//! | GET    | `/v1/workers`                | 워커 목록 (admin/CLI용)        |
//! | GET    | `/v1/workers/:id`            | 단일 워커 상세                  |
//! | DELETE | `/v1/workers/:id`            | 등록 해제                      |
//! | GET    | `/v1/health`                 | 헬스체크 (로드밸런서/프로브용)  |
//!
//! ## 인증
//!
//! Phase 3에서는 bearer token 헤더를 검사합니다. 개발 모드에서는
//! `--allow-no-auth`로 검증을 건너뜁니다. Phase 4에서 Cloudflare Access
//! 미들웨어로 교체됩니다.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

mod app;
mod cloudflare;
mod error;
mod handlers;
pub mod metrics;
mod schema;

#[cfg(test)]
mod test_support;

pub use app::{build_app, run_http_server, AppState};
pub use cloudflare::{cloudflare_access_middleware, VerifiedUser};
pub use error::ApiError;
pub use schema::{
    BootstrapTokenSummary, CreateBootstrapTokenRequest, CreateBootstrapTokenResponse, JoinRequest,
    JoinResponse,
};
