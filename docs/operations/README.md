---
type: domain-index
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/operations/README.md"
last_verified: "2026-08-16"
---

# Operations

운영 도메인은 배포·설정·Worker enrollment·장애 전환·향후 운영 자동화 제안을 다룬다.
현재 실행 가능한 runbook과 미구현 제안을 섞지 않는다.

| 영역 | 현재 정본 또는 보존 위치 | 지위 |
|---|---|---|
| 배포·설정·게이트웨이 | [`../deployment/`](../deployment/README.md) | 현재 운영 기준 |
| Worker enrollment | [`../worker-bootstrap/`](../worker-bootstrap/README.md) | 현재 가입 흐름 기준 |
| Cold Standby | [`../architecture/control-plane-authority-and-failover.md`](../architecture/control-plane-authority-and-failover.md) | 운영 기관 결정 |
| 운영 자동화 제안 | [`proposals/`](./proposals/README.md) | 미구현 제안, 운영 기준 아님 |

기존 `deployment/`와 `worker-bootstrap/` 경로는 안정적인 외부 참조를 위해 유지한다.
새 운영 문서는 이 도메인 지도에서 어느 영역에 속하는지 먼저 선언한다.
