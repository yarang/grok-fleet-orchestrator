---
type: runbook
authority: canonical
implementation: partial
verification: code-checked
source: "docs/deployment/backup-recovery.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["operations"]
---

# 데이터베이스 백업·복구 Runbook

이 문서는 PostgreSQL Fleet 데이터의 백업·복원·검증 절차다. PITR은 현재 제공하지 않는다.

## 백업

`scripts/db-backup.sh`는 전체 custom-format dump, SHA-256 sidecar, 보존 기간 정리를 제공한다.
백업 파일은 제한된 권한으로 저장하고 오프호스트 사본·암호화·RPO는 운영자가 별도로 결정한다.

백업 뒤에는 dump, checksum, 생성 시각, DB revision, binary version을 기록한다. 정기적으로 별도
DB에 restore drill을 실행한다.

## 기본 복구

1. 대상 backup의 checksum과 schema/binary 호환성을 확인한다.
2. 새 데이터베이스에 복원한다.
3. 서비스를 그 복원본에 연결하기 전에 schema, health, 읽기 전용 확인을 수행한다.
4. 검증 결과와 전환 결정을 운영 기록에 남긴다.

`scripts/db-restore.sh`의 기본 경로도 새 DB 복원이다.

## In-place 복구

`--in-place`는 live DB 객체를 삭제할 수 있는 파괴적 작업이다. 다음이 없으면 실행하지 않는다.

- control plane 중단과 Primary fencing
- 최신 별도 backup 및 rollback 책임자
- 대상 DB·복원 파일·영향 범위의 이중 확인
- 복원 뒤 health, 인증, Worker heartbeat, task dispatch 검증

migration down은 지원되지 않는다. rollback은 이전 binary만으로 해결된다고 가정하지 않는다.
