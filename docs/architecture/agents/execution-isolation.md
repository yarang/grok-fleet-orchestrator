---
type: architecture
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/agents/execution-isolation.md"
last_verified: "2026-09-02"
last_verified_commit: "working-tree"
---

# Agent 실행 격리

## 책임

이 문서는 위험도에 따른 실행 격리 선택, immutable snapshot, cleanup 범위를 정한다. Agent
생성 절차는 [프로비저닝](provisioning.md), terminal 접근은 [터미널 접근](terminal-access.md)이
담당한다.

## 결정

| isolation | 선택 조건 | 필수 경계 |
|---|---|---|
| `host_trusted` | 신뢰된 단일 Project의 저위험 작업 | 전용 `fleet` 사용자, Project별 workspace·tmux socket, 최소 권한 |
| `container_required` | 다중 Project, 신뢰되지 않은 입력, 높은 영향도 중 하나 | rootless·비특권 container, read-only base, 제한 mount/egress, 자원 제한 |

스케줄러와 Worker는 더 약한 격리로 fallback하지 않는다. `container_required` 실행에 host tmux
attach를 제공하지 않는다. 더 강한 격리로 다시 시도하려면 새 Task를 만들고 이유를 남긴다.

공유 Host의 기본값은 `container_required`다. `host_trusted`는 Host가 실행 시점에 단일 Project만
수용하고, 운영자가 그 사실을 증명하는 exclusive mode일 때만 선택할 수 있다. Agent 수 상한이나
Project grant는 isolation을 대체하지 않는다.

## Container 실행 프로파일

`container_required` Task는 image digest로 고정된 rootless·비특권 container에서만 시작한다.
Worker는 아래 profile을 구조화된 snapshot으로 runtime adapter에 넘기며, prompt나 임의 CLI 인자로
mount·network·권한을 바꾸게 하지 않는다.

| 영역 | 기본 규칙 | 예외 승인 |
|---|---|---|
| 사용자·권한 | root 금지, privilege/capability 추가 금지, host PID/IPC/UTS namespace 공유 금지 | 없음 |
| filesystem | read-only rootfs, 실행 중 Task의 Agent worktree만 read-write bind mount | 승인된 build output/artifact mount만 별도 경로로 허용 |
| host socket/device | Docker/Podman socket, SSH agent, `/dev` 일반 mount, host `/proc`·`/sys` 금지 | capability class에 명시된 단일 device만 Worker policy로 허용 |
| cache | Project/Agent 간 공유 write cache 금지 | content-addressed read-only cache 또는 Task 전용 cache |
| secret | Security Manager가 전달한 tmpfs read-only mount/file descriptor만 | 원문을 env·이미지 layer·workspace에 쓰지 않음 |
| network | egress deny 기본 | Project policy의 egress profile에 등록된 hostname·port·purpose만 허용 |
| ingress | 없음 | terminal attach proxy만 별도 short-lived grant |

egress profile은 `git.agentthread.dev`, 승인된 model gateway, package mirror처럼 목적별 endpoint를
명시한다. raw IP·임의 DNS resolver·metadata endpoint·사설망 대역은 기본 거부한다. DNS 재해결과
redirect도 policy enforcement 지점에서 검사하며, container 내부의 임의 proxy 설정으로 우회할 수
없어야 한다.

## Workspace와 Git 경계

- Worker는 Agent별 worktree를 만들고 해당 Task 실행에는 그 경로만 mount한다. Project root, 다른
  Agent worktree, Worker 설정 디렉터리, credential backend는 mount하지 않는다.
- worktree의 Git remote와 branch/ref는 Task의 execution snapshot으로 고정한다. untrusted Task가 remote,
  hooks, global Git config, credential helper를 바꾸는 것을 금지한다.
- Git push credential은 Security Manager가 실행 구간에 묶어 발급한 credential으로만 제공하며, user global
  credential store·SSH agent forwarding·repository config의 embedded token을 사용하지 않는다.
