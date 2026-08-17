---
type: architecture
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/agents/provisioning.md"
last_verified: "2026-08-17"
---

# Agent 프로비저닝

## 책임과 현황

이 문서는 Agent의 생성·명령·ACK·회수 상태 전이만 정의한다. 실행 격리는
[실행 격리](execution-isolation.md), harness 내용은 [하네스 구성](harness-composition.md)이
담당한다. 현재 구현은 Worker당 단일 runner와 heartbeat만 제공하며 Agent 엔티티·명령 ACK·
reconciliation은 없다.

## 상태와 명령

`Requested → Starting → Running → Draining → Stopped`가 정상 경로다. `Failed`는 어느
실행 상태에서도 갈 수 있으며, `Stopped`는 cleanup 증거가 있을 때만 허용한다. Project drain은
새 Agent 시작을 막지만 기존 실행을 즉시 삭제하지 않는다.

각 명령에는 `agent_id`, `request_id`, `generation`, `attempt_snapshot_id`, `actor`, `expires_at`가
필수다. Worker는 자신에게 배정된 Agent와 현재 incarnation만 처리하고, 오래된 generation 또는
만료 명령을 거절한다. ACK는 같은 식별자와 결과·오류 분류·cleanup 증거를 반환한다. control plane은
CAS로 ACK를 반영하므로 지연된 ACK가 새 상태를 덮어쓰지 못한다.

## 재조정과 회수

재조정은 관측된 Worker 상태와 마지막 확정 generation을 비교해 idempotent 명령만 다시 낸다.
결과가 불명확한 start/stop은 자동 재실행하지 않고 `OutcomeUnknown`으로 승격한다. idle 회수는
새 TaskAttempt·attach lease·보존해야 할 artifact가 없음을 확인한 뒤 drain을 요청한다.

## 구현 게이트

1. 중복·역순 ACK 및 Worker 재기동을 포함한 CAS 시험
2. cleanup 실패에서 `Stopped` 전이를 막는 시험
3. drain 중 새 시작 거절과 기존 실행 보존 시험
4. 명령·ACK·actor·generation의 감사 추적 시험
