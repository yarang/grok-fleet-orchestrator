---
type: architecture
authority: canonical
implementation: partial
verification: design-reviewed
source: "docs/architecture/agents/provisioning.md"
last_verified: "2026-08-30"
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

**배정은 봉투의 일부가 아니라 봉투의 선행이다.** 위 문단의 "자신에게 배정된 Agent"는 배정이
이미 존재함을 전제한다. 배정의 저장 자리는 `agents.worker_id`이고, 그것이 없으면 명령을 **어느
Worker의 응답에 실을지** 결정할 수 없다 — 봉투는 명령을 받을 *프로세스*를 만들지만 명령이 갈
*방향*은 만들지 못한다. 배정을 고르는 규칙은 Task용 `WorkerSelector`와 같은 순서(online →
liveness → circuit → least-loaded)를 쓰되 부하의 출처는 워커 자기보고가 아니라 오케스트레이터
원장(배정된 Agent 수)이다. Task 선택이 `#67` 3단계에서 같은 이유로 자기보고를 버린 것과 같은
판단이다.

**명령은 큐가 아니라 desired state로 구현한다.** 위 봉투의 필드는 대부분 `agents` 행의 상태다 —
`agent_id`는 행 자신, `generation`/`control_epoch`/`worker_incarnation`/`actor`는 컬럼, `task_id`는
그 Agent가 지금 무엇을 위해 존재하는지다. 그래서 별도 명령 큐 테이블을 두는 대신 행에
`desired_status`·`command_generation`·`last_acked_generation`을 두고, heartbeat 응답이 그 Worker에
배정된 Agent들의 desired state와 generation을 싣고, Worker가 관측 상태를 같은 generation으로
ACK한다. 이때 "지연된 ACK가 새 상태를 덮어쓰지 못한다"는 `WHERE command_generation =
$ack_generation` CAS로 **그대로** 성립한다 — 수렴 모델이 그 성질을 따로 구현하지 않고 갖는다.
이 선택은 정본으로부터의 이탈이 아니라 구현 형태의 결정이며, 정본이 큐를 요구한 적이 없다.

**heartbeat의 `desired_state`와 Agent의 desired state는 다른 축이다.** 이미 있는
`HeartbeatResponse.desired_state`(`"running"`|`"drain"`)는 **그 Worker 자신**에게 내리는 신호이며,
CPU/RAM 90% 초과 자가 판정이 만든다(`fleet-api/src/handlers.rs`). Agent의 desired state는 **그
Worker에 배정된 Agent들**에 대한 것이다. 같은 이름이라고 한 필드에 합치면 두 축이 서로를 덮으므로,
Agent 명령은 형제 필드로 따로 싣는다. `command_generation`이 Agent별 원소 안에 들어가므로 기존
필드는 `&'static str` 그대로 두어도 된다.

Worker의 `drain`은 Agent 명령을 **억제하지 않는다**. `drain`은 이미 "새 배정 후보에서 빠진다"는
뜻이고 4a의 배정 선택기가 `Online`이 아닌 Worker를 이미 거른다 — 그래서 draining Worker에는 새
Agent가 오지 않는다. 이미 배정된 Agent를 drain 시점에 죽이는 것은 만들지 않은 별개의 결정이며
(위 유예 표의 Agent `Draining`), 4c가 이 문장 없이 임의로 발명하지 않도록 여기에 적어 둔다.

**수렴 프로토콜은 새 관측 상태를 만들지 않는다.** `Running`은 "이 Agent의 포트·secret·cwd로 grok
프로세스가 살아 있다"는 관측이고, 그것을 볼 수 있는 주체는 4c의 프로세스 매니저뿐이다. 프로세스
없이 Worker가 `running`을 ACK하면 저장된 값이 거짓이 되고, 4c는 이미 기록된 컬럼의 **의미를**
바꿔야 한다. `Starting`도 마찬가지로 들어갈 문은 있지만 나갈 문(`Running`)이 없다 — 구현 게이트
③의 `OutcomeUnknown`을 "나갈 수 없는 비terminal 위상"이라며 물린 것과 같은 결함이다. 더구나
`Starting`은 `(desired_status, command_generation, last_acked_generation, status)`의 **순수
함수**라 컬럼이 필요 없다: 컬럼으로 만들면 027의 `status IN ('ready','stopped')` CHECK를 아무도
관측하지 않는 값 때문에 넓혀야 하고, generation 컬럼과 어긋날 수 있는 두 번째 진실 원천이 생긴다
(3단계에서 워커 자기보고 대신 원장을 택한 것과 같은 판단). 그래서 4b는 `AgentStatus`에 variant를
추가하지 않고, `Starting`은 표시용 파생값으로만 노출한다.

