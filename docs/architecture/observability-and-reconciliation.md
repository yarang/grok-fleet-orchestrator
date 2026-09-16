---
type: architecture-decision
authority: canonical
implementation: partial
verification: code-checked
source: "docs/architecture/observability-and-reconciliation.md"
last_verified: "2026-09-05"
last_verified_commit: "working-tree"
owners: ["operations", "scheduler", "security"]
---

# 관측성·재조정·장애 복구 계약

## 목적

Fleet는 단순 health check가 아니라 **원하는 상태와 관측된 상태의 차이**를 운영자가 판별하고
안전하게 수렴시켜야 한다. 이 문서는 metric·audit 상관관계, reconciliation의 자동 범위, 운영자
개입 경계를 소유한다. Active/Cold Standby 권한은 [Control Plane 권한과 장애 전환](control-plane-authority-and-failover.md),
Task effect의 결과 판정은 [실행 일관성](tasks/execution-consistency.md)이 정본이다.

```mermaid
flowchart LR
    Desired["Desired state\nProject policy · Task · lease"] --> Reconcile["Reconciler\ncompare / classify"]
    Observed["Observed state\nWorker inventory · receipt · grant"] --> Reconcile
    Reconcile --> Safe["safe automatic convergence"]
    Reconcile --> Unknown["OutcomeUnknown / Quarantined"]
    Unknown --> Operator["operator evidence + approval"]
    Safe --> Audit["audit + metrics"]
    Operator --> Audit
```

## 상태 분류

모든 제어 대상은 `Desired`, `Observed`, `ReconciliationResult`를 분리해 기록한다. health가
`green`이어도 lease, effect, credential 상태가 불명확하면 Fleet 전체를 정상으로 표시하지 않는다.

| 대상 | 최소 desired/observed 증거 | 불일치 결과 |
|---|---|---|
| Control plane | active instance, epoch, DB lease | `ControlPlaneFenced` |
| Worker | incarnation, liveness mode, control channel, process inventory 시각 | `WorkerUnreachable` 또는 `WorkerUnchecked` |
| Agent/lease | Agent generation, fencing token, expected container/process ID | `AgentOrphaned` 또는 `OutcomeUnknown` |
| Task 실행 | 상태, deadline, Worker ACK, checkpoint | `OutcomeUnknown` 또는 `CancelUnconfirmed` |
| Tool effect | ledger 상태, provider receipt/조회 결과 | `PartiallyApplied` |
| Credential delivery | grant ID, expiry, Worker/Task binding, revocation | `GrantLeakSuspected` 또는 `CredentialUnavailable` |
| Project archive | terminal Task, process/lease/grant cleanup, open holds | `ArchiveBlocked` |

`Unknown`, `Unchecked`, `CancelUnconfirmed`, `PartiallyApplied`, `ArchiveBlocked`, `Fenced`는 오류를 숨기는 중간 상태가
아니다. UI·API·alert는 원인, 마지막 관측 시각, 다음 자동 재평가 시각, 필요한 운영자 action을 함께
보여야 한다.

## Metric·event·audit 규칙

Prometheus metric은 집계와 alert용이고, audit/event log는 개별 원인 추적용이다. 둘을 서로 대신하지
않는다.

| 신호 | 필수 측정/기록 | 금지 |
|---|---|---|
| 제어 권한 | active epoch, lease renewal 실패, fencing 거절 수 | instance ID를 무제한 label로 사용 |
| Worker/Agent | mode별 관측 age, slot 사용량, warm eviction, orphan/unknown 수 | worker/agent/task UUID label |
| Task 실행 | queue age, dispatch/ACK latency, terminal/outcome-unknown 수 | prompt, repository URL, 사용자 입력 |
| effect | class별 `Started`/`Unknown`/compensation 실패 수 | provider 요청/응답 원문, idempotency key |
| Security | grant 발급/거절/만료, revoke 지연, helper 거절 | credential ID, secret fingerprint, token |
| Archive | drain age, open hold 수와 kind, blocked age | Project name/ID label |

모든 audit/event에는 가능한 범위에서 `request_id`, `project_id`, `task_id`, `agent_id`,
`worker_id`, `lease_generation`, `fencing_token`, `control_epoch`, actor를 상관관계 필드로 남긴다.
이 필드는 조회용 structured record에만 두며 metric label·로그 메시지 본문에 secret, prompt, credential
원문, raw provider payload를 넣지 않는다.

## Reconciler의 자동 권한

Reconciler는 Active Orchestrator epoch에서만 동작한다. 한 sweep은 스냅샷 기준으로 판단하고,
각 수정은 fencing token·generation·현재 상태를 조건으로 하는 CAS여야 한다.

| 상황 | 자동 동작 | 자동으로 하지 않는 일 |
|---|---|---|
| 만료된 delivery grant | grant revoke·WarmIdle drain | credential 재발급/다른 Project grant |
| token 불일치 orphan process | Worker self-fence/cleanup 요청, lease quarantine | 같은 Agent의 즉시 재시작 |
| Start/stop ACK 유실 | `OutcomeUnknown`, inventory 조회 | 중복 start/재실행 |
| Worker 미도달 | 신규 dispatch 차단, lease expiry 관찰 | 외부 effect 재시도/성공 추정 |
| terminal checkpoint 누락 | Task 성공 확정 차단, evidence 요청 | 임의 Git force push |
| effect `Started`/`Unknown` | provider 조회, `PartiallyApplied` 승격 | 자동 redrive/보상 추정 |
| `CancelUnconfirmed` Task | inventory·effect ledger 조회, `Cancelled`/`PartiallyApplied`/`OutcomeUnknown` 해소 | 증거 없는 `Cancelled` 확정 |
| archive cleanup 미완료 | `ArchiveBlocked` hold 생성 | context/Git/audit 삭제 |

자동 reconcile은 durable data 삭제, Project reopen, irreversible tool 실행, external effect redrive,
risk acceptance, security/legal hold 해제, master-key/credential rotation을 수행하지 않는다.

## 재시작·장애 복구 순서

Primary 재시작 또는 수동 승격 뒤에는 신규 dispatch보다 관측이 우선이다.

1. DB control lease와 epoch를 획득하고 이전 owner가 fenced됐음을 확인한다.
2. Worker의 incarnation·control channel·process inventory를 수집한다. `on_demand` Worker는
   `Unchecked`로 표시하고 probe 전 정상으로 간주하지 않는다.
3. 활성 Agent lease와 process inventory, delivery grant, Task fencing token을 대조한다.
4. 불일치는 `OutcomeUnknown` 또는 quarantine으로 보존하고, safe cleanup만 수행한다.
5. effect ledger의 `Started`/`Unknown`, archive hold, checkpoint 누락을 먼저 재평가한다.
6. 이 과정이 끝난 Project/Worker에 대해서만 Pending Task dispatch를 재개한다.

