---
type: architecture-decision
authority: canonical
implementation: partial
verification: design-reviewed
source: "docs/architecture/agents/agent-template.md"
last_verified: "2026-08-29"
last_verified_commit: "working-tree"
owners: ["agent-platform", "security", "architecture"]
---

# AgentTemplate (custom prompt) 계약

## 결정

Agent의 역할 prompt, harness/skill 집합, tool allow-list, runtime 선택을 **재사용 가능한 템플릿**으로
분리한다. 템플릿은 두 계층이다.

- `agent_templates` — 정체성. 이름, 소유 Project(또는 global), 수명 상태. 무상태에 가깝다.
- `agent_template_revisions` — **불변 본문**. 한 번 `Published`가 되면 내용이 바뀌지 않는다.

한 계층으로 합치면 "revision immutability"를 담을 곳이 없다. 이름 변경 같은 메타데이터 수정이 본문
revision을 올리면 [재현 가능한 Skill loading](harness-composition.md)(`#65`)의 재현성이 깨져 보인다.

## 참조 방식

템플릿 참조는 **`(template_id, content_revision, content_hash)` 3튜플**이다.

- **content hash 단독을 기각한다.** hash에는 revoke 상태를 붙일 수 없고, 내용이 같은 두 Project의
  템플릿을 하나로 병합해 **인가 경계를 병합**한다.
- **id+revision 단독도 기각한다.** 저장소가 손상되거나 이관될 때 본문이 그대로임을 증명할 수단이 없다.
- hash는 필수 동반 증인이다. 같은 내용을 재발행하면 새 revision id를 받되 `content_hash`는 같다.

## 실행 중 Task에 대한 불변식

**실행 중 Task는 어떤 revision 전이에도 영향받지 않는다.** Task는 revision을 *참조*하는 것이
아니라 제출 시점에 본문과 hash를 **materialize**한다. 참조만 두면 retention purge가 재현을 깨뜨린다.

| 주체 | `Deprecated` 전이 | `Retired` 전이 |
|---|---|---|
| 실행 중 Task | 영향 없음 | 영향 없음 |
| WarmIdle Agent | process 유지, 경고 metric | `Hibernated`로 evict (reconciler의 기존 "WarmIdle drain" 권한 내) |
| Hibernated Agent | 기동 가능 | admission 즉시 거절 — `FailureKind::TemplateUnavailable`. 다른 revision fallback 없음 |
| Project `default_agent_template_id` | 변화 없음 | **Project 상태를 바꾸지 않는다** — 템플릿 관리자가 Project를 벽돌로 만들 수 있으면 안 된다 |

Agent는 기본적으로 revision을 **pin**한다(`template_upgrade_policy: pinned`). WarmIdle 재사용
호환성 키에 `agent_template_revision_id`가 포함되므로, revision이 다르면 재사용하지 않고
`WarmIdle → Hibernated → Starting`을 거친다.

`Retired`에는 선행 조건이 있다: dependent set 해시를 함께 제출해야 하며, 그 사이 의존 집합이
바뀌었으면 `409`로 거절한다.

## 권한 상승 차단 (핵심 보안 불변식)

**템플릿 편집 권한이 tool 부여 권한이 되어서는 안 된다.** 그렇지 않으면
`agent_template:update`가 `agent:manage`를 우회하며, 이는 [Project 기능 설계](../project-feature-design.md)가
구현을 차단한 것과 같은 구조의 우회로다. (`agent:manage`는 `#49` 1단계에서 실제로 만들어졌다 —
이 문서가 작성될 당시의 `AgentCreate`는 존재하지 않는 이름이었다.)

실효 tool 집합의 계산은 교집합과 차집합만 사용한다.

```
tools_effective = tools_template ∩ allow_project \ deny_project
```

