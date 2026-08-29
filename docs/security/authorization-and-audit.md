---
type: security-architecture
authority: canonical
implementation: partial
verification: design-reviewed
source: "docs/security/authorization-and-audit.md"
last_verified: "2026-08-28"
last_verified_commit: "working-tree"
owners: ["security", "api-contracts", "agent-platform"]
---

# Authorization·Project Scope·감사 계약

## 결정

HTTP, Dashboard, MCP, Worker control은 같은 `AuthorizationContext`와 policy evaluator를 사용한다.
transport가 다르다고 권한 모델이 달라지지 않는다. 현재 Dashboard RBAC와 `/v1` bearer allow-list는
부분 구현일 뿐, 이 문서의 목표 계약을 보장하지 않는다.

```mermaid
flowchart LR
    Request["HTTP / Dashboard / MCP / Worker control"] --> Authn["authenticate principal"]
    Authn --> Context["AuthorizationContext\nprincipal · capabilities · scopes · auth time"]
    Context --> Policy["policy evaluation"]
    Policy --> Preconditions["resource scope · revision · lifecycle"]
    Preconditions --> Allow["execute + audit"]
    Policy --> Deny["deny + security audit"]
```

## Principal과 인증 수단

| Principal | 인증 수단 | 허용 경계 | 금지 |
|---|---|---|---|
| HumanUser | Dashboard session/OIDC, 민감 작업은 step-up MFA | 자신의 Project scope와 부여 capability | Worker·Security Manager 위장 |
| AutomationService | 짧은-lived workload credential 또는 mTLS | 명시 capability·Project scope·만료 | wildcard admin/무기한 bearer |
| Worker | `worker_id`에 결합된 mTLS certificate 또는 scoped credential | 자기 inventory, command ACK/result, 자기 grant 수령 | 다른 Worker/Project 제어, policy 변경 |
| AgentProcess | 일반 control-plane principal 없음 | Task의 실행 구간에 묶인 Security/privileged-helper grant | `/v1`, MCP, Dashboard의 일반 호출 |
| SecurityManager | 별도 service identity | credential metadata·grant·revoke·rotation workflow | Task/Project 정책 임의 변경 |
| BootstrapInstaller | one-time bootstrap token | join/enrollment 한 번 | 운영 API·credential 사용 |

Agent process는 control plane의 사용자로 취급하지 않는다. Agent가 필요한 privileged operation과
credential은 각각 실행 구간에 묶인 grant를 통해 helper/Security Manager에 요청하며, 그 grant가 Project
policy·fencing token·만료를 다시 확인한다. 여기서 실행 구간은 Task 행의 수명이 아니라 **dispatch부터
terminal까지**다 — 상세는 [제어면 보안 모델](control-plane-security-model.md)이 정본이다.

## AuthorizationContext와 평가 순서

모든 요청은 최소 `principal_id`, `principal_type`, `authentication_method`, `authenticated_at`,
`capabilities`, `project_scopes`, `request_id`, `policy_revision`을 가진다. mutation에는 canonical payload
hash와 idempotency key/request ID, resource revision 또는 expected generation을 추가한다.

평가 순서는 고정이다.

1. principal 인증·만료·revocation을 확인한다.
2. endpoint/tool의 capability와 인증 강도(step-up 필요 여부)를 확인한다.
3. URL/body가 아니라 저장된 resource 관계에서 Project scope를 해석한다.
4. lifecycle, policy revision, expected generation/fencing token precondition을 확인한다.
5. capability가 있어도 상위 Project deny·security hold·isolation policy가 있으면 거절한다.
6. 허용·거절·break-glass decision을 audit에 기록한다.

Project 밖 resource는 존재 여부를 숨기는 `404`를, scope 안이지만 capability가 없는 요청은 `403`을
반환한다. 인증 실패/만료는 `401`이다. 정책·revision·lifecycle precondition 불일치는 `409`이며,
입력 검증 오류는 `422`다.

## Capability와 scope

Capability는 역할명이 아니라 최소 동작 단위이며, Project scope는 capability에 더해지는 제약이다.
현재 `PermissionKind`는 Dashboard 중심의 부분 카탈로그다. 아래 목표 capability를 추가하되,
`*` wildcard와 Project scope 없는 write capability를 만들지 않는다.

