# 프로젝트(Project) 기능 설계

> 작성일: 2026-08-14. 로드맵 [`#48`](../roadmap/roadmap.md)에 대응하는 설계 문서입니다.
> 이 문서는 **설계 확정** 단계이며, 아직 구현되지 않았습니다 — 구현 진행 상황은
> `roadmap.md` #48 항목을 정본으로 확인하세요.

## 1. 배경 및 동기

`crates/fleet-core/src/ids.rs`에는 이미 `ProjectId` newtype이, `tasks` 테이블에는
`project_id UUID` nullable 컬럼(`013_task_threads.sql`)이 예약돼 있습니다 — 둘 다
"나중에 project 기능이 도입되면 재귀 backfill 없이 바로 채워질 수 있도록" 미리
만들어 둔 자리표시자였고, 실제 `Project` 엔티티나 host/agent(워커) 소속 개념은
지금까지 전혀 구현되지 않았습니다.

이 설계는 그 자리표시자를 실제 기능으로 채우는 것을 목표로 합니다:

1. **프로젝트가 여러 host와 agent(워커)를 담을 수 있어야 한다** — 하나의 인프라
   풀을 여러 프로젝트가 공유하는 멀티 프로젝트 운영을 지원.
2. **하나의 프로젝트에 여러 agent가 배치될 수 있어야 한다** — 즉 프로젝트
   스코프 태스크가 여러 워커에 분산 디스패치될 수 있어야 한다(단일 워커 전용이
   아님).
3. 위 둘을 지원하는 **디스패치 프로토콜과 배치/해제 절차**를 정의한다.

## 2. 핵심 설계 결정 (사용자 확인 완료, 2026-08-14)

이 두 결정은 스키마·API·디스패처 로직에 직접 영향을 주므로, 구현 착수 전에
사용자에게 확인받았습니다.

| 결정 사항 | 채택안 | 근거 |
|---|---|---|
| 프로젝트 ↔ host/agent 소속 관계 | **다대다(M:N)** — 하나의 host/worker가 여러 프로젝트에 동시 배치 가능 | 인프라 풀을 여러 프로젝트가 공유하는 운영 형태를 지원. 조인 테이블(`project_workers`/`project_hosts`)로 구현 |
| project_id 지정 태스크의 디스패치 범위 | **소프트 힌트(soft)** — 배치된 agent가 없거나 전부 불가하면 전체 풀로 폴백 | 가용성 우선. 프로젝트 경계가 자원 격리를 강제하지 않음(엄격 격리가 필요해지면 후속 항목으로 재검토) |

M:N 소속을 택했기 때문에 "한 워커가 여러 프로젝트의 태스크를 동시에 받을 때
용량을 어떻게 나눌 것인가?"라는 질문이 남는데, §5에서 다루듯 **별도의 쿼터/공정
분배 메커니즘을 신설하지 않습니다** — 기존 `Worker.active_tasks`/`max_concurrent`
기반 최소부하 선택이 이미 프로젝트 출처와 무관하게 워커의 총 부하를 반영하므로,
바쁜 워커는 어느 프로젝트에서 오는 요청이든 자연히 후순위로 밀립니다. 이것으로
1단계 구현 범위에서는 충분하다고 판단했습니다 — 엄격한 프로젝트별 용량 예약이
필요해지면 별도 로드맵 항목으로 분리합니다.

## 3. 데이터 모델

![Project Data Model](../assets/diagrams/architecture/project-data-model.mermaid)

### 신규 테이블 (`015_projects.sql`)

```sql
CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    created_by TEXT,                          -- username, audit_log의 actor_label과 동일한
                                                -- "하드 FK 아님" 패턴 — 계정 삭제로 프로젝트
                                                -- 이력이 끊기지 않도록.
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- updated_at 자동 갱신 트리거는 007_hosts.sql의 update_hosts_updated_at() 패턴을 재사용.

CREATE TABLE project_workers (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    worker_id  UUID NOT NULL REFERENCES workers(id)  ON DELETE CASCADE,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, worker_id)
);
CREATE INDEX idx_project_workers_worker_id ON project_workers(worker_id);

CREATE TABLE project_hosts (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    host_id    UUID NOT NULL REFERENCES hosts(id)    ON DELETE CASCADE,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, host_id)
);
CREATE INDEX idx_project_hosts_host_id ON project_hosts(host_id);

-- 013_task_threads.sql이 예약해 둔 컬럼에 이제야 FK를 건다. 기존 행은
-- project_id IS NULL인 채로 계속 유효(013의 설계 의도 그대로).
ALTER TABLE tasks
    ADD CONSTRAINT tasks_project_id_fkey
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL;
```