연산이 `∩`과 `\`뿐이므로 `tools_effective ⊆ allow_project`가 구조적으로 성립한다. 템플릿에 무엇을
써넣어도 Project가 허용하지 않은 tool은 나오지 않는다.

**판정 시점은 Task admission이다.** 저장 시점 검증은 보조일 뿐 정본이 아니다 — 저장 시 통과해도
이후 Project grant가 좁아지면 저장 시점 결과는 무효다.

격리는 집합이 아니라 **전순서**다. 템플릿은 Project가 정한 floor 이상만 선택할 수 있고 `min()`
연산을 쓰지 않는다. `required`로 선언된 skill이 deny되면 조용히 축소하지 않고 **시작을 거절**한다.

`role_prompt`는 **데이터이지 권한이 아니다.** 어떤 capability 판정 입력에도 들어가지 않는다.
템플릿 본문에는 credential을 넣을 수 없고 참조만 허용한다.

## Capability

| serde 이름 | 의미 |
|---|---|
| `agent_template:read` | 목록·상세·revision 이력 조회 |
| `agent_template:create` | 새 템플릿 정체성 생성 |
| `agent_template:update` | 메타데이터 수정과 새 revision 생성 |
| `agent_template:lifecycle` | 수명 주기 전이(publish·deprecate·retire·discard) |
| `agent_template:revision_revoke` | 특정 revision의 신규 pin 금지 |
| `agent_template:manage_global` | `project_id IS NULL` 템플릿 관리 |

[UI 설계](../../ui-dashboard/ui-design.md)가 쓰는 단일 `AgentTemplateManage`는 채택하지 않는다.
`#66`(LLM credential read/export/manage 분리)과 `#72`(`admin_token:manage`/`list` 분리)의 선례를
따른다. `agent:*`(Agent 엔티티) capability와 이름이 겹치지 않게 한다.

### 편집 권한의 필드별 게이팅 (2026-08-22 결정)

`agent_template:update` 하나로 모든 편집을 다루지 않는다. **무엇을 바꾸는지에 따라 추가 권한을
요구한다.**

| 바뀌는 필드 | 요구 권한 |
|---|---|
| `role_prompt`, 이름·설명 등 메타데이터 | `agent_template:update` |
| `tools`, `skills`, `isolation_class` 선택 | `agent_template:update` **+ Agent tool-binding 권한**(`AgentManage` 상당) |

근거는 세 가지다.

1. **tool 상승은 이미 정본상 불가능하다.** [배치·맥락 계약](../entity-placement-and-context.md)의
   우선순위 사슬(`catalog → Project grant → Agent template(subset only) → Task request → snapshot`)과
   "Project deny 또는 capability 부족은 Agent template으로 다시 허용할 수 없다"가 이미 확정돼 있다.
   따라서 이 게이팅의 목적은 상승 차단이 아니다.
2. **그러나 [도구 카탈로그](tool-catalog.md)가 "tool binding 변경은 `AgentManage` 권한과 Project
   범위 검사를 요구한다"고 이미 정한다.** 템플릿의 tool 집합이 Agent tool binding의 출처이므로,
   tool 필드를 바꾸는 편집에 그 권한을 요구하지 않으면 기존 정본과 충돌한다.
3. **[Project 기능 설계](../project-feature-design.md)의 구현 차단 조건은 이 경우에 적용하지
   않는다.** 그 차단은 "Project 정책 변경과 Task 생성이 **자동 Agent provisioning을 통해**
   Agent 생성 권한(당시 문언은 `AgentCreate`, 현재 이름은 `agent:manage`)을 우회"하는 경로를
   겨냥한다. 템플릿 편집은 Agent를 만들지 않으므로 다른
   메커니즘이며, 같은 차단을 적용하면 과잉이다.

남는 위험은 prompt authorship — 허용된 tool 범위 안에서 행동을 지시하는 힘 — 이다. 이는
`TaskCreate` 보유자가 이미 갖는 힘과 같은 종류이되, 템플릿은 **지속적이고 다른 사람의 Task에도
적용된다**는 점에서 비대칭이다. 그래서 별도 capability로 두되 tool 변경보다는 낮은 문턱에 둔다.

