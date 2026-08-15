---
type: wiki
status: canonical
source: "docs/architecture/agent-terminal-access-design.md"
last_verified: "2026-08-15"
---

# 에이전트 터미널 모니터링·CLI 직접 접속 설계 (tmux 기반)

> 작성일: 2026-08-14. 로드맵 [`#50`](../roadmap/roadmap.md)에 대응하는 설계
> 문서입니다. [`#49` 에이전트 동적 프로비저닝 설계](agent-provisioning-design.md) 위에
> 쌓이는 후속 확장이며, **`#49` Phase 4(`GrokRunner` 다중 프로세스 재작성)에
> 전적으로 의존**합니다 — 그 전까지는 구현 착수 대상이 아닙니다. 아직
> 구현되지 않았습니다. 개정 이력(왜 이렇게 결정했는지, 검증 경위)은
> [`log.md`](log.md)의 "agent-terminal-access-design.md" 절을 참고하세요 —
> 이 문서 본문은 현재 확정된 설계만 담습니다.

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
| 연결 방식 | **하이브리드** — 기본은 읽기 전용 모니터링(오케스트레이터가 `tmux capture-pane`을 주기적으로 가져와 노출), 완전한 인터랙티브 제어가 필요할 때만 권한 있는 운영자가 SSH+tmux attach로 에스컬레이션 | 상시 인터랙티브 채널은 보안 범위가 넓고 구현 비용도 큼 — 대부분의 필요는 "지금 뭐 하고 있나 보기"이므로 그것부터 싸게 제공 |
| 적용 범위 | **`#49` 이후부터**(에이전트별 다중 프로세스) | 지금(단일 grok 프로세스, 워커당 1개)에 굳이 붙일 이유가 약하고, 로그 수집 문제 자체가 다중 프로세스가 되면서 생기는 문제라 자연히 `#49` Phase 4와 한 몸으로 설계 |
| tmux 세션 소유 위치 | **호스트 위, `GrokRunner`가 grok을 tmux 세션 안에서 spawn** | 운영자가 호스트에 SSH로 직접 붙어 `tmux attach`만 해도 동작하는 최소 경로를 항상 보장 — 오케스트레이터가 죽어 있어도 로컬 디버깅 가능 |
| 인터랙티브 attach의 실제 경유 경로 | **오케스트레이터가 SSH 홉을 대신 함**(운영자가 직접 호스트 SSH 키를 갖지 않음) | 기존 SSH 키 볼트(호스트 프로비저닝, `fleet-dashboard/src/provisioning.rs`)가 이미 서버 사이드에서만 복호화되는 모델 — 운영자에게 원본 개인키를 노출하지 않는 기존 보안 경계를 그대로 유지 |
| tmux를 grok 실행의 기본 envelope로 삼을지, 태스크 디스패치 메커니즘까지 바꿀지 | **실행 envelope만** — 태스크 디스패치는 기존 ACP WebSocket을 그대로 유지, tmux는 grok 프로세스의 생명주기(spawn/종료)와 보조 모니터링만 담당 | ACP를 tmux 터미널 상호작용(`send-keys`/`capture-pane`)으로 대체하는 안도 검토했으나, 구조화된 프로토콜(스트리밍 부분결과·도구호출 가시성·세션관리)을 raw 터미널 파싱으로 바꾸는 건 훨씬 큰 체급의 변경이라 기각 |
| `fleet-worker` 재시작에도 grok/tmux가 살아남게 할지 | **아니오 — 기존 운영 철학(`KillMode=mixed`, 재시작 시 전부 정리) 유지** | `examples/fleet-worker.service`가 이미 `KillMode=mixed` + "grok 서브프로세스도 함께 종료" 주석으로 의도적으로 선택한 설계 — `KillMode=process`로 뒤집으면 systemd의 고아 프로세스 자동 정리 안전망이 사라져 `#49` `GrokRunner`가 그 책임을 전부 떠안아야 함. tmux의 가치 제안은 "재시작 생존"이 아니라 "같은 fleet-worker 수명 동안의 모니터링/attach" |
| 에이전트 종료 시 tmux 세션(=grok 프로세스)을 재활용할지 | **아니오 — 1:1 테어다운 유지, 재활용/풀링 안 함** | `#49` §4.1의 "자동 생성은 반드시 자동 회수와 짝을 이뤄야 한다" 원칙 및 `KillMode=mixed`("깔끔한 전체 정리") 철학과 일관. 재활용의 실제 이점(시작 부하 절감)도 근거가 약함 — 병목은 `tmux new-session`(수 ms)이 아니라 grok 프로세스 부팅 + `agent_commands`/heartbeat 왕복(최대 15초)이라, tmux 래퍼만 재활용해선 그 지연이 전혀 줄지 않음. 진짜 지연을 줄이려면 "agent_id에 묶이지 않은 범용 grok 웜풀"이라는 완전히 다른(그리고 더 큰) 기능이 필요 — §9에 향후 후보로만 기록 |

