---
type: review
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/reviews/ui-management-and-issue-spec-2026-08-22.md"
last_verified: "2026-08-22"
owners: ["architecture", "agent-platform", "security"]
---

# UI 관리 대상과 Issue 추적 명세 설계 (2026-08-22)

> 목표: UI에서 관리해야 할 대상(host, project, task, agent, agent_template)을 로드맵에 기록하고
> Issue 관리 기능을 추가한다.
>
> 체계: Main Architect가 기능을 분리하고 Protocol Specialist·Workflow Specialist가 병렬로 상세
> 설계. 두 전문가 모두 초안을 비준하지 않고 정정했다.

## 1. 목표 재해석

사용자가 제시한 목록은 균질하지 않았다.

| 대상 | 실제 상태 | 이 설계의 조치 |
|---|---|---|
| host, worker, task | UI 구현됨 | 없음 |
| project, agent | UI 설계됨·미구현 (`#48`, `#49`) | 중복 등록하지 않음 |
| agent_template | UI 라우트·필드·capability가 여러 문서에 흩어져 있으나 **소유하는 정본도 ID도 없음** | 정본 신설 + `#86`·`#87` |
| **issue** | 저장소에 개념 없음 | 정본 신설 + `#88`~`#91` |

따라서 산출물은 "7개 화면"이 아니라 **agent_template 정본화**, **Issue 추적 신설**, 그리고 둘을
안전하게 노출하기 위한 **관리 표면 공통 계약**이다.

## 2. 초안에 대한 정정 (두 전문가 공통)

| # | 초안 주장 | 판정 | 근거 |
|---|---|---|---|
| 1 | "Agent가 Issue를 연다" — Agent를 API 호출 주체로 상정 | 틀림 | AgentProcess는 control plane principal이 아니다. Worker control stream 보고 → control plane 대리 생성으로 대체 |
| 2 | "모듈 A는 신규 엔티티의 계약" | 불충분 | `#73`의 fail-closed는 `/v1`에만 설계됐다. Dashboard `/api`에는 중앙 행렬 자체가 없다 |
| 3 | "열린 Issue가 archive를 막는가"를 열린 질문으로 방치 | 위험 | Agent 생성 Issue 하나로 Project archive를 무기한 교착시킬 수 있다 |
| 4 | "revision immutability"만 언급, 참조 방식 미결 | 미완 | `(template_id, content_revision, content_hash)` 3튜플로 확정 |
| 5 | "템플릿이 Project 미허용 tool을 부여할 수 없다" | 방향은 맞으나 시점 미지정 | 저장 시점 검증은 Project grant가 나중에 좁아진 경우를 못 막는다. Attempt admission 시점 교집합이 정본 |

두 전문가가 독립적으로 추가한 구조 정정:

- **템플릿을 2계층으로 분리**(`agent_templates` + `agent_template_revisions`). 한 계층이면
  revision immutability를 담을 곳이 없고, 이름 변경이 phantom revision을 만들어 `#65` 재현성이
  깨져 보인다.
- **Attempt는 revision을 참조하지 않고 materialize**한다. 참조만 두면 retention purge가 재현을 깨뜨린다.
- **`InProgress` Issue 상태를 두지 않는다.** 비터미널 연관 Task로부터 유도 가능하며, 상태로 승격하면
  Task 상태를 복제해 두 상태 머신 경쟁이 시작된다.

## 3. 두 전문가가 합의한 것

겉보기에 갈렸던 한 지점이 실제로는 같은 설계였다. Protocol은 "Agent는 principal이 아니므로 Worker
control stream 경유", Workflow는 "`worker:self` + `attempt_id` + fencing token으로 인증"이라고
썼는데, 둘 다 **워커가 보고하고 control plane이 대리 생성하며 `project_id`는 저장된 Attempt 행에서
유도한다**는 동일한 메커니즘이다.

그 밖의 합의:

- Issue↔Task는 join 테이블 연관이며 `tasks`에 `issue_id`를 넣지 않는다
- 열린 Issue는 archive를 막지 않는다 — `blocks_archive` 플래그와 별도 capability만 hold를 만든다
- 단일 `AgentTemplateManage`를 기각하고 read/create/update/archive/revoke로 분리(`#66`·`#72` 선례)
- 모듈 B의 FK에는 `CASCADE`를 쓰지 않는다 — `#78`이 CASCADE 한 쌍으로 암호화 credential을 파괴한
  직후이므로, 폭발 반경을 논증하지 않은 CASCADE는 허용하지 않는다