### 기본 역할 배정

| 역할 | 부여 |
|---|---|
| `admin` | 전부 (`BuiltinRole::Admin`이 `PermissionKind::all()`이며 `builtin_roles_cover_all_permissions` 테스트가 이를 강제하므로 자동) |
| `operator` | `agent_template:read` + `agent_template:update`. tool-binding 권한은 주지 않으므로 **실질적으로 prompt 편집만 가능**하다 |
| `viewer` | `agent_template:read` |

Operator에게 `update`를 주는 것은 운영 중 프롬프트 개선을 admin 없이 할 수 있게 하려는 것이며,
필드별 게이팅이 tool 선택 변경을 자동으로 막는다. `BuiltinRole::Operator`의 고정 목록에 두 항목을
추가해야 한다 — 추가하지 않으면 operator는 아무것도 받지 못한다.

**viewer의 `read`는 2026-08-29 구현에서 이 표를 고친 것이다.** 원안은 "없음"이었으나, viewer는
이미 `agent:read`를 갖고 Agent 상세에 `agent_template_id`/`agent_template_revision_id`가 노출된다.
template read가 없으면 viewer는 **풀 수 없는 참조**를 보게 된다 — 그 pin이 무엇을 뜻하는지
물어볼 방법이 없다. 템플릿 본문은 비밀이 아니며(비밀은 credential 쪽 capability가 따로 지킨다)
`agent:read`를 이미 준 이상 숨겨서 얻는 것이 없다. 필드별 게이팅이 지키려는 것은 **쓰기**이고,
그 성질은 read를 주더라도 유지된다.

## FK 정책

이 도메인의 FK에는 **`CASCADE`를 쓰지 않는다.** 전부 `RESTRICT` 또는 `SET NULL`이다. `#78`에서
worker 삭제 한 번이 두 개의 `CASCADE`를 타고 암호화된 LLM credential을 파괴한 사례가 있으므로,
FK마다 폭발 반경을 논증하지 않은 `CASCADE`는 허용하지 않는다.

## 구현 상태 (2026-08-29, 1단계)

정체성·revision·pin과 그 관리 표면이 랜딩했다. 코드 기준 정본은
`crates/fleet-core/src/agent_template.rs`, migration `029_agent_templates.sql`,
`Store`의 AgentTemplate 절, Dashboard `/api/agent-templates` 계열이다.

**표면은 Dashboard뿐이며 MCP에는 의도적으로 없다.** LLM이 직접 부르는 표면에 템플릿 편집
권한을 주면 Agent가 자기 role prompt와 도구 목록을 스스로 고칠 수 있고, 그것이 이 문서가
막으려는 권한 상승 경로다.

`agents.agent_template_id`/`agent_template_revision_id`를 **같은 커밋에** 만든 이유는
`027_agents.sql`이 남긴 유예를 갚기 위해서다. 그 마이그레이션은 "채울 주체가 없어 항상 NULL이
되는 컬럼은 만들지 않는다"며 이 컬럼을 미뤘고, `#86`이 그 주체다. 컬럼 없이 템플릿만 만들면
게이트 3(의존 집합 해시)의 의존 집합이 영원히 비어 그 시험이 공허해진다.

### 미룬 것

| 항목 | 왜 미뤘나 | 선행 |
|---|---|---|
| `isolation_class` | 격리 등급을 해석·집행할 주체가 없다. 지금 만들면 아무도 읽지 않는 컬럼이다 | `#52` |
| Project 정책과의 tool 교집합 | `projects`에 정책 컬럼이 하나도 없어 교집합의 한쪽 항이 없다 | `#48` |
| `projects.default_agent_template_id` | Agent를 자동으로 만드는 경로가 없어 기본값을 읽을 주체가 없다 | `#49` 2단계 |
| `builtin/default@1` 시드 삽입 | 본문(`AgentTemplateBody::builtin_default`)과 그 `content_hash`는 코어에 있고 테스트가 고정하지만, 행을 넣는 주체가 없다 | `#52` |
| retire 의존 집합에 Attempt pin 포함 | 1단계의 의존자는 Agent뿐이다. Attempt가 revision을 materialize하면 종류가 는다 | `#87` |
| 템플릿 정체성 편집(`PATCH`) | 이름·설명 수정은 revision 이력에 남지 않아 감사에서 본문 변경과 구분되지 않는다. 동시 편집 의미와 함께 설계한다 | — |

