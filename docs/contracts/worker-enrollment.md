---
type: api-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/worker-enrollment.md"
last_verified: "2026-08-26"
last_verified_commit: "working-tree"
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
  join 응답 `worker.toml`의 `[grok].secret`에 평문으로 기록한다 — 이건 그 워커 자신에게 1회
  전달되는 정당한 경로이므로 그대로 유지한다. 같은 값이 `workers.endpoint` 컬럼에도 저장되는데,
  **이 컬럼을 되읽어 응답·이벤트로 내보내는 경로(`GET /v1/workers/{id}`, Dashboard
  `/api/workers`·`/api/events`, MCP `fleet_list_workers`)는 `#75`(완료, 2026-08-23)가
  `fleet_core::mask_server_key`로 마스킹했다** — 목표 계약 6번의 "응답·이벤트에서 secret 분리"
  절반이 닫혔다. 인가(누가 `GET /v1/workers/{id}`를 호출할 수 있는가)는 별개로 `#73`이 이미
  기본값 deny로 닫아 뒀다(→ [Authorization 계약](../security/authorization-and-audit.md)) —
  둘은 서로 다른 계층이라 각각 별도로 막아야 했다. **나머지 절반(URL query 밖으로 이전)은
  `#94`(완료, 2026-08-26)가 닫았다** — 단, 닫힌 것은 전선(wire)이지 **저장 형태**가 아니다.
  `fleet-transport::acp_transport::build_ws_client`가 다이얼 직전에 `fleet_core::split_server_key`로
  URL에서 `?server-key=<값>`을 떼어내고 그 값을 `Authorization: Bearer <값>` 헤더로 옮긴다.
  `workers.endpoint` 컬럼과 `worker.toml`의 저장 형태는 **하나도 바뀌지 않았다**. 그래서 `#75`의
  마스킹 의무는 그대로 유효하고(마스킹 대상이 사라진 것이 아니다), 대신 새 저장 위치도 생기지
  않았다 — 새 컬럼을 만들었다면 `#75`가 닫은 네 경로를 전부 다시 열어야 했을 것이다.
  `agent_endpoint`/`mtls_agent_endpoint`(생산자)도 바뀌지 않아 `#94` 전후에 등록된 워커가
  동일하게 동작한다.
- **`#94`가 실제로 닫는 구멍은 중간 프록시의 access log다.** nginx를 비롯한 역프록시는 요청
  라인(따라서 쿼리 문자열)을 기본 로그 포맷에 평문으로 남기지만 헤더는 남기지 않는다. 우리가
  소유하지 않는 로그에 secret이 축적되는 것을 막는 것이 이 항목의 실질이다.
- **"mTLS client cert만으로 충분"은 구조적으로 불가능하다.** `crates/fleet-transport`의
  `MtlsProxy`가 TLS를 **종단**하고 grok에는 평문 TCP를 넘기므로(`copy_bidirectional`) grok은
  client certificate를 애초에 볼 수 없다. 실측으로도 로컬에 있는 grok 세 버전(0.2.102 / 0.2.112 /
  1.0.5)이 모두 인증 없는 `/ws` 연결을 거절했다. 따라서 `#94`는 secret을 **이전**만 할 수 있고
  **제거**는 할 수 없다 — 제거는 grok 자체의 변경(이 저장소 범위 밖)을 요구한다. 세 버전 모두
  `Authorization: Bearer` 헤더로 옮긴 secret을 받아들였고(major 경계를 가로지른다), 그럼에도
  `FLEET_ACP_AUTH=query`로 예전 동작으로 되돌릴 수 있게 남겨 뒀다.
- join CLI는 Authorization header를 보내지 않으므로 API token 보호 모드에서는 middleware가 join을
  handler 이전에 거절할 수 있다(join route는 bootstrap body를 자체 인증 수단으로 처리한다).