recovery snapshot은 control epoch, binary/schema compatibility, 정책 revision, lease/task/effect
요약, 마지막 관측 시각을 담되 secret 원문·prompt·provider payload는 포함하지 않는다.

## Alert와 운영자 action

임계값은 deployment 정책으로 설정하되 아래 조건은 alert를 피할 수 없다.

| 조건 | 기본 severity | 운영자 최초 action |
|---|---|---|
| control lease renewal 실패 또는 둘 이상의 owner 관측 | Critical | gateway 차단, fencing/epoch 증거 확인 |
| `OutcomeUnknown` Task 또는 orphan Agent | High | Worker inventory와 effect ledger 확인, 재시작 금지 |
| credential grant revoke 지연/누수 의심 | High | Worker isolate, grant revoke 증거 확인 |
| `PartiallyApplied` effect | High | provider receipt·보상·risk acceptance 결정 |
| `ArchiveBlocked` SLA 초과 | Medium | hold owner/evidence 확인 |
| WarmIdle slot 과점유/TTL 반복 초과 | Medium | eviction 정책·capacity 확인 |
| on-demand Worker probe 반복 실패 | Medium | Worker를 `Unchecked`/Unavailable로 유지 |

운영자는 recovery action마다 incident/request ID, 관측 근거, 선택한 조치, 승인자, 결과를 audit에
남긴다. 단일 alert 해제는 안전한 회복 증거가 아니며, 해당 reconciliation result가 `Converged`가 된
것을 확인해야 한다.

## 구현 게이트

1. metric에 고카디널리티 ID·prompt·secret이 노출되지 않는 시험
2. control failover 뒤 inventory-first recovery가 신규 dispatch보다 먼저 수행되는 E2E 시험
3. ACK 유실·orphan process·grant expiry가 자동 중복 실행 없이 quarantine/cleanup되는 시험
4. `Started` effect와 archive hold가 운영자 승인 없이 자동 redrive/archive되지 않는 시험
5. on-demand Worker가 `Unchecked`에서 probe 성공 전 dispatch되지 않는 시험
6. audit 상관관계 필드만으로 incident의 Project·Task·lease·effect 경로를 재구성하는 시험
7. `CancelUnconfirmed` Task가 증거 기반으로 해소되기 전까지 Project archive가 진행되지 않는 시험

## 구현 상태 (2026-09-06)

게이트별 현황이다. **닫힌 것은 1과 5 둘, 부분은 2와 3 둘이며, 남은 셋은 4·6·7이다**
(2026-09-16에 2가 차단에서 부분으로 옮겨졌다 — 아래 표와 그 아래 절 참고). 다만 그
넷의 막힘이 전부 같은 종류는 아니다 — 게이트 4와 7은 하부 구조(effect ledger의 상태 기계,
`CancelUnconfirmed`)가 저장소에 **없어서** 시험을 작성할 수조차 없다. 게이트 2는 워커가 명령과
무관하게 들고 있는 것을 오케스트레이터가 **묻는** process inventory를 기다린다. 게이트 6은
그 둘과 또 달라서, 필드가 아니라 **제어면 결정의 감사 경로**가 선행인데 그 선행은
2026-09-06에 들어왔다(아래 행 참고).

**2026-09-12 정정 — 이 문단이 병합 직후 두 군데에서 표와 어긋나 있었다.** (1) "게이트 2와 3은
같은 하나를 기다린다"고 적었으나, 게이트 3이 기다리던 inventory 절반은 orphan 자기 보고로
채워졌다(3행 참고). 지금 process inventory를 기다리는 것은 **게이트 2뿐**이고, 3의 남은 절반은
ACK 유실과 grant expiry다. (2) 게이트 6에 "남은 것은 `lease_generation`·`fencing_token`이다"라고
적었으나 **그 둘은 만들 대상이 아니다** — [제어면 권한과 failover](control-plane-authority-and-failover.md)의
2026-09-01 처분표가 `agents.command_generation`으로 **충족 처리**했다(031이 이미 DB가 발행하는
Agent별 단조 증가 값을 만들었고, 이름만 다르고 역할이 같다). 즉 그 둘의 grep 0건은 차단 근거가
아니며, `da523a6`이 이미 한 번 걷어낸 폐기 사유가 되살아난 것이다. 게이트 6에 남은 것은
**effect 경로 하나**이고, 그 선행은 게이트 4다.

없는 것을 미리 만들지 않기 위해, 그리고 이미 들어온 것을 다시 만들지 않기 위해, 무엇이 어느
쪽인지를 여기에 명시한다.

**차단 사유는 매번 다시 확인한다.** 2026-09-06 하루에 셋(4·5·6)을 재판정했고 셋 다
부정확했는데 틀린 방향이 매번 달랐다 — 4는 순환이었고(뚫렸다), 5는 유효하되 함정이 숨어
있었으며, 6은 낡았지만 **유리한 방향이 아니었다**(없다던 필드가 있었고, 그럼에도 게이트는
열리지 않았다). 사유를 그대로 믿었다면 6에서 "필드만 더하면 된다"는 잘못된 걸음으로 갔을
것이다.

게이트 3이 2026-09-02에 차단에서 부분으로 옮겨졌고, **그 이동 자체가 기록할 값어치가 있다.**
그때까지의 판정은 "비교할 process inventory가 없어 orphan을 지목할 수 없다"였는데, 그 문장은
inventory를 **오케스트레이터가 들고 대조하는 목록**으로 읽고 있었다. 실제로 필요했던 것은
Worker 자신이 이미 쥐고 있던 두 근거였다 — 명령 목록에서의 부재와, 이전 incarnation이 디스크에
남긴 기록이다. 즉 막고 있던 것은 없는 하부 구조가 아니라 **어디를 보아야 하는지에 대한 오해**
였다. 차단으로 적힌 나머지 넷도 같은 종류의 오해를 품고 있을 수 있으므로, 선행이 도착하기를
기다리기 전에 그 판정의 근거를 한 번 더 읽는 편이 낫다.

