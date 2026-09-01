---
type: architecture
authority: canonical
implementation: partial
verification: design-reviewed
source: "docs/architecture/agents/provisioning.md"
last_verified: "2026-09-01"
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

> **2026-09-01 인용 정정.** 바로 위 문단의 `` `#49` 2단계는 "`#48`/`#67` 뒤 Hibernated E2E"라 ``는
> 로드맵을 잘못 옮긴 것이다. `#49` 행의 원문은 "**2단계 이후**: #48/#67 뒤 Hibernated 단일 Agent
> E2E"이며, `#48`/`#67`이 붙은 대상은 2단계가 아니라 **2단계 이후**의 E2E다. 2단계 자체는 그 행이
> 정의한 적이 없다(아래 §"`#49` 2단계의 범위"가 그 빈자리를 채운다).
>
> **그런데도 분할의 결론은 그대로 선다.** 인용이 정확했더라도 결론이 바뀌지 않기 때문인데, 이유는
> "다른 근거가 또 있어서"가 아니라 **두 문장이 서로 다른 대상을 말하기 때문**이다 — 2단계가
> 소유하는 것은 Task를 Agent로 보내는 라우팅(`tasks.agent_id`)이고, 4a가 주인을 찾던 것은 Agent를
> Worker에 놓는 배정(`agents.worker_id`)이다. 어느 쪽으로 읽어도 2단계는 배정을 소유하지 않는다.

| 항목 | 왜 미뤘나 | 선행 |
|---|---|---|
| ~~`Starting`/`Running`/`Failed` 상태~~ | 셋 다 **관측**이고 관측 주체가 프로세스 매니저뿐이다. 4b(수렴 프로토콜)는 명령의 전달만 만들 뿐 프로세스를 보지 못하므로 셋 중 어느 것도 4b에서 도달하지 않는다 — `Starting`은 그나마 generation 컬럼들의 순수 함수라 컬럼 없이 파생으로 표시한다 | **4c-B에서 셋 중 둘만 해소**: `running`/`failed`가 `agents.observed_status`로 왔다. `Starting`은 **만들지 않았다** — 4c-A에 health check가 없어 생산자가 없다(아래 §관측) |
| `WarmIdle` | execution lease가 없어 "slot을 잡은 채 쉬는 상태"를 표현할 수 없다 | `#67` 후속 |
| `Hibernated` | snapshot 불일치 판정에 AgentTemplate과 harness 구성이 필요하다 | `#86`, `#51` |
| `Draining` | 위 실행 상태들이 없으면 drain할 대상이 없다. 4b에서도 만들지 않는다 — Worker의 `Draining`이 operator 개입 없이는 되돌아오지 않는 일방향 문(`fleet-api/src/handlers.rs`)이며, 그 모양을 Agent가 물려받을 이유가 없다 | `#67` 4c 이후 |
| ~~`fencing_token`~~ | 4b 시점의 유예 사유는 "생산자 자체가 없다 — `worker_execution_lease` 테이블이 존재하지 않는다"였고, 그 사유는 테이블이 언젠가 온다는 전제 위에 있었다. **그 전제가 철회됐다** — 테이블은 만들지 않고, 봉투가 이미 싣고 있는 `generation`(`agents.command_generation`, 031)이 그 역할을 그대로 한다. 봉투는 여전히 8-필드이며 **필드가 늘지 않았다**: 부족했던 것은 값이 아니라 그 값을 발행할 권한의 판정이었고, 그것은 봉투가 아니라 **쓰기 술어**에 있다 | **①-B 해소** ([권한과 장애 전환](../control-plane-authority-and-failover.md) §2026-09-01 범위 정정) |
| 재조정과 `OutcomeUnknown` | 비교할 process inventory가 없다 | `#67` 4c 후속 |
| ~~`tasks.agent_id`~~ | 지금 채우면 항상 NULL인 컬럼이 된다 — dispatch가 Agent를 고르지 않는다. 이것은 transport 사실이 아니라 **스케줄러 사실**이다 | **`#49` 2단계 해소** (`034_task_agent_id.sql`). 단 사유의 부정이 그대로 구현된 것은 아니다 — 채우는 주체는 dispatch가 아니라 **제출자**이고 dispatch는 지킬 뿐이다(아래 §"`#49` 2단계의 설계") |
| `agent:attach` capability | 붙을 터미널 세션도 grant 발급자도 없다 | `#50` |
| ACK가 Agent process의 endpoint·secret을 돌려주는 것 | 소비자가 없다 — Task를 Agent로 라우팅하는 것은 `#49` 2단계이고, 지금 넣으면 secret을 한 번도 나른 적 없는 경로에 secret을 새로 얹게 된다 | **`#49` 2단계로는 열리지 않는다.** 2단계는 Task를 Agent가 놓인 *Worker*로 보낼 뿐 Agent 프로세스에 직접 닿지 않으므로 endpoint·secret의 소비자는 여전히 없다. 이 행이 기다리는 것은 singleton `grok agent serve`를 Agent별 프로세스로 쪼개는 결정이며 그것은 2단계의 하류다(아래 §"매니저는 `GrokRunner`를 대체하지 않고 옆에 선다") · 미정 |
| 명령 payload의 포트·secret·cwd | 4b의 명령은 `(agent_id, desired_status, generation)` **뿐**이다. 이 셋이 들어오는 순간 heartbeat 응답은 통째로 로깅해도 안전한 값이 아니게 되므로, 4c가 무심코 얹지 않도록 지금 적어 둔다 | `#67` 4c-A(해소: 워커 로컬 파생) |
| ~~ACK가 관측 상태를 싣는 것~~ | 볼 프로세스가 없다. 4b의 ACK는 generation만 돌려주며 그것이 정직한 최대치다 | **4c-B 해소**. 단 ACK에 얹지 **않았다** — heartbeat 요청의 형제 필드 `agent_observations`로 분리했다. ACK는 명령당 부기이고 관측은 명령 없이도 생기기 때문이다(프로세스는 아무 명령 없이 죽는다) |
| ~~Worker가 신고하는 `max_agent_processes`~~ | 4c-B 시점의 유예 사유는 "읽을 소비자가 없다"였고, 그 소비자(배정 시점의 하드 상한)는 게이트 ①에 묶여 있었다. **게이트 ①-A-1이 그 소비자를 만들면서 사유가 만료됐다** — 아래 §"배정 슬롯 상한"이 정본이다. 4c-B의 `observed_reason = 'cap_reached'`는 대체되지 않고 남는다: 두 수가 세는 것이 다르기 때문이다(오케스트레이터는 배정된 `agents` 행, Worker는 살아 있는 프로세스) | **①-A-1 해소** |
| 포트 소진·상한 거절과 4a 원장의 불일치 | Worker가 거절해도 `agents.worker_id`는 그대로 남는다 — 원장은 "배정됐다"고 세고 Worker는 "못 띄운다"고 본다. 이 창을 닫으려면 배정 시점에 자리를 예약하는 CAS slot claim이 필요하다. ①-A-1의 상한 필터는 이 창을 **좁히지만 닫지 않는다** — 필터는 읽은 시점의 카운트로 판정하므로 동시 요청 둘이 함께 통과할 수 있다 | `#67` 구현 게이트 ①-A-2 |
| per-Agent secret의 회전 | 회전은 살아 있는 프로세스를 재시작시키는 결정이고, 재시작을 관측·보고할 채널이 4c-A에는 없다. 워커 재기동 시 전부 새로 생성되는 것이 지금의 유일한 회전이다 | 미정 |
| `agent_workspace_root`와 Project Git workspace의 관계 | 4c-A의 작업 디렉터리는 **프로세스의 cwd**일 뿐 checkout이 아니다. Git workspace·checkpoint는 `#69`가 소유하며, 지금 둘을 합치면 `#69`가 자기 설계를 4c-A의 디렉터리 규약에 맞춰야 한다 | `#69` |
| Agent 프로세스의 우아한 종료 | 지금은 SIGTERM을 **보내지 않는다** — `fleet-worker`는 `#![forbid(unsafe_code)]`이고 `libc::kill`은 unsafe, `nix`는 의존성에 없다. singleton이 이미 같은 제약 아래 있으므로 4c-A가 새로 만든 문제가 아니다 | 미정 |

위 "구현 게이트" 6개는 전부 명령·ACK 계층의 시험이므로 1단계 범위 밖이다. 1단계가 실제로
증명한 것은 (a) FK·유일성 위반이 `StoreError::Conflict`로 번역되는지, (b) 살아 있는 Agent 하나가
Task 없이도 Project를 `draining`에 붙잡는지 — 두 가지이며 `crates/fleet-store/tests/agents.rs`,
`fleet-mcp`/`fleet-dashboard` 테스트가 근거다.

