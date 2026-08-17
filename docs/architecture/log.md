---
type: architecture-log
authority: historical
implementation: not-applicable
verification: not-applicable
source: "docs/architecture/log.md"
last_verified: "2026-08-16"
---

# 아키텍처 설계 문서 변경 로그

`docs/architecture/` 하위 설계 문서의 개정 이력을 문서별·날짜별로 기록합니다.
**각 문서 본문은 항상 "현재 확정된 설계"만 담고, "어떻게 이 결정에
도달했는지"는 여기에 append-only로 쌓습니다** — 결정 자체를 재검토할 필요는
거의 없지만, 왜 그렇게 결정했는지는 나중에 반드시 다시 찾게 되기 때문입니다.

새 개정을 만들 때는 해당 문서 섹션 맨 아래에 새 날짜/차수 항목을 추가하세요.
기존 항목은 절대 수정하지 않습니다(append-only) — 이후 개정으로 뒤집힌
결정이라도 "당시엔 왜 그렇게 판단했는지"는 그대로 남겨둡니다.

---

## `project-feature-design.md` (로드맵 [`#48`](../roadmap/roadmap.md))

### 2026-08-14, 1차 — 최초 설계
프로젝트가 여러 host/agent(워커)를 담고, 하나의 프로젝트에 여러 agent가
분산 디스패치될 수 있어야 한다는 요구사항으로 착수. host/worker↔project
소속을 **다대다(M:N, `project_workers`/`project_hosts` 조인 테이블)**,
`project_id` 지정 태스크의 디스패치는 **소프트 힌트**(배치된 agent가 없으면
전체 풀로 폴백)로 AskUserQuestion을 통해 확정.

### 2026-08-14, 2차 — 배타적 소유로 전면 개정
`#49`(에이전트 동적 프로비저닝) 설계 논의 중 "host에는 여유가 있을 때만
agent를 만든다"는 요구사항을 구체화하면서, 사용자가 "프로젝트의 하드 격리가
기본이어야 충돌/경쟁을 예방한다"는 원칙을 제시. 처음엔 "host 소유권만
하드로 하고 기존 워커 M:N은 안 건드리는" 절충안을 제안했으나, 사용자가
**"host 소유권이 비배타적이어도 워커를 실행하면서 리소스 경쟁이 생기지
않는가?"**라고 정확히 반문 — 워커가 M:N으로 공유되면 그 워커가 도는
host의 물리 자원과 워커 자체의 `max_concurrent` 세션 슬롯을 다른 프로젝트
태스크와 항상 경합하게 된다는 걸 인정하고, **워커/호스트 모두 배타적
(1:N) 소유 + 하드 디스패치**로 전면 개정. `project_workers`/`project_hosts`
조인 테이블을 `workers.project_id`/`hosts.project_id` 직접 FK로 교체,
디스패치는 소프트 폴백 대신 하드 필터(`SelectionError::NoWorkerForProject`)로
전환하되 `#38`의 기존 재시도/Dead-Letter 경로를 그대로 재사용(새 메커니즘
없음). 구현 전이라 재작업 비용 없이 바로잡음.

### 2026-08-14, 3차 — 전체 재검토(버그 8건 + UI/UX)
구현 착수 전 사용자 요청으로 `#48`/`#49` 설계 문서 전체를 재검토. 이 문서에
해당하는 수정: (1) `Project.default_agent_template_id`가 아직 정의되지
않은 `#49`의 `AgentTemplateId`를 참조해 컴파일이 안 되는 전방 참조 버그 —
`tasks.project_id`의 FK-후행 예약 패턴(013_task_threads.sql)과 동일하게
원시 `Uuid`로 완화. (2) `AgentProvisioningMode` 소유권을 `#48`
(`fleet-core::project`)로 명확화. (3) `workdir_template` 필드가 프로즈
서술만 있고 실제 스키마/구조체엔 없던 누락 수정. (4) 워커 재등록 시
`project_id`를 host 기준으로 매번 재동기화하는 불변식 명시(호스트 재배정
중 재연결하는 워커가 예전 값을 들고 있는 레이스 방지). (5) `unassign_worker`/
`unassign_host` → `unassign_worker_from_project`/`unassign_host_from_project`로
개명해 `assign_*` 계열과 접미사 통일. §UI/UX 절 신설(질문 목록만, 세부
설계는 다음 라운드로).

### 2026-08-14, 4차 — `#38` 대비 절차 재검증
실제 코드(`crates/fleet-scheduler/src/reconcile.rs`)를 그라운딩해 §5의
디스패치 실패 처리 서술이 정확한지 확인 — `dispatch_existing()`이
`SelectionError` 세부 변형과 무관하게 `Err`를 균일하게 `WorkerUnavailable`로
취급한다는 서술은 실측과 일치함을 확인(추가 코드 변경 불필요). 다만
dead-letter 시 `TaskFailure.error`가 `"dispatch retries exhausted (N
attempts)"`로 원래 `SelectionError` 메시지를 덮어써, `NoWorkerForProject`로
죽은 태스크와 다른 이유로 죽은 태스크가 `Failed` 상태에서 구분되지 않는
문제를 발견 — 마지막 에러 텍스트를 보존하는 개선안을 §5에 기록(`#38` 구현
범위, `#48`이 그 개선에 의존한다는 사실만 기록).

### 2026-08-15, 5차 — 다중 에이전트 팀 검토(13개 관점) + 확정 발견 반영
사용자 요청("10회 이상의 검토와 분석을 진행하도록 팀을 구성하라")으로
`Workflow` 도구를 이용해 13개 관점 리뷰어 + 3표 적대적 검증 + 종합으로
구성된 다중 에이전트 팀을 실행(계정 월간 사용량 한도로 검증/종합 단계
일부가 중도 실패 — 검증까지 끝까지 통과한 발견만 "확정"으로 취급).
이 문서에 해당하는 확정 발견 6건을 반영: (1) **critical**
`assign_worker_to_project`가 §3의 배타적 소유 불변식을 강제하지 않아
host에 연결된 워커를 다른 프로젝트로 개별 재배정할 수 있던 구멍 — 409
가드 추가. (2) **major** `upsert_worker` 재동기화 규칙이 host 미연결
독립 워커를 다루지 않아 재등록마다 `project_id`가 조용히 `NULL`로
초기화될 수 있던 문제 — 독립 워커는 재동기화 대상에서 제외하도록 명시.
(3) **major** 재배정 시 진행 중 태스크 정책이 `roadmap.md`에만 있고 이
문서 본문엔 없던 것 — §5에 정식 반영. (4) **minor** §5의 디스패치
파이프라인 단계 번호가 실제 `selector.rs` 주석과 불일치 — 정정. (5)
**minor** `ProjectAssign`이 `Operator` 기본 권한인 게 다른 인프라 변경
권한(Admin 전용)과 비일관 — 정책 변경은 안 하고 §9 열린 질문으로 기록.
(6) **note** `project-assignment-lifecycle.mermaid`가 어느 절에서도
참조되지 않던 고아 다이어그램 — §3 앞에 참조 추가.

