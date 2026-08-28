---
type: proposed-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/project-management.md"
last_verified: "2026-08-28"
last_verified_commit: "working-tree"
owners: ["project-platform", "api-contracts"]
---

# Project 관리 계약

이 문서는 Project 관리에 필요한 Dashboard HTTP 및 MCP 호출 표면을 정의한다. **로드맵 `#48` 1단계(2026-08-24 완료)로 아래 표의 `GET`/`POST`/`DELETE`(Dashboard)와 `fleet_create_project`/`fleet_list_projects`/`fleet_delete_project`/`fleet_dispatch_task` 확장(MCP)이 구현됐다** — "차단 중인 표면" 절의 `PATCH`와 정책 관리만 여전히 제안 단계다(host·worker 배정은 그 절에서 설계상 배제됐다). Project 모델과 배정 불변식은 [Project 모델과 거버넌스](../architecture/project-feature-design.md)가 소유한다.

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
archive 요청이며 `202 Accepted`와 archive progress를 반환하고, 모든 Task가 terminal이고 effect
hold가 해소되고 Agent process·lease·credential grant cleanup이 확인된 뒤에만 `Archived`가 되는
것이다. 1단계 구현은 이 중 "Task가 전부 terminal인가"만 확인했다(effect ledger·Agent process
cleanup은 그 대상 자체가 없었다). **`#49` 1단계(2026-08-28)로 게이트가 두 조건이 됐다** —
"비종료 Task 없음 **그리고** 살아 있는(`ready`) Agent 없음"이다(`Store::project_has_live_agents`).
Agent process cleanup 증거는 여전히 확인하지 않는다 — 뒤에 프로세스가 없어 정리할 대상이 없고,
확인하는 것은 Agent **행**이 회수됐는지뿐이다. 실행 관점에서 Task 조건은 **완전하다** — [흡수
판정](../architecture/project-task-agent-lifecycle.md#attempt-흡수-판정)으로 따로 확인할 비터미널
Attempt가 남아 있지 않기 때문이다 — 게이트를 통과하면 같은 요청 안에서 곧바로
`Draining → Archived`까지 진행하고, 항상 `200`과 현재 Project 상태를 반환한다(`202`+비동기 progress
아님, `request_id` 입력도 받지 않음 — 배경 drain 작업이 없어 즉시 판정 가능하기 때문). 재호출은
안전하다(같은 게이트를 다시 평가할 뿐). 영구 삭제는 archive 보존 기간·감사 정책을 통과한 별도
관리 작업이며 이 계약의 호출 표면에 포함하지 않는다(1단계에도 없음).

### 차단 중인 표면

`fleet_update_project_policy`를 비롯한 **정책 변경** 표면은 여전히 구현하거나 노출하지 않는다.
[Project 기능 설계](../architecture/project-feature-design.md)의 차단 조건 2·3(자동 provisioning이
권한 상승 경로가 아님을 증명, agent slot 경쟁·lease 회수 통합 검증)이 남아 있다.

**`#49` 1단계로 Agent 엔티티는 생겼지만 두 조건은 그대로 열려 있다.** 조건 2는 *자동*
provisioning 경로를 시험하는데 그 경로가 없다 — Task 제출은 Agent를 만들지 않고, Agent는
`agent:manage` 보유자가 명시적으로 만들 때만 생긴다. 조건 3의 agent slot 경쟁·lease 회수는
`worker_execution_leases`(`#67` 후속)가 여전히 없다. 즉 차단을 푸는 것은 Agent 엔티티가 아니라
**자동 provisioning과 lease**이며, 그 둘은 `#89`·`#67` 후속에 걸려 있다.

**2026-08-27 승인으로 이 절의 범위가 좁아졌다.** 이전 문언은 `PATCH /api/projects/{id}`와
host·worker Project 배정·해제 endpoint를 `project:assign`·`agent:create`의 관계 승인까지 함께
묶었다. 세 부분 모두 정정한다.

- **`project:assign`과 `agent:create`는 목표 capability 표에 없는 이름이었다.** 승인된 규칙은
  `project:policy_manage`와 `agent:manage`의 관계로 기록됐다 —
  [Authorization·Project Scope·감사](../security/authorization-and-audit.md)의 "Project 정책 변경과
  Agent 생성의 관계"가 정본이다.
- **host·worker Project 배정·해제 endpoint는 승인 대기가 아니라 설계상 존재하지 않는다.**
  [공유 실행 풀 불변식](../architecture/project-feature-design.md)이 "Host와 Worker에는
  `project_id`를 두지 않는다"로 이미 배제했다. 차단 후보로 남겨 두면 언젠가 승인되어 생길 것처럼
  읽히므로 후보 목록에서 뺀다.
- **`PATCH /api/projects/{id}`의 남은 차단 사유는 보안이 아니다.** `name`·`description` 편집은
  Agent를 만들지 않으므로 위 관계의 대상이 아니다(승인 결정 3). 다만 구현 전에 동시 편집 의미를
  먼저 확정해야 한다 — `projects`에 정책 revision 컬럼이 없으므로 `updated_at` 기반 `If-Match`로
  갈지 revision 컬럼을 신설할지가 실제 결정 대상이고, `request_id` 재시도 의미도 함께 정한다.
  확정 전까지 노출하지 않는다.

공통 실패는 인증되지 않은 요청의 `401`, 권한 부족의 `403`, 없는 Project·host·worker의 `404`, 배정 불변식 위반 또는 허용되지 않은 lifecycle 변경의 `409`, 잘못된 입력의 `422`다.

## MCP

승인 후 MCP 표면은 다음 최소 집합으로 제한한다.

| 도구 | 입력 | 상태 |
|---|---|---|
| `fleet_create_project` | `name`, 선택 `description` | 구현됨 |
| `fleet_list_projects` | 선택 `limit`, `offset` | 구현됨 |
| `fleet_delete_project` | `project_id` | 구현됨 (Dashboard `DELETE`와 동일한 1단계 축소판) |
| `fleet_update_project_policy` | `project_id`, revision, agent 상한·worker eligibility | 차단 — 권한 규칙은 승인됨(2026-08-27), 정책 컬럼·Agent 엔티티 부재로 차단 조건 2·3 미충족 |
| `fleet_dispatch_task` 확장 | 선택 `project_id` | 구현됨 — 저장·조회를 넘어 검증까지 한다: 존재하지 않거나 `active`가 아닌 `project_id`는 제출 자체를 거절한다(아래 참고) |

Host/Worker 배정 MCP 도구는 범위에 포함하지 않는다. `fleet_dispatch_task`의 선택 `project_id`는
존재·상태 검증을 수행한다 — "현재 저장·조회만 지원"이라던 이전 상태는 지났다. 다만 검증을 통과한
뒤의 실제 dispatch는 여전히 project 무관 일반 풀 규칙을 그대로 쓴다(policy·Agent admission은 아직
적용되지 않는다) — [Project 모델](../architecture/project-feature-design.md)의 "디스패치 자격" 절
참고.

**두 표면의 동일 동작 보장(2단계)**: Task 제출 시 `project_id` 검증과 archive 진행 절차는
`fleet_store::project_rules`(`ensure_project_accepts_new_tasks` / `advance_project_archive`)에 한 번만
구현돼 있고, Dashboard와 MCP 핸들러는 그 결과를 각자의 에러 타입(HTTP `ApiError` / JSON-RPC
`JsonRpcError`)으로 옮기기만 한다 — 규칙을 표면마다 따로 구현해 시간이 지나며 갈라지는 것을
구조적으로 막는다.

## 활성화 게이트

목표 계약 전체(policy 관리, Agent admission 연동)의 게이트는 아래와 같이 남아 있다. 1단계
(project 존재/조회/생성/archive-요청)는 각 항목이 실제로 요구하는 하부 구조(Agent, effect ledger,
policy revision)가 없어 해당하지 않는다 — 대신 1단계 자체의 검증은 `crates/fleet-store/tests/projects.rs`,
`crates/fleet-dashboard/tests/dashboard_api.rs`, `crates/fleet-mcp/src/handlers.rs`의 단위/통합
테스트가 담당한다.

- Project 데이터와 agent slot·Worker lease 불변식의 저장·통합 테스트 — **여전히 미해당(agent slot·Worker lease 없음)**. `#49` 1단계가 만든 것은 Agent 행과 archive 게이트이며 그 부분은 `crates/fleet-store/tests/agents.rs`가 검증한다
- `project:policy_manage`로 `agent:manage`를 우회할 수 없다는 권한 테스트 — **규칙은 승인됨(2026-08-27), 시험은 여전히 미해당**. `#49` 1단계로 `agent:manage`는 생겼지만 `project:policy_manage`와 집행 대상인 정책 컬럼이 없어 우회할 대상이 없다. 시험 형태는 [감사 계약](../security/authorization-and-audit.md)의 구현 게이트 9가 소유한다
- Project의 capability·slot 조건 부재 시 일반 풀 context로 폴백하지 않는 디스패치 테스트 — **미해당(그 자격 검증 자체가 아직 없음 — 지금은 애초에 project 무관 일반 풀 규칙만 있다)**
- Dashboard와 MCP의 동일한 권한·오류 응답 검증 — 확인됨(둘 다 `project:{read,create,delete}` 사용). 2단계에서 검증·archive 규칙을 `fleet_store::project_rules`로 단일화해 구조적으로 보장한다
- 목록 pagination·caller Project scope와 삭제 lifecycle의 revision/충돌 검증 — pagination(limit/offset)은 확인됨; Project scope(호출자가 자기 Project만 보는 것)는 아직 모든 인증된 호출자가 전체 Project를 본다(RBAC의 Project 단위 scope는 미구현); revision/충돌은 정책 revision 자체가 없어 미해당
