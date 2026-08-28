---
type: architecture
authority: canonical
implementation: partial
verification: design-reviewed
source: "docs/architecture/agents/provisioning.md"
last_verified: "2026-08-28"
last_verified_commit: "working-tree"
---

# Agent 프로비저닝

## 책임과 현황

이 문서는 Agent의 생성·명령·ACK·회수 상태 전이만 정의한다. Worker daemon과 Agent process,
durable context의 관계는 [배치·맥락 계약](../entity-placement-and-context.md)이 소유한다. 실행 격리는
[실행 격리](execution-isolation.md), harness 내용은 [하네스 구성](harness-composition.md)이
담당한다. 아래 상태·명령·재조정 절은 **목표 설계**이며, 현재 구현된 부분은 마지막의
"구현 상태" 절이 정본이다.

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

## 구현 상태 (`#49` 1단계)

1단계는 **Agent 엔티티와 그 판독자**까지만 구현했다. 뒤에 프로세스가 없으므로 위의 명령·ACK·
fencing·재조정은 한 줄도 구현되지 않았다.

구현된 것:

- `agents` 테이블(`027_agents.sql`): `project_id`는 `projects(id)`를 참조하는 실제 FK이며
  `ON DELETE RESTRICT`다. 이름은 `UNIQUE (project_id, name)`으로 **Project 안에서만** 유일하다.
- `AgentStatus`는 8-상태가 아니라 `Ready`/`Stopped` 2-상태다. 나머지 6개는 아무도 생성할 수
  없으므로 만들지 않았다(아래 유예 표).
- capability는 `agent:read`(Operator·Viewer 기본)와 `agent:manage`(Admin 전용) 둘이다.
  `agent:attach`는 붙을 세션도, step-up grant 발급자도 없어 **만들지 않았다**.
- 감사 이벤트 `agent.create`, `agent.stop`.
- 표면: MCP `fleet_create_agent`/`fleet_list_agents`/`fleet_stop_agent`,
  Dashboard `GET|POST /api/agents`와 `DELETE /api/agents/{id}`, Project 상세 페이지의 Agents 절.

Agent 행이 죽은 데이터가 아님을 보장하는 판독자 셋:

1. 생성 시 `ensure_project_accepts_new_agents`가 Project 상태를 검사한다 — archived/draining
   Project에는 Agent를 만들 수 없다.
2. `advance_project_archive`가 살아 있는 Agent를 archive 게이트로 센다. 이제 archive는
   "활성 Task 없음 **그리고** 살아 있는 Agent 없음" 두 조건이다.
3. `agent:read`/`agent:manage`가 드디어 검사할 대상을 갖는다.

`project_id`는 불변이다 — Store 갱신 메서드도, API 필드도, PATCH route도 만들지 않았다.
옮기고 싶으면 대상 Project에 새 Agent를 만든다.

### 유예 목록

| 항목 | 왜 미뤘나 | 선행 |
|---|---|---|
| `Starting`/`Running`/`Failed` 상태 | 상태를 옮길 주체가 Worker 제어 스트림뿐이며 그 스트림이 없다 | `#89` |
| `WarmIdle` | execution lease가 없어 "slot을 잡은 채 쉬는 상태"를 표현할 수 없다 | `#67` 후속 |
| `Hibernated` | snapshot 불일치 판정에 AgentTemplate과 harness 구성이 필요하다 | `#86`, `#51` |
| `Draining` | 위 실행 상태들이 없으면 drain할 대상이 없다 | `#89` |
| 9-필드 명령 봉투와 ACK | 명령을 받을 상대가 없다 | `#89` |
| `generation`/`control_epoch`/`fencing_token` | 경합할 두 번째 writer가 없다 | `#89` |
| 재조정과 `OutcomeUnknown` | 비교할 process inventory가 없다 | `#89`, `#67` 후속 |
| `tasks.agent_id` | 지금 채우면 항상 NULL인 컬럼이 된다 — dispatch가 Agent를 고르지 않는다 | `#89` |
| `agent:attach` capability | 붙을 터미널 세션도 grant 발급자도 없다 | `#50` |

위 "구현 게이트" 6개는 전부 명령·ACK 계층의 시험이므로 1단계 범위 밖이다. 1단계가 실제로
증명한 것은 (a) FK·유일성 위반이 `StoreError::Conflict`로 번역되는지, (b) 살아 있는 Agent 하나가
Task 없이도 Project를 `draining`에 붙잡는지 — 두 가지이며 `crates/fleet-store/tests/agents.rs`,
`fleet-mcp`/`fleet-dashboard` 테스트가 근거다.
