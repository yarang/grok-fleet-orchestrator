---
type: protocol-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/mcp-tools.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["fleet-mcp"]
---

# MCP 도구 계약

이 문서는 `fleet serve`가 stdio JSON-RPC로 노출하는 MCP 도구의 정본 진입점이다. 실제 도구
이름·입력 스키마·응답은 [`crates/fleet-mcp/src/schema.rs`](../../crates/fleet-mcp/src/schema.rs)와
handler 테스트를 기준으로 한다.

## 범위

- MCP stdio transport와 `tools/list`, `tools/call`의 Fleet 구현
- Task, Worker, Host, bootstrap token 관련 도구의 입력·출력 스키마
- MCP 호출의 인증·권한 경계

HTTP `/v1`이나 Dashboard `/api/*`는 이 문서의 범위 밖이다. 이전 MCP 상세 문서는 삭제됐으며,
도구의 현재 wire schema는 코드와 이 문서의 연결된 정본을 따른다.

## 보안 상태

현재 MCP capability 모델은 목표 보안 계약을 완전히 구현하지 않는다. capability, project scope,
fail-closed 정책은 [control-plane security model](../security/control-plane-security-model.md)을 따르며,
구현 전에는 도구가 그 정책을 보장한다고 서술하지 않는다.
