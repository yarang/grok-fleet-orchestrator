# UI/UX 화면 설계서

이 문서는 Grok Fleet Orchestrator 웹 대시보드의 화면 설계, 사용자 흐름,
내비게이션 패턴, 공통 컴포넌트를 정의합니다. 시스템 아키텍처는
[`architecture.md`](../architecture/overview.md), RBAC 구현 계획은 해당 plan 문서를
참조하세요.

## TL;DR

- **8개 페이지** 제안: 운영 코어 3 + 인증 2 + 관리 2 + 고급 1
- **단일 디자인 시스템**: Apple Design System(white/parchment/dark tiles, Action Blue, SF Pro, pill CTA)
- **3개 핵심 흐름**: 온보딩(Bootstrap → Login → Overview), 일반 운영
  (Login → Overview → Worker → Task), 관리자(User Mgmt → Audit Log)
- **공통 컴포넌트 11종**: StatusPill, Badge, Card, DataTable, EventLog,
  EmptyState, Avatar 등
- **구현 우선순위 3단계**: P0(MVP) → P1(운영 강화) → P2(확장)

---

## 1. 정보 아키텍처

```
fleet.agentthread.dev/
│
├── /                          # 메인 대시보드 (Overview)        [P0]
├── /login                     # 로그인                          [P0]
├── /bootstrap                 # 최초 관리자 설정                [P0]
│
├── /hosts                     # 호스트 인벤토리                 [P1.5]
├── /hosts/:hostname           # 호스트 상세 (히스토리)          [P1.5]
│
├── /workers                   # 워커 목록 (Overview에 통합)
├── /workers/:id               # 워커 상세                       [P1]
│
├── /tasks                     # 태스크 큐                       [P1]
├── /tasks/:id                 # 태스크 상세 (큐에 통합)
│
├── /admin/users               # 사용자 관리                     [P1]
├── /admin/activity            # 활동 로그 (작업·워커 이벤트)     [P2]
│
└── /admin/tools               # MCP 도구 탐색기                 [P2]
```

### 라우트 가드 매트릭스

| 라우트           | 인증 | 최소 권한    | 비고                          |
| ---------------- | ---- | ------------ | ----------------------------- |
| `/login`         | ✗    | -            | 이미 로그인 시 `/`로 리다이렉트 |
| `/bootstrap`     | ✗    | -            | OTP 토큰 필요, 1회성          |
| `/`              | ✓    | viewer       | 기본 랜딩                    |
| `/hosts`         | ✓    | viewer       | 읽기 전용                     |
| `/hosts/:hostname` | ✓  | viewer       | 읽기 전용                     |
| `/workers/:id`   | ✓    | viewer       | 읽기 전용                     |
| `/tasks`         | ✓    | viewer       | 읽기 전용                     |
| `/admin/users`   | ✓    | administrator| 관리자 전용                   |
| `/admin/activity`| ✓    | viewer       | events:list — 전 역할 열람    |
| `/admin/tools`   | ✓    | operator     | 도구 호출은 operator 이상     |

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
| "Force reconnect" 버튼 | operator+ 권한 필요, 확인 다이얼로그         |
| Recent Events 행 클릭 | Audit Log의 해당 이벤트로 딥링크              |

---

### 3.2.5 페이지 #2.5 — 호스트 인벤토리 (Host Inventory)

**라우트**: `/hosts`  **권한**: viewer+  **스타일**: Apple tile system

**목적**: 물리/가상 호스트 전체의 가시성 확보. 워커 등록 상태,
grok CLI 설치 여부/버전, 프로비저닝 이력을 한눈에.

