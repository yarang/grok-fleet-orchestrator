# 에이전트(Agent) 동적 프로비저닝 · 메모리 · 스레드 요약 · 도구 바인딩 설계

> 작성일: 2026-08-14. 로드맵 [`#49`](../roadmap/roadmap.md)에 대응하는 설계 문서입니다.
> [`#48` 프로젝트 기능 설계](project-feature-design.md) 위에 쌓이는 후속 확장입니다.
> **개정 (2026-08-14, 2차)**: 최초 작성 이후 사용자가 "custom 프롬프트/도구를
> 통한 에이전트 생성" 요구사항을 추가로 명확히 해 §5(도구 바인딩)·§6(중앙
> 카탈로그·템플릿)을 신설하고 단계별 계획을 재구성했습니다.
> **개정 (2026-08-14, 3차)**: `#48`이 host/worker↔project를 배타적 소유로
> 전면 개정(다대다 → 1:N 배타적)함에 따라, 이 문서도 그 위에 다시 정렬 —
> Agent는 항상 자신이 도는 host의 project_id를 그대로 물려받고(§3),
> `AgentProvisioningMode`(수동/자동)와 자동 생성 에이전트의 유휴 자동 종료
> 정책을 신설했습니다(§4.1).
> **개정 (2026-08-14, 4차)**: 구현 착수 전 `#48`/`#49` 설계 문서 전체를
> 재검토했습니다. `AgentProvisioningMode`의 정의 소유권을 `#48`(`fleet-core::project`)로
> 이전(이 문서는 재수출만 참조), `agents.provisioned_by` 컬럼 신설,
> `mcp_servers` 삭제를 `ON DELETE CASCADE`에서 `RESTRICT`로 변경, §4.1
> 유휴 판단 기준을 프로세스 신호가 아닌 fleet 자체 신뢰 소스 기반으로 전면
> 재작성, §12에 메모리 보존 정책을 열린 질문으로 추가, §13(설치·운영 고려
> 사항)과 §UI/UX 절을 신설했습니다 — 자세한 경위는 `roadmap.md` #49 항목의
> 4차 개정 기록 참고.
> **개정 (2026-08-14, 5차 — 프로토콜/절차 재검토)**: 실제 코드(하트비트
> 프로토콜, `GrokRunner`, 서킷브레이커, `Reconciler`)를 확인해 §4의 절차를
> 재검증했습니다. "인바운드 연결 없음(#42)" 주장의 범위를 정정, `agents`에
> `idle_timeout_secs` 스냅샷 컬럼 신설(프로젝트 삭제 시 좀비 방지),
> `AgentAutoProvisioner`의 `Pending` 상태 누락으로 인한 중복 생성 레이스
> 수정, `agent_commands` ACK 프로토콜(`Starting` 전이 시점 포함) 구체화,
> `hosts.max_agents` 체크의 TOCTOU 레이스 수정, `worker.name` 유일성 요구사항
> 명시, 호스트 삭제 가드(RESTRICT 정책) 신설. 자세한 경위는 `roadmap.md`
> `#49` 항목의 5차 개정 기록 참고. 아직 구현되지 않았습니다 — 진행 상황은
> `roadmap.md` #49 항목을 정본으로 확인하세요.

## 1. 배경 및 사용자 요구사항 원문 요약

**1차 요청**(host당 다중 에이전트, 메모리, 스레드 요약 관련):

1. 하나의 host에서 에이전트가 **여러 개** 동작할 수 있어야 한다.
2. **custom 프롬프트**로 여러 에이전트를 구분해서 쓸 수 있어야 한다.
3. 프로젝트가 **필요한 경우 에이전트를 만들어서 호출**할 수 있어야 한다.
4. 에이전트는 host에 **여유가 있을 때** 만들어서 운영할 수 있어야 한다.
5. 개별 에이전트는 **여러 세션에 걸친 맥락**을 유지해야 한다.
6. 그 맥락은 **메모리**로 관리하고, **프로젝트별로** 스코프돼야 한다.
7. 프로젝트에 속하지 않는 태스크 스레드는 **스레드별 요약**으로 관리한다.
8. 필요하면 결과물을 **디렉토리**로 관리한다.

**2차 요청**(같은 날, 도구/CLI/템플릿 관련 — 위 2번을 구체화):

9. custom 프롬프트는 **오케스트레이터가 중앙 관리**하고, 이를 **CLI**와 연결한다.
10. custom 프롬프트를 **tool 혹은 skill과 연결**시켜서 에이전트를 만들고, 이
    에이전트에게 태스크를 할당하는 구조.
11. **tool과 MCP를 중앙에서 관리**해 필요한 tool을 제공.
12. 에이전트에게 필요한 tool을 미리 연결해주는 **template** 설정.
13. **필수(required) tool과 옵션(optional) tool**이 필요 시 제공되도록.

## 2. 핵심 설계 결정

### 2.1 1차 확인 (AskUserQuestion, 2026-08-14)

| 결정 사항 | 채택안 | 근거 |
|---|---|---|
| Agent ↔ Worker 관계 | **Agent를 신규 엔티티로 도입** (Worker와 분리) | Worker는 저수준 접속/용량 개념 유지, Agent가 custom_prompt·메모리·프로젝트 소속·도구 바인딩을 담당. Running 상태일 때만 정확히 하나의 Worker에 연결(1:0..1) |
| 동적 프로비저닝 범위 | **진짜 동적 프로비저닝** — 오케스트레이터가 실행 중 원격으로 grok 프로세스 시작/종료 | 요구사항을 문자 그대로 만족 |
| 메모리 구현 방식 | **구조화된 텍스트/JSON 누적 + 프롬프트 주입** | 임베딩/벡터DB 같은 신규 인프라 불필요 |
| 로드맵 항목 | **`#49`로 분리** (`#48`과 독립) | 기술 표면이 크게 다름 |

