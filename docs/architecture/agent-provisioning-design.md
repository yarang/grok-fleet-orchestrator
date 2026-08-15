---
type: wiki
status: canonical
source: "docs/architecture/agent-provisioning-design.md"
last_verified: "2026-08-15"
---

# 에이전트(Agent) 동적 프로비저닝 · 메모리 · 스레드 요약 · 도구 바인딩 설계

> 작성일: 2026-08-14. 로드맵 [`#49`](../roadmap/roadmap.md)에 대응하는 설계
> 문서입니다. [`#48` 프로젝트 기능 설계](project-feature-design.md) 위에
> 쌓이는 후속 확장입니다. 아직 구현되지 않았습니다 — 진행 상황은
> `roadmap.md` #49 항목을 정본으로 확인하세요. 개정 이력(왜 이렇게
> 결정했는지, 조사 경위, AskUserQuestion 답변)은 [`log.md`](log.md)의
> "agent-provisioning-design.md" 절을 참고하세요 — 이 문서 본문은 현재
> 확정된 설계만 담습니다.

## 1. 배경 및 사용자 요구사항

1. 하나의 host에서 에이전트가 **여러 개** 동작할 수 있어야 한다.
2. **custom 프롬프트**로 여러 에이전트를 구분해서 쓸 수 있어야 한다.
3. 프로젝트가 **필요한 경우 에이전트를 만들어서 호출**할 수 있어야 한다.
4. 에이전트는 host에 **여유가 있을 때** 만들어서 운영할 수 있어야 한다.
5. 개별 에이전트는 **여러 세션에 걸친 맥락**을 유지해야 한다.
6. 그 맥락은 **메모리**로 관리하고, **프로젝트별로** 스코프돼야 한다.
7. 프로젝트에 속하지 않는 태스크 스레드는 **스레드별 요약**으로 관리한다.
8. 필요하면 결과물을 **디렉토리**로 관리한다.
9. custom 프롬프트는 **오케스트레이터가 중앙 관리**하고, 이를 **CLI**와 연결한다.
10. custom 프롬프트를 **tool 혹은 skill과 연결**시켜서 에이전트를 만들고, 이
    에이전트에게 태스크를 할당하는 구조.
11. **tool과 MCP를 중앙에서 관리**해 필요한 tool을 제공.
12. 에이전트에게 필요한 tool을 미리 연결해주는 **template** 설정.
13. **필수(required) tool과 옵션(optional) tool**이 필요 시 제공되도록.

## 2. 핵심 설계 결정

| 결정 사항 | 채택안 | 근거 |
|---|---|---|
| Agent ↔ Worker 관계 | **Agent를 신규 엔티티로 도입** (Worker와 분리) | Worker는 저수준 접속/용량 개념 유지, Agent가 custom_prompt·메모리·프로젝트 소속·도구 바인딩을 담당. Running 상태일 때만 정확히 하나의 Worker에 연결(1:0..1) |
| 동적 프로비저닝 범위 | **진짜 동적 프로비저닝** — 오케스트레이터가 실행 중 원격으로 grok 프로세스 시작/종료 | 요구사항을 문자 그대로 만족 |
| 메모리 구현 방식 | **구조화된 텍스트/JSON 누적 + 프롬프트 주입** | 임베딩/벡터DB 같은 신규 인프라 불필요 |
| 로드맵 항목 | **`#49`로 분리** (`#48`과 독립) | 기술 표면이 크게 다름 |
| 도구(MCP) 바인딩 메커니즘 | 데이터 모델(중앙 카탈로그+템플릿+필수/옵션)은 확정, **실제 연결 경로는 Phase 0 검증 스파이크로 확정**(§5.2) | ACP `with_mcp_server()`가 unstable 피처 필요 + Rust 프록시 구현 필요, grok 자체 로컬 설정 파일 지원 여부도 미확인 — 두 후보 다 미검증 |
| 필수/옵션 도구 활성화 시점 | **명시적 선택** — 태스크가 `requested_optional_tools`로 직접 요청 | grok이 세션 도중 자동 판단하는 방식은 훨씬 복잡한 양방향 프로토콜 필요 |
| Agent의 `project_id` | **host에서 상속**(직접 지정 불가) | `#48`의 배타적 1:N 모델 — host 자체가 이미 한 프로젝트에만 속하므로 자동 결정됨(§3) |
| `AgentProvisioningMode`(수동/자동) | `Project.agent_provisioning_mode`로 모델링, `Automatic`은 `AgentAutoProvisioner` 백그라운드 루프가 처리(§4.1) | 사용자가 "직접 설정 vs 오케스트레이터가 만들어 사용" 옵션 요청 |

## 3. 데이터 모델

![Agent Data Model](../assets/diagrams/architecture/agent-data-model.mermaid)

### 신규 타입 (`fleet-core`)

```rust
// crates/fleet-core/src/ids.rs — 기존 TaskId/WorkerId/ProjectId 패턴과 동일
pub struct AgentId(pub Uuid);
pub struct AgentTemplateId(pub Uuid);
pub struct McpServerId(pub Uuid);

// crates/fleet-core/src/agent.rs (신규)
pub struct Agent {
    pub id: AgentId,
    pub host_id: Uuid,
    /// host_id가 가리키는 host의 project_id를 그대로 상속(#48의 배타적 소유
    /// 모델 — host 자체가 이미 1개 프로젝트에만 속하므로 그 위의 Agent도
    /// 자동으로 모호함 없이 결정됨). 별도로 지정하지 않음 — 생성 시점에
    /// host에서 읽어와 채우는 파생 필드.
    pub project_id: Option<ProjectId>,
    pub worker_id: Option<WorkerId>,
    pub template_id: Option<AgentTemplateId>,
    pub name: String,
    pub custom_prompt: Option<String>,   // 템플릿에서 상속, 개별 오버라이드 가능
    pub status: AgentStatus,
    /// "manual" | "automatic" — §4.1의 유휴 자동 종료 대상 판단에 필수
    /// ("Manual로 만든 에이전트는 자동 종료 대상 아님" 규칙이 이 컬럼에 의존).
    pub provisioned_by: AgentProvisionedBy,
    /// 생성 시점에 `project.agent_idle_timeout_secs`를 그대로 복사한 스냅샷.
    /// `#48` §6의 "왜 스냅샷인가(템플릿 라이브 참조 금지)"와 동일한 이유 —
    /// 프로젝트를 매번 라이브 조회하면 이 에이전트의 소속 프로젝트가 나중에
    /// 삭제(`agents.project_id`는 `ON DELETE SET NULL`)돼도 이 행 자체는
    /// 남는데, §4.1의 유휴 스윕이 "project → agent" 방향으로 순회하므로
    /// 프로젝트가 사라진 자동생성 에이전트가 영원히 스윕 대상에서 빠지는
    /// 좀비가 되는 걸 방지한다.
    pub idle_timeout_secs: Option<u32>,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum AgentProvisionedBy { Manual, Automatic }

pub enum AgentStatus { Pending, Starting, Running, Stopping, Stopped, Failed }

pub struct AgentMemoryEntry {
    pub id: Uuid,
    pub agent_id: AgentId,
    pub kind: String,
    pub content: String,
    pub source_task_id: Option<TaskId>,
    pub created_at: DateTime<Utc>,
}

pub struct ThreadSummary {
    pub thread_id: TaskId,
    pub summary: String,
    pub turn_count: u32,
    pub updated_at: DateTime<Utc>,
}