- cleanup은 opaque workspace/container ID로만 수행하고 canonical path가 Fleet workspace root 아래임을
  확인한다. glob·사용자 입력 path·symlink 추적 삭제는 금지한다.

### 현재 구현 (2026-08-28, 로드맵 `#69` 전제)

위 결정은 아직 구현되지 않았다 — Agent별 worktree를 만들려면 dispatch가 어떤 Agent의 것인지
알아야 하고, 그 바인딩은 `#49` 2단계에 있다. 그때까지 Task의 작업 디렉터리는 **클라이언트가
제출한 `Task.cwd`** 하나이며, Worker가 만들거나 검증한 경로가 아니다.

2026-08-28 이전에는 그 값이 없으면 `AcpTransport::dispatch`가 `PathBuf::from("/")`로 대체해
파일시스템 루트에서 세션을 열었다. 지금은 지어내지 않고 거절한다. 검증은 `fleet-core`의
`validate_workspace_cwd`가 정본이고, 생산 표면 3곳(MCP·Dashboard·CLI)과 `Dispatcher::submit()`,
그리고 transport 두 구현이 같은 규칙을 건다.

**이 검증은 어휘적(lexical)이다.** 절대 경로일 것, `..` 세그먼트가 없을 것, `/` 자체가 아닐 것,
interior NUL이 없을 것만 본다. 위 절이 요구하는 **containment**(canonical path가 Fleet workspace
root 아래인가)는 판정하지 못한다 — 오케스트레이터의 `canonicalize`는 자기 파일시스템을 보므로
워커의 경로에 대해 아무것도 말하지 않고, symlink 해석은 그 경로가 존재하는 쪽에서만 가능하다.
따라서 워커측 relay나 `#64`의 container mount 경계 중 하나가 선행이며, **지금 상태는 "경계가
있다"가 아니라 "지어낸 루트는 더 이상 쓰지 않는다"까지다.**

### `#69` 1단계의 범위 (2026-09-02 확정)

로드맵 `#69`의 완료 게이트는 넷(Agent worktree isolation, checkpoint push/restore, secret scan,
Worker 이동 복구 E2E)인데, 착수 시점에 **셋은 정본에 대상이 없었다.** "secret scan"은 `docs/`
전체에서 로드맵 행 자신 말고는 한 번도 나오지 않고, "checkpoint"는 다섯 문서에 스쳐 지나갈 뿐
소유 절이 없다. 지목이 넷인데 대상이 비어 있으면 지목마다 다른 것을 상상하게 되므로, 여기서
정의한다. 이것은 [프로비저닝](provisioning.md)의 "`#49` 2단계의 범위"가 같은 이유로 한 일이다.

#### 대상의 정의

- **Project Git repository**: Project가 갖는 remote repository 하나와 그 default branch. Project는
  repository를 **갖지 않을 수 있고**, 그때는 지금과 똑같이 동작한다(worktree 없음, `Task.cwd`만).
  이 NULL은 자리표시자가 아니라 "이 Project는 Git을 쓰지 않는다"는 도달 가능한 상태다 — `#67`
  4a가 `agents.worker_id`의 NULL을 다룬 방식과 같다.
- **worktree**: Worker가 `agent_workspace_root/<agent_id>`에 만드는 **checkout**. 지금 그 경로는
  프로세스의 cwd일 뿐이며([프로비저닝](provisioning.md)의 유예 표), 1단계가 그 디렉터리의 규약을
  바꾼다. 유예 표가 "지금 둘을 합치면 `#69`가 자기 설계를 4c-A의 디렉터리 규약에 맞춰야 한다"고
  적어 미뤄 둔 합침을, 여기서 `#69` 쪽 규약으로 한다.
- **checkpoint**: worktree의 작업을 Project repository의 **원격 ref로 올린 것**. 로컬 커밋만으로는
  checkpoint가 아니다 — Worker 이동은 디스크를 바꾸므로 로컬 커밋은 따라가지 않는다. 이 정의가
  아래 표에서 게이트 2·4를 push credential에 묶는다.
