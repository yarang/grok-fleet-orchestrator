---
type: wiki
status: canonical
source: "docs/architecture/project-feature-design.md"
last_verified: "2026-08-15"
---

# 프로젝트(Project) 기능 설계

> 작성일: 2026-08-14. 로드맵 [`#48`](../roadmap/roadmap.md)에 대응하는 설계
> 문서입니다. **설계 확정** 단계이며 아직 구현되지 않았습니다 — 구현 진행
> 상황은 `roadmap.md` #48 항목을 정본으로 확인하세요. 개정 이력(왜 이렇게
> 결정했는지)은 [`log.md`](log.md)의 "project-feature-design.md" 절을
> 참고하세요 — 이 문서 본문은 현재 확정된 설계만 담습니다.

## 1. 배경 및 동기

`crates/fleet-core/src/ids.rs`에는 이미 `ProjectId` newtype이, `tasks` 테이블에는
`project_id UUID` nullable 컬럼(`013_task_threads.sql`)이 예약돼 있습니다 — 둘 다
"나중에 project 기능이 도입되면 재귀 backfill 없이 바로 채워질 수 있도록" 미리
만들어 둔 자리표시자였고, 실제 `Project` 엔티티나 host/agent(워커) 소속 개념은
지금까지 전혀 구현되지 않았습니다.

이 설계는 그 자리표시자를 실제 기능으로 채우는 것을 목표로 합니다:

1. **프로젝트가 여러 host와 agent(워커)를 담을 수 있어야 한다.**
2. **하나의 프로젝트에 여러 agent가 배치될 수 있어야 한다** — 프로젝트 스코프
   태스크가 여러 워커에 분산 디스패치될 수 있어야 함(단일 워커 전용 아님).
3. 위 둘을 지원하는 **배치/해제 절차와 디스패치 프로토콜**을 정의하되,
   **프로젝트 간 리소스 경쟁/충돌을 구조적으로 예방**한다.

## 2. 핵심 설계 결정

| 결정 사항 | 채택안 | 근거 |
|---|---|---|
| 프로젝트 ↔ host/worker 소속 관계 | **배타적(exclusive) 1:N** — host/worker는 최대 1개 프로젝트에만 소속, 미소속이면 일반 풀 | `workers.project_id`/`hosts.project_id` 직접 FK. host의 물리 자원과 그 위에서 도는 워커 프로세스의 세션 슬롯을 여러 프로젝트가 나눠 쓰면 항상 경쟁 상태가 생긴다 — 스키마 레벨에서 원천 차단하는 게 더 단순하고 확실함 |
| project_id 지정 태스크의 디스패치 범위 | **하드(strict)** — 그 프로젝트 소유 워커만 후보. 후보가 없으면 전체 풀로 폴백하지 않음 | §5 참고 — 새 에러/재시도 메커니즘을 만들지 않고 **`#38`의 기존 `WorkerUnavailable` 재시도/Dead-Letter 경로를 그대로 재사용** |

전체 생성→배정→디스패치→해제 흐름은 아래 시퀀스 다이어그램 참고(팀
검토에서 이 다이어그램이 어느 절에서도 참조되지 않는 고아 파일이었음을
발견 — note, 이번에 참조를 추가):

![Project Assignment Lifecycle](../assets/diagrams/architecture/project-assignment-lifecycle.mermaid)

## 3. 데이터 모델

![Project Data Model](../assets/diagrams/architecture/project-data-model.mermaid)

### 신규 테이블 (`015_projects.sql`)

