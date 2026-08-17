---
type: architecture-reference
authority: derived
implementation: partial
verification: code-checked
source: "docs/architecture/system-entities-mapping.md"
last_verified: "2026-08-17"
owners: ["architecture"]
---

# 시스템 엔티티 관계 참조

이 문서는 현재 엔티티와 목표 엔티티의 관계를 빠르게 찾는 Derived 지도다. 관계의 규칙은
[정본 지도](canonical-map.md)가 가리키는 문서가 소유한다.

```mermaid
flowchart LR
    Project["Project\n목표 정책 경계"] --> Task["Task\n현재 저장·dispatch 단위"]
    Task --> Worker["Worker\n현재 실행 대상"]
    Worker --> Host["Host\n등록·인벤토리"]
    Agent["Agent\n목표 실행 인스턴스"] --> Task
    Attempt["TaskAttempt\n목표 실행 기록"] --> Task
```

| 엔티티 | 현재 상태 | 정본 |
|---|---|---|
| Task | 현재 `fleet-core`와 Store에 존재하며 Worker dispatch 단위다 | [Task management](task-management-design.md) |
| Worker | 현재 등록·heartbeat·model/label/capacity 정보를 가진 실행 대상이다 | [Worker liveness](worker-liveness-policy.md) |
| Host | 현재 등록 표면이 있으며 inventory 경계다 | [HTTP contract](../contracts/http-api.md) |
| Project | 목표 정책·권한·배정 경계다 | [Project model](project-feature-design.md) |
| TaskAttempt | 목표 멱등성·재시도·부작용 기록이다 | [Execution consistency](task-execution-consistency.md) |
| Agent | 목표 장기 실행 인스턴스다 | [Agent domain](agents/README.md) |
| Skill·Tool·Memory | 목표 harness 입력과 capability 경계다 | [Agent harness](agents/harness-composition.md) |

Project/Agent/TaskAttempt 및 이들의 hard isolation은 현재 구현된 데이터 모델로 간주하지 않는다.
상태, 권한, prompt 조립, scheduler 필터를 이 문서에 복제하지 않는다.
