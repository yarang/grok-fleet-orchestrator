---
type: contract
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/contracts/project-management.md"
last_verified: "2026-08-17"
---

# Project 관리 계약

이 문서는 Project 관리에 필요한 Dashboard HTTP 및 MCP 호출 표면을 정의한다. 모든 항목은 **제안됨**이며 현재 라우터나 MCP 서버에 구현되어 있지 않다. Project 모델과 배정 불변식은 [Project 모델과 거버넌스](../architecture/project-feature-design.md)가 소유한다.

## Dashboard HTTP

| Method | 경로 | 필요 권한 | 목적 |
|---|---|---|---|
| `GET` | `/projects` | `project:read` | Project 목록 화면 |
| `GET` | `/projects/{id}` | `project:read` | Project 상세 화면 |
| `GET` | `/projects/new` | `project:create` | Project 생성 화면 |
| `GET` | `/api/projects` | `project:read` | 목록 JSON |
| `POST` | `/api/projects` | `project:create` | Project 생성 |
| `GET` | `/api/projects/{id}` | `project:read` | Project 조회 |
| `PATCH` | `/api/projects/{id}` | 미승인 | Project 정책 수정 |
| `DELETE` | `/api/projects/{id}` | `project:delete` | Project 삭제 요청 |
| `PUT` | `/api/workers/{id}/project` | 미승인 | worker 배정 |
| `DELETE` | `/api/workers/{id}/project` | 미승인 | worker 배정 해제 |
| `PUT` | `/api/hosts/{id}/project` | 미승인 | host 배정 |
| `DELETE` | `/api/hosts/{id}/project` | 미승인 | host 배정 해제 |

`PATCH`와 배정 호출은 `project:assign`과 `agent:create`의 관계가 승인될 때까지 구현하거나 노출하지 않는다. 화면 경로도 해당 권한이 확정되기 전에는 활성 기능처럼 표시하지 않는다.

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
