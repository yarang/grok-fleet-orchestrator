---
type: operations-reference
authority: canonical
implementation: partial
verification: code-checked
source: "docs/deployment/configuration.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["deployment", "security"]
---

# 구성과 비밀 관리

Orchestrator는 TOML이 아니라 `fleet serve`의 CLI·환경변수로 구성한다. `DATABASE_URL`은 필수다.
Worker는 `worker.toml`을 읽는다. 이 문서는 경로·소유권·사전 검증을 정의하며, secret 값은 기록하지 않는다.

## 현재 구성 경계

| 대상 | 현재 경로 또는 입력 | 주의 |
|---|---|---|
| Orchestrator | `/etc/fleet/fleet.env` 또는 service environment | `DATABASE_URL` 필수 |
| Worker | `/etc/fleet/worker.toml` | 현재 bootstrap token 원문이 기록될 수 있음 |
| Provisioner inventory | `/etc/fleet/workers.yaml` | 운영은 strict host-key 검증 권장 |
| SSH known hosts | `/etc/fleet/known_hosts` | `fleet scan-host-keys`로 사전 수집 가능 |

## 프로덕션 사전 검증

1. 외부 bind를 사용하면 API token 또는 Cloudflare audience를 반드시 설정한다.
2. no-auth와 non-loopback bind 조합이면 시작을 중단한다. 현재 바이너리는 이 조합을 자동 차단하지 않는다.
3. `fleet.env`, private key, `worker.toml`은 서비스 계정만 읽도록 제한한다.
4. `FLEET_TRUSTED_PROXIES`는 실제 reverse proxy 대역만 포함한다.
5. `/health`와 `/metrics`의 무인증 노출이 gateway ACL과 일치하는지 확인한다.

## 현재 구현과 목표의 차이

- `liveness_mode`, on-demand ACP probe, root-owned `fleet-worker-wipe`는 목표 계약이며 현재 배포
  자산으로 검증되지 않았다. 설정 파일에 현재 지원 값처럼 넣지 않는다.
- Worker join은 bootstrap token과 지속 credential을 분리하지 않았다. token 보관·rotation·revoke의
  목표는 [Worker enrollment](../contracts/worker-enrollment.md)과
  [Control-plane security model](../security/control-plane-security-model.md)을 따른다.
- mTLS는 선택 feature이며 provisioner가 certificate/key 전달까지 완료하지 않는다.

## 검증

서비스 시작 전 설정 revision/hash, binary version, service account, bind 주소를 운영 기록에 남긴다.
시작 뒤에는 인증 요청·비인증 요청·health·DB 접근을 별도로 확인한다.