미검증(예산 소진으로 검증 못 받음, critical) 1건도 함께 반영: **Agent의
`project_id`가 host 재배정 시 재동기화 경로가 없어 Worker(재동기화됨)와
Agent(옛 값 고정) 사이에 불일치가 생길 수 있는 문제** — `assign_host_to_project`가
같은 트랜잭션에서 그 host 위의 모든 Agent의 `project_id`도 함께 갱신하도록
Store 트레이트 doc comment에 명시.

### 2026-08-15, 6차 — 미검증 발견 재검증 라운드(6개 관점 재검토)
5차에서 계정 월간 사용량 한도로 검증받지 못한 6개 관점(cross-rbac-consistency,
cross-terminology-consistency, operational-readiness, ui-backend-consistency,
unverified-assumptions-audit, platform-narrative-coherence)을, 그사이
문서가 여러 차례 수정됐으므로 옛 발견을 복원하지 않고 **현재 문서 상태
기준으로 새로 검토**하도록 재설계해 다시 실행(사용자 요청). 이번엔 "세션
사용량 한도"(5차의 월간 한도와 다름, 한국시간 12:40pm 리셋)에 걸려
78개 에이전트 중 32개만 완료 — 원시 발견 24건 중 9건만 3표 검증 통과,
1건은 실제로 반박됨(L0 계층이 MCP 클라이언트와 워커측 런타임 벤더를
혼동한다는 발견 — 재검증 결과 반박), 나머지 14건은 검증 자체가 세션
한도로 실패해 미검증 상태로 보류(한도 리셋 후 재검증 예정). 확정 9건 중
이 문서에 해당하는 것은 없음(모두 `#49`/`#50`/`#51`/`#52`/`ui-design.md`
소관).

### 2026-08-15, 7차 — 나머지 14건 재검증(3표 완주) + 확정 반영
6차에서 세션 한도로 검증 못 받은 14건을 예산 회복 후 다시 3표 적대적
검증(사용자 요청, 42개 에이전트 전원 정상 완료 — 이번엔 부분 실패 없음).
14건 중 12건 확정, 2건은 실제로 반박(둘 다 직전 라운드에서 이미 고쳐져
있었음 — `agent-data-model.mermaid`의 `capture_terminal` 누락은 검증
에이전트 다수가 놓쳤지만 제가 직접 재확인한 결과 실제로는 여전히
누락돼 있어 판정을 뒤집어 반영, `ui-design.md`의 IA 트리/라우트 매트릭스
누락은 이미 6차에서 고쳐진 게 맞아 반박을 그대로 수용).

이 문서에 해당하는 확정 발견 2건을 반영: (1) **critical, 원래 §9의
"`ProjectAssign`을 `Operator` 기본 권한에 둘지" 열린 질문을 critical로
격상**: `Operator`가 이미 보유한 `ProjectAssign` + `TaskCreate` 조합만으로
`agent-provisioning-design.md`의 `AgentAutoProvisioner`를 트리거해
`AgentCreate`(Admin 전용) 없이도 사실상 Agent를 생성시킬 수 있는 구체적
우회 경로를 확인 — Phase 1 착수 전 확정해야 하는 차단 항목으로
격상. (2) **major** §7 REST 표면의 `GET /projects`/`/projects/:id`/`/projects/new`가
"세션"만 요구한다고 서술해 `ui-design.md`가 명시한 `ProjectRead`/`ProjectCreate`
권한 게이트와 정면 충돌하던 문제 — 세 라우트 모두 실제 권한으로 정정,
`PATCH /api/projects/:id`(`ProjectCreate`, 신규) 엔드포인트도 함께
추가해 `agent-provisioning-design.md` §4.1이 전제하던 "Automatic 전환
API"의 실체를 채움.

`agent-memory-injection-flow.mermaid`(critical)와
`project-aware-dispatch-logic.mermaid`(major)도 같은 라운드에서 확정 —
전자는 "#48의 소프트 선호 필터(변경 없음)"라는 초과된 레이블을 하드
배타적 필터로 정정, 후자는 회로차단기/용량 필터 단계 번호를 실제
`selector.rs` 주석(3/3.5)과 일치하도록 재작성(§5 프로즈는 이미
정확했으나 다이어그램만 구 번호를 유지하고 있었음).

---

## `agent-provisioning-design.md` (로드맵 [`#49`](../roadmap/roadmap.md))

### 2026-08-14, 1차 — 최초 설계
사용자가 두 차례에 걸쳐 요구사항을 확장(1차: host당 다중 에이전트/custom
프롬프트/프로젝트발 생성/host 여유 기반 생성/세션 간 맥락 유지·메모리/
스레드 요약/디렉토리 결과물 — 8개 항목. 2차, 같은 날: custom 프롬프트
중앙관리·CLI 연결/tool·skill 연결/tool·MCP 중앙관리/template/필수·옵션
tool — 5개 항목, 1차의 (2)를 구체화). AskUserQuestion으로 핵심 결정 확정:
Agent를 Worker와 분리된 신규 엔티티로 도입(Worker는 저수준 접속/용량
개념 유지), 진짜 동적 프로비저닝(오케스트레이터가 실행 중 원격 시작/종료),
메모리는 구조화된 텍스트/JSON 누적 + 프롬프트 주입, 로드맵 항목은 `#48`과
독립된 `#49`로 분리.

이어서 사용자가 "도구/CLI/템플릿을 어떻게 판단하는가" 직접 요청 — 추가
질문 없이 코드 조사 후 판단 제시. CLI 연결은 `fleet-cli`의 기존 명령 그룹
패턴에 `Agent` 그룹 추가로 동의. 도구(MCP) 바인딩 메커니즘 조사 결과를
정직하게 반영 — vendor ACP SDK의 `SessionBuilder::with_mcp_server()`가
이미 존재하지만 (a) `fleet-transport`가 필요한 `unstable_mcp_over_acp`
피처를 켜지 않은 상태, (b) SDK 자체가 "unstable" 표시, (c) 외부 MCP
서버에 단순 연결하는 게 아니라 Rust로 구현한 `McpServerConnect`
인프로세스 프록시가 필요해, 처음 판단만큼 간단하지 않다는 걸 재조사로
확인하고 스스로 정정(⚠️ "방금 드린 답변을 정정해야겠습니다"). grok 자체가
로컬 MCP 설정 파일을 읽는지(더 단순한 대안)도 미확인 — 그래서 데이터
모델은 확정하되 "실제 연결 메커니즘"은 구현 착수 시 최우선 검증 스파이크
(신설 Phase 0)로 미루기로 판단. 필수/옵션 도구 활성화는 명시적 선택
(`requested_optional_tools`) 권고.

### 2026-08-14, 2차 — 도구 바인딩 요구사항 반영
"custom 프롬프트/도구를 통한 에이전트 생성" 요구사항을 §5(도구 바인딩)·
§6(중앙 카탈로그·템플릿)으로 신설하고 단계별 계획 재구성.

