---
type: api-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/worker-enrollment.md"
last_verified: "2026-08-22"
last_verified_commit: "574feb4"
owners: ["fleet-api", "fleet-worker"]
---

# Worker enrollment 계약

이 문서는 Worker join, register, heartbeat의 현재 구현과 목표 보안 계약을 분리한다.
운영자는 목표 계약이 구현되기 전 self-service join을 일반 운영 절차로 취급하면 안 된다.

## 현재 구현

1. `fleet-worker join`은 bootstrap token을 요청 본문에 넣어 `/v1/workers/join`을 호출한다.
   token 원문은 `--token-file <path>`(`-`이면 stdin)로 전달하며, argv/env를 쓰는 `--token`은
   deprecated다.
2. 서버는 bootstrap 소비·Worker 생성·operational credential 발급을 **하나의 transaction**으로
   처리한다(`enroll_worker`; PgStore는 `pool.begin()`, MemStore는 단일 lock 스코프).
3. 서버는 join 성공 시 `fwo_` operational token을 새로 발급해 응답 `worker.toml`에만 1회 싣고,
   저장소에는 SHA-256 digest·worker_id·lifecycle metadata만 남긴다. bootstrap token 원문은
   재기록하지 않는다.
4. Worker daemon은 `operational_token`을 register/heartbeat/deregister의 Bearer로 사용한다.
   `worker.toml`의 legacy `[worker] bootstrap_token` 키는 파싱 단계에서 명시적으로 거절한다
   (자동 마이그레이션 없음).
5. register/heartbeat/deregister는 credential binding과 요청 대상 worker_id가 다르면 `403`을
   반환한다(`worker:self` binding).

### 해소된 제한

다음은 `#60`의 1~8단계에서 닫혔다. 상세 근거는 [Worker credential 전환](../roadmap/worker-credential-migration.md).

- ~~join 응답에 bootstrap 원문 token을 재기록하고 Bearer로 재사용~~ → operational credential로 대체
- ~~저장소 오류 문자열에 원문 token 포함~~ → 고정 문자열(`token is exhausted or expired` /
  `token not found`)만 반환
- ~~token을 먼저 소비하고 name 존재를 나중에 확인해 중복 name이 유효 token을 소진~~ →
  단일 transaction rollback

### 남은 노출 경계

- **`agent_endpoint`의 `server-key`가 secret-bearing 정규 필드다.** server는 이 query 값을 읽어
  join 응답 `worker.toml`의 `[grok].secret`에 평문으로 기록하며, 같은 값이 `workers.endpoint`
  컬럼에 저장되어 `GET /v1/workers/{id}`, Dashboard `/api/workers`·`/api/events`,
  MCP `fleet_list_workers` 응답으로 전파된다. `GET /v1/workers/{id}`는 현재 capability 행렬에
  등록되어 있지 않아 인증만 통과하면 호출되므로(→ [Authorization 계약](../security/authorization-and-audit.md)),
  워커 A의 operational token 보유자가 워커 B의 ACP secret을 얻어 orchestrator를 우회할 수 있다.
  이 필드는 redaction 대상이자 목표 계약 6번의 대상이다.
- join CLI는 Authorization header를 보내지 않으므로 API token 보호 모드에서는 middleware가 join을
  handler 이전에 거절할 수 있다(join route는 bootstrap body를 자체 인증 수단으로 처리한다).
- `fleet provision`(SSH 자동 프로비저닝)은 아직 `/v1/workers/join`과 배선되어 있지 않아
  legacy `bootstrap_token` 키를 기록한다. 5번의 fail-closed 거절 덕분에 조용히 동작하지는 않지만,
  프로비저닝된 워커는 `fleet-worker join` 재실행 또는 credential rotate 결과의 수동 반영이 필요하다.
- mTLS(9단계)와 staging rehearsal(10단계)은 착수 전이다.