### 2.2 2차 판단 (사용자가 직접 의견을 요청, AskUserQuestion 없이 조사 기반 판단 후 공유)

이번엔 사용자가 "판단"을 직접 요청해, 추가 질문 없이 코드 조사로 근거를 확보한 뒤
판단을 제시하고 그대로 반영했습니다.

- **CLI 연결**: 동의. `fleet-cli`의 기존 `Workers`/`Tasks`/`Token` 등 명령 그룹
  패턴에 `Agent` 그룹을 추가하는 것이 자연스럽습니다(§10).
- **도구(MCP) 바인딩 메커니즘 — 중요한 조사 결과**: `crates/fleet-transport/src/acp_transport.rs:447`의
  세션 생성 호출(`NewSessionRequest::new(cwd)`)은 지금 `cwd`만 넘깁니다. 그런데
  vendor로 들여온 ACP SDK(`vendor/agent-client-protocol-rust-sdk`)에는
  세션에 MCP 서버를 붙이는 `SessionBuilder::with_mcp_server()`
  (`session.rs:164`)가 **이미 존재하지만 fleet 코드는 한 번도 호출한 적이
  없습니다.**

  다만 정밀 조사 결과 이게 단순한 해결책은 아닙니다:
  1. `fleet-transport/Cargo.toml:50`이 `agent-client-protocol` 의존성에
     `unstable_end_turn_token_usage`만 활성화했고, `with_mcp_server()`가 요구하는
     `unstable_mcp_over_acp` 피처는 **꺼져 있습니다** — 지금은 컴파일조차 안 됩니다.
  2. SDK 자체가 이 기능을 "unstable"로 표시하고 있습니다.
  3. `McpServer<Counterpart, Run>`(`mcp_server/server.rs`)은 **외부 MCP
     서버(URL/커맨드)에 연결하는 방식이 아니라, `McpServerConnect` 트레이트를
     구현한 Rust 인프로세스 서버를 세션에 붙이는 방식**입니다. 즉 "카탈로그에
     등록한 임의의 외부 MCP 서버를 그냥 붙인다"가 SDK 기본 기능으로 되는 게
     아니라, **범용 stdio 서브프로세스 프록시 + 범용 HTTP/SSE 프록시를 각각
     한 번씩 `McpServerConnect`로 구현**해야 카탈로그를 순수 데이터(커맨드/인자
     또는 URL)로 관리할 수 있게 됩니다.
  4. 대안으로, `grok`이 `grok build`처럼(`README.md:81`, `~/.config/grok/mcp.json`)
     `grok agent serve`에서도 로컬 MCP 설정 파일을 읽는지는 **확인되지
     않았습니다** — 만약 읽는다면 unstable ACP 피처 없이 `fleet-worker`가 에이전트별
     설정 파일을 host에 써주는 것만으로 해결되는, 더 단순하고 리스크가 낮은 경로입니다.

  **판단**: 데이터 모델(중앙 카탈로그 + 템플릿 + 필수/옵션)은 확신을 갖고
  설계하되(§6), "실제로 grok 세션에 어떻게 연결하는지"는 두 후보 경로(ACP
  `with_mcp_server` 경유 vs grok 자체 로컬 설정 파일 경유) 모두 검증되지 않은
  상태이므로, **구현 착수 시 가장 먼저 검증 스파이크**를 두도록 계획에
  명시합니다(§11 Phase 0). 카탈로그/템플릿의 데이터 모델과 API는 두 경로 중
  어느 쪽이 실제로 동작해도 그대로 재사용 가능하도록 "연결 스펙(전송 방식 +
  커맨드/인자 또는 URL)"만 저장하는 형태로 설계했습니다 — 특정 메커니즘에
  종속되지 않습니다.
- **필수/옵션 도구의 활성화 시점**: **명시적** 선택을 기본안으로 제안합니다 —
  태스크 제출 시 `requested_optional_tools`로 이번 태스크에서 켤 옵션 도구를
  직접 나열합니다. grok이 세션 도중 "이 도구가 더 필요하다"고 판단해 자동으로
  요청하는 지능적 방식은 훨씬 복잡한 양방향 프로토콜이 필요해 이번 단계에서는
  권장하지 않습니다.
- **로드맵 항목 배정**: 새 항목(`#50`)으로 쪼개기보다, **`#49`(Agent 생성 방식
  자체를 다루는 항목) 안에 통합**했습니다 — custom_prompt/도구 바인딩은
  "Agent가 어떻게 만들어지는가"의 본질적인 부분이라 별도 기능이 아니라
  `#49`의 확장이라고 판단했습니다.

### 2.3 3차 결정 (`#48` 하드 격리 개정에 따른 재정렬, 2026-08-14)

`#48`이 host/worker↔project를 배타적 1:N으로 개정하면서(리소스 경쟁 예방이
사용자가 제시한 원칙), 이 문서에도 두 가지가 자연히 딸려 왔습니다:

- **Agent의 project_id는 항상 host에서 상속**: `agents.host_id`가 가리키는
  host의 `project_id`가 곧 그 Agent의 project입니다(host가 일반 풀 소속이면
  `NULL`인 Agent도 만들어질 수 있음 — 프로젝트 전용이 아닌 범용 에이전트).
  §2.2에서 "에이전트가 여러 프로젝트에 공유되는 경우"를 열린 질문으로
  남겨뒀었는데, `#48`의 하드 모델에서는 **host 자체가 이미 배타적으로 한
  프로젝트에만 소속**되므로 그 host 위의 Agent도 자동으로 모호함 없이
  하나의 프로젝트에만 속하게 됩니다 — 이 열린 질문은 사실상 해소됐습니다
  (§12에서 갱신).
