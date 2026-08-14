# 에이전트 하네스 구성 설계 (Skill · 프로젝트 헌법 · 계층 모델)

> 작성일: 2026-08-15. 로드맵 [`#51`](../roadmap/roadmap.md)에 대응하는 설계
> 문서입니다. [`#49` 에이전트 동적 프로비저닝 설계](agent-provisioning-design.md)의
> custom_prompt/도구 바인딩 위에 쌓이는 후속 확장이며, `#49` Phase 2(템플릿/
> 카탈로그/도구 바인딩)와 함께 구현하는 것을 권장합니다(완전히 같은 패턴을
> 재사용하므로 분리 구현할 이유가 약함). 아직 구현되지 않았습니다. 개정
> 이력은 [`log.md`](log.md)를 참고하세요.

## 1. 배경 및 목적

`#49` 1차 요구사항 10번은 "custom 프롬프트를 tool **혹은 skill**과 연결"을
요구했지만, 지금까지의 설계는 tool(MCP)만 구현하고 skill은 누락돼
있었습니다. 사용자가 "에이전트-호스트-custom 프롬프트-tool의 관계와 층위를
분석하고, 여기에 project/task/skill까지 결합하며, 하네스 엔지니어링 요소
도입도 검토하라"고 요청 — 이 문서는 그 분석과 그로부터 나온 설계를
기록합니다.

## 2. 관계·층위 분석 — 세 개의 독립된 축

지금까지의 설계를 다시 보면, 서로 다른 세 가지 질문이 하나의 "Agent
설계"로 뭉뚱그려져 있었습니다. 명시적으로 axis를 분리하면 각 요소의 관계가
선명해집니다.

### 축 1 — WHERE(물리적 배치): `#48`/`#49`에서 이미 완전히 설계됨

`Project → Host → Agent → Worker`. **Host는 순수하게 인프라적**입니다 —
용량(`max_agents`)·네트워크 위치·`#50`의 tmux/프로세스 envelope만
결정하고, "무엇을 할 줄 아는가"(축 2)에는 관여하지 않는 것이 원칙입니다.

### 축 2 — WHAT(행동 구성): 이 문서의 핵심 — "페르소나 → 스킬 → 도구" 3계층

| 계층 | 성격 | 질문 |
|---|---|---|
| **custom_prompt**(기존) | 정체성/페르소나. 에이전트당 단일 텍스트, 항상 컨텍스트에 있음, 거의 안 바뀜 | "이 에이전트가 누구인가" |
| **Skill**(신규) | 절차적 지식. 필요할 때만 로드되는 모듈형 "어떻게 하는가" 묶음. 여러 개를 조합 가능, 여러 에이전트/템플릿이 공유 참조 | "이 상황에서 어떤 절차를 따르는가" |
| **Tool/MCP**(기존) | 외부 API 표면. 실제로 뭔가를 "하게" 해주는 손 | "무엇을 실행할 수 있는가" |

custom_prompt가 상황에 따라 어떤 스킬을 꺼낼지 판단하는 상위 판단자이고,
스킬의 절차 안에서 도구를 호출하도록 지시하는 경우가 많다는 점에서
**custom_prompt → Skill → Tool은 위에서 아래로 갈수록 구체적**입니다. 이
3계층은 Claude Code 자신의 하네스(시스템 프롬프트 → Skill 로딩 →
Bash/Read/Edit 같은 하위 도구 호출) 구조와 같은 모양입니다.

### 축 3 — WHEN/스코프(누가 어느 범위에서 결정하는가)

`Project(기본값) → Template(프리셋 묶음) → Agent(인스턴스화, 스냅샷+오버라이드)
→ Task(1회성 추가)`. Tool은 이미 이 체인을 따릅니다
(`agent_template_tools`→`agent_tools`→`task.requested_optional_tools`) —
**Skill도 정확히 같은 체인을 그대로 복제**합니다(§4). 다만 기존 체인엔
"Project" 층이 비어 있었습니다 — `agent_provisioning_mode`/
`workdir_template` 같은 구조적 필드는 있어도, "이 프로젝트의 모든
에이전트가 항상 지켜야 할 행동 지침"을 표현할 필드가 없었습니다. 이 문서가
그 빈 층을 **프로젝트 헌법(constitution)**으로 채웁니다(§7.2).

