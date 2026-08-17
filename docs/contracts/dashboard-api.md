---
type: api-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/dashboard-api.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["fleet-dashboard"]
---

# Dashboard API 계약

이 문서는 first-party Dashboard가 사용하는 `/api/*` 표면의 정본 진입점이다. route·session·RBAC의
실제 구현은 [`crates/fleet-dashboard/src/app.rs`](../../crates/fleet-dashboard/src/app.rs)와
schema·handler 테스트를 기준으로 한다.

Dashboard API는 같은 저장소의 first-party UI와 함께 배포된다. 따라서 `/v1` Worker API와 달리
독립 외부 클라이언트 호환성을 아직 보장하지 않는다. 외부 공개가 필요해지면 versioning과 OpenAPI
범위를 별도 결정으로 추가한다.

인증·세션·RBAC의 목표 정책은
[control-plane security model](../security/control-plane-security-model.md)이 정본이다. 이 문서는
route 목록을 중복 관리하지 않으며, API surface 확장 시 구현 근거와 함께 세부 계약을 보강한다.
