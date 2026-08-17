---
type: architecture-index
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/architecture/README.md"
last_verified: "2026-08-17"
owners: ["architecture"]
---

# Architecture

시스템 내부 설계 결정과 구현 참조의 진입점이다. 외부 호출 표면은
[Contracts](../contracts/README.md), 운영 절차는 [Deployment](../deployment/README.md), 보안 정책은
[Security model](../security/control-plane-security-model.md)이 각각 소유한다.

## 읽기 순서

| 순서 | 문서 | 책임 |
|---:|---|---|
| 1 | [정본 지도](canonical-map.md) | 질문별 단일 정본 선택 |
| 2 | [시스템 개요](overview.md) | 시스템 경계와 현재 구현의 빠른 탐색 |
| 3 | [구현 참조](implementation-reference.md) | Rust 구성요소와 현재 제약 |
| 4 | 해당 정본 | control plane, Task, Project, lifecycle, routing, Agent 플랫폼의 결정 |

## 정본

| 주제 | 문서 |
|---|---|
| Control plane 운영 기관 | [control-plane-availability.md](control-plane-availability.md) |
| 실행 의미론 | [task-execution-consistency.md](task-execution-consistency.md) |
| Worker liveness | [worker-liveness-policy.md](worker-liveness-policy.md) |
| Project 관리 | [project-feature-design.md](project-feature-design.md) |
| Task 관리 | [task-management-design.md](task-management-design.md) |
| 교차 lifecycle | [project-task-agent-lifecycle.md](project-task-agent-lifecycle.md) |
| Agent 실행 플랫폼 | [agents/README.md](agents/README.md) |
| 지능형 routing | [intelligent-task-routing-and-budget-control-design.md](intelligent-task-routing-and-budget-control-design.md) |

## Derived와 기록

[system-entities-mapping.md](system-entities-mapping.md)는 엔티티 관계를 빠르게 보는 Derived
지도다. 비교·대안·feasibility 검토는 [Reviews](../reviews/README.md)에, 시간순 변경은
[architecture log](log.md)에 둔다. 이 문서들은 정본을 바꾸지 않는다.