- Agent는 `[*] → Open`과 append만 가능하다. Issue를 닫을 수 있는 Agent는 자기 실패를 은폐한다

## 4. 이 설계가 새로 발견한 결함

로드맵 밖에서 발견됐고 조정자가 코드로 재확인했다.

| # | 결함 | 확인 |
|---|---|---|
| D1 | **Dashboard에 중앙 capability 행렬이 없다.** `crates/fleet-dashboard/src/app.rs`에 `required_capability` 부재, 핸들러에 `PermissionKind` 검사 29곳 산재. `#73`은 `/v1`만 고치는데 `#86`~`#92`의 관리 화면은 대부분 이 표면에 놓인다 | `#73` 범위에 반영함 |
| D2 | MCP 표면도 동형 | `#92` 게이트에 포함 |
| D3 | **`ApiError`에 422/428/429가 없다.** `BadRequest/Unauthorized/Forbidden/NotFound/Conflict/Store/Internal`뿐이라 낙관적 동시성(`If-Match` → 428)과 rate limit(429)을 표현할 수 없다 | `#92` 선행 조건 |

## 5. 등록된 로드맵 항목

`#86`(AgentTemplate 정본) · `#87`(Attempt snapshot 고정) · `#88`(Issue 엔티티) ·
`#89`(Agent 보고 경로와 폭주 방지) · `#90`(dead-letter 집계 자동 생성) · `#91`(archive hold 승격) ·
`#92`(관리 표면 노출).

순서 근거는 로드맵 절 서두에 있다. 요지: `#86`은 `#87`의 전제(존재하지 않는 것을 snapshot할 수
없다), `#88`은 `#89`의 전제, `#89`가 `#90`보다 먼저인 이유는 dead-letter 자동 생성이 `#89`가
만드는 dedup/budget 기구를 재사용하기 때문이다 — 뒤집으면 같은 기구를 두 번 만든다.

## 6. 사람이 결정해야 하는 항목

두 전문가가 추측하지 않고 남긴 것들이다. 권고를 붙였으나 승인이 필요하다.

| # | 결정 | 권고 |
|---|---|---|
| H1 | `agent_template:update`와 `AgentCreate`/`project:policy_manage`의 관계 | **결정됨(2026-08-22) — 필드별 게이팅.** 아래 §8 참조 |
| H2 | dead-letter 자동 Issue 생성의 kind별 기본값 | `CredentialMissing` 즉시 ON, `WorkerUnavailable` hysteresis 후 ON, 일반 `WorkerError` OFF, `Cancelled` never |
| H3 | fleet scope Issue(`project_id = NULL`)가 존재하는가 | 존재한다. `tasks.project_id` 관례를 미러링하고 `issue:read_fleet` 신설 |
| H4 | AutomationService(CI 등)에 `issue:close` 부여 | 부여하지 않음 — 자동 close는 "Task 성공이 Issue를 닫지 않는다"를 우회하는 가장 쉬운 길 |
| H5 | fenced/stale Attempt의 Issue 보고 수락 여부 | 수락하되 `provenance=stale_attempt` 표시. fencing 목적과 부딪히므로 보안 검토 필요 |
| H6 | security/legal 라벨 Issue의 archive hold 자동 승격 | 기본 ON, Project override 가능 |
| H7 | `builtin/default@1` 시드의 tool binding | `ReadOnly` 등급 한정 — 시드가 곧 미구성 배포의 기본 권한선이다 |
| H8 | `Closed(stale)` 자동 close | 기능은 두되 기본 OFF. 켜면 digest 필수 — 자동 close 자체가 우리가 막으려는 조용한 유실의 한 형태 |

## 7. 기존 정본에 필요한 변경

`agent.md` §5에 따라 범위가 바뀌면 설계 정본을 먼저 갱신한다. 이 명세가 요구하는 것은 다음뿐이며,
**Task/Attempt 상태 머신은 한 글자도 바뀌지 않는다** — 그것이 교착 없음 증명의 전제이자 결과다.