## 3. 아키텍처 개요

![Terminal Access Architecture](../assets/diagrams/architecture/agent-terminal-access-architecture.mermaid)

- **세션 명명**: tmux 세션 이름은 `fleet-agent-<agent_id 전체 UUID>` —
  `#49`에서 확정한 "축약 없는 전체 UUID" 원칙과 동일한 이유(충돌 방지).
  `agent_commands`나 `agents` 테이블에 별도 저장하지 않습니다 —
  `agent_id`로부터 결정적으로(deterministic) 파생되는 이름이라 저장할
  필요가 없습니다.
> ⚠️ **[major, 팀 검토] 용어 정합성**: 이 절 전체에서 "`GrokRunner`"라는
> 명칭은 `#52`(`agent-runtime-vendor-design.md`) 이전에 쓰던 구체 타입
> 이름입니다 — `#52`는 이를 `AgentRunner` 트레잇(`spawn`/`terminate`/
> `capture_snapshot` 세 메서드만 정의) + 벤더별 구현체(`NetworkBindRunner`/
> `StdioBridgeRunner`)로 대체했고, tmux 매핑은 두 구현체 모두에 적용됩니다
> (`#52` §4). 아래 서술에서 "`GrokRunner`"는 모두 **"`AgentRunner` 구현체
> (`NetworkBindRunner`/`StdioBridgeRunner`)"**로 읽으세요. 또한 아래 2번의
> "주기적으로 tmux를 폴링하는 컴포넌트"는 `#52`가 정의한 `AgentRunner`
> **트레잇의 새 공개 메서드가 아니라, 각 구현체의 `spawn()` 내부에서
> 시작되는 백그라운드 폴링 태스크**입니다 — `#52`의 트레잇 표면(3개
> 메서드)은 그대로 유지되고, 죽음을 감지했을 때 재시작 여부를 판단하는
> 로직만 그 구현체 내부에 새로 생깁니다(트레잇 밖에서 별도로 호출하는
> 컴포넌트가 아님).

- **spawn 변경**: `AgentRunner` 구현체가 `Command::new("grok")`로 직접
  spawn하던 것을, `Command::new("tmux").args(["new-session", "-d", "-s",
  &session_name, "--", grok_bin, "agent", "serve", ...])`로 감쌉니다.
  tmux가 이미 스크롤백 버퍼·detach/reattach를 다 해주므로, 별도 로그 캡처
  인프라를 새로 만들지 않습니다 — `#49` §13의 "다중 프로세스 로그 수집
  부재" 문제가 이 변경만으로 사실상 해소됩니다.