// ── 도구(MCP) 바인딩 ──────────────────────────────
pub struct AgentTemplate {
    pub id: AgentTemplateId,
    pub name: String,
    pub description: Option<String>,
    pub custom_prompt: Option<String>,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 중앙 MCP 도구 카탈로그 항목. "실제로 어떻게 grok 세션에 붙이는지"는
/// 아직 확정하지 못했으므로, 특정 메커니즘에 종속되지 않는 연결 스펙만
/// 저장한다 — Phase 0 검증 스파이크 결과에 따라 소비하는 쪽만 바뀐다.
pub struct McpServerConfig {
    pub id: McpServerId,
    pub name: String,
    pub description: Option<String>,
    pub transport: McpTransport,   // Stdio | Http | Sse
    pub command: Option<String>,   // transport=Stdio
    pub args: Vec<String>,         // transport=Stdio
    pub url: Option<String>,       // transport=Http | Sse
    pub env: HashMap<String, String>,  // 1단계는 평문 — 시크릿이면 fleet-credentials 연동은 §12 열린 질문
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub enum ToolRequirement { Required, Optional }
```

### 신규 마이그레이션 (`016_agents.sql`, `#48`의 `015_projects.sql` 다음 번호)

```sql
-- host_id는 ON DELETE CASCADE를 유지한다(mcp_servers와 달리 RESTRICT로
-- 바꾸지 않음) — Stopped/Failed로 끝난 과거 agent 기록까지 호스트 삭제를
-- 막으면 지나치게 엄격하다. 대신 "실행 중(Running/Starting/Pending/Stopping)
-- agent가 있으면 호스트 삭제 자체를 애플리케이션 레벨에서 409로 차단"하는
-- 가드를 hosts 삭제 핸들러에 추가한다(§10). RESTRICT처럼 FK 제약으로
-- 강제하지 않는 이유는 이 조건이 status 값에 달려 있어 순수 SQL CHECK/FK로
-- 표현할 수 없기 때문 — mcp_servers RESTRICT는 무조건 차단이라 FK로
-- 충분했지만, 이건 상태 조건부 차단이라 애플리케이션 코드가 필요하다.
-- ⚠️ 팀 검토에서 발견(critical): agents가 agent_templates를 FK로 참조하는데
-- 이 파일 안에서 agent_templates는 훨씬 아래(도구 바인딩 절)에서야
-- 만들어집니다 — 그대로면 이 CREATE TABLE 자체가 실패합니다. 013/015가
-- 이미 쓴 "FK 없이 원시 컬럼만 먼저 예약 → 참조 대상 테이블이 생긴 뒤
-- ALTER TABLE로 제약 추가" 패턴을 여기도 그대로 적용합니다 — template_id는
-- 아래에서 UUID 원시 컬럼으로만 두고, agent_templates 생성 직후(§3 하단)
-- FK 제약을 별도로 겁니다.
CREATE TABLE agents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    host_id UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    worker_id UUID REFERENCES workers(id) ON DELETE SET NULL,
    template_id UUID,  -- FK는 agent_templates 생성 후 아래에서 ALTER TABLE로 추가
    name TEXT NOT NULL UNIQUE,
    custom_prompt TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    provisioned_by TEXT NOT NULL DEFAULT 'manual',  -- 'manual' | 'automatic'
    idle_timeout_secs INTEGER,  -- 생성 시점 project.agent_idle_timeout_secs 스냅샷
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_agents_host_id ON agents(host_id);
CREATE INDEX idx_agents_project_id ON agents(project_id);

ALTER TABLE hosts ADD COLUMN IF NOT EXISTS max_agents INTEGER NOT NULL DEFAULT 1;
-- 기존 host는 전부 1로 시작 — "host당 최대 1워커"이던 기존 실질 동작을
-- 조용히 바꾸지 않는다. 운영자가 명시적으로 올려야 다중 에이전트가 열린다.
-- "여유" 카운트/체크는 반드시 host 행을 SELECT ... FOR UPDATE로 잠근
-- 트랜잭션 안에서 수행한다 — 잠금 없이 카운트만 하면 동시에 들어온 두
-- POST /api/agents 요청이 둘 다 "여유 있음"으로 읽어 max_agents를
-- 초과하는 TOCTOU 레이스가 가능하다.

CREATE TABLE agent_commands (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    host_id UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    command_type TEXT NOT NULL,              -- 'start' | 'stop' | 'capture_terminal'(#50)
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

-- ── 도구(MCP) 바인딩 ──────────────────────────────

CREATE TABLE agent_templates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    custom_prompt TEXT,
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 이제 agent_templates가 존재하므로 위에서 미뤄둔 FK를 건다(팀 검토
-- critical 수정).
ALTER TABLE agents
    ADD CONSTRAINT agents_template_id_fkey
    FOREIGN KEY (template_id) REFERENCES agent_templates(id) ON DELETE SET NULL;

-- #48의 015_projects.sql이 "#49 Phase 1이 agent_templates를 만들 때 이
-- FK를 추가한다"고 예고했던 것을 실제로 이행(팀 검토 major — 예고만 되고
-- 실제로 추가된 적이 없었음).
ALTER TABLE projects
    ADD CONSTRAINT projects_default_agent_template_id_fkey
    FOREIGN KEY (default_agent_template_id) REFERENCES agent_templates(id) ON DELETE SET NULL;

CREATE TABLE mcp_servers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    transport TEXT NOT NULL,   -- 'stdio' | 'http' | 'sse'
    command TEXT,
    args JSONB,
    url TEXT,
    env JSONB,
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- mcp_server_id는 ON DELETE RESTRICT(정책 결정) — CASCADE였다면 관리자가
-- 카탈로그 항목을 지웠을 때 이를 참조하는 템플릿/에이전트에서 도구
-- 바인딩이 조용히 사라진다(운영 리스크). 참조 중인 mcp_server는 삭제를
-- 막고, API(`DELETE /api/mcp-servers/:id`)도 참조 존재 시 409 + 참조
-- 중인 template/agent 목록을 응답 본문에 포함한다(§10).
CREATE TABLE agent_template_tools (
    template_id UUID NOT NULL REFERENCES agent_templates(id) ON DELETE CASCADE,
    mcp_server_id UUID NOT NULL REFERENCES mcp_servers(id) ON DELETE RESTRICT,
    requirement TEXT NOT NULL,   -- 'required' | 'optional'
    PRIMARY KEY (template_id, mcp_server_id)
);

-- 에이전트 인스턴스화 시점에 template_tools에서 복사, 이후 개별 오버라이드 가능.
CREATE TABLE agent_tools (
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    mcp_server_id UUID NOT NULL REFERENCES mcp_servers(id) ON DELETE RESTRICT,
    requirement TEXT NOT NULL,
    PRIMARY KEY (agent_id, mcp_server_id)
);

-- 태스크가 이번 한 번만 특정 optional 도구를 켜고 싶을 때.
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS agent_id UUID REFERENCES agents(id) ON DELETE SET NULL;
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS requested_optional_tools JSONB;
```

`hosts.worker_id`(기존)는 건드리지 않습니다 — 전적으로 덧붙이는 방식입니다.

## 4. 동적 프로비저닝 프로토콜

![Agent Dynamic Provisioning Sequence](../assets/diagrams/architecture/agent-dynamic-provisioning-sequence.mermaid)

`fleet-worker`의 제어 플레인(등록/하트비트/디레지스터)은 항상 아웃바운드
폴링만 씁니다(mTLS 배포에서 태스크/세션 데이터 플레인용 인바운드 리스너가
따로 있지만, 그건 agent 제어 커맨드 용도로는 쓰지 않습니다) — 그래서 새
인바운드 채널을 만들지 않고 **기존 heartbeat 폴링에 커맨드를 얹는 방식**을
씁니다. 전체 흐름:

1. `POST /api/agents`가 host 여유를 확인하고(아래 "여유" 판단 기준 — **호스트
   행을 `SELECT ... FOR UPDATE`로 잠근 트랜잭션 안에서** 확인/기록) `agents`
   (Pending, `idle_timeout_secs`는 프로젝트 설정을 스냅샷) + `agent_commands`
   (start)를 만든다.
2. 다음 하트비트 응답에 `pending_commands`가 실려 온다 — 이를 위해
   `HeartbeatResponse`(현재 `{ok, desired_state: &'static str, server_time}`
   고정 구조, `crates/fleet-api/src/schema.rs`)를 확장하는 것 자체가
   Phase 4의 스키마 변경 범위입니다.
3. `fleet-worker`가 커맨드를 실행하기 전 **자기 자신 dedup**을 먼저 합니다 —
   로컬 `agent_id` 키드 프로세스 레지스트리에 이미 그 agent용 프로세스가
   있으면 spawn을 스킵하고 ack만 재전송합니다(하트비트 ack 유실로 같은
   `start` 커맨드가 두 번 내려와도 프로세스가 중복 spawn되지 않도록 —
   `agent_commands` 멱등성은 `command_id` 단위가 아니라 "agent_id당
   프로세스 1개"라는 **효과** 단위로 보장합니다).
4. 신규 `POST /v1/workers/agent-commands/:command_id/ack {status}` 엔드포인트로
   먼저 `status: acked`를 보고합니다 — 이 시점에 `agent.status`가
   `Pending → Starting`으로 전이합니다(이 전이 시점을 명확히 정의해야
   §4.1의 `AgentAutoProvisioner` eligibility 체크가 아직 ack되지 않은
   `Pending` 에이전트를 "없음"으로 오판해 중복 생성하는 레이스를 막을 수
   있습니다).
5. `fleet-worker`의 (Phase 4에서 재작성될) 프로세스 레지스트리가 새 grok
   프로세스를 spawn(동적 포트). 기존 `GrokRunner`는 host당 프로세스
   **1개만** 관리했으므로(`crates/fleet-worker/src/grok_process.rs`), 이번
   확장은 "설정 조정"이 아니라 **agent_id 키드 레지스트리로의 재작성**입니다.
   spawn 실패/헬스체크 타임아웃 시 `status: failed`로 ack하고
   `agent.status → Failed` 전이.
6. 기동 성공 시 새 프로세스가 기존 `/v1/workers/register` 흐름으로 자기
   등록 — `worker.name`에 `agent_id`를 **축약 없이 전체 UUID로** 인코딩해
   상관관계를 확보합니다. `/v1/workers/register`는 이름 유일성을 검사하지
   않고 **upsert**하므로(`crates/fleet-api/src/handlers.rs` — 유일성
   검사는 별도의 `/v1/workers/join`에만 있음), 이름을 짧게 자르면 서로
   다른 두 agent가 같은 축약 이름을 만들어낼 경우 조용히 서로의 워커
   레코드를 덮어쓰는 위험이 있어 전체 UUID를 포함해야 합니다.
7. 오케스트레이터가 `agents.worker_id`를 채우고 `status: Starting → Running`
   전이, `status: done`으로 ack. **이 시점에 새 Worker 행의 `project_id`를
   그 Agent의 `project_id`로 직접 설정합니다**(팀 검토 critical — 이전
   버전은 이 단계를 명시하지 않아, 동적으로 만들어진 워커가
   `project_id`를 영영 못 받아 `#48`의 프로젝트 하드 디스패치 필터에
   걸려 자기 자신에게 온 태스크조차 못 받는 구멍이 있었습니다). `#48`
   §3의 "host에 연결된 워커는 heartbeat 재연결마다 host 기준으로
   재동기화"라는 규칙을 여기 그대로 쓸 수 없다는 점에 주의하세요 — 그
   규칙은 `hosts.worker_id`가 host당 워커를 최대 1개만 가리킬 수 있다고
   가정하는데, `#49`는 host 하나에 여러 Agent(=여러 Worker)가 동시에
   존재하므로 그 1:1 가정이 깨집니다. 그래서 이 워커는 `hosts.worker_id`를
   통한 host-경유 재동기화 대상이 아니라, **자신이 속한 Agent를 통해
   `project_id`를 직접(그리고 배타적으로) 받는 별도 경로**를 씁니다 —
   이후 그 Agent가 다른 host로 옮겨가는 일은 없으므로(Agent는 `host_id`가
   불변) 이 값은 최초 1회 설정으로 충분하고 재동기화가 필요 없습니다.
8. 이후 디스패치는 기존 `Dispatcher`/`WorkerSelector` 경로 그대로.
9. 종료(`stop`)도 동일한 커맨드 큐 패턴을 재사용하되, **기존 `GrokRunner`가
   비정상 종료 시 자동으로 프로세스를 재시작하는 루프**(`restart_delay_secs`
   후 재spawn)를 갖고 있다는 점을 반드시 고려해야 합니다 — 신호 없이 바로
   kill하면 그 재시작 루프가 죽은 프로세스를 곧바로 되살려버립니다. 그래서
   stop 처리는 **먼저 해당 agent 전용 shutdown 채널로 "의도된 종료"임을
   신호한 뒤에** kill해야 합니다.
10. 호스트가 `offline_worker_grace`(기존 `Reconciler` 값, 300초)를 넘겨
    하트비트가 끊기면, 그 호스트의 `pending` 커맨드는 `failed`로, 그 위의
    **터미널이 아닌 모든 상태(`Pending`/`Starting`/`Running`/`Stopping`)**
    에이전트는 `Failed`로 일괄 전이하는 스윕을 `AgentAutoProvisioner`
    주기에 추가합니다.
11. **`Stopping` 정체 방지**: 위 10단계는 호스트가 오프라인일 때만
    발동하므로, 호스트는 정상인데 `fleet-worker` 자체가 종료 처리 도중
    재시작해 완료 ack이 유실되는 경우는 못 잡습니다 — `fleet-worker` 기동
    시 `tmux kill-server`로 이전 세션을 이미 정리했으므로(`#50` §3)
    실제로는 종료가 달성됐지만 DB에는 반영되지 않습니다.
    `AgentAutoProvisioner` 스윕에 "`Stopping` 상태가 하트비트 간격의
    여러 배(예: 5분)를 넘기면 `Stopped`로 강제 전이 + 경고 이벤트 기록"
    규칙을 추가합니다 — `Failed`가 아니라 `Stopped`로 낙관적으로 확정하는
    이유는 `kill-server` 정책이 실제 정리를 이미 보장하기 때문(의도한
    종료가 사실상 달성됨).
12. **`Starting` 정체 방지**(팀 검토 critical — 신설): 4단계에서 `status:
    acked` ack을 받아 `Starting`으로 전이한 직후, 5단계(실제 grok spawn)를
    끝내기 전에 `fleet-worker`가 재시작하면 — ack은 이미 보냈으니
    `agent_commands`는 더 이상 "pending"이 아니라 완료된 것으로 보이는데,
    정작 grok 프로세스는 뜬 적이 없거나(재시작 전에 spawn 전이었다면)
    떴어도 tmux와 함께 정리됐습니다(`#50` §3). 이 경우 11단계의 `Stopping`
    처럼 낙관적으로 확정할 근거가 없습니다 — "의도한 시작"이 실제로
    성공했는지 알 방법이 없기 때문입니다. `AgentAutoProvisioner` 스윕에
    "`Starting` 상태가 하트비트 간격의 여러 배(예: 5분)를 넘기고 아직
    `agents.worker_id`가 채워지지 않았으면(=register가 온 적 없으면)
    `Failed`로 강제 전이" 규칙을 추가합니다 — `Stopped`가 아니라 `Failed`로
    확정하는 이유는 시작이 성공했다는 근거가 전혀 없어 실패로 보는 게
    안전하기 때문(운영자/`AgentAutoProvisioner`가 필요하면 새 Agent를
    다시 만들면 됩니다).
13. **`fleet-worker` 재시작 시 `Running` 에이전트 정리**(팀 검토 major —
    신설): 10~12단계는 각각 "호스트 오프라인"과 "특정 커맨드의 ack
    유실"만 다루는데, `fleet-worker`가 **호스트는 정상이고 그 자체만
    빠르게 재시작**하면(배포·크래시 후 즉시 복구) 그 순간 `Running`이던
    모든 Agent의 grok/tmux가 `kill-server`로 한꺼번에 죽는데, 어떤
    `agent_commands`도 이 사실을 보고하지 않습니다(애초에 stop 커맨드가
    발행된 적이 없으므로) — 그래서 10~12단계 중 어느 스윕도 이 경우를
    잡지 못합니다. 이를 잡으려면 `fleet-worker`가 하트비트에 **자기
    자신의 부팅 시각/난수 인스턴스 ID**(`process_incarnation`)를 함께
    보고해야 합니다 — 오케스트레이터가 그 host에 대해 마지막으로 기록한
    값과 다르면 "이 host의 `fleet-worker`가 방금 재시작했다"고 판단해,
    그 host 위의 `Running`/`Starting` Agent 전부를 즉시 `Failed`로 일괄
    전이합니다(11단계의 `Stopping`과 달리 `Running`은 "잘 돌고 있던
    중"이었으므로 낙관적으로 `Stopped`가 아니라 `Failed`로 확정 — 그
    시점에 진행 중이던 태스크가 있었을 수 있고, 그건 `#38`의 기존 orphan
    `Dispatched` 태스크 정리 경로가 별도로 처리합니다). 이 필드는 §4 2단계의
    `HeartbeatResponse` 확장과 같은 시점(Phase 4)에 `HeartbeatRequest`
    쪽에 추가합니다.

### "여유" 판단 기준

`hosts.max_agents`(운영자 설정 정수 상한)와 현재 **터미널 상태(`Stopped`/
`Failed`)가 아닌** 에이전트 개수(`Pending`/`Starting`/`Running`/**`Stopping`**
전부 포함)를 비교합니다 — `Pending`은 "아직 실행 중은 아니지만 이미
슬롯을 예약한" 상태, `Stopping`은 "아직 실제로 프로세스/tmux 세션이
정리되지 않은" 상태이므로 **둘 다 여유 계산에서 빠지면 `max_agents`
상한이 우회될 수 있습니다**(팀 검토 critical — 이전 버전은 `Stopping`을
빠뜨려서, 종료 처리 중인 agent가 아직 실제로는 host 자원을 점유하고
있는데도 "여유 있음"으로 오판해 그 자리에 새 agent를 추가로 만들 수
있는 구멍이 있었습니다). `HostMetrics` 기반 자동 판단은 §12 열린 질문으로
보류.

### 4.1 수동/자동 프로비저닝 모드

```rust
// crates/fleet-core/src/project.rs — 정의 소유권은 #48(Project의 필드이므로).
// 이 문서는 재수출만 참조한다.
pub enum AgentProvisioningMode { Manual, Automatic }
```

`Project.agent_provisioning_mode`(기본값 `Manual` — 운영자가 명시적으로
켜야 자동화가 시작됨):

- **`Manual`**(기본): 관리자/프로젝트가 `POST /api/agents`로 명시적으로만
  에이전트를 만듭니다 — §4의 흐름 그대로. 생성된 Agent의
  `provisioned_by = 'manual'`.
- **`Automatic`**: `default_agent_template_id`**와 `agent_idle_timeout_secs`
  둘 다** 설정돼 있어야 합니다(팀 검토 major — 이전 버전은
  `default_agent_template_id`만 필수로 서술했는데, `agent_idle_timeout_secs`가
  `NULL`이어도 `Automatic`이 그냥 켜지면 자동 생성된 에이전트가 영원히
  회수되지 않아 이 문서 스스로 "자동 생성은 반드시 자동 회수와 짝을
  이뤄야 한다"고 규정한 원칙이 깨집니다). `Project`를 `Automatic`으로
  전환하는 API/CLI 호출은 이 두 필드가 모두 채워져 있는지 확인하고, 아니면
  `400 Bad Request`로 거부합니다 — 이 엔드포인트 자체는
  [`project-feature-design.md`](project-feature-design.md) §7의
  `PATCH /api/projects/:id`(`ProjectCreate` 권한, 팀 검토로 신설)입니다.
  이 전제가 갖춰지면 신규 백그라운드 루프
  **`AgentAutoProvisioner`**가 기존
  `Reconciler`(`fleet-scheduler/src/reconcile.rs`)와 동일한 "설정 + spawn +
  JoinHandle 기반 abort" 패턴으로 주기적으로(예: 30초, `Reconciler`의
  `interval`과 같은 관례) 다음을 확인합니다:
  1. `agent_provisioning_mode = 'automatic'`인 프로젝트 중, `project_id`가
     일치하는 `Pending` 태스크가 있고
  2. 그 프로젝트에 **터미널 상태(`Stopped`/`Failed`)가 아닌** Agent(`Pending`/
     `Starting`/`Running`/`Stopping`)가 하나도 없거나, 존재하는 것들이 전부
     `Running`이면서 `has_capacity() == false`이며(동일한 이유로 `Stopping`도
     "존재하는 에이전트"로 카운트합니다 — 위 "여유" 판단 기준과 같은 근거)
  3. 그 프로젝트에 배정된 host 중 `max_agents` 여유가 있는 host가 있으면
  → `default_agent_template_id`로 새 Agent를 만들고(`provisioned_by =
  'automatic'`) `agent_commands`(start)를 발행합니다(§4의 수동 흐름과 완전히
  동일한 커맨드 큐 메커니즘 — 트리거만 다름).

#### 유휴 판단 기준

**fleet가 이미 신뢰하는 소스만** 사용합니다 — 프로세스 stdio/CPU 사용률
같은 저수준 신호는 쓰지 않습니다(노이즈가 많고, "조용히 추론 중"인 정상
동작과 실제 유휴 상태를 구분하지 못함).

**"동작 중" 판정**: 에이전트가 다음 중 **하나라도** 참이면 동작 중으로
간주해 타임아웃 대상에서 제외합니다.

1. `tasks` 테이블에 이 `agent_id`를 참조하며 `status = Dispatched`인 행이
   하나라도 있음 — **오케스트레이터가 디스패치 시점에 직접 쓰는 값이라
   실시간·권위 있는 1차 신호**.
2. 그 에이전트의 `worker_id`에 대해 `Worker.active_tasks > 0` — 이 값은
   오케스트레이터가 아니라 **워커 프로세스 자신이 하트비트로 자기
   보고**하는 값이라 최대 한 하트비트 주기(기본 15초)만큼 지연될 수
   있습니다 — 1번의 지연 창을 보강하는 2차 확인입니다.
3. `agent_commands`에 이 agent에 대한 `status = 'pending'`인 명령이 있음
   (방금 시작 명령을 냈는데 아직 등록도 끝나지 않은 상태 — 이때 끄면 안 됨).

**타이머 기준 시각**: `GREATEST(agents.created_at, 그 agent의 가장 최근
완료 태스크의 완료 시각)`. 방금 만들어져 아직 태스크를 한 번도 받지 못한
에이전트가 즉시 타임아웃되는 것을 방지합니다.

**레이스 방지**: `AgentAutoProvisioner`의 스윕이 "타임아웃 대상"으로 판단한
직후, 실제로 `agent_commands`(stop)를 발행하기 **직전에 위 3개 조건을 한 번
더 재확인**합니다 — 스윕 판단과 커맨드 발행 사이 새 태스크가 들어왔을 극히
드문 경우를 방어합니다.

**적용 대상**: 위 판단 기준 자체는 `Manual`/`Automatic` 공통 인프라로
구현하지만, 유휴 자동 종료의 **적용 대상은 `Automatic`으로 생성된 에이전트
(`provisioned_by = 'automatic'`)로 한정**합니다 — `agents.idle_timeout_secs`가
`NULL`이 아니면, `AgentAutoProvisioner`가 같은 주기에 위 기준으로 "동작
중이 아니고" 타이머가 만료된 `automatic` 에이전트를 찾아
`agent_commands`(stop)를 발행합니다. **이 정책이 없으면 자동 생성된
에이전트가 영원히 host 여유를 점유하게 되어, "여유가 있을 때만 만든다"는
원래 동기 자체가 무의미해집니다** — 자동 생성은 반드시 자동 회수와 짝을
이뤄야 합니다. `Manual`로 만든 에이전트는 판단 기준이 아무리 정교해져도
이 정책의 대상이 아닙니다 — 사람이 명시적으로 만든 것을 시스템이 임의로
끄는 것은 최소 놀람 원칙에 어긋나므로, 이는 판단 정확도와 무관하게
유지되는 정책적 결정입니다.

### 4.2 전체 생명주기 상태 다이어그램

![Agent Lifecycle State Machine](../assets/diagrams/architecture/agent-lifecycle-state-machine.mermaid)

`orchestrator`와 `fleet-worker`는 대칭적인 실시간 채널이 아니라 **"오케스트레이터가
의도를 큐에 쌓는다 → `fleet-worker`가 폴링해서 실행한다 → ack로 보고한다 →
오케스트레이터가 상태를 확정한다"는 비대칭 폴링 패턴**으로만 협업합니다 —
`#42`/`#50`에서 확인한 "제어 플레인은 항상 아웃바운드"라는 원칙이 상태
전이 전체에 일관되게 적용됩니다.

| 전이 | 트리거 | 판단 주체 | 근거 데이터 |
|---|---|---|---|
| `[*] → Pending` | `POST /api/agents`(Manual) 또는 `AgentAutoProvisioner` 적격 판정(Automatic) | 오케스트레이터 | host `FOR UPDATE` 잠금 + 비-터미널 agent 카운트(§4 "여유" 판단 기준) |
| `Pending → Starting` | 다음 heartbeat로 `pending_commands` 수신, 로컬 dedup 통과 | `fleet-worker` | `agent_id` 키드 로컬 프로세스 레지스트리 |
| `Starting → Running` | grok의 `/v1/workers/register` 성공 | 오케스트레이터 | `link_agent_worker` 성공 여부 |
| `Starting → Failed` | tmux/grok 스폰 실패, 헬스체크 타임아웃 | `fleet-worker` → 오케스트레이터 | ack `{status: failed, error}` |
| `Running → Stopping` | 관리자 명시 요청(Manual) 또는 유휴 판정(Automatic) | 오케스트레이터 | §4.1의 3개 "동작 중" 신호 + `idle_timeout_secs` 스냅샷 타이머 |
| `Stopping → Stopped` | 의도된 종료 신호 → tmux 종료 → deregister | `fleet-worker` → 오케스트레이터 | ack `{status: done}` |
| `(Pending\|Starting\|Running\|Stopping) → Failed` | 호스트 하트비트 유실 300초(`offline_worker_grace`) | 오케스트레이터 | 기존 `Reconciler` 패턴 재사용 스윕 |
| `Stopping → Stopped`(정체 타임아웃) | `Stopping` 정체 5분 초과(호스트는 정상) | 오케스트레이터 | `fleet-worker` 재시작 시 `kill-server`가 실제 정리를 보장한다는 전제로 낙관적 확정(§4 11단계) |
| `Starting → Failed`(정체 타임아웃, 팀 검토 신설) | `Starting` 정체 5분 초과 + `worker_id` 미충족(호스트는 정상) | 오케스트레이터 | 시작 성공 근거가 없어 안전하게 `Failed` 확정(§4 12단계) |
| `(Running\|Starting) → Failed`(팀 검토 신설) | `fleet-worker` 재시작 감지(`process_incarnation` 값 변경) | 오케스트레이터 | 호스트는 정상이지만 로컬 tmux 세션이 전부 정리됐음을 인스턴스 ID로 추론(§4 13단계) |

`Failed`/`Stopped`는 터미널이며 자동 재시도가 없습니다 —
`AgentAutoProvisioner` eligibility 체크에서 "터미널 상태"로 취급돼 새
Agent 생성을 막지 않습니다(같은 프로젝트에 새 에이전트가 자연히
생성됩니다). `capture_terminal` 조회·인터랙티브 attach(`#50`)는 이 상태
머신을 바꾸지 않는 읽기 전용 보조 동작입니다.

## 5. Custom 프롬프트 및 도구(MCP) 바인딩

> **Skill(신규, `#51`)**: custom_prompt(정체성)와 Tool(실행) 사이에
> "절차적 지식" 계층을 추가하는 [`agent-harness-composition-design.md`](agent-harness-composition-design.md)를
> 참고하세요 — Tool과 완전히 같은 바인딩 패턴(템플릿 스냅샷 + 필수/옵션)을
> 그대로 재사용하며, 전체 프롬프트 조립 순서(Project 헌법 → custom_prompt
> → Skill → 메모리 → 스레드 → 새 프롬프트)도 그 문서 §5가 정본입니다.

### 5.1 Custom 프롬프트 — 프롬프트 조립 시점 주입

`custom_prompt`는 grok CLI 인자로 넘기지 않습니다(grok 프로세스 수준
시스템 프롬프트 CLI 표면이 확인되지 않음). 대신 **디스패치 시점 프롬프트
조립 단계**에서 텍스트로 앞에 붙입니다 — 기존
`dispatcher.rs::build_threaded_prompt()`(⚠️ 팀 검토 minor로 소속 파일
정정 — `acp_transport.rs`가 아니라 `crates/fleet-scheduler/src/
dispatcher.rs`)가 스레드 이력을 이어붙이는 것과
정확히 같은 위치, 같은 방식입니다. grok 프로세스 자체는 변경 없이 재사용되고,
custom_prompt를 바꿔도 프로세스 재시작이 불필요합니다.

### 5.2 도구(MCP) 바인딩 — 검증 스파이크 필요

실제 연결 메커니즘은 **두 후보 중 하나**이며 둘 다 미검증입니다:

- **경로 A**: ACP `SessionBuilder::with_mcp_server()` — `unstable_mcp_over_acp`
  피처를 켜고, `mcp_servers` 카탈로그 항목마다(stdio/http 전송 방식별로) 범용
  `McpServerConnect` 프록시 구현체를 통해 세션에 동적으로 붙인다.
- **경로 B**: grok 자체의 로컬 MCP 설정 파일(`grok build`의 `~/.config/grok/mcp.json`과
  유사한 것이 `grok agent serve`에도 있다면) — `fleet-worker`가 에이전트 기동
  직전에 그 host의 해당 경로에 설정 파일을 써주기만 하면 됨. unstable ACP
  피처가 불필요해 리스크가 낮음. ⚠️ **팀 검토 major — 이 경로는 host당
  grok 프로세스가 1개일 때(`#49` 이전)를 전제로 한 설명이라, `#49`
  자체가 도입하는 "host당 여러 Agent" 상황에서 설정 파일 경로가
  프로세스 간에 공유되면 서로 충돌합니다.** `~/.config/grok/mcp.json`처럼
  유저 홈 디렉토리 전역 경로면 같은 host 위의 서로 다른 Agent(서로 다른
  도구 바인딩을 가짐)가 같은 파일을 두고 경합합니다 — 경로 B가 채택되면
  **경로 자체를 agent별로 격리**(예: grok이 프로젝트-로컬
  `.grok/mcp.json`도 지원한다면 `agent.workdir_template` 하위에, 아니면
  `--config-dir` 류 CLI 플래그로 agent별 디렉토리를 지정할 수 있는지)해야
  하며, 이것도 Phase 0 검증 스파이크 범위에 포함합니다.

어느 경로든 **디스패치 로직의 도구 해석 규칙은 동일**합니다:

```text
attach(session) = agent_tools(agent_id, requirement='required')
                 ∪ { t ∈ agent_tools(agent_id, requirement='optional')
                     | t.mcp_server.name ∈ task.requested_optional_tools }
```

즉 필수 도구는 항상 붙고, 옵션 도구는 그 태스크가 이름으로 명시 요청한 것만
붙습니다. 세션 도구 표면을 필요한 만큼만 유지해 컨텍스트 낭비/도구 선택
혼란을 줄입니다.

## 6. 중앙 도구/MCP 카탈로그 및 템플릿

- **`mcp_servers`**(카탈로그): 관리자가 등록하는 "쓸 수 있는 도구 목록" —
  이름, 설명, 연결 스펙(§3). 여러 템플릿/에이전트가 같은 항목을 공유 참조.
- **`agent_templates`**: "이런 종류의 에이전트를 만들 때 기본으로 뭘 줄지"의
  프리셋 — `custom_prompt` 기본값 + `agent_template_tools`(필수/옵션 도구
  목록). 예: "코드 리뷰어" 템플릿 = custom_prompt "당신은 코드 리뷰 전문
  에이전트입니다..." + 필수[linter-mcp, github-mcp] + 옵션[slack-mcp].
- **에이전트 생성 흐름**: `POST /api/agents {template_id, host_id, name}`
  (⚠️ 팀 검토 major — 이전 버전은 여기에 `project_id`도 입력받게
  서술돼 있었는데, 이는 §2 결정표의 "Agent의 `project_id`는 host에서
  상속(직접 지정 불가)"과 정면으로 모순됩니다 — `project_id`는 요청
  바디에서 아예 받지 않고 `host_id`가 가리키는 host의 `project_id`를
  서버가 그대로 채웁니다. 요청에 `project_id`가 포함돼 있으면 무시하는
  게 아니라 `400 Bad Request`로 거부해 혼동을 방지합니다) → 템플릿의
  `custom_prompt`/`agent_template_tools`를 그대로
  `agents.custom_prompt`/`agent_tools`에 복사(스냅샷) → 필요하면 생성 직후
  개별 오버라이드(`PATCH /api/agents/:id`, 도구 추가/제거는 별도 엔드포인트).
  템플릿 없이(`template_id: null`) 커스텀 프롬프트/도구를 처음부터 직접
  지정해 만드는 것도 가능(템플릿은 편의 기능이지 필수 경유지가 아님).
- **왜 스냅샷인가(라이브 참조가 아닌)**: 템플릿을 나중에 고쳐도 이미 떠 있는
  에이전트가 조용히 바뀌지 않아야 운영이 예측 가능합니다 — `Worker`가
  `Host`의 필드를 실시간 참조하지 않고 등록 시점에 스냅샷하는 것과 같은
  설계 관례를 따릅니다.

## 7. 메모리 및 컨텍스트 연속성

![Agent Memory Injection Flow](../assets/diagrams/architecture/agent-memory-injection-flow.mermaid)

기존 ACP 세션 모델("태스크당 새 세션")은 바꾸지 않습니다. "여러 세션에 걸친
맥락 유지"는 디스패치 시점 프롬프트 조립으로 흉내냅니다:

- `task.agent_id` 지정 시: agent가 `Running`인지 확인(아니면 `server_hint`의
  `HintedUnavailable`과 동일한 엄격도로 에러) → `agent_memory`에서 최근 N개
  조회 → `custom_prompt` + 메모리 + (스레드 이력 있으면) + 새 프롬프트 순으로
  조립.
- 메모리 **쓰기**: 태스크가 `Completed` 전이 시, `agent_id`가 있었다면 완료된
  Q&A를 `agent_memory`에 한 건 추가(자동 요약 없음, 원문 누적 — 보존 정책은
  §12 열린 질문).
- **프로젝트별 스코프**: `agent_memory`는 `agent_id`로만 키가 잡히지만, Agent
  자체가 `project_id`를 가지므로 결과적으로 프로젝트별로 분리됩니다.

## 8. 프로젝트에 속하지 않는 스레드의 요약

`task.project_id`가 `NULL`인 스레드는 Agent 메모리 대상이 아니라
`thread_summaries`로 스레드 단위 요약만 관리합니다. 스레드가 일정 turn 수를
넘으면 요약을 만들고, 이후엔 "저장된 요약 + 최근 원문 turn만" 이어붙입니다.
요약 생성 로직 자체(규칙 기반 vs 모델 기반)는 §12 열린 질문.

## 9. 디렉토리 기반 결과물 관리

`Project.workdir_template`(`#48` §3, `projects` 테이블 컬럼) **하나만**
둡니다 — ⚠️ 팀 검토(minor)에서 이전 서술이 "`Agent`/`Project`에
`workdir_template` 필드를 둬"라고 해 마치 `Agent`도 자기 컬럼을 갖는
것처럼 읽혔지만, 실제로 `agents` 스키마(§3)에는 그런 컬럼이 없고 추가할
계획도 없습니다. `Agent`별 디렉토리는 별도 컬럼 없이
**`{project.workdir_template}/{agent.name}`처럼 Agent 이름을 하위
디렉토리로 붙이는 파생 규칙**으로 만듭니다 — `DispatchRequest.cwd` 기본값을
계산할 때 §5의 프롬프트 조립과 같은 디스패치 시점 로직에서 처리하며,
Phase 3(메모리+프롬프트 조립, §11)에서 이 계산 로직도 함께 구현합니다.
오케스트레이터로의 결과물 동기화(rsync/S3 등)는 범위 밖 — 필요성 확인 시
별도 항목.

## 10. RBAC 및 API/CLI/MCP 표면

| 변형 | 직렬화 이름 | 의미 |
|---|---|---|
| `AgentCreate` | `agent:create` | 에이전트 생성 |
| `AgentRead` | `agent:read` | 에이전트/메모리 조회 |
| `AgentDelete` | `agent:delete` | 에이전트 중지/삭제 |
| `AgentManage` | `agent:manage` | custom_prompt/도구 바인딩 수정 |
| `AgentTemplateManage` | `agent_template:manage` | 템플릿/카탈로그 CRUD |

`Admin`만 `AgentCreate`/`AgentDelete`/`AgentTemplateManage` 기본 보유,
`Operator`는 `AgentRead`+`AgentManage`.

> ⚠️ **[critical, 팀 검토] `AgentCreate` 우회 경로**: `AgentAutoProvisioner`(§4.1)는
> RBAC 검사 없이 시스템이 직접 Agent를 생성합니다. `Operator`가 이미 보유한
> `#48`의 `ProjectAssign`(여유 host를 `automatic` 프로젝트에 배정)과 기존
> `TaskCreate`(그 프로젝트 스코프 태스크 제출)를 조합하면, `Operator`가
> `AgentCreate` 없이도 사실상 Agent를 생성시킬 수 있습니다 — 이 문서만
> 봐서는 `AgentCreate`가 유일한 생성 경로처럼 보이지만 실제로는 아닙니다.
> 해소책(가장 유력한 안: `ProjectAssign`을 `Admin` 전용으로 격상)은 정책
> 결정이라 이 문서에서 임의로 바꾸지 않고
> [`project-feature-design.md`](project-feature-design.md) §9에 Phase 1
> 착수 전 확정해야 하는 차단 항목으로 기록했습니다 — 그쪽을 정본으로
> 참고하세요.

**REST**: `/api/agents/*`, `/api/agent-templates/*`, `/api/mcp-servers/*` —
`#48`과 동일한 `/<resource>` + `/api/<resource>` 페어링 관례.
`DELETE /api/mcp-servers/:id`는 `agent_template_tools`/`agent_tools`가 여전히
그 항목을 참조 중이면 `ON DELETE RESTRICT`(§3)로 인해 실패하므로, API가 이를
`409 Conflict`로 변환하고 응답 본문에 참조 중인 template/agent 목록을
포함합니다.

**CLI**(신규 `fleet agent` 명령 그룹, `fleet-cli`의 기존 `Workers`/`Tasks`/`Token`
패턴과 동일): `fleet agent create --template <name> --host <id>`(⚠️ 팀
검토 major — `--project <id>` 플래그는 제공하지 않습니다, `POST
/api/agents`와 동일하게 host에서 파생), `fleet agent list [--project
<id>]`(조회 시 필터링은 여전히 가능), `fleet agent stop <id>`, `fleet
agent memory <id>`,
`fleet agent-template create/list`, `fleet mcp-server register/list`.
`fleet agent create`는 완전 플래그 기반이 기본이되, 필수 플래그(`--host`,
`--name`)가 비어 있고 stdin이 TTY이면 대화형으로 값을 물어보는 보조
경로를 추가합니다(`fleet-worker join`처럼 완전히 별도인 대화형
서브커맨드는 만들지 않음 — 구현 비용을 낮추고 스크립팅 시엔 플래그만으로
완결).

**MCP 도구**(fleet-mcp, `fleet_*`): `fleet_create_agent`, `fleet_list_agents`,
`fleet_stop_agent`, `fleet_get_agent_memory`. `fleet_dispatch_task`는
`agent_id`/`requested_optional_tools` 입력을 추가로 받습니다.

**호스트 삭제 가드**(정책 결정): `DELETE /api/hosts/:hostname`은 이 문서가
아니라 기존 호스트 인벤토리 기능(`ui-design.md` §3.2.5, ⚠️ 팀 검토 minor
— 이전 서술의 §3.9는 `#48`이 이후 프로젝트 목록 페이지로 재사용한
번호라 잘못된 참조였습니다)이 소유한
엔드포인트지만, `#49`가 그 위에 Agent를 도입하면서 새로운 위험이
생겼습니다 — 삭제 시 `agents.host_id`가 `ON DELETE CASCADE`라 실행 중인
agent 행과 `agent_commands`가 조용히 사라지고 실제 grok 프로세스만
고아로 남습니다. **호스트 핸들러에 가드를 추가해, 그 호스트에 터미널
상태가 아닌(`Pending`/`Starting`/`Running`/`Stopping`) agent가 하나라도
있으면 삭제를 `409 Conflict`로 차단**합니다(참조 중인 agent 목록을 응답
본문에 포함 — `mcp_servers` RESTRICT와 동일한 사용자 경험). 운영자는 먼저
각 agent를 stop해 `Stopped`로 만든 뒤에만 호스트를 삭제할 수 있습니다.

## 11. 단계별 구현 계획

`#48`보다 리스크가 커 더 잘게 쪼갭니다. **Phase 0을 신설**해 가장 위험한
미확인 가정(도구 바인딩 메커니즘)을 가장 먼저 검증합니다 — 값싼 검증을
먼저 해서 비싼 구현이 잘못된 가정 위에 지어지는 걸 방지합니다.

0. **Phase 0 — 검증 스파이크** (신규, 구현 착수 전 필수): (a) `grok agent
   serve`가 로컬 MCP 설정 파일을 읽는지 실제 확인(경로 B), 안 되면 (b)
   `unstable_mcp_over_acp` 피처를 켜고 최소 `McpServerConnect` 프록시 하나로
   `with_mcp_server()`가 실제 grok 세션에서 동작하는지 실기기 확인(경로 A).
   결과에 따라 §5.2의 "어느 경로를 쓸지"만 확정 — 데이터 모델(§3, §6)은
   변경 없음.
1. **Phase 1 — 스키마 + 정적 등록**: `016_agents.sql`, `fleet-core` 타입,
   `Store` 확장(PgStore+MemStore 필수 구현). 아직 진짜 spawn 없이 사전 기동된
   grok을 "등록"만 하는 정적 경로로 Agent 개념 자체를 검증.
2. **Phase 2 — 템플릿/카탈로그/도구 바인딩**: `agent_templates`/`mcp_servers`/
   `agent_template_tools`/`agent_tools`, Phase 0에서 확정된 경로로 실제
   세션에 도구 부착. 신규 테스트: 필수/옵션 해석 규칙, 템플릿 스냅샷 동작.
3. **Phase 3 — 메모리 + 스레드 요약**: `agent_memory`/`thread_summaries`
   저장소, §7/§8의 프롬프트 조립 로직.
4. **Phase 4 — 동적 프로비저닝**(최고 위험도): `agent_commands` 큐,
   `HeartbeatResponse` 확장(현재 고정 구조 `{ok, desired_state, server_time}`에
   `pending_commands` 추가) + 신규 `POST /v1/workers/agent-commands/:id/ack`
   엔드포인트, `GrokRunner`를 host당 1프로세스에서 `agent_id` 키드
   프로세스 레지스트리로 재작성(자동 재시작 루프를 agent별 의도된 종료
   신호로 무력화하는 로직 포함, §4 9단계), §4.1의 `AgentAutoProvisioner`
   (Automatic 모드 + 유휴 자동 종료 + 호스트 오프라인 정리 스윕). **`#50`
   설계를 함께 반영**(grok spawn을 tmux 세션으로 감싸는 것으로 다중
   프로세스 로그 수집 문제도 함께 해결). **`GrokRunner`는 처음부터
   `#52`의 `AgentRunner` 트레잇(`NetworkBindRunner` 구현체)으로 작성**하는
   것을 권장 — grok 전용으로 만든 뒤 나중에 일반화하는 것보다, 벤더
   중립 인터페이스 위에 grok 구현체 하나만 있는 상태로 시작하는 게
   재작업이 없습니다(`agent-runtime-vendor-design.md` §4). 실기기 대상
   수동 검증 필수.
5. **Phase 5 — API + CLI + MCP + 대시보드 UI**: §10 표면 전체.

## 12. 열린 질문

- **도구 바인딩 메커니즘 최종 확정**: Phase 0 결과로 확정(§5.2).
- **호스트 리소스 기반 자동 "여유" 판단**: 1단계는 명시적 `max_agents`만
  (§4.1의 `AgentAutoProvisioner`도 동일 기준 사용).
- **`agent_commands` ack 재전송 규칙**: 멱등 단위는 "agent_id당 프로세스
  1개" 효과로 확정(§4 3단계)했으나, 정확한 ack 재전송 횟수/간격은 Phase 4
  구현 시 확정.
- **스레드 요약 생성 방법**: 규칙 기반 vs 모델 기반, Phase 3에서 결정.
- **`mcp_servers.env`의 시크릿 처리**: 1단계는 평문 저장 — API 키 등 민감값이
  섞이면 `fleet-credentials`(기존 AES-256-GCM 마스터키 암호화)와 연동할지
  검토 필요.
- **`agent_memory` 보존/정리 정책 미정**: 현재 설계는 `agent_memory`를
  완전히 무제한 누적합니다(자동 요약 없음, 삭제 로직 없음) — 장수명
  에이전트는 이 테이블이 무한정 자랍니다. 기존 `SessionCleanup`(로그인
  시도 로그의 보존 기간 정리)과 동일한 패턴으로 보존 기간 또는 최대 건수
  기준 정리 잡을 두는 것이 필요할 것으로 예상됩니다 — Phase 3(메모리
  구현) 착수 시 확정. 1단계 설계/구현 자체를 막지는 않되, 무기한 방치는
  안 되므로 명문화해 둡니다.

## 13. 설치·운영 고려 사항

두 설계 문서 모두 "무엇을 만드는가"는 상세하지만 "운영자가 이걸 어떻게
운영하는가"가 비어 있었습니다. 이번 단계에서 전부 해결하지는 않되, 최소한
알려진 리스크로 명문화합니다:

1. **다중 grok 프로세스 로그 수집**: [`agent-terminal-access-design.md`](agent-terminal-access-design.md)(`#50`)가
   `GrokRunner`의 grok spawn을 tmux 세션으로 감싸는 것으로 해소하도록
   설계했습니다 — Phase 4 구현 시 `#50` 설계를 함께 반영해야 합니다
   (별도 로그 인프라를 따로 만들지 않음).
2. **동적 포트 할당 범위 미정**: 방화벽/보안그룹이 고정 포트만 열어둔
   클라우드 배포 환경(`docs/deployment/nginx-gateway.md` 등 기존 배포
   문서가 고정 포트를 전제)과 충돌할 수 있습니다 — host별 포트 **범위**(예:
   `agent_port_range_start`/`_end`)를 `hosts` 테이블에 추가하는 안을 열린
   질문으로 기록합니다.
3. **기존 단일 워커 배포와의 업그레이드 경로**: 기본값들(`max_agents = 1`,
   `agent_provisioning_mode = 'manual'`)이 기존 동작을 그대로 보존하도록
   설계돼 있습니다 — 마이그레이션 적용 직후에는 host당 워커 1개까지만
   허용되고 자동 프로비저닝도 꺼져 있어, 운영자가 명시적으로 설정을 올리기
   전까지는 기존 "host당 워커 1개" 동작과 관찰 가능한 차이가 없습니다.
   **단, `#50`이 Phase 4에 얹이면서 `tmux`가 새로운 필수 호스트
   의존성이 됩니다** — Phase 4 배포 전 전체 호스트 인벤토리에서 tmux
   설치 여부를 먼저 확인/일괄 재프로비저닝하도록 안내해야 합니다(상세는
   [`agent-terminal-access-design.md`](agent-terminal-access-design.md) §7).
4. **프로비저닝 실패 알림 경로 없음**: `agent_commands.status = 'failed'`가
   쌓여도 관리자가 능동적으로 조회하지 않으면 알 방법이 없습니다 — 최소
   대시보드 배지/카운트(예: overview 페이지에 "실패한 에이전트 명령 N건")
   정도는 필요하다고 열린 질문에 기록합니다. ⚠️ 팀 검토(minor)로 재확인:
   `ui-design.md`가 §3.9~§3.14로 갱신될 때 이 배지가 실제로 반영되지 않았음을
   확인했습니다 — 다른 상태(예: "waiting (no project worker)")는 StatusPill
   변형까지 확정한 것과 대비되는 격차이므로, `#50` 구현 단계에서 이 항목도
   함께 화면에 반영해야 합니다.
5. **[critical, 팀 검토] mTLS 배포 토폴로지가 host당 다중 에이전트(동적
   포트) 모델과 구조적으로 충돌**: 기존 프로덕션 mTLS 배포(`docs/deployment/server-topology.md`
   §3.1.3/§4.1, `MtlsProxy::bind(listen_addr, upstream_addr, ...)` —
   `crates/fleet-transport/src/mtls_proxy.rs`, `crates/fleet-worker/src/runner.rs:240`)는
   host당 **고정 단일 로컬 upstream**(`127.0.0.1:2419`)을 `wss://worker-ip:2420`
   하나로 mTLS 종단하는 1:1 구조입니다. 반면 Phase 4가 도입하는 "`agent_id`
   키드 다중 프로세스 레지스트리(동적 포트)"는 host 하나에 N개의 grok
   프로세스가 서로 다른 포트에서 동시에 뜨는 것을 전제로 합니다. 오케스트레이터가
   mTLS 배포에서 특정 agent의 동적 포트로 어떻게 접속하는지(`MtlsProxy`가
   agent별로 라우팅해야 하는지, 포트마다 별도 `MtlsProxy` 인스턴스가
   필요한지 등)가 `#48`~`#52` 어디에도 서술돼 있지 않습니다 — 위 2번 항목
   (동적 포트 할당 범위)이 plain 배포 관점에서만 열려 있고, mTLS 배포에서는
   포트 범위를 정하는 것만으로는 해결되지 않는 라우팅 문제라는 점이
   별개로 드러났습니다. **Phase 4 착수 전 반드시 해소해야 하는 설계
   공백**으로 기록 — 후보안(각 agent 포트마다 별도 `MtlsProxy` 인스턴스
   vs. `MtlsProxy` 자체를 agent-aware 라우터로 확장)은 Phase 0 스파이크
   범위에 포함시킵니다.

## UI/UX 설계

에이전트 관련 화면은 `ui-design.md`(대시보드 화면 설계 정본)에 아래와 같이
추가했습니다 — 데이터/API는 이 문서가, 화면은 `ui-design.md`가 각각 정본을
담당합니다. 대시보드는 사용자 토글형 다크 모드가 아니라 **단일 Apple
Design System**([`ui-design.md`](../ui-dashboard/ui-design.md) §2)임에
유의 — 신규 페이지도 이를 그대로 물려받습니다.

- **에이전트 생성 흐름**: [`ui-design.md`](../ui-dashboard/ui-design.md) §3.12 —
  **단일 폼(진행형 공개), 마법사 아님**으로 결정. `/tasks/new`·`/projects/new`와
  같은 단순 폼 컨벤션을 유지하는 게 다단계 마법사보다 구현 비용이 낮고,
  host→project가 자동 파생이라 실제로 분기하는 단계가 없기 때문.
- **에이전트 메모리 UI**: [`ui-design.md`](../ui-dashboard/ui-design.md) §3.13 —
  **읽기 전용 목록 + 항목별 수동 삭제**로 결정(자동 보존/정리 정책은 여전히
  §12 열린 질문이나, 구현 전까지 이 수동 삭제가 유일한 정리 수단이 되도록
  UI에 미리 반영).
- **`agents.name`과 `worker.name`의 혼동 위험**: [`ui-design.md`](../ui-dashboard/ui-design.md)
  §3.11 인터랙션에 명시 — 대시보드/CLI는 항상 `agents.name`만 노출하고 내부 `worker.name`은
  어디에도 노출하지 않는 규칙으로 확정.
- **신규 페이지 컨벤션 상속**: `/agents`, `/admin/agent-templates`,
  `/admin/mcp-servers`도 `ui-design.md` §2 디자인 시스템·§6 공통 컴포넌트·
  §8 반응형 전략을 그대로 물려받습니다 — 새 컨벤션을 만들지 않습니다.

## 관련 문서

- [`docs/roadmap/roadmap.md`](../roadmap/roadmap.md) #49 — 구현 진행 상황 정본.
- [`docs/architecture/log.md`](log.md) — 이 설계에 도달한 경위(개정 이력).
- [`docs/architecture/project-feature-design.md`](project-feature-design.md) — `#48`.
- [`docs/architecture/agent-terminal-access-design.md`](agent-terminal-access-design.md) — `#50`,
  `#49` Phase 4에 전적으로 의존하는 후속 확장(tmux 기반 터미널 모니터링/attach).
- [`docs/architecture/agent-harness-composition-design.md`](agent-harness-composition-design.md) — `#51`,
  이 문서의 도구 바인딩·프롬프트 조립을 Skill·프로젝트 헌법으로 확장.
- [`docs/architecture/agent-runtime-vendor-design.md`](agent-runtime-vendor-design.md) — `#52`,
  `GrokRunner`를 벤더 중립 `AgentRunner` 트레잇으로 일반화(grok build/Gemini CLI 등).
- [`docs/ui-dashboard/ui-design.md`](../ui-dashboard/ui-design.md) §3.11~§3.14 —
  화면 설계 정본.