조인 테이블을 `hosts`/`workers`처럼 별도 `id` PK 없이 복합 PK(`project_id`,
`worker_id`)로 둔 것은 "배치 여부"만 표현하면 충분하고 배치 자체를 참조하는
하위 엔티티가 없기 때문입니다(`host_events`처럼 자체 이력을 갖는 엔티티가
아님).

### `fleet-core` 신규 타입

```rust
// crates/fleet-core/src/project.rs (신규 파일)
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub description: Option<String>,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct ProjectFilter {
    pub limit: usize,   // WorkerFilter/TaskFilter와 동일한 관례, 기본 100
    pub offset: usize,
}
```

기존 `TaskFilter`/`WorkerFilter` 관례(`Option<T>` 필드 + `limit`/`offset`)를
그대로 따릅니다. 1단계에서는 이름 검색 등 세부 필터는 추가하지 않습니다(필요성이
확인되면 후속으로 추가).

### `Task`/`TaskRequest`에 대한 변경

`Task.project_id: Option<ProjectId>`는 이미 존재합니다. `TaskRequest`에도 동일
필드를 추가해 `fleet_dispatch_task`/대시보드 태스크 생성 폼에서 지정할 수 있게
합니다(현재 `TaskRequest`에는 이 필드가 없음 — `Task::from_request()`가 항상
`project_id: None`으로 채우는 중).

## 4. `Store` 트레이트 확장

Task/Worker CRUD와 동일하게 **필수(mandatory) 메서드**로 추가합니다 — RBAC용
메서드들처럼 `Unsupported` 기본 구현에 기대지 않습니다. 이유: 프로젝트 배치
조회(`list_project_workers`)가 `WorkerSelector`(핵심 디스패치 경로)에서 호출되므로
`fleet_store::mem::MemStore`(테스트 전용)도 반드시 실제 동작해야 스케줄러
단위 테스트가 의미를 가집니다 — RBAC처럼 "이 경로는 PgStore만 있으면 됨"이 아닙니다.

```rust
// ── Project ──────────────────────────────────────────────────────
async fn insert_project(&self, project: &Project) -> Result<(), StoreError>;
async fn get_project(&self, id: ProjectId) -> Result<Option<Project>, StoreError>;
async fn list_projects(&self, filter: &ProjectFilter) -> Result<Vec<Project>, StoreError>;
async fn delete_project(&self, id: ProjectId) -> Result<bool, StoreError>;

async fn assign_worker_to_project(&self, project_id: ProjectId, worker_id: WorkerId) -> Result<(), StoreError>;
async fn unassign_worker_from_project(&self, project_id: ProjectId, worker_id: WorkerId) -> Result<bool, StoreError>;
/// WorkerSelector가 소프트 우선순위 판단에 직접 호출하는 경로 — 워커 ID만
/// 필요하므로 전체 Worker를 반환하지 않고 ID 목록만 반환한다(불필요한
/// 조인/변환 비용 회피).
async fn list_project_worker_ids(&self, project_id: ProjectId) -> Result<Vec<WorkerId>, StoreError>;
/// 대시보드 프로젝트 상세 페이지용 — 전체 Worker 반환.
async fn list_project_workers(&self, project_id: ProjectId) -> Result<Vec<Worker>, StoreError>;

async fn assign_host_to_project(&self, project_id: ProjectId, host_id: Uuid) -> Result<(), StoreError>;
async fn unassign_host_from_project(&self, project_id: ProjectId, host_id: Uuid) -> Result<bool, StoreError>;
async fn list_project_hosts(&self, project_id: ProjectId) -> Result<Vec<Host>, StoreError>;
```