- ⚠️ **생존 감지 방식 전면 재설계(팀 검토 critical 수정)**: 이전 설계는
  기존 `AgentRunner` 구현체가 `tokio::process::Child`를 `child.wait()`해
  종료 코드로 재시작 여부를 판단하던 방식을 그대로 유지한다고 암묵적으로
  전제했지만, **`tmux new-session -d`는 세션을 만들자마자(수십~수백 ms
  내로 추정 — 아직 실측하지 않은 값입니다, §9 참고) 종료 코드 0으로
  리턴하는 명령입니다**(`-d`가 "detach"인 이유) — `child.wait()`가 실제로
  잡는 건 grok 프로세스가 아니라 이 순간적으로 끝나는 tmux 런처이므로,
  기존 재시작 정책(종료 코드로 재시작 여부 판단)이 통째로 무력화됩니다.
  그래서 생존 감지 자체를 다시 설계합니다:
  1. 세션 생성 직후 `tmux set-option -t <session> remain-on-exit on`을
     실행합니다 — grok이 죽어도 pane이 즉시 사라지지 않고 마지막 화면을
     유지합니다(tmux 기본값이 `off`라는 것과 아래 2번의 `pane_dead`/
     `pane_dead_status` 포맷 변수가 의도대로 동작한다는 것은 **아직 공식
     문서나 실기기로 확인하지 않았습니다** — §9 "실기기 검증 필요"로
     이관, 팀 검토에서 발견. 이대로 두면 §4가 예로 든 "크래시 스택트레이스
     캡처" 유스케이스 자체가 성립하지 않을 수 있습니다 — 팀 검토 major,
     같이 수정).
  2. `AgentRunner` 구현체가 이 세션에 대해 주기적으로(예: 2~5초 간격)
     `tmux list-panes -t <session> -F '#{pane_dead} #{pane_dead_status}'`로
     생존 여부와 종료 코드를 폴링합니다(구현체 내부 백그라운드 태스크 —
     위 안내 참고) — `child.wait()`를 완전히 대체합니다.
  3. `pane_dead == 1`을 감지하면: (a) 필요하면 `capture_terminal`과 같은
     방식으로 마지막 화면을 캡처해 실패 사유로 남기고, (b) `#{pane_dead_status}`
     값으로 기존과 동일한 재시작 판단(0 → 정상 종료로 간주, 재시작 안 함
     / 0 아님 → `restart_delay_secs` 후 재spawn)을 적용한 뒤, (c)
     `tmux kill-session -t <session>`으로 실제 정리하고 필요하면 새
     세션으로 재spawn합니다.
- **종료 변경(의도된 종료)**: 기존 `terminate()`(`#52` `AgentRunner`
  트레잇의 공개 메서드, 이전 명칭 `terminate_child()`)는 이제 프로세스가
  아니라 **tmux 세션**을 대상으로 합니다 — `tmux send-keys -t <session>
  C-c` 등으로 그레이스풀 종료를 시도한 뒤, 타임아웃 시 `tmux kill-session
  -t <session>`. 이때는 위 폴링 루프가 "의도된 종료였다"는 걸 알 수
  있도록(재시작 로직이 오작동하지 않도록) 종료를 시작하기 전에 그 세션을
  폴링 대상에서 먼저 제외합니다. `C-c`(SIGINT)가 grok에 대해 실제로 기존
  SIGTERM 정책과 동등한 그레이스풀 종료로 작동하는지는 아직 검증하지
  않았습니다(§9).
- **호스트 요구사항**: `tmux`가 호스트에 설치돼 있어야 합니다 —
  `fleet-provisioner`의 프로비저닝 플레이북(`crates/fleet-provisioner`)에
  설치 스텝 추가 필요(§7).
- **`fleet-worker` 기동 시 이전 세션 일괄 정리**: `KillMode=mixed` 정책상
  grok/tmux는 항상 `fleet-worker`와 함께 종료되므로(§2), 재시작 후
  "살아있는 세션을 재발견"하는 절차는 불필요합니다. 대신 혹시 남아있을 수
  있는 고아 세션에 대비해 `fleet-worker` 기동 시 `tmux kill-server`로
  이전 세션을 **전부 정리하고 시작**합니다 — 이 운영 철학과 일관됩니다.

## 4. 읽기 전용 모니터링 프로토콜

