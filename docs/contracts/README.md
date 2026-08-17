---
type: domain-index
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/contracts/README.md"
last_verified: "2026-08-17"
---

# External Contracts

외부 소비자가 호출·구현해야 하는 wire contract를 분리한다. 이 디렉터리의 문서가 현재
계약의 정본이다. 충돌 시 이 디렉터리의 계약과 코드가 우선한다.

| 계약 | 독자와 정본 |
|---|---|
| HTTP API | Worker·자동화 — [`http-api.md`](./http-api.md) |
| MCP | MCP client — [`mcp-tools.md`](./mcp-tools.md) |
| Worker enrollment | Worker 운영자·daemon — [`worker-enrollment.md`](./worker-enrollment.md) |
| Dashboard API | first-party Dashboard — [`dashboard-api.md`](./dashboard-api.md) |
| Agent management | Agent 운영자·자동화 — [`agent-management.md`](./agent-management.md) |
