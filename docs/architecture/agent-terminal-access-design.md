# 에이전트 터미널 모니터링·CLI 직접 접속 설계 (tmux 기반)

> 작성일: 2026-08-14. 로드맵 [`#50`](../roadmap/roadmap.md)에 대응하는 설계 문서입니다.
> [`#49` 에이전트 동적 프로비저닝 설계](agent-provisioning-design.md) 위에 쌓이는
> 후속 확장이며, **`#49` Phase 4(`GrokRunner` 다중 프로세스 재작성)에 전적으로
> 의존**합니다 — 그 전까지는 구현 착수 대상이 아닙니다.
> **개정 (2026-08-14, 2차 — 자체 재감사)**: 사용자가 "tmux 이슈가 완전히
> 해결됐나, 숨긴 게 있나"고 직접 반문 — 재검토 결과 §9에 명시적으로 열어둔
> 것 외에도 **본문에 확정처럼 서술했지만 실은 검증 안 된 가정**(russh의
> PTY+exec 지원 여부, `C-c`가 grok에 대해 그레이스풀 종료로 작동하는지,
> tmux 서버가 `fleet-worker`/systemd 재시작에서 실제로 살아남는지 등)과
> **아예 언급조차 안 한 새 갭**(동시 세션 생성 레이스, tmux 소켓 권한,
> `capture_terminal` 큐잉 모델, 결과 텍스트 보존 정책 등)을 다수 발견해
> §9를 전면 재작성했습니다.
> **개정 (2026-08-14, 3차 — 최우선 2개 항목 실제 검증)**: docs.rs로
> `russh 0.46.0`(`Cargo.lock` 고정 버전) API를 직접 확인한 결과
> **PTY 지원은 확인됨**(`request_pty`/`request_shell`/`exec`/
> `window_change`/`data` 전부 존재) — §5는 그대로 구현 가능합니다.
> 반면 **tmux 서버 생존성은 실제로 검증해보니 틀렸습니다** — 실제 배포
> 유닛 파일 `examples/fleet-worker.service`가 `KillMode=mixed`를
> 명시적으로 설정하고 "grok 서브프로세스도 함께 종료"라는 주석까지
> 남겨뒀습니다(systemd 공식 문서로 재확인: `mixed`는 `TimeoutStopSec`
> 이후 cgroup에 남은 모든 프로세스에 SIGKILL — daemonize해도 cgroup을
> 벗어나지 못함). 즉 tmux는 **fleet-worker 재시작에서 살아남지
> 못합니다** — 이건 버그가 아니라 이 프로젝트가 이미 의도적으로 선택한
> 운영 철학(재시작 시 깔끔한 전체 정리)이라, AskUserQuestion으로 이
> 철학을 유지할지 뒤집을지 확인했고 **유지하기로 결정**했습니다. 그 결과
> §3의 "재시작 시 세션 재발견" 절차를 **폐기**했습니다(아래 §3, §9 참고) —
> tmux는 이제 "fleet-worker 재시작 생존"이 아니라 **"같은 fleet-worker
> 수명 동안의 모니터링/attach"로 가치 제안을 좁혀 확정**합니다.
> **개정 (2026-08-14, 4차 — 세션 재활용 여부 논의)**: 사용자가 "에이전트
> 종료 시 tmux 세션을 재활용할지, 아니면 없앨지" 직접 질문 — **1:1
> 테어다운 유지로 확정**(재활용/풀링 안 함, §2). 시작 지연을 줄이려는
> 의도의 "웜풀" 대안도 함께 검토했으나, 그건 tmux 재활용이 아니라 완전히
> 별개의(그리고 더 큰) 기능이라 이번 스코프에서는 설계하지 않고 §9에
> 향후 후보로만 기록했습니다.
> 아직 구현되지 않았습니다.

## 1. 배경 및 요구사항

사용자 요청 원문: "worker의 동작을 tmux로 터미널 동작을 모니터링하고 cli로
직접 연결하는 것을 지원하고 싶다."

