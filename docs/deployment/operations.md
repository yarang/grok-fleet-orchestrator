---
type: runbook
authority: canonical
implementation: partial
verification: code-checked
source: "docs/deployment/operations.md"
last_verified: "2026-08-26"
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

**먼저 정상 종료인지 확인한다.** Primary를 `SIGTERM`/`SIGINT`로 정상 종료하면 lease를 즉시
반납하므로(2026-08-26 라이브 측정: 신호로부터 1.7ms) 이미 기동해 polling 중인 Standby는
약 2초 안에 스스로 승격한다 — 이 경우 아래 1·2단계는 "일어난 일을 확인"하는 절차이고,
운영자가 승격을 유도할 필요가 없다. 계획된 재시작·배포는 이 경로를 쓴다.

**TTL 만료만으로 승격하지 않는다.** 비정상 종료된 Primary의 lease는 TTL(기본 15초) 뒤
만료되고, 그때 Standby는 **운영자 개입 없이 자동으로** lease를 얻는다. 그러나 이 경로에서
Standby가 가진 증거는 시계뿐이다 — 전 Primary가 실제로 죽었는지, GC 정지나 네트워크 분단으로
갱신만 못 하고 있는지 구분하지 못한다. 그 창을 닫아야 할 epoch 강제는 아직 구현되지 않았다
([Control Plane 권한과 장애 전환 계약](../architecture/control-plane-authority-and-failover.md)의
"구현 상태와 유예"). 따라서 **TTL 만료는 승격의 근거가 아니다.** 아래 1단계의 fencing 증거를
독립적으로 확보하지 못했다면, lease가 이미 넘어갔더라도 gateway 트래픽을 전환하지 말고 전
Primary를 확실히 정지시키는 것을 먼저 한다.

1. 기존 Primary의 접근 가능성, 프로세스 종료 또는 네트워크 fencing을 기록한다.
   lease 만료 사실은 이 증거를 대체하지 못한다.
2. DB에서 lease 소유자와 만료 상태를 확인한다.

   ```sql
   SELECT active_instance_id, epoch,
          expires_at < NOW() AS expired,
          expires_at - last_renewed_at AS gap
     FROM control_plane_lease;
   ```

   **`gap`은 두 조건이 모두 참일 때만 종료 유형을 말한다** — `active_instance_id`가 아직
   죽은 Primary이고, `expired`가 참일 때다. 이때 `gap`이 TTL(기본 15초)과 같으면 갱신이
   끊긴 것이므로 **비정상 종료**이고, TTL보다 뚜렷이 작으면(0 ~ `renew_interval` 기본
   5초) **명시적 반납**이다. 갱신은 `expires_at`과 `last_renewed_at`을 `NOW()+TTL`과
   `NOW()`로 함께 쓰지만 명시적 반납은 `expires_at`만 `NOW()`로 당기고 `last_renewed_at`은
   그대로 두므로, 두 구간은 겹치지 않는다.

   **그 밖의 경우에는 `gap`을 읽지 않는다.** 정상적으로 갱신 중인 살아 있는 lease도
   `gap`이 항상 정확히 TTL이다 — `expired`를 함께 보지 않으면 건강한 Primary를 비정상
   종료로 오독한다. 그리고 `control_plane_lease`는 cluster당 한 행이고 획득이
   `active_instance_id`·`acquired_at`·`expires_at`·`last_renewed_at`을 모두 덮어쓰므로,
   Standby가 이미 승격한 뒤에는 이 행에 전 Primary의 종료 증거가 **남아 있지 않다**.
   그때 보이는 `gap`은 새 소유자의 갓 얻은 lease를 기술할 뿐이다. 이미 기동해 polling
   중인 Standby가 있으면 증거가 남아 있는 창은 `poll_interval`(기본 3초) 수준이므로,
   실무에서는 운영자가 이 쿼리에 도달했을 때 이미 덮어써져 있는 쪽이 흔하다. 그런
   경우 1단계에서 독립적으로 확보한 fencing 증거가 유일한 입력이다.
3. Standby의 binary version, schema compatibility, DB 접근, 인증 설정을 확인한다.
   (기동 시 자동 호환성 검사는 아직 없다 — 수동 확인이 유일한 게이트다.)
4. Standby를 기동해 새 epoch의 lease를 얻었는지 확인한다. 이미 기동해 있었다면 승격
   여부만 확인한다.
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