| 영역 | capability | scope/추가 조건 |
|---|---|---|
| Project | `project:read`, `project:create`, `project:update`, `project:archive`, `project:policy_manage` | read/update/archive는 해당 Project; policy는 revision CAS |
| Task | `task:read`, `task:create`, `task:cancel`, `task:output`, `task:redrive` | Task의 저장된 Project scope; redrive는 effect disposition 확인 |
| Agent | `agent:read`, `agent:manage`, `agent:attach` | Agent immutable Project; attach는 step-up·짧은 grant |
| AgentTemplate | `agent_template:read`, `agent_template:create`, `agent_template:update`, `agent_template:lifecycle`, `agent_template:revision_revoke`, `agent_template:manage_global` | 편집은 **필드별 게이팅** — 도구/스킬 변경은 `agent:manage`를 추가 요구. `manage_global`은 `project_id IS NULL` 템플릿 |
| Worker/Fleet | `worker:read`, `worker:provision`, `worker:operate`, `worker:self` | `worker:self`은 mTLS subject와 동일 worker_id만 |
| Credential | `credential:read_metadata`, `credential:grant`, `credential:rotate`, `credential:revoke`, `credential:break_glass_export` | grant는 Project/Agent/Tool subset; export는 dual approval |
| Effect/archive | `effect:resolve`, `project:hold_manage` | `PartiallyApplied` evidence, reason, approval 필요 |
| Security | `audit:read`, `policy:manage`, `incident:manage` | global SecurityAdmin 또는 명시 delegate |

### 현재 랜딩된 `PermissionKind`와의 대응

위 표는 **목표 명명**이다. 실제 `crates/fleet-core/src/auth.rs`에 존재하는 값은 다음과 같으며,
새 capability를 추가할 때 기존 이름을 재사용하지 않는다(`#66`에서 `worker:delete`가
LLM credential 삭제를 흡수한 사고의 재발 방지).

| 목표 capability | 현재 `PermissionKind` | 비고 |
|---|---|---|
| `worker:read` | `worker:list` | 개별 조회(`GET /v1/workers/{id}`)는 행렬 미등록 |
| `worker:provision` | `host:provision` | `POST /v1/hosts/register`는 행렬 미등록 |
| `worker:operate` | **없음** | MCP `fleet_reset_worker_breaker`가 `worker:delete`를 요구해 최소 권한이 성립하지 않는다 |
| `worker:self` | `AuthorizationContext.worker_id` binding | capability가 아니라 binding 검사로 구현됨 |
| `credential:rotate`/`revoke` (operational) | `worker:credential:manage` | Worker operational identity 전용 |
| `credential:read_metadata` / (원문 열람) | `worker:llm_credential:read` / `:export` / `:manage` | export는 read·manage와 분리, 감사 실패 시 거절 |
| (admin bearer 관리) | `admin_token:manage`, `admin_token:list` | 목표 표에 대응 항목 없음 — 추가 필요 |
| (bootstrap token) | `token:issue`, `token:list`, `token:revoke` | 목표 표에 대응 항목 없음 — 추가 필요 |
| `audit:read` | `audit:read` | 일치 |
| `project:update`, `project:policy_manage`, `task:redrive`, `agent:*`, `effect:resolve` | **없음** | `project:read`/`create`/`delete`(archive 요청)는 `#48` 1단계로 랜딩했다. 나머지는 대상 엔티티·컬럼이 아직 없다 — 아래 승인 절 참고 |

기본 역할은 `ProjectViewer`, `ProjectContributor`, `ProjectOperator`, `ProjectManager`, `FleetOperator`,
`SecurityAdmin`으로 구성할 수 있으나 역할은 convenience bundle일 뿐 evaluator는 capability와 scope만
검사한다. Task 생성 권한이 Agent 생성·Project 정책 변경·credential grant 권한을 암묵적으로 주지 않는다.

### Project 정책 변경과 Agent 생성의 관계 (승인 2026-08-27)

[Project 기능 설계](../architecture/project-feature-design.md)가 "권한과 구현 차단 조건" 1번으로
남겨 둔 사람 결정이다. 승인 시점까지 제안 문언 자체가 없었다 — [Project
model review](../reviews/project-model-review-2026-08-17.md)는 "보안 모델과 구현 계획에서 결정한 뒤
정본 계약에 반영한다"고 미뤘을 뿐이다. 따라서 이 절이 승인된 규칙 자체를 소유하고, 설계·계약
문서는 여기를 참조한다.