- **`AgentProvisioningMode`(수동/자동) 신설**: 사용자가 "동작할 수 있는
  agent를 사용자가 직접 설정하는 방법과 오케스트레이터가 만들어서 사용하는
  것을 허용하는 옵션"을 요청 — `Project.agent_provisioning_mode`로 모델링하고
  (`#48` §3에 필드 추가 완료), `Automatic`일 때 오케스트레이터가 주기적으로
  "이 프로젝트에 대기 태스크가 있고 배정된 host에 여유가 있는가"를 확인해
  자동으로 `agent_commands`(start)를 발행하는 백그라운드 루프를 신설합니다
  (§4.1) — 기존 `Reconciler`/`HealthChecker`/`SessionCleanup` 패턴을 그대로
  재사용합니다.

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
    /// (2026-08-14 4차 개정, 재검토에서 발견한 버그 수정: 이 컬럼 없이는
    /// §4.1이 서술하는 "Manual로 만든 에이전트는 자동 종료 대상 아님" 규칙을
    /// 구현할 방법이 없었음).
    pub provisioned_by: AgentProvisionedBy,
    /// 생성 시점에 `project.agent_idle_timeout_secs`를 그대로 복사한 스냅샷
    /// (2026-08-14 5차 개정 — 프로토콜/절차 재검토에서 발견한 버그 수정).
    /// `#48` §6의 "왜 스냅샷인가(템플릿 라이브 참조 금지)"와 동일한 이유:
    /// 프로젝트를 매번 라이브 조회하면, 이 에이전트의 소속 프로젝트가
    /// 나중에 삭제(`agents.project_id`는 `ON DELETE SET NULL`)돼도 이 행
    /// 자체는 남는데, §4.1의 유휴 스윕이 "project → agent" 방향으로
    /// 순회하므로 프로젝트가 사라진 자동생성 에이전트는 영원히 스윕
    /// 대상에서 빠지는 좀비가 된다. 스냅샷하면 스윕이 agent 자체를 기준으로
    /// 순회할 수 있어 이 문제가 원천적으로 없어진다.
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

