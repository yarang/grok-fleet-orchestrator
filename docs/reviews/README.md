---
type: domain-index
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/reviews/README.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["documentation-governance"]
---

# Reviews

이 디렉터리는 설계 정본에 넣지 않는 비교, 감사, 대안 검토, 논의 부기 문서를 관리한다.
현재 구현 규칙은 각 도메인의 정본 문서가 우선하며, 이 문서들은 Derived 근거다.

| 문서 | 대상과 역할 |
|---|---|
| [`documentation-quality-review-2026-08-17.md`](./documentation-quality-review-2026-08-17.md) | 전 문서 품질·정본성·분량·검증 상태 감사 |
| [`document-rewrite-discussion-report-2026-08-17.md`](./document-rewrite-discussion-report-2026-08-17.md) | 1차 재작성의 코드·보안·정책 비교와 합의 |
| [`multi-agent-realignment-report.md`](./multi-agent-realignment-report.md) | 코어·인프라·운영 설계의 비교·합의 기록 |
| [`deployment-rewrite-review-2026-08-17.md`](./deployment-rewrite-review-2026-08-17.md) | deployment 재작성의 코드·보안·정책 비교와 삭제 근거 |
| [`agent-rewrite-review-2026-08-17.md`](./agent-rewrite-review-2026-08-17.md) | Agent 기능별 정본 분리의 근거·보안 게이트·후속 구현 조건 |
| [`project-model-review-2026-08-17.md`](./project-model-review-2026-08-17.md) | Project 권한·격리·일반 풀 호환성의 비교 근거 |
| [`system-entities-critique.md`](./system-entities-critique.md) | 엔티티 매핑의 동시성·격리·비용 대안 검토 |
| [`entity-lifecycle-consistency-review.md`](./entity-lifecycle-consistency-review.md) | Project·Task·Agent lifecycle 정합성 검토 기록 |
| [`feature-feasibility-testing.md`](./feature-feasibility-testing.md) | 드레인·이관·Skill·다중 Agent 기능의 feasibility 검토 |

새 review는 대상 정본, 비교 근거, 결정 결과, 정본 반영 링크를 적는다. 설계 규칙이나 운영
절차를 여기서 새로 정의하지 않는다.
