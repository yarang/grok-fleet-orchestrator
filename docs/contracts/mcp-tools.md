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

## 전송과 오류

현재 구현은 MCP protocol `2024-11-05`의 stdio JSON-RPC 2.0을 사용한다. `initialize`,
`tools/list`, `tools/call`을 지원하며, `tools/list`의 JSON Schema가 필드의 기계 판독
정본이다. JSON-RPC 수준에서는 parse `-32700`, invalid request `-32600`, method not found
`-32601`, invalid params `-32602`, internal `-32603`을 사용한다. 도구의 논리적 실패는
정상 JSON-RPC 응답 안에서 `isError: true`로 반환된다.

## 범위

- MCP stdio transport와 `tools/list`, `tools/call`의 Fleet 구현
- Task, Worker, Host, bootstrap token 관련 도구의 입력·출력 스키마
- MCP 호출의 인증·권한 경계

HTTP `/v1`이나 Dashboard `/api/*`는 이 문서의 범위 밖이다. 이전 MCP 상세 문서는 삭제됐으며,
도구의 현재 wire schema는 코드와 이 문서의 연결된 정본을 따른다.

## 현재 도구 표면

| 도구 | 주요 입력 | 결과 범주 |
|---|---|---|
| `fleet_dispatch_task` | `prompt`, 선택 `cwd`, `model`, `server_hint`, labels, turn·timeout | 비동기 `task_id` |
| `fleet_get_task_status` | `task_id` | Task 상태·결과 |
| `fleet_list_tasks` | 선택 status, limit, offset | Task 목록 |
| `fleet_cancel_task` | `task_id`, 선택 reason | 취소 결과 |
| `fleet_wait_for_task` | `task_id`, 선택 timeout | terminal Task 또는 timeout 오류 |
| `fleet_stream_task_output` | `task_id`, offset, polling 설정 | 출력 chunk와 현재 상태 |
| `fleet_collect_results` | `task_ids` | 여러 Task 결과 |
| `fleet_list_workers` | 선택 status, labels, limit | Worker 목록 |
| `fleet_list_hosts` | 선택 status | Host 목록 |
| `fleet_reset_worker_breaker` | Worker 식별자 | circuit breaker reset 결과 |
| `fleet_list_bootstrap_tokens` | 입력 없음 | token 메타데이터 목록 |
| `fleet_revoke_bootstrap_token` | `token_id` | revoke 결과 |

Project와 Agent 관리 도구는 제안 계약일 뿐 현재 `tools/list`에 포함되지 않는다. 새 도구는
여기 표, `tools/list` schema, handler 테스트를 한 변경으로 갱신한다.

## 보안 상태

현재 MCP capability 모델은 목표 보안 계약을 완전히 구현하지 않는다. capability, project scope,
fail-closed 정책은 [control-plane security model](../security/control-plane-security-model.md)을 따르며,
구현 전에는 도구가 그 정책을 보장한다고 서술하지 않는다.
