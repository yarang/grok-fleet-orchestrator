---
type: architecture-index
authority: canonical
implementation: partial
verification: design-reviewed
source: "docs/architecture/agents/README.md"
last_verified: "2026-08-29"
last_verified_commit: "working-tree"
---

# Agent 실행 플랫폼

Agent 실행 플랫폼의 진입점이다. 이 도메인은 Agent를 만들고 실행하는 계약만 다룬다.
Project·Task의 상위 lifecycle은 [교차 lifecycle](../project-task-agent-lifecycle.md), 권한과
시크릿 경계는 [보안 모델](../../security/control-plane-security-model.md)이 정본이다.

현재 코드에는 Agent **엔티티와 그 관리 표면**이 있다(`#49` 1단계, 2026-08-28): `agents` 테이블,
`fleet-core/src/agent.rs`, MCP `fleet_create_agent`/`fleet_list_agents`/`fleet_stop_agent`,
Dashboard `GET|POST /api/agents`와 `DELETE /api/agents/{id}`, capability `agent:read`/`agent:manage`.
없는 것은 **그 엔티티 뒤의 실행**이다 — 9-필드 명령 봉투와 ACK, 다중 runtime catalog,
terminal attach, 장기 메모리는 한 줄도 구현되지 않았다. 그래서 `AgentStatus`는 목표 8-상태가
아니라 `Ready`/`Stopped` 2-상태다.

봉투 필드 중 이름이 겹치는 것 하나를 구분해 둔다: `tasks.dispatch_control_epoch`는 `#67` 1단계로
실재하지만 그것은 **Task dispatch 세대**이지 Agent 명령의 `control_epoch`가 아니다. Agent 쪽
`generation`·`control_epoch`·`worker_incarnation`·`fencing_token`은 넷 다 없다.

구현된 범위와 유예 목록의 정본은 [프로비저닝](provisioning.md)의 "구현 상태" 절이다
(하위 설계 문서 중에서는 그 문서만 `implementation: partial`이고 나머지 일곱은 `proposed`다).
코드 근거 전반은 [구현 참조](../implementation-reference.md)에 둔다.

## 읽기 순서

| 순서 | 문서 | 담당 책임 |
|---|---|---|
| 1 | [배치·맥락 계약](../entity-placement-and-context.md) | Worker daemon, Agent process, durable context, WarmIdle의 관계 |
| 2 | [실행 격리](execution-isolation.md) | 위험도에 따른 host/container 결정과 cleanup 경계 |
| 3 | [프로비저닝](provisioning.md) | 생성·명령·ACK·회수·재조정 상태 전이 |
| 4 | [런타임 어댑터](runtime-adapters.md) | 허용된 runtime의 실행·능력 선언·입출력 경계 |
| 5 | [하네스 구성](harness-composition.md) | prompt·Skill·tool의 immutable 실행 snapshot |
| 6 | [도구 카탈로그](tool-catalog.md) | 도구 정의·권한·비밀값 전달 금지 |
| 7 | [컨텍스트와 메모리](context-and-memory.md) | thread context와 장기 메모리의 분리 |
| 8 | [터미널 접근](terminal-access.md) | capture와 interactive attach의 별도 보안 게이트 |

호출 표면은 [Agent 관리 계약](../../contracts/agent-management.md)을 따른다. 각 문서를 동시에
수정해 같은 규칙을 복제하지 않는다.
