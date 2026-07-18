//! # fleet-mcp
//!
//! Model Context Protocol (MCP) 서버. stdio로 JSON-RPC 2.0을 처리하며,
//! AI 코딩 클라이언트(grok build, Claude Code, Cursor, Gemini CLI 등)에
//! Fleet 작업 디스패치 도구를 노출합니다.
//!
//! ## 지원 도구 (Phase 1)
//!
//! - `fleet_dispatch_task` — 작업 제출, `task_id` 반환
//! - `fleet_get_task_status` — 상태 조회 (논블로킹)
//! - `fleet_list_workers` — 등록된 워커 목록
//!
//! ## 프로토콜 호환성
//!
//! 모든 도구 이름은 `^[a-zA-Z_][a-zA-Z0-9_-]{0,63}$`을 준수하여
//! 크로스 클라이언트 호환성을 보장합니다.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

pub mod handlers;
pub mod schema;
pub mod server;

pub use server::{run_mcp_server, McpServer};