| API 인증 설정 | 현재 join CLI | 결과 |
|---|---|---|
| 미설정 | 별도 인증 header 없음 | join 가능 |
| API token만 설정 | Authorization 미전송 | middleware에서 `401` |
| Cloudflare만 설정 | Cloudflare assertion 미전송 | edge middleware에서 거절 |
| 둘 다 설정 | 두 header 모두 미전송 | join 불가 |

따라서 현재 흐름은 `partial`이다. bootstrap 원문 저장과 scoped Worker identity는 해결됐고,
`server-key` 평문 전파와 provision 배선, mTLS가 남아 있다.

## 목표 계약

1. bootstrap token은 1회 join 승인에만 사용한다.
2. 서버는 token 원문 대신 digest와 식별자를 저장한다.
3. 승인 후에는 revocation·rotation 가능한 Worker-scoped credential 또는 mTLS identity를 발급한다.
4. register와 heartbeat는 Worker 자신의 scope에서만 동작한다.
5. join route의 인증 경계는 bootstrap token과 edge 보호를 분리해 정의한다.
6. agent endpoint URL에 secret을 넣지 않으며, join 응답·이벤트·Worker 설정에서 secret을 분리해
   전달·보관한다.
7. Worker name 검증·예약과 bootstrap token 소비를 하나의 transaction으로 처리해 실패 요청이
   token 사용량을 소진하지 않게 한다.

## 확정된 operational credential 전환

bootstrap token은 join 요청에서만 허용한다. join이 성공하면 Orchestrator는 새 `worker_id`에 결합된
고엔트로피 operational credential을 생성하고, **그 원문을 join 응답에서만 1회** 반환한다. 저장소에는
`credential_digest`, `worker_id`, `issued_at`, `expires_at`, `revoked_at`, `rotation_generation`만 남긴다.

```mermaid
sequenceDiagram
    participant W as "fleet-worker join"
    participant O as Orchestrator
    participant S as Credential Store

    W->>O: "bootstrap token + enrollment request"
    O->>S: "validate name + consume bootstrap + create worker + digest operational credential (transaction)"
    O-->>W: "worker_id + operational credential (one-time)"
    W->>W: "0600 worker.toml: operational_token only"
    W->>O: "register/heartbeat/deregister + Bearer operational credential"
    O->>S: "digest lookup; worker_id self-binding + revocation/expiry check"
```

Worker credential principal은 `worker:<worker_id>`이며 `worker:self`만 가진다. register에는
`existing_worker_id`가 반드시 credential의 worker ID와 같아야 하고, heartbeat/deregister의 path/body
worker ID도 같아야 한다. 이름·endpoint·labels·capability 변경은 별도 관리 command로 전환하기 전까지
기존 등록을 덮어쓸 수 없다.

`worker.toml`의 `bootstrap_token`은 제거 대상이다. 호환 기간에도 join 응답은 bootstrap 원문을 쓰지
않으며, old config는 명시 migration 없이는 operational API 호출에 사용할 수 없다. credential 회전은
새 token 발급 후 Worker inventory ACK를 확인하고 이전 token을 revoke한다. 재발급/삭제/만료는
audit event를 남기며 raw value는 어떤 log·event·DB export에도 기록하지 않는다.

token 보관·redaction·mTLS·권한의 정본은
[control-plane security model](../security/control-plane-security-model.md)이다.

## 검증 게이트

- join token은 발급 응답에서만 원문으로 보이고 목록·회수·오류·로그에는 나타나지 않는다.
- join 후 Worker credential은 bootstrap token과 다르며, rotate와 revoke를 테스트한다.
- API token 보호 모드에서도 명시된 join 인증 경계가 회귀 테스트로 검증된다.
- register/heartbeat는 다른 Worker나 일반 API token의 scope를 넘지 못한다.
- join 요청, 응답, event, 설정 파일 어디에도 endpoint query secret이 남지 않는지 검증한다.
- 중복 Worker name 또는 다른 등록 실패가 bootstrap token 사용량을 소비하지 않는지 검증한다.