### Host와 축 2의 실제 관계 — 예외는 하나, "제약"이지 "출처"가 아님

Host는 축 1이므로 축 2를 직접 규정하지 않는 것이 원칙이지만, 예외가
하나 있습니다: **stdio 전송 방식의 MCP 도구는 그 바이너리/자격증명이
물리적으로 그 host에 있어야만 동작**합니다. 지금까지 설계엔 `mcp_servers`
카탈로그에 "어느 host에서 쓸 수 있는가"를 표현할 방법이 없어, 템플릿이
stdio 도구를 바인딩했는데 정작 에이전트가 배정된 host엔 그 바이너리가
없어 조용히 실패할 위험이 있었습니다 — 실제 설계 공백이었습니다. 이
문서는 **Host를 Tool의 출처가 아니라 Tool 가용성에 대한 필터/제약
조건**으로 관계 맺도록 스키마를 보강합니다(§6) — 기존 `WorkerSelector`의
`required_labels` 필터와 정확히 같은 패턴입니다.

## 3. 핵심 설계 결정

| 결정 사항 | 채택안 | 근거 |
|---|---|---|
| Skill을 신규 엔티티로 도입할지 | **도입** — Tool과 완전히 같은 패턴(카탈로그 + 템플릿/에이전트 바인딩 + 태스크 1회성 요청) | `#49` 1차 요구사항 10번이 애초에 요구했으나 누락돼 있던 것을 채움 |
| Skill 내용 저장 방식 | **DB 텍스트**(`skills.content TEXT`, `mcp_servers`와 동일 패턴) | AskUserQuestion으로 확인(2026-08-15). 파일/git 기반(Claude Code 스타일)은 더 강력하지만 host 배포 메커니즘이 새로 필요하고 스크립트 실행이 가능해지면 보안 검토가 훨씬 커짐 — 순수 지침형 스킬로 범위를 좁혀 구현 비용을 낮춤 |
| 프로젝트 헌법(constitution) 도입 여부 | **도입** — `projects.constitution_prompt`, 그 프로젝트의 모든 에이전트에 템플릿/개별 custom_prompt보다 먼저 항상 주입 | 축 3에 비어 있던 "Project 레벨" 층을 채움. 이 저장소 자신이 `agent.md`/`CLAUDE.md`로 쓰는 패턴과 정확히 같은 개념이라 메타적으로도 일관됨 |
| Host↔Tool 가용성 제약 | **`hosts.labels` 신규 추가 + `mcp_servers.required_host_labels`로 stdio 도구만 필터** | Worker에 이미 있는 `labels`/`required_labels` 패턴을 Host에도 확장 — 새 메커니즘을 발명하지 않음 |
| Hooks(PreToolUse 등) 도입 여부 | **개념만 기록, 이번엔 설계 보류**(§7.3) | 오케스트레이터 레벨(디스패치 전/완료 후)은 지금도 구현 가능하지만, grok 세션 **내부** 도구 호출 단위 후킹은 ACP가 그 수준의 개입을 지원하는지 자체가 미검증(`#49` §5.2와 같은 리스크) — 검증 안 된 능력 위에 설계하지 않음 |
| Permission Mode(plan/acceptEdits 등) 도입 여부 | **열린 질문으로만 기록**(§10) | grok 자체에 그런 세션 플래그가 있는지 미확인 |
| 서브에이전트 위임 | **새 엔티티 불필요, 문서화만**(§7.4) | `fleet_dispatch_task`를 `mcp_servers` 카탈로그에 "자기 참조형"으로 등록하면 이미 자연히 가능 |

## 4. 데이터 모델

![Agent Harness Data Model](../assets/diagrams/architecture/agent-harness-data-model.mermaid)

### 신규 마이그레이션 (`017_agent_harness.sql`, `#49`의 `016_agents.sql` 다음 번호)