| 게이트 | 상태 | 근거 / 막고 있는 것 |
| --- | --- | --- |
| 1. metric 노출 금지 | **닫힘** | `crates/fleet-api/src/metrics.rs`의 `metrics_body_never_exposes_ids_prompts_or_secrets`(fixture의 UUID·prompt·리포지터리 URL·`?server-key=` secret이 본문에 없음)와 `metrics_body_labels_stay_within_a_bounded_allow_list`(라벨 이름·값이 유한 허용 목록 안) |
| 2. inventory-first recovery E2E | **부분** | **Reconciler는 있고 fencing도 있다** — `crates/fleet-scheduler/src/reconcile.rs`의 `Reconciler`는 `lease_allows_control()`로 lease를 잃은 인스턴스의 sweep 전체를 건너뛴다(시험 `reconcile_once_skips_the_whole_sweep_when_control_plane_lease_is_fenced`). 선행으로 적혀 있던 `#63`의 lease/epoch primitive도 1~4단계로 들어왔다. 없는 것은 **inventory-first라는 순서**다: 이 Reconciler는 `#62`의 stale `Pending`/`Dispatched` sweeper라 **저장소가 기억하는 작업만** 훑고, failover 뒤 워커에게 "무엇을 들고 있는가"를 먼저 묻지 않는다. 그 물음의 답이 게이트 3이 지목하는 inventory와 같은 것이므로, 선행은 `#63`이 아니라 **게이트 3**이다. (**2026-09-12 정정 — 이 마지막 문장이 틀렸다.** 두 inventory는 **축이 다르다**: 게이트 3이 지목하는 것은 **Agent 프로세스 축**으로 `fleet-worker`가 자기가 띄운 것을 heartbeat의 `agent_orphans`로 보고하는 것이고, 게이트 2가 필요로 하는 것은 **task 실행 축**으로 오케스트레이터가 grok의 ACP 세션에 직접 붙어 묻는 것이다(오케스트레이터는 워커의 `endpoint`에 직접 연결하며 `fleet-worker` 프로세스는 task 세션을 보지 못한다). 상대도 채널도 다르므로 **게이트 3을 끝까지 닫아도 게이트 2는 열려 있다.** 게이트 2의 실제 선행은 **task 실행 신원의 내구화**다 — ACP `session_id`는 `fleet-transport`의 인메모리 맵에만 있고 `fleet-store`·`fleet-core`에 grep 0건이라, 재시작하면 "내가 띄운 세션"을 가리킬 이름 자체가 사라진다. `session/list` 왕복은 이미 `probe`가 하고 있으나 응답 본문을 버린다 — 인벤토리의 절반이 이미 도착하고 있는데 붙일 왼쪽이 없다.) **2026-09-16 — "grok의 지원 여부가 실측되지 않았다"는 사유가 틀렸다.** 미지가 아니라 **읽지 않은 것**이었다: ACP는 `initialize` 응답의 `agentCapabilities.sessionCapabilities.list`로 `session/list` 지원을 **선언**하고, 그 응답은 모든 연결에서 이미 도착하고 있었다. `acp_transport.rs`가 그것을 `if let Err(e)`로 실패만 보고 `Ok`의 본문을 통째로 버리고 있었다 — 위 문단이 지목한 "응답 본문을 버린다"(probe)와 **같은 결함이 두 겹**이었던 셈이다. 이제 handshake가 그 선언을 세션에 붙잡고, `WorkerTransport::list_sessions`가 세 값을 가른다: `Reported(Vec<String>)`·`Undeclared`(광고하지 않음 — **요청을 보내지도 않는다**)·`Refused`(광고했는데 거절했거나 페이지가 잘렸다). 셋을 하나로 뭉개면 "메서드를 모른다"와 "세션이 하나도 없다"가 같은 빈 결과로 도착한다. **왼쪽은 `041`의 `tasks.acp_session_id`다.** Reconciler의 `Some(_) => continue` 분기가 이 게이트가 지목하던 구멍이었고(워커는 멀쩡한데 그 위의 **실행**이 사라진 경우를 어느 분기도 보지 않았다), 이제 그 자리에서 워커에게 묻고 목록에 없는 세션의 Task를 `Failed(ExecutionVanished)`로 회수한다. 새 `FailureKind`인 이유는 `ResultLost`와 **재제출 안전성이 다르기** 때문이다 — 저쪽은 "아직 돌고 있을 수도 있다"라 재제출이 이중 실행을 낳을 수 있고, 이쪽은 워커가 지금 들고 있는 것 전부를 답한 뒤의 부재라 실행이 끝났다는 것이 확정이다. **여전히 부분인 이유**: 이 경로는 `Reported`에서만 발동하고, 오늘 실제 Agent가 이 capability를 광고하는지는 **아직 관측되지 않았다**(광고하지 않는 배포에서는 회수가 한 건도 일어나지 않고 시험이 그것을 부정 단정으로 고정한다). 그리고 이 게이트가 요구하는 "failover 뒤 inventory-first가 **신규 dispatch보다 먼저**"라는 **순서**는 아직 없다 — 지금 조회는 재조정 sweep 안에서 워커당 한 번 일어날 뿐 dispatch 경로를 막지 않는다. |
| 3. ACK 유실·orphan·grant expiry quarantine | **부분** | start/stop ACK는 `#67` 4b에서 왔고 `worker_incarnation`은 `workers.incarnation_started_at`(028)으로 있다. `#67` 4c-B가 관측 채널까지 넣었지만 **그것으로 orphan을 지목할 수는 없다** — `crates/fleet-worker/src/agent_process.rs`의 재조정 루프는 관측을 `Vec::with_capacity(commands.len())`에 담고 `commands.iter().filter(desired_status == Running)`만 순회하므로, 오케스트레이터가 **이미 배치했다고 믿는** Agent를 확인·부인할 뿐 명령한 적 없는 프로세스를 실어 나를 형식이 없다. 같은 루프 2단계는 목록에서 사라진 프로세스를 **보고 없이** 종료하고, `procs`는 in-memory라 워커가 SIGKILL되면 워커 자신도 자기 자식을 잃는다. 남은 것은 워커가 **명령과 무관하게** 들고 있는 것을 여는 inventory 보고이며, 게이트 2도 같은 것을 기다린다. (**2026-09-12 갱신**: 이 문장이 지목한 inventory는 아래 orphan 자기 보고로 **절반이 채워졌다**. 남은 전수 조회는 이제 게이트 2 단독의 선행이고, 이 칸의 남은 절반은 문장 끝이 적듯 ACK 유실과 grant expiry다 — 한 칸이 서로 다른 두 절반을 지목하고 있었다.) lease quarantine은 요구에서 빠진다: `worker_execution_lease`를 만들지 않기로 확정했다(2026-09-01, `#67` 게이트 ①-B) **2026-09-06 — 이 행의 근거는 `claude/pam-phase2-task5` 병합 전 트리에서 재도출된 것이라, 아래 사실이 그 뒤에 들어왔다.** orphan 쪽은 닫혔다 — 워커가 배정받지 않은 Agent 프로세스를 종료한 사실을 heartbeat의 `agent_orphans`로 올리고 오케스트레이터가 `agent.orphan_terminated`로 감사한다. 근거가 둘이고 서로 다른 실패를 덮는다: 명령 목록에서의 부재(`unplaced`)와 이전 incarnation이 남긴 디스크 기록(`stale_incarnation`, `.fleet-agent.json` + 시작 시각 대조로 pid 재사용을 거른다). **위 문단이 "형식이 없다"고 적은 그 형식이 이것이다.** 다만 그것이 곧 process inventory는 아니다 — 이 보고는 워커가 **자기가 띄운 것**에 대해 말하는 것이고, 게이트 2가 기다리는 것은 워커가 명령과 무관하게 들고 있는 것 전부를 여는 조회다. 남은 절반은 ACK 유실과 grant expiry다 |
| 4. `Started` effect·archive hold 자동 redrive 금지 | 차단 (2026-09-14 **의도적 보류** — 증명할 비가역 부작용이 아직 없다. 아래 절 참고) | effect ledger가 코드에 존재하지 않는다(`EffectLedger`/`PartiallyApplied` grep 0건). archive hold 테이블은 `#91` **2026-09-06 — 이 행의 근거는 `claude/pam-phase2-task5` 병합 전 트리에서 재도출된 것이라, 아래 사실이 그 뒤에 들어왔다.** 원장은 여전히 없지만 **그것이 읽을 증거는 이제 남는다.** 차단 사유가 순환이었다는 것이 드러났고("원장이 없어서 원장을 못 만든다") 진짜 막힌 자리는 더 앞이었다 — 도구는 Worker의 grok 프로세스 안에서 돌고 오케스트레이터는 `session/prompt`만 보내는데, ACP가 `session/update`로 주는 `ToolCall`/`ToolCallUpdate`를 `acp_transport.rs`가 `_ => None`으로 전부 버리고 있었다. 그 알림을 `WorkerEvent::ToolCall` → `FleetEvent::TaskToolCall`로 올린다(`fleet_core::ToolInvocation`). 여덟 상태·idempotency key·external receipt는 **하나도** 만들지 않았으므로 게이트는 차단이다. 남기는 것은 `tool_call_id`·`name`·`kind`·`status` 넷뿐 — `title`·`raw_input`·`raw_output`·`content`·`locations`는 위 금지 목록에 걸려 버린다. `status = Completed`는 **effect 증거가 아니다**([실행 일관성](tasks/execution-consistency.md)이 적은 대로 모델의 "완료" 서술이지 provider receipt가 아니다) |
| 5. on-demand Worker probe 전 dispatch 금지 | **닫힘** | 안전한 절반(확인 없는 dispatch 금지)은 원래 `WorkerSelector::select` 1.5단계가 `on_demand` 워커를 후보에서 통째로 제외해 닫혀 있었고, **2026-09-06에 그 제외가 probe로 대체됐다** — 1.5단계는 이제 liveness를 거르지 않고(`selector.rs`의 "liveness는 여기서 **거르지 않는다**" 주석), 확인은 결승전인 8단계 `responds()`가 승자에게만 건다(probe는 왕복이라 후보를 좁히는 필터로 두면 뒤 필터에서 어차피 떨어질 워커까지 전부 왕복시킨다). `on_demand`·probe 시험 6건 (2026-09-12 정정: 이 문장은 병합 전 트리의 서술이라 "1.5단계가 제외한다"와 "시험 4건" 둘 다 지금 트리에서 거짓이었다). 나머지 절반인 **probe 성공 후 dispatch 허용**의 막힘은 수단이 아니라 배선이다: `WorkerTransport::ping(worker_id) -> Duration`이 이미 트레이트에 있고, 없는 것은 selector가 dispatch 직전에 그것을 부르고 결과를 후보 판정에 되먹이는 경로다. **소유는 이 게이트(`#70`)이지 `#67`이 아니다** — probe는 Agent 프로비저닝이 아니라 Worker liveness이고 수단인 `ping`도 `fleet-transport`에 있다(2026-09-06 귀속 정정: 이 표와 `selector.rs`·`health.rs` 주석은 `#67`을, `#67` 로드맵 행과 `placement.rs`는 `#70`을 지목해 **서로에게 미루고 있었다**). `Unchecked` 워커 상태는 만들지 않았다 — probe 없이는 빠져나올 수 없는 도달 불가 상태가 되기 때문 **2026-09-06 — 이 행의 근거는 `claude/pam-phase2-task5` 병합 전 트리에서 재도출된 것이라, 아래 사실이 그 뒤에 들어왔다.** **게이트를 닫았다.** `WorkerTransport::probe`가 `session/list`를 실제로 왕복시키고(부작용 없는 유일한 client→agent 요청이다), `WorkerSelector`가 고른 워커의 `liveness_mode`가 `on_demand`이면 dispatch 직전에 그것을 건다. **판정은 "답이 왔는가"이지 "성공했는가"가 아니다** — grok이 그 메서드를 몰라 `-32601`을 줘도 살아 있다는 증거로는 같다. 구현에서 가장 틀리기 쉬운 자리는 Agent의 거절과 연결의 죽음이 SDK에서 같은 `Err`로 온다는 것이고 실제로 첫 구현이 거기서 틀렸다(시험이 잡았다); 연결 상태(구조적)와 SDK의 문구 표식(텍스트)을 AND로 묶어 갈랐다. probe는 필터가 아니라 **결승전**에서 돈다 — 왕복이라, 값싼 필터가 좁힌 뒤 승자에게만 건다. 신선도는 어디에도 들지 않는다(저장하면 "얼마나 오래 유효한가"라는 두 번째 파라미터가 생긴다). `placement.rs`는 따라가지 않는다 — 그쪽 제외의 근거는 liveness가 아니라 `#61`의 모드 계약이다 |
| 6. audit 상관관계 필드로 경로 재구성 | 차단 | **일부는 들어왔다** — `project_id`는 `037`로 `audit_log`의 1급 컬럼이 되어 `AuditFilter` 술어와 인덱스를 갖고(`#95` 1단계), Agent 배정·회수의 `generation`은 `detail`에 실린다(`#67`, provisioning.md 「구현 게이트」 4). 없는 것은 **effect 경로**다: effect ledger가 없으므로(게이트 4) 감사는 "누가 언제 무엇을 시켰는가"까지만 말하고 "그 시킴이 어디까지 적용됐는가"를 말하지 못한다. `lease_generation`·`fencing_token`은 요구에서 빠진다 — `worker_execution_lease`를 만들지 않기로 확정했다(2026-09-01). `control_epoch`는 `026`·`035`로 이미 저장소에 있으나, 감사 필드로 승격할지는 effect 경로가 생긴 뒤에 판단한다 **2026-09-06 — 이 행의 근거는 `claude/pam-phase2-task5` 병합 전 트리에서 재도출된 것이라, 아래 사실이 그 뒤에 들어왔다.** **막힌 자리가 필드가 아니라는 것이 드러났고, 그 선행이 들어왔다.** 실측: 감사 emitter 35군데가 전부 `fleet-api`·`fleet-dashboard`·`fleet-core`이고 `fleet-scheduler`는 **0건**이라 dispatch·펜싱 같은 제어면 결정이 감사에 아무것도 남기지 않았다. 컬럼만 먼저 만들었다면 모든 emitter에서 항상 NULL이었을 것이다. `FleetState::audit_control`이 그 경로를 열어 셋을 남긴다: `control.dispatch_refused`(리스가 없어 스스로 물러섬), `control.write_fenced`(리스가 있다고 믿고 쓰러 갔다가 저장소가 세대 불일치로 막음 — **"둘 이상의 owner"의 직접 증거이며 `control_epoch`가 실제로 채워지는 유일한 경우다**), `control.outcome_abandoned`(리스를 잃어 워커 결과의 제어 처리를 건너뜀). 마이그레이션 040이 `audit_log.control_epoch`를 더한다; NULL이 두 가지 정상을 뜻하므로(운영자 행위, 세대를 갖지 못한 채 내린 거절) nullable이다. 행위자는 `cluster_id`가 아니라 `instance_id`다 — 같은 cluster의 두 인스턴스는 구분되지 않는다. **게이트는 여전히 차단이다 — 다만 남은 것은 필드가 아니라 `effect` 경로 하나다**. `lease_generation`·`fencing_token`의 grep 0건은 차단 근거가 **아니다**: [제어면 권한과 failover](control-plane-authority-and-failover.md)의 2026-09-01 처분표가 그 둘을 `agents.command_generation`으로 **충족 처리**했다(031이 이미 DB가 발행하는 Agent별 단조 증가 값을 만들었고 이름만 다르다). 이 칸이 요구하는 `Project·Task·lease·effect` 네 경로 중 Project(`037`)·Task·lease(`040`과 `control.*` 감사 셋)는 필드와 실제 Postgres 왕복 시험(`crates/fleet-store/tests/audit_integration.rs`의 `a_control_epoch_survives_the_round_trip`)까지 갖췄다. 없는 것은 effect ledger뿐이고, 따라서 이 칸의 선행은 이제 **게이트 4 단 하나**다 (2026-09-12 정정: 이 자리에 있던 `lease_generation`·`fencing_token` 사유는 `da523a6`이 이미 걷어낸 폐기 사유가 되살아난 것이었다) |
| 7. `CancelUnconfirmed` 전 archive 차단 | 차단 | `CancelUnconfirmed` 상태가 코드에 존재하지 않는다(grep 0건). 선행 `#67`·`#91` |

