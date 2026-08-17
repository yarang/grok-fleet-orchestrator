---
type: governance-policy
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/governance/documentation-policy.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["documentation-governance"]
---

# 문서 관리 정책

이 문서는 Grok Fleet Orchestrator 문서의 책임, 정본 관계, 메타데이터와 부기 원칙을 정의한다.
재작성 순서와 완료 검사는 [문서 재작성 가이드](documentation-rewrite-guide.md)가 담당하고,
특정 시점의 감사·비교 결과는 [`reviews/`](../reviews/README.md), 변경 이력은
[`docs/log.md`](../log.md)가 담당한다.

## 1. 문서 구조와 책임

문서는 독자와 변경 주기가 같은 기능 도메인에 둔다. 시스템 설계는 `architecture/`, 외부 계약은
`contracts/`, 현재 배포 절차는 `deployment/`, 보안 정책은 `security/`, 비교·감사 기록은
`reviews/`, 협업 규칙은 `governance/`가 소유한다.

문서가 있는 각 도메인은 `README.md` 또는 `index.md` 하나를 진입점으로 둔다. 도메인 진입점은
범위, 읽기 순서, 파일별 책임과 정본 관계를 관리한다. 루트 [`docs/index.md`](../index.md)는 도메인
탐색을 위한 카탈로그이며, 세부 문서 목록의 정본은 각 도메인 진입점이다. 독립 부기 체계를 가진
하위 위키는 자체 인덱스와 로그 또는 레지스트리를 사용할 수 있다.

`docs/assets/`는 문서 도메인이 아니라 공유 자산 컨테이너다. 다이어그램 목록과 규칙은
[`assets/diagrams/README.md`](../assets/diagrams/README.md)가 관리한다.

## 2. 정본과 파생 문서

문서 하나는 독자 질문 하나와 기능상 책임 하나를 담당한다. 같은 사실을 여러 문서가 설명해야
한다면 하나만 `canonical`로 두고 나머지는 `derived`로 표시한다.

- `canonical`: 해당 계약·정책·절차의 우선 출처다.
- `derived`: 정본을 요약하거나 코드 구조·검증 근거를 설명하며 새 정책을 만들지 않는다.
- `historical`: 특정 시점의 기록이며 현재 지침으로 사용하지 않는다.
- `deprecated`: 삭제 전 전환 상태다. 대체 경로와 inbound link를 정리한 뒤 제거한다.

정본과 파생 문서가 충돌하면 정본이 우선한다. 변경은 정본에 먼저 반영한 뒤 파생 문서를
동기화한다. `canonical`은 구현 완료나 운영 검증을 의미하지 않으므로 구현·검증 상태를 별도
필드로 표시한다.

## 3. 현재 사실과 목표 계약

API, 환경변수, 심볼, 포트, 기본값과 권한은 실제 코드·테스트·설정 자산으로 확인한다. 예시 값이
필요하면 실제 값으로 오인되지 않게 명시하며 secret, token, password는 기록하지 않는다.

현재 동작과 목표 설계가 다르면 같은 주장으로 섞지 않는다. 현재 사실에는 코드 또는 테스트
근거를 연결하고, 목표는 `proposed` 또는 `partial` 상태와 완료 조건을 함께 기록한다. 보안·인증·
credential 전달처럼 조합에 따라 동작이 달라지는 내용은 표나 상태 흐름으로 표현한다.

## 4. 메타데이터

새 문서와 크게 개정한 문서는 다음 프론트매터를 사용한다.

```yaml
---
type: "<문서의 기능 유형>"
authority: canonical | derived | historical | deprecated
implementation: not-applicable | proposed | partial | implemented | retired
verification: assumed | design-reviewed | code-checked | integration-tested | production-proven
source: "docs/<정본 또는 자기 경로>.md"
last_verified: "YYYY-MM-DD"
last_verified_commit: "<commit 또는 working-tree>"
owners: ["<domain owner>"]
---
```

`type`은 폐쇄 enum이 아니다. `domain-index`, `architecture-decision`, `api-contract`, `runbook`,
`governance-policy`처럼 기능을 드러내는 안정적인 kebab-case 값을 사용한다. 다른 필드는 위 허용값을
사용한다. `source`는 canonical 문서에서는 자기 경로, derived 문서에서는 우선하는 정본 경로다.

## 5. 링크와 시각 자료

- Markdown 링크는 저장소 상대 경로를 사용한다. 개인 환경의 `file:///` 절대 링크는 금지한다.
- 삭제·이동 전 파일명, 표시명, 상대 링크와 자산 참조를 검색한다.
- 구조·상태·흐름은 필요한 경우 Mermaid 또는 SVG로 표현하고 ASCII-art 박스는 사용하지 않는다.
- 50줄 미만의 단일 문서용 Mermaid는 인라인으로 둘 수 있다.
- 100줄 이상, 재사용되는 Mermaid와 모든 SVG는 `docs/assets/diagrams/<domain>/`에 둔다.
- 다이어그램을 변경하면 참조 문서와 [`assets/diagrams/README.md`](../assets/diagrams/README.md)를
  함께 확인한다.

## 6. 중앙 부기

- 도메인 파일과 읽기 순서는 해당 도메인 진입점에서 관리한다.
- 도메인 추가·이동·폐기 또는 탐색 구조 변경은 [`docs/index.md`](../index.md)에 반영한다.
- 문서의 생성·대규모 재작성·삭제·정합성 수정은 [`docs/log.md`](../log.md)에 `ingest` 또는
  `lint`로 기록한다.
- 비교, 감사, 대안과 논의 과정은 [`docs/reviews/`](../reviews/README.md)에 두고 정본에는 확정된
  결과만 반영한다.

이 정책의 적용 절차와 검증 게이트는 [문서 재작성 가이드](documentation-rewrite-guide.md)를 따른다.