- **secret scan**: checkpoint push **직전에** 올라갈 tree/diff를 검사해 credential이 원격으로 나가는
  것을 막는 검사이며, 독립 기능이 아니라 push의 전제 조건이다. 최소 대상은 **Fleet 자신이 그
  worktree에 넣은 credential**이다: 위 결정이 push credential을 "실행 구간에 묶어 발급"한다고
  정했는데, 그 credential이 `.git/config`의 embedded token이나 helper가 쓴 파일로 worktree에 남으면
  다음 checkpoint가 그것을 실어 나른다. 일반적인 사용자 secret 탐지는 그 위에 얹는 것이지 그
  반대가 아니다.

#### 게이트의 실제 선행

로드맵 행은 네 게이트가 **모두** `#49` 2단계(dispatch에 `agent_id`를 싣는 것)에 걸린다고 적었다.
그 2단계는 2026-09-01에 닫혔지만, **선행이 하나였다는 서술은 첫 번째에만 참이다.**

| 게이트 | 실제 선행 | 1단계 |
| --- | --- | --- |
| Agent worktree isolation | `#49` 2단계(닫힘) + Project repository 바인딩 | 닫는다 |
| checkpoint push/restore | push credential을 발급하는 주체 | 아니다 |
| secret scan | checkpoint push가 존재할 것 | 아니다 |
| Worker 이동 복구 E2E | 위 둘 | 아니다 |

"`#49` 2단계가 닫혔으니 `#69`가 열렸다"고 읽으면 나머지 셋을 착수한 뒤에 막힘을 재발견한다.

#### 1단계가 아닌 것

| 미룬 것 | 왜 | 어디로 |
| --- | --- | --- |
| checkpoint push/restore | Git push credential을 발급하는 Security Manager가 없다. 위 결정은 user global credential store·SSH agent forwarding·repository config의 embedded token을 **모두** 금지하므로, 발급자가 없으면 push할 수단 자체가 없다 | 미정(Security Manager를 소유하는 로드맵 항목이 아직 없다) |
| secret scan | 위가 없으면 검사 대상(push될 tree)이 생기지 않는다 | checkpoint push 이후 |
| Worker 이동 복구 E2E | 이동은 디스크를 바꾸므로 원격 ref 없이는 복구할 것이 없다 | 위 둘 이후 |
| worktree containment 판정 | 위 "현재 구현"이 적은 그대로다 — 오케스트레이터의 `canonicalize`는 워커의 파일시스템을 보지 못한다. worktree를 **워커가** 만들면 그 경로는 워커가 정한 것이므로 클라이언트 입력보다 낫지만, 그것은 검증이 아니라 출처의 차이다 | 워커측 relay 또는 `#64` |
| execution snapshot의 remote/ref 고정 | snapshot 필드는 아래 "실행 snapshot과 cleanup"이 소유하고, 그 고정은 dispatcher capability 검증(게이트 1)과 함께 간다 | 게이트 1 |
| Project 하나에 repository 여럿 | 복수는 worktree 배치 규약을 바꾸므로 checkpoint 정의가 굳기 전에 정하면 두 번 바꾸게 된다 | 미정 |

#### Worker는 repository를 어떻게 아는가

worktree를 만들려면 Worker가 Agent의 Project와 그 repository를 알아야 하는데, **지금은 둘 다
모른다.** heartbeat 응답의 `AgentCommand`는 `(agent_id, desired_status, generation)` 셋뿐이고, 그
키 집합은 스키마 테스트가 잠그고 있다. 4c-A는 port·secret·cwd를 **워커 로컬 파생**으로 해결해
이 봉투를 좁게 유지했지만, repository URL은 로컬로 파생할 수 없다.

두 갈래가 있고, 결과가 다르다.

