---
type: api-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/dashboard-api.md"
last_verified: "2026-08-28"
last_verified_commit: "working-tree"
owners: ["fleet-dashboard"]
---

# Dashboard API 계약

이 문서는 first-party Dashboard가 사용하는 `/api/*` 표면의 정본 진입점이다. route·session·RBAC의
현재 구현은 [`crates/fleet-dashboard/src/app.rs`](../../crates/fleet-dashboard/src/app.rs)와
schema·handler 테스트를 기준으로 한다.

Dashboard API는 같은 저장소의 first-party UI와 함께 배포된다. 따라서 `/v1` Worker API와 달리
독립 외부 클라이언트 호환성을 아직 보장하지 않는다. 외부 공개가 필요해지면 versioning과 OpenAPI
범위를 별도 결정으로 추가한다.

## 현재 route 표면

`/api/users/resend-verification`을 제외한 아래 `/api/*` route는 session이 필요하다. session은
최소 경계일 뿐이고, handler가 `events:list`, `host:provision`, 사용자 관리 등 세부 capability를
추가로 확인한다. 각 응답 필드는 `crates/fleet-dashboard/src/schema.rs`, 상세 오류와 권한은 handler
테스트가 현재 구현 근거다.

| 표면 | Method | 현재 capability | 목적 |
|---|---|---|---|
| `/api/overview` | GET | `DashboardView` | 운영 요약 |
| `/api/me` | GET | session | 현재 session 사용자 |
| `/api/workers`, `/api/workers/{id}` | GET | `WorkerList` | Worker 목록·상세 |
| `/api/tasks` | GET, POST | `TaskList`, `TaskCreate` | Task 목록·제출 |
| `/api/tasks/{id}`, `/api/tasks/{id}/thread` | GET | `TaskRead`; output은 `TaskOutput` | Task 상세·thread 조회 |
| `/api/task-threads` | GET | `TaskList` | 스레드 목록 |
| `/api/events`, `/api/events/stream` | GET | `EventsList` | 이벤트 목록·SSE stream |
| `/api/hosts`, `/api/hosts/{hostname}` | GET | `DashboardView` | Host 목록·상세 |
| `/api/projects` | GET, POST | `project:read`, `project:create` | Project 목록·생성 (로드맵 #48) |
| `/api/projects/{id}` | GET, DELETE | `project:read`, `project:delete` | Project 상세·archive 요청 |
| `/api/agents` | GET, POST | `agent:read`, `agent:manage` | Agent 목록(`project_id` 필터)·생성 (로드맵 #49) |
| `/api/agents/{id}` | DELETE | `agent:manage` | Agent 회수(`ready → stopped`, idempotent) |
| `/api/issues`, `/api/issues/{id}` | GET, POST, PATCH | `issue:read`, `issue:create`, `issue:update` | Issue 목록·생성·수정 (로드맵 #92) |
| `/api/issues/{id}/transition` | POST | 목표 상태별 (`required_capability_for_transition`) | Issue 상태 전이 |
| `/api/issues/{id}/comments` | GET, POST | `issue:read`, `issue:comment` | Issue 코멘트 |
| `/api/issues/{id}/links`, `/api/issues/{id}/links/{task_id}` | GET, POST, DELETE | `issue:read`, `issue:link` | Issue↔Task 연결 |
| `/api/audit` | GET | `AuditRead` | 인증·권한 감사 로그 |
| `/api/tools` | GET | `DashboardView` | MCP 도구 카탈로그 |
| `/api/users` | GET, POST | `UserRead`, `UserCreate` | 사용자 목록·생성 |
| `/api/users/{id}/toggle`, `/api/users/{id}/delete` | POST | `UserCreate`, `UserDelete` | 사용자 상태 변경·삭제 |
| `/api/ssh-keys`, `/api/ssh-keys/{name}` | GET, POST, DELETE | `HostProvision` | 프로비저닝용 SSH 비밀키 관리 |
| `/api/hosts/provision` | POST | `HostProvision` | 원격 host provisioning 요청 |
| `/api/users/resend-verification` | POST | public; 현재 rate limit 없음 | 인증 전 이메일 재전송 |

`/api/agents`에 `PATCH`가 없는 것은 미구현이 아니라 규칙이다 — Agent의 `project_id`는 불변이라
갱신 경로 자체를 만들지 않았다. 옮기려면 대상 Project에 새 Agent를 만든다. Project·Issue 행은
`#48`/`#92`가 route를 낼 때 이 표에 반영되지 않았던 것을 `#49` 1단계에서 함께 채운 것이다.
`/api/issues/{id}`의 `PATCH`는 `issue:update`를 요구하되 담당자 필드를 건드리면 `issue:assign`을
추가로 확인한다 — 표의 한 칸에 담기지 않으므로 `handlers::update_issue_api`가 정본이다.

`DELETE /api/projects/{id}`의 응답은 Project 상태에 더해 archive 게이트를 **막은 사유**를 싣는다:

```json
{ "id": "...", "name": "...", "status": "draining", "archive_blocked_by": ["agents"] }
```

`archive_blocked_by`는 `tasks`(비종료 Task가 남음)와 `agents`(회수되지 않은 `Ready` Agent가 남음)를
값으로 가지며, 게이트를 통과했으면 비어 있어 실리지 않는다. 두 조건은 **단락 평가하지 않으므로**
둘 다 막고 있으면 둘 다 실린다 — 하나만 알려 주면 호출자가 첫 사유를 해소한 뒤에야 두 번째를 알게
된다. 상태만 돌려주던 동안 화면은 사유를 짐작할 수밖에 없었고, `#49`가 Agent 조건을 추가하자
Task가 0건인 Project에 "tasks still running"이라고 표시했다. 같은 어휘를 MCP
`fleet_delete_project`도 싣는다([MCP 도구 계약](mcp-tools.md)) — 사유를 만드는 곳은 게이트를
평가하는 `fleet_store::ArchiveBlockers` 한 곳이다.

## 계획된 표면 — Task 삭제와 스레드 목록 (`#96`)

아래는 아직 **구현되지 않았다**. 위 표는 실제 route만 담으므로 여기에 분리해 둔다.

| route | method | 필요 permission | 목적 |
|---|---|---|---|
| `/api/tasks/{id}` | DELETE | `TaskDelete` | terminal Task 삭제 |
| `/api/task-threads` | GET | `TaskList` | 스레드 단위 페이지 — 구성원 포함 |

`DELETE /api/tasks/{id}`의 응답 코드는 다음과 같이 구분한다. 셋을 뭉뚱그리면 UI가 "먼저 취소하세요"와
"의존 태스크를 먼저 정리하세요"를 같은 문구로 보여 주게 되고, 사용자는 어느 쪽도 해결할 수 없다.

| 상황 | 코드 | envelope `code` |
|---|---|---|
| 삭제됨 | 204 | — |
| 행이 없음 | 404 | `not_found` |
| terminal이 아님 | 409 | `conflict` |
| `Pending` 의존자가 있음 | 409 | `conflict` (메시지에 의존자 id 나열) |
| permission 없음 | 403 | `forbidden` |

terminal 여부와 부재는 `DELETE ... AND status_phase = ANY($2)`의 0행으로 함께 나타나므로, 404와 409를
가르려면 0행일 때만 행 존재를 한 번 더 조회한다. `compare_and_set_task_status`가 거절과 부재를
구분하려고 쓰는 것과 같은 형태이며, 그 조회는 판정이 아니라 **보고**를 위한 것이다.

`GET /api/task-threads`는 기존 `/api/tasks/{id}/thread`(단일 스레드 조회, 구현됨)와 다르다. 후자는 id
하나를 스레드로 확장하고, 전자는 스레드들을 페이지 단위로 고른다. 계약과 그룹핑 규칙은
[UI 설계](../ui-dashboard/ui-design.md)의 태스크 큐 절, 삭제 계약은
[Task Management](../architecture/tasks/management.md)가 정본이다.

## 오류와 mutation 경계

`ApiError`를 사용하는 Dashboard JSON handler의 현재 envelope는 다음과 같다.

```json
{ "error": { "code": "not_found", "message": "worker not found" } }
```

해당 타입의 코드는 `bad_request`, `unauthorized`, `forbidden`, `not_found`, `conflict`,
`store_error`, `internal_error`, `unavailable`이다. 모든 route가 아직 이 envelope로 통합된 것은 아니다.
세부 요청·응답 schema와 mutation별 CSRF 적용은 기계 판독 계약으로 통합되지 않았으므로 handler와
테스트가 실행 사실이다. 특히 public resend JSON API는 현재 rate limit이 없고, 존재하지 않는 계정과
이미 검증된 계정에 서로 다른 오류를 반환해 계정 상태를 구분할 수 있다. session cookie가 있다는
이유만으로 모든 mutation의 CSRF 검증이나 멱등성을 가정하면 안 된다. 외부 공개 전에는 versioning,
schema 생성, pagination, CSRF와 idempotency를 별도 호환성 결정으로 확정한다.

인증·세션·RBAC의 목표 정책은 [Authorization·Project Scope·감사](../security/authorization-and-audit.md)가
정본이다. API 표면을 바꿀 때는 이 표, route, schema, handler 테스트를 함께 갱신한다.