`#49` Phase 4가 `GrokRunner`를 host당 프로세스 1개에서 `agent_id` 키드
다중 프로세스 레지스트리로 재작성하면서, 다음 문제가 함께 딸려 왔습니다
(`agent-provisioning-design.md` §13에서 이미 "다중 프로세스 로그 수집
부재"로 지적):

- 현재 `GrokRunner`는 grok의 stdout/stderr를 `fleet-worker` 자신에게 그냥
  상속시킬 뿐(`crates/fleet-worker/src/grok_process.rs`), 캡처/로그
  파일화가 전혀 없습니다. 프로세스가 여러 개가 되면 전부 뒤섞입니다.
- 운영자가 "지금 이 에이전트가 실제로 뭘 하고 있는지" 눈으로 볼 방법이
  없습니다(태스크 상태만 보임, grok 자체의 출력은 안 보임).
- 문제 재현/디버깅 시 프로세스에 직접 개입(재시작 없이 값 확인, 수동
  명령 전송 등)할 방법도 없습니다.

## 2. 핵심 설계 결정

| 결정 사항 | 채택안 | 근거 |
|---|---|---|
| 연결 방식 | **하이브리드** — 기본은 읽기 전용 모니터링(오케스트레이터가 `tmux capture-pane`을 주기적으로 가져와 노출), 완전한 인터랙티브 제어가 필요할 때만 권한 있는 운영자가 SSH+tmux attach로 에스컬레이션 | AskUserQuestion으로 확인(2026-08-14). 상시 인터랙티브 채널은 보안 범위가 넓고 구현 비용도 큼 — 대부분의 필요는 "지금 뭐 하고 있나 보기"이므로 그것부터 싸게 제공 |
| 적용 범위 | **`#49` 이후부터**(에이전트별 다중 프로세스) | AskUserQuestion으로 확인. 지금(단일 grok 프로세스, 워커당 1개)에 굳이 붙일 이유가 약하고, 로그 수집 문제 자체가 다중 프로세스가 되면서 생기는 문제라 자연히 `#49` Phase 4와 한 몸으로 설계 |
| tmux 세션 소유 위치 | **호스트 위, `GrokRunner`가 grok을 tmux 세션 안에서 spawn** | 운영자가 호스트에 SSH로 직접 붙어 `tmux attach`만 해도 동작하는 최소 경로를 항상 보장 — 오케스트레이터가 죽어 있어도 로컬 디버깅 가능 |
| 인터랙티브 attach의 실제 경유 경로 | **오케스트레이터가 SSH 홉을 대신 함**(운영자가 직접 호스트 SSH 키를 갖지 않음) | 기존 SSH 키 볼트(`#`호스트 프로비저닝, `fleet-dashboard/src/provisioning.rs`)가 이미 서버 사이드에서만 복호화되는 모델 — 운영자에게 원본 개인키를 노출하지 않는 기존 보안 경계를 그대로 유지 |
| tmux를 grok 실행의 기본 envelope로 삼을지, 태스크 디스패치 메커니즘까지 바꿀지 | **실행 envelope만** — 태스크 디스패치는 기존 ACP WebSocket을 그대로 유지, tmux는 grok 프로세스의 생명주기(spawn/종료)와 보조 모니터링만 담당 | 2026-08-14 후속 논의(AskUserQuestion)로 확인. ACP를 tmux 터미널 상호작용(`send-keys`/`capture-pane`)으로 대체하는 안도 검토했으나, 구조화된 프로토콜(스트리밍 부분결과·도구호출 가시성·세션관리)을 raw 터미널 파싱으로 바꾸는 건 훨씬 큰 체급의 변경이라 기각 |
| `fleet-worker` 재시작에도 grok/tmux가 살아남게 할지 | **아니오 — 기존 운영 철학(`KillMode=mixed`, 재시작 시 전부 정리) 유지** | 2026-08-14 3차 개정, 실제 검증 후 AskUserQuestion으로 확인. `examples/fleet-worker.service`가 이미 `KillMode=mixed` + "grok 서브프로세스도 함께 종료" 주석으로 의도적으로 선택한 설계였음을 발견 — 이걸 `KillMode=process`로 뒤집으면 systemd의 고아 프로세스 자동 정리 안전망이 사라져 `#49` `GrokRunner`가 그 책임을 전부 떠안아야 함. tmux의 가치 제안을 "재시작 생존"에서 "같은 fleet-worker 수명 동안의 모니터링/attach"로 좁힘 — §3의 "재시작 시 세션 재발견" 절차는 폐기(§3 참고) |
| 에이전트 종료 시 tmux 세션(=grok 프로세스)을 재활용할지 | **아니오 — 1:1 테어다운 유지, 재활용/풀링 안 함** | 2026-08-14 4차 개정, AskUserQuestion으로 확인. 사용자가 직접 "에이전트 없이 tmux 세션만 남는 건 문제"라고 정확히 지적 — 이는 `#49` §4.1의 "자동 생성은 반드시 자동 회수와 짝을 이뤄야 한다" 원칙 및 이 문서가 방금 확정한 `KillMode=mixed`("깔끔한 전체 정리") 철학과도 일관됨. **재활용의 실제 이점(시작 부하 절감)도 근거가 약함**을 함께 확인 — 병목은 `tmux new-session`(수 ms)이 아니라 grok 프로세스 부팅 + `agent_commands`/heartbeat 왕복(최대 15초)이라, tmux 래퍼만 재활용해선 그 지연이 전혀 줄지 않음. 진짜 지연을 줄이려면 "agent_id에 묶이지 않은 범용 grok 웜풀"이라는 완전히 다른(그리고 더 큰) 기능이 필요한데, 이는 §9에 향후 후보로만 기록하고 **이번 스코프에서는 보류**하기로 함 |

## 3. 아키텍처 개요

![Terminal Access Architecture](../assets/diagrams/architecture/agent-terminal-access-architecture.mermaid)

- **세션 명명**: tmux 세션 이름은 `fleet-agent-<agent_id 전체 UUID>` —
  `#49` 5차 개정에서 확정한 "축약 없는 전체 UUID" 원칙과 동일한 이유
  (충돌 방지). `agent_commands`나 `agents` 테이블에 별도 저장하지
  않습니다 — `agent_id`로부터 결정적으로(deterministic) 파생되는 이름이라
  저장할 필요가 없습니다.
- **spawn 변경**: `GrokRunner`가 `Command::new("grok")`로 직접 spawn하던
  것을, `Command::new("tmux").args(["new-session", "-d", "-s", &session_name,
  "--", grok_bin, "agent", "serve", ...])`로 감쌉니다. tmux가 이미 스크롤백
  버퍼·detach/reattach를 다 해주므로, 별도 로그 캡처 인프라를 새로 만들지
  않습니다 — **`#49` §13의 "다중 프로세스 로그 수집 부재" 문제가 이
  변경만으로 사실상 해소됩니다.**
- **종료 변경**: 기존 `terminate_child()`(SIGTERM 대기 → SIGKILL)는 이제
  프로세스가 아니라 **tmux 세션**을 대상으로 합니다 —
  `tmux send-keys -t <session> C-c` 등으로 그레이스풀 종료를 시도한 뒤,
  타임아웃 시 `tmux kill-session -t <session>`. ⚠️ **(2026-08-14 2차 개정
  — 자체 재감사에서 발견) `C-c`(SIGINT)가 grok에 대해 실제로 기존
  SIGTERM 정책과 동등한 그레이스풀 종료로 작동하는지는 검증하지
  않았습니다** — 많은 CLI 도구가 SIGINT를 SIGTERM과 다르게 처리합니다
  (예: 첫 SIGINT는 확인 프롬프트, 두 번째만 종료). "정확히 같은 맥락"이라는
  이전 서술은 과장이었습니다 — §9 참고.
- **호스트 요구사항 변경**: `tmux`가 호스트에 설치돼 있어야 합니다 —
  `fleet-provisioner`의 프로비저닝 플레이북(`crates/fleet-provisioner`)에
  설치 스텝 추가 필요(§7).
- ~~**`fleet-worker` 재시작 시 세션 재발견**~~ — **2026-08-14 3차 개정으로
  폐기.** 원래는 "`fleet-worker` 재시작 후 살아있는 tmux 세션을
  `tmux list-sessions`로 재발견해 레지스트리를 복구"하는 절차였으나,
  실제 검증 결과(§2 표, §9 항목 1) `examples/fleet-worker.service`가
  `KillMode=mixed`를 명시적으로 선택해 grok/tmux가 **항상**
  `fleet-worker`와 함께 종료됩니다 — 이 철학을 유지하기로 결정했으므로
  (AskUserQuestion), 재발견할 세션이 존재하는 상황 자체가 발생하지
  않습니다. `fleet-worker` 기동 시에는 대신 (혹시 남아있을 수 있는 고아
  세션에 대비해) `tmux kill-server`로 이전 세션을 **전부 정리하고
  시작**하는 편이 이 운영 철학과 일관됩니다 — Phase 4 구현 시 반영.
- **동시 생성 레이스(2026-08-14 2차 개정 신설)**: `#49`의 `agent_commands`
  dedup 로직(§4 3단계, agent_id당 프로세스 1개)에 버그가 있거나 레이스가
  발생해 같은 `start` 커맨드가 두 번 실행되면, `tmux new-session -d -s
  <같은 이름>`을 두 번 호출하게 됩니다 — tmux가 이 경우 에러를 내는지,
  기존 세션에 조용히 영향을 주는지(`-A` 플래그 없이는 보통 에러이지만
  실측 안 함) 확인하지 않았습니다.
- **tmux 소켓/권한 설계 없음(2026-08-14 2차 개정 신설)**: `fleet-worker`가
  어떤 유저로 도는지, 기본 소켓을 쓸지 커스텀 소켓(`tmux -S`)을 쓸지,
  호스트에 다른 유저가 있는 다중 사용자 환경에서 세션 접근이
  `fleet-worker` 유저로 격리되는지 전혀 설계하지 않았습니다.

## 4. 읽기 전용 모니터링 프로토콜

> **모니터링의 범위(2026-08-14 신설, 후속 논의에서 명확화)**: `tmux
> capture-pane`이 보여주는 건 **grok 프로세스 자체의 헬스/저수준
> stdout·stderr**(예: 크래시 스택트레이스, 시작 로그)이지, "이 태스크에
> 뭐라고 응답했는지" 같은 **태스크 콘텐츠 수준의 정보가 아닙니다** — 그건
> 이미 ACP 세션/대시보드 태스크 로그가 담당합니다. 이 구분을 명시하는
> 이유는, grok이 ACP로 태스크를 처리하는 동안 실제로 자기 stdout/stderr에
> 뭔가를 찍는지 자체가 아직 **미검증**이기 때문입니다(§9 열린 질문 —
> §9의 "grok의 TTY 인식 동작" 검증과 함께 Phase 0 성격의 확인이 필요).
> capture-pane이 텅 비어 있거나 시작 로그만 있을 가능성도 염두에 둬야
> 합니다 — 이 경우 §4의 가치는 "완전히 멈췄는지/크래시했는지 확인" 수준으로
> 좁혀집니다.

![Read-only Terminal Snapshot Sequence](../assets/diagrams/architecture/agent-terminal-snapshot-sequence.mermaid)

새 인바운드 채널을 만들지 않고, `#49`에서 이미 확정한 **`agent_commands`
큐 + heartbeat 폴링 패턴을 그대로 재사용**합니다:

- `command_type`에 `'capture_terminal'`을 추가(`'start' | 'stop' |
  'capture_terminal'`).
- 운영자가 대시보드/CLI에서 "새로고침"하면 `POST /api/agents/:id/terminal`이
  `agent_commands(type=capture_terminal)`를 큐잉(즉시 반환, `202 Accepted`).
- 다음 heartbeat에 이 커맨드가 `pending_commands`로 내려가면, `fleet-worker`가
  `tmux capture-pane -t <session> -p -S -200`(최근 200줄)를 실행하고, 결과
  텍스트를 기존 ack 엔드포인트의 응답 페이로드로 실어 보냅니다 — 이를 위해
  `POST /v1/workers/agent-commands/:id/ack`의 요청 바디에 `result:
  Option<String>` 필드를 추가합니다(`#49` §4 4단계에서 이미 `error` 필드로
  실패 텍스트를 보내는 것과 대칭 — `capture_terminal`은 성공 시 `result`에
  캡처 텍스트, 실패 시 `error`에 사유).
- 오케스트레이터는 받은 텍스트를 (스키마 변경 없이) 그 요청의 응답으로
  즉시 반환하지 않고 — 요청 자체가 비동기(202)이므로 — 별도로 저장하지
  않고 **다음 폴링(대시보드가 `GET /api/agents/:id/terminal`을 폴링)** 시
  가장 최근 `agent_commands(type=capture_terminal, status=done)`의 `result`를
  읽어 보여줍니다. 지연은 최대 heartbeat 간격(기본 15초) — "지금 뭐 하고
  있나" 확인 용도로는 충분하다고 판단(§12 열린 질문에 실시간성 개선 여지
  기록).
