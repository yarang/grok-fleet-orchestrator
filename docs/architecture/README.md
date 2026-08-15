---
type: wiki
status: canonical
source: "docs/architecture/README.md"
last_verified: "2026-08-15"
---

# 핵심 아키텍처 & API (Core Reference)

> 이 도메인은 시스템 내부 설계와 외부 API 계약을 다룬다. 전체 문서 카탈로그는
> [`../index.md`](../index.md), 운영 규칙은 [`../llm-wiki/README.md`](../llm-wiki/README.md)의
> 정본/사본 스키마를 따른다.

| 문서 | 정본 범위 |
|---|---|
| [`overview.md`](./overview.md) | 내부 구조 — Store trait, CircuitBreaker, WorkerSelector, ACP 전송, 워커 데몬, mTLS, 부트스트랩 토큰 |
| [`api-reference.md`](./api-reference.md) | 외부 계약 — HTTP REST 엔드포인트 + MCP 12종 도구 (2026-08-12 코드 대조로 이름/개수 정정, 2026-08-13 호스트/브레이커/토큰 4종 추가) |
| [`mcp-specification.md`](./mcp-specification.md) | 표준 연동 — Model Context Protocol (MCP) 표준 규격 준수 및 도구 상세 명세 |
| [`project-feature-design.md`](./project-feature-design.md) | 프로젝트 격리 — 워커/호스트 배타적(1:N) 격리 단위 및 하드 디스패치 설계 |
| [`agent-provisioning-design.md`](./agent-provisioning-design.md) | 에이전트 프로비저닝 — custom prompt, 메모리, MCP 도구 동적 주입 및 수명주기 설계 |
| [`agent-terminal-access-design.md`](./agent-terminal-access-design.md) | 에이전트 터미널 — tmux 기반 PTY 릴레이 및 WebSocket 인터랙티브 attach 설계 |
| [`agent-harness-composition-design.md`](./agent-harness-composition-design.md) | 에이전트 하네스 — prompt ➔ skill ➔ tool의 3계층 하네스 및 프로젝트 헌법 구성 |
| [`agent-runtime-vendor-design.md`](./agent-runtime-vendor-design.md) | 벤더 추상화 — AgentRunner 트레잇 기반 NetworkBind 및 StdioBridge 다중 런타임 수용 설계 |
| [`system-entities-mapping.md`](./system-entities-mapping.md) | 엔티티 매핑 — Project, Host, Worker, Agent, Task 간의 물리(WHERE)/행동(WHAT)/스코프(WHEN) 매핑 및 제약 규칙 |
| [`system-entities-critique.md`](./system-entities-critique.md) | 설계 비판 — 관계 매핑 설계의 비판적 분석, 잠재적 레이스 컨디션 및 토큰 최적화 대안 제안서 |
| [`multi-agent-realignment-report.md`](./multi-agent-realignment-report.md) | 교차 조정 — 코어/인프라/운영 도메인 간의 통합 가입, 드레인 소거, Nginx 업스트림 합의 요약 보고서 |
| [`feature-feasibility-testing.md`](./feature-feasibility-testing.md) | 검증 방안 — 드레인, Git 이관, 동적 스킬, 멀티 에이전트 구현 기술 분석 및 로컬 검증 테스트 사양 |
| [`log.md`](./log.md) | 아키텍처 로그 — 도메인 설계 개정 역사의 append-only 보존 기록 |

두 문서는 상호 보완적이다: `overview.md`가 "왜 이렇게 설계했는가"를, `api-reference.md`가
"실제로 무엇을 호출할 수 있는가"를 답한다. 값이 어긋나면(예: 엔드포인트 경로 불일치)
`api-reference.md`가 실제 라우터 코드(`crates/fleet-api/src/`)와 더 직접 대응하므로 우선한다 —
단, 최종 근거는 항상 코드다.