`last_acked_generation == command_generation`은 **전달·수락**이지 **수렴**이 아니다. 프로세스
매니저가 없어도 Worker는 "이 generation의 명령을 받았다"를 정직하게 ACK할 수 있고, 바로 그 점이
이 컬럼들을 030이 만들기를 거부한 항상-기본값 컬럼과 구분한다. 4c가 이 등식을 "running"으로 읽는
순간 어떤 테스트도 잡지 못하는 조용한 오탐이 되므로, 마이그레이션 주석에도 같은 문장을 남긴다.

수렴 모델에서 `expires_at`은 대부분 해소된다. desired state는 매 heartbeat마다 재전송되므로
"오래된 명령이 뒤늦게 실행되는" 창이 큐 모델처럼 열리지 않고, 신선도 판정은 `expires_at`이 아니라
`command_generation`이 한다. TTL 규칙을 쓰는 대신 이 문장을 남긴다.

**수렴은 `on_demand` Worker를 풀지 못한다.** `WorkerLivenessMode::OnDemand`는 idle 시 heartbeat을
보내지 않는 모드이므로, heartbeat에 실려 가는 desired state는 그 Worker에 **원리적으로 도달하지
않는다**. `WorkerSelector`가 이미 같은 이유로 `on_demand` 워커를 Task 후보에서 빼고 있다
(`SelectionError::AllUnprobed`). `worker.rs`의 doc이 이 제약의 해소처로 `#67`을 지목하지만, 실제로
해소하는 것은 명령 전달이 아니라 **dispatch 직전 ACP probe**(로드맵 `#70`)다.

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
   "활성 Task 없음 **그리고** 살아 있는 Agent 없음" 두 조건이며, 막혔을 때 **어느 쪽이
   막았는지**(`ArchiveBlockers`)를 함께 돌려준다 — 두 조건은 해소 방법이 다르다. Task는 끝나기를
   기다리면 되지만 `Ready` Agent는 저절로 끝나지 않고 사람이 회수해야 하므로, "곧 끝난다"는 안내가
   Agent에는 성립하지 않는다.
3. `agent:read`/`agent:manage`가 드디어 검사할 대상을 갖는다.

`project_id`는 불변이다 — Store 갱신 메서드도, API 필드도, PATCH route도 만들지 않았다.
옮기고 싶으면 대상 Project에 새 Agent를 만든다.

### 유예 목록

> **2026-08-30 귀속 정정.** 아래 표는 여섯 칸을 `#89`(Agent 보고 경로와 폭주 방지)로 귀속시켰으나
> 오표기였다. Worker 제어 스트림과 위 §"상태와 명령"의 9-필드 명령 봉투·ACK는 **이 문서를 설계
> 정본으로 갖는 `#67`("Worker execution lease·Agent command ACK")의 범위**다. `#89`는 그 스트림
> 위에 "Agent가 Issue를 연다"를 얹는 소비자이며, 그 근거로 `#89`의 선행이 `#67`이다. 오표기를
> 그대로 두면 `#67`이 스트림을 `#89`로 미루고 `#89`는 `#67`을 기다리는 **순환**이 생기고, 실제로
> [권한과 장애 전환](../control-plane-authority-and-failover.md)과 이 표가 그렇게 적혀 있었다.

> **2026-08-30 분할.** `#67` 4단계는 배정·수렴 프로토콜·워커측 프로세스 매니저 셋을 한 덩어리로
> 적고 있었으나, 그중 배정(`agents.worker_id`)에는 **주인이 없었다** — `#49` 2단계는 "`#48`/`#67`
> 뒤 Hibernated E2E"라 배정을 소유하지 않고, `#67` 4단계 정의문은 "봉투가 상대를 만든다"로 *프로세스*
> 부재만 해소했지 *방향* 부재는 해소하지 않았다. 그래서 셋으로 나눈다: **4a** 배정, **4b** 수렴
> 프로토콜(desired state + generation + ACK), **4c** 워커측 1:N 프로세스 매니저. 아래 "선행" 칸은
> 이 분할을 반영한다.

