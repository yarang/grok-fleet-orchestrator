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

이 흐름에는 두 제한이 있다.

- token 사용량은 join 시 소비될 수 있으나 이후 Bearer 인증은 별도 정적 API token 목록을 사용한다.
- join CLI는 Authorization header를 보내지 않으므로 API token 보호 모드에서는 middleware가 join을
  handler 이전에 거절할 수 있다.

따라서 현재 흐름은 `partial`이며, 원문 token의 DB·파일 저장과 scoped Worker identity를 해결하지
않은 상태다.

## 목표 계약

1. bootstrap token은 1회 join 승인에만 사용한다.
2. 서버는 token 원문 대신 digest와 식별자를 저장한다.
3. 승인 후에는 revocation·rotation 가능한 Worker-scoped credential 또는 mTLS identity를 발급한다.
4. register와 heartbeat는 Worker 자신의 scope에서만 동작한다.
5. join route의 인증 경계는 bootstrap token과 edge 보호를 분리해 정의한다.

token 보관·redaction·mTLS·권한의 정본은
[control-plane security model](../security/control-plane-security-model.md)이다.

## 검증 게이트

- join token은 발급 응답에서만 원문으로 보이고 목록·회수·오류·로그에는 나타나지 않는다.
- join 후 Worker credential은 bootstrap token과 다르며, rotate와 revoke를 테스트한다.
- API token 보호 모드에서도 명시된 join 인증 경계가 회귀 테스트로 검증된다.
- register/heartbeat는 다른 Worker나 일반 API token의 scope를 넘지 못한다.