### 2026-08-14, 3차 — `#48` 하드 격리 개정에 따른 재정렬
`#48`이 host/worker↔project를 배타적 1:N으로 전면 개정하면서 이 문서에도
자연히 딸려온 두 가지: (1) Agent의 `project_id`는 항상 host에서 상속
(host 자체가 이미 배타적으로 한 프로젝트에 속하므로 그 위 Agent도 자동
결정됨 — "에이전트가 여러 프로젝트에 공유되는 경우" 열린 질문 해소).
(2) `AgentProvisioningMode`(수동/자동) 신설 — 사용자가 "에이전트를 사용자가
직접 설정하는 방법과 오케스트레이터가 만들어서 사용하는 옵션"을 요청,
`Automatic`일 때 기존 `Reconciler`/`HealthChecker`/`SessionCleanup` 패턴을
재사용하는 `AgentAutoProvisioner` 백그라운드 루프 신설(§4.1).

### 2026-08-14, 4차 — 전체 재검토(버그 8건 중 이 문서분 + 정책 결정 3건)
`#48`과 함께 구현 착수 전 전체 재검토. 이 문서 수정: `agents.provisioned_by`
컬럼 신설(유휴 자동 종료 대상 판정에 필수 — 이 컬럼 없이는 "Manual로 만든
에이전트는 자동 종료 대상 아님" 규칙 구현 불가), `mcp_servers` 삭제를
`ON DELETE CASCADE`→`RESTRICT`로 변경(관리자가 카탈로그 항목을 지웠을 때
참조 중인 템플릿/에이전트가 조용히 도구를 잃는 운영 리스크 차단), §12에
`agent_memory` 보존/정리 정책 누락을 열린 질문으로 추가, §13(설치·운영
고려 사항)과 §UI/UX 절 신설.

**§4.1 유휴 판단 기준 전면 재작성**(가장 중요한 수정): 기존 초안은
"마지막 태스크 완료 후 시간 경과 + 대기 중 태스크 없음"만 기준으로
삼았으나, 사용자가 **"동작 중인지 판단하는 근거가 정확히 뭐냐, 프로세스
stdio만 본다면 실제로 조용히 작업 중인 에이전트도 타임아웃으로 오판될 수
있다"**고 지적 — 정확한 지적이었음. fleet가 이미 신뢰하는 소스만
사용하도록 재설계: (1) `tasks.status=Dispatched` 존재, (2)
`Worker.active_tasks > 0`, (3) `agent_commands` pending 존재 — 셋 중
하나라도 참이면 동작 중. 타이머 기준은 `GREATEST(created_at, 마지막 완료
시각)`, 발행 직전 재확인으로 레이스 방지. 적용 대상은 `Automatic` 생성
에이전트로 한정(Manual은 정책적으로 제외 — 최소 놀람 원칙).

정책 결정 3건(AskUserQuestion): 재배치 시 진행 중 태스크는 완료까지
그대로 진행. `mcp_server` 삭제는 참조 중이면 RESTRICT. Manual 에이전트는
유휴 타임아웃 정책 대상에서 계속 제외.

### 2026-08-14, 5차 — 프로토콜/절차 재검토(코드 그라운딩)
실제 코드(하트비트 프로토콜 `crates/fleet-api/src/schema.rs`, `GrokRunner`
`crates/fleet-worker/src/grok_process.rs`, 서킷브레이커
`crates/fleet-scheduler/src/breaker.rs`, `Reconciler`)를 근거로 §4 절차와
에러 처리를 재검증. 실제 발견/수정:

1. "워커는 인바운드 연결을 받지 않는다(`#42`)" 원칙이 mTLS 배포에선
   부정확함을 확인(`MtlsProxy::bind`, 기본 2420포트 — 실제 인바운드
   리스너 존재) — 범위를 "제어 플레인(등록/하트비트)만 아웃바운드"로 정정.
2. `HeartbeatResponse`가 현재 확장 불가능한 고정 구조
   (`{ok, desired_state: &'static str, server_time}`)라 `pending_commands`
   필드 추가 자체가 Phase 4 스키마 변경 범위임을 명시(이전 문서는 이미
   확장 가능한 것처럼 서술).
3. **`AgentAutoProvisioner`가 막 생성돼 아직 ack되지 않은 `Pending`
   에이전트를 "없음"으로 오판해 같은 대기 태스크에 중복 에이전트를 생성할
   수 있는 레이스** 발견 — eligibility 체크에 `Pending` 포함하도록 수정.
4. `agent_idle_timeout_secs`를 프로젝트에서 매번 라이브 조회하면 프로젝트
   삭제 시 그 소속이던 자동생성 에이전트가 영원히 유휴 스윕에서 빠지는
   좀비가 됨을 발견 — `agents.idle_timeout_secs` 스냅샷 컬럼으로 전환
   (`#48` §6 "왜 스냅샷인가"와 동일한 이유).
5. `hosts.max_agents` 체크가 TOCTOU 레이스임을 발견 — `SELECT ... FOR
   UPDATE` 트랜잭션으로 수정.
6. `/v1/workers/register`가 이름 유일성을 검사하지 않는(upsert) 것을
   확인 — `worker.name`에 `agent_id` 전체 UUID를 포함해 충돌을 구조적으로
   차단하도록 명시(축약하면 서로 다른 두 agent가 조용히 서로의 워커
   레코드를 덮어쓸 위험).
7. `agent_commands` ACK 프로토콜 구체화: 신규 `POST /v1/workers/
   agent-commands/:id/ack` 엔드포인트, `Pending→Starting→Running` 전이
   시점(이전엔 `Starting` 전이 시점 자체가 미정의였음), 실패 경로, 멱등성을
   "agent_id당 프로세스 1개" 효과 단위로 확정.
8. 기존 `GrokRunner`의 "비정상 종료 시 자동 재시작" 루프가 `stop`
   커맨드의 kill을 그대로 되살릴 수 있음을 발견 — 의도된 종료 신호를
   먼저 보내도록 명시.
9. 호스트 삭제 시 `agents.host_id`의 `ON DELETE CASCADE`로 실행 중
   agent가 조용히 사라지고 프로세스가 고아로 남는 문제 발견 — **정책
   결정(AskUserQuestion): 터미널 상태가 아닌 agent가 있으면 호스트 삭제를
   애플리케이션 레벨 409로 차단**(RESTRICT, `mcp_servers` 정책과 동일
   기조 — DB FK가 아니라 앱 코드인 이유는 상태 조건부라 순수 FK로 표현
   불가하기 때문).

`active_tasks`(하트비트 자기보고, 최대 15초 지연) vs `Dispatched` 태스크
존재(오케스트레이터 직접 기록, 실시간) 신뢰도 우선순위도 재정정 — 이전
문서는 `active_tasks`를 "가장 신뢰할 수 있는 지표"라고 서술했으나 실측
결과 순서가 반대임을 확인.

### 2026-08-14, 6차 — 전체 생명주기 다이어그램 + 협업 분석
사용자 요청으로 여러 라운드에 흩어진 상태 전이(§4 프로토콜, §4.1 유휴
판정, `#50`의 tmux 매핑, 호스트 오프라인 스윕)를 하나의 상태 다이어그램
(`agent-lifecycle-state-machine.mermaid`, 신규 §4.2)으로 통합하고
오케스트레이터/`fleet-worker` 협업 패턴("의도 큐잉 → 폴링 실행 → ack
보고 → 상태 확정"이라는 비대칭 폴링 패턴)을 분석. 이 과정에서 처음
드러난 갭 2건 발견 즉시 수정: (1) 호스트 오프라인 스윕이 `Pending` 상태를
언급하지 않아 커맨드는 `failed` 처리되는데 Agent 자체는 `Pending`에
무기한 남는 문제 — 스윕 대상을 터미널이 아닌 모든 상태로 확장. (2)
`Stopping`이 호스트 오프라인 스윕에서도 별도 정체 처리에서도 빠져 있어,
`fleet-worker`가 종료 처리 도중 재시작하면(완료 ack 유실) 무기한 정체
가능 — "정체 5분 초과 시 `Stopped`로 강제 확정" 규칙 신설(`#50`의
`kill-server` 기동 정책이 실제 정리를 보장한다는 전제로 낙관적 확정).

### 2026-08-15, 7차 — 다중 에이전트 팀 검토(13개 관점) + 확정 발견 반영
`project-feature-design.md`와 동일한 팀 검토 라운드(13개 관점 리뷰 + 3표
적대적 검증, 계정 사용량 한도로 일부 관점 검증 중도 실패). 이 문서에
해당하는 확정 발견 12건을 반영: (1) **critical** `016_agents.sql`에서
`agents.template_id`가 아직 생성되지 않은 `agent_templates` 테이블을
인라인 `REFERENCES`로 참조하는 전방 참조 마이그레이션 오류 — 원시
`UUID` 컬럼 선언 후 `agent_templates` 생성 직후 `ALTER TABLE ... ADD
CONSTRAINT`로 FK 후행 추가(§48 3차의 동일 패턴 재사용). (2) **critical**
같은 자리에서 `#48`이 약속했던 `projects.default_agent_template_id` FK도
실제로는 한 번도 추가된 적이 없던 것을 함께 추가. (3) **critical** §4.1/
`AgentAutoProvisioner` 여유 판정이 "터미널 상태가 아닌 에이전트"라면서
`Stopping`을 누락해 `max_agents`를 우회할 수 있던 버그 — 명시적으로
`Stopping` 포함. (4) **critical** §4 7단계에서 동적 프로비저닝된 Worker에
`project_id`를 설정하는 절차가 없어 `#48`의 하드 디스패치 필터가 신규
워커를 자기 프로젝트에서도 조용히 배제하던 문제 — 등록 직후
`worker.project_id = agent.project_id` 직접 설정 단계 추가. (5) **major**
신규 12단계 "`Starting` 정체 방지"(5분 초과 시 `Failed`로 강제 전이,
`Stopped`가 아닌 이유는 시작 성공 증거가 아예 없기 때문). (6) **major**
신규 13단계 "`fleet-worker` 재시작 시 `Running` 에이전트 정리" —
`process_incarnation` 부팅 인스턴스 ID 필드 신설, 값이 바뀌면 그 host의
`Running`/`Starting` 에이전트를 전부 `Failed`로 강제 전이(오프라인 스윕과
개별 커맨드 ack 추적 둘 다 놓치던 "같은 host의 빠른 fleet-worker 재시작"
갭을 닫음). (7) **major** `POST /api/agents`에 `project_id`/`--project`가
남아 있어 "project_id는 host에서 파생, 직접 설정 불가" 결정과 모순 —
요청에서 완전히 제거, 포함 시 400. (8) **major** `Automatic` 모드 전환
전제조건이 `default_agent_template_id`만 요구해 `agent_idle_timeout_secs`
없이도 전환 가능했던 문제 — 회수 불가능한 자동생성 에이전트를 막기 위해
둘 다 필수로 확정. (9) **minor** §5.2 Path B(로컬 MCP 설정 파일) 방식이
같은 host의 여러 에이전트 간 설정 파일 경로 충돌을 고려하지 않았던
누락 — Phase 0 스파이크 범위에 추가. (10) **minor** §9 workdir_template이
`Agent`에도 별도 컬럼이 있는 것처럼 서술된 모순 — `Project` 전용 필드로
정정, 에이전트별 디렉토리는 디스패치 시점에 계산되는 파생 규칙
(`{project.workdir_template}/{agent.name}`)으로 명시. (11) **minor**
`build_threaded_prompt()` 위치 오기재(`acp_transport.rs`) — 실제 위치
`dispatcher.rs`로 정정. (12) **minor** "호스트 인벤토리 기능
(`ui-design.md` §3.9)" 인용 오류 — 실제 §3.2.5로 정정(§3.9는 `#48`이
나중에 추가한 "프로젝트 목록").

`agent-lifecycle-state-machine.mermaid`에 새 전이 3개 추가:
`Starting → Failed`(정체 타임아웃), `Running → Failed`/`Stopping →
Failed`(둘 다 `process_incarnation` 재시작 감지 경유), `Starting →
Running` 전이에 `worker.project_id = agent.project_id` 단계 주석 추가.

### 2026-08-15, 8차 — 미검증 발견 재검증 라운드(6개 관점 재검토)
`project-feature-design.md` 6차와 동일한 재검증 라운드(세션 한도로 78개
중 32개 에이전트만 완료, 확정 9건/반박 1건/미검증 14건 — 경위는
`project-feature-design.md` 6차 항목 참고). 이 문서에 해당하는 확정
발견 2건을 반영: (1) **critical** 기존 프로덕션 mTLS 배포
(`docs/deployment/server-topology.md`, `MtlsProxy::bind`)가 host당
고정 단일 upstream 1:1 구조인데, Phase 4의 host당 다중 에이전트(동적
포트) 모델과 어떻게 공존하는지 `#48`~`#52` 어디에도 서술이 없던 구조적
공백 — §13에 새 항목(5번)으로 기록, Phase 4 착수 전 반드시 해소해야
하는 설계 공백으로 표시하고 후보안 검토를 Phase 0 스파이크 범위에 포함.
(2) **minor** §13.4(프로비저닝 실패 알림 경로 없음)이 열린 질문으로만
남고 이후 `ui-design.md` §3.9~§3.14 갱신에 전혀 반영되지 않았던 격차 —
§13.4에 재확인 주석 추가.

이 라운드는 `ui-design.md`의 `/projects/new` 폼에 `default_agent_template_id`/
`agent_idle_timeout_secs` 입력 필드가 없어 §4.1의 "Automatic 전환 시 둘 다
필수" 규칙을 실제로 만족시킬 UI 경로가 없다는 major 발견도 확정했습니다
(파일 소재는 `ui-design.md`지만 근거 규칙은 이 문서 §4.1) — `ui-design.md`
§3.9 인터랙션 표에 두 필드를 추가해 반영했습니다.

### 2026-08-15, 9차 — 나머지 14건 재검증(3표 완주) + 확정 반영
`project-feature-design.md` 7차와 동일한 재검증 라운드(경위는 그쪽 항목
참고). 이 문서에 해당하는 확정 발견 2건을 반영: (1) **critical** — §4.1의
"Automatic 전환 API/CLI 호출"이 어떤 엔드포인트인지 다섯 문서 어디에도
정의돼 있지 않던 공백을 확인 — `project-feature-design.md` §7에 신설된
`PATCH /api/projects/:id`(`ProjectCreate`)를 크로스 레퍼런스로 추가.
(2) **critical** §10에 새 경고 박스 신설 — `AgentAutoProvisioner`가 RBAC
검사 없이 Agent를 생성하는데, `Operator`가 이미 보유한 `#48`의
`ProjectAssign`+`TaskCreate` 조합만으로 이를 트리거해 `AgentCreate`(Admin
전용)를 우회할 수 있는 구체 경로를 발견 — 해소책은 정책 결정이라 이
문서에서 임의로 바꾸지 않고 `project-feature-design.md` §9로 정본을
넘김(그쪽 문서 7차 항목 참고).

---

## `agent-terminal-access-design.md` (로드맵 [`#50`](../roadmap/roadmap.md))

### 2026-08-14, 1차 — 최초 설계
사용자 요청("worker의 동작을 tmux로 터미널 동작을 모니터링하고 cli로 직접
연결하는 것을 지원하고 싶다")으로 신규 등록. `#49` Phase 4가 host당 여러
grok 프로세스를 관리하게 되면서 로그 수집/모니터링/디버깅 개입 수단이
없다는 문제를 함께 풀기로 함. AskUserQuestion으로 핵심 결정 확정: 연결
방식은 **하이브리드**(기본 읽기 전용 모니터링, 필요 시 SSH+tmux
인터랙티브 attach로 에스컬레이션), 적용 범위는 **`#49` 이후부터**(에이전트별
다중 프로세스). 읽기 전용 스냅샷은 `agent_commands`/heartbeat 폴링 큐
재사용(`capture_terminal` 커맨드 타입 신설), 인터랙티브 attach는 오케스트레이터가
기존 SSH 키 볼트로 호스트에 붙어 PTY를 열고 `fleet-cli`와는 새 WebSocket
릴레이로 raw 바이트 중계. 신규 RBAC `AgentAttach`(Admin 기본 전용).

이어진 후속 논의("실행/디스패치 자체를 tmux로 바꿀지" 질문)에서, 태스크
디스패치는 기존 ACP WebSocket을 그대로 유지하고 tmux는 grok 프로세스의
실행 envelope(생명주기)+보조 모니터링 역할로만 한정하기로 확정(ACP를
터미널 상호작용으로 대체하는 안은 구조화된 프로토콜을 잃는 훨씬 큰
체급의 변경이라 기각). 이 논의 중 "fleet-worker 재시작 시 세션 재발견"
절차를 최초 설계에 추가했으나(당시엔 검증 안 된 가정), 3차 개정에서
검증 후 폐기됨(아래 참고).

### 2026-08-14, 2차 — 자체 재감사
사용자가 **"tmux 이슈가 완전히 해결됐나, 잠재적으로 숨긴 것인가?
해결되지 않은 문제들은 다시 다 꺼내놓아라"**고 직접 반문 — 재검토 결과
§9에 명시적으로 열어둔 것 외에도, 본문에 확정처럼 서술했지만 실은 검증
안 된 가정(russh의 PTY+exec 지원 여부, `C-c`가 grok에 대해 그레이스풀
종료로 작동하는지, tmux 서버가 `fleet-worker`/systemd 재시작에서 실제로
살아남는지 등)과 아예 언급조차 안 한 새 갭(동시 세션 생성 레이스, tmux
소켓 권한, `capture_terminal` 큐잉 모델, 결과 텍스트 보존 정책 등)을
다수 발견해 §9를 심각도순으로 전면 재작성(14개 항목, 최우선 2개는 설계
실현 가능성 자체를 좌우).

### 2026-08-14, 3차 — 최우선 2개 항목 실제 검증
사용자 요청으로 "설계에서 구현하고자 하는 것들이 실제 구현 가능한지"
검증. **russh PTY 지원 — 확인됨(긍정)**: docs.rs로 `russh 0.46.0`
(`Cargo.lock` 고정 버전) API를 직접 확인 — `request_pty`/`request_shell`/
`exec`/`window_change`/`data` 전부 존재. **tmux 서버 생존성 — 확인됨(부정)
+ 정책 결정**: 실제 배포 유닛 파일 `examples/fleet-worker.service`를
열어보니 `KillMode=mixed` + "grok 서브프로세스도 함께 종료" 주석으로
이미 의도적으로 선택된 설계였음을 발견 — systemd 공식 문서(`systemd.kill`)로
재확인한 결과 이 설정에서는 daemonize해도 cgroup을 벗어나지 못해 tmux가
항상 `fleet-worker`와 함께 죽음. 이건 버그가 아니라 이미 내려진 운영
결정이라, AskUserQuestion으로 유지할지 뒤집을지 재확인 — **기존 철학
유지로 확정**(`KillMode=process`로 뒤집지 않음, systemd의 고아 프로세스
자동 정리 안전망을 유지). tmux의 가치 제안을 "재시작 생존"에서 "같은
fleet-worker 수명 동안의 모니터링/attach"로 좁히고, "재시작 시 세션
재발견" 절차 폐기(대신 기동 시 `tmux kill-server`로 이전 세션 일괄 정리).

### 2026-08-14, 4차 — 세션 재활용 여부
사용자가 "에이전트 종료 시 tmux 세션을 재활용할지, 아니면 없앨지 —
재활용 없이 세션만 남는 건 문제일 것 같지만, 재활용하면 시작 부하를
줄일 수 있지 않나?" 질문. **1:1 테어다운 유지로 확정**(재활용/풀링 안
함) — 근거: (1) `#49` §4.1 "자동 생성은 자동 회수와 짝을 이뤄야 한다"
원칙 및 3차 개정에서 확정한 `KillMode=mixed` 철학과 일관. (2) 재활용의
실제 latency 이점도 근거가 약함을 분석 — 병목은 tmux 래퍼(수 ms)가
아니라 grok 프로세스 부팅 + `agent_commands`/heartbeat 왕복(최대 15초)이라
세션만 재활용해선 지연이 줄지 않음. 진짜 시작 지연을 줄이려면 "agent_id에
묶이지 않은 범용 grok 웜풀"이라는 완전히 별개의(더 큰) 기능이 필요 —
이번 스코프에서는 설계하지 않고 §9에 향후 후보로만 기록.

### 2026-08-15, 5차 — 다중 에이전트 팀 검토(13개 관점) + 확정 발견 반영
동일 팀 검토 라운드에서 이 문서에 해당하는 확정 발견 3건을 반영: (1)
**critical** §3 "생존 감지 방식"이 `tmux new-session -d`가 즉시 반환·detach
되어 런처 프로세스의 `child.wait()`가 실제로는 grok 프로세스의 생사를
전혀 추적하지 못하는데도 그 위에 재시작 판단 로직 전체가 서 있던 근본
결함 — `remain-on-exit on`을 세션 생성 직후 설정(죽은 뒤 pane이 즉시
사라지며 크래시 출력이 유실되던 별도 major 발견도 함께 해소)하고,
`AgentRunner`가 `tmux list-panes -t <session> -F '#{pane_dead}
#{pane_dead_status}'`를 2~5초 간격으로 폴링해 사망을 감지하고 종료 코드를
얻는 방식으로 `child.wait()`를 완전히 대체하도록 §3 전면 재작성. (2)
**major** 의도된 종료(그레이스풀 stop) 경로가 생존 폴링 루프와 경합할 수
있던 문제 — stop을 시작하기 전에 해당 세션을 폴링 대상 집합에서 먼저
제거하도록 명시(의도된 kill을 크래시로 오판해 재시작하는 것을 방지). (3)
**minor** "호스트 인벤토리 기능(`ui-design.md` §3.9)" 인용 오류 — `#49`
7차와 동일한 클래스의 오류가 이 문서에도 독립적으로 존재 — 실제 §3.2.5로
정정.

### 2026-08-15, 4차 — 미검증 발견 재검증 라운드(6개 관점 재검토)
`project-feature-design.md` 6차와 동일한 재검증 라운드(경위는 그쪽 항목
참고). 이 문서에 해당하는 확정 발견 2건을 반영, 둘 다 **operational-readiness**
관점: (1) **major** §5의 `POST /api/agents/:id/attach/ws` WebSocket
업그레이드 엔드포인트가 대시보드 `/api/*` 네임스페이스를 타는데, 기존
프로덕션 nginx 설정(`docs/deployment/nginx-gateway.md`,
`docs/deployment/deployment.md`)이 모든 프록시 location에서 `Connection`
헤더를 비우고 `Upgrade` 헤더를 전달하지 않아 WebSocket 핸드셰이크가
백엔드에 도달하지 못하는 배포 전제조건 누락 — §5에 `proxy_set_header
Upgrade`/`Connection $connection_upgrade` 추가가 `#50` 구현 시 선행돼야
한다는 경고 주석 추가(배포 문서 자체는 이 문서 소유 범위가 아니므로
구현 착수 시점에 함께 갱신하기로 기록만). (2) **major** §4의
`capture_terminal` 결과 저장 계획(`agent_commands.result` 필드 추가)이
실제로는 `agent_commands` 테이블 스키마(`#49`의 `016_agents.sql`)에
`result` 컬럼이 아예 없는데도 대응 마이그레이션 계획이 어느 문서에도
없던 누락 — §9에 `019_agent_commands_result.sql`(가칭) 신규 예약 필요를
명시.

### 2026-08-15, 5차 — 나머지 14건 재검증(3표 완주) + 확정 반영
`project-feature-design.md` 7차와 동일한 재검증 라운드(경위는 그쪽 항목
참고). 이 문서에 해당하는 확정 발견 4건을 반영: (1) **major**
"`AgentRunner`"라는 이름이 `#50`과 `#52`에서 서로 다른 걸 가리키는
용어 충돌 — `#52`는 `AgentRunner`를 `spawn`/`terminate`/`capture_snapshot`
세 메서드만 있는 트레잇으로 정의하는데, `#50` §3은 같은 이름에 "2~5초
간격 tmux 폴링"이라는 상태 유지 책임을 얹고 인접 문장에선 `#52` 이전
구체 타입 `GrokRunner`도 계속 언급하며 종료 메서드도 `terminate_child()`로
불러 혼란스러웠던 문제 — §3 전체를 "`GrokRunner`" → "`AgentRunner`
구현체(`NetworkBindRunner`/`StdioBridgeRunner`)"로 정정하고, 폴링 루프는
트레잇의 새 공개 메서드가 아니라 `spawn()` 내부 백그라운드 태스크라고
명확화하는 안내 박스를 §3 앞에 신설. `terminate_child()`도 `#52`의
`terminate()`로 통일. (2) **major** §3의 생존 감지 메커니즘 3가지 세부
사실(`tmux new-session -d` 리턴 시점, `remain-on-exit` 기본값, `pane_dead`
포맷 변수 정확도)이 검증 표시 없이 확정 사실로 서술돼 있던 문제 — §9
"실기기 검증 필요"에 새 항목(3.5번)으로 이관. (3) **minor** §5의 russh
"구현 가능함을 검증했습니다"가 API 시그니처 존재 확인과 실제 동작 검증을
동일시한 과대 주장이던 문제 — "존재를 확인했습니다"로 표현을 낮추고
§9에 4번 항목(실제 `request_pty`+`exec` 조합의 인터랙티브 스트림 생성
여부는 미검증)으로 이관. (4) **note**(제 직접 재확인으로 판정 override
— 검증 에이전트 3표 중 2표가 반박했으나 `agent-data-model.mermaid`를
직접 grep한 결과 여전히 `capture_terminal`이 빠져 있어 반박을 기각하고
반영) `AGENT_COMMANDS.command_type` ER 필드에 `capture_terminal` 값 추가.

---

## `agent-harness-composition-design.md` (로드맵 [`#51`](../roadmap/roadmap.md))

### 2026-08-15, 1차 — 최초 설계
사용자 요청("에이전트-호스트-custom 프롬프트-tool의 관계와 층위를
분석하고, project/task/skill까지 결합하며, 하네스 엔지니어링 요소 도입도
검토하라")으로 신규 등록. `#49` 1차 요구사항 10번("tool 혹은 skill과
연결")이 애초에 skill을 요구했으나 지금까지 tool만 설계되고 skill은
누락돼 있었음을 확인.

**관계·층위 분석**: 기존 설계를 세 축으로 분리 — 축 1(WHERE, `#48`/`#49`가
이미 완성 — Host는 순수 인프라적), 축 2(WHAT, custom_prompt "정체성" →
Skill(신규) "절차" → Tool "실행"의 3계층, Claude Code 자신의 하네스
구조와 동형), 축 3(WHEN/스코프, Project→Template→Agent→Task 체인, Tool이
이미 따르는 걸 Skill도 복제). 이 과정에서 Host↔Tool 관계를 재정의 — Host는
Tool의 "출처"가 아니라 "가용성 제약"(stdio 도구는 host에 바이너리가
있어야 함)이라는 걸 명확히 하고, `hosts.labels`/
`mcp_servers.required_host_labels` 가드를 신규 설계(기존 Worker의
`labels`/`required_labels` 패턴 재사용, 새 메커니즘 발명 안 함).

**핵심 설계 결정**(AskUserQuestion): Skill 내용은 **DB 텍스트**로 저장
(`mcp_servers`와 동일 패턴) — 파일/git 기반(Claude Code 스타일)도 검토했으나
호스트 배포 메커니즘이 새로 필요하고 스크립트 실행이 가능해지면 보안
검토가 훨씬 커져, 순수 지침형 스킬로 범위를 좁혀 구현 비용을 낮춤. Skill
바인딩은 `agent_template_tools`/`agent_tools`와 완전히 동일한 구조
(`agent_template_skills`/`agent_skills`, required/optional, `#49`가
이미 정한 스냅샷 원칙 재사용)로 설계 — 새 패턴을 만들지 않음.
`Project.constitution_prompt`(CLAUDE.md 유사 개념)도 함께 도입해 축 3에
비어 있던 "Project 레벨" 층을 채움 — 이 저장소 자신이 `agent.md`/
`CLAUDE.md`로 쓰는 패턴과 정확히 같은 개념이라는 메타적 관찰도 있었음.

**하네스 엔지니어링 요소 검토**(Claude Code 하네스 개념 대입): Skill·
프로젝트 헌법은 지금 도입. **Hooks**(세션 내부 도구 호출 후킹)와
**Permission Mode**는 grok/ACP가 그 수준의 개입을 지원하는지 자체가
`#49` §5.2의 도구 바인딩 메커니즘과 똑같이 미검증이라 개념만 기록하고
설계는 보류(검증 안 된 능력 위에 설계하지 않는다는 `#49`의 기존 원칙을
그대로 따름). **서브에이전트 위임**은 `fleet_dispatch_task`를 자기
참조형 `mcp_servers` 카탈로그 항목으로 등록하는 것만으로 이미 자연히
가능함을 확인 — 새 엔티티 불필요, 문서화만.

구현은 `#49` Phase 2(템플릿/카탈로그/도구 바인딩)와 같은 Phase에서 함께
하는 것을 권장 — Skill 바인딩이 도구 바인딩과 스키마·API·UI 패턴이
완전히 동일해 분리 구현하면 낭비.

### 2026-08-15, 2차 — 다중 에이전트 팀 검토(13개 관점) + 확정 발견 반영
동일 팀 검토 라운드에서 이 문서에 해당하는 확정 발견 3건을 반영: (1)
**major** §2 축 2 표에서 Skill 행의 "필요할 때만 로드"가 마치 모든
Skill에 적용되는 것처럼 서술돼 §5의 실제 합성 메커니즘(필수 Skill은
`custom_prompt`와 동일하게 매 디스패치마다 주입)과 모순 — "필요할 때만
로드"는 옵션 Skill에만 해당함을 명시. (2) **major** §3 결정표의 Skill
저장 방식(DB 텍스트 vs grok 네이티브 포맷) 결정이 `#52`가 grok build의
네이티브 Skill/Hook 시스템을 발견하기 이전에 내려졌고, 그 발견은 §7.3
Hooks 결정에만 반영되고 Skill 저장 결정 자체는 재검토되지 않았던 누락 —
Skill 저장 결정도 Phase 0 스파이크에서 재검토 가능한 잠정 결정임을 명시
플래그 추가. (3) **major** §8 UI 서술이 "세 번째 탭으로 추가"라 표현해
`ui-design.md` §3.14가 실제로는 별도 라우트(`/admin/skills`)로 설계한
것과 모순 — 같은 관리자 메뉴 그룹 내 독립 라우트로 정정.

### 2026-08-15, 3차 — 미검증 발견 재검증 라운드(6개 관점 재검토)
`project-feature-design.md` 6차와 동일한 재검증 라운드(경위는 그쪽 항목
참고). 이 문서에 해당하는 확정 발견 1건(**major**, ui-backend-consistency
관점)을 반영: §4가 Skill 바인딩을 `agent_template_tools`/`agent_tools`와
완전히 동일한 구조(required/optional)로 설계했다고 명시하지만, 정작
`ui-design.md`의 에이전트 생성 폼(§3.12)·관리 모달(§3.13)에는 도구
바인딩 UI만 있고 Skill 선택/토글 UI가 전혀 없어 화면에서만 이 대칭이
깨져 있던 문제 — `ui-design.md` §3.12에 Skill 체크박스 목록(7번 항목,
도구와 동일 UI 패턴)을, §3.13에 Skill 토글·필수/옵션 표시를 추가해
반영. `ui-design.md`의 IA 트리·라우트 가드 매트릭스에 `/admin/skills`가
누락돼 있던 별도 minor 발견도 같은 라운드에서 함께 반영.

### 2026-08-15, 4차 — 나머지 14건 재검증(3표 완주) + 확정 반영
`project-feature-design.md` 7차와 동일한 재검증 라운드(경위는 그쪽 항목
참고). 이 문서에 해당하는 확정 발견 1건(**minor**, cross-rbac-consistency
관점)을 반영: §4가 `agent_skills`(Agent별 필수/옵션 스킬 오버라이드)를
`agent_tools`와 완전히 동일한 구조로 설계했다고 서술하면서도, 정작 이
뮤테이션 표면(Agent별 스킬 add/remove)을 어느 권한으로 게이트하는지 §8
어디에도 없던 공백 — `SkillManage`(카탈로그 CRUD 전용)와 별개로,
`AgentManage`(`agent-provisioning-design.md` §10, 이미 도구 바인딩 수정을
포괄)가 스킬 바인딩 수정도 함께 담당한다고 §8에 명시해 Tool/Skill 대칭
원칙을 권한 정의에도 완성. (참고: 같은 발견이 지적한 "화면에 스킬 토글이
없다"는 부분은 이 문서 3차에서 이미 `ui-design.md`에 반영돼 있었습니다.)

---

## `agent-runtime-vendor-design.md` (로드맵 [`#52`](../roadmap/roadmap.md))

### 2026-08-15, 1차 — 최초 설계
사용자 질문("이 설계를 grok-build cli에 적용할 수 있는가? gemini cli에도
적용할 수 있는가?")에 답하기 위해 WebSearch/WebFetch로 공개 문서를 조사.

**조사 결과**: (1) `grok agent serve`는 별개 제품이 아니라 Grok Build
(`grok` 바이너리) 자신의 headless/ACP 서버 모드 — 이미 네이티브 Skills·
Hooks·Plugins·MCP servers 시스템을 갖고 있어(`grok inspect`로 확인
가능) `#51`이 fleet 레벨에 새로 설계한 Skill/Hooks와 개념이 겹침을 발견.
(2) Gemini CLI도 ACP를 지원하지만(`gemini --acp`) grok의 네트워크 bind
방식과 달리 stdio 기반 JSON-RPC — 트랜스포트 아키텍처가 근본적으로
다름을 확인. `fleet-transport::WorkerTransport`/`AcpTransport`는 코드
그라운딩으로 이미 프로토콜 중립적(grok 문자열 리터럴 없음)임을 재확인 —
걸림돌이 `fleet-worker`의 프로세스 스폰 계층뿐임을 좁혀냄.

**핵심 설계 결정**(사용자가 "지금 바로 설계 문서화" 선택): `GrokRunner`를
`AgentRunner` 트레잇으로 일반화, 벤더별 구현체 2종
(`NetworkBindRunner`/`StdioBridgeRunner`) 분리. 핵심 통찰 — stdio 브릿지가
grok과 동일한 네트워크 엔드포인트 모양을 오케스트레이터에 제공하므로
오케스트레이터 측 코드는 전혀 안 바뀜, 벤더 차이가 전적으로
`fleet-worker` 안에 갇힘. 벤더별 바이너리/인자는 신규 `agent_runtimes`
카탈로그로 데이터화(`mcp_servers`/`skills`와 동일 패턴). grok build
네이티브 Skill/Hook vs `#51`의 fleet 자체 계층 중 어느 쪽을 쓸지는
`#49` Phase 0 스파이크 범위를 확장해 실기기로 결정하기로 함(설계
시점에 미리 정하지 않음 — `#49` §5.2가 이미 겪은 원칙을 그대로 따름).

### 2026-08-15, 2차 — 다중 에이전트 팀 검토(13개 관점) + 확정 발견 반영
동일 팀 검토 라운드에서 이 문서에 해당하는 확정 발견 3건을 반영: (1)
**critical** `018_agent_runtimes.sql`이 `ALTER TABLE ... ADD COLUMN ...
DEFAULT (SELECT id FROM agent_runtimes WHERE name = 'grok')` 형태로
`DEFAULT` 절에 서브쿼리를 넣었는데, PostgreSQL은 `DEFAULT` 절에 서브쿼리를
허용하지 않아 마이그레이션 자체가 실패하는 구문 오류 — 컬럼을 기본값 없이
추가 → `UPDATE ... SET runtime_id = (SELECT ...) WHERE runtime_id IS
NULL`로 백필 → `ALTER COLUMN runtime_id SET NOT NULL`로 확정하는 3단계로
재작성(`agent_templates.runtime_id`/`agents.runtime_id` 둘 다 동일하게
수정 — `agent_templates.runtime_id`가 프로즈에서는 필수라면서 SQL에서는
nullable로 남아 있던 별도 minor 불일치도 함께 해소). (2) **major**
`StdioBridgeRunner`가 "raw JSON-RPC 바이트를 그대로 중계"한다고 서술한
부분 — WebSocket 프레이밍과 stdio 개행 구분 JSON-RPC 프레이밍의 메시지
경계 규약이 서로 달라 원시 바이트 그대로 중계하면 메시지가 분할/병합될
위험이 있음 — 양쪽에서 완전한 JSON-RPC 메시지 단위로 파싱 후 각 프로토콜에
맞게 재구성(stdio 쪽은 개행 구분, WS 쪽은 JSON-RPC 메시지당 WS 메시지 1개)
하도록 정정, 정확한 방식 확정은 Phase 0 스파이크로 위임. (3) **minor**
§6 "네 번째 탭"이라는 표현 — `#51` 2차와 동일한 클래스의 오류, 독립 라우트로
정정.

### 2026-08-15, 3차 — 미검증 발견 재검증 라운드(6개 관점 재검토)
`project-feature-design.md` 6차와 동일한 재검증 라운드(경위는 그쪽 항목
참고). 이 문서에 해당하는 확정 발견 2건을 반영: (1) **major**
(ui-backend-consistency) §6이 "Agent 상세(`ui-design.md` §3.13) 헤더에
runtime Badge 추가"라고 Canonical-Derived 갱신을 선언했지만, 실제
`ui-design.md` §3.13 헤더 와이어프레임에는 runtime 값이 전혀 없어 선언과
실제가 어긋나 있던 문제(`agent.md` §5.3 정합성 동기화 규칙 위반 사례) —
`ui-design.md` §3.13 헤더에 실제로 runtime Badge(`agent_runtimes.name`,
예: "grok")를 추가해 반영. (2) **major** (platform-narrative-coherence)
`platform-layer-stack.mermaid`가 L7(이 문서, `#52`)을 L4(`#49`)에만
의존하는 것으로 그렸으나, 이 문서 스스로 `#51`(하네스 구성)에도 명시적으로
의존한다고 두 번 이상 밝히고 있어 `L5 --> L7` 엣지가 빠져 있던 문제 —
다이어그램에 해당 엣지 추가. `ui-design.md`의 IA 트리·라우트 가드
매트릭스에 `/admin/agent-runtimes`가 누락돼 있던 별도 minor 발견도 같은
라운드에서 함께 반영(`#51` 3차와 동일한 위치에 함께 추가).

`agent-runtime-data-model.mermaid`에 `AGENT_TEMPLATES.runtime_id`를
`NOT NULL`로 주석 갱신, `DEFAULT` 서브쿼리 버그 수정 경위 메모 추가.

## 2026-08-17 — Architecture 책임 재분류

- Architecture 진입점과 정본 지도를 현재 정본·Derived·Review 경계만 보이도록 축소했다.
- 구현 참조는 현재 Rust 구성요소와 제약만 남기고, 과거 명령·자기치유 제안·설계 대안을 제거했다.
- 엔티티 비판, lifecycle 정합성, feasibility 검토는 `docs/reviews/`로 이관했다.
- host integrity monitoring 제안은 `docs/operations/proposals/`로 이관했다.

## 2026-08-17 — 정본 지도 통합

- `canonical-map.md`의 질문별 정본 선택표를 Architecture README에 통합했다.
- Architecture README가 이 도메인의 유일한 진입점과 정본 탐색 지도가 되었으며, 기존 지도 파일은 링크를 교체한 뒤 삭제했다.

### 2026-08-15, 4차 — 나머지 14건 재검증(3표 완주) + 확정 반영
`project-feature-design.md` 7차와 동일한 재검증 라운드(경위는 그쪽 항목
참고). 이 문서에 해당하는 확정 발견 3건을 반영: (1) **major**
`agent_runtimes.required_host_labels`가 `#51`이 `mcp_servers.required_host_labels`에
명시적으로 확립한 `NOT NULL DEFAULT '[]'` 관례(같은 필드명, 같은 패턴
재사용을 자처하면서도)를 어기고 nullable/기본값 없음으로 재도입했던
문제(Rust 타입 `Vec<String>`도 non-Option이라 `NULL`을 표현 못 함) —
SQL/Rust doc/ER 다이어그램 세 곳 모두 `NOT NULL DEFAULT '[]'`로 통일.
(2) **minor** §4의 grok wire-format 비표준성 주장이 존재하지 않는
"`#49` §2.2"를 근거로 인용된 추적 불가능한 인용 — 실제 근거인
`crates/fleet-transport/src/acp_transport.rs` 코드 주석으로 정정.
(3) **minor** §1이 "Grok Build는 이미 네이티브 Skills·Hooks·Plugins·MCP
servers 시스템을 갖고 있습니다"를 확정 사실처럼 서술하고, §7의 미검증
캐비어트는 제목이 "바이너리 동일성"으로 좁게 한정돼 이 더 넓은 주장까지
커버하는지 모호했던 문제 — §1에 인라인 캐비어트 추가, §7 항목 제목/범위를
"§1의 Grok Build 관련 주장 전체"로 명시적으로 넓힘. 이 확정 서술이
캐비어트 없이 `agent-harness-composition-design.md` §7.3에 "확인됐습니다"로
전파돼 있던 것도 같은 라운드에서 함께 정정("추정됩니다" + 캐비어트 추가).