> **핵심 차이**: `workers` 테이블은 "현재 등록된 워커"만 추적한다.
> 이 페이지는 `hosts` 테이블(신규)을 기반으로, 등록 여부와 무관하게
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
  <text x="460" y="144" font-family="Inter, sans-serif" font-size="14" fill="#444">Ready 1</text>
  <text x="660" y="144" font-family="Inter, sans-serif" font-size="14" fill="#444">Failed 0</text>
  <text x="60" y="216" font-family="Inter, sans-serif" font-size="14" fill="#444">Host table</text>
  <text x="60" y="256" font-family="Inter, sans-serif" font-size="13" fill="#111">10.0.1.10 • 0.2.112 • v0.1.0 • online • ap-ne-2 • [12 ev]</text>
  <text x="60" y="296" font-family="Inter, sans-serif" font-size="13" fill="#111">10.0.1.11 • 0.2.112 • v0.1.0 • online • ap-ne-2 • [8 ev]</text>
  <text x="60" y="336" font-family="Inter, sans-serif" font-size="13" fill="#111">10.0.2.20 • — • — • ready • us-west • [3 ev]</text>
</svg>

#### 인터랙션

| 요소              | 동작                                                |
| ----------------- | --------------------------------------------------- |
| Host 행 클릭      | `/hosts/:hostname` 상세 페이지 이동                 |
| History [N ev] 클릭 | `/hosts/:hostname#events` 이벤트 섹션으로 스크롤    |
| ↻ Refresh 버튼    | 즉시 폴링 트리거                                     |
| Status pill       | online(green) / ready(amber) / failed(red) / unknown(gray) |
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
| grok 버전         | 하트비트에 `grok_version` 필드 추가 | 워커 하트비트 (15s)        |
| fleet-worker 버전 | 하트비트에 `worker_version` 필드   | 워커 하트비트 (15s)        |
| OS 정보           | 하트비트에 `os_info` 필드          | 워커 등록 시 1회           |
| 프로비저닝 이력   | `fleet provision` 실행 시 `host_events` INSERT | 프로비저닝 실행 |
| grok 설치 이력    | 프로비저닝 스크립트 실행 시 이벤트 기록 | 프로비저닝 시 1회    |

> **하트비트 확장**: 현재 워커 하트비트는 load_avg, mem, disk만 전송.
> `grok_version`, `fleet_worker_version`, `os_info` 필드를 추가하여
> 별도 SSH 프로브 없이도 소프트웨어 버전을 추적.

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
| Sign in 버튼        | POST `/api/auth/login`, 성공 시 `/`로 리다이렉트        |
| 실패 응답           | 입력 아래 적색 텍스트, 흔들림 애니메이션                |
| 5회 실패            | 15분 쿨다운, "Try again later" 메시지                   |
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
| 비밀번호 강도         | zxcvbn 기반 4단계 게이지, 3단계 이상 필요             |
| Activate 버튼         | POST `/api/bootstrap/activate`, 성공 시 `/`로         |
| 이미 활성화된 경우    | `/login`으로 자동 리다이렉트 + 안내 토스트            |
| OTP 만료/오용         | "Token invalid or expired. Issue a new one."          |

---

### 3.6 페이지 #6 — 사용자 관리 (User Management)

**라우트**: `/admin/users`  **권한**: administrator  **스타일**: Apple auth surface

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
  <text x="60" y="258" font-family="Inter, sans-serif" font-size="13" fill="#111">YA • Yarang • administrator • active • 2m ago</text>
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

**라우트**: `/admin/tools`  **권한**: operator+  **스타일**: Apple auth surface

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
  <text x="60" y="144" font-family="Inter, sans-serif" font-size="14" fill="#444">7 tools exposed via JSON-RPC 2.0 stdio</text>
  <text x="60" y="222" font-family="Inter, sans-serif" font-size="13" fill="#111">workers.list</text>
  <text x="340" y="222" font-family="Inter, sans-serif" font-size="13" fill="#111">workers.inspect</text>
  <text x="620" y="222" font-family="Inter, sans-serif" font-size="13" fill="#111">tasks.dispatch</text>
  <text x="60" y="364" font-family="Inter, sans-serif" font-size="14" fill="#444">Detail panel: fleet.tasks.dispatch</text>
  <text x="60" y="400" font-family="Inter, sans-serif" font-size="13" fill="#111">Input schema • usage example • metrics</text>