- 권한: `AgentRead`(viewer+) — 읽기 전용이라 기존 메모리 조회와 동일
  등급.

⚠️ **미해결 (2026-08-14 2차 개정 — 자체 재감사에서 발견, §9로 이관):**

- `capture_terminal`은 `start`/`stop`과 성격이 다른 커맨드입니다 —
  `start`/`stop`은 "agent_id당 프로세스 1개"라는 1회성 lifecycle 효과로
  멱등성을 보장했지만, `capture_terminal`은 운영자가 새로고침할 때마다
  **반복 발행**됩니다. 연속 새로고침 시 `pending` 커맨드가 쌓이는 문제
  (오래된 요청 중복 제거? 최신 것만 유지? 전부 순차 실행?)를 전혀
  설계하지 않았습니다.
- `agent_commands.result`(캡처 텍스트)의 크기 제한·보존 정책이 없습니다
  — `agent_memory`에서 이미 지적했던 "무제한 누적" 리스크(`#49` §12)가
  여기서도 그대로 반복될 수 있는데 언급조차 안 했습니다.

## 5. 인터랙티브 Attach 프로토콜

![Interactive Attach Sequence](../assets/diagrams/architecture/agent-terminal-attach-sequence.mermaid)

- 신규 CLI: `fleet agent attach <agent_id>`(`fleet-cli`의 기존 `Workers`/
  `Tasks`/`Agent` 명령 그룹 패턴과 동일 — `agent-provisioning-design.md`
  §10 CLI 표면에 추가).
