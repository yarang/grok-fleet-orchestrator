# UI/UX 화면 설계서

이 문서는 Grok Fleet Orchestrator 웹 대시보드의 화면 설계, 사용자 흐름,
내비게이션 패턴, 공통 컴포넌트를 정의합니다. 시스템 아키텍처는
[`architecture.md`](../architecture/overview.md), RBAC 구현 계획은 해당 plan 문서를
참조하세요.

## TL;DR

> ⚠️ **정정 (2026-08-13)**: 아래 "8개 페이지"는 실측과 크게 어긋납니다. 실제
> `crates/fleet-dashboard/src/app.rs`의 HTML 페이지 라우트는 **18개**이고, 이 문서가
> 다루는 것도 실제로는 10개 절(§3.2.5/§3.2.6 포함)입니다. 또한 §3.2.5~§3.2.6·§10.3
> (호스트 인벤토리)은 "P1.5 제안"으로 서술돼 있지만 **이미 마이그레이션·API·라우트가
> 전부 구현되어 배포된 상태**입니다 — 아래 각 절에 정정 배너를 추가했습니다.

- **8개 페이지** 제안: 운영 코어 3 + 인증 2 + 관리 2 + 고급 1 (⚠️ 위 정정 참고 — 실제는 18개 라우트)
- **단일 디자인 시스템**: Apple Design System(white/parchment/dark tiles, Action Blue, SF Pro, pill CTA)
- **3개 핵심 흐름**: 온보딩(Bootstrap → Login → Overview), 일반 운영
  (Login → Overview → Worker → Task), 관리자(User Mgmt → Audit Log)
- **공통 컴포넌트 11종**: StatusPill, Badge, Card, DataTable, EventLog,
  EmptyState, Avatar 등
- **구현 우선순위 3단계**: P0(MVP) → P1(운영 강화) → P2(확장)

---

## 1. 정보 아키텍처

⚠️ **정정 (2026-08-13)**: 아래 트리는 2026-07-20 작성 당시의 제안이며, 실제
`app.rs` 라우터에는 이 트리에 없는 라우트가 7개 더 있습니다: `/tasks/new`,
`/hosts/provision`, `/admin/ssh-keys`, `/verify-email`, `/forgot-password`,
`/reset-password`, `/resend-verification`. `/workers`는 실제로 존재하지 않는
라우트입니다(Overview에 통합됐다는 각주만 있고 실제 경로 자체가 없음 — IA
트리에서 항목으로 나열할 게 아니라 각주로만 남겨야 합니다).

```
fleet.agentthread.dev/
│
├── /                          # 메인 대시보드 (Overview)        [P0]
├── /login                     # 로그인                          [P0]
├── /bootstrap                 # 최초 관리자 설정                [P0]
├── /verify-email              # 이메일 인증                     [P0] (⚠️ 신규 추가, 실제 존재)
├── /forgot-password           # 비밀번호 재설정 요청            [P0] (⚠️ 신규 추가, 실제 존재)
├── /reset-password            # 비밀번호 재설정                 [P0] (⚠️ 신규 추가, 실제 존재)
├── /resend-verification       # 인증 메일 재발송                [P0] (⚠️ 신규 추가, 실제 존재)
│
├── /hosts                     # 호스트 인벤토리                 [구현됨 — 아래 §3.2.5 정정 참고]
├── /hosts/:hostname           # 호스트 상세 (히스토리)          [구현됨]
├── /hosts/provision           # 호스트 프로비저닝                [구현됨] (⚠️ 신규 추가, 실제 존재)
│
├── (`/workers` 경로 자체는 없음 — 워커 목록은 Overview에 통합)
├── /workers/:id               # 워커 상세                       [P1]
│
├── /tasks                     # 태스크 큐                       [P1]
├── /tasks/:id                 # 태스크 상세 (큐에 통합)
├── /tasks/new                 # 새 태스크 생성                  [P1] (⚠️ 신규 추가, 실제 존재)
│
├── /admin/users                # 사용자 관리                     [P1]
├── /admin/activity             # 활동 로그 (작업·워커 이벤트)     [P2]
├── /admin/tools                # MCP 도구 탐색기                 [P2]
├── /admin/ssh-keys             # SSH 키 금고 관리                 [구현됨] (⚠️ 신규 추가, 실제 존재)
│
├── /projects                   # 프로젝트 목록                   [P2] (⚠️ 미구현 — #48, 설계만 완료, 아래 §3.9)
├── /projects/:id               # 프로젝트 상세                   [P2] (⚠️ 미구현 — #48, §3.10)
├── /projects/new               # 프로젝트 생성                   [P2] (⚠️ 미구현 — #48)
├── /agents                     # 에이전트 목록 (전체 host 가로지름) [P2] (⚠️ 미구현 — #49, §3.11)
├── /agents/:id                 # 에이전트 상세 (메모리 브라우저 포함) [P2] (⚠️ 미구현 — #49, §3.13)
├── /agents/new                 # 에이전트 생성                   [P2] (⚠️ 미구현 — #49, §3.12)
├── /admin/agent-templates      # 에이전트 템플릿 관리             [P2] (⚠️ 미구현 — #49, §3.14)
└── /admin/mcp-servers          # MCP 도구 카탈로그 관리           [P2] (⚠️ 미구현 — #49, §3.14)
```

> `/projects*`·`/agents*`·`/admin/agent-templates`·`/admin/mcp-servers`는
> 2026-08-14 재검토(`roadmap.md` `#48`/`#49` 4차 개정) 결과 이 문서에 추가된
> **설계만 완료된 미구현 라우트**입니다 — 다른 라우트처럼 실측 확인된 것이
> 아니라, `#48`/`#49` 구현 착수 시 이 IA 트리를 정본으로 따르라는 목적입니다.

### 라우트 가드 매트릭스

⚠️ **정정 (2026-08-13)**: `/admin/tools`의 실제 권한 검사는 `operator`가 아니라
`DashboardView`(viewer도 보유)입니다 — `admin_tools_page`/`list_tools_api` 둘 다
`PermissionKind::DashboardView`만 검사합니다(`crates/fleet-dashboard/src/
handlers.rs`). 실제 도구 실행(MCP 프로토콜 자체)에는 RBAC 검사가 전혀 없습니다
— "도구 호출은 operator 이상"이라는 서술은 현재 코드에 대응하는 강제 로직이
없습니다. 또한 표의 `administrator` 역할명은 실제 코드의 역할 식별자와 다릅니다
— 실제로는 `admin`/`operator`/`viewer`(`Role::as_str()`, `crates/fleet-core/src/
auth.rs`)입니다.