```sql
-- ── 프로젝트 헌법 ──────────────────────────────────────────────
-- projects는 #48이 만든 테이블 — 여기서 컬럼만 추가.
ALTER TABLE projects ADD COLUMN IF NOT EXISTS constitution_prompt TEXT;
-- NULL이면 프로젝트 레벨 지침 없음(기존 동작 그대로 보존).

-- ── Host 레이블(Worker의 기존 labels 패턴을 Host에도 확장) ─────
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS labels JSONB NOT NULL DEFAULT '{}';

-- mcp_servers는 #49가 만든 테이블 — stdio 전송 도구에만 의미 있는
-- host 요구 레이블 컬럼 추가. http/sse는 host 무관이라 항상 NULL.
ALTER TABLE mcp_servers ADD COLUMN IF NOT EXISTS required_host_labels JSONB;

-- ── Skill 카탈로그 ─────────────────────────────────────────────
CREATE TABLE skills (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    content TEXT NOT NULL,  -- markdown 지침 원문
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- agent_template_tools / agent_tools와 완전히 동일한 구조(RESTRICT 정책
-- 포함 — mcp_servers와 동일한 이유: 참조 중인 skill을 삭제하면 이미 떠
-- 있는 에이전트가 조용히 지침을 잃는 운영 리스크).
CREATE TABLE agent_template_skills (
    template_id UUID NOT NULL REFERENCES agent_templates(id) ON DELETE CASCADE,
    skill_id UUID NOT NULL REFERENCES skills(id) ON DELETE RESTRICT,
    requirement TEXT NOT NULL,   -- 'required' | 'optional'
    PRIMARY KEY (template_id, skill_id)
);

CREATE TABLE agent_skills (
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    skill_id UUID NOT NULL REFERENCES skills(id) ON DELETE RESTRICT,
    requirement TEXT NOT NULL,
    PRIMARY KEY (agent_id, skill_id)
);

-- 태스크가 이번 한 번만 특정 optional skill을 쓰고 싶을 때
-- (requested_optional_tools와 완전히 동일한 패턴).
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS requested_optional_skills JSONB;
```

### `fleet-core` 신규/확장 타입

```rust
// crates/fleet-core/src/skill.rs (신규)
pub struct SkillId(pub Uuid);

pub struct Skill {
    pub id: SkillId,
    pub name: String,
    pub description: Option<String>,
    pub content: String,   // markdown 지침 원문
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// crates/fleet-core/src/project.rs — Project에 필드 추가.
pub struct Project {
    // ...(#48 기존 필드)
    pub constitution_prompt: Option<String>,
}

// crates/fleet-core/src/host.rs — Host에 필드 추가(Worker의 기존
// labels: HashMap<String, String> 패턴과 동일 타입).
pub struct Host {
    // ...(#48/#49 기존 필드)
    pub labels: HashMap<String, String>,
}

// crates/fleet-core/src/agent.rs — McpServerConfig에 필드 추가.
pub struct McpServerConfig {
    // ...(#49 기존 필드)
    /// stdio 전송에서만 의미 있음 — 이 도구를 쓰려는 host가 반드시
    /// 가지고 있어야 하는 label 키 목록(값은 검사하지 않음, Worker의
    /// `matches_labels()`와 동일 시맨틱). http/sse는 항상 빈 벡터.
    pub required_host_labels: Vec<String>,
}
```

`AgentTemplate.template_skills: Vec<(SkillId, ToolRequirement)>`,
`Agent.agent_skills: Vec<(SkillId, ToolRequirement)>`는 `agent_template_tools`/
`agent_tools`와 동일하게 별도 조회 메서드로 다룹니다(구조체 필드로 인라인하지
않음 — 기존 도구 바인딩과 동일한 관례).

## 5. 프롬프트 조립 순서 (전체 확정)

`#49` §5.1/§7이 부분적으로만 서술했던 조립 순서를 이번에 전체 확정합니다 —
디스패치 시점에 다음 순서로 텍스트를 이어붙입니다:

