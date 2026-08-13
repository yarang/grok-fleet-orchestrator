# 에이전트(Agent) 동적 프로비저닝 · 메모리 · 스레드 요약 설계

> 작성일: 2026-08-14. 로드맵 [`#49`](../roadmap/roadmap.md)에 대응하는 설계 문서입니다.
> [`#48` 프로젝트 기능 설계](project-feature-design.md) 위에 쌓이는 후속 확장입니다 —
> `#48`은 "기존 워커를 프로젝트에 M:N으로 배치"만 다뤘고, 이 문서는 "프로젝트가
> 필요할 때 새 에이전트를 직접 만들어서 쓰는" 능력을 다룹니다. 아직 구현되지
> 않았습니다 — 진행 상황은 `roadmap.md` #49 항목을 정본으로 확인하세요.

## 1. 배경 및 사용자 요구사항 원문 요약

사용자가 2026-08-14에 다음을 요청했습니다(설계에 직접 반영):

1. 하나의 host에서 에이전트가 **여러 개** 동작할 수 있어야 한다.
2. **custom 프롬프트**로 여러 에이전트를 구분해서 쓸 수 있어야 한다.
3. 프로젝트가 **필요한 경우 에이전트를 만들어서 호출**할 수 있어야 한다.
4. 에이전트는 host에 **여유가 있을 때** 만들어서 운영할 수 있어야 한다.
5. 개별 에이전트는 **여러 세션에 걸친 맥락**을 유지해야 한다.
6. 그 맥락은 **메모리**로 관리하고, **프로젝트별로** 스코프돼야 한다.
7. 프로젝트에 속하지 않는 태스크 스레드는 **스레드별 요약**으로 관리한다.
8. 필요하면 결과물을 **디렉토리**로 관리한다.

## 2. 핵심 설계 결정 (사용자 확인 완료, 2026-08-14)

`#48`과 마찬가지로, 구현 착수 전에 스키마·프로토콜 형태를 가르는 4가지를
AskUserQuestion으로 확인했습니다.

| 결정 사항 | 채택안 | 근거 |
|---|---|---|
| Agent ↔ Worker 관계 | **Agent를 신규 엔티티로 도입** (Worker와 분리) | Worker는 "접속/용량/회로차단기" 저수준 개념으로 그대로 두고, Agent가 custom_prompt·메모리·프로젝트 소속 같은 상위 개념을 담당. Agent는 Running 상태일 때만 정확히 하나의 Worker에 연결(1:0..1) |
| 동적 프로비저닝 범위 | **진짜 동적 프로비저닝** — 오케스트레이터가 실행 중 원격으로 grok 프로세스 시작/종료 | 사용자가 명시적으로 "가장 큰 작업이지만 요구사항을 가장 문자 그대로 만족" 옵션을 선택 |
| 메모리 구현 방식 | **구조화된 텍스트/JSON 누적 + 프롬프트 주입** | 기존 `build_threaded_prompt`(스레드 내 이어붙이기) 패턴의 자연스러운 확장. 임베딩/벡터DB 같은 신규 인프라 불필요 |
| 로드맵 항목 | **`#49`로 분리** (`#48`과 독립) | `#48`(스키마/RBAC/소프트 디스패치 필터)과 `#49`(프로세스 수명주기/제어 채널/메모리 저장소)는 기술적 표면이 크게 달라 독립적으로 구현·검증 가능해야 함 |

## 3. 데이터 모델

![Agent Data Model](../assets/diagrams/architecture/agent-data-model.mermaid)

### 신규 타입 (`fleet-core`)