`assign_worker_to_project`는 이미 배치된 워커를 다시 배치하면 `ON CONFLICT
(project_id, worker_id) DO NOTHING`(멱등)으로 처리해 재시도 안전성을 보장합니다.

## 5. 디스패치 프로토콜 확장 (`WorkerSelector`)

![Project-Aware Dispatch Logic](../assets/diagrams/architecture/project-aware-dispatch-logic.mermaid)

기존 파이프라인(`crates/fleet-scheduler/src/selector.rs`, §1 조사 결과 참고)에
**5.5단계**로 프로젝트 소프트 선호 필터를 삽입합니다 — 회로차단기/용량 필터
(4~5단계, `retain()`으로 후보를 제거) 뒤, `server_hint` 처리(6단계) 앞입니다:

- `task.project_id`가 `None`이면 이 단계는 완전히 스킵(기존 동작과 100% 동일 —
  회귀 없음).
- `Some(project_id)`이면 `list_project_worker_ids(project_id)`로 배치된 워커
  ID 집합을 조회하고, 현재 후보 풀과의 교집합을 계산합니다.
  - 교집합이 비어있지 않으면 후보 풀을 그 교집합으로 좁힙니다(§2에서 확정한
    "소프트 선호").
  - 교집합이 비어있으면(배치된 워커가 없거나 전부 온라인/용량/회로 필터에서
    걸러짐) **후보 풀을 그대로 유지**합니다 — 에러를 내지 않고 전체 풀로
    폴백합니다(§2에서 확정한 "소프트 폴백").
- `server_hint`는 이 필터와 무관하게 항상 그대로 존중됩니다 — 힌트는 명시적
  단일 워커 지정이라 소프트 선호보다 우선순위가 높습니다. 즉 `project_id`와
  `server_hint`를 동시에 지정했는데 힌트 워커가 그 프로젝트에 배치돼 있지
  않아도 힌트가 이깁니다(모순처럼 보일 수 있으나, "소프트" 설계 원칙과 일관됨
  — 프로젝트는 자원 격리를 강제하지 않으므로 더 명시적인 신호인 힌트를 막을
  이유가 없음).

## 6. RBAC 권한 추가

기존 `PermissionKind`(콜론 구분 `resource:action` 네이밍, §1 조사 결과)에 맞춰
4개 변형을 추가합니다:

| 변형 | 직렬화 이름 | 의미 |
|---|---|---|
| `ProjectCreate` | `project:create` | 프로젝트 생성 |
| `ProjectRead` | `project:read` | 프로젝트 목록/상세 조회 |
| `ProjectDelete` | `project:delete` | 프로젝트 삭제 |
| `ProjectAssign` | `project:assign` | host/worker 배치·해제(양쪽 다 이 하나로 커버 — 세분화가 필요해지면 `ProjectAssignHost`/`ProjectAssignWorker`로 쪼갤 수 있으나, 1단계에서는 과설계로 판단해 보류) |

`BuiltinRole` 매핑: `Admin`은 전부, `Operator`는 `ProjectRead`+`ProjectAssign`
(운영자가 배치 조정은 하되 프로젝트 생성/삭제는 admin 전용으로 유지 — 기존
`Operator`가 워커 등록/삭제는 못 하지만 태스크 관리는 하는 것과 동일한 결의
패턴), `Viewer`는 `ProjectRead`만.

## 7. API 표면

### 대시보드 REST (`/api/projects/*`, 기존 `/<resource>` + `/api/<resource>` 페어링 관례)

| Method | Path | 권한 | 설명 |
|---|---|---|---|
| GET | `/projects` | 세션 | 프로젝트 목록 페이지 |
| GET | `/projects/:id` | 세션 | 프로젝트 상세 페이지 (배치된 host/worker + 최근 태스크) |
| GET | `/projects/new` | 세션 | 생성 폼 |
| GET | `/api/projects` | `ProjectRead` | 목록 JSON |
| POST | `/api/projects` | `ProjectCreate` | 생성 |
| GET | `/api/projects/:id` | `ProjectRead` | 상세 JSON |
| DELETE | `/api/projects/:id` | `ProjectDelete` | 삭제 |
| POST | `/api/projects/:id/workers` | `ProjectAssign` | 워커 배치 (`{worker_id}`) |
| DELETE | `/api/projects/:id/workers/:worker_id` | `ProjectAssign` | 워커 배치 해제 |
| POST | `/api/projects/:id/hosts` | `ProjectAssign` | 호스트 배치 (`{host_id}`) |
| DELETE | `/api/projects/:id/hosts/:host_id` | `ProjectAssign` | 호스트 배치 해제 |

