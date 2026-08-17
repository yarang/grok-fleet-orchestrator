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
완전한 기계 판독 명세는 [`crates/fleet-api/src/openapi.yaml`](../../crates/fleet-api/src/openapi.yaml)이며,
실행 중에는 `GET /openapi.yaml`으로 조회한다. Dashboard의 `/api/*` 계약은 이 문서의 범위 밖이다.

## 현재 계약

| 표면 | 목적 | 정본 근거 |
|---|---|---|
| `GET /v1/health`, `GET /metrics`, `GET /openapi.yaml` | 상태·관측·명세 조회 | OpenAPI와 `fleet-api` router |
| `/v1/workers/*` | register, heartbeat, 목록, 자격증명 | OpenAPI |
| `POST /v1/hosts/register` | provisioned host 등록 | OpenAPI |
| `/v1/bootstrap-tokens/*` | 발급·목록·회수 | OpenAPI 및 worker enrollment 계약 |

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

## 호환성과 오류

경로·요청·응답·오류 형식은 OpenAPI를 우선한다. API를 변경할 때는 OpenAPI, router, 회귀 테스트를
같이 수정한다. 이전 HTTP·MCP 혼합 문서는 삭제됐으며, 삭제·분리 판단은
[`docs/reviews/`](../reviews/README.md)에 부기한다.
