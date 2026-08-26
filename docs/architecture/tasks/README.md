---
type: domain-index
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/architecture/tasks/README.md"
last_verified: "2026-08-17"
owners: ["architecture", "scheduler"]
---

# Task Architecture

Task 도메인의 내부 설계 정본 진입점이다. 외부 HTTP·MCP 호출 형식은
[Contracts](../../contracts/README.md)가, Project·Task·Agent의 교차 수명 전이는
[Lifecycle 계약](../project-task-agent-lifecycle.md)이 소유한다.

| 읽는 순서 | 정본 | 답하는 질문 |
|---:|---|---|
| 1 | [Task 관리](management.md) | 작업을 제출·의존성·취소·삭제·결과·감사까지 어떻게 관리하는가? |
| 2 | [실행 일관성](execution-consistency.md) | 상태 전이 CAS, 실패 처리, 멱등성, 부작용을 어떻게 안전하게 처리하는가? |
| 3 | [Routing 정책](routing-policy.md) | 논리 profile을 어떤 Worker 선택 규칙으로 해석하는가? |
| 4 | [예산 제어](budget-control.md) | usage budget과 routing telemetry 제한을 어떻게 적용하는가? |

문서 간 책임은 순방향이다. 관리 문서는 사용자 의도와 Task 수명을, 실행 일관성은
실행 시도를, routing은 실행 대상을, 예산 제어는 실행 중 한계를 소유한다. 같은 상태나
오류 규칙을 다른 문서에 복제하지 않는다.