| 항목 | 왜 미뤘나 | 선행 |
|---|---|---|
| `Starting`/`Running`/`Failed` 상태 | 셋 다 **관측**이고 관측 주체가 프로세스 매니저뿐이다. 4b(수렴 프로토콜)는 명령의 전달만 만들 뿐 프로세스를 보지 못하므로 셋 중 어느 것도 4b에서 도달하지 않는다 — `Starting`은 그나마 generation 컬럼들의 순수 함수라 컬럼 없이 파생으로 표시한다 | `#67` 4c |
| `WarmIdle` | execution lease가 없어 "slot을 잡은 채 쉬는 상태"를 표현할 수 없다 | `#67` 후속 |
| `Hibernated` | snapshot 불일치 판정에 AgentTemplate과 harness 구성이 필요하다 | `#86`, `#51` |
| `Draining` | 위 실행 상태들이 없으면 drain할 대상이 없다. 4b에서도 만들지 않는다 — Worker의 `Draining`이 operator 개입 없이는 되돌아오지 않는 일방향 문(`fleet-api/src/handlers.rs`)이며, 그 모양을 Agent가 물려받을 이유가 없다 | `#67` 4c 이후 |
| `fencing_token` | 위 둘과 달리 **생산자 자체가 없다** — `worker_execution_lease` 테이블이 존재하지 않는다(migration 018~029에 없고, `021_control_plane_lease.sql`은 오케스트레이터 리더 선출용의 다른 테이블이다). 그래서 봉투는 9-필드가 아니라 8-필드로 만든다 | `#67` 구현 게이트 ① |
| 재조정과 `OutcomeUnknown` | 비교할 process inventory가 없다 | `#67` 4c 후속 |
| `tasks.agent_id` | 지금 채우면 항상 NULL인 컬럼이 된다 — dispatch가 Agent를 고르지 않는다. 이것은 transport 사실이 아니라 **스케줄러 사실**이다 | `#49` 2단계 |
| `agent:attach` capability | 붙을 터미널 세션도 grant 발급자도 없다 | `#50` |
| ACK가 Agent process의 endpoint·secret을 돌려주는 것 | 소비자가 없다 — Task를 Agent로 라우팅하는 것은 `#49` 2단계이고, 지금 넣으면 secret을 한 번도 나른 적 없는 경로에 secret을 새로 얹게 된다 | `#49` 2단계 |
| 명령 payload의 포트·secret·cwd | 4b의 명령은 `(agent_id, desired_status, generation)` **뿐**이다. 이 셋이 들어오는 순간 heartbeat 응답은 통째로 로깅해도 안전한 값이 아니게 되므로, 4c가 무심코 얹지 않도록 지금 적어 둔다 | `#67` 4c |
| ACK가 관측 상태를 싣는 것 | 볼 프로세스가 없다. 4b의 ACK는 generation만 돌려주며 그것이 정직한 최대치다 | `#67` 4c |
| Worker가 신고하는 `max_agent_processes` | 프로세스 매니저가 없는 동안 Worker는 자기 상한을 **집행할 수 없다**. 집행되지 않는 숫자를 신고받는 것은 "항상 NULL인 컬럼"의 뒤집힌 형태다. 4a의 배정은 하드 상한 없이 원장 기반 least-loaded만 쓴다 | `#67` 4c |

위 "구현 게이트" 6개는 전부 명령·ACK 계층의 시험이므로 1단계 범위 밖이다. 1단계가 실제로
증명한 것은 (a) FK·유일성 위반이 `StoreError::Conflict`로 번역되는지, (b) 살아 있는 Agent 하나가
Task 없이도 Project를 `draining`에 붙잡는지 — 두 가지이며 `crates/fleet-store/tests/agents.rs`,
`fleet-mcp`/`fleet-dashboard` 테스트가 근거다.

## 구현 상태 (`#67` 4a — 배정)

4a는 **명령이 갈 방향**만 만들었다. 명령 자체(desired state·generation·ACK)는 4b, 그 명령을
받아 프로세스를 띄우는 쪽은 4c다. 즉 이 단계가 끝나도 Agent 프로세스는 여전히 하나도 뜨지 않는다.

구현된 것:

