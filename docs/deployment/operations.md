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

## 중단과 장애 초기 대응

- 인증 없는 외부 API, 예상 밖 bind, secret 노출이 발견되면 서비스 공개를 중단하고
  [security model](../security/control-plane-security-model.md)의 대응을 따른다.
- DB 연결 실패, health 실패, Worker heartbeat 실패는 [troubleshooting.md](troubleshooting.md)에서
  증거를 수집한 뒤 복구한다.
- Primary 전환과 fencing은 [Control-plane availability](../architecture/control-plane-availability.md)가
  정본이다. 이 Runbook은 Active-Active 전환 절차를 제공하지 않는다.

## 운영 기록

시작·중단·설정 변경마다 시각, 작업자, binary version, 설정 hash, 대상 host, health 결과를 남긴다.
