---
type: api-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/dashboard-api.md"
last_verified: "2026-08-17"
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

| 표면 | Method | 목적 |
|---|---|---|
| `/api/overview`, `/api/me` | GET | 요약과 현재 session 사용자 |
| `/api/workers`, `/api/workers/{id}` | GET | Worker 목록·상세 |
| `/api/tasks` | GET, POST | Task 목록·제출 |
| `/api/tasks/{id}`, `/api/tasks/{id}/thread` | GET | Task 상세·thread 조회 |
| `/api/events`, `/api/events/stream` | GET | 이벤트 목록·SSE stream |
| `/api/hosts`, `/api/hosts/{hostname}` | GET | Host 목록·상세 |
| `/api/audit` | GET | 인증·권한 감사 로그 |
| `/api/tools` | GET | MCP 도구 카탈로그 |
| `/api/users` | GET, POST | 사용자 목록·생성 |
| `/api/users/{id}/toggle`, `/api/users/{id}/delete` | POST | 사용자 상태 변경·삭제 |
| `/api/ssh-keys`, `/api/ssh-keys/{name}` | GET, POST, DELETE | SSH key 메타데이터·관리 |
| `/api/hosts/provision` | POST | 원격 host provisioning 요청 |
| `/api/users/resend-verification` | POST | 인증 전 이메일 재전송 |

인증·세션·RBAC의 목표 정책은 [control-plane security model](../security/control-plane-security-model.md)이
정본이다. API 표면을 바꿀 때는 이 표, route, schema, handler 테스트를 함께 갱신한다.