- `fleet provision`(SSH 자동 프로비저닝)은 `/v1/workers/join`과 배선되어 있다(로드맵 `#82`) —
  프로비저너 자신은 admin bearer로 호스트별 1회용 bootstrap token만 발급하고, 대상 호스트에서
  `fleet-worker join --token-file -`을 원격 실행해 그 호스트가 직접 join하게 한다.
  `operational_token`은 대상 호스트만 알고 프로비저너는 보지도 저장하지도 않는다. 자세한 절차는
  [Worker provisioning](../deployment/worker-provisioning.md)에서 확인한다.
- mTLS(9단계)와 staging rehearsal(10단계)은 착수 전이다.

| API 인증 설정 | 현재 join CLI | 결과 |
|---|---|---|
| 미설정 | 별도 인증 header 없음 | join 가능 |
| API token만 설정 | Authorization 미전송 | middleware에서 `401` |
| Cloudflare만 설정 | Cloudflare assertion 미전송 | edge middleware에서 거절 |
| 둘 다 설정 | 두 header 모두 미전송 | join 불가 |

따라서 현재 흐름은 `partial`이다. bootstrap 원문 저장, scoped Worker identity, provision 배선,
응답·이벤트에서의 `server-key` 마스킹(`#75`), ACP 인증을 URL query 밖으로 옮기는 것(`#94`)은
해결됐고, mTLS 관련 나머지가 남아 있다. 목표 계약 6번은 "URL에 secret을 넣지 않는다"를
문자 그대로 요구하지만, 위에서 적었듯 grok이 URL 또는 헤더의 secret을 요구하는 한 저장 형태
자체를 비울 수는 없다 — 이 저장소가 닫을 수 있는 범위(전선과 읽기 경로)는 닫혔고, 남은 것은
grok 쪽 변경에 달려 있다.

### `#94`가 미룬 것과 그 이유

| 미룬 것 | 왜 지금 하지 않았나 | 무엇이 생기면 다시 볼 것인가 |
|---|---|---|
| 비mTLS 워커의 `/ws/{name}` nginx 홉이 `Authorization`을 상류로 전달하는지 실측 | 그 nginx 설정은 운영자 소유이고 이 저장소에 없다 — 로컬에서 재현할 대상 자체가 없다. `proxy_pass`는 기본적으로 요청 헤더를 전달하지만 `proxy_set_header`로 지운 배포는 조용히 401을 받게 된다 | 실제 배포의 nginx 설정을 이 저장소가 소유하게 되거나(프로비저너가 생성), staging rehearsal(`#83` 계열)에서 비mTLS 워커에 실제 디스패치를 돌릴 수 있게 될 때 |
| grok 0.2.102 미만 버전과의 호환성 | 로컬에 그 바이너리가 없다. 없는 것을 근거로 "지원한다/안 한다"를 쓸 수 없다 | 더 낮은 버전을 실제로 운용해야 할 때. 그때까지의 대비는 `FLEET_ACP_AUTH=query` 하나로 충분하다 |
| 워커별 인증 모드 자동 협상(헤더로 시도 → 거절되면 query로 재시도) | 헤더를 거절하는 grok이 **하나도 관측되지 않았다**. 가정 위에 협상 기계를 지으면 트리거되지 않는 분기가 영구히 남는다 | 헤더를 거절하는 배포가 실제로 나타날 때. 그전까지는 프로세스 단위 escape hatch(`FLEET_ACP_AUTH=query`)로 되돌린다 |
| 저장 형태에서 secret 제거(별도 컬럼/보관소) | grok이 secret 자체를 요구하는 한 어딘가엔 원문이 있어야 한다. 새 컬럼은 `#75`가 닫은 네 개 읽기 경로를 다시 열고 새 마스킹 의무를 만든다 — 유출면을 **옮기는** 것이지 줄이는 것이 아니다 | grok이 client certificate나 단명 토큰으로 인증할 수 있게 될 때 |

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
