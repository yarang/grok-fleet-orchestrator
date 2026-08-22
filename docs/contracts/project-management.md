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
| `DELETE` | `/api/projects/{id}` | `project:delete` | Project archive 요청 (즉시 영구 삭제 아님) |

`DELETE`는 `request_id`를 받는 `Active → Draining` idempotent archive 요청이며 `202 Accepted`와
archive progress를 반환한다. 모든 Attempt가 terminal이고, effect hold가 해소되고, Agent process·lease·
credential grant cleanup이 확인된 뒤에만 `Archived`가 된다. 미해결 effect 또는 cleanup 실패는
`ArchiveBlocked`와 hold summary로 반환한다. 영구 삭제는 archive 보존 기간·감사 정책을 통과한 별도
관리 작업이며 이 계약의 호출 표면에 포함하지 않는다.

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
| `fleet_update_project_policy` | `project_id`, revision, agent 상한·worker eligibility | 권한 승인 전 차단 |
| `fleet_dispatch_task` 확장 | 선택 `project_id` | 제안됨 |

Host/Worker 배정 MCP 도구는 범위에 포함하지 않는다. `fleet_dispatch_task`의 선택 `project_id`는 현재 저장·조회만 지원한다. Project control plane이 도입된 뒤 [Project 모델](../architecture/project-feature-design.md)의 policy·Agent admission을 적용한다.

## 활성화 게이트

- Project 데이터와 agent slot·Worker lease 불변식의 저장·통합 테스트
- `ProjectPolicyManage`로 `AgentCreate`를 우회할 수 없다는 권한 테스트
- Project의 capability·slot 조건 부재 시 일반 풀 context로 폴백하지 않는 디스패치 테스트
- Dashboard와 MCP의 동일한 권한·오류 응답 검증
- 목록 pagination·caller Project scope와 삭제 lifecycle의 revision/충돌 검증