- 신규 RBAC 권한 **`AgentAttach`**(`agent:attach`) — **`AgentManage`/
  `AgentDelete`보다 상위 등급으로 별도 분리**, 기본 `Admin`만 보유(
  `Operator`도 기본 미보유). 근거: 인터랙티브 attach는 사실상 호스트
  셸 접근과 동급의 권한이라, 기존 "custom_prompt/도구 바인딩 수정"
  (`AgentManage`)이나 "정지/삭제"(`AgentDelete`)보다 훨씬 민감합니다.
- 흐름:
  1. `fleet-cli`가 `POST /api/agents/:id/attach/ws`로 WebSocket 업그레이드
     요청(세션 토큰 인증, `require_permission(AgentAttach)`).
  2. 오케스트레이터가 그 agent의 host에 대해 **기존 SSH 키 볼트로**
     `russh` 세션을 새로 엽니다(`fleet-provisioner::ssh::SshClient` 재사용).
     단, 기존 `RemoteExecutor::exec`는 PTY가 없는 1회성 실행 채널만
     지원하므로, **PTY 채널을 여는 신규 메서드**(예:
     `open_interactive_shell`, `channel.request_pty` + `channel.exec("tmux
     attach -t <session>")`)를 `SshClient`에 추가해야 합니다(신규 구현
     범위). ⚠️ **(2026-08-14 2차 개정 — 자체 재감사에서 발견) 이 메서드
     시그니처는 순수 가정입니다** — `russh` 라이브러리가 실제로
     `request_pty` + `exec` 조합(또는 `request_shell`)을 지원하는지,
     지원한다면 정확한 API 시맨틱이 뭔지 코드/문서로 확인한 적이 없습니다.
     인터랙티브 attach 전체의 실현 가능성이 여기 달려 있습니다 — §9
     최우선 항목.
  3. 오케스트레이터가 이 SSH PTY 채널과 방금 연 WebSocket 사이에서 **raw
     바이트를 그대로 양방향 릴레이**합니다(JSON-RPC 프레이밍 없음 — 기존
     ACP WebSocket 전송과는 별개의, 훨씬 단순한 채널).
  4. `fleet-cli`는 로컬 터미널을 raw 모드로 전환해 stdin을 그대로 WS로
     전송하고, WS에서 받은 바이트를 stdout에 그대로 씁니다. 터미널 리사이즈
     (`SIGWINCH`)는 별도 제어 메시지로 WS에 실어 보내고, 오케스트레이터가
     이를 `channel.window_change`로 SSH 쪽에 반영합니다. ⚠️ **(2026-08-14
     2차 개정 신설) `fleet-cli`의 raw 터미널 모드 자체가 완전히 신규
     구현 범위**입니다(현재 `fleet-cli`는 clap 기반 배치형 CLI라 인터랙티브
     터미널 제어 코드가 전혀 없음) — 크레이트 선택(예: `crossterm`)이나
     크로스플랫폼(Windows 터미널 등) 지원 여부를 전혀 검토하지 않았습니다.
  5. `Ctrl-b d`(tmux 기본 detach 키)로 세션에서 빠져나와도 grok 프로세스는
     계속 실행됩니다 — tmux의 기본 동작을 그대로 활용, 별도 "detach API"
     불필요.
