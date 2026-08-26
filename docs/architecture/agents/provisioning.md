---
type: architecture
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/agents/provisioning.md"
last_verified: "2026-08-27"
last_verified_commit: "working-tree"
---

# Agent 프로비저닝

## 책임과 현황

이 문서는 Agent의 생성·명령·ACK·회수 상태 전이만 정의한다. Worker daemon과 Agent process,
durable context의 관계는 [배치·맥락 계약](../entity-placement-and-context.md)이 소유한다. 실행 격리는
[실행 격리](execution-isolation.md), harness 내용은 [하네스 구성](harness-composition.md)이
담당한다. 현재 구현은 Worker당 단일 runner와 heartbeat만 제공하며 Agent 엔티티·명령 ACK·
reconciliation은 없다.

## 상태와 명령

`Ready → Starting → Running → WarmIdle|Hibernated → Draining → Stopped`가 정상 경로다.
`WarmIdle`은 명시적 lease가 있을 때만 사용하며, 기본 Task 완료 경로는 `Hibernated`다. `Failed`는
어느 실행 상태에서도 갈 수 있고, `Stopped`는 cleanup 증거가 있을 때만 허용한다. Project drain은
새 Agent activation과 warm lease 연장을 막지만 기존 실행을 즉시 삭제하지 않는다.

WarmIdle 전이는 Worker의 `WarmIdleGranted` ACK가 있고, Task가 terminal이며, credential
delivery grant·interactive attach grant가 모두 회수됐을 때만 가능하다. WarmIdle에는 새 Task를
실행하지 않는 동안에도 execution lease와 Worker process slot이 남아 있다. TTL 또는 eviction에는
`StopAgent(reason)`를 보내고 cleanup ACK 뒤 lease를 release한다. 새 Task가 현재 process와
runtime/image·isolation·workspace·Tool/Skill·egress/privileged policy snapshot이 다르면
`WarmIdle → Hibernated → Starting`만 허용한다.

각 명령에는 `agent_id`, `request_id`, `generation`, `task_id`, `actor`, `expires_at`,
`control_epoch`, `worker_incarnation`, `fencing_token`이 필수다. Worker는 자신에게 배정된 Agent와
현재 incarnation만 처리하고, 오래된 generation·epoch·token 또는 만료 명령을 거절한다. ACK는 같은
식별자와 관측한 process/container ID·결과·오류 분류·cleanup 증거를 반환한다. control plane은 CAS로
ACK를 반영하므로 지연된 ACK가 새 상태를 덮어쓰지 못한다.

## 재조정과 회수

재조정은 관측된 Worker process inventory, 마지막 확정 generation, fencing token을 비교해
idempotent 명령만 다시 낸다. 결과가 불명확한 start/stop은 자동 재실행하지 않고 `OutcomeUnknown`으로
승격하며, 그 Agent에는 관측 완료 전 새 lease를 주지 않는다. Worker가 control lease를 잃으면 새
Task를 시작하지 않고 grace 뒤 self-fence/drain한다. idle 회수는 새 Task·attach lease·보존해야
할 artifact가 없음을 확인한 뒤 drain을 요청한다.

재조정은 WarmIdle을 실행 중 Task로 잘못 복구하지 않는다. process inventory가 WarmIdle lease와
일치하지 않거나 credential grant가 남아 있으면 process를 drain하고 `Failed` 또는 `Hibernated`로
전이한다. Worker 압박 시 만료·drain/revoked·LRU 순서로 WarmIdle만 축출하며 실행 중인 Task는
effect/cancel 계약을 따른다.

## 구현 게이트

1. 중복·역순 ACK 및 Worker 재기동을 포함한 CAS 시험
2. cleanup 실패에서 `Stopped` 전이를 막는 시험
3. drain 중 새 시작 거절과 기존 실행 보존 시험
4. 명령·ACK·actor·generation의 감사 추적 시험
5. network partition 중 self-fence와 stale token 거절 시험
6. WarmIdle 전 grant 회수와 eviction 후 cleanup/lease release 시험