```rust
// crates/fleet-core/src/ids.rs — 기존 TaskId/WorkerId/ProjectId 패턴과 동일
pub struct AgentId(pub Uuid);

// crates/fleet-core/src/agent.rs (신규)
pub struct Agent {
    pub id: AgentId,
    pub host_id: Uuid,
    /// 이 에이전트를 만든/소유하는 프로젝트. `None`이면 범용(일반 풀) 에이전트.
    /// #48의 Worker↔Project M:N 배치와는 별개 축이다 — Agent는 "프로젝트가
    /// 필요해서 만든" 것이므로 기본적으로 자신을 만든 프로젝트에 귀속된다.
    /// (한 에이전트를 여러 프로젝트가 공유하는 케이스는 §9 열린 질문 참고.)
    pub project_id: Option<ProjectId>,
    /// Running 상태가 되면 자기 자신을 등록한 Worker의 ID로 채워진다.
    pub worker_id: Option<WorkerId>,
    pub name: String,
    /// 시스템 프롬프트/페르소나. grok 프로세스 실행 인자가 아니라 **디스패치
    /// 시점에 태스크 프롬프트 앞에 붙이는 텍스트**로 적용된다(§6).
    pub custom_prompt: Option<String>,
    pub status: AgentStatus,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum AgentStatus {
    Pending,   // 생성 요청됨, 아직 커맨드 미발행/미확인
    Starting,  // start 커맨드가 host로 전달됨, grok 프로세스 기동 대기
    Running,   // worker_id 확보, 디스패치 가능
    Stopping,  // stop 커맨드 전달됨
    Stopped,   // 정상 종료
    Failed,    // 기동 실패(예: host 용량 초과, grok spawn 실패)
}

pub struct AgentMemoryEntry {
    pub id: Uuid,
    pub agent_id: AgentId,
    pub kind: String,           // "note" | "summary" | "fact" — 1단계는 자유 텍스트, 종류는 태그 정도로만 사용
    pub content: String,
    pub source_task_id: Option<TaskId>,
    pub created_at: DateTime<Utc>,
}

pub struct ThreadSummary {
    pub thread_id: TaskId,      // tasks.thread_id 값을 그대로 키로 사용(별도 PK 엔티티 아님)
    pub summary: String,
    pub turn_count: u32,
    pub updated_at: DateTime<Utc>,
}
```

### 신규 마이그레이션 (`016_agents.sql`, `#48`의 `015_projects.sql` 다음 번호)

```sql
CREATE TABLE agents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    host_id UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    worker_id UUID REFERENCES workers(id) ON DELETE SET NULL,
    name TEXT NOT NULL UNIQUE,
    custom_prompt TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_agents_host_id ON agents(host_id);
CREATE INDEX idx_agents_project_id ON agents(project_id);

-- host당 동시 운영 가능한 agent 상한 — "여유 있음" 판단의 1차 게이트(§5).
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS max_agents INTEGER NOT NULL DEFAULT 1;
-- 기존 host는 전부 max_agents=1로 시작 — 지금까지의 "host당 최대 1워커" 실질
-- 동작과 동일하게 유지되며, 운영자가 명시적으로 늘려야 다중 에이전트가 열린다
-- (조용한 동작 변화 없음).

CREATE TABLE agent_commands (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    host_id UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    command_type TEXT NOT NULL,   -- 'start' | 'stop'
    status TEXT NOT NULL DEFAULT 'pending',  -- 'pending' | 'acked' | 'done' | 'failed'
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    acked_at TIMESTAMPTZ
);
CREATE INDEX idx_agent_commands_host_pending ON agent_commands(host_id) WHERE status = 'pending';

CREATE TABLE agent_memory (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    kind TEXT NOT NULL DEFAULT 'note',
    content TEXT NOT NULL,
    source_task_id UUID REFERENCES tasks(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_agent_memory_agent_id ON agent_memory(agent_id, created_at DESC);

CREATE TABLE thread_summaries (
    thread_id UUID PRIMARY KEY,
    summary TEXT NOT NULL,
    turn_count INTEGER NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 태스크가 특정 agent를 직접 지정할 수 있도록 — server_hint와 동격의 신규 필드.
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS agent_id UUID REFERENCES agents(id) ON DELETE SET NULL;
```

