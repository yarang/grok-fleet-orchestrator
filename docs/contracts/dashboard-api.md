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

| 표면 | Method | 현재 capability | 목적 |
|---|---|---|---|
| `/api/overview` | GET | `DashboardView` | 운영 요약 |
| `/api/me` | GET | session | 현재 session 사용자 |
| `/api/workers`, `/api/workers/{id}` | GET | `WorkerList` | Worker 목록·상세 |
| `/api/tasks` | GET, POST | `TaskList`, `TaskCreate` | Task 목록·제출 |
| `/api/tasks/{id}`, `/api/tasks/{id}/thread` | GET | `TaskRead`; output은 `TaskOutput` | Task 상세·thread 조회 |
| `/api/events`, `/api/events/stream` | GET | `EventsList` | 이벤트 목록·SSE stream |
| `/api/hosts`, `/api/hosts/{hostname}` | GET | `DashboardView` | Host 목록·상세 |
| `/api/audit` | GET | `AuditRead` | 인증·권한 감사 로그 |
| `/api/tools` | GET | `DashboardView` | MCP 도구 카탈로그 |
| `/api/users` | GET, POST | `UserRead`, `UserCreate` | 사용자 목록·생성 |
| `/api/users/{id}/toggle`, `/api/users/{id}/delete` | POST | `UserCreate`, `UserDelete` | 사용자 상태 변경·삭제 |
| `/api/ssh-keys`, `/api/ssh-keys/{name}` | GET, POST, DELETE | `HostProvision` | 프로비저닝용 SSH 비밀키 관리 |
| `/api/hosts/provision` | POST | `HostProvision` | 원격 host provisioning 요청 |
| `/api/users/resend-verification` | POST | public; 현재 rate limit 없음 | 인증 전 이메일 재전송 |

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