### `#49` 2단계의 범위 (2026-09-01 확정)

위 유예 표의 두 칸(`tasks.agent_id`, ACK가 endpoint·secret을 돌려주는 것), 아래 §"구현 게이트"의
`worker_execution_lease` 행, 그리고 [권한과 장애 전환](../control-plane-authority-and-failover.md)의
게이트 ①-B·②가 **넷 다 `#49` 2단계를 선행으로 지목한다**. 그런데 지목당한 쪽에는 정의가 없었다 —
로드맵 `#49` 행은 1단계만 서술하고 "2단계 이후"의 E2E만 예고할 뿐, 2단계가 무엇인지는 적은 적이
없다. 지목이 넷인데 대상이 비어 있으면 지목마다 다른 것을 상상하게 되고, 바로 위 인용 정정이 그
결과다. 그래서 여기서 정의한다.

**2단계 = dispatch가 Agent를 고른다.** 구체적으로 `tasks.agent_id`와 그 라우팅이다. 지금
`Selector::select`는 `WorkerId`를 돌려주고(`crates/fleet-scheduler/src/selector.rs`), `Task`에는
`agent_id` 필드가 없다(`crates/fleet-core/src/task.rs`). 2단계는 이 두 사실을 바꾸는 일이며, 그것이
끝나는 순간 위 네 지목이 함께 열린다.

| 2단계가 아닌 것 | 어디로 | 왜 |
|---|---|---|
| Hibernated 단일 Agent E2E | 3단계 | 로드맵 원문이 "2단계 **이후**"라고 적은 대상이 이것이다. 선행도 다르다 — `#48`/`#67`에 더해 snapshot 불일치 판정용 AgentTemplate(`#86`)과 harness 구성(`#51`)이 필요하다(위 표 `Hibernated` 행) |
| 항목 제목의 `memory·summary·tool binding` | 3단계 이후(별도) | 제목에만 있고 로드맵 `#49` 본문에도 이 문서의 유예 표에도 **한 번도 나오지 않는다**. 셋 다 Agent에 무엇을 실을지의 문제라 harness 구성(`#51`)에 걸리며, 어디로 보낼지(라우팅)와는 다른 축이다. 2단계에 끌어들이면 방금 채운 것과 같은 종류의 미정의 경계를 하나 더 만든다 |

