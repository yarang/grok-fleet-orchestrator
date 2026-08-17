---
type: runbook
authority: canonical
implementation: partial
verification: code-checked
source: "docs/deployment/troubleshooting.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["operations"]
---

# 장애 진단 Runbook

진단은 증상을 바꾸기 전에 증거를 수집한다. secret, bootstrap token, authorization header를
명령행·URL·공유 로그에 넣지 않는다.

| 증상 | 먼저 확인할 증거 | 다음 조치 |
|---|---|---|
| 서비스가 시작하지 않음 | service journal, 환경 파일 존재·권한, `DATABASE_URL` | 설정을 고치고 `fleet doctor` 재실행 |
| API가 예상보다 공개됨 | bind 주소, API token/Cloudflare audience, gateway ACL | 공개를 중단하고 configuration preflight 재검증 |
| Worker가 등록되지 않음 | Worker config check, join 차단 조건, host-key 정책 | [Worker enrollment](../contracts/worker-enrollment.md) 확인 후 격리된 개발 경로만 사용 |
| Worker가 offline | heartbeat 기록, Worker service journal, ACP/네트워크 상태 | host를 격리하고 health·설정·credential을 순서대로 확인 |
| DB 복구가 필요함 | backup checksum, schema/binary version, 영향 범위 | [backup-recovery.md](backup-recovery.md)의 새 DB 복구부터 실행 |

`fleet-worker --config <path> --check`, `fleet doctor`, systemd journal은 지원되는 진단 수단이다.
SSH host-key 기본 정책은 TOFU이므로 운영 문제 분석·재프로비저닝 전에는 strict policy와
`fleet scan-host-keys` 사전 수집 여부를 확인한다.
