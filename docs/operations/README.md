---
type: domain-index
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/operations/README.md"
last_verified: "2026-08-17"
---

# Operations

운영 도메인은 배포·설정·Worker enrollment·장애 전환의 현재 정본 위치를 연결한다.

| 영역 | 현재 정본 또는 보존 위치 | 지위 |
|---|---|---|
| 배포·설정·게이트웨이 | [`../deployment/`](../deployment/README.md) | 현재 운영 기준 |
| Worker enrollment | [`../worker-bootstrap/`](../worker-bootstrap/README.md) | 현재 가입 흐름 기준 |
| Cold Standby | [`../architecture/control-plane-authority-and-failover.md`](../architecture/control-plane-authority-and-failover.md) | 운영 기관 결정 |

기존 `deployment/`와 `worker-bootstrap/` 경로는 안정적인 외부 참조를 위해 유지한다.
새 운영 자동화 요구는 먼저 Roadmap ID를 부여한다. 구현 전에 Architecture 또는 Security에서
권한·실패·복구 경계를 승인하고, 구현되어 검증된 절차만 Deployment Runbook으로 승격한다.
