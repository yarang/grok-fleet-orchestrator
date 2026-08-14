# 프로젝트(Project) 기능 설계

> 작성일: 2026-08-14. 로드맵 [`#48`](../roadmap/roadmap.md)에 대응하는 설계 문서입니다.
> **개정 (2026-08-14, 2차)**: 최초 설계는 host/worker↔project를 다대다(M:N) +
> 소프트 디스패치로 확정했으나, 사용자가 "리소스 경쟁/충돌 예방을 위해 하드
> 격리가 기본이어야 한다"고 재검토를 요청 — 워커 M:N 공유도 결국 같은 host의
> 물리 자원과 워커 프로세스 자체의 세션 슬롯을 여러 프로젝트가 경합하게 만든다는
> 점을 재확인하고, **워커/호스트 모두 배타적 소유 + 하드 디스패치**로
> 전면 개정했습니다. 이 문서는 **설계 확정** 단계이며, 아직 구현되지 않았습니다 —
> 구현 진행 상황은 `roadmap.md` #48 항목을 정본으로 확인하세요.

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

### 2.1 최종 결정 (2026-08-14, 2차 개정)

| 결정 사항 | 채택안 | 근거 |
|---|---|---|
| 프로젝트 ↔ host/worker 소속 관계 | **배타적(exclusive) 1:N** — host/worker는 최대 1개 프로젝트에만 소속, 미소속이면 일반 풀 | `workers.project_id`/`hosts.project_id` 직접 FK. host의 물리 자원과 그 위에서 도는 워커 프로세스의 세션 슬롯을 여러 프로젝트가 나눠 쓰면, "소프트"로 표현해도 실제로는 항상 경쟁 상태가 생긴다 — 스키마 레벨에서 원천 차단하는 게 더 단순하고 확실함 |
| project_id 지정 태스크의 디스패치 범위 | **하드(strict)** — 그 프로젝트 소유 워커만 후보. 후보가 없으면 전체 풀로 폴백하지 않음 | §5 참고 — 새 에러/재시도 메커니즘을 만들지 않고 **`#38`의 기존 `WorkerUnavailable` 재시도/Dead-Letter 경로를 그대로 재사용** |

### 2.2 1차 결정에서 뒤집힌 이유 (경위 기록)