### MCP 도구 (`fleet_*`, 기존 12개에 추가)

| 도구 | 입력 | 비고 |
|---|---|---|
| `fleet_create_project` | `name`(필수), `description` | |
| `fleet_list_projects` | `limit`, `offset` | |
| `fleet_delete_project` | `project_id`(필수) | |
| `fleet_assign_worker_to_project` | `project_id`, `worker_id`(둘 다 필수) | |
| `fleet_unassign_worker_from_project` | 위와 동일 | |

`fleet_dispatch_task`는 새 도구를 만들지 않고 기존 입력 스키마에 `project_id`
(선택) 필드만 추가합니다 — `server_hint`와 동일한 패턴.

`fleet_assign_host_to_project`/`unassign`은 1단계 MCP 범위에서는 제외합니다(호스트
프로비저닝 자체가 현재 대시보드 UI/SSH 흐름 중심이라 MCP로 조작할 실사용 시나리오가
약함 — 필요해지면 후속 추가). 대시보드 REST API에는 포함합니다(§7 표 참고).

## 8. 단계별 구현 계획

과거 `#38`/`#41`/`#42`와 동일하게, 스키마 → 백엔드 → 디스패치 통합 → UI 순으로
쪼갭니다. 각 단계는 독립적으로 커밋·검증·배포 가능해야 합니다.

1. **Phase 1 — 스키마 + Store + RBAC**: `015_projects.sql`, `fleet-core::Project`/`ProjectFilter`,
   `Store` 트레이트 확장(`PgStore` + `MemStore` 둘 다 구현), `PermissionKind` 4종
   추가. 신규 테스트: `fleet-store` 통합 테스트(assign/unassign 멱등성, M:N 배치,
   `ON DELETE CASCADE`/`SET NULL` 동작 검증 — 실제 Postgres 대상).
2. **Phase 2 — 디스패치 통합**: `WorkerSelector`에 5.5단계 삽입, `TaskRequest.project_id`
   추가, `fleet_dispatch_task` 입력 스키마 확장. 신규 테스트: `fleet-scheduler`
   단위 테스트(프로젝트 없음/소속 워커 있음/소속 워커 전부 불가 → 폴백 3가지
   경로), 기존 selector 테스트 전부 그린 유지(회귀 없음 확인).
3. **Phase 3 — API + MCP**: §7의 REST 엔드포인트 + MCP 도구 5종.
4. **Phase 4 — 대시보드 UI**: `/projects`, `/projects/:id`, `/projects/new` 페이지.
   기존 `tasks.html`/`hosts.html` 패턴(테이블 + 정렬, `#14` 참고) 재사용.

## 9. 열린 질문 (구현 중 재검토 가능)

- **하드 격리 옵션**: 소프트 폴백이 부적절한 운영 환경(예: 고객사별 프로젝트
  격리가 계약상 필수인 경우)이 생기면, 프로젝트 자체에 `strict_isolation: bool`
  플래그를 추가해 선택적으로 하드 실패(§2에서 보류한 옵션)로 전환하는 안을 고려.
  1단계에서는 구현하지 않음.
- **워커별 프로젝트 우선순위/쿼터**: 현재는 최소부하 선택에 암묵적으로 의존.
  실사용 중 특정 프로젝트가 항상 밀리는 문제가 관측되면 가중치 기반 선택으로
  재검토.
- **project_id 없는 태스크의 취급**: 계속 완전히 허용(모든 워커 후보). 프로젝트
  도입이 기존 미배정 태스크 흐름을 전혀 바꾸지 않음.

## 관련 문서

- [`docs/roadmap/roadmap.md`](../roadmap/roadmap.md) #48 — 구현 진행 상황 정본.
- [`docs/architecture/overview.md`](overview.md) §데이터 모델 — 구현 완료 후 이
  문서의 요약을 그쪽에도 반영.