// ── 도구(MCP) 바인딩 (2차 확장 신규) ──────────────────────────────
pub struct AgentTemplate {
    pub id: AgentTemplateId,
    pub name: String,
    pub description: Option<String>,
    pub custom_prompt: Option<String>,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 중앙 MCP 도구 카탈로그 항목. "실제로 어떻게 grok 세션에 붙이는지"는
/// §2.2에서 확정하지 못했으므로, 특정 메커니즘에 종속되지 않는 연결 스펙만
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
--막으면 지나치게 엄격하다. 대신 "실행 중(Running/Starting/Pending/Stopping)
-- agent가 있으면 호스트 삭제 자체를 애플리케이션 레벨에서 409로 차단"하는
-- 가드를 hosts 삭제 핸들러에 추가한다(2026-08-14 5차 개정, 정책 결정 —
-- 이전엔 이 가드가 없어 호스트 삭제 즉시 실행 중이던 agent 행과
-- agent_commands가 CASCADE로 조용히 사라지고 실제 grok 프로세스만 고아로
-- 남는 문제가 있었다). RESTRICT처럼 FK 제약으로 강제하지 않는 이유는
-- 이 조건이 status 값에 달려 있어 순수 SQL CHECK/FK로 표현할 수 없기
-- 때문 — mcp_servers RESTRICT는 무조건 차단이라 FK로 충분했지만, 이건
-- 상태 조건부 차단이라 애플리케이션 코드가 필요하다.
CREATE TABLE agents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    host_id UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    worker_id UUID REFERENCES workers(id) ON DELETE SET NULL,
    template_id UUID REFERENCES agent_templates(id) ON DELETE SET NULL,
    name TEXT NOT NULL UNIQUE,
    custom_prompt TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    -- 'manual' | 'automatic' — §4.1 유휴 자동 종료 대상 판단에 필수.
    -- 2026-08-14 4차 개정으로 추가(재검토에서 발견한 누락 컬럼).
    provisioned_by TEXT NOT NULL DEFAULT 'manual',
    -- 생성 시점 project.agent_idle_timeout_secs 스냅샷. 2026-08-14 5차
    -- 개정으로 추가 — 위 fleet-core 타입 주석과 동일한 이유(좀비 방지).
    idle_timeout_secs INTEGER,
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
-- 트랜잭션 안에서 수행한다(2026-08-14 5차 개정 — 잠금 없이 카운트만
-- 하면 동시에 들어온 두 POST /api/agents 요청이 둘 다 "여유 있음"으로
-- 읽어 max_agents를 초과하는 TOCTOU 레이스가 실제로 가능했음).

CREATE TABLE agent_commands (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    host_id UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    command_type TEXT NOT NULL,              -- 'start' | 'stop'
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

-- ── 도구(MCP) 바인딩 (2차 확장 신규) ──────────────────────────────

CREATE TABLE agent_templates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    custom_prompt TEXT,
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

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

-- mcp_server_id는 ON DELETE RESTRICT(2026-08-14 4차 개정, 정책 결정) —
-- CASCADE였다면 관리자가 카탈로그 항목을 지웠을 때 이를 참조하는
-- 템플릿/에이전트에서 도구 바인딩이 조용히 사라진다(운영 리스크). 참조 중인
-- mcp_server는 삭제를 막고, API(`DELETE /api/mcp-servers/:id`)도 참조 존재 시
-- 409 + 참조 중인 template/agent 목록을 응답 본문에 포함한다(§10).
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

> ⚠️ **정정 (2026-08-14, 5차 개정 — 프로토콜/절차 재검토)**: "`fleet-worker`는
> 오케스트레이터로부터 인바운드 연결을 받지 않는다"는 원래 서술은 실측과
> 다릅니다. mTLS 배포에서는 `fleet-worker`가 실제로 인바운드 리스너를
> 엽니다(`MtlsProxy::bind`, 기본 `mtls.listen_addr=0.0.0.0:2420` —
> `crates/fleet-transport/src/mtls_proxy.rs`, `crates/fleet-worker/src/
> runner.rs`). 논-mTLS 배포도 SSH 역터널로 사실상 인바운드가 닿습니다.
> 정확한 원칙은 **"제어 플레인(등록/하트비트/디레지스터)은 항상 아웃바운드
> 폴링만 쓴다"** — 위 인바운드 채널은 태스크/세션 데이터 플레인(grok ACP
> 세션) 전용이라 agent 제어 커맨드에는 부적합합니다. 그래서 아래처럼
> **기존 heartbeat 폴링에 커맨드를 얹는 선택 자체는 유지**하되, 근거
> 문구만 정정합니다. (전체 흐름은 다이어그램 참고, 이번 개정으로 함께
> 갱신됨.) 요약:

1. `POST /api/agents`가 host 여유를 확인하고(아래 "여유" 판단 기준 — **호스트
   행을 `SELECT ... FOR UPDATE`로 잠근 트랜잭션 안에서** 확인/기록) `agents`
   (Pending, `idle_timeout_secs`는 프로젝트 설정을 스냅샷) + `agent_commands`
   (start)를 만든다.
2. 다음 하트비트 응답에 `pending_commands`가 실려 온다 — **이를 위해
   `HeartbeatResponse`(현재 `{ok, desired_state: &'static str, server_time}`
   고정 구조, `crates/fleet-api/src/schema.rs`)를 확장해야 한다는 것 자체가
   Phase 4의 스키마 변경 범위임을 명시**(이전 문서는 이미 확장 가능한
   구조인 것처럼 서술했음).
3. `fleet-worker`가 커맨드를 실행하기 전 **자기 자신 dedup**을 먼저 한다 —
   로컬 `agent_id` 키드 프로세스 레지스트리에 이미 그 agent용 프로세스가
   있으면 spawn을 스킵하고 ack만 재전송한다(하트비트 ack 유실로 같은
   `start` 커맨드가 두 번 내려와도 프로세스가 중복 spawn되지 않도록 —
   `agent_commands` 멱등성은 `command_id` 단위가 아니라 "agent_id당
   프로세스 1개"라는 **효과** 단위로 보장한다, §12).
4. 신규 `POST /v1/workers/agent-commands/:command_id/ack {status}` 엔드포인트로
   먼저 `status: acked`를 보고한다 — 이 시점에 `agent.status`가
   `Pending → Starting`으로 전이한다. **이 전이 시점을 명시하는 것 자체가
   이번 개정의 버그 수정이다**: 이전 문서는 `Starting` 상태로 언제 바뀌는지
   전혀 정의하지 않았고, 그 결과 §4.1의 `AgentAutoProvisioner` eligibility
   체크("Running/Starting 에이전트가 없으면 생성")가 아직 ack되지 않은
   `Pending` 에이전트를 "없음"으로 오판해 **같은 대기 태스크에 대해
   에이전트를 중복 생성**할 수 있는 레이스가 있었다(§4.1에서 수정).
5. `fleet-worker`의 (재작성된, Phase 4) 프로세스 레지스트리가 새 grok
   프로세스를 spawn(동적 포트). 기존 `GrokRunner`는 host당 프로세스
   **1개만** 관리했으므로(`crates/fleet-worker/src/grok_process.rs`), 이번
   확장은 "설정 조정"이 아니라 **agent_id 키드 레지스트리로의 재작성**이다.
   spawn 실패/헬스체크 타임아웃 시 `status: failed`로 ack하고
   `agent.status → Failed` 전이(§4.1엔 없던 실패 경로 — 다이어그램에 반영).
6. 기동 성공 시 새 프로세스가 기존 `/v1/workers/register` 흐름으로 자기
   등록 — `worker.name`에 `agent_id`를 **축약 없이 전체 UUID로** 인코딩해
   상관관계를 확보한다. **왜 전체 UUID인가(이번 개정에서 명시)**:
   `/v1/workers/register`는 이름 유일성을 검사하지 않고 **upsert**한다
   (`crates/fleet-api/src/handlers.rs` — 유일성 검사는 별도의
   `/v1/workers/join`에만 있음). 이름을 짧게 자르면 서로 다른 두 agent가
   같은 축약 이름을 만들어낼 경우 조용히 서로의 워커 레코드를 덮어쓰는
   실제 위험이 있으므로, 스킴 자체로 충돌이 구조적으로 불가능하도록
   전체 UUID를 포함해야 한다.
7. 오케스트레이터가 `agents.worker_id`를 채우고 `status: Starting → Running`
   전이, `status: done`으로 ack.
8. 이후 디스패치는 기존 `Dispatcher`/`WorkerSelector` 경로 그대로.
9. 종료(`stop`)도 동일한 커맨드 큐 패턴을 재사용하되, **기존 `GrokRunner`가
   비정상 종료 시 자동으로 프로세스를 재시작하는 루프**(`restart_delay_secs`
   후 재spawn)를 갖고 있다는 점을 반드시 고려해야 한다 — 신호 없이 바로
   kill하면 그 재시작 루프가 죽은 프로세스를 곧바로 되살려버린다. 그래서
   stop 처리는 **먼저 해당 agent 전용 shutdown 채널로 "의도된 종료"임을
   신호한 뒤에** kill해야 한다(이전 문서에 없던 요구사항 — 이번 개정에서
   명시, 다이어그램 반영).
10. 호스트가 `offline_worker_grace`(기존 `Reconciler` 값, 300초)를 넘겨
    하트비트가 끊기면, 그 호스트의 `pending` 커맨드는 `failed`로, 그 위의
    `Running`/`Starting` 에이전트는 `Failed`로 일괄 전이하는 스윕을
    `AgentAutoProvisioner` 주기에 추가한다(이전엔 이 정리 절차 자체가
    없어서, 호스트가 영구히 오프라인이 되면 그 위 에이전트/커맨드가
    무기한 `Running`/`pending`으로 남아 있었음).

### "여유" 판단 기준

`hosts.max_agents`(운영자 설정 정수 상한)와 현재 **터미널 상태(`Stopped`/
`Failed`)가 아닌** 에이전트 개수를 비교합니다 — `Running`/`Starting`뿐
아니라 **`Pending`도 포함**해야 합니다(2026-08-14 5차 개정: `Pending`은
"아직 실행 중은 아니지만 이미 슬롯을 예약한" 상태이므로 여유 계산에서
빠지면 위 §4 1단계의 `FOR UPDATE` 잠금과 무관하게 과다 생성이 가능해짐).
`HostMetrics` 기반 자동 판단은 §12 열린 질문으로 보류.

### 4.1 수동/자동 프로비저닝 모드 (`#48` 3차 결정 반영)

```rust
// crates/fleet-core/src/project.rs — 정의 소유권은 #48(2026-08-14 4차 개정,
// 재검토에서 소유권 불명확 문제를 바로잡음). 이 문서는 재수출만 참조한다.
pub enum AgentProvisioningMode { Manual, Automatic }
```

`Project.agent_provisioning_mode`(기본값 `Manual` — 이 세션에서 계속 지켜온
"더 보수적인 옵션을 기본값으로" 원칙과 일관됨, 운영자가 명시적으로 켜야
자동화가 시작됨):

- **`Manual`**(기본): 관리자/프로젝트가 `POST /api/agents`로 명시적으로만
  에이전트를 만듭니다 — 지금까지 §4에서 설명한 흐름 그대로. 생성된 Agent의
  `provisioned_by = 'manual'`.
- **`Automatic`**: `default_agent_template_id`가 설정돼 있어야 하며(없으면
  자동 프로비저닝이 동작하지 않음 — 어떤 custom_prompt/도구로 만들지 알
  방법이 없으므로), 신규 백그라운드 루프 **`AgentAutoProvisioner`**가 기존
  `Reconciler`(`fleet-scheduler/src/reconcile.rs`)와 동일한 "설정 + spawn +
  JoinHandle 기반 abort" 패턴으로 주기적으로(예: 30초, `Reconciler`의
  `interval`과 같은 관례) 다음을 확인합니다:
  1. `agent_provisioning_mode = 'automatic'`인 프로젝트 중, `project_id`가
     일치하는 `Pending` 태스크가 있고
  2. 그 프로젝트에 **터미널 상태(`Stopped`/`Failed`)가 아닌** Agent(`Pending`/
     `Starting`/`Running`)가 하나도 없거나, 존재하는 것들이 전부
     `Running`이면서 `has_capacity() == false`이며(2026-08-14 5차 개정 —
     이전엔 `Running`/`Starting`만 체크해, 방금 커맨드는 발행했지만 아직
     `Starting`으로 ack되지 않은 `Pending` 에이전트를 "없음"으로 오판하는
     레이스가 있었음: 같은 30초 스윕 주기 안에 두 번째 틱이 돌면 같은
     대기 태스크에 대해 에이전트를 중복 생성할 수 있었음 — §4 4단계 참고)
  3. 그 프로젝트에 배정된 host 중 `max_agents` 여유가 있는 host가 있으면
  → `default_agent_template_id`로 새 Agent를 만들고(`provisioned_by =
  'automatic'`) `agent_commands`(start)를 발행합니다(§4의 수동 흐름과 완전히
  동일한 커맨드 큐 메커니즘 — 트리거만 다름).

#### 유휴 판단 기준 (2026-08-14 4차 개정 — 전면 재작성)

기존 초안은 "마지막 태스크 완료 후 시간 경과 + 대기 중인 태스크 없음"만
기준으로 삼았으나, 재검토 과정에서 사용자가 **"동작 중인지 판단하는 근거가
정확히 뭐냐, 프로세스 stdio만 본다면 실제로 조용히 작업 중인 에이전트도
타임아웃으로 오판될 수 있다"**고 지적했습니다 — 맞는 지적입니다. 아래처럼
**fleet가 이미 신뢰하는 소스만** 사용하도록 다시 설계합니다. 프로세스
stdio/CPU 사용률 같은 저수준 신호는 쓰지 않습니다 — 노이즈가 많고, "조용히
추론 중"인 정상 동작과 실제 유휴 상태를 구분하지 못합니다.

**"동작 중" 판정**: 에이전트가 다음 중 **하나라도** 참이면 동작 중으로
간주해 타임아웃 대상에서 제외합니다.

1. `tasks` 테이블에 이 `agent_id`를 참조하며 `status = Dispatched`인 행이
   하나라도 있음 — **오케스트레이터가 디스패치 시점에 직접 쓰는 값이라
   실시간·권위 있는 1차 신호**(2026-08-14 5차 개정으로 우선순위 정정: 이전
   문서는 아래 2번을 "가장 신뢰할 수 있는 지표"라고 서술했으나, 실측 확인
   결과 `Worker.active_tasks`는 오케스트레이터가 아니라 **워커 프로세스
   자신이 하트비트로 자기 보고**하는 값이라 최대 한 하트비트 주기(기본
   15초)만큼 지연될 수 있음 — 방금 디스패치된 태스크가 다음 하트비트
   전까지는 `active_tasks`에 반영되지 않는 창이 실제로 존재).
2. 그 에이전트의 `worker_id`에 대해 `Worker.active_tasks > 0`(하트비트로
   갱신, 위 1번의 지연 창을 보강하는 2차 확인 — 1번이 놓칠 수 있는 경우를
   메꾸는 이중 확인이지 그 반대가 아님).
3. `agent_commands`에 이 agent에 대한 `status = 'pending'`인 명령이 있음
   (방금 시작 명령을 냈는데 아직 등록도 끝나지 않은 상태 — 이때 끄면 안 됨).

**타이머 기준 시각**: `GREATEST(agents.created_at, 그 agent의 가장 최근
완료 태스크의 완료 시각)`. 방금 만들어져 아직 태스크를 한 번도 받지 못한
에이전트가 즉시 타임아웃되는 것을 방지합니다.

**레이스 방지**: `AgentAutoProvisioner`의 스윕이 "타임아웃 대상"으로 판단한
직후, 실제로 `agent_commands`(stop)를 발행하기 **직전에 위 3개 조건을 한 번
더 재확인**합니다 — 스윕 판단과 커맨드 발행 사이(같은 tick 내, 보통 수
ms~수백 ms) 새 태스크가 들어왔을 극히 드문 경우를 방어합니다.

**적용 대상**: 위 판단 기준 자체는 `Manual`/`Automatic` 공통 인프라로
구현하지만, 유휴 자동 종료의 **적용 대상은 `Automatic`으로 생성된 에이전트
(`provisioned_by = 'automatic'`)로 한정**합니다 — `agents.idle_timeout_secs`
(생성 시점에 `Project.agent_idle_timeout_secs`를 스냅샷한 값, §3 — **2026-08-14
5차 개정: 프로젝트를 매번 라이브 조회하지 않고 Agent 행 자체에 스냅샷한
값을 쓴다.** 라이브 조회 방식이었다면, 이 에이전트의 소속 프로젝트가
나중에 삭제돼도(`agents.project_id`는 `ON DELETE SET NULL`) 이 agent 행
자체는 남는데, 스윕이 "project → agent" 방향으로 순회하다 보니 프로젝트가
사라진 자동생성 에이전트는 영원히 스윕 대상에서 빠지는 좀비가 되는 버그가
있었음)가 `NULL`이 아니면, `AgentAutoProvisioner`가 같은 주기에 위 기준으로
"동작 중이 아니고" 타이머가 만료된 `automatic` 에이전트를 찾아
`agent_commands`(stop)를 발행합니다. **이 정책이 없으면 자동 생성된
에이전트가 영원히 host 여유를 점유하게 되어, "여유가 있을 때만 만든다"는
원래 동기 자체가 무의미해집니다** — 자동 생성은 반드시 자동 회수와 짝을
이뤄야 합니다. `Manual`로 만든 에이전트(`provisioned_by = 'manual'`)는
판단 기준이 아무리 정교해져도 이 정책의 대상이 아닙니다 — 사람이 명시적으로
만든 것을 시스템이 임의로 끄는 것은 최소 놀람 원칙에 어긋나므로, 이는 판단
정확도와 무관하게 유지되는 정책적 결정입니다.

## 5. Custom 프롬프트 및 도구(MCP) 바인딩

### 5.1 Custom 프롬프트 — 프롬프트 조립 시점 주입

`custom_prompt`는 grok CLI 인자로 넘기지 않습니다(§2.2에서 확인했듯 grok
프로세스 수준 시스템 프롬프트 CLI 표면이 확인되지 않음). 대신 **디스패치
시점 프롬프트 조립 단계**에서 텍스트로 앞에 붙입니다 — 기존
`acp_transport.rs::build_threaded_prompt()`가 스레드 이력을 이어붙이는 것과
정확히 같은 위치, 같은 방식입니다. grok 프로세스 자체는 변경 없이 재사용되고,
custom_prompt를 바꿔도 프로세스 재시작이 불필요합니다.

### 5.2 도구(MCP) 바인딩 — 검증 스파이크 필요

§2.2에서 확인한 대로, 실제 연결 메커니즘은 **두 후보 중 하나**이며 둘 다
미검증입니다:

- **경로 A**: ACP `SessionBuilder::with_mcp_server()` — `unstable_mcp_over_acp`
  피처를 켜고, `mcp_servers` 카탈로그 항목마다(stdio/http 전송 방식별로) 범용
  `McpServerConnect` 프록시 구현체를 통해 세션에 동적으로 붙인다.
- **경로 B**: grok 자체의 로컬 MCP 설정 파일(`grok build`의 `~/.config/grok/mcp.json`과
  유사한 것이 `grok agent serve`에도 있다면) — `fleet-worker`가 에이전트 기동
  직전에 그 host의 해당 경로에 설정 파일을 써주기만 하면 됨. unstable ACP
  피처가 불필요해 리스크가 낮음.

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
- **에이전트 생성 흐름**: `POST /api/agents {template_id, project_id, host_id,
  name}` → 템플릿의 `custom_prompt`/`agent_template_tools`를 그대로
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
  구현 단계에서 확정).