## 구현 게이트

1. `Draft/Published/Deprecated/Retired/Discarded` 전이와 `Retired → Published` 간선 부재
2. published revision의 content 변경 시도가 거절되고, 같은 content 재발행이 새 revision id에 같은
   `content_hash`를 만드는 시험
3. retire가 dependent set 해시 없이 실패하고 집합 변화 시 `409`
4. 실행 중 revision retire가 Task의 harness manifest hash를 바꾸지 않는 E2E
5. `Retired` pin의 `Hibernated → Starting`이 `TemplateUnavailable`로 admission 즉시 거절되는 시험
6. 템플릿이 Project grant를 넘는 tool을 부여하지 못하며, 저장 후 Project grant가 좁아진 경우에도
   admission에서 차단되는 시험
6b. **필드별 게이팅**: `agent_template:update`만 가진 principal이 `role_prompt`는 바꿀 수 있고
   `tools`/`skills`/`isolation_class` 변경은 `403`으로 거절되는 시험. operator 역할이 실제로
   prompt만 편집 가능함을 역할 번들 기준으로 검증
7. `builtin/default@1` 시드가 MemStore/PgStore에서 같은 `content_hash`이고 tool binding이
   `ReadOnly` 등급으로 한정됨
8. MemStore/PgStore 공유 행동 테스트 (`#78`의 교훈)

### 1단계에서 도달한 게이트

| 게이트 | 상태 | 증적 |
|---|---|---|
| 1 전이표 | 통과 | `agent_template.rs`의 `transition_table_matches_canon`·`retired_is_terminal_and_never_returns_to_published`, `tests/agent_templates.rs::gate1` |
| 2 revision immutability | 통과 | `tests/agent_templates.rs::gate2` — 본문을 바꾸는 Store 메서드가 **존재하지 않는 것**이 집행 수단이다 |
| 3 dependent set 해시 | 통과 | `tests/agent_templates.rs::gate3` |
| 6b 필드별 게이팅 | 통과(권한 판정 절반) | `required_permissions_for_change`의 단위 테스트 3건 + Dashboard revision 생성 핸들러 |
| 8 두 store 공유 행동 | 통과 | 시나리오 5개를 MemStore/PgStore에 **같은 함수로** 건다 |
| 4 실행 중 retire와 manifest hash | 미도달 | 실행 중인 Agent 프로세스가 없다(`#89`) |
| 5 `TemplateUnavailable` admission 거절 | 미도달 | 같은 이유. `FailureKind`에 그 variant를 미리 만들지 않았다 |
| 6 Project grant 교집합 | 미도달 | `projects`에 정책 컬럼이 없다(`#48`) |
| 7 `builtin/default@1` 시드와 `ReadOnly` 등급 | 부분 | 본문 해시는 고정했으나 시드 행이 없고, `ReadOnly` 등급을 나타내는 타입 자체가 코드에 없다(`tool-catalog.md`는 정본이나 소유 로드맵 항목이 없다) |

## 관련 문서

- [Harness composition](harness-composition.md) — skill 합성과 revision 고정(`#65`)
- [Agent provisioning](provisioning.md) — Agent 상태와 admission
- [Entity placement & context](../entity-placement-and-context.md) — WarmIdle 호환성 키
- [Authorization·Project Scope·감사](../../security/authorization-and-audit.md) — capability 카탈로그
- [Project 기능 설계](../project-feature-design.md) — `default_agent_template_id`와 정책 revision