1. **`Project.constitution_prompt`**(project_id가 있고 값이 설정돼 있으면) — 항상.
2. **`Agent.custom_prompt`**(템플릿 기본값 또는 개별 오버라이드) — 항상.
3. **Skill 내용** — 필수 skill은 항상, 옵션 skill은 `task.requested_optional_skills`가
   이름으로 명시 요청한 것만(Tool의 필수/옵션 해석 규칙과 완전히 동일):
   ```text
   attach(session) = agent_skills(agent_id, requirement='required')
                    ∪ { s ∈ agent_skills(agent_id, requirement='optional')
                        | s.skill.name ∈ task.requested_optional_skills }
   ```
4. **Agent 메모리**(`agent_memory`에서 최근 N개, `task.agent_id` 지정 시).
5. **스레드 이력**(있으면, 기존 `build_threaded_prompt()`).
6. **새 태스크 프롬프트**.

Skill은 도구처럼 세션에 "부착"되는 별도 채널이 아니라 **프롬프트 텍스트로
주입**됩니다 — 순수 지침형 스킬(§3 결정)이므로 실행 가능한 리소스가 없어
이 방식으로 충분합니다. 도구(Tool)의 부착 방식(§5.2, Phase 0 검증 대상)과는
독립적입니다.

## 6. Host↔Tool 가용성 검증 (신규 가드)

에이전트 생성(`POST /api/agents`) 또는 도구 바인딩 변경 시, 그 에이전트에
바인딩되는(템플릿 상속분 포함) 각 필수(`required`) MCP 도구에 대해:

- `transport = 'stdio'`이고 `required_host_labels`가 비어있지 않으면,
  대상 host의 `labels`가 그 요구를 전부 만족하는지 확인(`Worker::matches_labels()`와
  동일한 "키 존재 여부만 확인" 시맨틱).