- `agents.worker_id`(`workers(id)` FK, `ON DELETE SET NULL`)와 `agents.assigned_at`
  (`030_agent_placement.sql`). 둘은 **both-or-neither**이며 `CHECK`가 그것을 강제한다 —
  한쪽만 채워진 행은 "배정됐는데 언제인지 모른다" 또는 그 반대라서, 어느 쪽도 읽는 쪽이
  해석할 방법이 없다.
- `ON DELETE SET NULL`이 `worker_id`만 비우면 `CHECK`를 깨뜨리므로, `BEFORE UPDATE OF worker_id`
  트리거가 `assigned_at`도 함께 비운다. 이 트리거가 **FK가 유발한 UPDATE에서도 발동한다**는 것은
  버려질 클론 DB(`createdb -T`)에서 직접 확인했다 — 마이그레이션은 `sqlx::migrate!`가 파일 전체를
  체크섬하므로 `fleet_test`에 적용하면 그 시점부터 수정이 불가능해진다.
- 배정 선택기 `fleet_scheduler::placement`: `Offline`·`on_demand`·회로 개방 Worker를 후보에서
  빼고 **원장(`count_agents_by_worker`) 기준 least-loaded**를 고른다. 동률은 먼저 등록된 쪽이
  이긴다(결정적 순서). 원장은 `worker_id IS NOT NULL AND status <> 'stopped'`만 세므로 회수가
  자리를 자동으로 비운다 — 그래서 `unassign_agent_worker`는 **만들지 않았다**(생산자가 없다).
- 배정은 생성과 **같은 INSERT**에 실린다. 별도 UPDATE였다면 중간 실패가 "아무도 고치지 않을
  미배정 행"을 남긴다. 반대로 후보가 없어도 **생성은 실패하지 않는다** — Agent 정의가 Worker
  가용성에 인질로 잡히면 Worker를 붙이기 전에 Project를 설계하는 정상 사용이 막힌다.
- 그 대신 `NULL`을 비종단으로 만드는 회복 경로: Dashboard `POST /api/agents/{id}/place`,
  MCP `fleet_place_agent`(둘 다 `agent:manage`). 자동 선택이 기본이고 `worker_id`를 명시하면
  그 Worker로 간다 — 자동 선택이 least-loaded뿐이라 특정 Worker를 지목할 다른 방법이 없다.
- 조회: `AgentFilter.worker_id`, Dashboard `GET /api/agents?worker_id=`, MCP `fleet_list_agents`의
  같은 필드, Project 상세 화면의 Worker 열.
- 감사 이벤트 `agent.assign`(`previous_worker_id` 포함). 생성 시 배정은 이 이벤트를 내지 않고
  `agent.create`의 detail에 실린다 — 그래야 `agent.assign` 건수가 정확히 **생성 이후의 배정
  변경 횟수**가 된다. 여기에는 Worker 간 이동뿐 아니라 `NULL`에서의 회복도 들어간다
  (`previous_worker_id: null`로 구분된다). 2026-08-30 실측: Agent 3개(생성 시 배정 1개 포함)와
  명시 배정 1회에 대해 `agent.create` 3건 · `agent.assign` 1건이었고, 생성 시 배정된 Agent의
  `agent.create` detail에만 `worker_id`가 실려 있었다.

의도적으로 만들지 않은 것:

- **하드 상한이 없다.** `workers.max_concurrent`는 Task 동시성 상한이지 Agent 프로세스 상한이
  아니며, 후자를 강제할 프로세스 매니저가 4c 전에는 없다(위 유예 표의 `max_agent_processes`와
  같은 논리).
- **배경 재조정기가 없다.** 배정은 관측이 아니라 **결정**이고, 4a에는 옮길 프로세스가 없으므로
  재조정할 대상도 없다. 인라인 배정이라 감사 이벤트의 actor가 실재하는 사람이며, 두 오케스트레이터
  인스턴스가 같은 Agent 행을 두고 경합할 경로가 없어 `lease_allows_control()` 게이트도 필요 없다.
- **`HalfOpen`은 배제하지 않는다** — dispatch의 `is_open()`과 같은 술어를 쓴다. 여기서만 더 보수적으로
  굴면 같은 Worker가 Task는 받고 Agent는 못 받는 설명 불가능한 상태가 된다.

검증 한계:

- 회로 상태의 출처는 `workers.circuit_state` 컬럼이지 인메모리 `BreakerRegistry`가 아니다
  (Dashboard의 `DashboardState`에는 `FleetState`가 없다). 따라서 **레지스트리보다 한 번의 쓰기만큼
  뒤진다**. `Store::update_worker_circuit_state`는 기본 구현이 no-op이므로 MemStore 기반 테스트는
  `Worker::circuit_state`를 직접 세팅한다.