> **모니터링의 범위**: `tmux capture-pane`이 보여주는 건 **grok 프로세스
> 자체의 헬스/저수준 stdout·stderr**(예: 크래시 스택트레이스, 시작
> 로그)이지, "이 태스크에 뭐라고 응답했는지" 같은 **태스크 콘텐츠 수준의
> 정보가 아닙니다** — 그건 이미 ACP 세션/대시보드 태스크 로그가 담당합니다.
> grok이 ACP로 태스크를 처리하는 동안 실제로 자기 stdout/stderr에 뭔가를
> 찍는지 자체가 아직 **미검증**입니다(§9) — capture-pane이 텅 비어 있거나
> 시작 로그만 있을 가능성도 염두에 둬야 합니다. 이 경우 §4의 가치는
> "완전히 멈췄는지/크래시했는지 확인" 수준으로 좁혀집니다.

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
- 오케스트레이터는 받은 텍스트를 별도로 저장해뒀다가, **다음 폴링(대시보드가
  `GET /api/agents/:id/terminal`을 폴링)** 시 가장 최근
  `agent_commands(type=capture_terminal, status=done)`의 `result`를 읽어
  보여줍니다. 지연은 최대 heartbeat 간격(기본 15초) — "지금 뭐 하고 있나"
  확인 용도로는 충분하다고 판단(§9에 실시간성 개선 여지 기록).
- 권한: `AgentRead`(viewer+) — 읽기 전용이라 기존 메모리 조회와 동일 등급.

`capture_terminal`은 `start`/`stop`과 성격이 다른 커맨드입니다 —
`start`/`stop`은 "agent_id당 프로세스 1개"라는 1회성 lifecycle 효과로
멱등성을 보장했지만, `capture_terminal`은 운영자가 새로고침할 때마다
반복 발행됩니다. 연속 새로고침 시 `pending` 커맨드가 쌓이는 문제(오래된
요청 중복 제거? 최신 것만 유지? 전부 순차 실행?)와 `agent_commands.result`의
크기 제한·보존 정책은 아직 설계하지 않았습니다(§9).

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
> ⚠️ **[major, 팀 검토] 배포 전제조건**: `/api/agents/:id/attach/ws`는
> 대시보드 API 네임스페이스(`/api/*`)라 프로덕션 nginx 게이트웨이의
> `location /`(8082 프록시) 블록을 탑니다. 그런데 기존 nginx 설정
> (`docs/deployment/nginx-gateway.md`, `docs/deployment/deployment.md`)은
> 모든 프록시 location에서 `proxy_set_header Connection "";`로 Connection
> 헤더를 비우고 `Upgrade` 헤더 자체를 전달하지 않습니다 — 이대로면
> WebSocket 업그레이드 핸드셰이크가 백엔드에 도달하지 못해 attach 기능이
> 전혀 동작하지 않습니다. **`#50` 구현 시 nginx 설정에
> `proxy_set_header Upgrade $http_upgrade; proxy_set_header Connection
> $connection_upgrade;`(맵 변수 포함) 추가가 선행돼야 합니다** — 배포
> 문서 갱신은 이 문서의 소유 범위가 아니므로 여기서는 전제조건으로만
> 기록하고, `#50` Phase 구현 착수 시 `docs/deployment/nginx-gateway.md`/
> `deployment.md`를 함께 갱신합니다.

- 흐름:
  1. `fleet-cli`가 `POST /api/agents/:id/attach/ws`로 WebSocket 업그레이드
     요청(세션 토큰 인증, `require_permission(AgentAttach)`).
  2. 오케스트레이터가 그 agent의 host에 대해 **기존 SSH 키 볼트로**
     `russh` 세션을 새로 엽니다(`fleet-provisioner::ssh::SshClient` 재사용).
     기존 `RemoteExecutor::exec`는 PTY가 없는 1회성 실행 채널만 지원하므로,
     **PTY 채널을 여는 신규 메서드**(`open_interactive_shell`,
     `channel.request_pty` + `channel.exec("tmux attach -t <session>")`)를
     `SshClient`에 추가합니다 — `russh 0.46.0`(`Cargo.lock` 고정 버전)
     API를 docs.rs로 확인한 결과 `request_pty`/`request_shell`/`exec`/
     `window_change`/`data`가 전부 **존재함을 확인했습니다**(⚠️ 팀 검토로
     표현 정정 — "그대로 구현 가능함을 검증했다"는 API 시그니처 존재
     확인과 실제 동작 검증을 동일시한 과대 주장이었습니다. `request_pty` +
     `exec("tmux attach -t <session>")` 조합이 실제로 양방향 인터랙티브
     스트림을 만들어내는지는 §9 4번의 미검증 항목입니다).
  3. 오케스트레이터가 이 SSH PTY 채널과 방금 연 WebSocket 사이에서 **raw
     바이트를 그대로 양방향 릴레이**합니다(JSON-RPC 프레이밍 없음 — 기존
     ACP WebSocket 전송과는 별개의, 훨씬 단순한 채널).
  4. `fleet-cli`는 로컬 터미널을 raw 모드로 전환해 stdin을 그대로 WS로
     전송하고, WS에서 받은 바이트를 stdout에 그대로 씁니다. 터미널 리사이즈
     (`SIGWINCH`)는 별도 제어 메시지로 WS에 실어 보내고, 오케스트레이터가
     이를 `channel.window_change`로 SSH 쪽에 반영합니다. `fleet-cli`의 raw
     터미널 모드 자체는 완전히 신규 구현 범위입니다(현재 `fleet-cli`는
     clap 기반 배치형 CLI라 인터랙티브 터미널 제어 코드가 전혀 없음) —
     크레이트 선택(예: `crossterm`)이나 크로스플랫폼 지원 여부는 Phase 구현
     시 확정(§9).
  5. `Ctrl-b d`(tmux 기본 detach 키)로 세션에서 빠져나와도 grok 프로세스는
     계속 실행됩니다 — tmux의 기본 동작을 그대로 활용, 별도 "detach API"
     불필요.
