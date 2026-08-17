---
type: review
authority: derived
implementation: proposed
verification: design-reviewed
source: "docs/reviews/project-model-review-2026-08-17.md"
last_verified: "2026-08-17"
---

# Project 모델 검토 부기

대상 정본은 [Project 모델과 거버넌스](../architecture/project-feature-design.md)와 [Project 관리 계약](../contracts/project-management.md)이다. 이 문서는 구현 전 선택이 필요한 비교 근거를 보관하며, 실행 규칙을 정의하지 않는다.

| 항목 | 관찰 | 정본 반영 |
|---|---|---|
| `ProjectAssign` | Operator의 배정과 Task 생성이 자동 provisioning을 거쳐 Admin 전용 `AgentCreate`를 우회할 수 있다. | 역할 배정이 승인되기 전 API·MCP를 차단한다. |
| Worker 격리 | Project Worker가 없을 때 일반 풀 폴백은 자원 소유 경계를 무너뜨린다. | hard eligibility와 기존 실패 경로를 유지한다. |
| Project 없는 Task | 기존 일반 풀 Task와의 호환성이 필요하다. | nullable `project_id`를 계속 일반 풀로 해석한다. |
| 장기 Agent 메모리 | Project 정책과 Agent 실행 상태의 책임이 다르다. | 메모리 보존은 Agent 도메인에서 결정한다. |

`ProjectAssign` 역할, 자동 provisioning의 권한 확인 위치, 정책 수정 권한은 보안 모델과 구현 계획에서 결정한 뒤 정본 계약에 반영한다.
