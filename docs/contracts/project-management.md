---
type: proposed-contract
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/contracts/project-management.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["project-platform", "api-contracts"]
---

# Project 관리 계약

이 문서는 Project 관리에 필요한 Dashboard HTTP 및 MCP 호출 표면을 정의한다. 모든 항목은 **제안됨**이며 현재 라우터나 MCP 서버에 구현되어 있지 않다. Project 모델과 배정 불변식은 [Project 모델과 거버넌스](../architecture/project-feature-design.md)가 소유한다.

## Dashboard JSON API

`/projects`, `/projects/{id}`, `/projects/new` 화면과 navigation은
[Dashboard UI 설계](../ui-dashboard/ui-design.md)가 소유한다. 이 문서는 JSON wire 표면만 정의한다.

| Method | 경로 | 필요 권한 | 목적 |
|---|---|---|---|
| `GET` | `/api/projects` | `project:read` | 목록 JSON |
| `POST` | `/api/projects` | `project:create` | Project 생성 |
| `GET` | `/api/projects/{id}` | `project:read` | Project 조회 |
| `DELETE` | `/api/projects/{id}` | `project:delete` | Project 삭제 요청 |

### 승인 전 차단 후보

`PATCH /api/projects/{id}`와 host·worker Project 배정·해제 endpoint는 `project:assign`과
`agent:create`의 관계가 승인될 때까지 구현하거나 노출하지 않는다. 승인 후에는 revision 또는
`If-Match`, `request_id`, Project scope 검사 시점과 동시 배정 충돌 의미를 먼저 확정한다.

공통 실패는 인증되지 않은 요청의 `401`, 권한 부족의 `403`, 없는 Project·host·worker의 `404`, 배정 불변식 위반 또는 허용되지 않은 lifecycle 변경의 `409`, 잘못된 입력의 `422`다.

## MCP

승인 후 MCP 표면은 다음 최소 집합으로 제한한다.

| 도구 | 입력 | 상태 |
|---|---|---|
| `fleet_create_project` | `name`, 선택 `description` | 제안됨 |
| `fleet_list_projects` | 선택 `limit`, `offset` | 제안됨 |
| `fleet_delete_project` | `project_id` | 제안됨 |
| `fleet_assign_worker_to_project` | `project_id`, `worker_id` | 권한 승인 전 차단 |
| `fleet_dispatch_task` 확장 | 선택 `project_id` | 제안됨 |

host 배정 MCP 도구는 초기 범위에 포함하지 않는다. `fleet_dispatch_task`에 `project_id`가 주어지면 [Project 모델](../architecture/project-feature-design.md)의 hard eligibility를 적용한다.

## 활성화 게이트

- Project 데이터와 host/worker 소유 불변식의 저장·통합 테스트
- `ProjectAssign`으로 `AgentCreate`를 우회할 수 없다는 권한 테스트
- Project Worker 부재 시 일반 풀로 폴백하지 않는 디스패치 테스트
- Dashboard와 MCP의 동일한 권한·오류 응답 검증
- 목록 pagination·caller Project scope와 삭제 lifecycle의 revision/충돌 검증