`hosts.worker_id`(기존, 013 이전부터 존재)는 **건드리지 않습니다** — 이번 확장은
전적으로 덧붙이는 방식입니다. `agents` 테이블이 없는 host는 지금까지처럼
`hosts.worker_id` 단일 연결만으로 계속 동작합니다. `max_agents` 기본값 1은 기존
동작을 그대로 보존하기 위한 안전장치입니다.

## 4. 동적 프로비저닝 프로토콜

![Agent Dynamic Provisioning Sequence](../assets/diagrams/architecture/agent-dynamic-provisioning-sequence.mermaid)

**설계 원칙**: `fleet-worker`는 오케스트레이터로부터 인바운드 연결을 받지
않습니다 — `#42` 조사에서 확인했듯, `fleet-worker`는 항상 아웃바운드로만
통신하는 것이 의도된 설계입니다(NAT/방화벽 뒤 호스트도 문제없이 동작하게
하기 위함). 그래서 이 설계는 **새 인바운드 제어 채널을 만들지 않고, 기존
heartbeat 폴링에 커맨드를 얹는 방식**을 택합니다.

1. 관리자/프로젝트가 `POST /api/agents`로 에이전트 생성을 요청하면, 오케스트레이터는
   `hosts.max_agents`와 현재 그 host에서 `Running`/`Starting` 상태인 agent
   개수를 비교해 여유를 확인합니다. 여유가 있으면 `agents` 행(`status=Pending`)과
   `agent_commands` 행(`command_type=start`)을 함께 만듭니다.
2. `fleet-worker`의 기존 하트비트 루프(`registration.rs::run_heartbeat_loop`,
   기본 15초 주기, 변경 없음)가 다음 하트비트를 보낼 때, 오케스트레이터의
   `HeartbeatResponse`에 그 host 앞으로 온 `pending` 커맨드 목록이 실려서
   돌아옵니다(`HeartbeatResponse`에 `pending_commands: Vec<AgentCommand>` 필드
   신설).
3. `fleet-worker`가 `start` 커맨드를 받으면 `GrokRunner`가 **새 grok 프로세스를
   spawn**합니다 — 이게 이번 확장에서 가장 크게 손대는 부분입니다: 지금
   `GrokRunner`는 정적 설정으로 프로세스 1개만 관리하는 구조인데, 이제 여러
   프로세스를 동시에 관리(각자 다른 포트, 각자 다른 재시작 루프)할 수 있어야
   합니다. 포트는 host별로 관리하는 사용 가능 범위에서 동적 할당합니다.
4. 새 grok 프로세스가 뜨면, `fleet-worker`가 **기존 `POST /v1/workers/register`
   흐름을 그대로 재사용**해 등록합니다 — `worker.name`에 `agent_id`를 인코딩해
   (`"<host>-agent-<agent_id 앞 8자리>"`) 오케스트레이터가 새로 생긴
   `worker_id`를 어느 `Agent`와 연결해야 하는지 상관관계를 잡습니다. 새 도구나
   새 등록 프로토콜을 만들지 않습니다.
5. 등록이 완료되면 오케스트레이터가 `agents.worker_id`를 채우고
   `status=Running`으로 전이시키며, `agent_commands`를 `done`으로 마킹합니다
   (ack는 `POST /v1/workers/agent-commands/:id/ack` 신규 엔드포인트, 또는
   register 요청 자체에 `agent_command_id`를 실어 보내 한 번에 처리하는 것도
   가능 — 구현 단계에서 확정).
6. 이후 이 agent로의 디스패치는 **기존 `Dispatcher`/`WorkerSelector` 경로를
   그대로 탑니다**(worker_id가 채워졌으므로) — §6에서 다루듯 custom_prompt/메모리
   주입은 디스패치 "이전" 프롬프트 조립 단계의 일이지, 새로운 디스패치
   프로토콜이 필요한 게 아닙니다.