```sql
CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    created_by TEXT,
    -- #49 통합: 이 프로젝트의 에이전트를 수동으로만 만들지, 오케스트레이터가
    -- 자동으로도 만들지(agent-provisioning-design.md §3 AgentProvisioningMode).
    agent_provisioning_mode TEXT NOT NULL DEFAULT 'manual',  -- 'manual' | 'automatic'
    -- automatic 모드에서 쓸 기본 템플릿. FK 없음 — #49의 agent_templates 테이블이
    -- 아직 존재하지 않으므로(#48이 #49보다 먼저/독립적으로 구현될 수 있어야 함),
    -- 013_task_threads.sql의 tasks.project_id 예약과 동일한 패턴으로 원시 UUID만
    -- 둔다. #49 Phase 1이 agent_templates 테이블을 만들 때
    -- `ALTER TABLE projects ADD CONSTRAINT ... REFERENCES agent_templates(id)`로
    -- FK 제약을 추가한다.
    default_agent_template_id UUID,
    agent_idle_timeout_secs INTEGER,  -- automatic 모드로 만든 에이전트의 유휴 자동 종료 기준(NULL이면 자동 종료 안 함)
    workdir_template TEXT,  -- 결과물 디렉토리 기본 경로 템플릿(agent-provisioning-design.md §9), nullable
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- updated_at 자동 갱신 트리거는 007_hosts.sql의 update_hosts_updated_at() 패턴을 재사용.
-- `#51`(agent-harness-composition-design.md)이 이후 마이그레이션에서
-- constitution_prompt TEXT 컬럼을 추가한다 — 이 프로젝트의 모든 에이전트에
-- 항상 선행 주입되는 프로젝트 전역 지침("이 프로젝트의 CLAUDE.md").

-- 013_task_threads.sql이 예약해 둔 컬럼에 이제야 FK를 건다. 기존 행은
-- project_id IS NULL인 채로 계속 유효(013의 설계 의도 그대로).
ALTER TABLE tasks
    ADD CONSTRAINT tasks_project_id_fkey
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL;

-- 배타적 소유 — M:N 조인 테이블이 아니라 직접 FK. 워커/호스트는 최대 1개
-- 프로젝트에만 소속되며, NULL이면 일반 풀(어느 프로젝트에도 안 속함).
ALTER TABLE workers ADD COLUMN IF NOT EXISTS project_id UUID REFERENCES projects(id) ON DELETE SET NULL;
ALTER TABLE hosts   ADD COLUMN IF NOT EXISTS project_id UUID REFERENCES projects(id) ON DELETE SET NULL;
CREATE INDEX idx_workers_project_id ON workers(project_id);
CREATE INDEX idx_hosts_project_id   ON hosts(project_id);
```

**일관성 불변식(애플리케이션 레벨로 강제, Postgres는 테이블 간 CHECK를 지원하지
않음)**: 워커가 `hosts.worker_id`로 특정 host에 연결돼 있다면, 그 워커의
`project_id`는 host의 `project_id`와 일치해야 합니다 — host를 프로젝트에
배정/해제할 때 그 host에 연결된 워커의 `project_id`도 함께 동기화합니다
(`#49`에서 `agents.host_id`도 이 불변식을 그대로 물려받음 — host가 없는
관계로 남는 워커는 없어야 항상 정합성이 유지됩니다).

