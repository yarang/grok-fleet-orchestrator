# 멀티 벤더 에이전트 런타임 설계 (grok build · Gemini CLI 등)

> 작성일: 2026-08-15. 로드맵 [`#52`](../roadmap/roadmap.md)에 대응하는 설계
> 문서입니다. [`#49` 에이전트 동적 프로비저닝 설계](agent-provisioning-design.md)
> Phase 4(`GrokRunner` 재작성)와 [`#51` 하네스 구성 설계](agent-harness-composition-design.md)
> 위에 쌓이는 후속 확장입니다. 아직 구현되지 않았습니다. 개정 이력은
> [`log.md`](log.md)를 참고하세요.

## 1. 배경 및 조사 결과

사용자 질문: "이 설계를 grok-build cli에 적용할 수 있는가? 그리고 gemini
cli에도 적용할 수 있는가?" — 조사 결과 두 가지 중요한 사실을 확인했습니다
(출처: [xAI 공식 문서](https://docs.x.ai/build/overview),
[xai-org/grok-build](https://github.com/xai-org/grok-build),
[Gemini CLI ACP 모드 문서](https://geminicli.com/docs/cli/acp-mode/)):

1. **`grok agent serve`는 별개 제품이 아니라 Grok Build(`grok` 바이너리)
   자신의 headless/ACP 서버 모드입니다.** README가 "grok build"를 "다른 MCP
   클라이언트"로 기술한 건 사용자가 그 같은 바이너리를 대화형 TUI 모드로
   터미널에서 직접 쓸 때의 얘기고, `fleet-worker`가 관리하는 건 같은
   바이너리의 headless 모드입니다 — 즉 지금 설계는 이미 grok build에
   적용돼 있는 것에 가깝습니다. **Grok Build는 이미 네이티브 Skills·
   Hooks·Plugins·MCP servers 시스템을 갖고 있습니다**(`grok inspect`로
   확인 가능) — `#51`이 fleet 레벨에 새로 설계한 Skill/Hooks 개념과
   겹칩니다.
2. **Gemini CLI도 ACP를 지원하지만(`gemini --acp`) 전송 방식이 근본적으로
   다릅니다** — grok은 `--bind`로 네트워크(WebSocket)에 리슨하는데,
   Gemini는 **stdio(표준입출력) 기반 JSON-RPC**이고 인증 플래그도 없습니다.
   이건 설정값 차이가 아니라 **트랜스포트 아키텍처 차이**입니다.

`fleet-transport::WorkerTransport`/`AcpTransport`는 이미 프로토콜
중립적(ACP 표준 타입만 사용, grok 문자열 리터럴 없음 — 코드 그라운딩으로
확인)이라 ACP 자체는 걸림돌이 아닙니다. 걸림돌은 딱 둘로 좁혀집니다:
(a) `fleet-worker`가 프로세스를 스폰하고 네트워크로 노출하는 방식이 벤더마다
다름, (b) 벤더가 이미 갖고 있는 네이티브 Skill/Hook/MCP 지원과 `#51`이
설계한 fleet 자체 계층이 중복될 수 있음.

## 2. 핵심 설계 결정

| 결정 사항 | 채택안 | 근거 |
|---|---|---|
| 벤더 확장 지점 | **`fleet-worker`의 스폰 계층만 일반화** — `fleet-transport`(ACP 프로토콜 계층)와 오케스트레이터 측(`Dispatcher`/`WorkerSelector`)은 **변경 없음** | 이미 프로토콜 중립적으로 설계돼 있었음(그라운딩으로 확인) — 문제는 "프로세스를 어떻게 띄우고 네트워크로 노출하는가"뿐 |
| `GrokRunner` → 벤더 중립 트레잇 | **`AgentRunner` 트레잇 + 벤더별 구현체 2종**: `NetworkBindRunner`(grok류 — 스스로 `--bind`해 리슨), `StdioBridgeRunner`(Gemini류 — stdio로만 통신하는 프로세스를 fleet-worker가 로컬 브릿지로 감싸 네트워크에 노출) | grok과 Gemini의 근본적 차이(스스로 리슨 vs stdio 전용)를 하나의 구현으로 억지로 통합하지 않고, `#48`~`#51`이 일관되게 써온 "트레잇 + 구현체" 패턴(`Store`, `WorkerTransport`, `RemoteExecutor`)을 그대로 재사용 |
| 벤더/런타임 카탈로그 | **신규 `agent_runtimes` 테이블** — `mcp_servers`/`skills`와 동일한 카탈로그 패턴(이름, 바이너리 경로 템플릿, 전송 방식, 호출 인자 템플릿) + `agent_templates`/`agents`에 스냅샷 바인딩 | 벤더별 바이너리 경로/인자를 Rust 소스에 하드코딩(`grok_process.rs`처럼)하지 않고 데이터로 관리 — 새 벤더 추가가 코드 변경 없이 카탈로그 등록만으로 가능해짐 |
| Host↔Runtime 가용성 | **`#51`이 도입한 `hosts.labels`/`required_host_labels` 패턴 재사용** | 이미 Tool용으로 만든 메커니즘을 Runtime에도 그대로 적용 — 새 메커니즘 발명 안 함 |
| grok build 네이티브 Skill/Hook과의 관계 | **Phase 0 검증 스파이크로 결정 — 이번엔 fleet 자체 계층(프롬프트 주입, `#51`)을 기본 경로로 유지** | `#49` §5.2와 동일한 상황(두 경로 다 미검증) — grok build의 네이티브 기능이 실제로 어떻게 동작하는지 실기기 확인 없이 설계를 바꾸지 않음(§5) |
| 벤더 선택 스코프 | **AgentTemplate 레벨**(Agent가 인스턴스화 시 스냅샷) | Host 레벨이 아님 — 같은 host 위에 grok 에이전트와 Gemini 에이전트가 공존할 수 있어야 함(`#49`의 host당 다중 에이전트 요구사항과 일관) |

## 3. 데이터 모델

![Agent Runtime Data Model](../assets/diagrams/architecture/agent-runtime-data-model.mermaid)

### 신규 마이그레이션 (`018_agent_runtimes.sql`, `#51`의 `017_agent_harness.sql` 다음 번호)

```sql
CREATE TABLE agent_runtimes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,           -- 예: 'grok', 'gemini-cli'
    description TEXT,
    vendor TEXT NOT NULL,                -- 'grok' | 'gemini' | ... (자유 텍스트, enum 아님 — 새 벤더 추가가 스키마 변경 없이 가능해야 함)
    transport_kind TEXT NOT NULL,        -- 'network_bind' | 'stdio_bridge'
    bin_path_template TEXT NOT NULL,     -- 예: '/usr/local/bin/grok', '/usr/local/bin/gemini'
    -- network_bind 전용(transport_kind='network_bind'가 아니면 NULL):
    invocation_args TEXT,                -- 예: 'agent serve --bind {bind_addr} --secret {secret}'
    -- stdio_bridge 전용(transport_kind='stdio_bridge'가 아니면 NULL):
    stdio_invocation_args TEXT,          -- 예: '--acp'
    -- #51의 Host 가용성 가드와 동일한 패턴 — 이 런타임을 쓰려면 host가
    -- 가져야 하는 label 키 목록(예: 'gemini-cli-installed').
    required_host_labels JSONB,
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 기존 배포와의 호환성: 이미 존재하는 모든 agent_templates/agents는
-- 암묵적으로 'grok'(network_bind, agent serve --bind/--secret)을 쓴 것으로
-- 간주 — 이 마이그레이션이 'grok' 런타임 행을 하나 시드하고, 기존
-- agent_templates.runtime_id/agents.runtime_id를 전부 그 id로 채운다.
INSERT INTO agent_runtimes (name, vendor, transport_kind, bin_path_template, invocation_args)
VALUES ('grok', 'grok', 'network_bind', '/usr/local/bin/grok',
        'agent serve --bind {bind_addr} --secret {secret}');

ALTER TABLE agent_templates ADD COLUMN IF NOT EXISTS runtime_id UUID
    REFERENCES agent_runtimes(id) ON DELETE RESTRICT
    DEFAULT (SELECT id FROM agent_runtimes WHERE name = 'grok');
ALTER TABLE agents ADD COLUMN IF NOT EXISTS runtime_id UUID
    REFERENCES agent_runtimes(id) ON DELETE RESTRICT NOT NULL
    DEFAULT (SELECT id FROM agent_runtimes WHERE name = 'grok');
-- runtime_id도 다른 템플릿 바인딩처럼 생성 시점 스냅샷 — 이미 뜬 Agent는
-- 카탈로그가 나중에 바뀌어도 영향받지 않는다(#49 §6 "왜 스냅샷인가"와 동일).
```

### `fleet-core` 신규 타입

```rust
pub struct AgentRuntimeId(pub Uuid);

pub enum TransportKind { NetworkBind, StdioBridge }

pub struct AgentRuntime {
    pub id: AgentRuntimeId,
    pub name: String,
    pub description: Option<String>,
    pub vendor: String,
    pub transport_kind: TransportKind,
    pub bin_path_template: String,
    pub invocation_args: Option<String>,       // transport_kind=NetworkBind
    pub stdio_invocation_args: Option<String>, // transport_kind=StdioBridge
    pub required_host_labels: Vec<String>,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

`AgentTemplate.runtime_id: AgentRuntimeId`, `Agent.runtime_id: AgentRuntimeId`
(생성 시 템플릿에서 스냅샷, 기본 개별 오버라이드는 지원하지 않음 — 런타임은
"이 에이전트가 무엇으로 도는가"라는 근본적 성격이라 도구/스킬처럼 나중에
바꾸는 게 의미가 없음, §7 열린 질문).

## 4. `AgentRunner` 트레잇과 두 구현체

```rust
// crates/fleet-worker/src/agent_runner.rs (신규, GrokRunner를 대체)
#[async_trait]
pub trait AgentRunner: Send + Sync {
    /// 새 에이전트 프로세스를 스폰하고, 오케스트레이터가 ACP로 접속할 수
    /// 있는 네트워크 엔드포인트(host:port)를 반환한다 — 반환값의 시맨틱은
    /// 두 구현체 모두 동일(호출자인 register 흐름은 구현 차이를 모름).
    async fn spawn(&self, agent_id: AgentId, config: &AgentRuntime) -> Result<(ProcessHandle, SocketAddr), RunnerError>;
    /// 의도된 종료 신호 → 그레이스풀 대기 → 강제 종료(#49 §4 9단계 정책 유지).
    async fn terminate(&self, handle: &ProcessHandle) -> Result<(), RunnerError>;
    /// #50의 tmux capture-pane과 동일한 목적 — 구현체마다 캡처 방식이 다름.
    async fn capture_snapshot(&self, handle: &ProcessHandle) -> Result<String, RunnerError>;
}
```

- **`NetworkBindRunner`**(vendor=grok 등, `transport_kind=network_bind`):
  기존 `GrokRunner`를 그대로 일반화한 것 — `#50`에서 이미 확정한 tmux
  envelope(`tmux new-session -d -s <session> -- <bin> <invocation_args
  치환>`)로 spawn, 프로세스가 스스로 `--bind` 주소에 리슨하므로
  `spawn()`은 그 주소를 그대로 반환. **오케스트레이터 측
  `AcpTransport`는 이 주소로 기존과 완전히 동일하게 WebSocket 연결** —
  변경 없음.
- **`StdioBridgeRunner`**(vendor=gemini 등, `transport_kind=stdio_bridge`,
  신규): `<bin> <stdio_invocation_args>`를 로컬 자식 프로세스로 spawn하고,
  **`fleet-worker` 자신이 그 프로세스의 stdin/stdout 파이프를 로컬
  루프백 WebSocket 리스너로 브릿지**한다(raw JSON-RPC 바이트를 그대로
  중계 — `#50` §5의 인터랙티브 attach 릴레이와 같은 성격의 "바이트
  파이프 연결" 코드를 재사용 가능). `spawn()`은 그 로컬 리스너의 주소를
  반환 — **이 지점이 핵심 설계 통찰입니다: 브릿지가 grok과 똑같은
  모양(network_bind 엔드포인트)을 오케스트레이터에 제공하므로,
  오케스트레이터 측 코드(`AcpTransport`/`Dispatcher`/`WorkerSelector`)는
  Gemini류 벤더를 위해 단 한 줄도 바뀌지 않습니다.** 벤더 차이는
  전적으로 `fleet-worker` 안에 갇힙니다.
- tmux 매핑(`#50`)은 두 구현체 모두에 적용됩니다 — `StdioBridgeRunner`도
  자식 프로세스를 tmux 세션 안에서 spawn해 모니터링/attach 이점을 그대로
  누립니다(브릿지 자체는 tmux 세션 밖, fleet-worker 프로세스 안에서
  가볍게 실행 — 브릿지는 파이프 릴레이일 뿐 별도 모니터링 대상이 아님).

## 5. grok build 네이티브 Skill/Hook/MCP와의 관계 — Phase 0 스파이크로 결정

`grok inspect`가 "config sources, instructions, skills, plugins, hooks,
and MCP servers"를 보여준다는 건, grok build가 이미 완결된 네이티브
확장 시스템을 갖고 있다는 뜻입니다. 이건 `#49` §5.2가 이미 마주쳤던
것과 정확히 같은 종류의 질문을 Skill/Hook에도 그대로 제기합니다 — **경로
A(fleet 자체 계층 — `#51`의 DB 텍스트 + 프롬프트 주입) vs 경로 B(grok
네이티브 포맷으로 host에 직접 파일 배포)**. 이번 문서는 **어느 쪽이
맞는지 결정하지 않습니다** — 실기기 확인 없이 설계를 바꾸는 건 `#49`가
이미 겪은 실수(방금 드린 답변을 정정해야 했던 사례)를 반복하는 것이기
때문입니다. 대신:

- `#49` Phase 0 검증 스파이크의 범위를 확장해, grok build의 네이티브
  skill/hook/MCP 설정 파일 포맷과 위치(`grok inspect`가 보여주는 "config
  sources")를 함께 조사합니다.
- 만약 grok build가 프로젝트-로컬 설정 디렉토리(`.grok/` 류)에서 skill/MCP
  설정을 읽는다면, `#49` §5.2의 "경로 B"(로컬 설정 파일 배포)와 이 문서의
  Skill 네이티브 통합이 **동일한 메커니즘으로 한 번에 해결**될 가능성이
  높습니다 — `fleet-worker`가 에이전트 기동 직전에 그 host의 project
  workdir(`#48`의 `workdir_template`)에 grok가 읽는 형식으로 설정 파일을
  써주기만 하면 됩니다.
- Gemini CLI는 네이티브 skill/hook 시스템이 있는지 확인되지 않았습니다 —
  이 항목은 grok build 전용 조사입니다.

## 6. RBAC 및 API/CLI 표면

| 변형 | 직렬화 이름 | 의미 |
|---|---|---|
| `AgentRuntimeManage` | `agent_runtime:manage` | 런타임 카탈로그 CRUD |

`AgentTemplateManage`/`SkillManage`와 동일 등급(`Admin` 기본).

**REST**: `/api/agent-runtimes/*`(`#49`의 `/api/mcp-servers/*`와 동일 페어링
관례). `DELETE /api/agent-runtimes/:id`도 참조 중이면 409(`ON DELETE
RESTRICT` — 이미 뜬 에이전트가 자기 런타임을 잃으면 안 되므로 다른
카탈로그보다도 더 엄격하게 적용).

**CLI**: `fleet agent-runtime register/list`. `fleet agent-template create`에
`--runtime <name>` 플래그 추가(생략 시 `grok` 기본값 — 기존 동작 보존).

**대시보드 UI**: `/admin/agent-runtimes`(`ui-design.md` §3.14에 네 번째
탭으로 추가 — 기존 패턴 재사용). Agent 상세(§3.13) 헤더에 runtime Badge
추가(예: "grok" / "gemini-cli").

## 7. 열린 질문

- **런타임을 Agent 레벨에서 오버라이드 허용할지**: 현재는 템플릿
  스냅샷만이고 개별 변경 불가로 설계했습니다 — 실사용에서 "같은 템플릿을
  다른 벤더로 돌려보고 싶다" 요구가 나오면 재검토.
- **`StdioBridgeRunner`의 실제 성능/안정성**: 로컬 파이프↔WebSocket
  브릿지가 추가하는 지연/장애점을 실측해야 합니다 — Phase 0 스파이크
  범위에 포함.
- **Gemini CLI의 네이티브 확장 시스템 존재 여부**: grok build처럼
  skill/hook/plugin이 있는지 미확인 — 있다면 §5와 동일한 경로 A/B 갈림길이
  Gemini에도 생김.
- **벤더별 인증/자격증명 모델 차이**: grok은 `--secret` 기반이지만
  Gemini ACP 모드는 문서상 CLI 레벨 인증 플래그가 없음(ACP 프로토콜
  자체의 `authenticate` 메서드를 쓰는 것으로 보임) — 이 문서의 credential
  전달 방식(`fleet-credentials`와의 연동 여부)은 Phase 0에서 함께 확인.
- **`grok agent serve`가 실제로 Grok Build와 동일 바이너리의 서브커맨드인지
  최종 확인**: §1의 결론은 공개 문서 조사에 기반한 강한 추정이지 이
  저장소 안에서 실기기로 검증한 사실은 아닙니다 — Phase 0에서 `grok
  --version`/`grok --help` 출력으로 확정.

## 관련 문서

- [`docs/roadmap/roadmap.md`](../roadmap/roadmap.md) #52 — 구현 진행 상황 정본.
- [`docs/architecture/log.md`](log.md) — 개정 이력.
- [`docs/architecture/agent-provisioning-design.md`](agent-provisioning-design.md) — `#49`,
  이 문서가 벤더 중립화하는 `GrokRunner`/Phase 0 검증 스파이크의 선행 설계.
- [`docs/architecture/agent-harness-composition-design.md`](agent-harness-composition-design.md) — `#51`,
  이 문서가 재사용하는 `hosts.labels`/`required_host_labels` 가드 패턴과
  Skill 네이티브 통합 갈림길의 선행 설계.
