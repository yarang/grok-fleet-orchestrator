---
type: architecture-index
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/agents/README.md"
last_verified: "2026-08-17"
---

# Agent 실행 플랫폼

Agent 실행 플랫폼의 진입점이다. 이 도메인은 Agent를 만들고 실행하는 계약만 다룬다.
Project·Task의 상위 lifecycle은 [교차 lifecycle](../project-task-agent-lifecycle.md), 권한과
시크릿 경계는 [보안 모델](../../security/control-plane-security-model.md)이 정본이다.

현재 코드에는 Agent 엔티티, 명령 ACK, 다중 runtime catalog, Agent API가 없다. 아래 문서는
모두 구현 전 목표 계약이며, 현재 코드 근거는 [구현 참조](../implementation-reference.md)에 둔다.

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
