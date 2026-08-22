---
type: runbook
authority: canonical
implementation: partial
verification: code-checked
source: "docs/deployment/operations.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["operations"]
---

# 일상 운영 Runbook

이 문서는 이미 구성된 control plane의 시작, 상태 확인, 안전한 중단과 초기 장애 대응을 다룬다.
설치와 DB 복구는 각각 [install.md](install.md), [backup-recovery.md](backup-recovery.md)를 따른다.

## 시작 전

- [configuration.md](configuration.md)의 production preflight를 통과한다.
- Primary 역할, DB 연결, binary version, schema compatibility를 기록한다.
- 서비스를 시작하기 전에 외부 bind와 인증 설정을 다시 확인한다.

## 시작과 검증

1. 서비스 계정으로 orchestrator를 시작한다.
2. `fleet doctor`로 DB와 선택적 API/Dashboard health를 확인한다.
3. `/v1/health`, `/metrics` 및 인증된 API 요청을 확인한다.
4. Worker가 있는 경우 register/heartbeat와 작은 무해 태스크의 dispatch를 각각 확인한다.

health checker, stale-task reconciliation, CircuitBreaker 동기화는 기본 활성화 경로다. 서비스 재시작은
진행 중 TaskAttempt의 결과를 보장하지 않으므로, 중단 전 실행 중인 작업과 부작용을 확인한다.
desired/observed 상태, reconciliation의 자동 범위, alert와 운영자 action은
[관측성·재조정·장애 복구 계약](../architecture/observability-and-reconciliation.md)이 정본이다.

## 중단과 장애 초기 대응

- 인증 없는 외부 API, 예상 밖 bind, secret 노출이 발견되면 서비스 공개를 중단하고
  [security model](../security/control-plane-security-model.md)의 대응을 따른다.
- DB 연결 실패, health 실패, Worker heartbeat 실패는 [troubleshooting.md](troubleshooting.md)에서
  증거를 수집한 뒤 복구한다.
- Primary 전환의 권한·lease·fencing 계약은 [Control Plane 권한과 장애 전환](../architecture/control-plane-authority-and-failover.md)이
  정본이다. 이 Runbook은 Active-Active 전환 절차를 제공하지 않는다.

## 수동 Primary 승격

자동 failover는 지원하지 않는다. 다음 절차는 기존 Primary가 종료되었거나 네트워크 fencing되어
새 제어 명령을 낼 수 없다는 증거가 있을 때만 수행한다.

1. 기존 Primary의 접근 가능성, 프로세스 종료 또는 네트워크 fencing을 기록한다.
2. DB에서 기존 lease 만료와 현재 epoch를 확인한다.
3. Standby의 binary version, schema compatibility, DB 접근, 인증 설정을 확인한다.
4. Standby를 기동해 새 epoch의 lease를 얻었는지 확인한다.
5. readiness와 인증된 API를 확인한 뒤 gateway 트래픽을 전환한다.
6. Worker 재연결, pending/stale dispatched reconciliation, 무해 Task dispatch를 확인한다.

신규 dispatch보다 Worker inventory·Agent lease·delivery grant·effect ledger 관측을 먼저 수행한다.
`OutcomeUnknown`, `PartiallyApplied`, `ArchiveBlocked`는 해소 증거 없이 재시작·redrive하지 않는다.

실패하면 gateway를 되돌리고, lease 소유자와 fencing 증거를 다시 확인한다. 승격 중에는 두
인스턴스가 동시에 제어 명령을 내지 않게 한다.

## Standby 준비 점검

Standby에는 같은 Fleet binary와 schema compatibility 정보, DB 접근 경로, 해당 인스턴스가
소비하는 인증·mTLS·Nginx·LiteLLM·OTEL·SMTP 설정, skill/routing policy revision, SSH
`known_hosts`가 준비되어야 한다. secret은 원문을 운영 기록에 남기지 않고
[configuration.md](configuration.md)의 권한·전달 규칙으로 검증한다. ACP 연결과 프로세스
메모리는 복제 대상이 아니며, 승격 뒤 Worker 재연결과 reconciliation으로 복구한다.

## 운영 기록

시작·중단·설정 변경마다 시각, 작업자, binary version, 설정 hash, 대상 host, health 결과를 남긴다.
