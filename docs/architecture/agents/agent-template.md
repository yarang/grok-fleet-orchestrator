---
type: architecture-decision
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/agents/agent-template.md"
last_verified: "2026-08-22"
last_verified_commit: "411242c"
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

## 실행 중 Attempt에 대한 불변식

**실행 중 Attempt는 어떤 revision 전이에도 영향받지 않는다.** Attempt는 revision을 *참조*하는 것이
아니라 제출 시점에 본문과 hash를 **materialize**한다. 참조만 두면 retention purge가 재현을 깨뜨린다.

| 주체 | `Deprecated` 전이 | `Retired` 전이 |
|---|---|---|
| 실행 중 Attempt | 영향 없음 | 영향 없음 |
| WarmIdle Agent | process 유지, 경고 metric | `Hibernated`로 evict (reconciler의 기존 "WarmIdle drain" 권한 내) |
| Hibernated Agent | 기동 가능 | admission 즉시 거절 — `FailureKind::TemplateUnavailable`. 다른 revision fallback 없음, retry 예산 미소모 |
| Project `default_agent_template_id` | 변화 없음 | **Project 상태를 바꾸지 않는다** — 템플릿 관리자가 Project를 벽돌로 만들 수 있으면 안 된다 |

Agent는 기본적으로 revision을 **pin**한다(`template_upgrade_policy: pinned`). WarmIdle 재사용
호환성 키에 `agent_template_revision_id`가 포함되므로, revision이 다르면 재사용하지 않고
`WarmIdle → Hibernated → Starting`을 거친다.

`Retired`에는 선행 조건이 있다: dependent set 해시를 함께 제출해야 하며, 그 사이 의존 집합이
바뀌었으면 `409`로 거절한다.

## 권한 상승 차단 (핵심 보안 불변식)

**템플릿 편집 권한이 tool 부여 권한이 되어서는 안 된다.** 그렇지 않으면
`agent_template:update`가 `AgentCreate`를 우회하며, 이는 [Project 기능 설계](../project-feature-design.md)가
구현을 차단한 것과 같은 구조의 우회로다.

실효 tool 집합의 계산은 교집합과 차집합만 사용한다.

```
tools_effective = tools_template ∩ allow_project \ deny_project
```

연산이 `∩`과 `\`뿐이므로 `tools_effective ⊆ allow_project`가 구조적으로 성립한다. 템플릿에 무엇을
써넣어도 Project가 허용하지 않은 tool은 나오지 않는다.

**판정 시점은 Attempt admission이다.** 저장 시점 검증은 보조일 뿐 정본이 아니다 — 저장 시 통과해도
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
| `agent_template:archive` | 소프트 종료 |
| `agent_template:revision:revoke` | 특정 revision의 신규 pin 금지 |
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
   `AgentCreate`를 우회"하는 경로를 겨냥한다. 템플릿 편집은 Agent를 만들지 않으므로 다른
   메커니즘이며, 같은 차단을 적용하면 과잉이다.

남는 위험은 prompt authorship — 허용된 tool 범위 안에서 행동을 지시하는 힘 — 이다. 이는
`TaskCreate` 보유자가 이미 갖는 힘과 같은 종류이되, 템플릿은 **지속적이고 다른 사람의 Task에도
적용된다**는 점에서 비대칭이다. 그래서 별도 capability로 두되 tool 변경보다는 낮은 문턱에 둔다.

### 기본 역할 배정

| 역할 | 부여 |
|---|---|
| `admin` | 전부 (`BuiltinRole::Admin`이 `PermissionKind::all()`이며 `builtin_roles_cover_all_permissions` 테스트가 이를 강제하므로 자동) |
| `operator` | `agent_template:read` + `agent_template:update`. tool-binding 권한은 주지 않으므로 **실질적으로 prompt 편집만 가능**하다 |
| `viewer` | 없음 |

Operator에게 `update`를 주는 것은 운영 중 프롬프트 개선을 admin 없이 할 수 있게 하려는 것이며,
필드별 게이팅이 tool 선택 변경을 자동으로 막는다. `BuiltinRole::Operator`의 고정 목록에 두 항목을
추가해야 한다 — 추가하지 않으면 operator는 아무것도 받지 못한다.

## FK 정책

이 도메인의 FK에는 **`CASCADE`를 쓰지 않는다.** 전부 `RESTRICT` 또는 `SET NULL`이다. `#78`에서
worker 삭제 한 번이 두 개의 `CASCADE`를 타고 암호화된 LLM credential을 파괴한 사례가 있으므로,
FK마다 폭발 반경을 논증하지 않은 `CASCADE`는 허용하지 않는다.

## 구현 게이트

1. `Draft/Published/Deprecated/Retired/Discarded` 전이와 `Retired → Published` 간선 부재
2. published revision의 content 변경 시도가 거절되고, 같은 content 재발행이 새 revision id에 같은
   `content_hash`를 만드는 시험
3. retire가 dependent set 해시 없이 실패하고 집합 변화 시 `409`
4. 실행 중 revision retire가 Attempt harness manifest hash를 바꾸지 않는 E2E
5. `Retired` pin의 `Hibernated → Starting`이 `TemplateUnavailable`로 admission 즉시 거절되고 retry
   예산을 소모하지 않는 시험
6. 템플릿이 Project grant를 넘는 tool을 부여하지 못하며, 저장 후 Project grant가 좁아진 경우에도
   admission에서 차단되는 시험
6b. **필드별 게이팅**: `agent_template:update`만 가진 principal이 `role_prompt`는 바꿀 수 있고
   `tools`/`skills`/`isolation_class` 변경은 `403`으로 거절되는 시험. operator 역할이 실제로
   prompt만 편집 가능함을 역할 번들 기준으로 검증
7. `builtin/default@1` 시드가 MemStore/PgStore에서 같은 `content_hash`이고 tool binding이
   `ReadOnly` 등급으로 한정됨
8. MemStore/PgStore 공유 행동 테스트 (`#78`의 교훈)

## 관련 문서

- [Harness composition](harness-composition.md) — skill 합성과 revision 고정(`#65`)
- [Agent provisioning](provisioning.md) — Agent 상태와 admission
- [Entity placement & context](../entity-placement-and-context.md) — WarmIdle 호환성 키
- [Authorization·Project Scope·감사](../../security/authorization-and-audit.md) — capability 카탈로그
- [Project 기능 설계](../project-feature-design.md) — `default_agent_template_id`와 정책 revision
