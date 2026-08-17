---
type: domain-index
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/contracts/README.md"
last_verified: "2026-08-17"
---

# External Contracts

외부 소비자가 호출·구현해야 하는 wire contract를 분리한다. 이 디렉터리의 문서는
**목표 설계의 정본**이다. 구현이 설계와 다르면 설계가 이후 변경의 방향을 정하고,
문서는 `implementation` 상태와 현재 차이를 명시한다. 현재 동작의 사실 확인에는
연결된 코드·OpenAPI·테스트를 사용하며, 구현이 설계대로 동작한다고 추정하지 않는다.

| 계약 | 독자와 정본 |
|---|---|
| HTTP API | Worker·자동화 — [`http-api.md`](./http-api.md) |
| MCP | MCP client — [`mcp-tools.md`](./mcp-tools.md) |
| Worker enrollment | Worker 운영자·daemon — [`worker-enrollment.md`](./worker-enrollment.md) |
| Dashboard API | first-party Dashboard — [`dashboard-api.md`](./dashboard-api.md) |
| Agent management | Agent 운영자·자동화 — [`agent-management.md`](./agent-management.md) |
| Project management | Dashboard·MCP client — [`project-management.md`](./project-management.md) |