7. 종료(`stop`)도 동일한 커맨드-큐 패턴을 재사용합니다 — `GrokRunner`의 기존
   SIGTERM→5초 대기→SIGKILL 종료 정책을 그 프로세스에만 적용하고, 기존
   deregister 흐름으로 마무리합니다.

### "여유" 판단 기준 (host capacity)

1단계는 단순하게 갑니다: `hosts.max_agents`(운영자가 명시적으로 설정하는 정수
상한)와 현재 `Running`/`Starting` agent 개수만 비교합니다. `HostMetrics`(`load_avg`,
`mem_available_mb`, `disk_free_mb`, 이미 heartbeat로 수집 중)를 근거로 한
자동 판단(예: "메모리 여유가 X GB 이상이면 자동으로 여유로 판단")은 1단계
범위에서 제외하고 §9 열린 질문으로 남깁니다 — 리소스 기반 자동 스케일링은
검증 없이 들어가면 예측 불가능한 프로세스 폭증 위험이 있어, 명시적 운영자
상한을 1단계 안전장치로 우선합니다.

## 5. Custom 프롬프트 적용 방식

`custom_prompt`는 **grok CLI 인자로 넘기지 않습니다.** 현재
`grok_process.rs::spawn_grok()`은 `--bind`/`--secret`만 넘기고, 시스템
프롬프트를 프로세스 수준에서 주입하는 CLI 표면이 있는지 확인되지 않았습니다
(§9 열린 질문). 대신 **디스패치 시점에 프롬프트 조립 단계에서 텍스트로 앞에
붙입니다** — 기존 `acp_transport.rs::build_threaded_prompt()`가 스레드 이력을
이어붙이는 것과 정확히 같은 위치, 같은 방식입니다. 이렇게 하면:

- grok 프로세스 자체는 변경 없이 그대로 재사용.
- 하나의 grok 프로세스가 실행 도중에도(재시작 없이) custom_prompt를 바꿔
  적용 가능(에이전트 메타데이터만 갱신하면 됨 — 프로세스는 무관).

## 6. 메모리 및 컨텍스트 연속성

![Agent Memory Injection Flow](../assets/diagrams/architecture/agent-memory-injection-flow.mermaid)