**승인 범위.** 두 단계로 승인됐다. 1차에서 사용자가 승인한 것은 차단 조건 1이 **제기된 형태**
(`ProjectPolicyManage`와 `AgentCreate`의 관계를 보안 모델에서 확정한다)였고, 결정 1이 그 관계에
대한 직접적 답이다. 결정 2·3은 그것을 집행 가능한 규칙으로 구체화하며 이 문서가 작성한 것이므로
승인 범위 밖임을 명시하고 별도 확인을 요청했으며, 같은 날 문언 그대로 승인됐다. 따라서 세 결정
모두 승인된 규칙이고 이 절이 그 정본이다. 이후 변경은 새 승인을 받는다 — 무엇이 승인됐고 무엇이
파생인지 남겨 두는 이유가 그것이다.

**막는 것.** `project:policy_manage` 보유자가 Agent provisioning 관련 정책 필드
(`max_active_agents`, `max_warm_agents`, 기본 AgentTemplate, worker eligibility selector)를 바꾸면,
그 뒤 `task:create`만 가진 임의 principal의 Task 제출이 자동 provisioning을 통해 Agent를 만든다.
둘 중 누구도 Agent 생성 권한을 갖지 않은 채 Agent가 생긴다.

**결정 1 — 필드별 게이팅.** Agent의 **수 또는 provisioning 대상**을 바꾸는 정책 필드의 변경은
`project:policy_manage`에 더해 `agent:manage`를 요구한다. 나머지 정책 필드(`retention_policy_id`,
`retain_until` 등)는 `project:policy_manage`만으로 충분하다.
[AgentTemplate 계약](../architecture/agents/agent-template.md)의 H1 결정(2026-08-22)과 같은
형태다 — 위험한 것은 표면이 아니라 **어느 필드가 무엇을 만들 수 있는가**이므로, 표면 전체를 막는
대신 필드에 건다.

**결정 2 — 권한 확인 시점은 정책 쓰기다.** admission은 정책 한도를 **집행**하고 권한을
**판정하지 않는다**. Task 제출자에게 Agent 생성 권한을 요구하지 않는다 — 그는 이미 승인된 한도
안에서 Task를 낼 뿐이고, Agent를 만들 권한은 그 한도를 쓴 사람이 정책 쓰기 시점에 증명했다.
반대로 하면 Project 정책이 의미를 잃는다(모든 contributor가 `agent:manage`를 가져야 하고, 그러면
정책이 아니라 개별 권한이 상한을 정한다). "누가 이 Agent 생성을 승인했는가"는 Task가 아니라
정책 revision의 audit event가 답한다.

이것은 위 "Task 생성 권한이 Agent 생성 권한을 암묵적으로 주지 않는다"와 모순되지 않는다.
제출자는 `agent:manage`를 **얻지 않는다** — 상한을 올릴 수도, 템플릿·격리 class를 고를 수도, 다른
Project로 provisioning할 수도 없고, 한도 밖 제출은 admission이 거절한다. 일어나는 일은 암묵적 부여가
아니라 **한도로 감쇠된 사전 위임**이며, credential delivery grant를 발급자의 권한으로 미리 만들고
실행 구간에 묶는 것과 같은 형태다. 이 감쇠가 성립하지 않는 정책 필드가 나오면 그 필드는 결정 1의
`agent:manage` 추가 요구 대상이다.

**결정 3 — Project 메타데이터 편집은 이 관계와 무관하다.** `name`·`description` 변경은 Agent를
만들지 않으므로 `project:update`만 요구하며, 이 관계를 이유로 막지 않는다. H1이 "템플릿 편집은
Agent를 만들지 않는다"로 `#86`의 과잉 차단을 푼 것과 같은 판정이다.

**이름 정정.** 승인 요청 문언의 `AgentCreate`(UI 표기)와 `agent:create`([Project 관리
계약](../contracts/project-management.md) 표기)는 위 목표 capability 표에 없다. Agent 영역의 대응
capability는 `agent:manage`이며 이 절의 규칙은 그 이름에 묶인다. 존재하지 않는 이름에 규칙을 걸어
두면 구현 시점에 아무 이름에나 갖다 붙일 수 있고, 그것이 `#66`에서 `worker:delete`가 LLM
credential 삭제를 흡수한 경로다. [UI 설계](../ui-dashboard/ui-design.md)의
`AgentCreate`/`AgentDelete` 구분, 그리고 아직 열려 있는 차단 조건 2를 서술하며 같은 이름을 쓰는
[AgentTemplate 계약](../architecture/agents/agent-template.md)·[Issue 추적](../architecture/issues.md)의
표기는 Agent 엔티티가 랜딩할 때 이 표와 대조해 함께 정리한다 — 지금은 두 표기가 공존하므로
그때 한 번에 끊지 않으면 재발견될 드리프트다. **`#49` 1단계(2026-08-28)가 그 시점이며, 위
문서들의 `AgentCreate`/`agent:create` 표기를 `agent:manage`로 정리했다.** 코드가 만든 이름은
`agent:read`와 `agent:manage` 둘뿐이므로 이제 두 표기의 공존은 끝났다. 마찬가지로
`project:assign`도 이 표에 없으며, 그 대상이었던 host·worker의 Project 배정은 [공유 실행 풀
불변식](../architecture/project-feature-design.md)이 "Host와 Worker에는 `project_id`를 두지
않는다"로 이미 배제했다 — 승인 대기가 아니라 설계상 존재하지 않는다.

