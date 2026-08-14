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