| 라우트           | 인증 | 최소 권한    | 비고                          |
| ---------------- | ---- | ------------ | ----------------------------- |
| `/login`         | ✗    | -            | 이미 로그인 시 `/`로 리다이렉트 |
| `/bootstrap`     | ✗    | -            | OTP 토큰 필요, 1회성          |
| `/`              | ✓    | viewer       | 기본 랜딩                    |
| `/hosts`         | ✓    | viewer       | 읽기 전용                     |
| `/hosts/:hostname` | ✓  | viewer       | 읽기 전용                     |
| `/hosts/provision` | ✓  | admin        | `HostProvision` 권한 필요(기본 admin 전용) |
| `/workers/:id`   | ✓    | viewer       | 읽기 전용                     |
| `/tasks`         | ✓    | viewer       | 읽기 전용                     |
| `/tasks/new`     | ✓    | viewer(+ `TaskCreate` for 생성) | 목록은 viewer, 생성 API는 `TaskCreate` |
| `/admin/users`   | ✓    | admin        | `UserRead` 권한 필요(기본 admin 전용) |
| `/admin/activity`| ✓    | viewer       | `EventsList` — 전 역할 열람   |
| `/admin/tools`   | ✓    | viewer       | ⚠️ 정정: `DashboardView`만 검사, operator+ 강제 없음 |
| `/admin/ssh-keys`| ✓    | admin        | `HostProvision` 권한 필요(기본 admin 전용) |
| `/projects`      | ✓    | viewer       | `ProjectRead` — 읽기 전용 (⚠️ 미구현, #48) |
| `/projects/:id`  | ✓    | viewer       | `ProjectRead`, 배정/삭제 액션은 `ProjectAssign`/`ProjectDelete` (⚠️ 미구현) |
| `/projects/new`  | ✓    | admin        | `ProjectCreate` 권한 필요(기본 admin 전용, operator는 열람만) (⚠️ 미구현) |
| `/agents`        | ✓    | viewer       | `AgentRead` — 읽기 전용 (⚠️ 미구현, #49) |
| `/agents/:id`    | ✓    | viewer       | `AgentRead`, 정지/편집은 `AgentDelete`/`AgentManage` (⚠️ 미구현) |
| `/agents/new`    | ✓    | admin        | `AgentCreate` 권한 필요(기본 admin 전용) (⚠️ 미구현) |
| `/admin/agent-templates` | ✓ | admin  | `AgentTemplateManage` 권한 필요(기본 admin 전용) (⚠️ 미구현) |
| `/admin/mcp-servers`     | ✓ | admin  | `AgentTemplateManage` 권한 필요(기본 admin 전용) (⚠️ 미구현) |

---

## 2. 디자인 시스템

> 이 문서는 루트 [`DESIGN-apple.md`](../../DESIGN-apple.md)를 정본으로 삼아,
> 실제 대시보드 구현과 동일한 토큰/레이아웃 원칙으로 다시 쓰였다. 기존의
> 이중 테마 언어는 제거하고, 단일 Apple Design System으로 통합한다.

### 2.1 핵심 원칙

- **사진 중심, chrome 은 거의 보이지 않게**: 전체 페이지는 제품/상태를 강조하는
  tile 기반 섹션으로 구성한다.
- **단일 액센트**: 모든 클릭 가능한 요소와 포커스 표시에는 Action Blue(`#0066cc`)만 사용한다.
- **전역 네비게이션은 검은색**: 상단 글로벌 nav는 `surface-black`를 사용하고, 섹션별
  sub-nav는 parchment 배경 위에 frosted-glass 느낌을 유지한다.
- **CTA는 pill**: Primary 버튼은 완전한 capsule 모양의 pill CTA로 표시한다.
- **공백은 구조다**: 섹션 간 간격은 최소 80px, 카드/utility grid는 24px 패딩으로 정리한다.

### 2.2 색상 토큰

| 토큰 | 값 | 용도 |
| --- | --- | --- |
| `primary` | `#0066cc` | 링크, 버튼, 포커스, 활성 상태 |
| `primary-focus` | `#0071e3` | 키보드 포커스 링 |
| `primary-on-dark` | `#2997ff` | 다크 타일 위 inline link |
| `canvas` | `#ffffff` | 기본 화이트 섹션 |
| `canvas-parchment` | `#f5f5f7` | parchment 섹션 / footer |
| `surface-pearl` | `#fafafc` | secondary pill/ghost button |
| `surface-tile-1` | `#272729` | 기본 다크 타일 |
| `surface-tile-2` | `#2a2a2c` | 미세 구분용 다크 타일 |
| `surface-tile-3` | `#252527` | 하단/embedded frame 용 |
| `surface-black` | `#000000` | 글로벌 네비게이션 |
| `ink` | `#1d1d1f` | 본문/헤드라인 |
| `body-on-dark` | `#ffffff` | 다크 타일 위 텍스트 |
| `ink-muted-80` | `#333333` | 부드러운 본문 |
| `ink-muted-48` | `#7a7a7a` | footer fine print |
| `hairline` | `#e0e0e0` | 가는 경계선 |
| `divider-soft` | `#f0f0f0` | Pearl button 테두리 |

### 2.3 타이포그래피

| 토큰 | 값 | 적용 |
| --- | --- | --- |
| `hero-display` | 56px / 600 / -0.28px | 히어로 제목 |
| `display-lg` | 40px / 600 / 0 | 섹션 제목 |
| `lead` | 28px / 400 / 0.196px | 서브 헤드라인 |
| `tagline` | 21px / 600 / 0.231px | 카테고리명 / sub-nav |
| `body` | 17px / 400 / -0.374px | 본문 |
| `body-strong` | 17px / 600 / -0.374px | 카드 제목 / 라벨 |
| `caption` | 14px / 400 / -0.224px | 보조 텍스트 |
| `button-utility` | 14px / 400 | utility nav 링크 |
| `fine-print` | 12px / 400 | footer/legal |

- Display 크기에서만 음수 자간을 사용한다.
- 본문은 SF Pro Text, 헤드라인은 SF Pro Display를 사용한다.
- 인터랙티브 텍스트는 모두 Action Blue를 사용하고, 다크 타일에서는 `primary-on-dark`를 사용한다.

### 2.4 반경 / 간격 / 그림자

| 토큰 | 값 | 용도 |
| --- | --- | --- |
| `rounded.none` | `0px` | 전체 폭 tile |
| `rounded.lg` | `18px` | utility card |
| `rounded.pill` | `9999px` | primary CTA / chip / search input |
| `spacing.section` | `80px` | 섹션 수직 여백 |
| `spacing.lg` | `24px` | 카드 padding |
| `spacing.xl` | `32px` | 큰 유틸리티 카드 간격 |
| `shadow.product` | `rgba(0,0,0,0.22) 3px 5px 30px` | 제품 렌더링에만 적용 |

- 카드/버튼에는 그림자 없이 평면 구조를 유지한다.
- 제품 이미지가 surface 위에 놓일 때만 한 번의 soft shadow를 적용한다.
- 배경 전환(light tile ↔ dark tile) 자체가 시각적 구분 역할을 수행한다.

### 2.5 레이아웃 패턴

- **Hero tile**: 전체 폭, 흰색/파치먼트/다크 색상 중 하나로 채우고, 제목·서브 카피·CTA·제품 렌더를 수직 스택으로 배치한다.
- **Utility grid**: store/accessories 페이지에서 3~5열 카드 그리드를 사용한다.
- **Sub-nav**: 상단 global nav 아래에 52px 높이의 frosted sub-nav를 배치하고, 오른쪽 끝에 primary CTA를 둔다.
- **Footer**: parchment 배경에 dense link column을 배치하고, legal row를 하단에 고정한다.

---

## 3. 페이지별 상세 설계

### 3.1 페이지 #1 — 메인 대시보드 (Overview)

**라우트**: `/`  **권한**: viewer+  **스타일**: Apple tile system

**목적**: 시스템 전체 상태를 한 화면에. 운영자의 기본 랜딩.

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 520" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="480" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">Header / Fleet Orchestrator / online</text>
  <rect x="40" y="120" width="820" height="72" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="152" font-family="Inter, sans-serif" font-size="14" fill="#444">Overview metrics row</text>
  <rect x="40" y="208" width="240" height="110" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="300" y="208" width="240" height="110" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="560" y="208" width="300" height="110" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="338" width="820" height="120" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="238" font-family="Inter, sans-serif" font-size="14" fill="#444">Workers panel</text>
  <text x="320" y="238" font-family="Inter, sans-serif" font-size="14" fill="#444">Tasks panel</text>
  <text x="580" y="238" font-family="Inter, sans-serif" font-size="14" fill="#444">Events panel</text>
  <text x="60" y="372" font-family="Inter, sans-serif" font-size="14" fill="#444">arm2-prod • online • idle • closed</text>
</svg>

#### 인터랙션

| 요소              | 동작                                            |
| ----------------- | ----------------------------------------------- |
| 카드 클릭         | 해당 섹션으로 스무스 스크롤                     |
| Worker 행 클릭    | `/workers/:id` 이동                             |
| Task 행 클릭     | `/tasks/:id` 이동 또는 인라인 확장              |
| Events 패널       | SSE 자동 갱신, 과거 100줄 버퍼, 자동 스크롤    |
| Status pill       | `/healthz` 폴링(15s)으로 online/offline 토글    |
| 데이터 갱신 주기  | overview 5s, workers 5s, tasks 10s, events 실시간 |

#### 빈 상태

- Tasks: "No tasks yet — Dispatch one via MCP tools" + 문서 링크
- Events: "Awaiting first event..." + 깜빡이는 커서
- Workers: "No workers registered. Start a worker with `fleet worker join`."

---

### 3.2 페이지 #2 — 워커 상세 (Worker Detail)

**라우트**: `/workers/:id`  **권한**: viewer+  **스타일**: Apple tile system

**목적**: 단일 워커의 건강도, 연결 상태, 작업 이력을 진단.

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 560" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="520" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">← Workers / arm2-prod / online</text>
  <rect x="40" y="112" width="820" height="96" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="224" width="500" height="180" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="560" y="224" width="300" height="180" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="420" width="820" height="90" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="150" font-family="Inter, sans-serif" font-size="14" fill="#444">Identity card / worker summary</text>
  <text x="60" y="262" font-family="Inter, sans-serif" font-size="14" fill="#444">Heartbeat / circuit breaker</text>
  <text x="580" y="262" font-family="Inter, sans-serif" font-size="14" fill="#444">ACP connection / current task</text>
  <text x="60" y="455" font-family="Inter, sans-serif" font-size="14" fill="#444">Recent events panel</text>
</svg>

#### 인터랙션

| 요소                  | 동작                                          |
| --------------------- | --------------------------------------------- |
| Heartbeat 그래프      | 1h / 6h / 24h / 7d 범위 토글                  |
| Circuit state 노드    | 각 상태(closed/open/half-open) 설명 툴팁      |
| "Force reconnect" 버튼 | ⚠️ 미구현 — `fleet-dashboard`/`fleet-scheduler`에 대응하는 엔드포인트/핸들러 없음(2026-08-13 확인). 설계 제안으로만 유지. |
| Recent Events 행 클릭 | Audit Log의 해당 이벤트로 딥링크              |

---

### 3.2.5 페이지 #2.5 — 호스트 인벤토리 (Host Inventory)

> ✅ **정정 (2026-08-13): "P1.5 제안"이 아니라 이미 구현·배포된 기능입니다.**
> `007_hosts.sql` 마이그레이션, `/hosts`·`/hosts/:hostname` 라우트, `/api/hosts`·
> `/api/hosts/:hostname` API, 하트비트를 통한 `grok_version`/`fleet_worker_version`/
> `os_info` 갱신, 프로비저닝 성공/실패 시 `host_events` INSERT까지 전부 실측
> 확인됐습니다(`crates/fleet-store/migrations/007_hosts.sql`, `crates/fleet-core/
> src/host.rs`, `crates/fleet-dashboard/src/app.rs`·`handlers.rs`,
> `crates/fleet-worker/src/registration.rs`, `crates/fleet-dashboard/src/
> provisioning.rs`). 다만 **아래 §"필요 스키마 변경" DDL은 제안 당시 초안이고,
> 실제로 구현된 스키마와 컬럼 구성이 다릅니다** — 상세는 그 절 하단의 정정 참고.

**라우트**: `/hosts`  **권한**: viewer+  **스타일**: Apple tile system

**목적**: 물리/가상 호스트 전체의 가시성 확보. 워커 등록 상태,
grok CLI 설치 여부/버전, 프로비저닝 이력을 한눈에.

> **핵심 차이**: `workers` 테이블은 "현재 등록된 워커"만 추적한다.
> 이 페이지는 `hosts` 테이블을 기반으로, 등록 여부와 무관하게
> 인벤토리에 등록된 모든 호스트를 표시한다.

#### 데이터 소스

| 데이터            | 소스                              | 수집 시점                     |
| ----------------- | --------------------------------- | ----------------------------- |
| 호스트 목록       | `hosts` 테이블 (신규 마이그레이션) | `fleet provision` 실행 시 UPSERT |
| grok 버전         | 하트비트 payload 또는 SSH 프로브   | 하트비트(15s) 또는 프로비저닝 |
| fleet-worker 버전 | 하트비트 payload                   | 하트비트(15s)                 |
| 워커 등록 상태    | `hosts.worker_id` JOIN `workers`   | 실시간                        |
| 프로비저닝 이력   | `host_events` 테이블 (append-only) | 프로비저닝/배포/상태변경 시   |

#### 필요 스키마 변경

```sql
-- 007_hosts.sql — Phase P1.5

CREATE TABLE hosts (
    id              UUID PRIMARY KEY,
    hostname        TEXT UNIQUE NOT NULL,       -- IP 또는 호스트명
    name            TEXT UNIQUE,                -- worker-ec1 등 (표시명)
    ssh_user        TEXT,
    ssh_port        INTEGER NOT NULL DEFAULT 22,
    labels          JSONB NOT NULL DEFAULT '{}',
    region          TEXT,
    -- 런타임 정보 (하트비트/프로브로 갱신)
    grok_version    TEXT,                       -- "0.2.112" 또는 NULL
    fleet_worker_version TEXT,
    os_info         TEXT,                       -- "Linux 6.8.0 aarch64"
    status          TEXT NOT NULL DEFAULT 'unknown',
    -- 'provisioned' | 'online' | 'offline' | 'failed' | 'unknown'
    -- 관계
    worker_id       UUID REFERENCES workers(id) ON DELETE SET NULL,
    -- 타임스탬프
    last_provisioned_at TIMESTAMPTZ,
    last_seen_at   TIMESTAMPTZ,
    registered_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_hosts_status ON hosts(status);
CREATE INDEX idx_hosts_worker ON hosts(worker_id) WHERE worker_id IS NOT NULL;

-- 호스트별 append-only 이벤트 (프로비저닝/배포/상태변경)
CREATE TABLE host_events (
    seq         BIGSERIAL PRIMARY KEY,
    host_id     UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    event_type  TEXT NOT NULL,
    -- 'provisioned' | 'grok_installed' | 'grok_upgraded'
    -- | 'worker_started' | 'worker_stopped' | 'health_check'
    -- | 'deploy_failed' | 'config_changed'
    payload     JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_host_events_host ON host_events(host_id, created_at DESC);
```

> ⚠️ **정정 (2026-08-13)**: 위 DDL은 2026-07-20 제안 초안이며, 실제 구현된
> `hosts` 테이블(`crates/fleet-store/migrations/007_hosts.sql`)은 컬럼 구성이
> 다릅니다 — **`name`/`labels`/`region` 컬럼은 없습니다.** 실제 컬럼:
> `id, hostname(UNIQUE), worker_id, status(provisioned|online|offline|failed),
> ssh_host, ssh_port, ssh_user, grok_version, fleet_worker_version,
> os_info(JSONB — 위 제안의 TEXT가 아님), load_avg, mem_available_mb,
> disk_free_mb, last_heartbeat_at, provisioned_at, created_at, updated_at`.
> `host_events`는 실제로 `id UUID`(제안의 `BIGSERIAL seq`가 아님) +
> `severity` 컬럼이 추가돼 있습니다. 라벨/리전으로 인벤토리를 필터링하려면
> [`bootstrap-release-v0.2.md §3.2.1`](../worker-bootstrap/bootstrap-release-v0.2.md)의
> `host_alias`/`identity_file`/`labels` 확장 제안(아직 미구현)을 참조하세요 —
> 이 문서의 `labels`/`region` 제안과 목적은 비슷하지만 별도 트랙입니다.

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 520" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="480" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">Hosts / Inventory + agent health</text>
  <rect x="40" y="112" width="180" height="60" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="240" y="112" width="180" height="60" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="440" y="112" width="180" height="60" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="640" y="112" width="220" height="60" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="198" width="820" height="240" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <line x1="60" y1="230" x2="820" y2="230" stroke="#e0e0e0" />
  <line x1="60" y1="270" x2="820" y2="270" stroke="#e0e0e0" />
  <line x1="60" y1="310" x2="820" y2="310" stroke="#e0e0e0" />
  <line x1="60" y1="350" x2="820" y2="350" stroke="#e0e0e0" />
  <text x="60" y="144" font-family="Inter, sans-serif" font-size="14" fill="#444">Total 4</text>
  <text x="260" y="144" font-family="Inter, sans-serif" font-size="14" fill="#444">Online 3</text>
  <text x="460" y="144" font-family="Inter, sans-serif" font-size="14" fill="#444">Provisioned 1</text>
  <text x="660" y="144" font-family="Inter, sans-serif" font-size="14" fill="#444">Failed 0</text>
  <text x="60" y="216" font-family="Inter, sans-serif" font-size="14" fill="#444">Host table</text>
  <text x="60" y="256" font-family="Inter, sans-serif" font-size="13" fill="#111">10.0.1.10 • 0.2.112 • v0.1.0 • online • ap-ne-2 • [12 ev]</text>
  <text x="60" y="296" font-family="Inter, sans-serif" font-size="13" fill="#111">10.0.1.11 • 0.2.112 • v0.1.0 • online • ap-ne-2 • [8 ev]</text>
  <text x="60" y="336" font-family="Inter, sans-serif" font-size="13" fill="#111">10.0.2.20 • — • — • provisioned • us-west • [3 ev]</text>
</svg>

#### 인터랙션

| 요소              | 동작                                                |
| ----------------- | --------------------------------------------------- |
| Host 행 클릭      | `/hosts/:hostname` 상세 페이지 이동                 |
| History [N ev] 클릭 | `/hosts/:hostname#events` 이벤트 섹션으로 스크롤    |
| ↻ Refresh 버튼    | 즉시 폴링 트리거                                     |
| Status pill       | online(green) / provisioned(amber) / offline(gray) / failed(red) ⚠️ 정정(2026-08-13): 실제 `hosts.status` 값은 `provisioned\|online\|offline\|failed` 4가지이며, "ready"/"unknown"은 존재하지 않음 |
| 데이터 갱신 주기  | 10s 폴링                                             |

#### SSH Config 자동 임포트 UI 흐름 (New in v0.2)

호스트 인벤토리 우측 상단의 `[Import SSH Config]` 버튼을 통해 로컬 설정을 임포트하고 데이터베이스화하는 화면 설계 스펙입니다.

> ⚠️ **정정 (2026-08-12)**: `.ssh/config` 자동 임포트 자체는 아직 미구현 상태이며, 아래
> 화면 설계는 [`bootstrap-release-v0.2.md §3.2`](../worker-bootstrap/bootstrap-release-v0.2.md)의
> 최신 결정과 맞춰 다음 두 가지를 정정합니다: (1) 신규 `inventory_hosts` 테이블이 아니라
> 기존 `hosts` 테이블에 `host_alias`/`identity_file`/`labels` 컬럼을 확장하는 설계로
> 확정되었고, (2) IdentityFile 자동 수집은 새 암호화 저장소를 만드는 게 아니라 이미
> 구현되어 있는 SSH 키 금고(`ssh_keys` 테이블, `crates/fleet-dashboard/src/provisioning.rs`)를
> 재사용하는 것으로 확정되었습니다.

1. **임포트 진입 버튼**:
   - 위치: `/hosts` sub-nav 우측 끝의 primary pill CTA 버튼 옆.
   - 레이블: `Import SSH Config` (Action Blue 배경의 pill button).
2. **Import Configuration 모달**:
   - 모달 활성화 시 흐릿한 백드롭 필터(backdrop-filter: blur) 처리.
   - **File Selector**: 드래그 앤 드롭이 가능한 파일 업로드 영역(`Drop ~/.ssh/config here or click to upload`).
   - **Text Area**: 복사-붙여넣기 탭을 누르면 나타나는 평문 텍스트 입력창.
   - **YAML Metadata Selector (Optional)**: 호스트별 작업 매칭용 라벨을 결합하기 위해 `labels.yaml` 메타데이터 파일을 추가로 첨부할 수 있는 보조 업로드 영역 제공.
   - **동작**: 사용자가 설정을 첨부하거나 붙여넣은 후 `[Parse & Preview]` 버튼을 클릭.
3. **가져오기 미리보기 (Pre-import Preview)**:
   - 파싱 완료 시 모달 내에 동적으로 파싱된 호스트 목록 테이블을 렌더링.
   - 체크박스를 제공하여 특정 호스트(예: local loopback 등)는 임포트 대상에서 개별 제외할 수 있도록 지원.
   - 호스트 별칭(Host Alias), 실제 목적지 IP(HostName), SSH 계정명(User), Port, 키 파일(IdentityFile) 경로가 표시됨.
   - **Dynamic Label Column**: 각 호스트 행의 우측에 `+ Add Label` 입력 폼 및 Chip 컨테이너를 두어, 가져오기 실행 전에 웹 UI에서 마우스 클릭과 텍스트 입력만으로 라벨 메타데이터(예: `arch=arm64`, `gpu=true`)를 동적으로 커스텀 주입할 수 있는 인라인 에디터 내장.
   - 최종 `[Confirm & Import]` (Pill CTA)를 누르면 `hosts` 테이블(⚠️ 정정: 신규
     `inventory_hosts` 테이블이 아니라 기존 `hosts` 테이블 확장)에 적재되고, 모달이
     닫히며 호스트 목록 테이블이 실시간 리로드됨.
4. **키 파일(IdentityFile) 가져오기 정책 설정**:
   - 모달 하단에 `Private Key Access Option` 라디오 버튼 그룹을 제공하여 보안 취향에 맞춘 수집 레벨을 결정할 수 있도록 설계합니다.
     - **Option 1: 자동 가져오기 (Auto Import, 기본값)**: `.ssh/config` 내에 검출된 `IdentityFile` 로컬 경로를 오케스트레이터 백엔드가 자동으로 읽어들여, ⚠️ 정정: 별도의 새 암호화 저장소가 아니라 **기존 SSH 키 금고**(`ssh_keys` 테이블, `MasterKey` AES-256-GCM 암호화 — `bootstrap-release-v0.2.md §3.2.2` 참조)에 등록합니다. `hosts.identity_file` 컬럼에는 로컬 경로가 아니라 이 금고의 키 이름이 저장됩니다. (추가 수동 액션 없이 원클릭 프로비저닝 가능)
     - **Option 2: 수동 선택적 허용 (Manual Upload On Provision)**: 가져오기 시점에는 키 경로 텍스트만 기록하고, 이후 `/hosts` 페이지에서 실제 프로비저닝 버튼을 누르는 순간 브라우저 모달로 해당 SSH 개인키 파일을 사용자가 수동 지목/업로드하여 연동을 완료하도록 제어합니다(이 경로도 최종적으로는 같은 SSH 키 금고에 등록됩니다).

#### 빈 상태

- No hosts: "No hosts in inventory. Provision one with `fleet provision`."
- Grok 버전 없음: "—" (프로비저닝 전 또는 grok 미설치)

---

### 3.2.6 페이지 #2.6 — 호스트 상세 (Host Detail)

> ✅ **정정 (2026-08-13)**: §3.2.5와 동일하게 이미 구현·배포된 기능입니다.

**라우트**: `/hosts/:hostname`  **권한**: viewer+  **스타일**: Apple tile system

**목적**: 단일 호스트의 전체 히스토리 — 프로비저닝, grok 설치/업그레이드,
워커 시작/중지, 헬스체크 결과를 타임라인으로 표시.

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 560" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="520" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">← Hosts / worker-ec1 (10.0.1.10) • online</text>
  <rect x="40" y="112" width="820" height="84" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="214" width="500" height="180" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="560" y="214" width="300" height="180" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="414" width="820" height="90" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="150" font-family="Inter, sans-serif" font-size="14" fill="#444">Identity / software versions / registration details</text>
  <text x="60" y="252" font-family="Inter, sans-serif" font-size="14" fill="#444">Worker heartbeat / circuit state</text>
  <text x="580" y="252" font-family="Inter, sans-serif" font-size="14" fill="#444">Connection / current task panel</text>
  <text x="60" y="451" font-family="Inter, sans-serif" font-size="14" fill="#444">Event history timeline</text>
</svg>

#### 인터랙션

| 요소              | 동작                                                |
| ----------------- | --------------------------------------------------- |
| ← 뒤로 버튼       | `/hosts` 목록으로 복귀                              |
| Worker ID 링크    | `/workers/:id` 워커 상세로 이동                     |
| Event 행 클릭     | JSON payload 인라인 확장                             |
| Event 필터        | 타입별 필터 (provisioned/grok_*/worker_*/health_check) |
| 데이터 갱신 주기  | 30s 폴링                                            |

#### 데이터 수집 방식

| 데이터            | 방법                              | 트리거                     |
| ----------------- | --------------------------------- | -------------------------- |
| grok 버전         | 하트비트의 `grok_version` 필드 | 워커 하트비트 (15s)        |
| fleet-worker 버전 | 하트비트의 `fleet_worker_version` 필드(⚠️ 정정: `worker_version` 아님) | 워커 하트비트 (15s) |
| OS 정보           | 하트비트의 `os_info` 필드          | 워커 등록 시 1회           |
| 프로비저닝 이력   | `fleet provision` 실행 시 `host_events` INSERT | 프로비저닝 실행 |
| grok 설치 이력    | 프로비저닝 스크립트 실행 시 이벤트 기록 | 프로비저닝 시 1회    |

> ✅ **정정 (2026-08-13)**: "하트비트 확장" 절이 미래형으로 서술돼 있었지만
> 이미 구현되어 있습니다 — `WorkerHeartbeat`(`crates/fleet-core/src/worker.rs`)에
> `grok_version`/`fleet_worker_version`/`os_info` 필드가 있고,
> `crates/fleet-worker/src/registration.rs`가 실제로 수집해 전송합니다.

---

### 3.3 페이지 #3 — 태스크 큐 (Task Queue)

**라우트**: `/tasks`  **권한**: viewer+  **스타일**: Apple tile system

**목적**: 태스크 생명주기 추적, 실패 디버깅.

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 520" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="480" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">Tasks / filters / refresh</text>
  <rect x="40" y="112" width="820" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="188" width="280" height="250" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="340" y="188" width="520" height="250" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="144" font-family="Inter, sans-serif" font-size="14" fill="#444">Stats: pending 3 • dispatched 1 • completed 47 • failed 2</text>
  <text x="60" y="220" font-family="Inter, sans-serif" font-size="14" fill="#444">Task list</text>
  <text x="60" y="258" font-family="Inter, sans-serif" font-size="13" fill="#111">b7e2 • arm2 • done • 14:22 • 3.4s</text>
  <text x="60" y="296" font-family="Inter, sans-serif" font-size="13" fill="#111">a8f3 • arm2 • done • 14:21 • 4.2s</text>
  <text x="60" y="334" font-family="Inter, sans-serif" font-size="13" fill="#111">8d33 • — • pending • 14:19 • —</text>
  <text x="360" y="220" font-family="Inter, sans-serif" font-size="14" fill="#444">Selected task detail</text>
  <text x="360" y="258" font-family="Inter, sans-serif" font-size="13" fill="#111">Timeline • payload • output • logs</text>
</svg>

#### 인터랙션

| 요소              | 동작                                                |
| ----------------- | --------------------------------------------------- |
| 행 클릭           | 인라인 확장(위쪽 타임라인+payload+output)           |
| 필터 드롭다운     | URL 쿼리스트링로 동기화 (`?status=failed&worker=arm2`) |
| ⚠ 아이콘          | 실패한 태스크 재시도 다이얼로그 (operator+)         |
| "Export CSV"      | 현재 필터 기준                                      |

---

### 3.4 페이지 #4 — 로그인 (Login)

**라우트**: `/login`  **권한**: 공개  **스타일**: Apple auth surface

**목적**: 쿠키 세션 발급.

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 520" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="480" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="280" y="80" width="340" height="360" rx="18" fill="#ffffff" stroke="#e0e0e0" />
  <circle cx="450" cy="140" r="36" fill="#0066cc" />
  <text x="430" y="148" font-family="Inter, sans-serif" font-size="24" fill="#ffffff">F</text>
  <text x="390" y="196" font-family="Inter, sans-serif" font-size="18" fill="#111">Sign in to Fleet</text>
  <rect x="320" y="220" width="260" height="44" rx="8" fill="#fafafc" stroke="#e0e0e0" />
  <rect x="320" y="286" width="260" height="44" rx="8" fill="#fafafc" stroke="#e0e0e0" />
  <rect x="320" y="352" width="260" height="44" rx="9999px" fill="#0066cc" />
  <text x="340" y="246" font-family="Inter, sans-serif" font-size="13" fill="#7a7a7a">Email</text>
  <text x="340" y="312" font-family="Inter, sans-serif" font-size="13" fill="#7a7a7a">Password</text>
  <text x="430" y="380" text-anchor="middle" font-family="Inter, sans-serif" font-size="14" fill="#ffffff">Sign in</text>
  <text x="346" y="418" font-family="Inter, sans-serif" font-size="12" fill="#0066cc">Need access? Use bootstrap token</text>
</svg>

#### 인터랙션

| 요소                | 동작                                                    |
| ------------------- | ------------------------------------------------------- |
| Sign in 버튼        | POST `/login`(⚠️ 정정: `/api/auth/login`이 아닙니다 — `app.rs`), 성공 시 `/`로 리다이렉트 |
| 실패 응답           | 입력 아래 적색 텍스트, 흔들림 애니메이션                |
| 5회 실패            | ⚠️ 정정: 15분이 아니라 **60초** 쿨다운입니다(`MAX_FAILED_ATTEMPTS=5`, `FAILED_ATTEMPT_WINDOW_SECS=60`, `crates/fleet-dashboard/src/auth.rs`), "Try again later" 메시지 |
| 비밀번호 👁          | 평문 토글                                               |
| bootstrap 링크      | `/bootstrap` 이동                                       |
| Enter 키            | 폼 제출                                                  |

---

### 3.5 페이지 #5 — 부트스트랩 설정 (Bootstrap)

**라우트**: `/bootstrap`  **권한**: 공개(OTP 필요)  **스타일**: Apple auth surface

**목적**: 최초 관리자 계정 등록. 1회성.

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 520" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="480" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="240" y="80" width="420" height="360" rx="18" fill="#ffffff" stroke="#e0e0e0" />
  <text x="260" y="124" font-family="Inter, sans-serif" font-size="20" fill="#111">Activate your control plane</text>
  <rect x="260" y="148" width="380" height="48" rx="8" fill="#fafafc" stroke="#e0e0e0" />
  <rect x="260" y="220" width="40" height="40" rx="6" fill="#0066cc" />
  <rect x="312" y="220" width="40" height="40" rx="6" fill="#0066cc" />
  <rect x="364" y="220" width="40" height="40" rx="6" fill="#0066cc" />
  <rect x="416" y="220" width="40" height="40" rx="6" fill="#d9d9d9" />
  <rect x="468" y="220" width="40" height="40" rx="6" fill="#d9d9d9" />
  <rect x="520" y="220" width="40" height="40" rx="6" fill="#d9d9d9" />
  <rect x="260" y="290" width="380" height="44" rx="8" fill="#fafafc" stroke="#e0e0e0" />
  <rect x="260" y="350" width="380" height="44" rx="8" fill="#fafafc" stroke="#e0e0e0" />
  <rect x="260" y="410" width="380" height="44" rx="9999px" fill="#0066cc" />
  <text x="274" y="176" font-family="Inter, sans-serif" font-size="13" fill="#7a7a7a">Bootstrap token</text>
  <text x="274" y="316" font-family="Inter, sans-serif" font-size="13" fill="#7a7a7a">Email</text>
  <text x="274" y="376" font-family="Inter, sans-serif" font-size="13" fill="#7a7a7a">Password</text>
  <text x="450" y="438" text-anchor="middle" font-family="Inter, sans-serif" font-size="14" fill="#ffffff">Activate control plane</text>
</svg>

#### 인터랙션

| 요소                  | 동작                                                  |
| --------------------- | ----------------------------------------------------- |
| OTP 입력 박스         | 자동 포커스 이동(6개 박스), 붙여넣기 시 자동 분산     |
| 비밀번호 강도         | ⚠️ 미구현 — `fleet-dashboard`에 zxcvbn 의존성이나 강도 채점 로직 없음(2026-08-13 확인). 설계 제안으로만 유지. |
| Activate 버튼         | POST `/bootstrap`(⚠️ 정정: `/api/bootstrap/activate`가 아닙니다 — `app.rs`), 성공 시 `/`로 |
| 이미 활성화된 경우    | `/login`으로 자동 리다이렉트 + 안내 토스트            |
| OTP 만료/오용         | "Token invalid or expired. Issue a new one."          |

---

### 3.6 페이지 #6 — 사용자 관리 (User Management)

**라우트**: `/admin/users`  **권한**: `admin`(⚠️ 정정: 코드상 실제 역할 식별자는 `administrator`가 아니라 `admin` — `Role::as_str()`, `crates/fleet-core/src/auth.rs`)  **스타일**: Apple auth surface

**목적**: RBAC 관리 패널.

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 560" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="520" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">Fleet Orchestrator / Users &amp; Roles</text>
  <rect x="40" y="112" width="820" height="60" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="188" width="820" height="220" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="424" width="820" height="90" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="144" font-family="Inter, sans-serif" font-size="14" fill="#444">Total 3 • Active 2 • Admins 1 • Pending 0</text>
  <text x="60" y="220" font-family="Inter, sans-serif" font-size="14" fill="#444">User table</text>
  <text x="60" y="258" font-family="Inter, sans-serif" font-size="13" fill="#111">YA • Yarang • admin • active • 2m ago</text>
  <text x="60" y="296" font-family="Inter, sans-serif" font-size="13" fill="#111">JK • Jikang • operator • active • 1h ago</text>
  <text x="60" y="334" font-family="Inter, sans-serif" font-size="13" fill="#111">MS • Minsu • viewer • inactive • 3d ago</text>
  <text x="60" y="456" font-family="Inter, sans-serif" font-size="14" fill="#444">Permission matrix</text>
</svg>

#### 인터랙션

| 요소              | 동작                                                      |
| ----------------- | --------------------------------------------------------- |
| Invite user 버튼  | 모달: 이메일 + 역할 선택 → OTP 생성 → 클립보드 복사       |
| 행 ⋯ 메뉴         | 역할 변경 / 세션 폐기 / 비활성화 / 삭제                   |
| 현재 사용자 행    | 강조 배경(#f6f5f4), "You" 라벨                            |
| Permission matrix | 읽기 전용(설정은 Role edit 모달에서)                      |

---

### 3.7 페이지 #7 — 활동 로그 (Activity Log)

**라우트**: `/admin/activity`  **권한**: viewer (`events:list`)  **스타일**: Apple tile system

> 이 페이지는 작업·워커 **생명주기 이벤트**(`events` 테이블, `/api/events`)를 보여준다.
> 인증/권한 감사 로그(`audit_log` 테이블)는 `/api/audit`가 제공하며 전용 화면은 아직 없다.

**목적**: 보안 컴플라이언스, 침해 탐지, 운영 회귀 분석.

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 560" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="520" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">Audit Log / Filters</text>
  <rect x="40" y="112" width="820" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="188" width="420" height="260" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="480" y="188" width="380" height="260" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="144" font-family="Inter, sans-serif" font-size="14" fill="#444">Metrics: events 1,247 • failed logins 3 • perm changes 1</text>
  <text x="60" y="220" font-family="Inter, sans-serif" font-size="14" fill="#444">Event list</text>
  <text x="60" y="258" font-family="Inter, sans-serif" font-size="13" fill="#111">02:09:37 • yaran • auth • session</text>
  <text x="60" y="296" font-family="Inter, sans-serif" font-size="13" fill="#111">02:08:14 • syst • tasks • task.dispatch</text>
  <text x="500" y="220" font-family="Inter, sans-serif" font-size="14" fill="#444">Detail pane / JSON payload</text>
</svg>

#### 카테고리 컬러 코딩

| 카테고리  | 배경치    | 의미                          |
| --------- | --------- | ----------------------------- |
| `auth`    | blue/20   | 로그인, 로그아웃, 세션        |
| `rbac`    | purple/20 | 역할/권한 변경                |
| `tasks`   | green/20  | 태스크 디스패치/완료/실패     |
| `workers` | amber/20  | 워커 가입/퇴장/circuit        |
| `config`  | gray/20   | 환경설정/핑거프린트 변경      |
| `scheduler` | gray/20 | 시스템 틱 (저잡음)            |

---

### 3.8 페이지 #8 — MCP 도구 탐색기 (MCP Tools)

**라우트**: `/admin/tools`  **권한**: ⚠️ 정정: 실제로는 `viewer`도 접근 가능(`DashboardView`
권한만 검사, `crates/fleet-dashboard/src/handlers.rs`) — "operator+"를 강제하는
코드는 없습니다. MCP 프로토콜 자체(도구 실행 경로)에도 RBAC 검사가 없습니다.
**스타일**: Apple auth surface

**목적**: MCP 도구 자가발견성, AI 클라이언트 연동 가이드.

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 560" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="520" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">Fleet Orchestrator / MCP Tools</text>
  <rect x="40" y="112" width="820" height="64" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="192" width="260" height="120" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="320" y="192" width="260" height="120" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="600" y="192" width="260" height="120" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="40" y="332" width="820" height="160" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="144" font-family="Inter, sans-serif" font-size="14" fill="#444">12 tools exposed via JSON-RPC 2.0 stdio</text>
  <text x="60" y="222" font-family="Inter, sans-serif" font-size="13" fill="#111">fleet_list_workers</text>
  <text x="340" y="222" font-family="Inter, sans-serif" font-size="13" fill="#111">fleet_get_task_status</text>
  <text x="620" y="222" font-family="Inter, sans-serif" font-size="13" fill="#111">fleet_dispatch_task</text>
  <text x="60" y="364" font-family="Inter, sans-serif" font-size="14" fill="#444">Detail panel: fleet_dispatch_task</text>
  <text x="60" y="400" font-family="Inter, sans-serif" font-size="13" fill="#111">Input schema • usage example • metrics</text>
</svg>

> ⚠️ **정정 (2026-08-13)**: 실제 도구는 **12개**이며 전부 `fleet_` 접두사가 붙은
> snake_case 이름입니다(`fleet_dispatch_task`, `fleet_get_task_status`,
> `fleet_list_workers`, `fleet_list_tasks`, `fleet_cancel_task`,
> `fleet_wait_for_task`, `fleet_stream_task_output`, `fleet_collect_results`,
> `fleet_list_hosts`, `fleet_reset_worker_breaker`, `fleet_list_bootstrap_tokens`,
> `fleet_revoke_bootstrap_token` — `crates/fleet-mcp/src/schema.rs`). 뒤 4개는
> 2026-08-13에 로드맵 #28 대응으로 신규 추가됐습니다. 위 목업의 `workers.list`
> 같은 점(dot) 표기 네이밍은 실재하지 않습니다.

---

### 3.9 페이지 #9 — 프로젝트 목록 (Projects)

> 🆕 **설계 제안 (2026-08-14)**: `#48`(프로젝트 기능) 재검토에서 범위만
> 정리했던 UI/UX 열린 질문을 이번에 구체화했습니다. 아직 구현되지
> 않았습니다 — [`project-feature-design.md`](../architecture/project-feature-design.md)를
> 데이터/API 정본으로, 이 절을 화면 정본으로 삼습니다.

**라우트**: `/projects`  **권한**: `ProjectRead`(viewer+)  **스타일**: Apple tile system

**목적**: 등록된 프로젝트 전체 조망 — 배정 규모(host/worker 수)와 실행 중
에이전트 수를 한눈에 비교해, 어느 프로젝트가 리소스를 쓰고 있는지 파악.

#### 데이터 소스

| 데이터 | 소스 | 비고 |
| --- | --- | --- |
| 프로젝트 목록 | `GET /api/projects` | `project-feature-design.md` §7 |
| Host/Worker 배정 수 | `list_project_hosts`/`list_project_worker_ids` 카운트 | 목록 응답에 집계 포함하도록 API 확장 필요(현재 설계엔 없음 — 구현 시 반영) |
| 실행 중 Agent 수 | `#49` `agents` 테이블, `status IN ('Starting','Running')` 카운트 | 위와 동일하게 집계 확장 필요 |

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 480" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="440" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="660" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="712" y="40" width="148" height="56" rx="28" fill="#0066cc" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">Projects</text>
  <text x="740" y="72" font-family="Inter, sans-serif" font-size="13" fill="#ffffff">+ New Project</text>
  <rect x="40" y="112" width="820" height="328" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <line x1="60" y1="152" x2="820" y2="152" stroke="#e0e0e0" />
  <line x1="60" y1="200" x2="820" y2="200" stroke="#e0e0e0" />
  <line x1="60" y1="248" x2="820" y2="248" stroke="#e0e0e0" />
  <text x="60" y="136" font-family="Inter, sans-serif" font-size="13" fill="#7a7a7a">Name • Mode • Hosts • Workers • Running agents • Created</text>
  <text x="60" y="184" font-family="Inter, sans-serif" font-size="13" fill="#111">payments-migration • automatic • 3 • 3 • 2 • 2026-08-01</text>
  <text x="60" y="232" font-family="Inter, sans-serif" font-size="13" fill="#111">docs-refresh • manual • 1 • 1 • 0 • 2026-08-05</text>
  <text x="60" y="280" font-family="Inter, sans-serif" font-size="13" fill="#111">infra-audit • manual • 2 • 2 • 1 • 2026-08-10</text>
</svg>

#### 인터랙션

| 요소 | 동작 |
| --- | --- |
| 행 클릭 | `/projects/:id` 상세로 이동 |
| `+ New Project` | `/projects/new` (name, description, agent_provisioning_mode, workdir_template 입력하는 단일 폼 — 마법사 아님, 기존 `/tasks/new` 같은 단순 폼 컨벤션 재사용) |
| Mode 컬럼 | Badge(`manual` = parchment, `automatic` = Action Blue) |
| 데이터 갱신 주기 | 10s 폴링(기존 Overview/Workers와 동일 관례) |

#### 빈 상태

- No projects: "No projects yet. Create one to scope hosts and agents."(EmptyState, `+ New Project` CTA 포함)

---

### 3.10 페이지 #10 — 프로젝트 상세 (Project Detail)

> 🆕 **설계 제안 (2026-08-14)**: 재검토에서 확정한 섹션 우선순위 — 배정
> host/worker, 실행 중 agent, 최근 태스크 순. **Agent 메모리 브라우저는 이
> 페이지에 두지 않습니다** — 메모리는 `agent_id`로 스코프되므로 §3.13
> 에이전트 상세 페이지가 정본입니다(프로젝트 상세에서는 각 agent 행에서
> 링크만 제공).

**라우트**: `/projects/:id`  **권한**: `ProjectRead`(viewer+), 배정/삭제는 `ProjectAssign`/`ProjectDelete`(admin 기본)  **스타일**: Apple tile system

**목적**: 프로젝트 하나의 리소스 배정과 실행 상태를 단일 화면에서 확인·조작.

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 700" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="660" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="80" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">← Projects / payments-migration • automatic</text>
  <text x="60" y="96" font-family="Inter, sans-serif" font-size="13" fill="#444">idle timeout 900s • workdir /srv/agents/payments-migration</text>
  <rect x="40" y="136" width="820" height="140" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="160" font-family="Inter, sans-serif" font-size="14" fill="#444">배정된 Host / Worker  [+ Assign Host]</text>
  <line x1="60" y1="196" x2="820" y2="196" stroke="#e0e0e0" />
  <text x="60" y="220" font-family="Inter, sans-serif" font-size="13" fill="#111">worker-ec1 (10.0.1.10) → worker#a1b2 • online • 2/5 agents</text>
  <text x="60" y="252" font-family="Inter, sans-serif" font-size="13" fill="#111">worker-ec2 (10.0.1.11) → worker#c3d4 • online • 1/5 agents</text>
  <rect x="40" y="292" width="820" height="140" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="316" font-family="Inter, sans-serif" font-size="14" fill="#444">실행 중 Agent  [+ New Agent]</text>
  <line x1="60" y1="352" x2="820" y2="352" stroke="#e0e0e0" />
  <text x="60" y="376" font-family="Inter, sans-serif" font-size="13" fill="#111">code-reviewer-1 • running • automatic • worker-ec1 →</text>
  <text x="60" y="408" font-family="Inter, sans-serif" font-size="13" fill="#111">migration-bot • starting • manual • worker-ec2 →</text>
  <rect x="40" y="448" width="820" height="212" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="472" font-family="Inter, sans-serif" font-size="14" fill="#444">최근 태스크(이 프로젝트 스코프)</text>
  <line x1="60" y1="508" x2="820" y2="508" stroke="#e0e0e0" />
  <text x="60" y="532" font-family="Inter, sans-serif" font-size="13" fill="#111">#8f21 • dispatched • code-reviewer-1</text>
  <text x="60" y="564" font-family="Inter, sans-serif" font-size="13" fill="#111">#8f19 • ● waiting (no project worker) • retry 2/5</text>
  <text x="60" y="596" font-family="Inter, sans-serif" font-size="13" fill="#111">#8f10 • completed • migration-bot</text>
</svg>

#### 섹션 우선순위 (재검토에서 확정)

1. **헤더**: 이름/설명, `agent_provisioning_mode` Badge, (automatic일 때만)
   `agent_idle_timeout_secs`·`default_agent_template_id` 표시, `workdir_template`,
   Edit/Delete.
2. **배정된 Host/Worker**: `list_project_hosts` 기반 DataTable(호스트 행마다
   연결된 worker와 `has_capacity` 상태를 함께 표시). `+ Assign Host` 모달은
   `project_id IS NULL`인 host만 선택지로 노출(다른 프로젝트 소속 host는
   먼저 그쪽에서 해제해야 함 — 배타적 소유 원칙을 UI에서도 강제). Worker는
   host를 통해 종속적으로만 표시(§3 불변식 — host 배정 시 worker.project_id가
   자동 동기화되므로 독립적인 worker 배정 UI는 두지 않음, 다만 host 없이
   존재하는 워커의 예외 케이스는 별도 "독립 Worker 배정" 접이식 섹션으로
   숨겨둠).
3. **실행 중 Agent**: `agents` 테이블(project_id 일치) DataTable — Name,
   Status pill(Pending/Starting/Running/Stopping/Stopped/Failed), Host,
   `provisioned_by` Badge, Stop 버튼(`AgentDelete`). 행 클릭 시 §3.13
   에이전트 상세로 이동(메모리는 거기서 확인). `+ New Agent` → §3.12.
4. **최근 태스크**: 기존 §3.3 태스크 큐 DataTable을 `project_id` 필터로
   재사용(별도 컴포넌트 신설 없음). 아래 "하드 격리 대기 상태 표시" 참고.

#### 하드 격리 대기 상태 표시 (재검토에서 발견한 UX 리스크 해소)

`SelectionError::NoWorkerForProject`로 재시도 중인 태스크(`project-aware-
dispatch-logic.mermaid` 참고)는 일반 `pending`/`failed`와 시각적으로 구분해야
사용자가 "이 프로젝트에 워커를 안 붙여서 멈춘 것"임을 즉시 알 수 있습니다.
저장된 상태를 늘리지 않고(스키마 변경 없음) **API가 조회 시점에 파생
계산**합니다:

```text
pending_no_project_worker =
    task.status == Pending
    AND task.project_id IS NOT NULL
    AND (list_project_worker_ids(task.project_id) ∩ {online workers}) IS EMPTY
```

- StatusPill 신규 변형: `● waiting (no project worker)` — amber/violet(§6.1
  `circuit_open`과는 다른 색으로, "회로 차단"이 아니라 "배정 자체가 없음"임을
  구분). 일반 재시도 대기(`pending`, gray)와 나란히 놓여도 헷갈리지 않도록
  라벨 텍스트에 사유를 그대로 노출.
- 클릭 시 툴팁/확장: "이 프로젝트에 배정된 워커가 없거나 전부 오프라인입니다.
  `/projects/:id`에서 host를 배정하세요" + 해당 프로젝트로 링크.

#### 인터랙션

| 요소 | 동작 |
| --- | --- |
| `+ Assign Host` | 모달, `project_id IS NULL` host만 선택 가능 |
| Host 행의 Unassign | `DELETE /api/hosts/:id/project` 확인 모달("이 host의 워커도 함께 일반 풀로 돌아갑니다") |
| Agent 행 클릭 | `/agents/:id` |
| `+ New Agent` | `/agents/new?project_id=:id` (§3.12, host 선택지가 이 프로젝트로 사전 필터됨) |
| Task 행의 `waiting (no project worker)` 클릭 | 사유 툴팁 확장 |
| 데이터 갱신 주기 | 10s 폴링 |

---

### 3.11 페이지 #11 — 에이전트 목록 (Agents)

> 🆕 **설계 제안 (2026-08-14)**. 프로젝트 상세(§3.10)가 프로젝트별 뷰라면,
> 이 페이지는 host를 가로지르는 전체 에이전트 뷰입니다 — "이 host가 지금
> 얼마나 바쁜가"를 프로젝트 경계 없이 확인할 때 씁니다.

**라우트**: `/agents`  **권한**: `AgentRead`(viewer+)  **스타일**: Apple tile system

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 460" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="420" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="56" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">Agents</text>
  <rect x="40" y="112" width="820" height="60" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="148" font-family="Inter, sans-serif" font-size="13" fill="#444">FilterBar: Project ▾  Status ▾  Host ▾  Search</text>
  <rect x="40" y="188" width="820" height="232" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <line x1="60" y1="228" x2="820" y2="228" stroke="#e0e0e0" />
  <line x1="60" y1="268" x2="820" y2="268" stroke="#e0e0e0" />
  <line x1="60" y1="308" x2="820" y2="308" stroke="#e0e0e0" />
  <text x="60" y="212" font-family="Inter, sans-serif" font-size="13" fill="#7a7a7a">Name • Project • Host • Status • Provisioned by • Last active</text>
  <text x="60" y="252" font-family="Inter, sans-serif" font-size="13" fill="#111">code-reviewer-1 • payments-migration • worker-ec1 • running • automatic • 2m ago</text>
  <text x="60" y="292" font-family="Inter, sans-serif" font-size="13" fill="#111">migration-bot • payments-migration • worker-ec2 • starting • manual • —</text>
  <text x="60" y="332" font-family="Inter, sans-serif" font-size="13" fill="#111">docs-writer • (일반 풀) • worker-ec3 • running • manual • 14m ago</text>
</svg>

#### 인터랙션

| 요소 | 동작 |
| --- | --- |
| 행 클릭 | `/agents/:id` |
| Project 컬럼 값 없음 | "일반 풀"로 표시(§48 host가 project 미소속) — `agents.name`만 노출, 내부 `worker.name`은 어디에도 노출하지 않음(재검토에서 발견한 혼동 방지 규칙, §3.13에서도 동일) |
| FilterBar | Project/Status/Host 드롭다운 + 이름 검색, URL 쿼리스트링에 반영(기존 §7.1 관례) |
| 데이터 갱신 주기 | 10s 폴링 |

#### 빈 상태

- No agents: "No agents running. Create one from a project or `/agents/new`."

---

### 3.12 페이지 #12 — 에이전트 생성 (New Agent)

> 🆕 **설계 제안 (2026-08-14)**: 재검토에서 "마법사 vs 단일 폼"을 단일 폼
> **(진행형 공개, progressive disclosure)**으로 결정 — 기존 `/tasks/new`,
> `/projects/new` 같은 단순 폼 컨벤션과 일관되고, 다단계 마법사보다 구현
> 비용이 낮습니다. 다단계가 필요할 만큼 입력이 분기하지 않는다고 판단했습니다
> (host→project는 자동 파생, template은 선택적 프리필일 뿐).

**라우트**: `/agents/new`(`?project_id=`, `?host_id=` 쿼리로 사전 필터 가능)
**권한**: `AgentCreate`(admin 기본)  **스타일**: Apple auth surface(단일 폼 레이아웃)

#### 폼 구성 (위→아래)

1. **Host** 드롭다운 — `max_agents` 대비 여유 있는 host만(`"worker-ec1 (2/5 agents used)"`).
   `?project_id=`로 진입 시 그 프로젝트 소속 host로 사전 필터.
2. **Project**(읽기 전용 표시) — 선택한 host의 `project_id`를 그대로 보여줌
   (일반 풀이면 "—"). 사용자가 별도로 고르지 않음 — `#48`의 배타적 소유상
   host가 project를 결정하므로 편집 불가 필드로 명시해 혼동을 방지.
3. **Template**(선택, "직접 입력" 옵션 포함) — 선택 시 아래 4·6번 필드를
   템플릿 값으로 프리필(스냅샷 — 이후 템플릿을 고쳐도 이미 만든 Agent는
   영향받지 않음, §6 참고).
4. **Name** — 기본값 자동 제안(예: `<template-name>-N`), 편집 가능, 저장 시
   `agents.name` UNIQUE 검증.
5. **Custom Prompt**(textarea) — 템플릿 미선택 시 빈 값.
6. **도구 바인딩** — `mcp_servers` 카탈로그 체크박스 목록. 필수 도구는 항상
   체크·비활성화, 옵션 도구만 토글 가능. 카탈로그에 없으면 "관리자에게
   `/admin/mcp-servers`에서 등록을 요청하세요" 안내.
7. **생성**(pill CTA) → `POST /api/agents`.

![Agent Creation Prefill Flow](../assets/diagrams/ui-dashboard/agent-creation-flow.mermaid)

#### 인터랙션

| 요소 | 동작 |
| --- | --- |
| Host 변경 | Project 읽기 전용 필드 즉시 갱신, 이미 선택한 Template이 그 host의 프로젝트와 무관하므로 유지(템플릿은 host/project와 독립적인 프리셋) |
| Template 변경 | Custom Prompt/도구 체크박스를 템플릿 값으로 재프리필 — 사용자가 이미 손으로 고친 값이 있으면 덮어쓰기 전에 확인 모달("템플릿 값으로 덮어쓸까요?") |
| 필수 도구 체크박스 | 항상 체크된 채 비활성화(해제 불가) |
| 생성 실패(host 여유 없음) | 인라인 에러 "이 host는 이미 가득 찼습니다 — 다른 host를 선택하세요" |

---

### 3.13 페이지 #13 — 에이전트 상세 (Agent Detail)

> 🆕 **설계 제안 (2026-08-14)**: 재검토에서 확정 — 메모리 브라우저는
> **읽기 전용 + 개별 삭제**만 제공합니다(자동 보존/정리 정책은
> `agent-provisioning-design.md` §12 열린 질문, 구현 전까지는 이 수동 삭제가
> 유일한 정리 수단).

**라우트**: `/agents/:id`  **권한**: `AgentRead`(viewer+), Stop/편집은 `AgentDelete`/`AgentManage`  **스타일**: Apple tile system

#### 레이아웃 (SVG wireframe)

<svg viewBox="0 0 900 640" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="600" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="72" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="16" fill="#111">← Agents / code-reviewer-1 • running • automatic</text>
  <text x="60" y="96" font-family="Inter, sans-serif" font-size="13" fill="#444">host worker-ec1 • project payments-migration • template code-reviewer</text>
  <rect x="40" y="128" width="820" height="120" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="152" font-family="Inter, sans-serif" font-size="14" fill="#444">Custom Prompt / Tools  [Manage]</text>
  <line x1="60" y1="184" x2="820" y2="184" stroke="#e0e0e0" />
  <text x="60" y="208" font-family="Inter, sans-serif" font-size="13" fill="#111">required: linter-mcp, github-mcp  •  optional: slack-mcp</text>
  <rect x="40" y="264" width="820" height="240" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="288" font-family="Inter, sans-serif" font-size="14" fill="#444">Memory  [kind: all ▾]</text>
  <line x1="60" y1="324" x2="820" y2="324" stroke="#e0e0e0" />
  <line x1="60" y1="368" x2="820" y2="368" stroke="#e0e0e0" />
  <line x1="60" y1="412" x2="820" y2="412" stroke="#e0e0e0" />
  <text x="60" y="348" font-family="Inter, sans-serif" font-size="13" fill="#111">note • "reviewed PR #221, flagged 2 issues" • task #8f21 • 2m ago  [🗑]</text>
  <text x="60" y="392" font-family="Inter, sans-serif" font-size="13" fill="#111">fact • "repo uses pnpm, not npm" • task #8f10 • 1h ago  [🗑]</text>
  <text x="60" y="436" font-family="Inter, sans-serif" font-size="13" fill="#111">note • "initial setup complete" • task #8e99 • 3h ago  [🗑]</text>
  <rect x="40" y="520" width="820" height="90" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="544" font-family="Inter, sans-serif" font-size="14" fill="#444">이 Agent로 디스패치된 최근 태스크</text>
</svg>

#### 인터랙션

| 요소 | 동작 |
| --- | --- |
| `Manage` | 모달 — custom_prompt 편집, 옵션 도구 토글(필수 도구는 여기서도 해제 불가) — `AgentManage` |
| Memory kind 필터 | note / summary / fact 드롭다운 |
| Memory 행 `🗑` | `DELETE /api/agents/:id/memory/:entry_id` 확인 후 즉시 목록에서 제거 — `AgentManage` |
| Memory 행 텍스트 클릭 | 잘린 content 전체 펼침(inline expand) |
| `Stop` (헤더) | `agent_commands`(stop) 발행 확인 모달, 상태를 `Stopping`으로 즉시 반영 — `AgentDelete` |
| 데이터 갱신 주기 | 10s 폴링(상태), Memory는 진입 시 1회 로드 + 수동 새로고침 |

#### 빈 상태

- No memory yet: "This agent hasn't completed any tasks yet."

---

### 3.14 페이지 #14 — 에이전트 템플릿 · MCP 카탈로그 관리 (Admin)

> 🆕 **설계 제안 (2026-08-14)**. 다른 `/admin/*` 관리 페이지(사용자 관리,
> SSH 키 금고)와 동일한 관리자 전용 컨벤션을 따릅니다 — 별도 신규 패턴
> 없음.

**라우트**: `/admin/agent-templates`, `/admin/mcp-servers`  **권한**: `AgentTemplateManage`(admin 기본)  **스타일**: Apple auth surface

- **`/admin/agent-templates`**: Template DataTable(name, custom_prompt 미리보기,
  필수/옵션 도구 칩 목록, 사용 중인 Agent 수) + 생성/편집 모달.
- **`/admin/mcp-servers`**: `mcp_servers` 카탈로그 DataTable(name, transport
  badge, 참조 중인 template/agent 수) + 등록 모달. **삭제 시 참조 중이면
  409**(`agent-provisioning-design.md` §3/§10, `ON DELETE RESTRICT` 정책) —
  삭제 버튼 클릭 시 참조 목록을 먼저 보여주고, 참조가 하나도 없을 때만
  실제 삭제 버튼을 활성화.

---

## 4. 사용자 흐름도

### 4.1 온보딩 플로우 (최초 설치 직후)

![Onboarding Flowchart](../assets/diagrams/ui-dashboard/onboarding-flow.mermaid)

### 4.2 일반 로그인 플로우

![Login Flowchart](../assets/diagrams/ui-dashboard/login-flow.mermaid)

### 4.3 일반 운영 플로우 (모니터링 → 디버깅)

![Operational Flowchart](../assets/diagrams/ui-dashboard/operational-flow.mermaid)

### 4.4 관리자 플로우 (신규 사용자 초대 → 권한 검증)

![Admin Flowchart](../assets/diagrams/ui-dashboard/admin-flow.mermaid)

### 4.5 에지 케이스 플로우

#### 세션 만료 (SSE 연결 중)

![Session Timeout Sequence Diagram](../assets/diagrams/ui-dashboard/session-timeout-sequence.mermaid)

#### 권한 부족 (403 Forbidden)

![Permission Denied Sequence Diagram](../assets/diagrams/ui-dashboard/permission-denied-sequence.mermaid)

#### CF Access 연동 시나리오 (이중 인증)

![CF Access Flowchart](../assets/diagrams/ui-dashboard/cf-access-flow.mermaid)

---

## 5. 내비게이션 패턴

### 5.1 글로벌 내비게이션

Apple tile 기반 페이지 상단에 고정 헤더:

<svg viewBox="0 0 900 220" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="180" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="140" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <rect x="60" y="60" width="120" height="28" rx="9999px" fill="#0066cc" />
  <text x="90" y="78" text-anchor="middle" font-family="Inter, sans-serif" font-size="13" fill="#ffffff">Fleet Orchestrator</text>
  <rect x="220" y="60" width="70" height="24" rx="9999px" fill="#fafafc" stroke="#e0e0e0" />
  <rect x="306" y="60" width="76" height="24" rx="9999px" fill="#fafafc" stroke="#e0e0e0" />
  <rect x="394" y="60" width="64" height="24" rx="9999px" fill="#fafafc" stroke="#e0e0e0" />
  <rect x="620" y="60" width="116" height="24" rx="9999px" fill="#fafafc" stroke="#e0e0e0" />
  <rect x="752" y="60" width="78" height="24" rx="9999px" fill="#fafafc" stroke="#e0e0e0" />
  <circle cx="790" cy="126" r="24" fill="#0066cc" />
  <text x="790" y="132" text-anchor="middle" font-family="Inter, sans-serif" font-size="14" fill="#ffffff">YA</text>
  <text x="640" y="132" font-family="Inter, sans-serif" font-size="13" fill="#111">Sign out</text>
</svg>

- **로고 클릭**: 항상 `/`로 복귀
- **주요 링크**: Overview, Workers, Tasks (viewer+)
- **Admin 메뉴**: 드롭다운 (Users, Audit, Tools) — admin/operator만 표시 (⚠️ 역할 식별자 정정: `administrator`→`admin`)
- **Avatar/Sign out**: 우측 고정

### 5.2 브레드크럼

상세 페이지에서만 표시:

<svg viewBox="0 0 900 180" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg">
  <rect x="20" y="20" width="860" height="140" rx="12" fill="#f6f6f6" stroke="#b8b8b8" />
  <rect x="40" y="40" width="820" height="100" rx="8" fill="#ffffff" stroke="#c9c9c9" />
  <text x="60" y="72" font-family="Inter, sans-serif" font-size="15" fill="#111">← Workers / arm2-prod</text>
  <circle cx="260" cy="68" r="6" fill="#2ea44f" />
  <text x="280" y="72" font-family="Inter, sans-serif" font-size="13" fill="#444">online</text>
  <text x="60" y="104" font-family="Inter, sans-serif" font-size="15" fill="#111">← Tasks / task_b7e2</text>
  <circle cx="260" cy="100" r="6" fill="#0066cc" />
  <text x="280" y="104" font-family="Inter, sans-serif" font-size="13" fill="#444">completed</text>
  <text x="60" y="136" font-family="Inter, sans-serif" font-size="15" fill="#111">← Admin / Users / jikang</text>
  <text x="280" y="136" font-family="Inter, sans-serif" font-size="13" fill="#444">operator</text>
</svg>

뒤로 가기 버튼(←)은 항상 부모 목록으로 복귀.

### 5.3 인증 페이지 내비게이션

Login / Bootstrap 페이지는 **글로벌 헤더 없음**. 카드 자체가 전체 UI.
이탈 시 `/` (비인가 시 `/login`으로 리다이렉트).

---

## 6. 공통 컴포넌트

### 6.1 StatusPill

상태 표시용 작은 둥근 배지. Apple 스타일은 색상보다는 명확한 레이블과 아이콘을 우선한다.

> ✅ **정정 (2026-08-13)**: 실제 `WorkerStatus` enum(`crates/fleet-core/src/worker.rs`)은
> `Online | Degraded | Offline | CircuitOpen` 4가지뿐이다. 아래 `pending`/`active`/`inactive`는
> 존재하지 않는 상태이며, CircuitBreaker에 의한 자동 차단 상태인 `circuit_open`이 누락되어 있었다.

```text
[● online]        ← green dot + text
[● degraded]      ← amber
[● offline]       ← red
[● circuit_open]  ← purple/violet, CircuitBreaker에 의해 자동 차단됨
```

**Props**: `status: online|degraded|offline|circuit_open`, `label?: string`

### 6.2 Badge (역할/카테고리)

| 타입 | 스타일 | 용도 |
| --- | --- | --- |
| Role-admin | Action Blue pill, white text | admin (⚠️ 정정: `administrator` 아님) |
| Role-other | parchment surface, 1px hairline | operator/viewer |
| Category | tint chip on parchment | Audit categories |

### 6.3 Card

| 변형 | 배경 | 테두리 | 라디우스 | 그림자 |
| --- | --- | --- | --- | --- |
| Tile light | `#ffffff` | 없음 | `0px` | 없음 |
| Tile parchment | `#f5f5f7` | 없음 | `0px` | 없음 |
| Tile dark | `#272729` | 없음 | `0px` | 없음 |
| Utility card | `#ffffff` | `#e0e0e0` | `18px` | 없음 |
| Modal | `#ffffff` | `#e0e0e0` | `18px` | `rgba(0,0,0,0.08)` |

### 6.4 DataTable

공용 테이블 컴포넌트. Apple의 light-first 섹션에서 가장 흔하게 쓰인다.

| 속성 | 값 |
| --- | --- |
| 헤더 배경 | `#fafafc` |
| 행 높이 | 56px |
| 호버 | `#f5f5f7` |
| 선택 | left border 3px `#0066cc` |
| 빈 상태 | 중앙 + muted text |
| 페이지네이션 | 하단 (10/25/50/100) |

### 6.5 EventLog

터미널 스타일 로그 패널. 다크 tile 위에 놓일 때는 white text, light tile 위에 놓일 때는 ink를 사용한다.

- 배경: `#0d0f13` 또는 `#f5f5f7` (컨텍스트에 따라)
- 폰트: SF Mono / system monospace
- 타임스탬프: muted gray
- 상태 코드: green/red/amber
- 최대 1000줄 버퍼, 자동 스크롤(사용자가 스크롤 시 일시정지)

### 6.6 EmptyState

| 변형 | 아이콘 | 제목 | 부제목 |
| --- | --- | --- | --- |
| no-tasks | 📭 | No tasks yet | Dispatch one via MCP tools |
| no-workers | 🛰️ | No workers | Start a worker with `fleet join` |
| no-events | ⏳ | Awaiting events | Listen with `fleet events tail` |
| no-results | 🔍 | No matches | Try adjusting your filters |

### 6.7 Avatar

원형 아바타. Apple에서는 시각적 과잉 장식보다 초기값과 색상 일관성이 더 중요하다.

```text
        ┌──┐
        │YA│  ← Action Blue fill, white text
        └──┘
```

색상 할당 규칙 (해시 기반):

| 이니셜 | 색상 | Hex |
| --- | --- | --- |
| A-E | mustard | `#d9730d` |
| F-J | teal | `#0f7b6c` |
| K-O | coral | `#e8855e` |
| P-T | Action Blue | `#0066cc` |
| U-Z | purple | `#9085d8` |

### 6.8 기타

| 컴포넌트 | 설명 |
| --- | --- |
| MetricCard | 큰 숫자 + 라벨, Action Blue/neutral 상태 표시 |
| CodeBlock | SF Mono, 복사 버튼, light/dark 컨텍스트 대응 |
| TimelineStepper | created → dispatched → done 단계 표시 |
| FilterBar | dropdown + pill chip + search |
| Toast | 우측 하단, 자동 소멸(4s) |
| Modal | 중앙, white/parchment canvas, thin border |
| OTPInput | 6개 분할 입력 박스, 자동 포커스 이동 |
| StrengthGauge | 4단계 게이지, Action Blue 강조 |

---

## 7. 상태 관리

### 7.1 클라이언트 상태

| 상태            | 저장소              | 갱신 주기     |
| --------------- | ------------------- | ------------- |
| 세션 쿠키       | HttpOnly cookie     | 서버 관리     |
| 현재 사용자 정보| 메모리 (페이지 진입 시) | -             |
| Overview 데이터 | 메모리              | 5s 폴링       |
| Workers 데이터  | 메모리              | 5s 폴링       |
| Tasks 데이터    | 메모리              | 10s 폴링      |
| Events 스트림   | SSE EventSource     | 실시간        |
| URL 필터        | 쿼리스트링          | 사용자 조작   |

### 7.2 로딩 / 에러 상태

```
[로딩] skeleton placeholder (회색 박스, pulse 애니메이션)
[에러] 인라인 에러 메시지 + 재시도 버튼
[401] 전역 인터셉터 → /login 리다이렉트
[403] 인라인 "권한 부족" 메시지
[5xx] 토스트 + 자동 재시도(3회, exp backoff)
[네트워크 끊김] 배너 "연결 끊김 — 재연결 중..." (상단 고정)
```

---

## 8. 반응형 전략

| 브레이크포인트 | 너비       | 레이아웃 변경                                  |
| -------------- | ---------- | ---------------------------------------------- |
| `xs`           | < 640px    | 단일 컬럼, 모든 카드 full width                |
| `sm`           | 640-1024   | 2 컬럼 카드, 테이블 스크롤                     |
| `md`           | 1024-1280  | 3 컬럼 카드, 사이드 패널 축소                  |
| `lg`           | 1280-1440  | 설계 기준 (1440px)                             |
| `xl`           | > 1440     | 좌우 padding 증가, max-width 1600px            |

**모바일 최적화 원칙**: 모바일은 조회 전용으로 설계. 관리 액션(사용자
초대, 권한 변경 등)은 데스크톱 권장. 모바일에서 관리 액션 시도 시
안내 토스트.

---

## 9. 접근성 (WCAG 2.1 AA)

### 9.1 필수 준수 항목

| 항목             | 기준                                            |
| ---------------- | ----------------------------------------------- |
| 색 대비          | 본문 4.5:1 이상, 큰 텍스트 3:1 이상             |
| 키보드 내비게이션| 모든 인터랙티브 요소 Tab 접근 가능              |
| 포커스 표시      | `--shadow-glow` 3px ring, 명확히 보임           |
| ARIA 라벨        | 아이콘 전용 버튼, 동적 콘텐츠                   |
| 스크린 리더      | DataTable: `<th scope>`, `<caption>`             |
| 색 의존성        | 상태는 색+아이콘+텍스트 3중 표현                |
| 모션 민감도       | `prefers-reduced-motion` 존중                    |

### 9.2 Apple surface 대비 검증

| 색상 조합                 | 비율   | 합격 여부 |
| ------------------------- | ------ | --------- |
| #ffffff on #0f1115        | 18.1:1 | ✅        |
| #9ca3af on #0f1115        | 7.5:1  | ✅        |
| #6b7280 on #0f1115        | 4.3:1  | ⚠️ (AA 본문 한계) |
| #10b981 on #0f1115        | 7.6:1  | ✅        |
| #0075de on #ffffff        | 4.5:1  | ✅        |
| #6b6b6b on #f6f5f4        | 4.7:1  | ✅        |

> `--text-muted` (#6b7280)는 4.3:1로 본문용으로는 AA 한계치에 근접.
> 14px 미만 텍스트에는 `--text-secondary` (#9ca3af) 사용 권장.

---

## 10. 구현 우선순위

### 10.1 P0 — MVP (Phase 9.1.3)

| 페이지                | 이유                                       |
| --------------------- | ------------------------------------------ |
| #1 메인 대시보드      | 현재 동작하는 핵심, 사용자 가치 최대       |
| #4 로그인             | RBAC 도입의 필수 진입점                    |
| #5 부트스트랩         | 최초 관리자 생성의 유일한 경로             |

**예상 LOC**: ~1,200 (Apple Design System CSS + auth 프론트엔드)

### 10.2 P1 — 운영 강화 (Phase 9.2)

| 페이지                | 이유                                       |
| --------------------- | ------------------------------------------ |
| #2 워커 상세          | 디버깅/진단 워크플로우                     |
| #3 태스크 큐          | 운영 가시성, 실패 추적                     |
| #6 사용자 관리        | RBAC 운영 필수                             |

**예상 LOC**: ~800

### 10.3 P1.5 — 호스트 인벤토리 (Host Inventory)

> ✅ **정정 (2026-08-13)**: 아래 "선행 작업" 3가지는 **전부 완료되어 배포된
> 상태**입니다. 이 절은 더 이상 계획이 아니라 완료 기록으로 읽어야 합니다.

| 페이지                    | 이유                                                                 |
| ------------------------- | -------------------------------------------------------------------- |
| #2.5 호스트 인벤토리      | grok 설치 여부·버전 일관성, 미등록 호스트 발견                       |
| #2.6 호스트 상세          | 호스트 단위 히스토리(프로비저닝/하트비트/장애) 타임라인, 일원화 진단 |

**배경**: 기존 `workers` 테이블은 "등록된 워커"만 추적한다. 프로비저닝 직후·하트비트 끊김·grok 미설치 등 **호스트 단위 가시성**이 부족하여, `hosts` + `host_events` 스키마를 도입했다(실제 컬럼 구성은 §3.2.5의 정정 참고 — 제안 초안과 다름).

**완료된 작업** (⚠️ 원래 "선행 작업"으로 서술, 전부 완료 확인됨):

1. ✅ 마이그레이션 `007_hosts.sql` (hosts, host_events 테이블 — §3.2.5 참조)
2. ✅ fleet-worker 하트비트 확장: `grok_version` / `fleet_worker_version` / `os_info` 필드 전송 (`crates/fleet-worker/src/registration.rs`)
3. ✅ fleet-provisioner 이벤트 훅: 프로비저닝 성공/실패 시 `host_events` INSERT (`crates/fleet-dashboard/src/provisioning.rs`)

**예상 LOC**: ~1,000 (스키마 + heartbeat 확장 + 페이지 2종 + API) — 참고용, 실측 안 함.

### 10.4 P2 — 확장 (Phase 9.3+)

| 페이지                | 이유                                       |
| --------------------- | ------------------------------------------ |
| #7 감사 로그          | 보안 컴플라이언스, 침해 대응               |
| #8 MCP 도구 탐색기    | 자가발견성, AI 클라이언트 온보딩           |

**예상 LOC**: ~600

### 10.5 P2 — 프로젝트/에이전트 (`#48`/`#49`, 2026-08-14 설계 신설)

> 🆕 각 페이지 설계는 §3.9~§3.14 참고. `#48` 백엔드(Phase 1~4)와 `#49`
> 백엔드(Phase 0~5)가 최소 Phase 1~3까지 진행된 뒤에야 이 UI 작업을 시작할
> 수 있습니다 — 데이터 소스 자체가 아직 없는 상태에서 화면부터 만들 수는
> 없습니다.

| 페이지 | 이유 |
| --- | --- |
| #9 프로젝트 목록, #10 프로젝트 상세 | `#48` 배타적 소유/하드 디스패치 모델의 유일한 조작 표면(현재는 REST/MCP만 존재) |
| #11 에이전트 목록, #12 에이전트 생성, #13 에이전트 상세 | `#49` 동적 프로비저닝·메모리의 유일한 조작 표면 |
| #14 템플릿/카탈로그 관리 | `#49` §6 중앙 카탈로그를 관리자가 직접 유지보수하는 유일한 경로(CLI 대체 가능하나 발견성이 낮음) |

**선행 조건**: `#48` Phase 3(API+MCP), `#49` Phase 1~2(스키마+정적 등록,
템플릿/카탈로그) 완료. `#49` Phase 4(동적 프로비저닝)는 완료되지 않아도
§3.12 생성 폼 자체는 Phase 1(정적 등록) 단계에서부터 동작 가능(§4 dynamic
provisioning은 백그라운드에서 나중에 붙는 계층).

**예상 LOC**: 참고용, 실측 안 함 — Host Inventory(§10.3, 페이지 2종 기준
~1,000 LOC 중 스키마/heartbeat 제외분)보다 페이지 수가 많아(6종) 그
비례치로 어림하면 ~1,500~2,000 예상.

---

## 11. 파일 구조

⚠️ **정정 (2026-08-13)**: 아래는 2026-07-20 시점의 제안이었고, `styles/`·`scripts/`
하위 디렉토리 분리나 `auth.rs`/`bootstrap.rs`/`templates.rs` 분리는 실제로
채택되지 않았습니다. 실제 `crates/fleet-dashboard/assets/`는 **평평한 구조**로,
`.html`마다 동일 이름의 `.js`가 짝을 이루고 공용 스타일시트 하나(`styles.css`)와
로그인 전용 `login.css`만 별도로 있습니다. `worker.html`이 아니라
**`worker-detail.html`**입니다. `src/`도 `app.rs`/`handlers.rs`/`auth.rs`/
`provisioning.rs`/`sse.rs`/`error.rs`/`schema.rs`/`assets.rs` 등으로 구성되며
`bootstrap.rs`/`templates.rs`라는 별도 모듈은 없습니다(부트스트랩·로그인 라우트도
`handlers.rs`/`auth.rs`에 있음).

```
crates/fleet-dashboard/
├── assets/                     # 실제 구조 — 평평함, .html:.js 1:1 페어
│   ├── index.html / (app.js가 공용)
│   ├── login.html / login.css
│   ├── bootstrap.html
│   ├── verify-email.html
│   ├── forgot-password.html / reset-password.html / resend-verification.html
│   ├── worker-detail.html / worker-detail.js
│   ├── tasks.html / tasks.js
│   ├── task-new.html / task-new.js
│   ├── task-detail.html / task-detail.js
│   ├── admin-users.html / admin-users.js
│   ├── admin-activity.html / admin-activity.js
│   ├── admin-tools.html / admin-tools.js
│   ├── admin-ssh-keys.html / admin-ssh-keys.js
│   ├── hosts.html / hosts.js
│   ├── host-detail.html / host-detail.js
│   ├── provision.html / provision.js
│   └── styles.css              # 공용 스타일시트 (토큰/서피스/컴포넌트 통합)
├── src/
│   ├── app.rs                  # 라우터 구성
│   ├── handlers.rs             # 페이지/일반 API 핸들러
│   ├── auth.rs                 # 세션/RBAC/로그인
│   ├── provisioning.rs         # 호스트 프로비저닝 + SSH 키 금고
│   ├── sse.rs                  # /api/events/stream
│   ├── schema.rs               # 응답 타입
│   ├── error.rs                # ApiError
│   └── assets.rs               # rust-embed 정적 자산
└── tests/
```

---

## 12. 평가

### 12.1 강점

- **단일 Apple Design System**: 운영/인증/관리 화면이 모두 같은 토큰과 레이아웃 규칙을 공유한다.
- **Apple 디자인 철학 채택**: 인증/온보딩은 가벼운 canvas와 명확한 CTA로 인지 부담을 최소화한다.
- **8개 페이지 적정 범위**: 단일 목적의 작은 페이지들로 분할. 거대한
  SPA 대신 멀티 페이지 접근으로 복잡도 제어.
- **흐름 기반 설계**: 각 페이지가 아닌 사용자 흐름(온보딩/운영/관리)을
  기준으로 설계. 사용자 관점 부합.
- **핵심 인터랙션 정의**: 각 요소의 동작을 표로 명시 → 프론트엔드
  구현 시 애매함 최소.

### 12.2 약점 / 리스크

| 약점                                    | 완화 방안                                  |
| --------------------------------------- | ------------------------------------------ |
| 8개 페이지 분산 → 초기 구현 부담        | P0 3개로 MVP 축소 가능                     |
| 단일 시스템 유지 → CSS 중복/일관성 리스크   | tokens.css 기반으로 하나의 토큰 소스를 유지하고, surface·auth 변형만 오버라이드 |
| 프론트엔드 프레임워크 없이 Vanilla JS   | htmx + Alpine.js 도입 검토 (선택)          |
| 모바일 관리 액션 제한                   | 별도 모바일 최소 기능 명시 필요            |
| Mermaid 다이어그램 → 실제 코드와 drift  | 문서 자동 생성 또는 주기적 검토 프로세스   |

### 12.3 대안 검토

| 대안                | 장점                          | 단점                          | 결정 |
| ------------------- | ----------------------------- | ----------------------------- | ---- |
| 단일 Apple 시스템      | 일관성, 구현 단순             | 특정 페이지에서 과도하게 무난해질 수 있음      | 채택 |
| React/Next.js SPA   | 컴포넌트 재사용, 상태 관리 우수 | 빌드 파이프라인 복잡, Rust 서렁 부담 | 기각 |
| htmx + Alpine.js    | 서버 사이드 렌더링, 가벼움     | 복잡한 UI에서 한계            | **검토 중** |
| Vanilla JS + SSR    | 구현 자유도, 의존성 최소       | 컴포넌트 재사용 부족          | **기본 채택** |

---

## 13. 참고 자료

- [DESIGN-apple.md](../../DESIGN-apple.md) — Apple Design System 정본
- [architecture.md](../architecture/overview.md) — 시스템 아키텍처
- [api-reference.md](../architecture/api-reference.md) — HTTP API 명세
- [deployment.md](../deployment/deployment.md) — 배포 가이드 (Nginx 설정 포함, 2026-08-11 갱신 — 원문의 "Caddy" 기술은 폐기됨)
- RBAC 구현 계획 — plan 문서 (Phase 9.1.1 ~ 9.1.6)

---

**문서 버전**: v1.0 (2026-07-20)
**다음 리뷰**: P0 구현 완료 후 (#1, #4, #5 페이지 검증)