⚠️ **이 불변식은 지금까지 `assign_host_to_project`(host→그 host에 연결된
워커로 전파) 방향만 강제하고 있었고, 반대 방향인 `assign_worker_to_project`
(워커를 직접 다른 프로젝트로 재배정)는 아무 체크도 하지 않아 이 불변식을
그냥 깨뜨릴 수 있는 구멍이었습니다(팀 검토에서 발견, critical) — 리소스
경쟁을 구조적으로 막는다는 이 기능의 존재 이유를 무력화하는 문제라
`assign_worker_to_project`에도 반드시 같은 검사를 넣습니다**: 대상 워커가
`hosts.worker_id`로 어떤 host에 연결돼 있다면, 요청한 `project_id`가 그
host의 `project_id`와 다를 경우 `409 Conflict`("worker는 host
`<hostname>`에 연결돼 있어 그 host와 다른 프로젝트로 개별 재배정할 수
없습니다 — 먼저 host를 재배정하세요")로 차단합니다. host에 연결되지 않은
독립 워커(아래 참고)만 `assign_worker_to_project`로 자유롭게 재배정할 수
있습니다.

**워커 재등록 시 재동기화 규칙**: `upsert_worker`가 이미 알려진 워커를
재등록(heartbeat 재연결 등)할 때, 그 워커가 `hosts.worker_id`로 어떤
host에 연결돼 있다면 `project_id`는 그 host의 `project_id`로 **매번
무조건 재동기화**합니다(host가 정본, 워커는 항상 파생값) — 호스트가 다른
프로젝트로 재배정된 사이 그 host 위의 워커 프로세스가 재연결하면, 워커의
예전 `project_id`를 그대로 유지하지 않고 host의 현재 값으로 덮어써야 위
불변식이 항상 유지됩니다. ⚠️ **host에 연결되지 않은 독립 워커(팀 검토에서
발견, major — `ui-design.md` §3.10이 "독립 Worker 배정" 접이식 섹션으로
이미 이 케이스를 예상하고 있었으나 이 문서엔 반영이 안 돼 있었습니다)는
이 재동기화 대상이 아닙니다** — `upsert_worker`는 재등록 시 그 워커에
연결된 host 행이 있는지 먼저 확인하고, 없으면 `project_id`를 건드리지
않고 `assign_worker_to_project`로 직접 설정된 값을 그대로 보존합니다
(host 연결이 없는데 무조건 재동기화하면 그 값이 매 재등록마다 조용히
`NULL`로 초기화되는 버그가 생깁니다).

또한 `#49`가 host당 여러 에이전트(=여러 워커)를 도입하면서
`hosts.worker_id`가 표현할 수 있는 "그 host의 워커"는 최대 1개뿐이라는
전제가 깨졌습니다 — `hosts.worker_id` 컬럼 자체에는 여전히 DB 레벨
`UNIQUE` 제약이 없다는 것도 이번 검토에서 확인된 기존 공백입니다(minor,
`crates/fleet-store/migrations/007_hosts.sql`). `#49`의 동적 프로비저닝
워커는 이 host-단일-연결 재동기화 경로를 타지 않고 별도 경로로
`project_id`를 직접 설정합니다 —
[`agent-provisioning-design.md`](agent-provisioning-design.md) §4 6~7단계
참고.

### `fleet-core` 신규 타입

```rust
// crates/fleet-core/src/project.rs (신규 파일)

/// 이 프로젝트의 에이전트를 수동으로만 만들지, 오케스트레이터가 자동으로도
/// 만들지(agent-provisioning-design.md §4.1 AgentAutoProvisioner가 소비).
/// `Project`의 필드이므로 정의 소유권도 여기(#48/fleet-core::project)에
/// 둔다 — `#49` 문서는 이 타입을 재수출만 참조한다.
pub enum AgentProvisioningMode { Manual, Automatic }

pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub description: Option<String>,
    pub created_by: Option<String>,
    pub agent_provisioning_mode: AgentProvisioningMode,  // 기본 Manual
    /// `AgentTemplateId`는 `#49`에서 정의됨 — `#48` Phase 1은 원시 `Uuid`로만
    /// 다룬다(013_task_threads.sql의 `tasks.project_id` 예약과 동일 패턴).
    /// `#49` Phase 1이 `agent_templates` 테이블을 만들 때 FK 제약과 강타입
    /// 변환(`.map(AgentTemplateId)`)을 추가한다.
    pub default_agent_template_id: Option<Uuid>,
    pub agent_idle_timeout_secs: Option<u32>,
    /// 결과물 디렉토리 기본 경로 템플릿(agent-provisioning-design.md §9).
    pub workdir_template: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct ProjectFilter {
    pub limit: usize,
    pub offset: usize,
}
```

`Worker`/`Host`에도 `project_id: Option<ProjectId>` 필드가 추가됩니다(기존
구조체 확장, 새 파일 아님).

### `Task`/`TaskRequest`에 대한 변경

`Task.project_id: Option<ProjectId>`는 이미 존재합니다. `TaskRequest`에도 동일
필드를 추가합니다(현재는 없음 — `Task::from_request()`가 항상 `None`으로 채움).

## 4. `Store` 트레이트 확장

Task/Worker CRUD와 동일하게 **필수(mandatory) 메서드**로 추가합니다(RBAC류
`Unsupported` 기본 구현에 기대지 않음 — `WorkerSelector`가 직접 호출).

```rust
// ── Project ──────────────────────────────────────────────────────
async fn insert_project(&self, project: &Project) -> Result<(), StoreError>;
async fn get_project(&self, id: ProjectId) -> Result<Option<Project>, StoreError>;
async fn list_projects(&self, filter: &ProjectFilter) -> Result<Vec<Project>, StoreError>;
async fn delete_project(&self, id: ProjectId) -> Result<bool, StoreError>;

/// 워커를 프로젝트에 배타적으로 배정 — 이미 다른 프로젝트에 배정돼 있으면
/// 그 배정을 덮어쓴다(호출자가 먼저 확인 없이 재배정하면 이전 프로젝트에서
/// 조용히 빠진다는 뜻이므로, API/CLI 레벨에서 확인 프롬프트를 두는 것을 권장).
/// **이 워커가 `hosts.worker_id`로 어떤 host에 연결돼 있고 그 host의
/// `project_id`와 `project_id` 인자가 다르면 `StoreError::Conflict`를
/// 반환하고 배정하지 않는다**(§3 불변식 가드 — 팀 검토에서 발견한 강제
/// 누락 수정). 독립 워커(host 미연결)에만 자유롭게 적용된다.
async fn assign_worker_to_project(&self, project_id: ProjectId, worker_id: WorkerId) -> Result<(), StoreError>;
/// project_id를 NULL로(일반 풀로 되돌림).
async fn unassign_worker_from_project(&self, worker_id: WorkerId) -> Result<(), StoreError>;
async fn list_project_worker_ids(&self, project_id: ProjectId) -> Result<Vec<WorkerId>, StoreError>;
async fn list_project_workers(&self, project_id: ProjectId) -> Result<Vec<Worker>, StoreError>;

/// host를 프로젝트에 배타적으로 배정 — §3의 불변식대로, 이 host에 연결된
/// 워커(hosts.worker_id)가 있으면 그 워커의 project_id도 함께 동기화한다.
/// **`#49` 이후: 이 host 위에 떠 있는 모든 Agent(`agents.host_id`
/// 일치)의 `project_id`도 같은 트랜잭션에서 함께 갱신한다** — Agent의
/// `project_id`는 생성 시점에 host에서 읽어와 채우는 파생 필드라서
/// (`agent-provisioning-design.md` §3), host가 나중에 다른 프로젝트로
/// 재배정되면 이 캐스케이드가 없는 한 기존 Agent들의 `project_id`가
/// 영구히 옛 값에 고정된 채 남아 자신이 연결된 Worker의 `project_id`
/// (위 규칙으로 정상 재동기화됨)와 서로 모순되는 상태가 됩니다(팀 검토
/// critical — 미검증 상태로 보고됐으나 실제로 반영이 필요한 실질적 공백
/// 이라 이번에 바로 반영).
async fn assign_host_to_project(&self, project_id: ProjectId, host_id: Uuid) -> Result<(), StoreError>;
async fn unassign_host_from_project(&self, host_id: Uuid) -> Result<(), StoreError>;
async fn list_project_hosts(&self, project_id: ProjectId) -> Result<Vec<Host>, StoreError>;
```

## 5. 디스패치 프로토콜 확장 (`WorkerSelector`)

![Project-Aware Dispatch Logic](../assets/diagrams/architecture/project-aware-dispatch-logic.mermaid)

기존 파이프라인(`crates/fleet-scheduler/src/selector.rs`)에 프로젝트 하드
필터를 삽입합니다 — 회로차단기 제외(실제 코드 3단계)·용량 필터(실제
코드 3.5단계, `retain()`) 뒤, `server_hint` 처리(실제 코드 4단계) 앞입니다
(⚠️ 팀 검토에서 발견 — 이전 서술의 "4~5단계"/"6단계"라는 번호는 실제
`selector.rs`의 주석 번호와 일치하지 않았습니다, minor 수정). 새 필터는
이 문서에서 편의상 "5.5단계"로 부르지만 실제 구현 시 위 실제 단계 번호
사이에 삽입하면 됩니다. `required_labels`/`model` 필터와 정확히 같은
방식(하드 `retain()`)입니다 — 특별 취급 없음:

- `task.project_id`가 `None`이면 스킵(기존 동작과 100% 동일 — 회귀 없음).
- `Some(project_id)`이면 후보 풀을 `worker.project_id == Some(project_id)`인
  워커로 `retain()`합니다.
  - 결과가 비어있지 않으면 계속 진행(`server_hint` 또는 최소부하 선택 — 실제 코드 4단계).
  - **결과가 비어있으면 새 에러 `SelectionError::NoWorkerForProject(ProjectId)`를
    반환합니다** — `NoMatchingLabels`/`NoWorkerForModel`과 동일한 패턴(신규
    변형 하나 추가, 기존 5개 변형 그대로).
- `dispatch_existing()`은 이 에러를 **기존 `WorkerUnavailable` 실패 종류와
  동일하게 취급**합니다 — 새 재시도 로직을 만들지 않습니다:
  - `submit()`이 재시도 비활성(`max_dispatch_retries == 0`)이면 즉시 `Failed`.
  - 재시도 활성이면(기본, `#38`) 작업을 `Pending`으로 남기고 `retry_count`를
    올린 뒤 `Ok(task_id)` 반환 — `Reconciler`가 다음 tick에 재시도(예: 그
    사이 `#49`의 Automatic 모드가 새 에이전트를 프로비저닝했다면 그때 성공).
    소진되면 기존처럼 dead-letter(`Failed`).
  - **알려진 개선 여지**: 기존 `#38` dead-letter 로직(`reconcile.rs`)은
    소진 시 `TaskFailure.error`를 `"dispatch retries exhausted (N
    attempts)"`로 덮어써, 원래의 `SelectionError` 메시지(예:
    `NoWorkerForProject`)가 사라집니다. Pending 상태일 때는
    `ui-design.md` §3.10의 `waiting (no project worker)` 배지로 조회
    시점 계산이 가능하지만, `Failed`로 dead-letter된 뒤에는 저장된
    데이터만으로 사유를 복원할 수 없습니다. 개선 방향(신규 재시도 로직을
    만들지 않는다는 원칙은 유지, 메시지 포맷만 개선):
    `error = format!("dispatch retries exhausted ({attempts} attempts) —
    last error: {last_error}")`처럼 마지막 에러 텍스트를 보존 — `#38`
    구현 변경 범위이지만 `#48`이 이 개선에 실질적으로 의존.
- `server_hint`는 이 필터 **이후**에 평가되므로, 힌트 워커가 그 프로젝트
  소속이 아니면 애초에 후보 풀에 없어 `HintedNotFound`/`HintedUnavailable`로
  자연히 걸러집니다 — 별도 처리 불필요, 하드 격리가 힌트에도 일관되게
  적용됩니다.

### 재배정 시 진행 중 태스크 정책

워커/호스트가 프로젝트 A에서 B로 재배정될 때, 그 워커에서 이미
`Dispatched` 상태로 진행 중인 태스크는 **그대로 완료까지 진행합니다** —
재배정을 이유로 강제 취소하거나 다른 워커로 옮기지 않습니다. 재배정은
**향후 디스패치 자격에만 영향**을 줍니다(그 시점 이후의 신규 디스패치
부터 새 프로젝트 소속으로 취급). 이 정책은 AskUserQuestion으로 확인된
결정이었으나 이 문서 본문에는 반영이 안 되고 `roadmap.md`에만 기록돼
있던 걸 팀 검토(major)로 발견해 이번에 정식으로 옮겨 적습니다.

## 6. RBAC 권한 추가

| 변형 | 직렬화 이름 | 의미 |
|---|---|---|
| `ProjectCreate` | `project:create` | 프로젝트 생성 |
| `ProjectRead` | `project:read` | 프로젝트 목록/상세 조회 |
| `ProjectDelete` | `project:delete` | 프로젝트 삭제 |
| `ProjectAssign` | `project:assign` | host/worker 배정·해제 |

`Admin`은 전부, `Operator`는 `ProjectRead`+`ProjectAssign`, `Viewer`는 `ProjectRead`만.

## 7. API 표면

### 대시보드 REST

| Method | Path | 권한 | 설명 |
|---|---|---|---|
| GET | `/projects` | `ProjectRead` | 프로젝트 목록 페이지 |
| GET | `/projects/:id` | `ProjectRead` | 프로젝트 상세 페이지 |
| GET | `/projects/new` | `ProjectCreate` | 생성 폼(⚠️ 팀 검토, major — 이전엔 "세션"만 요구해 `ui-design.md`가 명시한 "operator는 열람만"(생성 폼조차 admin만 접근) 의도와 정면 충돌했습니다. 기존 관례(`crates/fleet-dashboard/src/provisioning.rs`의 `provision_page`가 `serve_page_if_permitted(HostProvision, ...)`로 `/hosts/provision` 페이지 자체를 권한 게이트하는 패턴)와 동일하게, 생성 폼 페이지 라우트 자체를 `ProjectCreate`로 게이트합니다) |
| PATCH | `/api/projects/:id` | `ProjectCreate` | 프로젝트 필드 수정(name/description/agent_provisioning_mode/workdir_template/default_agent_template_id/agent_idle_timeout_secs) — ⚠️ 팀 검토(minor)로 신설. `agent-provisioning-design.md` §4.1이 "Automatic 전환 API/CLI"의 존재를 전제하면서도 이 엔드포인트 자체가 어느 문서에도 없던 공백을 메웁니다. 생성과 동일한 등급(`ProjectCreate`, admin 기본)을 재사용 — 별도 `ProjectUpdate` 권한을 새로 만들 만큼 성격이 다르지 않다고 판단(수정 대상 필드가 모두 생성 시점에도 채우는 필드라 "생성을 나중에 마저 채우는 것"에 가까움) |
| GET | `/api/projects` | `ProjectRead` | 목록 JSON |
| POST | `/api/projects` | `ProjectCreate` | 생성 |
| GET | `/api/projects/:id` | `ProjectRead` | 상세 JSON |
| DELETE | `/api/projects/:id` | `ProjectDelete` | 삭제 |
| PUT | `/api/workers/:id/project` | `ProjectAssign` | 워커를 프로젝트에 배정(`{project_id}`, 배타적 — 이전 배정 덮어씀) |
| DELETE | `/api/workers/:id/project` | `ProjectAssign` | 워커 배정 해제(일반 풀로) |
| PUT | `/api/hosts/:id/project` | `ProjectAssign` | 호스트 배정(동일 패턴) |
| DELETE | `/api/hosts/:id/project` | `ProjectAssign` | 호스트 배정 해제 |

기존 `POST /api/projects/:id/workers`(M:N 추가) 대신 `PUT /api/workers/:id/project`
(배타적 단일값 설정)로 바뀐 점에 주의 — REST 시맨틱이 "컬렉션에 추가"에서
"단일 리소스 값을 설정"으로 달라졌습니다(배타적 소유이므로).

### MCP 도구 (`fleet_*`)

| 도구 | 입력 | 비고 |
|---|---|---|
| `fleet_create_project` | `name`(필수), `description` | |
| `fleet_list_projects` | `limit`, `offset` | |
| `fleet_delete_project` | `project_id`(필수) | |
| `fleet_assign_worker_to_project` | `project_id`, `worker_id`(둘 다 필수) | 배타적 — 이전 배정 덮어씀 |

`fleet_dispatch_task`는 기존 입력 스키마에 `project_id`(선택) 필드만 추가.
`fleet_assign_host_to_project`는 1단계 MCP 범위에서 제외(대시보드 REST만).

## 8. 단계별 구현 계획

1. **Phase 1 — 스키마 + Store + RBAC**: `015_projects.sql`,
   `fleet-core::Project`/`ProjectFilter`, `workers.project_id`/`hosts.project_id`
   확장, `Store` 확장(PgStore+MemStore), `PermissionKind` 4종. 신규 테스트:
   배정 시 이전 배정 덮어쓰기 동작, host↔worker `project_id` 동기화 불변식,
   `ON DELETE SET NULL` 동작(실제 Postgres 대상).
2. **Phase 2 — 디스패치 통합**: `WorkerSelector`에 5.5단계 하드 필터 +
   `SelectionError::NoWorkerForProject` 삽입, `dispatch_existing()`이 이를
   `WorkerUnavailable`과 동일하게 재시도 경로로 태우는지 확인,
   `TaskRequest.project_id` 추가. 신규 테스트: 프로젝트 없음(회귀 없음)/소속
   워커로 정상 디스패치/소속 워커 없음 → Pending+재시도 → 소진 시 dead-letter
   3가지 경로, 힌트가 다른 프로젝트 워커를 가리킬 때 자연히 거부되는지.
3. **Phase 3 — API + MCP**: §7 엔드포인트.
4. **Phase 4 — 대시보드 UI**: `/projects`, `/projects/:id`, `/projects/new`.

## 9. 열린 질문

- **하드 격리를 다시 완화해야 하는 경우**: 실사용 중 프로젝트에 배정된 워커가
  전부 다운됐을 때 태스크가 무기한 대기하는 게 문제가 되면(현재는 `#38`의
  `max_dispatch_retries` 소진 후 dead-letter로 끝남), 프로젝트별
  `strict_isolation: bool` 오버라이드를 재검토할 수 있습니다 — 1단계에서는
  하드가 기본이자 유일한 동작.
- **워커별 프로젝트 우선순위/쿼터**: 배타적 소유라 프로젝트 간 쿼터 경합
  자체가 사라졌습니다(같은 프로젝트 내 여러 워커 간 로드밸런싱은 기존
  최소부하 선택 그대로).
- **project_id 없는 태스크의 취급**: 계속 완전히 허용(일반 풀 워커 후보).
- **장수명 에이전트의 메모리 보존 정책**: 이 문서의 범위 밖(`agent_memory`는
  `#49`의 엔티티) — 상세 열린 질문은
  [`agent-provisioning-design.md`](agent-provisioning-design.md) §12 참고.
- **`ProjectAssign`을 `Operator` 기본 권한에 둘지 재검토**(팀 검토에서
  발견, **critical로 격상** — 처음엔 등급 비일관 정도의 minor 문제로만
  기록했으나, 재검증 라운드에서 이 비일관이 실제로 악용 가능한 구체적
  경로로 이어짐을 확인): `Operator`는 `ProjectAssign`(여유 host를
  `agent_provisioning_mode=automatic` 프로젝트에 배정 가능)과 기존
  `TaskCreate`(해당 프로젝트 스코프 태스크 제출 가능, `TaskRequest.project_id`에
  별도 권한 게이트 없음)를 이미 보유하고 있어, 이 둘을 조합하면
  `agent-provisioning-design.md` §4.1의 `AgentAutoProvisioner`가 RBAC
  검사 없이 자동으로 Agent를 생성하도록 트리거할 수 있습니다 — 즉
  `Operator`가 `AgentCreate`(`Admin` 전용으로 선언된 권한, §10)를 전혀
  거치지 않고도 사실상 Agent를 생성시키는 셈입니다. 이 코드베이스의 기존
  관례상 `WorkerRegister`/`WorkerDelete`/`HostProvision` 같은 비슷한 급의
  인프라 변경 권한은 전부 `Admin` 전용이므로, `ProjectAssign`을
  `Admin`으로 올리는 쪽이 유력한 해소책으로 보이지만 — 이는 정책 변경이라
  이번 라운드에서 임의로 바꾸지 않고, **Phase 1(스키마+Store+RBAC) 구현
  착수 전 반드시 확정해야 하는 차단 항목**으로 격상해 기록합니다.

## UI/UX 설계

프로젝트 관련 화면은 `ui-design.md`(대시보드 화면 설계 정본)에 아래와 같이
추가했습니다 — 데이터/API는 이 문서가, 화면은 `ui-design.md`가 각각 정본을
담당합니다(중복 서술 방지). 대시보드는 사용자 토글형 다크 모드가 아니라
**단일 Apple Design System**([`ui-design.md`](../ui-dashboard/ui-design.md) §2)임에
유의 — 신규 페이지도 이를 그대로 물려받습니다.

- [`ui-design.md`](../ui-dashboard/ui-design.md) §3.9 프로젝트 목록
- [`ui-design.md`](../ui-dashboard/ui-design.md) §3.10 프로젝트 상세 —
  섹션 우선순위(헤더 → 배정 host/worker → 실행 중 agent → 최근 태스크 순,
  agent 메모리 브라우저는 agent 상세로 위임)와 **하드 격리발 대기 상태를
  일반 pending/failed와 구분하는 StatusPill 변형**(`waiting (no project
  worker)`, API가 조회 시점에 파생 계산 — 스키마 변경 없음)을 확정.
- 신규 페이지는 `ui-design.md` §2 디자인 시스템(StatusPill/Badge/Card/
  DataTable/EmptyState 공통 컴포넌트, §6)과 §8 반응형 전략을 그대로
  물려받습니다 — 새 컨벤션을 만들지 않습니다.

## 관련 문서

- [`docs/roadmap/roadmap.md`](../roadmap/roadmap.md) #48 — 구현 진행 상황 정본.
- [`docs/architecture/log.md`](log.md) — 이 설계에 도달한 경위(개정 이력).
- [`docs/architecture/agent-provisioning-design.md`](agent-provisioning-design.md) — `#49`,
  이 하드 격리 모델 위에 host 내 동적 에이전트 생성을 쌓는 후속 설계.
- [`docs/architecture/agent-harness-composition-design.md`](agent-harness-composition-design.md) — `#51`,
  `constitution_prompt` 컬럼을 이 `projects` 테이블에 추가하는 후속 확장.
- [`docs/ui-dashboard/ui-design.md`](../ui-dashboard/ui-design.md) §3.9~§3.10 —
  화면 설계 정본.