### 실행 신원의 내구화 (2026-09-13) — 게이트 2·4·6·7의 공통 선행

차단된 네 게이트가 전부 같은 하나를 기다리고 있었다: **이 Task의 실행을 가리킬
이름**. ACP `session_id`는 `fleet-transport`의 인메모리 맵에만 있었고
`fleet-store`·`fleet-core`에 grep 0건이라, 오케스트레이터가 재시작하면 자기가
무엇을 띄웠는지 지목할 수단이 사라졌다.

**별도 `task_attempts` 테이블을 만들지 않았다.** 2026-08-26의
[Attempt 흡수 판정](project-task-agent-lifecycle.md#attempt-흡수-판정)(로드맵 `#97`)이
`TaskAttempt`를 `Task`에 흡수시켰고, 그 판정은 문서가 아니라 **코드에서 참**이다 —
`Task::allowed_predecessors(Pending)`이 빈 배열이라 `Dispatched → Pending`이 없고,
한 번 dispatch된 Task는 영원히 한 번만 dispatch된다. 재시도는
`NoWorker`/`CircuitOpen`에만 걸려 Task를 `Pending`에 남기므로 실행을 둘로 만들지
않는다. 따라서 `task_id`가 곧 실행 신원이고, 세션은 그 행에 1:1로 얹힌다.

마이그레이션 041이 `tasks.acp_session_id`를 더한다(nullable — 세션이 열리기 전,
열리지 못한 채 실패한 Task, 이 컬럼 이전의 모든 행은 세션이 **없는** 것이지 "빈
세션"이 아니다). 쓰기는 `Store::record_task_acp_session`이고 술어가 둘이다:
`acp_session_id IS NULL`과 fence. 전자는 낙관적 최적화가 아니라 **흡수 판정의
불변식을 DB에서 강제**하는 것이다 — 서로 다른 두 세션 id가 같은 행에 도착하면
그것은 덮어쓸 일이 아니라 위반이고, 덮어쓰기를 허용하면 먼저 열린 세션이
추적 불가능한 고아가 된다. 후자는 세션을 연 인스턴스가 이미 제어권을 잃었다면
그 기록도 그 인스턴스의 것이 아니기 때문이다. 둘 다 `UPDATE`와 같은 문장 안에 있다.

경로는 `WorkerEvent::SessionOpened`다. `session/new` 성공 직후 — 세션을 맵에 넣는
그 줄의 옆에서 — 발행되고, dispatcher가 fence를 걸어 기록한다. `Output`/`ToolCall`을
lease와 무관하게 흘려보내는 것과 갈리는 자리인데, **그 둘은 관측이고 이것은
신원**이기 때문이다: 관측은 틀려도 나중에 버리면 되지만 신원이 틀리면 재시작 뒤의
cleanup이 남의 세션을 지목한다.

**창을 닫지 않고 좁혔다.** `session_id`는 `Pending → Dispatched` CAS보다 늦게 생기므로
같은 문장에 실을 수 없다. 그 사이에 크래시하면 세션은 열렸는데 이름이 없는 창이
그대로 있고, 남은 폭은 `session/new` 왕복이다. 이것을 "닫았다"고 적으면 이 저장소가
반복해 온 바로 그 오류다.

시험은 `crates/fleet-store/tests/task_cas.rs`의 셋이며 `both_backends!`로 `MemStore`와
실제 Postgres 양쪽에서 돈다. 낡은 fence의 거절만 보면 "항상 거절한다"는 구현으로도
통과하므로 **살아 있는 fence로 같은 기록이 통과하는 대조**를 함께 뒀다. 전송 계층
쪽은 `acp_transport_integration.rs`가 실제 `session/new` 왕복에서 `SessionOpened`가
나오는지 단정한다 — 그 단정이 없으면 이벤트가 발행되지 않아도 그 시험은 통과한다.

**아직 소비자가 없다**: cancel의 재시작 후 폴백과 Reconciler의 inventory 대조가
다음이다. 게이트 2의 오른쪽 절반(`session/list` 응답)은 여전히 grok의 지원 여부에
달려 있고 그것은 아직 실측되지 않았다.

**2026-09-16 정정 — 마지막 문장이 틀렸다.** 지원 여부는 실측되지 않은 것이 아니라
`initialize` 응답에 **이미 들어 있었고 읽히지 않고 있었다**(아래
[인벤토리는 미지가 아니라 읽지 않은 답이었다](#인벤토리는-미지가-아니라-읽지-않은-답이었다-2026-09-16--게이트-2의-절반)).
그리고 이 절이 기다린다고 적은 두 소비자는 둘 다 도착했다 — cancel 폴백이 2026-09-14,
Reconciler의 inventory 대조가 2026-09-16이다.

### 인벤토리는 미지가 아니라 읽지 않은 답이었다 (2026-09-16) — 게이트 2의 절반

이 문서는 게이트 2와 7의 남은 절반에 대해 같은 문장을 두 번 적었다: "`session/list`는
여전히 grok의 지원 여부에 달려 있고 그것은 아직 실측되지 않았다." **그 사유가 틀렸다.**

ACP는 `initialize` 응답의 `agentCapabilities.sessionCapabilities.list`로 이 메서드의
지원을 **선언**한다. 스펙이 "광고하지 않으면 지원하지 않는다"로 정의하므로 이것은
추측이 아니라 저쪽의 단언이고, 그 응답은 **모든 연결에서 이미 도착하고 있었다.**
`acp_transport.rs`가 `if let Err(e) = init`로 실패만 보고 `Ok`의 본문을 통째로
흘려보내고 있었을 뿐이다.

**같은 결함이 두 겹이었다.** 위 게이트 5 행이 적은 대로 `probe`도 `session/list`를
왕복시키면서 응답 본문을 버린다 — 저쪽은 "답이 왔는가"만 묻기 때문에 그것이 옳은
처분이다. 그런데 그 옆에서 handshake도 본문을 버리고 있었고, 그래서 "이 Agent가
무엇을 할 수 있는가"라는 답이 두 번 도착했다가 두 번 버려졌다.

#### 세 값을 가른다

`WorkerTransport::list_sessions`의 반환형은 `SessionInventory`이고 값이 셋이다.

| 값 | 뜻 | 판정에 쓸 수 있는가 |
| --- | --- | --- |
| `Reported(Vec<String>)` | 광고했고 목록을 줬다. **빈 목록도 답이다** | 그렇다 |
| `Undeclared` | 광고하지 않았다 — **요청을 보내지도 않는다** | 아니다 |
| `Refused { message }` | 광고했는데 거절했거나 `nextCursor`로 잘린 목록을 줬다 | 아니다 |

셋을 하나로 뭉개면 "메서드를 모른다"와 "세션이 하나도 없다"가 같은 빈 결과로
도착한다. 그것이 이 게이트의 전부다 — 재시작한 오케스트레이터가 `Dispatched`로
기억하는 Task의 세션이 목록에 **없다**는 사실은, 그 목록이 권위 있을 때만 결론이 된다.

`Undeclared`와 `Refused`를 가른 것은 [cancel 폴백](#cancel-폴백-2026-09-14--게이트-7의-절반)이
`NoSession`과 `Unreachable`을 가른 것과 같은 판단이다. 둘 다 인벤토리를 주지 않지만
하나는 저쪽이 자기 자신에 대해 정직하게 말한 것이고 다른 하나는 선언과 행동이 어긋난
것이라, 후자에는 보고할 대상이 있다. 접으면 그것을 영영 관측할 수 없다.

**광고하지 않은 Agent에게는 보내지 않는다.** 보내고 `-32601`을 `Undeclared`로 접는
구현도 같은 값을 주지만, 매 재조정마다 아무에게도 답이 없을 왕복이 나가고 무엇보다
`Refused`를 만들 수 없게 된다. 시험은 "물어보면 답할 수 있게 켜 둔 mock이 광고만 끄면
요청을 한 건도 받지 않는다"를 수신 기록으로 단정한다 — 그 단정이 없으면 보내고 결과를
버리는 구현도 통과한다.

#### 왼쪽은 `041`이 이미 만들어 두었다

[실행 신원의 내구화](#실행-신원의-내구화-2026-09-13--게이트-2467의-공통-선행)가 남긴
`tasks.acp_session_id`가 대조의 왼쪽이다. 그 절이 "아직 소비자가 없다"고 적은 그
소비자가 이것이다.

Reconciler의 `reap_stale_dispatched`에는 분기가 넷 있었고, 앞의 셋(워커 row 삭제·워커
재시작·워커 장기 Offline)은 전부 **워커에 관한 사실**로 판정한다. 넷째가
`Some(_) => continue`였다 — 워커는 존재하고 Offline도 아니니 더 할 말이 없다는 뜻이다.
**그 자리가 게이트 2가 지목하던 구멍이다**: 워커는 멀쩡한데 그 위의 실행만 사라진 경우
(오케스트레이터가 재시작해 완료 이벤트를 놓쳤거나, Agent가 세션을 잃었거나)를 어느
분기도 보지 않았고, 그 Task는 완료되지도 실패하지도 않은 채 `Dispatched`로 영구히
남았다.

이제 그 자리에서 워커에게 **지금 무엇을 들고 있는지** 묻는다. 묻는 것은 워커당 한
번이고(Task마다 물으면 한 sweep 안에서 서로 다른 시점의 인벤토리로 판정하게 되어 같은
라운드의 두 판정이 모순될 수 있다), 권위 있는 답에서 세션이 빠져 있을 때만
`Failed(ExecutionVanished)`로 회수한다.

#### `ExecutionVanished`가 `ResultLost`와 다른 이유는 재제출 안전성이다

새 `FailureKind`를 만든 근거가 이것 하나다. `ResultLost`는 "아직 돌고 있을 수도, 이미
끝났을 수도 있다"이므로 재제출하면 같은 일을 두 번 시킬 위험이 있다. 이쪽은 워커가
**지금 들고 있는 것 전부**를 답한 뒤의 부재라 실행이 끝났다는 것이 확정이고, 남은 미지는
"어떻게 끝났는가" 하나다. 둘을 한 이름으로 뭉치면 운영자가 재제출해도 되는 경우와 안
되는 경우를 구분할 수 없다.

#### 무엇이 아직 열려 있는가

**게이트는 부분이지 닫힘이 아니다.** 둘이 남았다.

1. **오늘 이 capability를 광고하는 Agent가 있는지는 여전히 관측되지 않았다.** 다만
   그것이 이제 코드가 런타임에 읽어 로그에 찍는 값이라(`declares_session_list`),
   배포 한 번이면 답이 나온다 — 문서가 미지로 안고 있을 값이 아니게 됐다. 광고하지
   않는 배포에서는 회수가 **한 건도** 일어나지 않으며, 시험 두 건이 그것을 부정
   단정으로 고정한다(`Undeclared`를 빈 목록으로 읽는 구현은 오늘의 모든 배포에서
   진행 중인 모든 작업을 회수한다).
2. **순서가 없다.** 이 게이트의 문언은 "control failover 뒤 inventory-first recovery가
   **신규 dispatch보다 먼저**"를 요구한다. 지금 조회는 재조정 sweep 안에서 일어날 뿐
   dispatch 경로를 막지 않는다. 그것을 넣으려면 failover 직후의 1회성 단계가 필요하고,
   그 단계는 리스 획득 시점을 알아야 한다.

게이트 7의 남은 절반(`CancelUnconfirmed`의 확인 채널)도 같은 수단 위에 선다 — `Unreachable`로
끝난 취소의 세션이 인벤토리에서 사라졌는지를 되묻는 것이 그 확인이다. 다만 그것은 상태
기계를 바꾸는 일이라 이 변경의 범위가 아니다.

### 게이트 4는 지금 닫을 수 없다 — 그 이유와, 그래도 고친 것 (2026-09-14)

게이트 4에 착수하면서 [실행 일관성](tasks/execution-consistency.md) 정본을 먼저 읽었고,
**원장을 지금 만들면 안 된다**는 결론에 닿았다. 막는 것이 셋이고 셋 다 코드로 넘을 수 있는
종류가 아니다.

1. **생산자가 구조적으로 없다.** 정본은 "Worker가 호출 **전에** `tool_effect`를 durable
   ledger에 기록해야 한다"고 요구하는데, 도구는 워커의 grok 프로세스 **안에서** 돈다.
   오케스트레이터도 `fleet-worker`도 그 호출을 하지 않으므로 "호출 전"이라는 시점을 가질 수
   없다. 우리가 보는 것은 ACP `session/update`가 사후에 알려 주는 관측뿐이다.
2. **증명이 없다.** 정본이 못 박듯 `status = Completed`는 Agent가 그렇게 보고했다는 뜻이지
   부작용이 적용됐다는 증명이 아니다. 증명은 provider receipt/external reference에서 오고
   그것은 아직 없다.
3. **외부 idempotency key의 앵커가 설계 미결이다.** 키는 `Project ID + Task ID + effect scope +
   제출 시 정책 revision`의 HMAC인데, 무재시도 정책 아래에서 실패는 **새 Task**가 되므로
   Task ID가 바뀌고 키가 달라진다. 정본과 로드맵 `#62`가 이것을 **설계 미결**로 올려 두고
   "없는 것을 미리 만들지 않는다"고 적었다.

셋 중 하나라도 무시하고 여덟 상태 테이블을 만들면, 이 저장소가 037·040에서 이미 밟은
함정(생산자 없는 컬럼)과 `OutcomeUnknown`·`CancelUnconfirmed`에서 확인한 함정(출구 없는
상태)을 동시에 밟는다.

**2026-09-14 — 이 게이트를 의도적으로 보류로 둔다.** 이 fleet은 아직 외부 메시지 발송·DB
마이그레이션 같은 비가역 외부 부작용을 내는 워크로드를 돌리지 않는다. 즉 원장이 증명할 대상
자체가 없다. 이 행이 `차단`인 것은 방치가 아니라 **판정**이며, 근거와 재개 조건은
[실행 일관성](tasks/execution-consistency.md#원장을-언제-만들-것인가--2026-09-14-판정)이
정본으로 갖는다. 그 부재가 강제된 것이 아니라는 점(`bash`로는 오늘도 가능하다)도 같은 절에
적혀 있고, 그것을 닫는 것은 `#64`의 범위다.

**대신 고친 것**은 원장의 **입력**이 terminal 이후에 사라지던 결함이다 — 상세는
[실행 일관성](tasks/execution-consistency.md#원장의-입력이-terminal-뒤에-사라지던-것-2026-09-14-고침).
`Failed(ResultLost)`가 "아직 돌고 있을 수도 있다"를 뜻하는데 정말로 돌고 있다는 증거가
그 순간부터 버려지고 있었다. 게이트는 여전히 **차단**이고, 원장이 생기면 읽을 로그가
이제 그 구간에서 비어 있지 않다.

### cancel 폴백 (2026-09-14) — 게이트 7의 절반

앞 절이 신원을 내구화했고, 이 절이 그 첫 소비자다.

**고친 것**: `AcpTransport::cancel`은 task_id로 인메모리 세션 맵만 뒤졌다. 그 맵은
프로세스와 함께 비워지므로 **오케스트레이터가 한 번이라도 재시작하면 그 뒤의 모든
취소가 대상을 찾지 못한 채 `Ok(())`를 돌려줬다.** 저장소에는 `Cancelled`가 적히고
워커의 세션은 계속 돌았다. failover 없이도 평범한 배포마다 일어나는 일이다.

이제 호출부가 `CancelRequest`에 `tasks.acp_session_id`와 `worker_id`를 함께 싣고,
인메모리 맵이 모르면 그 단서로 해당 워커의 연결에 직접 보낸다.

**결과를 뭉개지 않는다.** 반환형이 `()`에서 `CancelDelivery`로 바뀌었다. 예전에는 세
경우가 전부 `Ok(())`였다 — 연결이 없을 때(주석은 "idempotent success"라고 적었지만
거짓이다), 통지를 보냈을 때, 세션을 못 찾았을 때. 셋을 가른다:

| 값 | 뜻 |
| --- | --- |
| `Sent` | 살아 있는 연결로 통지를 내보냈다. **확인이 아니다** — ACP cancel은 ack 없는 notification이라 "받았다"도 "멈췄다"도 뜻하지 않는다 |
| `NoSession` | 지목할 세션 자체가 없다. `Pending`이거나 `session/new` 전에 실패한 경우 |
| `Unreachable` | 세션은 아는데 연결이 없다. **보낼 것이 있는데 못 보냈다** |

`NoSession`과 `Unreachable`을 가르는 것이 이 변경에서 판단이 필요했던 자리다. 처음
구현은 "세션은 아는데 워커가 등록돼 있지 않다"를 `NoSession`으로 접었는데, 그러면 이
enum이 존재하는 이유가 사라진다 — 저쪽은 "보낼 것이 없다"이고 이쪽은 "저쪽이 계속
돌고 있을 수 있다"이다.

**미전달은 감사에 남는다.** `control.cancel_undelivered`가 새 액션이고,
`FleetState::audit_decision`으로 기록한다 — `audit_control`이 **아니다.** 저쪽은 리스가
없으면 아무것도 남기지 않는데(단일 인스턴스에는 경합이 없으므로 맞는 처분이다),
미전달 취소는 경합과 무관하고 오히려 **단일 인스턴스에서 더 흔하다**. `audit_control`에
실었다면 가장 흔한 경우가 통째로 사라졌을 것이다.

**게이트 7은 아직 닫히지 않았다.** 남은 절반은 `CancelUnconfirmed` 상태와 그 상태에서
빠져나오는 **확인 채널**이다. 지금은 `Unreachable`이어도 Task를 `Cancelled`로 확정한다 —
감사에 흔적을 남길 뿐 상태 기계를 바꾸지는 않았다. 확인은 세션이 실제로 사라졌는지를
되묻는 왕복이 있어야 하고, 그것은 `session/list`에 걸려 있어 여전히 grok의 지원 여부가
실측되지 않았다.

**2026-09-16 정정 — 마지막 절이 틀렸다.** 지원 여부는 `initialize`가 선언하며 그 답은
이미 도착하고 있었다. 되묻는 왕복도 이제 있다(`WorkerTransport::list_sessions`). 게이트 7에
남은 것은 수단이 아니라 **상태 기계**다 — `CancelUnconfirmed`라는 비terminal 상태와 그
상태에서 빠져나오는 전이가 없고, 지금은 `Unreachable`이어도 곧바로 `Cancelled`로 확정한다.

### 어느 게이트에도 귀속되지 않았던 archive의 제어 구멍 (2026-09-12, 닫음)

게이트 4·7이 archive를 다루지만, 둘 다 **무엇이 archive를 막아야 하는가**(effect, 미확인
cancel)에 관한 것이다. 그 아래에 **누가 archive할 수 있는가**가 비어 있었고 어느 행도 그것을
적지 않았다.

`PgStore::update_project_status`는 조건 없는 `UPDATE`였고, MCP(`fleet_delete_project`)와
Dashboard(`DELETE /api/projects/{id}`) 어느 핸들러도 `lease_allows_control()`을 부르지 않았다
(grep 0건). **즉 lease를 잃은 인스턴스가 Project를 archive할 수 있었다.** dispatch와 cancel은
`#62`·`#63`에서 fence로 막혔는데 archive만 남아 있었다 — 제어면 결정 중 유일하게 술어가 없는
쓰기였다.

닫는 방식은 이 저장소의 기존 fence와 같다: `update_project_status`가 `Option<&ControlFence>`를
받아 `AND EXISTS (SELECT 1 FROM control_plane_lease WHERE cluster_id = $3 AND epoch = $4)`를
**`UPDATE`와 같은 문장 안에** 넣는다. lease를 먼저 SELECT해서 분기하면 그 사이에 fenced되어도
이미 떠난 쓰기가 도착한다. `fence`가 `None`이면 술어를 걸지 않는다 — `lease_allows_control()`이
같은 경우에 `true`를 주는 것과 짝을 이루며, HA lease를 켜지 않은 단일 인스턴스 배포가 자기
자신에게 막히지 않게 한다.

`ArchiveProgress`에 `Fenced`를 더해 `Draining`과 **구분한다.** 뭉개면 호출부가 "아직 막는 것이
있다"로 읽고 재시도하는데, fenced 인스턴스의 재시도는 영원히 성공하지 않는다. 두 표면 모두
이 값을 blockers 목록이 아니라 오류로 돌려준다.

검증은 실제 Postgres다(`crates/fleet-store/tests/projects.rs`의
`a_fenced_instance_cannot_archive_a_project`, `archive_reports_fenced_separately_from_blocked`).
fence는 SQL 술어이므로 `MemStore`로 도는 시험은 이 `AND EXISTS`를 한 줄도 검증하지 않는다.
낡은 fence가 거절되는 것만 보면 "id가 없어서 0행"과 구분되지 않으므로, **같은 Project에 살아
있는 fence로 같은 전이가 통과하는 대조**를 함께 둔다.

**"언제"의 절반을 2026-09-15에 닫았다.** `advance_project_archive`는 check-then-act였다 — 두
blocker SELECT와 최종 `UPDATE`가 별개 문장이라, 게이트를 통과시킨 뒤 쓰기 **사이에** 들어온
Task나 Agent 위로 archive가 그대로 지나갔다. 이제 `Store::archive_project_if_drained`가
위상(`draining`)·Task 게이트·Agent 게이트·fence를 **전부 같은 `UPDATE`의 술어로** 건다.

순서를 뒤집은 것이 요지다. 진단(무엇이 막았는가)을 술어 **앞**에 두면 그 진단이 곧 창이
된다 — `control_fence_holds`에서 이미 배운 형태로, 진단은 0행 **뒤에** 와야 한다. 그래서
`advance_project_archive`는 먼저 시도하고, 0행이면 그때 두 게이트를 조회해 사유를 만든다.
`ArchiveBlockers` 보고는 그대로다.

시험은 **`advance_project_archive`를 거치지 않고** `archive_project_if_drained`를 직접 부른다.
상위 함수를 거치면 그 함수의 사전 조회가 막아 주기 때문에, 술어가 SQL에 없어도 시험이
통과한다(`crates/fleet-store/tests/projects.rs`의
`an_active_task_blocks_the_archive_write_itself` 외 2건).

**아직 남은 반대 방향**: 제출 경로(`ensure_project_accepts_new_tasks` → `insert_task_row`)는
여전히 두 문장이라 **archive 확정 뒤에 Task가 삽입되는 순서**가 성립한다. 그쪽을 닫으려면
INSERT에 project 위상 술어를 걸어야 하는데, 그 INSERT는 `ON CONFLICT`로 클라이언트 멱등성
(`#62` 2단계)을 구현하고 있고 **0행을 "중복 제출"로 해석**한다. 술어를 그대로 더하면 거절된
삽입이 중복으로 오독되어 호출자가 남의 Task를 돌려받는다. 0행의 이유를 가르는 진단이 함께
필요하며, 그것은 이 수정과 별개의 작업이다.

게이트 5의 판정 근거를 남긴다: 이 차단은 새로 도입한 제약이 아니라 **이미 문서가 요구하고 있었으나
집행되지 않던 것**이다. `fleet-api`의 `build_worker`는 `liveness_mode`와 무관하게
`WorkerStatus::Online`을 기록하고, `HealthChecker`는 heartbeat이 없는 `on_demand` 워커를 의도적으로
강등하지 않으며, `WorkerSelector`에는 liveness 조건이 없었다. 세 전제가 각각은 타당한데 합치면
"생존 여부를 확인할 수단이 없는 워커가 영구히 dispatch 대상"이 된다. `worker.rs`의
`WorkerLivenessMode::OnDemand` 문서와 `handlers.rs`가 렌더링하는 worker.toml의 경고가 이미 그
배정을 범위 밖이라고 적어 왔지만 강제하는 코드가 없었다.