- **프로젝트별 스코프**: `agent_memory`는 `agent_id`로만 키가 잡히지만, Agent
  자체가 `project_id`를 가지므로 결과적으로 프로젝트별로 분리됩니다(에이전트가
  여러 프로젝트에 공유되는 경우는 §12 참고).

## 8. 프로젝트에 속하지 않는 스레드의 요약

`task.project_id`가 `NULL`인 스레드는 Agent 메모리 대상이 아니라
`thread_summaries`로 스레드 단위 요약만 관리합니다. 스레드가 일정 turn 수를
넘으면 요약을 만들고, 이후엔 "저장된 요약 + 최근 원문 turn만" 이어붙입니다.
요약 생성 로직 자체(규칙 기반 vs 모델 기반)는 §12 열린 질문.

## 9. 디렉토리 기반 결과물 관리

`Agent`/`Project`에 `workdir_template` 필드를 둬 `DispatchRequest.cwd` 기본값을
프로젝트별 하위 디렉토리로 맞추는 정도로 1단계 범위를 제한합니다. 오케스트레이터로의
결과물 동기화(rsync/S3 등)는 범위 밖 — 필요성 확인 시 별도 항목.

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

**REST**: `/api/agents/*`, `/api/agent-templates/*`, `/api/mcp-servers/*` —
`#48`과 동일한 `/<resource>` + `/api/<resource>` 페어링 관례.
`DELETE /api/mcp-servers/:id`는 `agent_template_tools`/`agent_tools`가 여전히
그 항목을 참조 중이면 `ON DELETE RESTRICT`(§3)로 인해 실패하므로, API가 이를
`409 Conflict`로 변환하고 응답 본문에 참조 중인 template/agent 목록을
포함합니다(2026-08-14 4차 개정, 정책 결정).