- 원장을 읽고 INSERT하기까지가 **원자적이지 않다.** 동시에 두 Agent를 만들면 둘 다 같은 Worker를
  고를 수 있다. 상한이 없는 4a에서는 부하 분포가 잠시 기우는 것으로 끝나지만, 상한이 생기는 순간
  초과 배정이 된다 — 그래서 CAS slot claim이 `#67` 구현 게이트 ①이다.

## 구현 상태 (`#67` 4b — 수렴 프로토콜)

4a가 명령이 갈 **방향**을 만들었다면 4b는 **명령 자체**를 만든다. 그 명령을 받아 프로세스를
띄우는 쪽은 여전히 4c이므로, 이 단계가 끝나도 Agent 프로세스는 하나도 뜨지 않는다.

### 큐가 아니라 수렴을 고른 이유

명령을 큐에 쌓지 않고 `agents` 행에 **desired state 한 칸**으로 둔다. heartbeat 응답이 매 beat
목록 전체를 다시 싣고, Worker는 같은 generation을 돌려주며, `WHERE command_generation = $ack`
CAS가 그것을 받는다. 이 모양에서는 "지연된 ACK가 새 명령을 덮어쓰지 못한다"가 **공짜로**
성립하고, 명령의 신선도를 generation이 판정하므로 `expires_at`이 아예 필요 없어진다. 큐였다면
중복 배달·순서 역전·만료를 각각 따로 막아야 한다.

세 컬럼(`031_agent_desired_state.sql`):

| 컬럼 | 뜻 |
|---|---|
| `desired_status` | `running` \| `stopped`. 오케스트레이터가 원하는 상태 |
| `command_generation` | 그 명령의 세대. desired state가 바뀌거나 **재배정될 때** 오른다 |
| `last_acked_generation` | Worker가 확인한 마지막 세대. `<= command_generation`을 `CHECK`가 강제한다 |

재배정에서도 세대를 올린다. 옮겨 간 Worker에게 그 명령은 처음 보는 것이므로, 이전 Worker의
확인이 새 Worker의 확인을 대신할 수 없다.

`last_acked_generation == command_generation`은 **전달과 수락**이지 **수렴**이 아니다. 관측
주체가 4c에 생기기 전까지 오케스트레이터가 정직하게 말할 수 있는 최대치가 전달이며, 그래서
`Agent::command_delivered()`라는 이름을 쓴다.

### `desired_status = 'running'`의 생산자: 명시적 start 표면

생성도 배정도 아니고 **운영자의 명시적 start 호출**이 유일한 생산자다. Dashboard
`POST /api/agents/{id}/start`와 MCP `fleet_start_agent`(둘 다 `agent:manage`, 감사 이벤트
`agent.start`)가 그것이다.

세 후보를 검토했고 앞의 둘은 배제했다.

- **생성이 곧 running**: `AgentStatus::Ready`의 정의가 "정의는 끝났고 시작 명령을 **받을 수
  있다**"인데, 생성이 running을 뜻하면 그 문장이 빈다. `create_agent`는 정의 조작이다.
- **배정이 곧 running**: 4a의 `place_on_create`가 생성 시점에 **자동으로** 배정하므로
  "배정 ⇒ running"은 곧 "생성 ⇒ running"이고, 위 항목과 같은 것이 된다. 4a를 읽지 않으면
  독립적인 선택지처럼 보이지만 아니다.
- **명시적 start**(채택): 표면은 운영자 호출을 요구할 뿐 실재하는 생산자다. "채울 방법이 없는
  것은 미리 만들지 않는다"가 금지하는 것은 *아무도 채울 수 없는 컬럼*이지 *호출이 필요한
  표면*이 아니다. 회수가 이미 `fleet_stop_agent`/`DELETE /api/agents/{id}`라는 명시적 표면인
  것과 대칭이기도 하다.

start의 두 경계:

- **`Stopped`에는 start를 거부한다.** 회수는 종단이며, 4a의 `place_agent_api`가 회수된 Agent의
  배정을 거부하는 것과 같은 기준이다. 이 판정은 **핸들러**에 둔다 —
  `set_agent_desired_status`의 UPDATE에 `status <> 'stopped'`를 넣으면 "바뀐 것이 없음"과
  "그런 Agent가 없음"이 같은 0-row로 뭉개진다.
