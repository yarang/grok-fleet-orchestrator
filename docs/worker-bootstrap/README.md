---
type: domain-index
authority: canonical
implementation: partial
verification: code-checked
source: "docs/worker-bootstrap/README.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["worker-bootstrap"]
---

# 워커 부트스트랩 & 가입 인증 (Worker Bootstrap & Join Auth)

> 전체 문서 카탈로그는 [`../index.md`](../index.md).

Worker enrollment의 현재·목표 외부 계약은 [`../contracts/worker-enrollment.md`](../contracts/worker-enrollment.md)가
정본이다. 이 디렉터리는 그 계약을 구현·운영하기 위한 절차, 제안, 보존된 구현 스냅샷을 둔다.
현재 join 경로는 원문 bootstrap token의 재사용과 API-token 보호 모드의 인증 경계가 해결되지 않은
`partial` 상태이므로, 여기의 문서를 프로덕션 운영 절차로 읽기 전에 계약의 “현재 구현” 절을 확인한다.

![Worker Join Flow Overview Diagram](../assets/diagrams/worker-bootstrap/join-flow-overview.mermaid)

| 문서 | 정본 범위 |
|---|---|
| [`join-authentication.md`](./join-authentication.md) | 🔵 목표 가입 보안 모델 — 현재 계약은 `contracts/worker-enrollment.md`를 우선 |
| [`token-delivery.md`](./token-delivery.md) | 🟡 전달 방식 제안 — 현재 구현과 미구현 채널을 구분해 읽음 |
| [`ssh-provisioning.md`](./ssh-provisioning.md) | 🟡 SSH 프로비저닝 보존 참조 — 구형 token-file 절차는 사용 금지 |
| [`bootstrap-release-v0.2.md`](./bootstrap-release-v0.2.md) | 🔵 코드 대조 기반 release 스냅샷 |
| [`serve-and-bootstrap-design.md`](./serve-and-bootstrap-design.md) | ⚫ 여러 도메인이 섞인 보존 참조 — Architecture/Contracts/Bootstrap으로 분리 중 |