**CLI**(신규 `fleet agent` 명령 그룹, `fleet-cli`의 기존 `Workers`/`Tasks`/`Token`
패턴과 동일): `fleet agent create --template <name> --project <id> --host <id>`,
`fleet agent list`, `fleet agent stop <id>`, `fleet agent memory <id>`,
`fleet agent-template create/list`, `fleet mcp-server register/list`.

**MCP 도구**(fleet-mcp, `fleet_*`): `fleet_create_agent`, `fleet_list_agents`,
`fleet_stop_agent`, `fleet_get_agent_memory`. `fleet_dispatch_task`는
`agent_id`/`requested_optional_tools` 입력을 추가로 받습니다.

**호스트 삭제 가드**(2026-08-14 5차 개정 신설, 정책 결정): `DELETE
/api/hosts/:hostname`은 이 문서가 아니라 기존 호스트 인벤토리 기능(§3.2.5,
`ui-design.md`)이 소유한 엔드포인트지만, `#49`가 그 위에 Agent를 도입하면서
새로운 위험이 생겼습니다 — 삭제 시 `agents.host_id`가 `ON DELETE CASCADE`라
실행 중인 agent 행과 `agent_commands`가 조용히 사라지고 실제 grok
프로세스만 고아로 남습니다. **호스트 핸들러에 가드를 추가해, 그 호스트에
터미널 상태가 아닌(`Pending`/`Starting`/`Running`/`Stopping`) agent가 하나라도
있으면 삭제를 `409 Conflict`로 차단**합니다(참조 중인 agent 목록을 응답
본문에 포함 — `mcp_servers` RESTRICT와 동일한 사용자 경험). 운영자는 먼저
각 agent를 stop해 `Stopped`로 만든 뒤에만 호스트를 삭제할 수 있습니다.
DB 레벨 FK를 RESTRICT로 바꾸지 않는 이유는 §3 SQL 주석 참고(상태 조건부라
순수 FK로 표현 불가).

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
   (Automatic 모드 + 유휴 자동 종료 + 호스트 오프라인 정리 스윕). 실기기
   대상 수동 검증 필수.