- **감사 로그**: attach 시작/종료를 기존 활동 로그(`#14` §3.7, `FleetEvent`
  append 패턴)에 `agent_attached`/`agent_detached` 카테고리로 기록 —
  누가 언제 어느 agent에 인터랙티브로 접속했는지 추적 가능해야 함(민감한
  작업이므로).
- ⚠️ **미해결 (2026-08-14 2차 개정 신설)**: 프로비저닝에 이미 있는
  `HostKeyPolicy`(`AcceptAll`/`Tofu`/`Strict`, `known_hosts` 기반)를
  이 attach 흐름에서도 재사용하는지, 아니면 매번 별도로 확인하는지
  설계하지 않았습니다 — 같은 신뢰 저장소를 공유해야 정합성이 맞습니다.

## 6. 동시 Attach 정책

tmux는 한 세션에 여러 클라이언트가 동시에 `attach`하는 것을 기본
지원합니다(모든 클라이언트에 같은 화면 미러링). 이 자체는 유용하지만("같이
지켜보기"), **입력까지 공유되면 두 운영자가 동시에 타이핑해 서로 명령을
덮어쓰는 위험**이 있습니다. 그래서:

- 이미 다른 클라이언트가 attach 중인 세션에 새로 붙는 요청은 기본
  **읽기 전용**(`tmux attach -r`, 입력 불가)으로 처리합니다.
- 쓰기 권한이 필요하면 명시적으로 `fleet agent attach <id> --write`를
  요청 — 이 경우 기존 쓰기-attach 클라이언트가 있다면 경고와 함께 승격
  여부를 확인(강제 다운그레이드는 하지 않음, 상세 정책은 §12).

## 7. 호스트 프로비저닝 변경

`fleet-provisioner`의 부트스트랩 플레이북에 `tmux` 패키지 설치 스텝을
추가해야 합니다(`docs/worker-bootstrap/` 문서 계열과 연동 — 이 문서가
소유하지 않는 범위이므로 참조만). 기존 호스트(플레이북 재실행 전)는 tmux가
없을 수 있으므로, `GrokRunner`가 spawn 전에 `tmux -V` 존재 확인 실패 시
`agent.status = Failed`(에러: "host missing tmux — re-run provisioning")로
명확히 실패시키고, 프로세스를 tmux 없이 직접 spawn하는 폴백은 두지
않습니다(폴백을 두면 "이 에이전트는 왜 모니터링이 안 되지"라는 혼란만
남기고, 다중 프로세스 로그 수집 문제가 원점으로 돌아감).

**업그레이드 경로 갱신 필요(2026-08-14 신설)**: `#49` §13의 "기존 단일
워커 배포와의 업그레이드 경로" 항목은 `max_agents=1`/`agent_provisioning_mode=
manual` 기본값만 다뤘는데, 이 문서(`#50`)가 `#49` Phase 4에 얹이면서 tmux가
**새로 필수 의존성**이 됩니다 — `#49` Phase 4 배포 전에 운영자가 전체
호스트 인벤토리(`ui-design.md` §3.9 호스트 인벤토리)에서 tmux 설치 여부를
먼저 확인/일괄 재프로비저닝하도록 안내하는 절차가 필요합니다. `#49` §13에
이 항목을 추가로 반영해야 합니다(교차 참조만, 실제 수정은 `#49` 문서
쪽에서).

## 8. API/CLI 표면 요약

| 표면 | 추가 |
|---|---|
| REST | `POST /api/agents/:id/terminal`(스냅샷 요청 큐잉), `GET /api/agents/:id/terminal`(최근 스냅샷 조회), `POST /api/agents/:id/attach/ws`(WebSocket 업그레이드) |
| CLI | `fleet agent terminal <id>`(스냅샷 조회/폴링), `fleet agent attach <id> [--write]` |
| RBAC | `AgentAttach`(`agent:attach`, 신규, Admin 기본 전용) — `AgentRead`는 스냅샷 조회에 재사용(신규 권한 아님) |
| 워커용 API | `agent_commands.command_type`에 `'capture_terminal'` 추가, `POST /v1/workers/agent-commands/:id/ack` 요청 바디에 `result: Option<String>` 추가 |
| 대시보드 UI | Agent 상세 페이지(`ui-design.md` §3.13)에 "Terminal" 패널 신설 — 상세는 `ui-design.md` 참고 |

## 9. 열린 질문 (2026-08-14 3차 개정 — 최우선 2개 검증 완료)

> 사용자가 "tmux 이슈가 완전히 해결됐나, 숨긴 게 있나"고 직접 반문해
> 2차로 자체 재감사, 3차로 실제 검증(코드/문서/실기기)까지 거쳤습니다.

### 검증 완료

1. ✅ **`russh` PTY+exec 지원 — 확인됨(긍정)**: docs.rs로
   `russh 0.46.0`(`Cargo.lock` 고정 버전) API를 직접 확인 —
   `request_pty`/`request_shell`/`exec`/`window_change`/`data` 전부
   존재. §5의 `open_interactive_shell` 설계 그대로 구현 가능합니다.
2. ✅ **tmux 서버가 `fleet-worker` 재시작에서 살아남는지 — 확인됨(부정) +
   정책 결정**: `examples/fleet-worker.service`가 `KillMode=mixed` +
   "grok 서브프로세스도 함께 종료" 주석으로 이미 의도적으로 선택한
   설계였음을 발견. systemd 공식 문서로 재확인한 결과 `mixed`는
   `TimeoutStopSec` 이후 cgroup에 남은 모든 프로세스에 SIGKILL —
   daemonize해도 cgroup을 벗어나지 못하므로 tmux/grok은 항상
   `fleet-worker`와 함께 죽습니다. AskUserQuestion으로 **기존 철학
   유지를 확정**(`KillMode=process`로 뒤집지 않음) — §2·§3에 반영,
   "재시작 시 세션 재발견" 절차는 폐기했습니다. tmux의 가치 제안은
   "재시작 생존"이 아니라 "같은 fleet-worker 수명 동안의
   모니터링/attach"로 확정됐습니다.

### 여전히 미검증 — 본문에 확정처럼 썼지만 실은 가정인 것

3. **`tmux send-keys C-c`가 grok에 대해 그레이스풀 종료로 작동하는지
   미검증**(§3) — SIGINT를 SIGTERM과 다르게 처리하는 CLI가 흔합니다.
4. **grok의 TTY 인식 동작 + ACP 구동 중 stdout/stderr 실사용 여부
   미확인**(§4) — capture-pane이 비어 있으면 §4의 실질 가치가 "완전히
   멈췄는지" 확인 수준으로 크게 줄어듭니다.
5. **동시 세션 생성 레이스 미검증**(§3) — 같은 `start` 커맨드가 두 번
   실행되면 `tmux new-session`이 어떻게 반응하는지 실측 안 함.

### 여전히 설계하지 않은 갭

6. **`capture_terminal` 커맨드의 큐잉/멱등성 모델**(§4) — 반복 발행되는
   성격이 `start`/`stop`과 달라 기존 dedup 로직을 그대로 못 씀.
7. **`agent_commands.result`(캡처 텍스트)의 크기 제한·보존 정책 없음**(§4)
   — `agent_memory`와 같은 무제한 누적 리스크가 그대로 반복될 수 있음.
8. **tmux 소켓/권한/다중 사용자 격리 설계 없음**(§3).
9. **`fleet-cli` raw 터미널 모드의 크레이트 선택·크로스플랫폼 지원
   미검토**(§5).
10. **`HostKeyPolicy` 재사용 여부 미정**(§5) — 프로비저닝과 attach가
    같은 신뢰 저장소를 쓰는지 불명확.

### 세부 정책 미확정 (심각도 낮음, 설계 자체는 유효)

11. **읽기 전용 스냅샷의 실시간성**(15초 지연) 개선 여부, 스크롤백
    200줄 고정 여부 — 실사용 피드백 이후 결정.
12. **쓰기 attach 승격/충돌 정책 세부화** — §6 "경고 후 확인" 이상의
    강제 다운그레이드/타임아웃 자동 해제 등은 미정.
13. **`SshClient`의 PTY 지원이 `RemoteExecutor` 트레이트에 들어갈지
    별도 분리될지** — 시맨틱 차이(1회성 vs 장수명 양방향)상 분리가
    유력하나 미확정.
14. **tmux 세션 정리(좀비 세션) 스윕 필요 여부** — `fleet-worker` 기동 시
    `tmux kill-server`로 일괄 정리하는 §3의 새 방침으로 사실상 완화됐으나,
    비정상 잔존 세션이 남는 경로가 있는지는 Phase 구현 시 재확인.

**권장 다음 단계**: 3·4·5번(여전히 미검증, 실기기 필요)을 `#49` Phase 0
검증 스파이크와 함께 확인 — 나머지는 설계 자체의 실현 가능성과 무관한
세부 정책이라 Phase 4 구현 착수 시 확정해도 무방합니다.

### 검토했으나 이번 스코프에서 보류한 것 (2026-08-14 4차 개정 신설)

- **agent_id에 묶이지 않은 범용 grok 웜풀**: 에이전트 시작 지연(host
  여유 확인 → `agent_commands` → 다음 heartbeat까지 최대 15초 → grok
  부팅 → register 왕복)을 줄이려면, tmux 세션 재활용이 아니라 미리
  띄워둔 "범용" grok 프로세스 풀을 두고 필요할 때 즉시 claim(agent_commands/
  heartbeat 왕복 생략, DB 갱신만)하는 방식이 필요합니다. 이건 §3의
  1:1 테어다운 결정과 별개의, 훨씬 큰 기능(풀 크기 정책, 헬스체크,
  claim/release 프로토콜, "유휴 프로세스가 계속 떠 있다"는 동일한 우려를
  풀 단위로 다시 안고 감)이라 이번엔 설계하지 않았습니다 — 실사용에서
  콜드스타트 지연이 실제로 운영 문제가 되면 별도 로드맵 항목으로 재검토.

## 관련 문서

- [`docs/roadmap/roadmap.md`](../roadmap/roadmap.md) #50 — 구현 진행 상황 정본.
- [`docs/architecture/agent-provisioning-design.md`](agent-provisioning-design.md) — `#49`,
  이 문서가 전적으로 의존하는 선행 설계(특히 §4 동적 프로비저닝 프로토콜,
  §4.2 전체 생명주기 상태 다이어그램, §13 설치·운영 고려 사항).
- [`docs/ui-dashboard/ui-design.md`](../ui-dashboard/ui-design.md) §3.13 —
  Agent 상세 페이지의 Terminal 패널.