기존 ACP 세션 모델(`acp_transport.rs` 모듈 문서: "디스패치된 태스크마다 새
ACP 세션")은 **바꾸지 않습니다** — 태스크 라우팅 명확성을 위해 의도적으로
선택된 설계이기 때문입니다. 대신 "여러 세션에 걸친 맥락 유지"는 **디스패치
시점의 프롬프트 조립으로 흉내**냅니다:

- `task.agent_id`가 지정되면: 그 agent가 `Running` 상태인지 확인(아니면
  `server_hint`의 `HintedUnavailable`과 동일한 엄격도로 에러) → `agent_memory`에서
  해당 `agent_id`의 최근 N개 항목을 시간순으로 조회 → `custom_prompt` +
  메모리 + (스레드 이력이 있다면 기존 방식대로) + 새 프롬프트 순으로 조립.
- 메모리 **쓰기**는 1단계에서 단순하게: 태스크가 `Completed`로 전이될 때, 그
  태스크가 `agent_id`를 가지고 있었다면 완료된 Q&A를 `agent_memory`에 한 건
  추가합니다(자동 요약/증류 없음 — 원문 그대로 누적). 항목 수가 일정 개수를
  넘으면 오래된 것부터 잘라내는 보존 정책은 구현 단계에서 정합니다(무한정
  누적 방지).
- **프로젝트별 스코프**: `agent_memory`는 `agent_id`로만 키가 잡히지만,
  Agent 자체가 `project_id`를 갖고 있으므로(§3) 결과적으로 프로젝트별로
  분리됩니다 — 별도의 (agent, project) 복합 키가 필요하지 않습니다(에이전트가
  여러 프로젝트에 공유되는 경우는 §9 참고).

## 7. 프로젝트에 속하지 않는 스레드의 요약

`task.project_id`가 `NULL`인 스레드(즉 일반 풀 태스크, agent 지정도 없는 경우)는
Agent 메모리 메커니즘의 대상이 아닙니다 — 대신 `thread_summaries` 테이블로
**스레드 단위 요약**만 관리합니다:

- 현재 `build_threaded_prompt()`는 스레드의 모든 이전 turn을 원문 그대로
  이어붙입니다 — 스레드가 길어질수록 프롬프트가 무한정 커지는 문제가 이미
  잠재해 있었습니다.
- 이 설계는 스레드가 일정 turn 수(구현 단계에서 임계값 확정, 예: 5턴)를
  넘으면 `thread_summaries`에 요약을 만들고, 이후 디스패치부터는 "저장된
  요약 + 요약 시점 이후의 최근 원문 turn만" 이어붙이는 방식으로 전환합니다.
- 요약 **생성 자체**(원문을 실제로 압축하는 로직)는 1단계에서는 범위 밖으로
  둡니다 — 저장소(`thread_summaries` 테이블)와 조회/주입 경로만 먼저 만들고,
  요약을 실제로 채우는 방법(예: 별도 요약 전용 grok 호출, 또는 단순 최근 N턴만
  유지하고 그 이전은 버리는 규칙 기반 압축)은 후속 단계에서 정합니다.

## 8. 디렉토리 기반 결과물 관리

가장 범위가 느슨하게 요청된 항목입니다("필요한 경우"). 1단계 제안:

- `Agent`(또는 `Project`)에 `workdir_template: Option<String>` 같은 필드를 두어,
  그 agent/project로 디스패치되는 태스크의 `DispatchRequest.cwd` 기본값을
  프로젝트별 하위 디렉토리(예: `/var/lib/fleet/projects/<project_id>/`)로
  맞춥니다 — `Task.cwd`가 이미 존재하는 필드이므로 새 프로토콜 없이 기본값
  결정 로직만 추가하면 됩니다.
- 결과물을 오케스트레이터로 동기화(예: 워커→오케스트레이터 rsync, 또는 S3류
  업로드)하는 것은 이번 설계 범위에서 **제외**합니다 — 요청이 "필요한 경우"로
  조건부였고, 파일 동기화 프로토콜은 그 자체로 별도 설계가 필요한 큰 주제라
  판단했습니다. 필요성이 확인되면 별도 로드맵 항목으로 분리 권고.

## 9. RBAC 권한 추가

| 변형 | 직렬화 이름 | 의미 |
|---|---|---|
| `AgentCreate` | `agent:create` | 에이전트 생성(프로비저닝 요청) |
| `AgentRead` | `agent:read` | 에이전트 목록/상세/메모리 조회 |
| `AgentDelete` | `agent:delete` | 에이전트 중지 및 삭제 |
| `AgentManage` | `agent:manage` | custom_prompt 수정, 메모리 수동 편집/삭제 |

`#48`의 `ProjectAssign`과 별개입니다 — 프로젝트에 기존 워커를 "배치"하는 것과
프로젝트를 위해 새 프로세스를 "만드는" 것은 위험도가 다르다고 판단해 분리
했습니다(에이전트 생성은 host에 실제 프로세스를 띄우는 부작용이 있음). `Admin`만
`AgentCreate`/`AgentDelete` 기본 보유, `Operator`는 `AgentRead`+`AgentManage`.

## 10. API/MCP 표면 (개략)

Phase 3~4에서 확정하며, `#48`과 동일한 REST(`/api/agents/*`) + MCP(`fleet_*`)
페어링 관례를 따릅니다: `fleet_create_agent`, `fleet_list_agents`,
`fleet_stop_agent`, `fleet_get_agent_memory`. `fleet_dispatch_task`는
`agent_id`(선택) 입력을 하나 더 받습니다.

## 11. 단계별 구현 계획

`#48`보다 리스크가 큰 확장(특히 Phase 3의 다중 프로세스 관리)이라 더 잘게
쪼갭니다. 각 단계는 독립적으로 커밋·검증 가능해야 합니다.

1. **Phase 1 — 스키마 + 정적 등록**: `016_agents.sql`, `fleet-core::Agent`/`AgentStatus`/`AgentMemoryEntry`/`ThreadSummary`,
   `Store` 트레이트 확장. **아직 진짜 프로세스 spawn 없이** — 기존처럼 수동/사전
   기동된 grok 프로세스를 `POST /api/agents`로 "등록"만 하는 정적 경로부터
   시작해, Agent 개념과 RBAC를 검증합니다(위험을 낮추는 디딤돌 단계).
2. **Phase 2 — 메모리 + 스레드 요약**: `agent_memory`/`thread_summaries` 저장소,
   §6/§7의 프롬프트 조립 로직(`acp_transport.rs` 또는 `Dispatcher` 확장),
   `task.agent_id` 필드. 신규 테스트: 메모리 누적/조회, 스레드 요약 임계값
   전환, agent 미실행 상태 디스패치 시 에러 경로.
3. **Phase 3 — 동적 프로비저닝**(가장 위험도 높음): `agent_commands` 큐,
   `HeartbeatResponse.pending_commands` 확장, `fleet-worker`의 `GrokRunner`를
   다중 프로세스 관리자로 재작성, 커맨드 ack 흐름. 별도 실기기(최소 1개 호스트)
   대상 수동 검증을 반드시 병행 — 순수 유닛테스트만으로는 실제 프로세스
   spawn/kill 신뢰성을 담보하기 어려움.
4. **Phase 4 — API + MCP + 대시보드 UI**: §10 표면 + `/agents` 페이지군.

## 12. 열린 질문 (구현 중 재검토 필요)

- **grok의 실제 시스템 프롬프트 주입 CLI 표면**: `grok agent serve`가 프로세스
  수준에서 시스템 프롬프트를 받는 플래그가 있는지 확인되지 않았습니다. 없으면
  §5의 "프롬프트 조립 시점 주입" 방식이 유일한 선택지가 되고(이미 기본안으로
  채택), 있다면 프로세스 수준 주입과 병행할지 재검토.
- **에이전트가 여러 프로젝트에 공유되는 경우**: 현재 설계는 Agent가 최대
  1개 프로젝트에 귀속(`project_id: Option<ProjectId>`)한다고 가정합니다. 실제
  운영에서 "이미 떠 있는 에이전트를 다른 프로젝트도 재사용하고 싶다"는
  요구가 생기면, `agent_memory`를 `(agent_id, project_id)` 복합 키로 바꿔야
  합니다 — 지금 스키마에서는 마이그레이션이 필요한 변경이므로 조기에 필요성이
  확인되면 Phase 2 전에 재검토 권고.
- **호스트 리소스 기반 자동 "여유" 판단**: §4에서 1단계는 명시적
  `max_agents` 정수 상한만 쓰기로 했습니다. `HostMetrics`(메모리/로드) 기반
  자동 판단은 후속 검토.
- **`agent_commands`의 유실/중복 처리**: heartbeat 유실 시 커맨드가 다음
  heartbeat까지 지연되는 것은 허용 가능(하트비트 15초 주기라 최대 지연도
  작음)하지만, ack 유실로 같은 커맨드가 중복 실행되지 않도록 `agent_commands.id`
  기준 멱등 처리가 Phase 3 구현에서 필수.
- **스레드 요약 생성 방법**: §7에서 저장소만 설계하고 실제 압축 로직은
  범위 밖으로 뒀습니다 — 규칙 기반(최근 N턴만 유지) vs 모델 기반(별도 요약
  호출) 중 Phase 2 구현 시점에 결정.

## 관련 문서

- [`docs/roadmap/roadmap.md`](../roadmap/roadmap.md) #49 — 구현 진행 상황 정본.
- [`docs/architecture/project-feature-design.md`](project-feature-design.md) — `#48`,
  이 설계가 전제하는 프로젝트 기반 설계.