- **미배정(`worker_id IS NULL`)에는 start를 허용한다.** 명령은 갈 곳이 없을 뿐 잃어버리지
  않는다 — 다음 배정이 세대를 올리며 그때 실려 간다. 여기서 거부하면 `NULL`에서의 회복이
  "먼저 배정하고 그다음 start"라는 순서 제약을 갖게 되는데, 그 순서를 강제할 이유가 없다.

### 회수 명령은 확인될 때까지 실린다

`list_agent_commands`의 술어가 `status <> 'stopped'`뿐이면, 회수가 올린 세대는 같은 조회가
그 행을 빼 버리는 탓에 **영원히 전달되지 않고** 모든 회수된 Agent의 `command_delivered()`가
항상 false가 된다. 그래서 술어는
`worker_id = $1 AND (status <> 'stopped' OR last_acked_generation < command_generation)`이다.
확인이 오면 조건이 깨지며 행이 조용해지므로 목록은 무한히 자라지 않는다.

ACK는 `updated_at`을 **밀지 않는다**. 두 회수 표면이 이미 `Stopped`인 Agent에 UPDATE를 건너뛰는
것은 "언제 회수됐는가"를 보존하려는 것인데, 한 beat 뒤에 도착한 ACK가 그 시각을 밀면 그
불변식이 무효가 된다. ACK는 프로토콜 부기이지 운영자의 변경이 아니다.

### `agents`가 `Vec`이 아니라 `Option<Vec>`인 이유

4c의 Worker는 이 목록을 **권위 있는 전체 집합**으로 읽고 "목록에 없는 것은 정리한다"를 하게
된다. 그 전제에서 `Vec` 하나로는 두 가지가 구분되지 않는다:

| 값 | 뜻 |
|---|---|
| `None` | 이번 beat에는 권위 있는 목록이 없다(store 조회 실패, 또는 이 필드를 모르는 구버전 오케스트레이터). **아무것도 바꾸지 마라** |
| `Some([])` | 정말로 배정된 Agent가 없다 |

이 구분이 없으면 store 오류 한 번이 그 Worker의 Agent를 전부 죽이는 신호가 되고, Worker측의
`#[serde(default)]`는 **구버전 서버의 응답**을 "전부 죽여라"로 읽는다. 4b가 커밋되면 이 모양을
바꾸는 것은 breaking change이므로 필드가 갓 생긴 지금 정한다.

### `Starting`은 컬럼이 아니라 파생이다

`Starting`은 `(status, desired_status)`의 **순수 함수**다 — `Ready`이면서 desired가 `running`인
동안이 그것이다. 그래서 `Agent::is_starting()`으로 노출하고 `AgentStatus`에는 넣지 않는다.
넣었다면 `027`의 `status IN ('ready','stopped')` CHECK를 고쳐야 하고, 무엇보다 관측 주체 없이
관측 상태를 저장하게 된다(위 유예 표의 첫 줄).

### 의도적으로 만들지 않은 것

- **명령 payload는 `(agent_id, desired_status, generation)` 셋뿐이다.** 포트·secret·cwd가
  들어오는 순간 heartbeat 응답은 통째로 로깅해도 안전한 값이 아니게 된다.
- **ACK는 관측 상태를 싣지 않는다.** 볼 프로세스가 없으므로 generation만 돌려주는 것이 정직한
  최대치다. 여기서 상태를 지어내면 오케스트레이터에 거짓이 저장되고, 4c가 이미 기록된 값의
  의미를 바꿔야 한다.
- **재조정기가 없다.** 비교할 process inventory가 없다.

### 검증 한계

- 4b는 **전달까지만** 증명한다. Worker가 명령대로 프로세스를 띄웠는지는 이 단계의 어떤
  테스트도 말하지 않으며, 그것이 `command_delivered()`와 "수렴"을 구분해 이름 붙인 이유다.
- Worker측 소비는 ACK 버퍼링까지만 있다. `Option`이 4c의 "정리" 동작을 실제로 막는지는 그
  동작이 존재하지 않으므로 지금 시험할 수 없다 — 지금 확정하는 것은 **그때 필요한 구분을
  나중에 breaking change 없이 쓸 수 있게 하는 것**뿐이다.