**구현 상태 (2026-08-28 갱신).** 승인 당시에는 두 capability 모두 만들지 않았다 —
`project:policy_manage`가 관리할 정책 필드가 `projects` 테이블에 하나도 없고(migration 022는
`id`/`name`/`description`/`created_by`/`status`/시각뿐), `agent:manage`의 대상인 Agent 엔티티도
없었기 때문이다. 검사할 대상이 없는 권한은 항상 통과하거나 아무도 쓰지 않는 죽은 권한이 된다 —
`issue:archive_hold_manage`를 만들지 않은 것과 같은 판정이다.

`#49` 1단계에서 **`agent:manage`(와 `agent:read`)는 만들어졌다.** Agent 엔티티가 생겨 검사할
대상이 존재하기 때문이다. 현재 `agent:manage`는 Agent 생성과 회수를 가리고, `agent:read`는
목록·조회를 가린다. `agent:manage`는 Admin 전용이고 `agent:read`는 Operator·Viewer 기본이다.

`project:policy_manage`는 **여전히 만들지 않았다.** `projects`에 정책 컬럼이 하나도 없다는 사실은
1단계에서 바뀌지 않았다. 따라서 게이트 9도 아직 시험할 수 없다 — 결정 1이 거는 "Agent 수·
provisioning 대상 정책 필드"가 존재하지 않으므로 "그 필드를 바꾸지 못한다"를 증명할 대상이 없다.
게이트 9가 발동하는 시점은 정책 컬럼이 랜딩할 때다.

1단계가 실제로 확정한 것은 결정 2의 **반대 방향** 하나다. Task 제출은 Agent를 만들지 않으므로
제출자에게 `agent:manage`를 요구하지 않는다는 규칙이 자명하게 성립한다 — 자동 provisioning 경로
자체가 없기 때문이다. 이는 규칙의 증명이 아니라 규칙이 아직 시험되지 않았다는 뜻으로 읽어야 한다.

## Transport 적용

| Transport | identity 전달 | 추가 규칙 |
|---|---|---|
| Dashboard `/api/*` | session/OIDC → AuthorizationContext | CSRF, session rotation, mutation idempotency |
| HTTP `/v1/*` | Worker mTLS 또는 AutomationService workload credential | public health/metrics도 deployment ACL; Worker self binding |
| MCP stdio | authenticated launcher가 발급한 짧은 session assertion 또는 local peer identity | stdio/environment 자체를 신뢰하지 않음; ToolContext에 context 주입 |
| Worker control stream | mTLS Worker identity + control epoch/fencing token | Worker는 자기 command/result만 ACK |
| Security Manager | service mTLS + 실행 구간에 묶인 delivery grant | 원문 export 대신 grant; break-glass만 예외 |

### 등록되지 않은 route·tool의 판정 (fail-closed 불변식)

**어떤 transport에서든 required capability가 등록되지 않은 route/tool은 deny한다.** 등록 누락은
"권한 검사 불필요"가 아니라 "아직 판정되지 않음"이며, 판정되지 않은 요청을 통과시키면 인증만
통과한 임의 principal이 그 경로를 호출한다.

**`#73`(2026-08-23)로 해소됐다.** `authorize_http_endpoint`는 이제 `required_capability`가
`None`을 반환하면 `403`을 반환한다. `/health`와 `POST /workers/join`(body의 bootstrap token이
자체 인증 수단)만 함수 안에서 명시적으로 허용한다. 과거 누락이었던 `GET /v1/workers/{id}`
(`worker:list`)와 `POST /v1/hosts/register`(`host:provision`)는 이 전환과 함께 행렬에 등록됐다.