| 정본 | 변경 | 성격 |
|---|---|---|
| [Entity placement](../architecture/entity-placement-and-context.md) | WarmIdle 호환성 키에 `agent_template_revision_id` 추가 | 1줄 |
| [Agent provisioning](../architecture/agents/provisioning.md) | `Retired` revision의 admission 거절과 WarmIdle evict 사유 | 기존 목록 확장 |
| [관측성·재조정](../architecture/observability-and-reconciliation.md) | authority table에 Issue 관련 행 추가 | 표 확장 |
| [Lifecycle](../architecture/project-task-agent-lifecycle.md) | archive hold `kind`에 `'issue'` 추가 | 기존 메커니즘의 새 kind |
| `crates/fleet-core/src/task.rs` | `FailureKind::TemplateUnavailable` 추가 | `CredentialMissing` 선례와 동형 |
| [UI 설계](../ui-dashboard/ui-design.md) | 단일 `AgentTemplateManage`를 분리된 capability 집합으로 갱신 | 카탈로그 정합 |

위 변경은 `#86`~`#92` 착수 시점에 해당 항목과 함께 반영한다. 이 검토 시점에는 신규 정본 2건
([AgentTemplate](../architecture/agents/agent-template.md), [Issue 추적](../architecture/issues.md))만
추가했다.

## 8. H1 결정 (2026-08-22)

자료를 확인한 결과 문제의 성격이 처음 제기됐을 때와 달랐다.

- **tool 권한 상승은 이미 정본상 불가능하다.** [배치·맥락 계약](../architecture/entity-placement-and-context.md)의
  우선순위 사슬과 "Project deny는 Agent template으로 다시 허용할 수 없다"가 이미 canonical이다.
  Protocol Specialist가 "핵심 보안 불변식"으로 제시한 상승 차단 정리는 새 제안이 아니라 기존 정본의
  재확인이었다.
- **`#48`의 차단 조건은 다른 메커니즘을 겨냥한다.** "자동 Agent provisioning을 통한 `AgentCreate`
  우회"이며, 템플릿 편집은 Agent를 만들지 않는다. 같은 차단을 적용하면 과잉이고 `#86`이 무기한
  지연된다.
- **그러나 [도구 카탈로그](../architecture/agents/tool-catalog.md)는 "tool binding 변경은
  `AgentManage` 권한"을 이미 정하고 있다.** 템플릿의 tool 집합이 그 출처이므로 무시할 수 없다.
- 남는 실질 위험은 prompt authorship이며, 이는 `TaskCreate` 보유자가 이미 갖는 힘과 같은 종류다.
  다만 템플릿은 지속적이고 다른 사람의 Task에도 적용된다는 비대칭이 있다.

**결정 1 — 필드별 게이팅.** `role_prompt`·메타데이터 편집은 `agent_template:update`,
`tools`/`skills`/`isolation_class` 편집은 거기에 Agent tool-binding 권한을 추가로 요구한다.
정본 충돌 없이 `#86`을 `#48` 승인 없이 진행할 수 있다.

**결정 2 — Operator는 `read` + `update`.** tool-binding 권한을 주지 않으므로 실질적으로 prompt
편집만 가능하다. `admin`은 `PermissionKind::all()`로 자동 보유하며
`builtin_roles_cover_all_permissions` 테스트가 이를 강제한다. `BuiltinRole::Operator`의 고정
목록에 두 항목을 추가해야 하며, 추가하지 않으면 operator는 아무것도 받지 못한다.

상세는 [AgentTemplate 계약](../architecture/agents/agent-template.md)의 "편집 권한의 필드별
게이팅"과 "기본 역할 배정"이 소유한다.

## 9. 프레임 정정과 H2 (2026-08-23)

### 정정 — Issue는 이슈 트래커다

§1~§8은 Issue를 **orchestrator의 인프라 장애 추적**으로 다뤘다. 사용자 의도는
**프로젝트가 해결해야 할 일감을 관리하는 이슈 트래커**였다. 조정자가 목표를 잘못 해석한 채
서브 에이전트에게 위임했고, 두 전문가는 주어진 프레임 안에서 정확히 작업했다.

