---
type: domain-index
authority: canonical
implementation: partial
verification: code-checked
source: "docs/worker-bootstrap/README.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["worker-bootstrap"]
---

# Worker 가입

> 전체 문서 카탈로그는 [`../index.md`](../index.md).

이 디렉터리는 운영자가 Worker를 처음 가입시키는 현재 절차와 정본 탐색만 담당한다. 가입·등록·
heartbeat의 현재·목표 외부 계약은 [Worker enrollment](../contracts/worker-enrollment.md), token과
Worker identity의 보안 불변식은 [Control Plane 보안 모델](../security/control-plane-security-model.md),
SSH 설치 자동화는 [Worker 프로비저닝](../deployment/worker-provisioning.md)이 소유한다.

현재 self-service join은 원문 bootstrap token을 Worker의 지속 bearer로 다시 기록하고,
API-token 또는 Cloudflare 보호 모드에 필요한 인증 header를 보내지 않는다. 따라서 일반
프로덕션 가입 절차가 아니며, 실행 전에 계약의 차단 조건을 확인해야 한다.

## 읽기 순서와 책임

| 문서 | 책임 | 상태 |
|---|---|
| [수동 가입 절차](join.md) | 현재 `fleet token issue`와 `fleet-worker join` 실행·검증·중단 조건 | derived · partial |

미구현 token-file, SFTP token 파쇄, cloud-init 전달과 Worker-scoped credential 발급 절차는 이
디렉터리의 지원 Runbook이 아니다. 목표 계약이 승인·구현되면 정본을 먼저 변경하고 이 절차를
동기화한다.