최초 설계(1차)는 M:N + 소프트를 사용자 확인 후 채택했었습니다. 이후 사용자가
`#49`(에이전트 동적 프로비저닝) 설계 논의 중 "host에는 여유가 있을 때만
agent를 만든다"는 요구사항을 구체화하면서 "프로젝트의 하드 격리가 기본이어야
충돌/경쟁을 예방한다"는 원칙을 제시했습니다. 처음에는 "host 소유권만 하드로
하고 `#48`의 기존 워커 M:N은 안 건드리는" 절충안을 제안했으나, 사용자가
**"host 소유권이 비배타적이어도 워커를 실행하면서 리소스 경쟁이 생기지
않는가?"**라고 되물었고 — 맞는 지적이었습니다. 워커가 M:N으로 여러
프로젝트에 공유되면, 그 워커 프로세스가 도는 host의 물리 자원과 워커 자체의
`max_concurrent` 세션 슬롯을 다른 프로젝트의 태스크와 항상 경합하게 됩니다.
"소프트"라는 이름을 붙여도 경쟁 상태 자체는 없어지지 않습니다. 그래서
워커/호스트 양쪽 모두 배타적 소유로 전면 개정했습니다 — 아직 구현 전이라
재작업 비용 없이 지금 바로잡는 게 맞다고 판단했습니다.

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
    default_agent_template_id UUID,  -- automatic 모드에서 쓸 기본 템플릿(agent_templates FK, #49 도입 후 연결)
    agent_idle_timeout_secs INTEGER,  -- automatic 모드로 만든 에이전트의 유휴 자동 종료 기준(NULL이면 자동 종료 안 함)
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- updated_at 자동 갱신 트리거는 007_hosts.sql의 update_hosts_updated_at() 패턴을 재사용.

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

### `fleet-core` 신규 타입

```rust
// crates/fleet-core/src/project.rs (신규 파일)
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub description: Option<String>,
    pub created_by: Option<String>,
    pub agent_provisioning_mode: AgentProvisioningMode,  // #49 참고, 기본 Manual
    pub default_agent_template_id: Option<AgentTemplateId>,
    pub agent_idle_timeout_secs: Option<u32>,
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
async fn assign_worker_to_project(&self, project_id: ProjectId, worker_id: WorkerId) -> Result<(), StoreError>;
/// project_id를 NULL로(일반 풀로 되돌림).
async fn unassign_worker(&self, worker_id: WorkerId) -> Result<(), StoreError>;
async fn list_project_worker_ids(&self, project_id: ProjectId) -> Result<Vec<WorkerId>, StoreError>;
async fn list_project_workers(&self, project_id: ProjectId) -> Result<Vec<Worker>, StoreError>;

/// host를 프로젝트에 배타적으로 배정 — §3의 불변식대로, 이 host에 연결된
/// 워커(hosts.worker_id)가 있으면 그 워커의 project_id도 함께 동기화한다.
async fn assign_host_to_project(&self, project_id: ProjectId, host_id: Uuid) -> Result<(), StoreError>;
async fn unassign_host(&self, host_id: Uuid) -> Result<(), StoreError>;
async fn list_project_hosts(&self, project_id: ProjectId) -> Result<Vec<Host>, StoreError>;
```

## 5. 디스패치 프로토콜 확장 (`WorkerSelector`)

![Project-Aware Dispatch Logic](../assets/diagrams/architecture/project-aware-dispatch-logic.mermaid)

기존 파이프라인(`crates/fleet-scheduler/src/selector.rs`)에 **5.5단계**로
프로젝트 하드 필터를 삽입합니다 — 회로차단기/용량 필터(4~5단계, `retain()`)
뒤, `server_hint` 처리(6단계) 앞입니다. `required_labels`/`model` 필터와 정확히
같은 방식(하드 `retain()`)입니다 — 특별 취급 없음:

- `task.project_id`가 `None`이면 스킵(기존 동작과 100% 동일 — 회귀 없음).
- `Some(project_id)`이면 후보 풀을 `worker.project_id == Some(project_id)`인
  워커로 `retain()`합니다.
  - 결과가 비어있지 않으면 계속 진행(6단계 `server_hint` 또는 최소부하 선택).
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
- `server_hint`는 이 필터 **이후**에 평가되므로, 힌트 워커가 그 프로젝트
  소속이 아니면 애초에 후보 풀에 없어 `HintedNotFound`/`HintedUnavailable`로
  자연히 걸러집니다 — 별도 처리 불필요, 하드 격리가 힌트에도 일관되게
  적용됩니다(1차 설계의 "힌트가 소프트 선호보다 우선"이라는 예외를 이번
  개정으로 제거 — 하드 원칙과 모순되는 예외를 남기지 않기 위함).

## 6. RBAC 권한 추가

(1차와 동일, 변경 없음)

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
| GET | `/projects` | 세션 | 프로젝트 목록 페이지 |
| GET | `/projects/:id` | 세션 | 프로젝트 상세 페이지 |
| GET | `/projects/new` | 세션 | 생성 폼 |
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
  자체가 사라졌으므로 이번 개정으로 사실상 해소됨(같은 프로젝트 내 여러
  워커 간 로드밸런싱은 기존 최소부하 선택 그대로).
- **project_id 없는 태스크의 취급**: 계속 완전히 허용(일반 풀 워커 후보).

## 관련 문서

- [`docs/roadmap/roadmap.md`](../roadmap/roadmap.md) #48 — 구현 진행 상황 정본.
- [`docs/architecture/agent-provisioning-design.md`](agent-provisioning-design.md) — `#49`,
  이 하드 격리 모델 위에 host 내 동적 에이전트 생성을 쌓는 후속 설계.