- **봉투를 넓힌다** — `AgentCommand`에 repository를 싣는다. 구현은 가장 짧지만, `HeartbeatResponse`
  주석이 경고한 것이 그대로 실현된다: repository URL은 `https://user:token@host/...` 형태를 가질 수
  있으므로 heartbeat 응답이 **통째로 로깅해도 안전한 값이 아니게 된다.** 그 안전 속성은 지금
  `Worker::endpoint`가 `mask_server_key`를 거치는 이유와 같은 종류이고, 한 번 잃으면 응답을 만지는
  모든 곳이 마스킹 대상이 된다.
- **별도 인증 호출을 만든다** — Worker가 자신에게 배정된 Agent의 workspace 기술을 별도
  엔드포인트로 가져온다. 엔드포인트와 권한 매핑이 늘지만 봉투의 속성이 유지되고, 응답에 secret이
  섞이는 표면이 **한 곳으로 국한된다.**

**후자로 간다.** 근거는 취향이 아니라 비대칭이다 — 전자의 비용은 되돌릴 수 없고(응답의 로깅
안전성은 한 번 깨지면 그 뒤의 모든 코드가 그 전제로 쓰인다), 후자의 비용은 엔드포인트 하나다.

## sudo와 privileged operation

Fleet가 sudo를 쓸 수 있다는 것은 Agent가 임의 `sudo` shell을 얻는다는 뜻이 아니다. container
내 Agent에는 sudo를 설치·허용하지 않는다. Host 수준 privileged 작업은 Worker가 아닌 별도
`fleet-privileged-helper`만 수행한다.

```mermaid
flowchart LR
    Agent["Agent process"] --> Request["structured privileged request"]
    Request --> Policy{"Tool + Project policy\nallow-listed?"}
    Policy -- "no" --> Deny["deny + audit"]
    Policy -- "yes" --> Helper["fleet-privileged-helper\nroot-owned"]
    Helper --> Result["bounded result + audit"]
```

- 허용 operation은 고정된 tool ID와 typed argument schema를 가진다. 임의 shell, path wildcard,
  환경변수 보존, command substitution, interactive sudo는 허용하지 않는다.
- helper는 root-owned executable과 root-owned allow-list를 사용하고, `task_id`, Project scope,
  fencing token, request expiry를 재검증한다.
- privileged operation은 effect ledger에 최소 `Planned`/`Started`/receipt를 기록하고, 결과·전후
  fingerprint·actor를 감사한다. 비가역 operation은 operator approval이 필요하다.
- helper가 없거나 policy·fencing 검증에 실패하면 Task를 약한 격리로 fallback하지 않고 거절한다.

## 실행 snapshot과 cleanup

각 Task 실행은 `execution_isolation`, `isolation_policy_version`, 결정 이유와 요청 principal,
Worker capability, workspace 식별자, runtime/image digest, Skill revision/hash, egress profile,
mount manifest, privileged tool allow-list revision을 고정한다.
Worker는 요구 capability가 없으면 거절한다.

cleanup은 Fleet가 만든 container·socket·session·workspace 식별자에만 적용한다. host 전체
`tmux kill-server`와 공유 workspace 삭제는 금지한다. 취소·TTL 만료·권한 회수 때 실행과 attach
grant를 함께 닫고, Worker는 cleanup 증거를 ACK하기 전 `Stopped`로 전이하지 않는다.
cleanup은 process memory와 실행 자원만 지우며, 승인된 durable Project/Agent context와 audit
snapshot은 [배치·맥락 계약](../entity-placement-and-context.md)에 따라 보존한다.

## 구현 게이트

1. dispatcher capability 검증과 snapshot 고정 통합 시험
2. container의 host socket·권한 상승·비허용 egress 거절 시험
3. cancel·timeout·crash 뒤 다른 Task의 자원을 건드리지 않는 시험
4. 감사 기록만으로 isolation 결정과 실행 위치를 재구성하는 시험
5. 다른 Agent worktree·host socket·credential 원문·metadata/private network 접근 거절 시험
6. 승인되지 않은 sudo/임의 shell/만료된 fencing token의 privileged helper 거절 시험
7. redirect·DNS 재해결을 포함한 egress profile 우회 거절 시험