5. **Phase 5 — API + CLI + MCP + 대시보드 UI**: §10 표면 전체.

## 12. 열린 질문

- **도구 바인딩 메커니즘 최종 확정**: Phase 0 결과로 확정(§5.2).
- ~~**에이전트가 여러 프로젝트에 공유되는 경우**~~ — **2026-08-14 3차 개정으로
  해소됨**: `#48`이 host↔project를 배타적 1:N으로 확정하면서, host 위의
  Agent도 자동으로 모호함 없이 하나의 프로젝트에만 속하게 됩니다(§2.3).
  `agent_memory`를 복합 키로 바꿀 필요가 없어졌습니다.
- **호스트 리소스 기반 자동 "여유" 판단**: 1단계는 명시적 `max_agents`만
  (§4.1의 `AgentAutoProvisioner`도 동일 기준 사용).
- ~~**`agent_commands` 유실/중복 처리**~~ — **2026-08-14 5차 개정으로 구체화**:
  멱등 단위를 `agent_commands.id`가 아니라 **"agent_id당 프로세스 1개"라는
  효과**로 잡습니다 — `fleet-worker`가 커맨드 실행 전 로컬 `agent_id` 키드
  프로세스 레지스트리를 먼저 확인해, 이미 그 agent용 프로세스가 있으면
  spawn을 스킵하고 ack만 재전송합니다(§4 3단계). 완전히 닫힌 질문은
  아님 — 정확한 ack 재전송 규칙(예: 몇 번까지 재시도)은 Phase 4 구현 시
  확정.