정정이 바꾸지 않는 것 — 두 전문가가 도출한 핵심 경계는 이슈 트래커에도 그대로 유효하다.
Issue/Task는 부모-자식이 아닌 연관, Task 성공이 Issue를 닫지 않음, `InProgress` 부재,
교착 없음 불변식 I1·I2, Task/Attempt 상태 머신 불변.

정정이 바꾸는 것:

| 요소 | 조치 |
|---|---|
| `#90` dead-letter → Issue 자동 생성 | **취소.** 인프라 장애는 alert이지 프로젝트 일감이 아니다 |
| kind별 관측 요구 | `#70`이 흡수 |
| Agent 보고 경로(`#89`) | 유지하되 의미 변경 — "조용한 실패 방지"에서 "발견한 일감 등록"으로 |
| Agent 착수 | **신규 `#93`** — 아래 참조 |

### H2 조사에서 나온 사실

명세가 제시한 kind별 기본값 표는 실제 코드와 어긋나 있었다.

- `reconcile.rs`의 dead-letter 경로는 `CredentialMissing`과 `WorkerUnavailable` 두 kind만 붙인다.
  `WorkerError`·`CircuitOpen`은 실행·디스패치 시점 실패라 이 경로를 거치지 않는다.
- `FailureKind`의 **`Timeout`·`AuthFailed`·`Cancelled` 세 variant는 저장소 어디서도 생성되지
  않는다** — 죽은 코드다. 명세는 유령 범주에 정책을 붙이고 있었다.
- 명세가 제안한 "15분 hysteresis"는 대부분 중복이다. `interval=30s`, `stale_after=60s`,
  `max_dispatch_retries=20`이라 dead-letter까지 최소 약 11분이 걸린다.
- `/metrics`에는 `phase=failed` 집계만 있고 kind별 분해가 없다.

`#90`이 취소되면서 kind별 기본값 결정은 소멸했고, 위 관측 결함과 죽은 variant 정리는 `#70`으로
옮겼다.

### 결정 (2026-08-23)

**결정 3 — Agent는 Issue를 읽고 착수한다.** 백로그에서 자동 선택한다. 이것이 설계에 새로 들여오는
문제는 인가, 동시성, 예산이다.

- **인가는 상태가 소유한다.** `Triaged → ReadyForAgent` 전이는 사람만 하며 신설
  `issue:approve_agent_work` capability를 요구한다. Agent가 스스로 연 Issue를 스스로 착수하려면
  반드시 사람의 승인 전이를 거친다. "누가 이 일을 승인했는가"가 Issue 이력에 남는다.
  [Project 기능 설계](../architecture/project-feature-design.md)가 자동 provisioning의
  `AgentCreate` 우회를 이유로 건 차단 조건과 같은 종류의 위험이므로 같은 방식으로 명시적 승인
  지점을 둔다.
- **claim은 CAS + 만료 lease**다. `#62`의 lease 관례를 재사용하고 새 기구를 만들지 않는다. claim은
  Issue 상태를 바꾸지 않는다 — 상태를 하나 더 만들면 `InProgress`를 금지한 것과 같은 문제가 된다.
- **Project는 claim 예산을 갖는다.** 예산이 없으면 무한 생성-소비 루프가 성립한다.

**결정 4 — `#90` 취소.** 위 참조.

### 미결 — roadmap과의 소유권 관계

`docs/roadmap/roadmap.md`가 영구 ID `#1`~`#93`을 마크다운으로 관리하고 있고, 이번 세션에서 그
파일이 동시 편집으로 **세 번 회귀**했다(`docs/log.md`의 2026-08-20 항목 두 건과 08-22 항목).
이슈 트래커가 그 원장을 대체하는 물건인지, 아니면 roadmap이 계획 정본으로 남고 트래커는 더 작은
실무 단위를 다루는지는 **양쪽을 설계해 비교한 뒤 결정**하기로 했다.

비교해야 할 축: 이주 비용, 마이그레이션 경로, 설계 문서와 ID의 결합(roadmap 행이 정본 링크와 완료
게이트를 담는 현재 구조), DB 장애 시 계획 가시성, 그리고 동시 편집 회귀가 실제로 해소되는지.
비교 설계는 [Roadmap 원장과 Issue Tracker의 소유권 비교](roadmap-vs-issue-tracker-2026-08-23.md)에서 수행했다 — 권고는 단계적 Model C이며 1단계는 분리 운영이다.