**`#48`은 2단계의 선행이 아니다.** 로드맵 `#48` 본문은 `#49`를 한 번도 언급하지 않고, `#48`/`#67`이
붙은 대상은 "2단계 이후"의 E2E다. 방향은 오히려 반대다 — `#48`의 남은 차단 조건 2는 Task 제출이
Agent에 닿는 경로를 기다리는데(로드맵 `#49` 1단계 서술: "2번이 시험하는 *자동* provisioning 경로가
없고(Task 제출은 Agent를 만들지 않는다)"), 그 경로의 첫 절반이 2단계다. 2단계가 그 조건을 **닫는지는
별개**다: 조건 2는 고르는 것이 아니라 만드는 것을 시험할 수 있고, 조건 3은 lease를 따로 기다린다.
확실한 것은 `#48`을 2단계의 선행으로 읽으면 의존이 순환한다는 것이며, 이는 위 2026-08-30 귀속
정정이 `#67`↔`#89`에서 닫은 것과 같은 모양이다.

> **2026-09-01 범위 정정 (같은 날, 구현 착수 직전).** 위 §의 두 문장이 각각 과했다.
>
> 1. **"그것이 끝나는 순간 위 네 지목이 함께 열린다"** — 넷이 아니라 **셋**이다. "ACK가 Agent
>    process의 endpoint·secret을 돌려주는 것"은 열리지 않는다. 그 행이 기다리는 것은 singleton
>    `grok agent serve`를 Agent별 프로세스로 쪼개는 일인데, 아래 §"매니저는 `GrokRunner`를
>    대체하지 않고 옆에 선다"가 그것을
>    "**둘을 합치는 것은 dispatch가 Agent를 고르게 된 뒤의 결정**"이라고 적는다 — 즉 그 행은
>    2단계와 **함께** 열리는 것이 아니라 2단계의 **하류**다. 넷을 한 문장에 묶은 것은 지목의
>    개수를 세다가 방향을 잃은 것이다.
> 2. **"2단계 = dispatch가 Agent를 고른다"** — 로드맵의 유예 사유("dispatch가 Agent를 고르지
>    않는다")를 그대로 뒤집어 쓴 문장인데, 그 사유의 부정은 "dispatch가 **고른다**"가 아니라
>    "그 컬럼이 **항상 NULL이 아니게 된다**"이다. 아래 §가 정의를 좁힌다.

### `#49` 2단계의 설계 (2026-09-01 확정)

**2단계 = `tasks.agent_id`가 실재하고 dispatch가 그것을 지킨다.** 채우는 주체는 **제출자**이고,
dispatch는 지목된 Agent를 **지킬 뿐 고르지 않는다**. 자동 선택 정책은 2단계 밖이다.

좁힌 근거는 소비자다. 위 정정 뒤 남는 지목 셋 — `tasks.agent_id` 유예 행, `#67` 게이트 ①-B(lease
레코드가 `task_id`를 싣는다), 게이트 ② — 은 **셋 다 dispatch가 고를 것을 요구하지 않는다**. 셋 다
"컬럼이 존재하고, 채워지고, dispatch가 그것을 따른다"로 충족된다. 지금 선택기를 만들면 읽는
소비자가 하나도 없는 정책을 만드는 것이고, 이는 이 저장소가 컬럼에 대해 지켜 온 "채울 방법이 없는
것은 미리 만들지 않는다"를 한 층 위(정책)에 적용하지 않는 것뿐이다. 게다가 그 정책에는 이미 주인이
있다 — "고를 Agent가 하나도 없으면 만든다"는 `#48` 조건 2가 기다리는 *자동* provisioning이고,
여기서 만들면 위 §이 "별개"라고 남겨 둔 질문에 **우연히** 답해 버린다.

#### 결정 1 — 라우팅은 `server_hint`와 같은 자리·같은 의미다

`agent_id`는 `Selector::select`의 필터 사슬을 **통과한 뒤** 좁히며, 폴백하지 않는다. 이는
`server_hint`가 이미 확립한 모양이다(`crates/fleet-scheduler/src/selector.rs`, 테스트
`on_demand_worker_cannot_be_forced_by_server_hint`의 주석: "server_hint는 폴백을 막을 뿐 필터를
무시하는 권한이 아니다"). Agent의 Worker가 오프라인·용량초과·차단 상태면 그 Task는 배정되지
않는다 — 지목이 필터를 무시하는 권한이 되면 죽은 Worker로 Task를 보내게 된다.

**`agent_id`와 `server_hint`를 함께 준 요청은 제출에서 거절한다.** 같은 결정에 핀이 둘이고,
`agent_id`가 Worker를 이미 함축한다. 둘이 일치하는지 검사해서 통과시키는 방안은 택하지 않았다 —
Agent가 아직 배정되지 않았으면(`worker_id IS NULL`, `7009e4b` 기준 **회복 가능한 정상 상태**)
일치 여부를 알 수 없어, 검사가 언제 작동할지 요청자가 예측할 수 없게 된다.

#### 결정 2 — 존재 검증은 제출, 가용성 판정은 dispatch

없는 `agent_id`는 **제출에서 거절**한다. 이 저장소가 `project_id`에 대해 이미 그렇게 한다
(`fleet_dispatch_task`의 `dispatch_task_rejects_unknown_project_id`·`_archived_project_id`).
그 결과 dispatch가 보는 실패는 **일시적인 것들만** 남는다:

| dispatch 시점 실패 | `SelectionError` | 운영자가 할 일 |
|---|---|---|
| Agent 행이 사라짐 | `AgentNotFound` | 다른 Agent로 재제출 (현재 hard delete 경로가 없어 실제로는 도달 불가. 방어적 분기) |
| `desired=Stopped` 또는 `observed=Failed` | `AgentNotRunning` | Agent를 다시 Running으로 두거나 실패 원인 조사 |
| `start_pending()` — 시작 명령은 냈고 보고가 없음 | `AgentNotObserved` | ACK가 도착할 때까지 기다린다 |
| `worker_id IS NULL` | `AgentUnplaced` | 배정 회복을 기다린다 (`7009e4b`) |
| Worker가 필터에서 탈락 | `AgentWorkerUnavailable` | 그 Worker를 살린다 |
| Worker가 용량 초과 | `AgentWorkerAtCapacity` | 기다리거나 상한을 올린다 |

variant를 여섯으로 나눈 이유는 `AllAtCapacity`·`AllUnprobed`의 doc이 적어 둔 것과 같다 — 잘못된
variant는 운영자에게 **잘못된 대응**을 시킨다. 반대로 `FailureKind`는 **늘리지 않는다**:
`dispatch_existing`의 매핑은 `NoWorkerForCredential`만 특별 취급하고 나머지를
`WorkerUnavailable`로 모으며, 정확한 원인은 `e.to_string()`이 `TaskFailure::error`로 실어 나른다.
위 여섯은 전부 "지금은 못 간다, 나중엔 갈 수 있다"라 그 분류가 맞다.

**세 번째 행이 이 설계에서 유일하게 값이 갈리는 자리다.** `start_pending()`
(`crates/fleet-core/src/agent.rs`, "명령은 냈고 답이 없다")인 Agent를 배정 대상으로 볼 것인지에
대해 여기서는 **닫는 쪽**을 택했다 — `AllUnprobed`가 "probe가 구현될 때까지 이 워커는 배정
대상이 아니다"로 이미 같은 선택을 했기 때문이다. 반대 값도 방어할 수 있다(ACK 경로가 늦어도
Task는 흐르게 한다). 뒤집는 비용을 한 줄로 만들어 두려고 판정을 `agent_dispatchable` 한 곳에
모았다 — 뒤집는 것은 그 함수에서 `start_pending` 분기를 지우는 것이다.

**Store 조회가 실패하면 fail-open하지 않는다.** `count_dispatched_tasks_by_worker`가 이미 쓰는
관례대로 `tracing::error!` 후 일반 실패로 접는다 — 조회 실패를 "그런 Agent 없음"으로 보고하면
운영자가 있지도 않은 삭제를 쫓는다.

#### 결정 3 — `project_id`가 비면 Agent에서 물려받는다

`task.project_id`가 `None`이면 **지목된 Agent의 것을 물려받는다**. `Agent::project_id`는
`Option`이 아니라 `ProjectId`이므로(`crates/fleet-core/src/agent.rs`) 물려받을 값은 **항상
있다** — 즉 Agent를 지목한 Task는 절대 일반 풀로 떨어지지 않는다. 둘 다 있는데 다르면
거절한다(교차 project 금지, `link_issue_task`와 같은 계열). 물려받는 쪽을
택한 이유는 경계 불변식(`#58`)이 참으로 유지되기 때문이다 — 거절하면 오케스트레이터가 이미
아는 사실을 제출자가 매번 반복해야 하고, 생략을 오류로 만들면서 답은 하나뿐인 상황이 된다.

**물려받은 `project_id`도 명시 입력과 똑같이 입장 검증을 받는다.** 두 표면(MCP·Dashboard)은
`ensure_project_accepts_new_tasks`를 **명시 입력에만** 걸고 있었으므로, 상속 경로를 그대로 두면
`Draining`/`Archived` Project의 Agent를 지목하는 것이 그 Project에 새 Task를 넣는 우회로가
된다. 그래서 검증을 호출부가 아니라 `apply_agent_pin` **안**에 두었다 — 두 표면이 규칙을 각자
기억하지 않아도 되게. `inherit_from_parent`가 상속한 `project_id`를 호출부가 다시 검증해야 했던
것(`#48` 2단계)과 같은 함정이고, 같은 답이다.

#### 지목은 이어가기로 전파되지 않는다

`Task::inherit_from_parent`는 `server_hint`·`cwd`·`model`·`project_id`를 물려주지만 `agent_id`는
**물려주지 않는다**. 지목의 유효성은 제출 시점 존재 검증에 의존하는데(위 결정 2), 상속은 그
검증보다 뒤에 일어나므로 물려주면 이미 사라진 Agent를 검증 없이 다시 지목하게 된다. 같은
Agent로 이어가려면 제출자가 다시 지목한다.


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
- ~~원장을 읽고 INSERT하기까지가 **원자적이지 않다.**~~ 4a 시점의 한계였고 게이트 ①-A-2가
  닫았다. 동시에 두 Agent를 만들면 둘 다 같은 Worker를 고를 수 있었는데, 상한이 없던 4a에서는
  부하 분포가 잠시 기우는 것으로 끝났지만 게이트 ①-A-1이 상한을 들여온 뒤로는 그 창이 초과
  배정을 뜻했다. `choose_worker`의 선택은 여전히 원자적이지 않지만, **쓰기 시점의 선점**이
  그 뒤에서 불변식을 세운다 — 아래 §"배정 슬롯 상한"의 "선점은 `workers` 행의 잠금 아래에서
  센다"가 그 메커니즘의 정본이다. 남은 것은 불변식이 아니라 배정을 놓치는 것뿐이다
  (같은 §의 "남은 한계: 헛도는 미배정").

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

> **4c-B 후기.** 이 절의 결론은 유지되지만 술어는 항이 하나 늘었고 이름이 바뀌었다
> (`Agent::start_pending()`, 아래 §"`start_pending`으로 개명한 진짜 이유"). 여기서 남길 것은
> **관측을 얹는 변경이 파생 술어를 조용히 낡게 만든다**는 사실이다: `(Ready, running)` 두 항은
> 프로세스가 실제로 뜬 뒤에도 그대로라, 항을 늘리지 않으면 돌고 있는 Agent가 영원히
> "시작 중"으로 보고된다. 컴파일러는 이것을 잡지 못하고 4b의 어떤 테스트도 깨지지 않는다 —
> 그 테스트들은 새 컬럼을 모르기 때문이다. 파생값에 입력을 추가할 때의 일반형이다.

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

## 설계 결정 (`#67` 4c — 워커측 프로세스 매니저)

> **2026-08-31 재분할.** 4c를 **4c-A**(워커측 프로세스 매니저)와 **4c-B**(관측 상태의 왕복)로
> 나눈다. 아래 첫 절이 근거다. 이 절은 **두 단계 모두의** 결정을 지금 확정한다 — 그중 하나
> (`is_starting`의 이름)는 4c-B가 표면에 노출한 뒤에는 고치는 비용이 달라지기 때문이다.

### 왜 다시 나누는가 — 분할선은 마이그레이션 경계다

`Starting`/`Running`/`Failed`를 `agents.status`에 넣으려면 `027`의
`status IN ('ready','stopped')` CHECK를 넓혀야 하고, 그 세 값에는 **생산자가 있어야 한다**.
생산자는 프로세스를 보는 매니저인데, 매니저는 4c-A가 만든다. 그런데 매니저가 본 것이
`agents` 행까지 **도달**하려면 ACK가 관측 상태를 싣고 store가 그것을 적용해야 한다 — 즉
CHECK를 넓히는 커밋은 매니저·ACK 확장·store 경로·세 표면 스키마를 전부 포함해야 죽은 값을
만들지 않는다.

`#70`에서 생산자 없는 `FailureKind` variant 셋을 걷어낸 비용을 이 프로젝트는 이미 치렀다.
그래서 경계를 마이그레이션에 맞춘다:

| | 만드는 것 | 만들지 않는 것 |
|---|---|---|
| **4c-A** | 매니저·설정·포트/secret/workspace 파생·수렴과 정리 | 마이그레이션, `AgentStatus` variant, ACK 확장 |
| **4c-B** | 새 마이그레이션(`032`)의 관측 컬럼 셋과 CHECK, heartbeat 요청의 관측 필드, store 적용 경로, 세 표면 | `AgentStatus` variant 추가(관측은 **다른 축**이라 같은 컬럼에 얹지 않는다), `Starting`(생산자 없음), `max_agent_processes` 신고(소비자 없음) |

4c-A가 끝난 시점에 오케스트레이터가 아는 것은 여전히 **전달**까지다. 프로세스는 실제로 뜨지만
그 사실은 워커 로그에만 남는다. 이것은 구멍이 아니라 **명시된 한계**이며, 4b가
`command_delivered`를 수렴이라 부르지 않은 것과 같은 종류의 정직함이다. 4c-B가 이 한계를 닫는다.

> **4c-B에서 이 표의 왼쪽 칸이 틀렸음이 드러났다.** "CHECK 확장"은 `027`의
> `status IN ('ready','stopped')`를 넓힌다는 뜻이었는데, 실제로는 넓히면 **안 된다**(아래
> §관측은 `status`와 다른 축이다). 경계를 마이그레이션에 맞춘다는 원칙 자체는 옳았고 —
> 실제로 4c-B는 마이그레이션 하나(`032`)와 함께 그 값을 읽는 경로 전부를 같은 커밋에
> 담았다 — 틀린 것은 **어느 마이그레이션인가**였다.

### 매니저는 `GrokRunner`를 대체하지 않고 옆에 선다

1:1 singleton을 Agent별 1:N으로 "바꾼다"는 4단계 최초 정의문은 그대로 실행하면 안 된다.
singleton `grok agent serve`는 **Worker 자신의 ACP 종단**이고 그 주소가
`workers.endpoint`로 등록되어 있으며, 모든 Task dispatch가 그리로 간다. Agent별로 쪼개는
순간 dispatch는 `tasks.agent_id` 라우팅(`#49` 2단계)이 생기기 전까지 **갈 곳을 잃는다**.

따라서 4c-A의 `AgentProcessManager`는 `GrokRunner`와 **공존**한다. singleton은 Task 종단으로
남고, 매니저는 Agent별 프로세스를 따로 띄운다. 둘을 합치는 것은 dispatch가 Agent를 고르게 된
뒤의 결정이다.

### 포트·secret·cwd는 워커 로컬에서 파생한다

명령 payload는 `(agent_id, desired_status, generation)` 셋으로 **고정**이다(위 유예 표와
§"의도적으로 만들지 않은 것"). 그러면 프로세스마다 달라야 하는 셋은 Worker가 스스로 만들어야
한다. 규칙은 셋 다 워커 설정(`[grok]`)에서 나온다:

| 값 | 파생 규칙 | 기본값 |
|---|---|---|
| bind 주소 | `bind_addr`의 host + `agent_port_range`에서 비어 있는 포트 하나 | `2420-2519` |
| secret | Agent마다 **새로 생성**(`hex(rand 32B)`). `grok.secret` 재사용 금지 | — |
| cwd | `agent_workspace_root/<agent_id>` | `<grok.cwd 또는 워커 cwd>/fleet-agents` |

**`grok.secret`을 재사용하지 않는 이유**: 그 값은 이미 Worker의 ACP 종단을 여는 열쇠이고,
Agent 프로세스에 같은 값을 주면 Agent 하나가 샜을 때 Worker 종단까지 열린다 —
"secret 하나 = 노출 범위 하나"가 깨진다. 포트 범위가 `2420`에서 시작하는 것은 singleton의
`2419` 바로 다음이라는 뜻이며, 겹치지 않음이 눈으로 읽힌다.

**4c-A에서 per-Agent secret은 워커 밖에 소비자가 없다.** 소비자는 자식 프로세스 자신뿐이다
(`grok agent serve --secret`이 값을 요구한다). 이 격리가 실제로 무언가를 막는지는 Agent
프로세스에 **닿는 경로**가 생기는 `#49` 2단계에서야 관측 가능하다.

### 거절 경로는 하나다

포트 소진과 `max_agent_processes` 초과는 원인이 다르지만 **결과가 같다** — 그 Agent는 이번
beat에 뜨지 않는다. 두 개의 서로 다른 오류로 보고하면, 관측 상태가 생기는 4c-B에서 둘 다
`Failed`로 접히면서 운영자는 두 이름을 보고 같은 처방을 찾게 된다. 하나의 거절 경로로 모으고
**원인은 로그 필드로** 구분한다.

**4c-A에서 이 거절은 워커 로그 한 줄과 "뜨지 않음"이 전부다.** 오케스트레이터에 도달하는
채널이 없기 때문이다. 4c-B가 이 자리를 관측 상태로 잇기 전까지는 **조용한 실패 모드**이며,
적어 두지 않으면 4c-B가 그것을 물려받고도 모른다.

> **4c-B에서 닫혔다.** 두 거절은 각각 `observed_reason = 'cap_reached'`/`'no_free_port'`로
> 오케스트레이터에 도달한다. 여기 적어 둔 것이 실제로 값을 했다 — 4c-B가 어휘를 정할 때
> 후보를 이 절에서 그대로 읽었고, 그 과정에서 **어휘에 없던 세 번째 생산자**를 찾았다:
> `cmd.spawn()`의 `Err` 경로(`spawn_failed`)다. 그것 역시 4c-A에서는 로그 한 줄이었다.

### `is_starting`은 4c-B에서 `start_pending`으로 바꾼다

지금 `Agent::is_starting()`은 `(Ready, desired=running)`의 파생값이고 "명령은 냈는데 아직
개시되지 않았다"를 뜻한다. 4c-B가 관측 `Starting`을 저장하면 이름이 하나 더 생기는데, 두
값은 뜻이 다르다:

| | 뜻 | 주체 |
|---|---|---|
| 파생 `is_starting` | 시작을 지시했고 Worker가 아직 아무것도 보고하지 않았다 | 오케스트레이터 |
| 관측 `Starting` | Worker가 자식을 띄웠고 아직 health check 전이다 | Worker |

둘은 **상호배타**다. 그래서 4c-B 이후 `{"status":"starting","is_starting":false}`가 정상
응답이 되는데, 이것은 읽는 쪽에서 자기모순으로 보인다. 관측 쪽 이름은 상태 기계표가 이미
`Starting`으로 고정했으므로, **파생 쪽을 `start_pending`으로 바꾼다**.

바꾸는 시점은 4c-B다 — 충돌이 실제로 생기는 커밋에서 상태 어휘 변경 하나로 처리하는 편이,
워커 코드만 건드리는 4c-A에 세 표면의 이름 변경을 섞는 것보다 읽기 쉽다. 결정을 여기 적어
두는 것은 4c-B가 그 자리에 도착했을 때 **다시 판단하지 않게** 하기 위해서다.

### `start_pending`으로 개명한 진짜 이유

개명은 예정대로 4c-B에서 했지만 **위 절이 든 사유로는 아니다.** 위 절은 관측 `Starting`이
저장되어 이름이 충돌한다고 봤는데, 4c-B는 그 variant를 만들지 않았다(§관측의 어휘). 충돌은
일어나지 않았다.

바꾼 이유는 **술어의 뜻 자체가 달라졌기 때문**이다. 관측이 생긴 뒤 이 값은 "시작 중"이
아니라 **"명령은 냈고 아직 아무 답도 없다"**이며, 답이 오면 그 답이 성공이든 실패든
`false`가 된다:

```rust
self.status == Ready && self.desired_status == Running && self.observed_status.is_none()
```

운영상 이 술어가 구분하는 것은 **"Worker가 아직 집어가지 않았다"와 "집어갔는데 못
띄웠다"**이다. 후자는 `observed_status = 'failed'`이고 `start_pending`은 `false`다 — 예전
이름이었다면 못 띄운 Agent가 영원히 "시작 중"으로 보였을 자리다.

> 결정을 미리 적어 둔 것 자체는 값을 했다(개명 여부를 다시 판단하지 않았다). 값을 하지 못한
> 것은 **사유**다. 사유를 미리 적으면 그 사유가 사라졌을 때 결론만 남아 근거 없이 굳는다.

### 관측은 `status`와 다른 축이다

4c-A의 범위 표는 4c-B가 `027`의 `status IN ('ready','stopped')` CHECK를 넓힐 것으로 적었다.
넓히면 안 된다. `agents.status`에 관측을 얹으면 **한 컬럼에 두 명이 쓴다**:

1. 운영자가 회수한다 → `status = 'stopped'`.
2. 그 회수를 보기 **전에** 만들어진 heartbeat이 도착한다 → `status = 'running'`.

회수가 조용히 취소된다. 게다가 `AgentStatus::blocks_project_archive()`가 `status`를 읽으므로
회수된 Agent가 다시 Project archive를 막기 시작한다. 반대로 컬럼을 나누면 이 함수는 **손대지
않고도 계속 옳다** — 돌고 있는 Agent도 `status='ready'`라 막고, 회수된 Agent는
`status='stopped'`라 막지 않는다. 축이 실제로 다르다는 방증이다.

`031`의 마이그레이션 주석이 이미 같은 결론을 적어 뒀다: "관측은 4c가 **별도 필드로** 얹어야
한다."

정본이 관측 컬럼에 반대해 온 논거("두 번째 진실 원천")는 여기에 닿지 않는다. 그 논거는 다른
컬럼에서 **파생되는** 값을 저장하는 것을 겨눴다(예: generation 컬럼들에서 나오는 `Starting`).
`observed_status`는 파생되지 않는다 — Worker만 아는 정보다.

### 관측의 어휘 — 생산자가 있는 것만 만든다

| 값 | 생산자 (4c-A `reconcile`) |
|---|---|
| `running` | 이미 살아 있는 자식 / 방금 spawn 성공 |
| `failed` + `cap_reached` | 상한 거절 |
| `failed` + `no_free_port` | 포트 소진 거절 |
| `failed` + `spawn_failed` | `cmd.spawn()`의 `Err` |

만들지 않은 둘:

- **`starting`.** 상태 기계표는 "자식을 띄웠고 아직 health check 전"으로 정의했는데 4c-A에
  health check가 **없다**. `try_wait()`는 "죽지 않았다"만 말한다. 계산할 방법이 없는 variant는
  `#70`이 걷어낸 것과 같은 죽은 값이다.
- **`exited`.** `reconcile`은 0단계에서 죽은 자식을 거두고 3단계에서 **같은 beat에** 다시
  띄운다. 그러므로 한 beat의 정직한 관측은 `running`(또는 재기동 실패의 이유)이지 `exited`가
  아니다.

이유는 상태가 아니라 **필드**다(`observed_reason`). 상태로 만들면 "왜"가 상태 공간을 곱하고,
`failed`가 아닌 값에 이유가 붙는 불가능한 조합이 표현 가능해진다. `AgentObservation`을 구조체가
아니라 tagged enum으로 둔 것도 같은 이유다 — 구조체였다면 이유 없는 `failed`를 만들 수 있고,
그 결함은 원인(워커의 메시지)에서 아주 먼 곳(**DB CHECK 위반**)에서 드러난다.

`RejectReason::as_str()`(사람이 읽는 로그 문구)과 `AgentObservationReason::as_str()`(DB CHECK
값)은 **합치지 않는다.** 합치면 로그 문구를 다듬는 순간 스키마가 깨진다. 둘 사이는 명시적인
`From`이 잇는다.

### 관측 목록도 권위 있는 전체 집합이다 — 방향만 반대다

heartbeat **응답**의 명령 목록이 Worker에게 권위 있는 전체 집합인 것과 대칭으로, heartbeat
**요청**의 관측 목록은 오케스트레이터에게 권위 있는 전체 집합이다. 목록에 없는 Agent의 관측은
**지운다**. 이 단계가 없으면 회수된 Agent가 `observed_status='running'`인 채로 영원히 남는다 —
지워 줄 사람이 아무도 없다.

따라서 요청 필드도 `Option<Vec>`이며 4b가 응답에서 세운 것과 같은 구분을 갖는다:

| 값 | 뜻 |
|---|---|
| 필드 부재(`None`) | 말해 줄 것이 없다(구버전 Worker, 또는 권위 있는 명령 목록을 못 받은 beat). **아무것도 바꾸지 마라** |
| `[]` | 이 Worker에 돌고 있는 Agent가 하나도 없다. **남은 관측을 전부 지워라** |

`Vec` + `skip_serializing_if = "Vec::is_empty"`로 두면 둘이 구별되지 않고, 마지막 Agent를
회수한 순간부터 관측을 지울 방법이 사라진다. 타입이 잡아 주지 않는 결함이라
`fleet-api/tests/api_flow.rs`가 **선 위에서** 단정한다.

store 적용의 CAS 조건은 `worker_id` 하나뿐이다. 4b의 `ack_agent_commands`가 가진 세 조건 중
나머지 둘은 generation에 관한 것인데 **관측에는 generation이 없다** — 프로세스는 아무 명령
없이도 죽는다. `updated_at`은 밀지 않는다(ACK와 같은 이유: 프로토콜 부기이지 운영자의 조작이
아니며, 밀면 "언제 회수됐는가"가 한 beat 뒤에 무효가 된다).

### 4c-B의 검증 한계 — crash loop은 보이지 않는다

`reconcile`이 0단계에서 거두고 3단계에서 같은 beat에 다시 띄우므로, **매 beat 죽는 Agent도
`running`으로 관측된다.** 상태만 나르는 채널로는 원리적으로 드러나지 않는다 — 드러내려면
재기동 **횟수**나 전이 같은 **사건**이 필요하고, 그 채널도 그것을 읽을 소비자도 아직 없다.

이것은 4c-A의 "조용한 실패 모드"와 다른 종류다. 그때는 채널이 없어서 아무것도 몰랐고, 지금은
채널이 있고 그 채널이 나를 수 있는 것의 모양이 상태라서 사건이 빠진다.

### 4c-A가 발명하지 않는 것

- **Agent `Draining`.** 위 유예 표가 `#67` 4c 이후로 못박았다.
- **Worker `drain`이 Agent 명령을 억제하는 것.** §"재조정과 회수"가 금지한다. 이미 배정된
  Agent를 drain 시점에 죽이는 것은 별개의 결정이다.
- **배경 재조정기.** 비교할 process inventory는 4c-A가 만들지만, 그것을 오케스트레이터와
  맞대는 것은 관측이 왕복한 뒤의 일이다.
- **`on_demand` Worker에서의 수렴.** 그 워커는 heartbeat 루프를 아예 시작하지 않으므로
  (`runner.rs`) 매니저의 `reconcile`이 한 번도 호출되지 않는다. 4a의 배정 선택기가
  `on_demand`를 후보에서 이미 빼므로 지금은 도달 불가능한 조합이며, 그것이 옳다.

### 종료는 프로세스당 5초이므로 배치로 묶는다

`grok_process::terminate_child`는 자식이 스스로 끝나기를 5초 기다린 뒤 SIGKILL한다.
이름과 달리 **SIGTERM을 보내지 않으며**, `grok agent serve`는 끝날 이유를 통보받지
못하므로 그 5초를 **항상 다 쓴다**. singleton 하나일 때는 종료 경로에 한 번 붙는
고정 비용이라 눈에 띄지 않았지만, Agent가 여럿이면 곱해진다.

직렬로 정리하면 상한 4개 기준 20초이고, 그 20초 동안 `reconcile`은 프로세스 맵의
lock을 쥔 채 반환하지 않는다 — heartbeat 간격이 15초이므로 beat을 통째로 건너뛴다.
그래서 정리는 `terminate_all`로 묶어 **동시에** 돌린다. 배치 전체가 약 5초가 되고,
Agent 수와 무관해진다.

이것은 성능 조정이 아니라 **정확성 쪽에 가깝다**: 지연이 heartbeat 주기를 넘기면
Worker가 살아 있는데도 명령을 받지 못하는 구간이 생긴다.

### `Option<Vec>`이 여기서 비로소 하중을 받는다

4b가 `HeartbeatResponse.agents`를 `Option<Vec>`으로 둔 이유가 4c-A에서 처음으로 시험 가능해진다.
매니저는 목록을 **권위 있는 전체 집합**으로 읽고 "목록에 없는 것은 정리한다"를 하므로:

- `None` → 아무것도 하지 않는다. store 조회가 실패한 beat이 그 Worker의 Agent를 전부 죽이면 안 된다.
- `Some([])` → 전부 정리한다. 정말로 배정된 Agent가 없다는 뜻이다.

4c-A의 테스트는 이 두 값을 **반드시 구분해서** 확인한다 — 구분이 무너지는 순간의 대가가
"함대 전체 종료"이기 때문이다.

## 배정 슬롯 상한 (`#67` 구현 게이트 ①-A)

[권한과 장애 전환](../control-plane-authority-and-failover.md)의 게이트 ①은 두 개의
실패 모드가 한 칸에 묶여 있었다. **①-A는 배정 초과** — 동시에 들어온 두 생성 요청이
같은 Worker를 골라 그 Worker의 프로세스 상한을 넘긴다. **①-B는 낡은 제어면** —
승격된 오케스트레이터의 명령을 Worker가 거절하지 못한다. 둘 다 lease 테이블 없이
성립한다 — ①-A는 세어야 할 것이 lease 행이 아니라 **배정된 `agents` 행**이기 때문이고,
①-B는 판정이 레코드가 아니라 **쓰기 술어**에 살기 때문이다(2026-09-01 확정,
[권한과 장애 전환](../control-plane-authority-and-failover.md) §범위 정정).

> 이 문단은 2026-08-31까지 "후자는 `worker_execution_lease` 레코드가 필요하고 그
> 레코드는 `task_id`를 실으므로 `#49` 2단계에 걸린다"고 적었다. 그 판단이 틀렸던
> 지점은 선행 관계가 아니라 **fencing을 레코드로 상상한 것**이다. 낡은 제어면을
> 막는 데 필요한 것은 명령마다의 행이 아니라, 명령을 쓰는 그 문장이 "지금도 내가
> 리더인가"를 함께 묻는 것이다.

①-A는 다시 둘로 나뉘고, 각각 독립적으로 검증된다.

| | 내용 | 성립하는 것 | 상태 |
| --- | --- | --- | --- |
| **①-A-1** | Worker가 등록 시 상한을 보고 → `workers.max_agent_processes`(nullable) → `choose_worker`의 후보 필터 | 가득 찬 Worker가 후보에서 빠진다. 경합은 남는다 | 완료 |
| **①-A-2** | `workers` 행을 `FOR UPDATE`로 잠근 아래에서 세고 조건부로 배정 | 불변식이 성립한다 | 완료 |

①-A-1만으로는 불변식이 서지 **않았다**. 필터는 읽은 시점의 카운트로 판정하므로 동시
요청 둘이 같은 마지막 슬롯을 보고 함께 통과할 수 있다. 그런데도 먼저 커밋한 것은,
필터가 없으면 ①-A-2가 잠금 아래에서 셀 **숫자 자체가 존재하지 않기** 때문이다 —
상한이 도착하는 것이 선행 조건이고, 그것만으로도 정상 경로(경합 없는 배정)에서는
초과가 사라진다. ①-A-2가 그 위에 잠금을 얹으면서 불변식은 이제 성립한다(아래
§"선점은 `workers` 행의 잠금 아래에서 센다").

### 상한의 출처는 Worker이고, 경로는 등록이다

`max_agent_processes`는 이미 Worker config에 있다(`fleet-worker/src/config.rs`). 4c의
프로세스 매니저가 그 값을 집행하고, 초과 시 `observed_reason = 'cap_reached'`로 보고한다.
없는 것은 **오케스트레이터가 그 숫자를 아는 것**뿐이다.

경로는 `max_concurrent`을 그대로 비춘다 — 하트비트가 아니라 **등록**이다:

```
config.grok.max_agent_processes
  → fleet-worker registration.rs 의 RegisterRequest.max_agent_processes
  → fleet-api RegisterRequest → build_worker → Worker.max_agent_processes
  → workers.max_agent_processes (마이그레이션 033)
  → choose_worker 의 후보 필터
```

**join은 이 값을 싣지 않는다.** join 시점에는 worker.toml이 아직 없어서 워커가 자기
상한을 모르고, 보낼 수 있는 것은 CLI 기본값 추측뿐이다 — 그것은 아래 §이 금지하는
날조와 같은 종류다. join이 쓴 설정으로 기동한 워커가 곧바로 register를 호출하고,
`upsert_worker`의 `ON CONFLICT`가 이 컬럼을 갱신 목록에 포함하므로 NULL은 초 단위로
덮인다.

운영자에게는 세 표면(`GET /v1/workers`, Dashboard `/api/workers`, MCP
`fleet_list_workers`)이 이 값을 그대로 노출한다. 노출하지 않으면 "상한이 없는 Worker"와
"상한을 보고하지 않는 Worker"가 화면에서 구분되지 않는데, 그 구분이 곧 배정 편향의
설명이다.

하트비트가 아닌 이유는 이 값이 관측이 아니라 **설정**이기 때문이다. 관측은 매 주기
바뀔 수 있어 최신값이 의미를 갖지만, 설정은 프로세스 수명 동안 고정이고 바뀌는
계기는 Worker 재시작뿐이다. 재시작은 register를 정확히 1회 부르므로 등록이 그 계기를
정확히 덮는다 — `worker_version`·`liveness_mode`와 같은 취급이다.

### 모르는 상한은 필터하지 않는다

컬럼은 **nullable이고 기본값이 없다**. `max_concurrent`의 `NOT NULL DEFAULT 4`를
베끼지 않은 이유는 그 기본값이 **날조**이기 때문이다 — 실제 상한이 2인 구버전
Worker도 4로 기록되고, 배정은 그 4를 근거로 초과한다. NULL은 "이 Worker의 상한을
모른다"를 뜻하고, 모르는 상한은 필터를 걸지 않는다.

"모르면 배정하지 않는다"를 고르지 않은 것은 그것이 구버전 Worker를 **사용 불가**로
만들기 때문이고, 그래도 되는 근거가 없기 때문이다. 근거는 방향이 반대다:
**오케스트레이터의 상한은 유일한 방어선이 아니라 두 번째**다. 최종 집행자는 Worker의
프로세스 매니저이고, 오케스트레이터 숫자가 없거나 틀려도 최악의 결과는 "초과 spawn"이
아니라 "`cap_reached`로 거절된 관측"이다. 즉 이 필터가 하는 일은 불변식을 혼자
지키는 것이 아니라 **실패를 배정 시점으로 당기는 것**이다.

### `cap_reached`는 죽은 variant가 되지 않는다

배정에 하드 상한이 생기면 "아무도 만들지 않는 enum variant"가 하나 생기는 것 아니냐는
질문이 성립한다. 성립하지 않는다. 두 가지가 남는다:

1. 상한을 모르는 Worker(NULL)에는 필터가 걸리지 않으므로 여전히 초과할 수 있다.
2. 두 카운트가 **다른 것을 센다**. 오케스트레이터는 배정된 `agents` 행을 세고, Worker는
   살아 있는 프로세스를 센다. 배정됐지만 아직 기동하지 않은 Agent, 회수 명령을 받았지만
   아직 종료하지 않은 프로세스가 두 수를 갈라 놓는다.

### 선점은 `workers` 행의 잠금 아래에서 센다 (①-A-2)

`INSERT ... WHERE (SELECT COUNT(*) ...) < cap`은 **원자적이지 않다**. READ COMMITTED
에서 두 트랜잭션이 같은 카운트를 읽고 둘 다 통과한다 — 서브쿼리는 아무 행도 잠그지
않는 전형적인 phantom이다. 카운트 대상(`agents`)에 아직 존재하지 않는 행을 세는
것이므로 대상 자체를 잠글 수도 없다.

그래서 잠그는 것은 세는 대상이 아니라 **상한의 주인**이다. 한 트랜잭션 안에서
`SELECT ... FROM workers WHERE id = $1 FOR UPDATE`로 그 Worker 행을 잠그고, 잠금 아래에서
세고, 통과하면 배정을 쓰고 커밋한다. 같은 Worker를 노리는 두 번째 요청은 첫 번째의
커밋을 기다린 뒤 다시 세므로 방금 들어온 행을 본다. 서로 다른 Worker로 가는 배정은
그대로 병렬이다 — 잠금이 전역이 아니라 Worker별이기 때문이다.

이것이 게이트 ①이 말한 "CAS slot claim"이며, `worker_execution_lease` 없이 성립한다.
①-B도 마찬가지다 — claim의 대상이 명령의 세대로 바뀌어도 필요한 것은 새 테이블이
아니라 같은 문장 안의 술어 하나였다(`EXISTS (SELECT 1 FROM control_plane_lease ...)`).
두 게이트가 같은 처방을 공유하는 이유는 같다: **관측하고 나서 쓰면 그 사이가 창이다.**

READ COMMITTED이라서 `FOR UPDATE`만으로 충분하다는 점이 중요하다. 이 격리 수준에서는
**문 하나하나가 새 스냅샷을 뜨므로**, 잠금을 얻고 나서 다시 센 `COUNT(*)`가 방금
커밋된 승자의 행을 본다. REPEATABLE READ였다면 같은 코드가 직렬화 오류를 올렸을 것이고
재시도 루프가 필요했을 것이다 — 즉 여기서 격리 수준은 배경이 아니라 설계의 전제다.

#### 실패는 하나가 아니라 둘이다

선점 결과를 `bool`로 돌리지 않는다. 실패가 **서로 다른 두 사실**이기 때문이다. 상한에
걸린 것은 지금의 fleet 상태이므로 나중에 다시 하면 되고(409), 존재하지 않는 대상을
지목한 것은 요청의 결함이므로 다시 해도 같다(404/400). `bool`은 그 둘을 한 칸에 뭉개
호출부가 반드시 한쪽으로 오분류하게 만든다. 그래서 `Store::assign_agent_worker`는
`SlotClaim::{Claimed, CapReached, NoSuchAgent, NoSuchWorker}`를 돌린다.

**`NoSuchAgent`가 `NoSuchWorker`보다 우선한다.** Postgres 구현의 UPDATE는 Agent가 없으면
0행을 갱신하고 FK 검사에 닿지도 않으므로, 그 순서가 구현의 사실이다. MemStore도 같은
순서를 흉내 낸다. 이 순서를 지키려고 Agent 존재 확인을 **잠그지 않고** 먼저 읽는다 —
`SELECT ... FOR UPDATE`로 잠그면 `agents → workers` 순서가 생겨 아래 §의 잠금 순서를
깨뜨린다.

#### 잠금 순서는 `workers → agents` 하나뿐이다

두 경로(`create_agent`의 INSERT, `assign_agent_worker`의 UPDATE)가 모두 `workers` 행을
먼저 잠근 뒤 `agents`를 만진다. 순서가 하나뿐이면 순환이 없고, 순환이 없으면 데드락이
없다. MemStore에서는 이것이 권고가 아니라 **강제**다 — Mutex 두 개를 반대 순서로 잡으면
곧바로 서로를 기다리므로, `create_agent`에서 상한을 읽는 코드가 `agents` 잠금 **앞에**
있어야 한다.

#### 생성은 실패하지 않고, 배정만 떨어진다

`create_agent`가 상한에 걸렸을 때 Agent 생성 자체를 실패시키지 않는다. §"구현 상태
(`#67` 4a)"가 정한 대로 `worker_id = NULL`은 정상 상태이고, 재배정 경로
(`POST /api/agents/{id}/place`)가 회복을 맡는다. 그래서 `create_agent`의 반환형이
`Result<Option<WorkerId>, StoreError>`다 — **실제로 기록된 배정**을 돌려주고, 호출부는
로컬 구조체를 그 사실에 되맞춘다.

되맞추지 않으면 응답과 **감사 로그**가 일어나지 않은 배정을 기록한다. 두 생성 핸들러
(Dashboard·MCP)가 응답과 감사 detail을 같은 구조체에서 만들기 때문이다. 응답의 거짓은
다시 조회하면 드러나지만 감사 로그의 거짓은 남는다 — 저장소가 조용히 강등하면 안 되는
이유가 이것이고, 반환형을 넓힌 이유도 이것이다. 배정을 지울 때는
`Agent::without_placement()`로 `worker_id`와 `assigned_at`을 **함께** 지운다.
마이그레이션 `030`의 `agents_placement_complete` CHECK가 "둘 다 있거나 둘 다 없거나"를
요구한다.

#### 재배정은 자기를 세지 않는다

`assign_agent_worker`의 카운트에는 `AND id <> $agent_id`가 붙는다. 이미 그 Worker에 있는
Agent를 같은 Worker로 다시 배정하는 것은 슬롯을 **추가로 쓰지 않으므로**, 자기 제외가
없으면 정확히 가득 찬 Worker에서 그 무해한 no-op이 `CapReached`로 거절된다.
`create_agent`에는 이 제외가 필요 없다 — 그 시점에는 행이 아직 없다.

카운트 조건은 필터와 선점에서 **같아야 한다**(`status <> 'stopped'`). 둘이 슬롯의 정의를
달리 보면 필터를 통과한 Worker가 선점에서 거절되어, 배정이 이유 없이 실패한다.

#### 증명은 잠금 자리마다 따로 필요하다

`create_agent`와 `assign_agent_worker`는 각자 `FOR UPDATE`를 잡는다. 한쪽을 붉힌 것은
다른 쪽에 대해 아무것도 말하지 않으므로 테스트도 둘이다 —
`concurrent_creates_cannot_exceed_the_cap`과 `concurrent_placements_cannot_exceed_the_cap`이
상한 1인 Worker에 8-way 배리어 경합을 걸고, 반환 variant의 분포뿐 아니라 **저장된 행 수**도
함께 단정한다(반환값만 보면 Store가 거짓말을 해도 통과한다).

**동시성 테스트의 풀은 크기를 정하는 것으로 부족하고 미리 채워야 한다.** sqlx의 `connect()`는
커넥션을 하나만 열고 `min_connections`의 기본값은 0이므로, `max_connections`만 올리면 직렬화가
체크아웃에서 **접속**으로 한 겹 내려갈 뿐이다. 실측으로 실효 동시성이 2였고, 그 하네스에서
잠금을 지운 코드는 6회 내내 `left: (2, 6)`을 냈다 — 값이 흔들리지 않는 것은 창이 좁다는 뜻이
아니라 하네스가 묶고 있다는 단서였다. 커넥션 N개를 직접 `acquire()`했다 버리는 `race_pool()`을
쓰면 같은 코드가 `left: (8, 0)`이 된다. 8개 **전부**가 상한 1인 Worker에 앉는다.

**핸들러 쪽 되맞춤은 경합으로 증명할 수 없고, 결과를 직접 세워 증명한다.** 되맞춤
(§"생성은 실패하지 않고, 배정만 떨어진다")은 순차 경로에서 도달할 수 없다 — ①-A-1의 필터와
①-A-2의 선점이 `status <> 'stopped'`라는 같은 술어를 쓰므로 `choose_worker`가 가득 찬 Worker를
지목하지 않는다. 그렇다고 경합으로 닿을 수 있는 것도 아니다: MemStore의 연산 사이에는 `.await`
양보점이 없어 `place_on_create`와 `create_agent`가 한 태스크 안에서 이어 붙는다. 되맞춤을 지운
트리에서 2-way `tokio::join!` 12회, 8-way 배리어 12회 **모두 0건**이 붉어졌다 — 동시성을 올려
넓힐 창이 애초에 없다.

그래서 `MemStore::dropping_placements()`로 결과를 직접 세운다. 후보 Worker를 실어 보내도 배정
없이 저장하는 주입 스위치이며, 기존 `with_failing`으로는 표현할 수 없다 — 선점 실패는 오류가
아니라 `Ok(None)`이고, 생성은 성공한 채 배정만 떨어지기 때문이다. 스위치는 `if n >= cap`이라는
**진짜 판정 뒤에** 놓는다. 앞에 두면 상한 경로가 가려져 상한을 지워도 초록이 된다. 두 테스트
모두 `place_on_create`가 여전히 `Some`을 돌려주는지 먼저 단정해 공허한 통과를 막는다.

| 테스트 | 위치 | 무엇을 증명하는가 |
|---|---|---|
| `concurrent_creates_through_the_api_cannot_exceed_the_cap` | `fleet-dashboard/tests/dashboard_api.rs` | 8-way 경합에서 API 계층의 상한. **되맞춤 분기에는 닿지 못한다**(위 실측) |
| `create_agent_follows_the_store_when_the_slot_claim_drops_the_placement` | 같은 파일 | 선점이 떨어진 결과를 결정적으로 세워, 응답과 **감사 로그**가 저장된 사실을 따르는지 |
| `create_agent_follows_the_store_when_the_slot_claim_drops_the_placement` | `fleet-mcp/src/handlers.rs` 테스트 모듈 | 같은 시나리오의 MCP 표면. **감사 기록 호출이 없으므로 응답만** 본다 |

**두 표면의 위험 등급은 다르다.** 대시보드는 되맞춤 뒤에 `crate::audit::record`를 부르므로
되맞춤이 없으면 일어나지 않은 배정이 감사 로그에 영구히 남는다. `fleet-mcp`의
`handle_create_agent`에는 감사 기록 호출이 없어 거짓이 응답에만 실리고, 응답은 다시 읽으면
드러난다. 코드는 공유하지만 위험은 공유하지 않는다.

**두 증명이 만나는 자리는 실검증뿐이다.** 상한 테스트는 PgStore를 핸들러 없이 때리고
되맞춤 테스트는 핸들러를 MemStore로 때리므로, "Postgres가 잠금 아래에서 `Ok(None)`을
준다 → 핸들러가 되맞춘다 → 감사 로그에 null이 남는다"라는 합성은 트리 안의 어느
테스트도 통째로 밟지 않는다. 살아 있는 서버에 8-way로 `POST /api/agents`를 던져 그
합성을 확인했다 — 배정 성공 1건, **되맞춤 2건**(`placement dropped at slot claim`,
`attempted_worker=Some(...)`), 후보 없음 5건이고 8건 전부 응답·감사 로그·저장된 행이
일치했다. 되맞춤 여부는 응답으로는 구분되지 않고 `fleet::placement` 로그로만 갈린다.

#### 남은 한계: 헛도는 미배정

`choose_worker`는 승자를 **하나만** 돌려준다. 두 요청이 같은 후보를 골랐다가 한쪽이
선점에 실패하면, 다른 Worker가 비어 있어도 그 요청은 미배정으로 끝난다. 불변식은
깨지지 않고 **배정이 놓칠 뿐**이다. 답은 후보를 순위 목록으로 돌려 흘러내리게 하는
것이지만, `POST /api/agents/{id}/place`가 이미 회복 경로이므로 미룬다.

### 만들지 않은 것

| 미룬 것 | 이유 | 귀속 |
| --- | --- | --- |
| ~~`worker_execution_lease` 테이블~~ | 슬롯 상한에 필요 없다는 판단은 그대로이고, ①-B에도 필요 없는 것으로 **확정됐다** — 11필드 중 오늘 채울 주체가 있는 것은 `control_epoch` 하나뿐이었고, 그것은 테이블이 아니라 `agents` 컬럼 하나로 충분하다 | **만들지 않기로 확정 (2026-09-01)** — [권한과 장애 전환](../control-plane-authority-and-failover.md) §범위 정정이 정본 |
| 상한 변경의 하트비트 반영 | 설정값이므로 계기가 재시작뿐이고, 재시작은 register가 덮는다 | 계기가 생기면 |
| 상한 초과 시의 대기열 | 배정 실패는 이미 `worker_id = NULL`이라는 정상 상태로 표현된다(§"구현 상태 (`#67` 4a)"). 대기열은 그 상태를 소비할 재배정 루프를 전제하는데, 4a가 루프를 만들지 않기로 한 근거가 그대로 유효하다 | 재배정 루프가 생기면 |

## 재배정 관측 술어 (`#67` 구현 게이트 ②)

슬롯 상한(①-A)이 세는 것은 **배정된 `agents` 행**이고, 게이트 ②가 보는 것은 **워커가 보고한
프로세스**다. 둘은 축이 다르므로 상한을 아무리 정확히 지켜도 이 창은 닫히지 않는다: 상한 1인
Worker에 앉은 Agent를 다른 Worker로 옮기면 원장 위의 수는 여전히 1이지만, 옛 Worker가 제어면과
단절된 채 프로세스를 계속 돌리고 있으면 실제 프로세스는 2가 된다.

술어는 `assign_agent_worker`의 UPDATE **안**에 있다.

```sql
WHERE id = $1
  AND (worker_id = $2 OR observed_status IS NULL OR observed_status = 'failed')
```

미리 SELECT해서 판정하면 안 되는 이유는 ①-B와 같다 — NULL을 본 뒤 heartbeat이 `running`을 쓰고
그다음 우리 UPDATE가 나가는 순서가 정확히 막으려던 중복이다.

| 갈래 | 뜻 | 왜 안전한가 |
| --- | --- | --- |
| `worker_id = $2` | 같은 Worker로의 재배정 | 프로세스가 움직이지 않으므로 중복이 생길 여지가 없다. 상한 계산이 자기 자신을 빼는 것과 같은 근거다 |
| `observed_status IS NULL` | 한 번도 running을 보고한 적 없음 | 분할 중에는 새 프로세스가 생길 수 없다 — 명령은 heartbeat **응답**으로만 전달된다 |
| `= 'failed'` | 워커가 띄우려다 실패했다고 보고 | 세 사유(`cap_reached`·`no_free_port`·`spawn_failed`) 모두 프로세스가 생기지 않았다는 뜻이다 |
| `= 'running'` | 다른 Worker가 돌리고 있다고 보고 | **거절한다** — `SlotClaim::ObservedRunning`(Dashboard 409, MCP `-32602`) |

`running`을 거절하는 것은 분할 중에도 그 값이 **지워지지 않기** 때문에 성립한다. 관측을 신선도로
무효화하면(예: "N주기 이상 오래된 관측은 없는 것으로 본다") 분할된 Worker의 `running`이 곧바로
무시 가능해져 술어 자체가 무너진다. 그래서 남는 창 — 프로세스 기동과 그것을 보고하는 다음 beat
사이의 한 주기 — 은 여기서 닫지 않는다. 닫는 것은 게이트 ③(process inventory)의 몫이다.

### 회수 가능성은 마이그레이션 036이 산다

술어만 넣으면 이동이 안전해지는 것이 아니라 **없어진다**. 살아 있는 Agent는 술어가 막고, 회수된
Agent는 옛 400 가드가 막아 남는 교집합이 최초 배정뿐이 되기 때문이다. 그래서 036이 두 가지를
함께 한다.

- 트리거가 `worker_id`의 변화에 반응한다. 조건이 **둘이고 서로 다르다.**
  - 세 관측 컬럼은 `worker_id`가 **바뀌면**(`IS DISTINCT FROM`) 비운다. 관측은 "어떤 Worker가 이
    프로세스에 대해 한 말"이지 Agent의 속성이 아니므로, 배치가 옮겨가는 순간 그 말은 새 자리에
    대해 거짓이 된다. 세 컬럼이 **함께** 비워져야 하는 것은 032의 `agents_observation_complete`가
    "셋 다 NULL 또는 셋 다 non-NULL"만 허용하기 때문이다.
  - `assigned_at`은 NULL이 될 때만 비운다. 030의 조건 그대로다. `assign_agent_worker`가 같은
    UPDATE에서 `assigned_at = now()`를 쓰므로, 이쪽까지 "바뀌면"으로 옮기면 방금 찍은 배치 시각을
    트리거가 도로 지운다.
  - 030의 `agents_clear_assigned_at`을 확장하지 않고 이름을 바꾼 것은, 확장하면 그 이름이 거짓이
    되기 때문이다.
- **같은 Worker로의 재배정에서는 관측이 살아남는다.** `IS DISTINCT FROM`이 거짓이 되어 자동으로
  그렇게 되지만, 이것은 우연히 맞는 것이 아니라 게이트 ②가 성립하기 위한 조건이다. 여기서
  지우면 술어의 `worker_id = $2` 갈래가 그대로 게이트를 통과하는 문이 된다 — 같은 Worker로 한 번
  재배정해 관측을 없앤 뒤 아무 데로나 옮기는 2단계 우회다. 양쪽 저장소의
  `same_worker_reassignment_is_allowed_while_running`이 "관측이 남아 있고, 그 뒤에도 다른
  Worker로는 못 간다"까지 단정한다.
- 술어와 트리거는 서로를 방해하지 않는다. UPDATE의 WHERE는 **갱신 전 행**에 대해 평가되고 BEFORE
  ROW 트리거는 그 행이 선택된 **뒤**에 발화하므로, 트리거가 `observed_status`를 NULL로 만드는 것이
  그 행을 통과시킨 술어의 판정을 되돌리지 않는다.
- `agents_observation_requires_placement` CHECK가 "배정 없이 관측 없음"을 DB의 사실로 굳히고,
  백필이 기존 행을 정리한다. 트리거만으로는 **앞으로의** 전이만 옳다.

트리거인 이유는 Worker 삭제가 애플리케이션을 거치지 않고도 일어나기 때문이다 — 운영자의 직접
`DELETE FROM workers`, 또는 다른 인스턴스. `PgStore::delete_worker`에만 두면 그 경로가 stale한
`running`을 남겨 해당 Agent를 영구히 묶는다. 이 결정이 없애려는 바로 그 상태다.

**대가는 정직하게 적는다.** Worker 삭제를 "이 Worker는 없다"는 운영자의 선언으로 취급하는 것이므로,
운영자가 틀렸다면(실은 살아 있고 제어면과만 단절) 중복 실행이 그대로 발생한다. 판단의 주체를
사람으로 옮긴 것이지 위험을 없앤 것이 아니다.

### 두 저장소의 판정 **순서**를 고정한다

PgStore는 상한을 선행 검사로, 관측을 UPDATE 안의 술어로 본다. 그래서 둘 다 실패할 상황의 답은
`CapReached`다. MemStore도 관측 검사를 상한 계산 **뒤에** 두어 같은 답을 낸다. 순서가 갈리면 그
차이는 저장소를 바꿔 끼우는 호출부 테스트에서만, 그것도 어느 쪽을 골랐느냐에 따라서만 드러난다 —
`cap_is_decided_before_the_observation`(양쪽)이 이것을 고정한다.

### 회수된 Agent의 400 가드를 걷었다

두 공개 표면(`POST /api/agents/{id}/place`, `fleet_place_agent`)이 `status = 'stopped'`인 Agent의
배정을 400으로 막고 있었다. 근거는 "원장이 `stopped`를 세지 않으니 아무도 읽지 않는 값을 쓰는
것이 된다"였고, 그 근거는 두 겹으로 무너졌다.

1. 게이트 ② 아래에서 살아 있는 Agent를 옮기는 유일한 안전한 순서가 "회수 → 프로세스가 죽고
   관측이 비워짐 → 이동 → 재기동"인데, 그 두 번째 걸음에서 `status`가 `stopped`다.
2. 배정된 값은 실제로 읽힌다. `list_agent_commands`의 술어는
   `worker_id = $1 AND (status <> 'stopped' OR last_acked_generation < command_generation)`이라
   미확인인 동안 `stopped` 행도 싣는다(§"회수 명령은 확인될 때까지 실린다").

상한을 우회하지도 않는다. `stopped`는 원장에 세지 않으므로 배정 시점의 계산을 지나가지만, 실제
기동은 Worker가 자기 상한으로 막고 `failed`/`cap_reached` 관측으로 되돌려 준다 — 진짜 경계는
그쪽이고, 4c-B가 두 수를 나눠 둔 이유가 여기서 쓰인다.

**이것은 계약 변경이다** — 전에 400으로 거절하던 요청이 이제 성공한다. 표면 계약의 정본은
[Dashboard API](../../contracts/dashboard-api.md)와 [Agent 관리](../../contracts/agent-management.md)다.
