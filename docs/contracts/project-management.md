---
type: proposed-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/project-management.md"
last_verified: "2026-08-24"
last_verified_commit: "working-tree"
owners: ["project-platform", "api-contracts"]
---

# Project 관리 계약

이 문서는 Project 관리에 필요한 Dashboard HTTP 및 MCP 호출 표면을 정의한다. **로드맵 `#48` 1단계(2026-08-24 완료)로 아래 표의 `GET`/`POST`/`DELETE`(Dashboard)와 `fleet_create_project`/`fleet_list_projects`/`fleet_delete_project`/`fleet_dispatch_task` 확장(MCP)이 구현됐다** — "승인 전 차단 후보" 절의 `PATCH`/정책 관리/host·worker 배정만 여전히 제안 단계다. Project 모델과 배정 불변식은 [Project 모델과 거버넌스](../architecture/project-feature-design.md)가 소유한다.

## Dashboard JSON API

`/projects`, `/projects/{id}`, `/projects/new` 화면과 navigation은
[Dashboard UI 설계](../ui-dashboard/ui-design.md)가 소유한다. 이 문서는 JSON wire 표면만 정의한다.

| Method | 경로 | 필요 권한 | 목적 | 상태 |
|---|---|---|---|---|
| `GET` | `/api/projects` | `project:read` | 목록 JSON | 구현됨 |
| `POST` | `/api/projects` | `project:create` | Project 생성 | 구현됨 |
| `GET` | `/api/projects/{id}` | `project:read` | Project 조회 | 구현됨 |
| `DELETE` | `/api/projects/{id}` | `project:delete` | Project archive 요청 (즉시 영구 삭제 아님) | 구현됨(1단계 축소판, 아래) |

**목표 계약과의 차이(1단계)**: 목표는 `DELETE`가 `request_id`를 받는 `Active → Draining` idempotent
archive 요청이며 `202 Accepted`와 archive progress를 반환하고, 모든 Attempt가 terminal이고 effect
hold가 해소되고 Agent process·lease·credential grant cleanup이 확인된 뒤에만 `Archived`가 되는
것이다. 1단계 구현은 이 중 "Task가 전부 terminal인가"만 확인한다(`Attempt`·effect ledger·Agent
process cleanup은 그 대상 자체가 아직 없다) — 게이트를 통과하면 같은 요청 안에서 곧바로
`Draining → Archived`까지 진행하고, 항상 `200`과 현재 Project 상태를 반환한다(`202`+비동기 progress
아님, `request_id` 입력도 받지 않음 — 배경 drain 작업이 없어 즉시 판정 가능하기 때문). 재호출은
안전하다(같은 게이트를 다시 평가할 뿐). 영구 삭제는 archive 보존 기간·감사 정책을 통과한 별도
관리 작업이며 이 계약의 호출 표면에 포함하지 않는다(1단계에도 없음).

### 승인 전 차단 후보

`PATCH /api/projects/{id}`와 host·worker Project 배정·해제 endpoint는 `project:assign`과
`agent:create`의 관계가 승인될 때까지 구현하거나 노출하지 않는다. 승인 후에는 revision 또는
`If-Match`, `request_id`, Project scope 검사 시점과 동시 배정 충돌 의미를 먼저 확정한다.

공통 실패는 인증되지 않은 요청의 `401`, 권한 부족의 `403`, 없는 Project·host·worker의 `404`, 배정 불변식 위반 또는 허용되지 않은 lifecycle 변경의 `409`, 잘못된 입력의 `422`다.

## MCP

승인 후 MCP 표면은 다음 최소 집합으로 제한한다.

| 도구 | 입력 | 상태 |
|---|---|---|
| `fleet_create_project` | `name`, 선택 `description` | 구현됨 |
| `fleet_list_projects` | 선택 `limit`, `offset` | 구현됨 |
| `fleet_delete_project` | `project_id` | 구현됨 (Dashboard `DELETE`와 동일한 1단계 축소판) |
| `fleet_update_project_policy` | `project_id`, revision, agent 상한·worker eligibility | 권한 승인 전 차단 |
| `fleet_dispatch_task` 확장 | 선택 `project_id` | 구현됨 — **1단계로 저장·조회를 넘어 검증까지 한다**: 존재하지 않거나 `active`가 아닌 `project_id`는 제출 자체를 거절한다(아래 참고) |

Host/Worker 배정 MCP 도구는 범위에 포함하지 않는다. `fleet_dispatch_task`의 선택 `project_id`는
1단계부터 존재·상태 검증을 수행한다 — "현재 저장·조회만 지원"이라던 이전 상태는 지났다. 다만
검증을 통과한 뒤의 실제 dispatch는 여전히 project 무관 일반 풀 규칙을 그대로 쓴다(policy·Agent
admission은 아직 적용되지 않는다) — [Project 모델](../architecture/project-feature-design.md)의
"디스패치 자격" 절 참고.

## 활성화 게이트

목표 계약 전체(policy 관리, Agent admission 연동)의 게이트는 아래와 같이 남아 있다. 1단계
(project 존재/조회/생성/archive-요청)는 각 항목이 실제로 요구하는 하부 구조(Agent, effect ledger,
policy revision)가 없어 해당하지 않는다 — 대신 1단계 자체의 검증은 `crates/fleet-store/tests/projects.rs`,
`crates/fleet-dashboard/tests/dashboard_api.rs`, `crates/fleet-mcp/src/handlers.rs`의 단위/통합
테스트가 담당한다.

- Project 데이터와 agent slot·Worker lease 불변식의 저장·통합 테스트 — **미해당(Agent 없음)**
- `ProjectPolicyManage`로 `AgentCreate`를 우회할 수 없다는 권한 테스트 — **미해당(두 capability 다 없음)**
- Project의 capability·slot 조건 부재 시 일반 풀 context로 폴백하지 않는 디스패치 테스트 — **미해당(그 자격 검증 자체가 아직 없음 — 지금은 애초에 project 무관 일반 풀 규칙만 있다)**
- Dashboard와 MCP의 동일한 권한·오류 응답 검증 — 1단계 범위에서 확인됨(둘 다 `project:{read,create,delete}` 사용, 존재/상태 검증 동일)
- 목록 pagination·caller Project scope와 삭제 lifecycle의 revision/충돌 검증 — pagination(limit/offset)은 확인됨; Project scope(호출자가 자기 Project만 보는 것)는 아직 모든 인증된 호출자가 전체 Project를 본다(RBAC의 Project 단위 scope는 미구현); revision/충돌은 정책 revision 자체가 없어 미해당
