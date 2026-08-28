---
type: api-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/agent-management.md"
last_verified: "2026-08-28"
last_verified_commit: "working-tree"
owners: ["agent-platform", "api-contracts"]
---

# Agent 관리 계약

## 범위

이 계약은 Agent 생성·명령·상태 조회의 외부 표면을 정의한다. 상세 상태 전이는
[Agent 프로비저닝](../architecture/agents/provisioning.md), 권한은
[보안 모델](../security/control-plane-security-model.md)이 정본이다.

`#49` 1단계에서 **생성·목록·회수**만 MCP와 Dashboard에 구현됐다(아래 "구현된 표면"). 아래
"최소 표면" 표의 명령 요청 행과 `generation`·ACK는 여전히 미구현이며, transport와 concrete
schema 및 활성화 게이트가 승인되기 전에는 wire 호환성 약속이 아니다.

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
보안 게이트가 충족될 때 별도 capability로 추가한다. 1단계에서 `agent:attach`를 만들지 않은 것도
같은 이유다 — 붙을 세션도, step-up grant를 발급할 주체도 아직 없다.

## 구현된 표면 (`#49` 1단계)

| 작업 | MCP | Dashboard | capability |
|---|---|---|---|
| 생성 | `fleet_create_agent` | `POST /api/agents` | `agent:manage` |
| 목록 | `fleet_list_agents` | `GET /api/agents?project_id=` | `agent:read` |
| 회수 | `fleet_stop_agent` | `DELETE /api/agents/{id}` | `agent:manage` |

입력은 `project_id`, `name`, 선택 `description`이며 template/runtime 참조는 없다 — AgentTemplate
(`#86`)이 아직 없기 때문이다. 상태는 `ready`/`stopped` 둘뿐이라 응답에 `generation`도 마지막 ACK도
없다. 이름은 `(project_id, name)` 범위에서 유일하고, 중복은 `409`(MCP는 `invalid_params`)다.

`request_id` 멱등성은 **구현되지 않았다**. 대신 회수는 그 자체로 멱등이다 — 이미 `stopped`인
Agent를 다시 회수하면 쓰기를 건너뛰고 같은 `updated_at`을 반환한다. 생성의 중복 방지는
`request_id`가 아니라 이름 유일성 제약이 담당한다.

`project_id`를 바꾸는 표면은 만들지 않았다. "Agent 생성은 Project를 변경 가능한 placement 값으로
받지 않는다"는 위 문단이 구현에서는 **PATCH route의 부재**로 강제된다.

## 활성화 게이트

- HTTP, CLI 또는 MCP 중 노출 transport와 method·path·tool 이름 확정
- principal 종류, capability, Project membership 검사 시점 확정
- `request_id` 보존기간, canonical payload hash와 동시 중복 요청 결과 확정
- `401`, `429`, 재시도 가능한 `5xx`와 결과 불명 상태 계약
- 목록 표면이 추가되면 pagination과 version negotiation 계약

1단계는 이 게이트들을 **통과한 것이 아니라 우회한다** — 노출 transport(MCP·Dashboard)와 권한
검사 시점만 확정했고, `request_id`·`429`·결과 불명 상태·pagination 협상은 명령 계층(`#89`)이
생길 때 함께 정한다. 현재 목록은 limit/offset만 받으며 version negotiation이 없다.