`crates/fleet-api/src/app.rs`의 `capability_matrix_covers_router_routes`(router에 실제 등록된
모든 (method, path) 조합이 capability를 가짐을 확인)와
`authorize_http_endpoint_denies_by_default_for_any_unmatched_route`(임의의 미등록 조합이 항상
403임을 함수 수준에서 고정)가 같은 결함의 세 번째 재발을 막는다. 다만 두 테스트 모두 `build_app`의
route 목록을 손으로 병행 유지한다 — 새 route를 추가하면서 이 목록에 반영하지 않으면 커버리지
테스트는 통과하지만 실제로는 놓친다. axum Router의 런타임 route 열거로 이 목록 자체를 도출하는
것은 이 항목의 범위 밖으로 남긴다.

Dashboard `/api`와 MCP 표면에는 같은 불변식이 아직 없다 — `crates/fleet-dashboard`에는 중앙
capability 행렬이 없고 핸들러에 `PermissionKind` 검사가 산재해 있다. `#86`~`#93`의 관리 화면이
그 표면에 놓이므로 `#92`가 이를 다룬다.

### 매핑되지 않은 principal의 capability

**principal→capability 매핑이 없는 인증 주체에게 기본 capability를 부여하지 않는다.** 특히
write·export 계열(`worker:llm_credential:export`, `admin_token:manage`, `token:issue`,
`worker:delete`)은 명시 매핑 없이는 어떤 경로로도 부여되지 않는다.

**로드맵 `#74`(완료)** — 이 불변식은 이제 Cloudflare Access 전용 배포에도 적용된다.
`app.rs::cf_access_capabilities`는 `cf_principal_capabilities`가 `None`이어도 빈 `Vec`을
반환한다(과거에는 `PermissionKind::all()`을 반환했다 — CF Access 정책을 통과한 모든 사용자가
모든 워커의 LLM 프로바이더 API 키 원문 export와 admin token 발급 권한을 가졌던 실제 결함).
매핑 설정 경로도 `fleet-cli`에 생겼다 — `Command::Serve`의 `--cf-principal-capabilities`/
`FLEET_CF_PRINCIPAL_CAPABILITIES`(JSON 배열, `[{"email":...,"capabilities":[...]}]`)가
`AppState::with_cf_principal_capabilities`를 호출한다. `FLEET_CF_AUDIENCE`가 설정됐는데 이
매핑이 비어 있으면 `run_serve`가 non-loopback bind 거부와 같은 원칙으로 기동 자체를 거부한다
— fail-closed 상태로 조용히 기동해 모든 요청이 이유 없이 403을 받는 배포를 방치하지 않는다.

MCP의 tool 이름이나 사용자의 자연어 지시는 capability가 아니다. tool마다 required capability,
resource scope resolver, mutation precondition, audit event type을 등록해야 하며 등록되지 않은 tool은
fail-closed한다.

현재 초기 구현은 `FLEET_MCP_CAPABILITIES`를 launcher의 명시적 allow-list로 읽어, 허용된 도구만
`tools/list`와 `tools/call`에 노출한다. 값이 없거나 비어 있거나 알 수 없으면 stdio 서버는 기동하지
않는다. 이는 prompt/tool argument가 권한을 얻지 못하게 하는 첫 경계이며, 환경변수 자체를 신뢰할 수
있는 identity로 승격하지 않는다. 후속 구현은 local peer identity 또는 짧은-lived signed assertion을
`ToolContext`의 principal·Project scope·audit context로 전달해야 한다.

## Break-glass와 승인 분리

credential 원문 export, irreversible effect resolution, security hold 해제, emergency Worker isolate는
일반 admin capability만으로 즉시 실행하지 않는다.

- caller는 step-up 인증과 `reason`을 제공한다.
- policy가 요구하면 서로 다른 두 principal의 approval을 받아 short-lived approval grant를 만든다.
- grant는 대상 resource·operation·payload hash·expiry에 묶이며 재사용할 수 없다.
- 실행 뒤 outcome과 조회/보상 증거를 audit에 append한다.

긴급 상황에서 dual approval을 생략하는 policy는 `incident:manage`와 별도 reason code를 요구하며,
자동으로 security review hold를 생성한다.

## 감사 규칙

모든 mutation, capability/scope 거절, grant 발급·사용·회수, break-glass, privileged-helper effect,
archive hold 변경은 append-only audit event를 남긴다. event는 actor, impersonation 여부, request ID,
resource/project/task/lease 상관관계, policy revision, allow/deny outcome, reason code, 전후 metadata
hash를 가진다. prompt·secret·raw provider payload·session/bearer 값은 기록하지 않는다.

