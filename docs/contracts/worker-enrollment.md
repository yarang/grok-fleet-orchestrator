---
type: api-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/worker-enrollment.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["fleet-api", "fleet-worker"]
---

# Worker enrollment 계약

이 문서는 Worker join, register, heartbeat의 현재 구현과 목표 보안 계약을 분리한다.
운영자는 목표 계약이 구현되기 전 self-service join을 일반 운영 절차로 취급하면 안 된다.

## 현재 구현

1. `fleet-worker join`은 bootstrap token을 요청 본문에 넣어 `/v1/workers/join`을 호출한다.
2. 서버는 token 사용량을 원자적으로 소비하고, 응답으로 생성한 `worker.toml`에 같은 원문 token을
   다시 기록한다.
3. Worker daemon은 그 값을 register와 heartbeat의 Bearer 값으로 계속 사용한다.

또한 server는 `agent_endpoint` query의 `server-key` 값을 읽어 join 응답의
`worker.toml` `[grok].secret`에 평문으로 다시 기록한다. bootstrap token과 이 secret은 현재
DB·응답·로컬 파일 노출 경계를 만족하지 않는다.

이 흐름에는 두 제한이 있다.

- token 사용량은 join 시 소비될 수 있으나 이후 Bearer 인증은 별도 정적 API token 목록을 사용한다.
- join CLI는 Authorization header를 보내지 않으므로 API token 보호 모드에서는 middleware가 join을
  handler 이전에 거절할 수 있다.
- server는 token을 먼저 소비하고 같은 Worker name의 존재 여부를 나중에 확인한다. 중복 name
  요청은 등록 실패와 별개로 유효 token을 소진할 수 있다.

| API 인증 설정 | 현재 join CLI | 결과 |
|---|---|---|
| 미설정 | 별도 인증 header 없음 | join 가능 |
| API token만 설정 | Authorization 미전송 | middleware에서 `401` |
| Cloudflare만 설정 | Cloudflare assertion 미전송 | edge middleware에서 거절 |
| 둘 다 설정 | 두 header 모두 미전송 | join 불가 |

따라서 현재 흐름은 `partial`이며, 원문 token의 DB·파일 저장과 scoped Worker identity를 해결하지
않은 상태다.

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

token 보관·redaction·mTLS·권한의 정본은
[control-plane security model](../security/control-plane-security-model.md)이다.

## 검증 게이트

- join token은 발급 응답에서만 원문으로 보이고 목록·회수·오류·로그에는 나타나지 않는다.
- join 후 Worker credential은 bootstrap token과 다르며, rotate와 revoke를 테스트한다.
- API token 보호 모드에서도 명시된 join 인증 경계가 회귀 테스트로 검증된다.
- register/heartbeat는 다른 Worker나 일반 API token의 scope를 넘지 못한다.
- join 요청, 응답, event, 설정 파일 어디에도 endpoint query secret이 남지 않는지 검증한다.
- 중복 Worker name 또는 다른 등록 실패가 bootstrap token 사용량을 소비하지 않는지 검증한다.
