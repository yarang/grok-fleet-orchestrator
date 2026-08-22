---
type: proposed-contract
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/contracts/agent-management.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["agent-platform", "api-contracts"]
---

# Agent 관리 계약

## 범위

이 계약은 Agent 생성·명령·상태 조회의 외부 표면을 정의한다. 상세 상태 전이는
[Agent 프로비저닝](../architecture/agents/provisioning.md), 권한은
[보안 모델](../security/control-plane-security-model.md)이 정본이다. 현재 이 API, CLI, MCP,
Dashboard route는 구현되지 않았다. 이 문서는 Roadmap #49에 등록된 인터페이스 요구사항이며,
transport와 concrete schema 및 아래 게이트가 승인되기 전에는 wire 호환성 약속이 아니다.

## 최소 표면

| 작업 | 입력 | 성공 결과 | 실패 |
|---|---|---|---|
| Agent 생성 요청 | immutable Project 범위, template/runtime 참조, `request_id` | `agent_id`, `generation`, `Ready` | 권한 403, 범위/정책 409, 검증 422 |
| 명령 요청 | `agent_id`, 명령, `request_id`, 기대 generation | 새 generation과 명령 상태 | stale 409, 만료 409 |
| 상태 조회 | `agent_id`, Project 범위 | 상태·generation·마지막 ACK·감사 참조 | 권한 403, 없음 404 |

`request_id`는 같은 principal·동일 payload hash일 때만 같은 결과를 반환한다. 다른 payload hash는
409이다. Worker ACK와 cleanup 증거는 내부 control 계약으로, 외부 클라이언트가 임의로 확정할 수 없다.

Agent 생성은 Project를 변경 가능한 placement 값으로 받지 않는다. Agent는 하나의 immutable
Project 범위에 속하고, 실제 Worker placement와 Agent process activation은 control plane이
Project agent 상한·capability·isolation을 검증해 결정한다. Task terminal 뒤 process는 기본적으로
종료·hibernation하며, WarmIdle은 외부 클라이언트가 강제할 수 없는 정책 기반 lease다.

capture와 attach는 이 계약의 부속 기능이 아니며 [터미널 접근](../architecture/agents/terminal-access.md)의
보안 게이트가 충족될 때 별도 capability로 추가한다.

## 활성화 게이트

- HTTP, CLI 또는 MCP 중 노출 transport와 method·path·tool 이름 확정
- principal 종류, capability, Project membership 검사 시점 확정
- `request_id` 보존기간, canonical payload hash와 동시 중복 요청 결과 확정
- `401`, `429`, 재시도 가능한 `5xx`와 결과 불명 상태 계약
- 목록 표면이 추가되면 pagination과 version negotiation 계약
