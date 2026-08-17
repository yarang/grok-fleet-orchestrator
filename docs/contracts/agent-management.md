---
type: contract
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/contracts/agent-management.md"
last_verified: "2026-08-17"
---

# Agent 관리 계약

## 범위

이 계약은 Agent 생성·명령·상태 조회의 외부 표면을 정의한다. 상세 상태 전이는
[Agent 프로비저닝](../architecture/agents/provisioning.md), 권한은
[보안 모델](../security/control-plane-security-model.md)이 정본이다. 현재 이 API, CLI, MCP,
Dashboard route는 구현되지 않았다.

## 최소 표면

| 작업 | 입력 | 성공 결과 | 실패 |
|---|---|---|---|
| Agent 생성 요청 | Project 범위, template/runtime 참조, `request_id` | `agent_id`, `generation`, `Requested` | 권한 403, 범위/정책 409, 검증 422 |
| 명령 요청 | `agent_id`, 명령, `request_id`, 기대 generation | 새 generation과 명령 상태 | stale 409, 만료 409 |
| 상태 조회 | `agent_id`, Project 범위 | 상태·generation·마지막 ACK·감사 참조 | 권한 403, 없음 404 |

`request_id`는 같은 principal·동일 payload hash일 때만 같은 결과를 반환한다. 다른 payload hash는
409이다. Worker ACK와 cleanup 증거는 내부 control 계약으로, 외부 클라이언트가 임의로 확정할 수 없다.

capture와 attach는 이 계약의 부속 기능이 아니며 [터미널 접근](../architecture/agents/terminal-access.md)의
보안 게이트가 충족될 때 별도 capability로 추가한다.