- **스레드 요약 생성 방법**: 규칙 기반 vs 모델 기반, Phase 3에서 결정.
- **`mcp_servers.env`의 시크릿 처리**: 1단계는 평문 저장 — API 키 등 민감값이
  섞이면 `fleet-credentials`(기존 AES-256-GCM 마스터키 암호화)와 연동할지
  검토 필요(신규 발견, 구현 착수 전 재확인 권고).
- **`agent_memory` 보존/정리 정책 미정**(2026-08-14 4차 개정, 재검토에서
  발견한 누락 항목): 현재 설계는 `agent_memory`를 완전히 무제한 누적합니다
  (자동 요약 없음, 삭제 로직 없음) — 장수명 에이전트는 이 테이블이 무한정
  자랍니다. 기존 `SessionCleanup`(로그인 시도 로그의 보존 기간 정리)과 동일한
  패턴으로 보존 기간 또는 최대 건수 기준 정리 잡을 두는 것이 필요할 것으로
  예상됩니다 — Phase 3(메모리 구현) 착수 시 확정. 1단계 설계/구현 자체를
  막지는 않되, 무기한 방치는 안 되므로 명문화해 둡니다.

## 13. 설치·운영 고려 사항 (2026-08-14 4차 개정 신설)

두 설계 문서 모두 "무엇을 만드는가"는 상세하지만 "운영자가 이걸 어떻게
운영하는가"가 비어 있었습니다. 이번 단계에서 전부 해결하지는 않되, 최소한
알려진 리스크로 명문화합니다:

1. **다중 grok 프로세스 로그 수집 부재**: Phase 4에서 host당 여러 grok
   프로세스가 뜨는데, 각 프로세스의 stdout/stderr를 어디로 보낼지 설계가
   없습니다(현재 단일 프로세스도 `fleet-worker`가 별도로 리다이렉트하지 않고
   상속 — 다중 프로세스가 되면 뒤섞입니다). Phase 4 착수 전 확정이 필요한
   항목으로 명시합니다.
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
4. **프로비저닝 실패 알림 경로 없음**: `agent_commands.status = 'failed'`가
   쌓여도 관리자가 능동적으로 조회하지 않으면 알 방법이 없습니다 — 최소
   대시보드 배지/카운트(예: overview 페이지에 "실패한 에이전트 명령 N건")
   정도는 필요하다고 열린 질문에 기록합니다.

## UI/UX 설계 (2026-08-14 4차 개정 신설, 같은 날 후속 라운드에서 상세 설계로 확정)

> ⚠️ **정정**: 최초 이 절은 "다크 모드/`data-theme` 컨벤션을 물려받는다"고
> 서술했으나, 대시보드 정본 [`ui-design.md`](../ui-dashboard/ui-design.md)
> §2를 확인한 결과 실제로는 사용자 토글형 다크 모드가 아니라 **단일 Apple
> Design System**입니다 — 아래를 그에 맞게 정정하고, 아래 각 질문에 대한
> 답을 실제로 확정했습니다.

에이전트 관련 화면은 `ui-design.md`에 아래와 같이 추가했습니다 — 데이터/API는
이 문서가, 화면은 `ui-design.md`가 각각 정본을 담당합니다:

- **에이전트 생성 흐름**: [`ui-design.md`](../ui-dashboard/ui-design.md) §3.12 —
  **단일 폼(진행형 공개), 마법사 아님**으로 결정. `/tasks/new`·`/projects/new`와
  같은 단순 폼 컨벤션을 유지하는 게 다단계 마법사보다 구현 비용이 낮고,
  host→project가 자동 파생이라 실제로 분기하는 단계가 없기 때문.
- **에이전트 메모리 UI**: [`ui-design.md`](../ui-dashboard/ui-design.md) §3.13 —
  **읽기 전용 목록 + 항목별 수동 삭제**로 결정(자동 보존/정리 정책은 여전히
  위 §12 열린 질문이나, 구현 전까지 이 수동 삭제가 유일한 정리 수단이 되도록
  UI에 미리 반영).
- **`agents.name`과 `worker.name`의 혼동 위험**: [`ui-design.md`](../ui-dashboard/ui-design.md)
  §3.11 인터랙션에 명시 — 대시보드/CLI는 항상 `agents.name`만 노출하고 내부 `worker.name`은
  어디에도 노출하지 않는 규칙으로 확정.
- **CLI 대화형 모드**: `fleet agent create`는 §10과 동일하게 **완전 플래그
  기반을 기본**으로 하되, 필수 플래그(`--host`, `--name`)가 비어 있고 stdin이
  TTY이면 대화형으로 값을 물어보는 보조 경로를 추가합니다 —
  `fleet-worker join`처럼 완전히 별도인 대화형 서브커맨드를 새로 만들지
  않아 구현 비용이 낮고, 스크립팅 시엔 플래그만으로 완결됩니다.
- **신규 페이지 컨벤션 상속**: `/agents`, `/admin/agent-templates`,
  `/admin/mcp-servers`도 `ui-design.md` §2 디자인 시스템·§6 공통 컴포넌트·
  §8 반응형 전략을 그대로 물려받습니다 — 새 컨벤션을 만들지 않습니다.

## 관련 문서

- [`docs/roadmap/roadmap.md`](../roadmap/roadmap.md) #49 — 구현 진행 상황 정본.
- [`docs/architecture/project-feature-design.md`](project-feature-design.md) — `#48`.
- [`docs/ui-dashboard/ui-design.md`](../ui-dashboard/ui-design.md) §3.11~§3.14 —
  화면 설계 정본(2026-08-14 신설).
