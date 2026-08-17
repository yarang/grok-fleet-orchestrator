---
type: domain-index
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/roadmap/README.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["planning"]
---

# Roadmap 도메인

이 도메인은 승인된 설계의 구현 순서, 상태와 완료 게이트를 관리한다. 시스템의 현재 동작,
아키텍처 계약, 운영 절차와 검토 이력은 소유하지 않는다.

## 읽기 순서와 문서 책임

| 문서 | 책임 | Authority | 구현·검증 상태 |
|---|---|---|---|
| [구현 로드맵](roadmap.md) | 영구 작업 ID, 활성 구현 순서, 상태, 정본 링크와 완료 게이트 | canonical | partial · code-checked |

Architecture는 “어떻게 동작해야 하는가”, Roadmap은 “무엇을 언제 구현하고 언제 완료로
판정하는가”에 답한다. 상세 설계, 비교 분석, 회고, 테스트 개수와 운영 명령은 Roadmap에 복제하지
않는다.

## 상태와 ID 규칙

| 상태 | 의미 |
|---|---|
| 제안 | 요구와 가치가 등록됐지만 설계가 승인되지 않음 |
| 설계 필요 | 구현 전에 정본 설계 결정이 필요함 |
| 설계 확정·구현 대기 | 정본과 완료 게이트가 승인됐고 착수 전임 |
| 부분 구현 | 일부 게이트만 충족함 |
| 구현 중 | 코드 변경이 진행 중이며 완료 게이트는 아직 열려 있음 |
| 완료 | 코드·테스트·필요한 운영 문서가 완료 게이트를 충족함 |
| 강등·폐기 | 현재 실행 대기열에서 제외됨; ID는 보존함 |

- P0는 즉시 서비스 차단, P1은 보안·데이터·실행 신뢰성, P2는 기능·확장, P3는 개선이다.
- `#N`은 영구 참조 키다. 완료·강등·폐기 뒤에도 삭제하거나 재사용하지 않는다.
- 새 ID는 요구가 승인돼 실행 대기열에서 추적할 가치가 생겼을 때 발급한다.
- 완료는 파일 존재가 아니라 정상·실패 경로 테스트와 필요한 운영 검증으로 판정한다.
- 현재 계약은 해당 canonical 문서, 설계 변경 경위는 Git 이력 또는 `docs/reviews/`가 담당한다.

인접 정본은 [Architecture](../architecture/README.md), [Security](../security/README.md),
[Contracts](../contracts/README.md), [Deployment](../deployment/README.md)에서 찾는다.