</svg>

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
- **Admin 메뉴**: 드롭다운 (Users, Audit, Tools) — administrator/operator만 표시
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

```text
[● online]      ← green dot + text
[● degraded]    ← amber
[● offline]     ← red
[● pending]     ← amber, blinking
```

**Props**: `status: online|degraded|offline|pending|active|inactive`, `label?: string`

### 6.2 Badge (역할/카테고리)

| 타입 | 스타일 | 용도 |
| --- | --- | --- |
| Role-admin | Action Blue pill, white text | administrator |
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

| 페이지                    | 이유                                                                 |
| ------------------------- | -------------------------------------------------------------------- |
| #2.5 호스트 인벤토리      | grok 설치 여부·버전 일관성, 미등록 호스트 발견                       |
| #2.6 호스트 상세          | 호스트 단위 히스토리(프로비저닝/하트비트/장애) 타임라인, 일원화 진단 |

**배경**: 기존 `workers` 테이블은 "등록된 워커"만 추적한다. 프로비저닝 직후·하트비트 끊김·grok 미설치 등 **호스트 단위 가시성**이 부족하여, P1.5에서 `hosts` + `host_events` 스키마를 신규 도입한다.

**선행 작업**:

1. 마이그레이션 `007_hosts.sql` (hosts, host_events 테이블 — §3.2.5 참조)
2. fleet-worker 하트비트 확장: `grok_version` / `fleet_worker_version` / `os_info` 필드 전송
3. fleet-provisioner 이벤트 훅: 프로비저닝 성공/실패 시 `host_events` INSERT

**예상 LOC**: ~1,000 (스키마 + heartbeat 확장 + 페이지 2종 + API)

### 10.4 P2 — 확장 (Phase 9.3+)

| 페이지                | 이유                                       |
| --------------------- | ------------------------------------------ |
| #7 감사 로그          | 보안 컴플라이언스, 침해 대응               |
| #8 MCP 도구 탐색기    | 자가발견성, AI 클라이언트 온보딩           |

**예상 LOC**: ~600

---

## 11. 파일 구조 제안

```
crates/fleet-dashboard/
├── assets/
│   ├── index.html              # P0: Overview (#1)
│   ├── login.html              # P0: Login (#4)
│   ├── bootstrap.html          # P0: Bootstrap (#5)
│   ├── worker.html             # P1: Worker Detail (#2)
│   ├── tasks.html              # P1: Task Queue (#3)
│   ├── admin-users.html        # P1: User Mgmt (#6)
│   ├── hosts.html              # P1.5: Host Inventory (#2.5)
│   ├── host-detail.html        # P1.5: Host Detail (#2.6)
│   ├── admin-activity.html     # P2: Activity Log (#7)
│   ├── admin-tools.html        # P2: MCP Tools (#8)
│   ├── styles/
│   │   ├── tokens.css          # 디자인 토큰 (색상, 타이포, 라디우스)
│   │   ├── surfaces.css        # tile / surface variants
│   │   ├── auth.css            # auth-specific surface styles
│   │   └── components.css      # 공통 컴포넌트
│   └── scripts/
│       ├── app.js              # 공통 (세션 관리, fetch 래퍼)
│       ├── overview.js         # #1
│       ├── login.js            # #4
│       ├── bootstrap.js        # #5
│       └── ...
├── src/
│   ├── app.rs                  # 라우터 구성 (로그인/부트스트랩 추가)
│   ├── handlers.rs             # 기존 핸들러
│   ├── auth.rs                 # 신규: /api/auth/*
│   ├── bootstrap.rs            # 신규: /api/bootstrap/*
│   └── templates.rs            # 신규: HTML 템플릿 렌더링
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
