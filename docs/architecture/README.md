---
type: architecture-index
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/architecture/README.md"
last_verified: "2026-08-15"
---

# 핵심 아키텍처 & API (Core Reference)

> 이 도메인은 시스템 내부 설계와 외부 API 계약을 다룬다. 설계의 시작점과 각 주제의
> 단일 정본은 [아키텍처 정본 지도](canonical-map.md)다. 아래 목록은 정본뿐 아니라
> 보존된 검토·근거 문서도 포함하므로, 현재 규칙을 확인할 때는 정본 지위를 먼저 따른다.
> 전체 문서 카탈로그는 [`../index.md`](../index.md)이며, 운영 규칙은
> [`../governance/documentation-rewrite-guide.md`](../governance/documentation-rewrite-guide.md)를 따른다.

| 문서 | 역할 (정본 지위는 canonical-map.md 참조) |
|---|---|
| [`overview.md`](./overview.md) | 빠른 탐색 — 시스템 경계, 현재 구현 상태, 정본 읽기 순서 |
| [`implementation-reference.md`](./implementation-reference.md) | 구현 근거 — Store, CircuitBreaker, WorkerSelector, ACP, Worker daemon, mTLS, SSH와 보존된 구현 이력 |
| [`canonical-map.md`](./canonical-map.md) | 정본 지도 — 주제별 단일 정본과 Derived 구현 근거의 경계 |
| [`control-plane-availability.md`](./control-plane-availability.md) | 운영 기관 — Single Active Primary + Cold Standby, lease/epoch/fencing 및 장애 전환 |
| [`task-execution-consistency.md`](./task-execution-consistency.md) | 실행 의미론 — TaskAttempt, generation, CAS, 재시도, 멱등성 및 부작용 fencing |
| [`worker-liveness-policy.md`](./worker-liveness-policy.md) | Worker liveness — periodic heartbeat와 on-demand ACP probe의 선택·상태·스케줄링 계약 |
| [`../contracts/README.md`](../contracts/README.md) | 외부 계약 — HTTP, MCP, Dashboard, Worker enrollment의 현재 정본 진입점 |
| [`project-feature-design.md`](./project-feature-design.md) | Project model & governance — 정책·권한·격리·host/worker 배정 제약 |
| [`task-management-design.md`](./task-management-design.md) | Task management — Project 귀속, 제출·의존성·취소·결과·감사 정본 |
| [`project-task-agent-lifecycle.md`](./project-task-agent-lifecycle.md) | Lifecycle contract — Project/Task/Attempt/Agent의 교차 전이·snapshot·drain/archive |
| [`agents/README.md`](./agents/README.md) | Agent 실행 플랫폼 — 격리, provisioning, runtime, harness, tool, memory, terminal의 기능별 정본 진입점 |
| [`system-entities-mapping.md`](./system-entities-mapping.md) | 엔티티 매핑 — Project, Host, Worker, Agent, Task 간의 물리(WHERE)/행동(WHAT)/스코프(WHEN) 매핑 및 제약 규칙 |
| [`system-entities-critique.md`](./system-entities-critique.md) | 설계 비판 — 관계 매핑 설계의 비판적 분석, 잠재적 레이스 컨디션 및 토큰 최적화 대안 제안서 |
| [`entity-lifecycle-consistency-review.md`](./entity-lifecycle-consistency-review.md) | Lifecycle 검토 — Project·Task·Agent 상태/삭제/드레인 정합성 공백과 확정 전 제안 |
| [`feature-feasibility-testing.md`](./feature-feasibility-testing.md) | 검증 방안 — 드레인, Git 이관, 동적 스킬, 멀티 에이전트 구현 기술 분석 및 로컬 검증 테스트 사양 |
| [`host-integrity-and-security-monitoring-design.md`](./host-integrity-and-security-monitoring-design.md) | 무결성 감시 — 워커 비관리 파일/패키지 변경 실시간 커널 감시, 3계층 필터링 및 LLM 위험성 분석 아키텍처 보고서 |
| [`intelligent-task-routing-and-budget-control-design.md`](./intelligent-task-routing-and-budget-control-design.md) | 지능형 라우팅 & 예산 제어 — FreeRouter 정책 Rust 흡수, 3단계 소프트 예산 통제, Compact 압축 및 무비용 텔레메트리 설계서 |
| [`log.md`](./log.md) | 아키텍처 로그 — 도메인 설계 개정 역사의 append-only 보존 기록 |

`overview.md`는 어디를 읽어야 하는지를, `implementation-reference.md`는 코드 구조와 구현 제약을
답한다. 호출 가능한 HTTP/MCP/Dashboard/Worker enrollment 표면은 `contracts/`가 답한다. 값이
어긋나면 해당 정본과 코드가 우선한다.
