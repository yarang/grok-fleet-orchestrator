---
type: api-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/http-api.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["fleet-api"]
---

# HTTP API 계약

이 문서는 외부 Worker와 자동화가 호출하는 `/v1` HTTP API의 정본 진입점이다. 필드·경로의
기계 판독 wire schema는 [`crates/fleet-api/src/openapi.yaml`](../../crates/fleet-api/src/openapi.yaml)이며,
실행 중에는 `GET /openapi.yaml`으로 조회한다. Dashboard의 `/api/*` 계약은 이 문서의 범위 밖이다.
`GET /openapi.yaml` 자체는 OpenAPI paths에 포함되지 않은 명세 배포 route다. 코드와 OpenAPI가
다르면 router·handler가 현재 실행 사실이며, 차이를 결함으로 기록하고 함께 수정한다.

## 현재 계약

| 표면 | 목적 | 정본 근거 |
|---|---|---|
| `GET /v1/health`, `GET /metrics`, `GET /openapi.yaml` | 상태·관측·명세 조회 | OpenAPI와 `fleet-api` router |
| `POST /v1/workers/join`, `POST /v1/workers/register`, `POST /v1/workers/heartbeat` | 가입·등록·heartbeat | OpenAPI와 enrollment 계약 |
| `GET /v1/workers`, `GET·DELETE /v1/workers/{id}` | Worker 목록·조회·등록 해제 | OpenAPI |
| `GET·PUT /v1/workers/{name}/credentials` | Worker credential 목록·저장 | OpenAPI |
| `GET /v1/workers/{name}/credentials/{model_id}/export` | credential 원문 export | OpenAPI |
| `DELETE /v1/workers/{name}/credentials/{model_id}` | credential 삭제 | OpenAPI |
| `POST /v1/workers/{id}/credential/rotate`, `DELETE /v1/workers/{id}/credential` | Worker **operational** credential 회전·회수 | `fleet-api` router (OpenAPI 미반영 — 결함) |
| `POST /v1/hosts/register` | provisioned host 등록 | OpenAPI |
| `/v1/bootstrap-tokens/*` | 발급·목록·회수 | OpenAPI 및 worker enrollment 계약 |
| `POST·GET /v1/admin/tokens`, `POST /v1/admin/tokens/{principal_id}/rotate`, `DELETE /v1/admin/tokens/{principal_id}` | admin bearer token 생성·목록·회전·회수 | `fleet-api` router (OpenAPI 미반영 — 결함) |

단수형 `/workers/{id}/credential`과 복수형 `/workers/{name}/credentials`는 **다른 자원**이다.
전자는 Worker 자신의 operational identity(`worker:credential:manage`), 후자는 그 워커가 사용하는
LLM 프로바이더 API 키(`worker:llm_credential:*`)다. capability도 분리되어 있으므로 혼동하면
과대 권한 부여로 이어진다.

`/v1/workers/{id}/credential/*`과 `/v1/admin/tokens*`는 router에 존재하지만 `openapi.yaml`에
없다. 이 문서 서두의 규칙(OpenAPI 우선)에 따라 이는 정본의 공백이며 결함으로 기록한다.

## 인증 경계

현재 인증 조합은 다음과 같다. 이는 프로덕션 정책이 아니라 구현의 제약이며, 목표
fail-closed와 principal/capability 계약은 [control-plane security model](../security/control-plane-security-model.md)이 정본이다.

| 설정 | `/v1/health` | 나머지 `/v1/*` | `/metrics`, `/openapi.yaml` |
|---|---|---|---|
| API token·Cloudflare 미설정 | 허용 | 허용 | bearer 미들웨어 밖 |
| API token만 설정 | 허용 | Bearer 필요 | bearer 미들웨어 밖 |
| Cloudflare만 설정 | Cloudflare assertion 필요 | Cloudflare assertion 필요 | 배포 edge 정책으로 별도 보호 필요 |
| 둘 다 설정 | Cloudflare assertion 필요 | Cloudflare assertion과 Bearer 모두 필요 | bearer 미들웨어 밖 |

`/metrics`와 `/openapi.yaml`은 `/v1` bearer 미들웨어 바깥에 있으므로, 공개 bind 또는
reverse proxy 배포에서는 별도의 네트워크·gateway ACL을 검증해야 한다.

`/v1/workers/join`의 현재·목표 인증 차이와 Worker credential 전환은
[worker enrollment](worker-enrollment.md)에 둔다.

현재 bearer 검사는 route별 capability 행렬(`required_capability`)을 거친다. **`#73`(2026-08-23)
이후 행렬에 등록되지 않은 route는 검사 없이 통과하지 않고 기본값이 deny다** — `crates/fleet-api/src/app.rs`의
`authorize_http_endpoint`가 `required_capability`의 `None`을 `403`으로 처리하며, `/health`와
`POST /workers/join`(body의 bootstrap token이 자체 인증 수단)만 명시적으로 예외다. 과거 누락이었던
`GET /v1/workers/{id}`(`worker:list`)와 `POST /v1/hosts/register`(`host:provision`)는 이 전환과 함께
행렬에 등록됐다. `crates/fleet-api/src/app.rs`의 `capability_matrix_covers_router_routes`와
`authorize_http_endpoint_denies_by_default_for_any_unmatched_route`가 회귀를 막는다.

Cloudflare Access 전용 배포에서 principal→capability 매핑이 없으면 인증을 통과한 모든 주체가 전체
capability를 받는 fail-open은 아직 남아 있다(운영 배포에는 이 매핑을 설정할 경로가 아직 없다,
`#74` 대상).

재인증·break-glass·감사와 secret 관리 권한 분리, 그리고 위 두 fail-open의 불변식은
[Authorization·Project Scope·감사](../security/authorization-and-audit.md)의 목표이며 현재 보장으로
해석하면 안 된다.

목표 principal, capability, Project scope와 Worker self binding은
[Authorization·Project Scope·감사](../security/authorization-and-audit.md)가 정본이다.

## 호환성과 오류

경로·요청·응답·오류 형식은 OpenAPI를 우선한다. API를 변경할 때는 OpenAPI, router, 회귀 테스트를
같이 수정한다. 현재 Worker 목록의 `limit`·`offset`에는 최대 limit, snapshot consistency,
`has_more` 또는 next cursor 계약이 없다. 클라이언트는 전체 집합의 안정된 snapshot이나 무제한
요청을 가정하면 안 된다. 재시도 가능 오류, `429`·`503`, 정렬·pagination 계약은 구현 전
OpenAPI와 회귀 테스트에 함께 추가한다.
