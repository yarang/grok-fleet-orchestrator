//! # fleet-dashboard
//!
//! 브라우저 기반 실시간 현황 보드. React 빌드 파이프라인 없이 **순수 HTML + htmx**로
//! 구현하여 단일 러스트 바이너리에 정적 자산을 임베드합니다.
//!
//! ## 엔드포인트
//!
//! | Method | Path                  | 용도                                |
//! |--------|-----------------------|-------------------------------------|
//! | GET    | `/`                   | 대시보드 HTML                        |
//! | GET   | `/api/overview`        | 요약 통계 (worker/task 카운트)        |
//! | GET   | `/api/workers`         | 워커 목록 JSON                       |
//! | GET   | `/api/tasks`           | 작업 목록 JSON                       |
//! | GET   | `/api/events`          | 이벤트 로그 (페이지네이션)            |
//! | GET   | `/api/events/stream`   | SSE 실시간 이벤트 스트리밍            |
//! | GET   | `/health`              | 헬스체크                            |
//! | GET   | `/static/*`            | 임베드된 정적 자산 (CSS/JS)           |

#![forbid(unsafe_code)]
#![allow(missing_docs)]

pub mod app;
pub mod assets;
pub mod auth;
pub mod auth_util;
pub mod handlers;
pub mod schema;
pub mod sse;

pub use app::{build_dashboard_app, run_dashboard_server, DashboardState};
pub use auth::{AuthPrincipal, SESSION_COOKIE, SESSION_DURATION_SECS};
pub use schema::{OverviewResponse, TaskSummary, WorkerSummary};
