---
type: domain-index
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/contracts/README.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["api-contracts"]
---

# Consumer-facing Contracts

외부 Worker·자동화·MCP client와 first-party Dashboard가 의존하는 service contract를 분리한다.
CLI 사용법과 설정 파일 형식은 이 디렉터리의 범위가 아니다. 구현이 설계와 다르면 문서는
`implementation` 상태와 현재 차이를 명시하며, 구현이 목표대로 동작한다고 추정하지 않는다.

현재 동작은 router·handler·테스트가 실행 사실이고, OpenAPI와 `tools/list`가 기계 판독 wire schema다.
이 디렉터리의 prose는 현재 표면의 탐색·보안 경계와 목표 계약을 소유한다. 충돌이 발견되면 현재
사실을 숨기지 않고 차이를 기록한 뒤 코드 또는 목표 계약 중 승인된 쪽을 함께 수정한다.

## 현재 또는 부분 구현 계약

| 계약 | 소비자 | 기계 판독 근거 | 구현 상태 |
|---|---|---|---|
| [HTTP API](./http-api.md) | Worker·자동화 | OpenAPI | `partial` |
| [MCP](./mcp-tools.md) | MCP client | `tools/list` | `partial` |
| [Worker enrollment](./worker-enrollment.md) | Worker 운영자·daemon | HTTP schema·코드 | `partial` |
| [Dashboard API](./dashboard-api.md) | first-party Dashboard | route·schema 코드 | `partial` |

## Roadmap에 등록된 제안 계약

아래 문서의 계약은 **전부 또는 일부가** 아직 호출 가능한 기능이 아니다. 어느 부분이 실제로
동작하는지는 각 문서의 `implementation` 값과 그 문서가 지목하는 정본을 따른다 — 예를 들어
Project 계약의 MCP 도구 세 개(`fleet_create_project`, `fleet_list_projects`,
`fleet_delete_project`)는 [MCP 도구 계약](./mcp-tools.md)이 현재 도구 표면으로 정본화했지만,
같은 문서의 PATCH·host/worker 배정은 여전히 제안 상태다. 연결된 Roadmap 항목과 활성화 게이트가
충족되지 않은 부분은 wire contract가 승인된 것으로 보거나 클라이언트 호환성 약속·운영 절차로
사용하지 않는다.

| 계약 | 소비자 | Roadmap | 구현 상태 |
|---|---|---|---|
| [Agent management](./agent-management.md) | Agent 운영자·자동화 | [#49](../roadmap/roadmap.md#기능-확장-48-52) | `proposed` |
| [Project management](./project-management.md) | Dashboard·MCP client | [#48](../roadmap/roadmap.md#기능-확장-48-52) | `proposed` |
