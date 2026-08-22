---
type: architecture
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/project-feature-design.md"
last_verified: "2026-08-17"
---

# Project 모델과 거버넌스

Project는 개발 목표, 권한, 정책, Agent 소유를 묶는 경계다. Host와 Worker는 Fleet 공유 실행 풀이고 Project가 물리 자원을 예약하지 않는다. 이 문서는 Project의 데이터 모델, Agent admission 정책, 디스패치 자격, 권한 경계를 소유한다. 구현 상태는 **부분 구현**이다. `ProjectId`, `tasks.project_id` 저장, CLI/MCP 요청 전달은 구현되었지만 Project 엔터티와 정책 enforcement는 아직 구현되지 않았다.

## 범위

| 이 문서가 소유 | 다른 정본이 소유 |
|---|---|
| Project 데이터·정책 revision·Agent 소유 | [Task 관리](tasks/management.md)의 제출·결과·감사 |
| Agent admission과 shared Worker eligibility | [배치·맥락 계약](entity-placement-and-context.md)의 Host·Worker·Agent placement |
| Project Task의 Worker 선택 자격 | [실행 일관성](tasks/execution-consistency.md)의 Attempt·재시도·dead-letter |
| Project 권한의 승인 조건 | [Project 관리 계약](../contracts/project-management.md)의 HTTP·MCP 표면 |

화면 구성은 [UI Dashboard](../ui-dashboard/ui-design.md)가, Agent 생성·중지는 [Agent provisioning](agents/provisioning.md)이 소유한다. 이 문서는 SQL, Store 메서드 시그니처, 화면 명세, 구현 단계별 작업 목록을 반복하지 않는다.

```mermaid
flowchart LR
    Project["Project 모델·정책·소유"] --> Task["Task 관리"]
    Project --> Provisioning["Agent provisioning"]
    Project --> Selector["Worker 선택"]
    Task --> Attempt["실행 일관성"]
    Lifecycle["Lifecycle 계약"] -. "상태 전이" .-> Project
    Selector --> Attempt
```

## 목표 데이터 모델

| 대상 | 목표 필드·관계 | 의미 |
|---|---|---|
| `projects` | `id`, 고유 `name`, `description`, `created_by`, `max_active_agents`, `max_warm_agents`, worker eligibility selector, 정책 revision, `retention_policy_id`, `retain_until`, 생성·갱신 시각 | Project의 지속적 정책 경계 |
| `tasks` | nullable `project_id` | 값이 없으면 일반 풀 Task |
| `agents` | immutable `project_id`, role/context, 상태 | Project가 소유하는 논리 실행 주체 |
| `worker_execution_leases` | `agent_id`, `worker_id`, generation, 상태, 시각 | 활성 Agent의 일시적 Worker slot 점유 |
| `project_archive_holds` | `project_id`, kind, reason, opened/resolved 시각, actor, evidence | effect·cleanup·security/legal hold가 archive를 막는 기록 |

Agent provisioning 관련 기본 템플릿, 유휴 시간, 작업 디렉터리 같은 설정은 Project가 정책 값으로 제공할 수 있지만, Agent 템플릿과 실행 수명은 Agent 도메인이 소유한다. 정책이 바뀌면 revision을 올리고 새 Task/Attempt에만 적용한다. 이미 실행 중인 Attempt는 제출 시점 snapshot을 유지한다.

Agent는 Project를 상속하는 임시 Worker process가 아니라 immutable `project_id`를 가진 논리
엔티티다. Project가 Agent의 warm idle 정책과 agent 수 상한을 허용할 수는 있지만,
Project가 Task 하나를 제출했다고 Agent process를 계속 상주시켜서는 안 된다. 배치·context
보존·archive 뒤 Worker 처리의 정본은 [Entity placement & context](entity-placement-and-context.md)다.

## 공유 실행 풀 불변식

- Host와 Worker에는 `project_id`를 두지 않는다. 하나의 Worker는 시간에 따라 여러 Project의 Agent를 실행할 수 있다.
- Agent는 immutable `project_id`를 가진다. 실행 시에만 Worker lease를 얻고, lease 종료 후 Worker는 다른 Project에 즉시 사용될 수 있다.
- Project의 `max_active_agents`/`max_warm_agents`와 Worker의 `max_agent_processes`를 모두 만족해야 activation할 수 있다. 기본은 1/0이다.
- `max_active_agents`는 `Starting|Running`, `max_warm_agents`는 `WarmIdle`만 세며, WarmIdle도 Worker process slot을 점유한다. Project가 허용하지 않으면 Task 종료 뒤 항상 Hibernated다.
- Worker 선택은 Project가 허용한 capability label·격리 class를 만족해야 한다. GPU처럼 특수한 실행 환경은 raw resource allocation 대신 operator 관리 capability class로 선택한다.
- 여러 Project가 실행되는 Host에서는 `container_required` 또는 동등 이상의 격리가 필수다. `host_trusted`는 운영상 단일 Project만 실행 중인 Host에만 허용한다.

## 디스패치 자격

`task.project_id`가 있으면 해당 Project의 Agent와 정책을 통해서만 실행한다. 후보 Worker는 Project capability·격리·slot 조건으로 선택하며, 후보가 없더라도 일반 풀 context로 폴백하지 않는다. 이 경우의 재시도, 최종 실패, dead-letter 처리는 [실행 일관성](tasks/execution-consistency.md)의 `WorkerUnavailable` 경로를 따른다. `project_id`가 없는 Task는 기존 일반 풀 선택 규칙을 그대로 따른다.

Project가 `Draining`이면 새 Task, 새 Agent, 새 자원 배정을 받지 않는다. `Archived` 전이와 보존·정리 순서는 [Lifecycle 계약](project-task-agent-lifecycle.md)을 따른다.

## 권한과 구현 차단 조건

Project 권한 종류는 `project:create`, `project:read`, `project:update`, `project:delete`, `project:policy_manage`다. `ProjectRead`는 Project 범위 내 읽기만 허용한다. 생성·삭제와 정책 관리의 실제 역할 배정은 아직 승인된 계약이 아니다.

특히 Project 정책 변경과 Task 생성이 자동 Agent provisioning을 통해 `AgentCreate`를 우회할 가능성이 있다. 따라서 다음이 확정되고 검증되기 전에는 정책 변경 API나 MCP 도구를 구현하거나 활성화하지 않는다.

1. `ProjectPolicyManage` 역할과 `AgentCreate` 역할의 관계를 보안 모델에서 승인한다.
2. 자동 provisioning이 배정·Task 요청자의 권한 상승 경로가 아님을 테스트로 증명한다.
3. agent slot 경쟁, lease 회수, Project Worker 부재 경로를 통합 테스트로 검증한다.

미결 정책의 비교와 선택지는 [Project model review](../reviews/project-model-review-2026-08-17.md)에 기록한다. 이 문서는 승인된 불변식과 차단 조건만 보존한다.

## 관련 문서

- [Project 관리 외부 계약](../contracts/project-management.md) — 제안된 Dashboard HTTP와 MCP 표면
- [Project·Task·Agent lifecycle](project-task-agent-lifecycle.md) — 교차 상태 전이
- [Agent provisioning](agents/provisioning.md) — Agent 생성·중지와 Project 정책 소비
- [UI Dashboard](../ui-dashboard/ui-design.md) — Project 화면과 상태 표현
- [Roadmap](../roadmap/roadmap.md) — 구현 우선순위