audit read도 권한이며 Project 범위 읽기는 자신의 Project event만, SecurityAdmin은 global event를
읽을 수 있다. audit event 수정·삭제 API는 제공하지 않는다. Project 범위 읽기는 audit record에
`project_id`가 있어야 성립하므로, 상관관계 필드 추가가 이 규칙의 선행 조건이다.

### 현재 감사 범위

위 규칙은 목표 계약이다. **`#76`(2026-08-23, 1단계)로 HTTP `/v1` 표면의 mutation과 capability 거절은
대부분 감사된다.** Dashboard·MCP 표면과 상관관계 필드는 아직이다(`#95`).

| 경로 | 현재 감사 | 비고 |
|---|---|---|
| `GET /v1/workers/{name}/credentials/{model}/export` | 기록함 | 감사 기록 실패 시 평문을 반환하지 않는다(fail-closed) — `#76`이 발급(mint) 계열에도 같은 원칙을 적용했다 |
| `PUT /v1/workers/{name}/credentials/{model}` | 기록함 | |
| `DELETE /v1/workers/{name}/credentials/{model}` | 기록함 | |
| bootstrap token 발급·회수 | **기록함 (`#76`)** | `token.bootstrap.issue`/`.revoke`. 발급은 fail-closed(감사 실패 시 방금 만든 토큰 즉시 회수), 회수는 log-only |
| admin token 생성·회전·회수 | **기록함 (`#76`)** | `admin_token.create`/`.rotate`/`.revoke`. 생성·회전은 fail-closed, 회수는 log-only |
| Worker 등록·등록해제, Host 등록 | **기록함 (`#76`)** | `worker.register`/`.deregister`/`host.register`, 전부 log-only. heartbeat(고빈도)는 제외 |
| HTTP capability 거절 | **기록함 (`#76`)** | `http.capability_denied`, log-only — `auth_middleware`의 모든 인증 분기(개발 무인증 포함)에서 `authorize_http_endpoint`가 거절할 때 기록 |
| Dashboard·MCP mutation/거절 | **없음** | Dashboard는 중앙 capability 행렬 자체가 없다(`#92`가 다룸). MCP tool별 감사도 착수 전 |

또한 현재 `AuditEvent`에는 `request_id`, `project_id`, `policy_revision` 상관관계 필드가 없어 구현
게이트 6을 완전히 만족하지 못한다(`#95`). `project_id`는 대응하는 Project 엔티티가 아직 없어
(`#48` 계열 선행) 상관시킬 대상이 없다. **실행 상관 필드는 `attempt_id`가 아니라 `task_id`이며
그것은 이미 있다** — `TaskAttempt`를 만들지 않기로 한 [흡수 판정](../architecture/project-task-agent-lifecycle.md#attempt-흡수-판정)에
따라, "Attempt 엔티티가 생기면 상관시킨다"는 대기 사유는 성립하지 않는다.

## 구현 게이트

1. HTTP·Dashboard·MCP에서 동일 principal/capability/scope 조합이 같은 allow/deny를 내는 시험
2. Worker identity가 다른 worker/project resource에 접근하지 못하는 시험 — **조회 route 포함**
   (`GET /v1/workers/{id}`처럼 mutation이 아닌 경로도 대상이다)
3. Task ID만으로 Project scope를 우회하거나 존재를 열거하지 못하는 시험
4. MCP prompt injection/환경변수 위조가 principal·capability를 획득하지 못하는 시험
5. break-glass의 step-up·dual approval·expiry·감사 누락 거절 시험
6. 모든 mutation과 sensitive deny가 상관관계 필드·secret-free audit record를 남기는 시험
7. capability 행렬에 등록되지 않은 route/tool이 **deny**되는 시험(행렬 커버리지 테스트)
8. principal→capability 매핑이 없는 인증 주체가 write·export capability를 얻지 못하는 시험
9. `project:policy_manage`만 가진 principal이 Agent 수·provisioning 대상 정책 필드를 바꾸지 못하고
   (`agent:manage`를 추가로 요구), 그 필드가 바뀐 뒤의 Task 제출이 제출자의 권한으로 Agent를 만들지
   않는 시험 — 위 "Project 정책 변경과 Agent 생성의 관계"의 집행 증명.
   `#49` 1단계 기준 **아직 시험 불가**: `projects`에 정책 컬럼이 없어 바꿀 필드가 없다.