- **감사 로그**: attach 시작/종료를 기존 활동 로그(`FleetEvent` append
  패턴)에 `agent_attached`/`agent_detached` 카테고리로 기록 — 누가 언제
  어느 agent에 인터랙티브로 접속했는지 추적 가능해야 함(민감한 작업이므로).
- 프로비저닝에 이미 있는 `HostKeyPolicy`(`AcceptAll`/`Tofu`/`Strict`,
  `known_hosts` 기반)를 이 attach 흐름에서도 재사용할지는 아직 설계하지
  않았습니다(§9) — 같은 신뢰 저장소를 공유해야 정합성이 맞습니다.

## 6. 동시 Attach 정책

tmux는 한 세션에 여러 클라이언트가 동시에 `attach`하는 것을 기본
지원합니다(모든 클라이언트에 같은 화면 미러링). 이 자체는 유용하지만("같이
지켜보기"), **입력까지 공유되면 두 운영자가 동시에 타이핑해 서로 명령을
덮어쓰는 위험**이 있습니다. 그래서:

- 이미 다른 클라이언트가 attach 중인 세션에 새로 붙는 요청은 기본
  **읽기 전용**(`tmux attach -r`, 입력 불가)으로 처리합니다.
- 쓰기 권한이 필요하면 명시적으로 `fleet agent attach <id> --write`를
  요청 — 이 경우 기존 쓰기-attach 클라이언트가 있다면 경고와 함께 승격
  여부를 확인(강제 다운그레이드는 하지 않음, 상세 정책은 §9).

## 7. 호스트 프로비저닝 변경

`fleet-provisioner`의 부트스트랩 플레이북에 `tmux` 패키지 설치 스텝을
추가해야 합니다(`docs/worker-bootstrap/` 문서 계열과 연동 — 이 문서가
소유하지 않는 범위이므로 참조만). 기존 호스트(플레이북 재실행 전)는 tmux가
없을 수 있으므로, `AgentRunner` 구현체가 spawn 전에 `tmux -V` 존재 확인 실패 시
`agent.status = Failed`(에러: "host missing tmux — re-run provisioning")로
명확히 실패시키고, 프로세스를 tmux 없이 직접 spawn하는 폴백은 두지
않습니다(폴백을 두면 "이 에이전트는 왜 모니터링이 안 되지"라는 혼란만
남기고, 다중 프로세스 로그 수집 문제가 원점으로 돌아감).

**업그레이드 경로**: `#49` §13의 "기존 단일 워커 배포와의 업그레이드
경로" 항목은 `max_agents=1`/`agent_provisioning_mode=manual` 기본값만
다뤘는데, 이 문서(`#50`)가 `#49` Phase 4에 얹이면서 tmux가 **새로 필수
의존성**이 됩니다 — `#49` Phase 4 배포 전에 운영자가 전체 호스트
인벤토리(`ui-design.md` §3.2.5, `#49` 문서 검토에서 발견한 것과 동일한
§3.9 오기 수정)에서 tmux 설치 여부를 먼저 확인/일괄
재프로비저닝하도록 안내하는 절차가 필요합니다.

## 8. API/CLI 표면 요약

| 표면 | 추가 |
|---|---|
| REST | `POST /api/agents/:id/terminal`(스냅샷 요청 큐잉), `GET /api/agents/:id/terminal`(최근 스냅샷 조회), `POST /api/agents/:id/attach/ws`(WebSocket 업그레이드) |
| CLI | `fleet agent terminal <id>`(스냅샷 조회/폴링), `fleet agent attach <id> [--write]` |
| RBAC | `AgentAttach`(`agent:attach`, 신규, Admin 기본 전용) — `AgentRead`는 스냅샷 조회에 재사용(신규 권한 아님) |
| 워커용 API | `agent_commands.command_type`에 `'capture_terminal'` 추가, `POST /v1/workers/agent-commands/:id/ack` 요청 바디에 `result: Option<String>` 추가 |
| 대시보드 UI | Agent 상세 페이지(`ui-design.md` §3.13)에 "Terminal" 패널 신설 — 상세는 `ui-design.md` 참고 |

## 9. 열린 질문

`tmux` 서버 생존성(§2, §3)은 실제로 검증해 확정했습니다(경위는
[`log.md`](log.md)). `russh` PTY 지원(§5)은 **API가 docs.rs에 존재한다는
것만** 확인했습니다 — 아래 4번 참고(⚠️ 팀 검토 minor로 정정, 이전 서술은
"구현 가능함을 검증했다"고 과대 주장했습니다). 아래는 여전히 남은
항목입니다 — 대부분 실기기(grok 바이너리·실제 호스트)가 있어야 확인
가능해 `#49` Phase 0 검증 스파이크와 함께 확인할 예정입니다.

### 실기기 검증 필요

1. **`tmux send-keys C-c`가 grok에 대해 그레이스풀 종료로 작동하는지**
   (§3) — SIGINT를 SIGTERM과 다르게 처리하는 CLI가 흔합니다.
2. **grok의 TTY 인식 동작 + ACP 구동 중 stdout/stderr 실사용 여부**(§4) —
   capture-pane이 비어 있으면 §4의 실질 가치가 "완전히 멈췄는지" 확인
   수준으로 줄어듭니다.
3. **동시 세션 생성 레이스**(§3) — 같은 `start` 커맨드가 두 번 실행되면
   `tmux new-session`이 어떻게 반응하는지 실측 필요.
3.5. **§3 생존 감지 메커니즘의 세부 동작 3가지**(⚠️ 팀 검토 major로 신설
   — 이전엔 검증 표시 없이 확정 사실처럼 서술됐습니다): (a)
   `tmux new-session -d`가 실제로 "수십~수백ms 내" 종료 코드 0으로
   리턴하는지(정확한 시간 범위는 추정치), (b) tmux의 `remain-on-exit`
   기본값이 실제로 `off`인지(버전별 차이 가능성), (c)
   `tmux list-panes -F '#{pane_dead} #{pane_dead_status}'` 포맷 변수
   조합이 실제로 죽음/종료코드를 정확히 보고하는지. §3의 생존 감지
   전면 재설계 전체가 이 3가지 위에 서 있으므로, `russh` API처럼
   공식 문서로 확인하거나(가능하면) 실기기로 검증해야 합니다.
4. **`request_pty` 뒤에 `exec("tmux attach -t <session>")`를 호출하는
   조합이 실제로 양방향 인터랙티브 PTY 스트림을 만들어내는지**(§5,
   ⚠️ 팀 검토 minor로 신설) — docs.rs에서 확인한 건
   `request_pty`/`request_shell`/`exec`/`window_change`/`data` 메서드
   시그니처가 "존재한다"는 것뿐이고, 이 조합의 실제 동작 시맨틱(pty
   요청과 exec 조합, tmux attach와의 상호작용)은 검증되지 않았습니다 —
   "구현 가능함을 검증했다"가 아니라 "API는 존재를 확인했다"로 표현을
   낮춥니다.

### 아직 설계하지 않은 것

4. **`capture_terminal` 커맨드의 큐잉/멱등성 모델**(§4).
5. **`agent_commands.result`(캡처 텍스트)의 크기 제한·보존 정책**(§4).
   ⚠️ **[major, 팀 검토] 그보다 앞서 컬럼 자체가 마이그레이션 계획에
   없음**을 확인했습니다 — `agent_commands` 테이블(`#49` §3
   `016_agents.sql`)에는 `id, host_id, agent_id, command_type, status,
   error, created_at, acked_at`만 있고 `result` 컬럼이 없습니다.
   `#48`(015)→`#49`(016)→`#51`(017)→`#52`(018)가 각각 마이그레이션
   번호를 예약한 것과 달리 `#50`은 스키마 변경이 필요함에도 자체
   마이그레이션 계획이 없었습니다 — 구현 시 `019_agent_commands_result.sql`
   (가칭, `ALTER TABLE agent_commands ADD COLUMN result TEXT;`)로 신규
   예약해야 합니다.
6. **tmux 소켓/권한/다중 사용자 격리**(§3) — `fleet-worker`가 어떤
   유저로 도는지, 커스텀 소켓(`tmux -S`)을 쓸지 등.
7. **`fleet-cli` raw 터미널 모드의 크레이트 선택·크로스플랫폼 지원**(§5).
8. **`HostKeyPolicy` 재사용 여부**(§5).

### 세부 정책 미확정 (심각도 낮음, 설계 자체는 유효)

9. **읽기 전용 스냅샷의 실시간성**(15초 지연) 개선 여부, 스크롤백 200줄
   고정 여부 — 실사용 피드백 이후 결정.
10. **쓰기 attach 승격/충돌 정책 세부화** — §6 "경고 후 확인" 이상의
    강제 다운그레이드/타임아웃 자동 해제 등은 미정.
11. **`SshClient`의 PTY 지원이 `RemoteExecutor` 트레이트에 들어갈지
    별도 분리될지** — 시맨틱 차이(1회성 vs 장수명 양방향)상 분리가
    유력하나 미확정.
12. **tmux 세션 정리(좀비 세션) 스윕 필요 여부** — `fleet-worker` 기동 시
    `tmux kill-server`로 일괄 정리하는 §3 방침으로 사실상 완화됐으나,
    비정상 잔존 세션이 남는 경로가 있는지는 Phase 구현 시 재확인.

### 검토했으나 이번 스코프에서 보류한 것

- **agent_id에 묶이지 않은 범용 grok 웜풀**: 에이전트 시작 지연(host
  여유 확인 → `agent_commands` → 다음 heartbeat까지 최대 15초 → grok
  부팅 → register 왕복)을 줄이려면, tmux 세션 재활용이 아니라 미리
  띄워둔 "범용" grok 프로세스 풀을 두고 필요할 때 즉시 claim(agent_commands/
  heartbeat 왕복 생략, DB 갱신만)하는 방식이 필요합니다. 이건 §2의 1:1
  테어다운 결정과 별개의, 훨씬 큰 기능(풀 크기 정책, 헬스체크, claim/release
  프로토콜, "유휴 프로세스가 계속 떠 있다"는 동일한 우려를 풀 단위로 다시
  안고 감)이라 설계하지 않았습니다 — 실사용에서 콜드스타트 지연이 실제로
  운영 문제가 되면 별도 로드맵 항목으로 재검토.

## 관련 문서

- [`docs/roadmap/roadmap.md`](../roadmap/roadmap.md) #50 — 구현 진행 상황 정본.
- [`docs/architecture/log.md`](log.md) — 이 설계에 도달한 경위(개정 이력,
  검증 결과).
- [`docs/architecture/agent-provisioning-design.md`](agent-provisioning-design.md) — `#49`,
  이 문서가 전적으로 의존하는 선행 설계(특히 §4 동적 프로비저닝 프로토콜,
  §4.2 전체 생명주기 상태 다이어그램, §13 설치·운영 고려 사항).
- [`docs/ui-dashboard/ui-design.md`](../ui-dashboard/ui-design.md) §3.13 —
  Agent 상세 페이지의 Terminal 패널.
