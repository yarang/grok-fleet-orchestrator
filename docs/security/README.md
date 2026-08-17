---
type: domain-index
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/security/README.md"
last_verified: "2026-08-17"
owners: ["security"]
---

# Security

Security는 모든 도메인에 적용되는 신원, 권한, Worker credential, secret 경계를 소유한다.
HTTP·MCP·Dashboard의 개별 호출 형식은 [Contracts](../contracts/README.md), 실행 격리는
[Agent Architecture](../architecture/agents/README.md), 배포 절차는 [Deployment](../deployment/README.md)가
소유한다.

| 읽는 순서 | 문서 | 역할 |
|---:|---|---|
| 1 | [Control Plane 보안 모델](control-plane-security-model.md) | 목표 신원·capability·secret·fail-closed 계약 |
| 2 | [Worker enrollment 계약](../contracts/worker-enrollment.md) | join·register·heartbeat의 현재 제한과 완료 게이트 |
| 3 | [보안 발견 기록](reports/README.md) | 해결·미해결 항목의 historical 근거 |

설계가 구현보다 우선한다. 구현이 설계와 다를 때 현재 동작은 관련 계약과 코드 근거로
확인하고, 정본에는 목표·차이·검증 게이트만 반영한다.