- 만족하지 않으면 `409 Conflict`("host missing required label(s) for tool
  X" + 부족한 레이블 목록)로 생성/바인딩 자체를 차단합니다 — 만들어놓고
  런타임에 조용히 실패하는 것보다 생성 시점에 막는 게 `#49`가 일관되게
  지켜온 원칙(TOCTOU 락, 호스트 삭제 가드 등)과 같습니다.
- `http`/`sse` 전송이거나 `required_host_labels`가 비어있으면 검사를
  스킵합니다(host 무관).

## 7. 하네스 엔지니어링 요소 검토

Claude Code 자신의 하네스 개념을 이 fleet에 대입해 무엇을 지금 들여오고
무엇을 보류할지 정리합니다.

### 7.1 Skill — 도입 (§2~§6)

### 7.2 프로젝트 헌법(Constitution) — 도입

`Project.constitution_prompt`는 CLAUDE.md/`agent.md`가 이 저장소 자체에서
하는 역할과 동일합니다 — "이 프로젝트 안의 모든 에이전트가 항상 지켜야 할
것"(예: 커밋 메시지 컨벤션, 금지된 작업, 우선순위). 템플릿/개별
custom_prompt보다 먼저 주입되므로, 개별 에이전트가 그 지침을 프롬프트
안에서 덮어쓰려 시도해도 헌법이 항상 먼저 프레이밍합니다(완전한 강제는
아님 — LLM 프롬프트 수준의 우선순위이지 하드 제약은 아니라는 점은
명시해야 함, §10).

### 7.3 Hooks — 개념만 기록, 설계 보류

Claude Code의 PreToolUse/PostToolUse류 후킹을 대입하면 두 레벨로 나뉩니다:

- **오케스트레이터 레벨**(디스패치 직전 / 태스크 완료 직후): fleet 자신의
  `Dispatcher` 코드에 확장 포인트를 추가하는 것이라 **지금도 구현
  가능**합니다 — 다만 "무엇을 후킹하고 싶은가"(예: 특정 프로젝트의 모든
  디스패치를 감사 로그에 남기기, 완료 시 Slack 알림)가 아직 구체적인
  요구사항으로 제시되지 않아 이번 라운드에선 설계하지 않습니다.
- **세션 내부 레벨**(grok 세션 안에서 개별 도구 호출을 가로채기): ACP가
  이 정도 개입을 지원하는지 자체가 `#49` §5.2의 도구 바인딩 메커니즘과
  똑같이 **미검증**입니다 — 검증 안 된 능력 위에 설계하면 `#49`가 이미
  겪은 것과 같은 위험(방금 드린 답변을 정정해야 하는 상황)을 반복하게
  됩니다. Phase 0 검증 스파이크(§5.2)의 결과가 나온 뒤 재검토합니다.
- **2026-08-15 추가 발견**: 공개 문서 조사 결과 grok build(fleet이 실제로
  spawn하는 `grok agent serve`와 같은 바이너리로 추정 —
  [`agent-runtime-vendor-design.md`](agent-runtime-vendor-design.md)
  `#52` §1) 자체가 이미 네이티브 **Hooks** 시스템을 갖고 있는 것으로
  확인됐습니다(`grok inspect`로 노출). 즉 위 "세션 내부 레벨" Hooks가
  fleet이 처음부터 새로 만들 필요 없이 **grok의 네이티브 기능을 그대로
  노출/설정**하는 문제일 수 있습니다 — 이 가능성도 `#52`가 확장한 Phase 0
  스파이크 범위에 포함됩니다.

### 7.4 서브에이전트 위임 — 새 엔티티 불필요

`fleet-mcp`가 이미 `fleet_dispatch_task`/`fleet_list_agents` 등 도구를
노출합니다(`docs/architecture/agent-provisioning-design.md` §10). 이
도구 자체를 `mcp_servers` 카탈로그에 "자기 참조형" 항목(오케스트레이터
자신의 MCP 엔드포인트를 가리키는 stdio 또는 http 항목)으로 등록해두면,
그 도구를 필수/옵션으로 바인딩받은 에이전트는 **다른 에이전트에게 태스크를
위임하는 서브에이전트 오케스트레이션**을 자연히 할 수 있게 됩니다 —
새 스키마나 프로토콜이 필요 없습니다. 다만 재귀 위임의 깊이 제한(무한
루프 방지)은 §10 열린 질문으로 남깁니다.

### 7.5 Permission Mode — 열린 질문으로만 기록

grok 자체가 plan/acceptEdits/bypassPermissions류 세션 단위 실행 모드를
CLI 플래그나 세션 설정으로 노출하는지 확인되지 않았습니다. 확인되면
`custom_prompt`와 마찬가지로 프롬프트 주입으로 흉내낼 가능성이 높지만,
프롬프트 수준 지침은 강제력이 없어(§7.2와 동일한 한계) 실제 "위험한 작업
전 확인 요구" 같은 하드 게이트가 필요하면 §7.3의 세션 내부 Hooks만큼의
ACP 개입이 필요할 수 있습니다 — 두 항목이 같은 미검증 전제에 묶여
있습니다.

## 8. RBAC 및 API/CLI/MCP 표면

| 변형 | 직렬화 이름 | 의미 |
|---|---|---|
| `SkillManage` | `skill:manage` | 스킬 카탈로그 CRUD |

`AgentTemplateManage`와 동일 등급(`Admin` 기본) — 스킬도 카탈로그 성격이라
`AgentTemplateManage`에 통합할지 별도로 분리할지는 §10 열린 질문(기본은
분리, `mcp_servers`/`agent_templates` 관리 권한이 이미 분리돼 있는 것과
일관성을 맞추기 위함).

**REST**: `/api/skills/*`(`#49`의 `/api/mcp-servers/*`와 동일한 페어링
관례). `DELETE /api/skills/:id`도 `mcp_servers`와 동일하게 참조 중이면
`409 Conflict` + 참조 중인 template/agent 목록.

**CLI**: `fleet skill register/list/show`(`fleet mcp-server register/list`와
동일 패턴). `fleet agent-template create`에 `--skill <name>:<required|optional>`
플래그 추가(기존 `--tool` 플래그와 대칭).

**MCP 도구**: `fleet_register_skill`, `fleet_list_skills`. `fleet_create_agent`/
`fleet_dispatch_task`는 `requested_optional_skills` 입력을 추가로 받습니다.

**대시보드 UI**: `/admin/skills` 페이지(`ui-design.md` §3.14 "에이전트
템플릿 · MCP 카탈로그 관리"와 같은 절에 세 번째 탭으로 추가 — 새 페이지
패턴을 만들지 않음). 프로젝트 상세(`ui-design.md` §3.10) 헤더에
`constitution_prompt` 존재 여부/미리보기 노출.

## 9. 단계별 구현 계획

`#49` Phase 2(템플릿/카탈로그/도구 바인딩)와 **같은 Phase에서 함께
구현**하는 것을 권장합니다 — Skill 바인딩 테이블이 도구 바인딩 테이블과
스키마·API·UI 패턴이 완전히 동일해 분리 구현하면 같은 코드를 두 번 쓰는
낭비가 생깁니다.

1. `017_agent_harness.sql`(constitution_prompt, hosts.labels,
   required_host_labels, skills 3종 테이블), `fleet-core` 타입, `Store`
   확장(PgStore+MemStore). 신규 테스트: 필수/옵션 skill 해석 규칙(도구와
   동일 공식), host 레이블 부족 시 409 가드, constitution_prompt 조립
   순서.
2. §5의 프롬프트 조립 순서를 `acp_transport.rs`의 프롬프트 조립 지점에
   반영 — constitution → custom_prompt → skill → memory → thread → 새
   프롬프트.
3. §8 API/CLI/MCP/UI 표면.

## 10. 열린 질문

- **`SkillManage`를 별도 권한으로 분리할지 `AgentTemplateManage`에
  통합할지**: 기본은 분리(§8) — 확정은 구현 착수 시.
- **constitution/custom_prompt/skill의 강제력 한계**: 전부 프롬프트
  텍스트 수준 지침이라 LLM이 무시할 가능성을 배제할 수 없습니다 — 하드
  게이트가 필요한 요구사항이 나오면 §7.3/§7.5의 세션 내부 개입 검증이
  선행돼야 합니다.
- **서브에이전트 재귀 위임의 깊이 제한**: `fleet_dispatch_task`를 스스로
  호출할 수 있는 에이전트가 무한 루프를 만들 위험 — 최대 위임 깊이 또는
  같은 프로젝트 내 순환 참조 탐지가 필요할 수 있습니다. Phase 2 구현
  시가 아니라 서브에이전트 위임이 실제로 쓰이기 시작하는 시점에 재검토.
- **Hooks(§7.3)/Permission Mode(§7.5)**: `#49` Phase 0 도구 바인딩 검증
  스파이크에서 ACP의 개입 가능 범위가 밝혀지면 함께 재검토.
- **파일/git 기반 Skill로의 확장 여부**: 이번엔 DB 텍스트로 확정했지만
  (§3), 스킬에 스크립트/리소스 번들이 필요하다는 실사용 요구가 나오면
  별도 항목으로 재검토(호스트 배포 메커니즘 + 스크립트 실행 보안 검토가
  선행돼야 함).

## 관련 문서

- [`docs/roadmap/roadmap.md`](../roadmap/roadmap.md) #51 — 구현 진행 상황 정본.
- [`docs/architecture/log.md`](log.md) — 개정 이력.
- [`docs/architecture/agent-provisioning-design.md`](agent-provisioning-design.md) — `#49`,
  이 문서가 확장하는 도구 바인딩·프롬프트 조립·템플릿 설계.
- [`docs/architecture/agent-runtime-vendor-design.md`](agent-runtime-vendor-design.md) — `#52`,
  이 문서의 `hosts.labels`/`required_host_labels` 가드 패턴을 재사용하고,
  grok build 네이티브 Skill/Hook과의 관계를 검증 스파이크로 다룸.
- [`docs/architecture/project-feature-design.md`](project-feature-design.md) — `#48`,
  `constitution_prompt`를 추가하는 `Project` 엔티티 정본.
