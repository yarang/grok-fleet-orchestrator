---
type: domain-index
authority: canonical
implementation: partial
verification: code-checked
source: "docs/deployment/README.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["deployment"]
---

# 배포 & 인프라 (Deployment & Infra)

이 도메인은 설치, 구성, 일상 운영, 복구, 네트워크 경계의 현재 절차만 다룬다.
설계 결정은 `architecture/`, 외부 계약은 `contracts/`, 비교·감사 기록은 `reviews/`가 소유한다.

## 읽기 순서

1. 새 설치: install → configuration → worker-provisioning → operations
2. 운영 장애: troubleshooting → backup-recovery
3. 공개 경계 변경: topology → reverse-proxy → configuration
4. LLM gateway: litellm-gateway → reverse-proxy → configuration

| 문서 | 상태 | 역할 |
|---|---|---|
| [`install.md`](./install.md) | 🟢 Runbook | release 설치·artifact 검증·서비스 등록 준비 |
| [`configuration.md`](./configuration.md) | 🟢 정본 | env/TOML 경계, secret 경로·권한, production preflight |
| [`worker-provisioning.md`](./worker-provisioning.md) | 🟢 Runbook·부분 구현 | SSH host-key 검증, Worker binary·설정·service 배포와 실패 확인 |
| [`operations.md`](./operations.md) | 🟢 Runbook | 시작·상태 확인·안전 중단·초기 대응 |
| [`backup-recovery.md`](./backup-recovery.md) | 🟢 Runbook | DB 백업·새 DB 복원·in-place 복구 게이트 |
| [`troubleshooting.md`](./troubleshooting.md) | 🟢 Runbook | 증상별 증거 수집과 복구 경로 |
| [`reverse-proxy.md`](./reverse-proxy.md) | 🟡 부분 구현 | Nginx TLS·trusted proxy·공개 endpoint 경계 |
| [`topology.md`](./topology.md) | 🔵 사본·부분 구현 | Architecture 정본을 배포 관점으로 요약한 토폴로지·신뢰 경계 |
| [`litellm-gateway.md`](./litellm-gateway.md) | 🟢 Runbook·부분 구현 | liteLLM 준비·기동·검증·rollback과 Worker 경유 확인 |
| [`mcp-clients.md`](./mcp-clients.md) | 🟡 부분 구현 | Claude Code·Antigravity CLI stdio 연결, Gemini CLI 단종 안내, ChatGPT 미지원 사유 |

## 인접 정본

- [Control Plane 권한과 장애 전환](../architecture/control-plane-authority-and-failover.md)
- [Worker enrollment](../contracts/worker-enrollment.md)
- [Control-plane security model](../security/control-plane-security-model.md)
