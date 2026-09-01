---
type: protocol-contract
authority: canonical
implementation: partial
verification: code-checked
source: "docs/contracts/mcp-tools.md"
last_verified: "2026-09-01"
last_verified_commit: "working-tree"
owners: ["fleet-mcp"]
---

# MCP 도구 계약

이 문서는 `fleet serve`가 stdio JSON-RPC로 노출하는 MCP 도구의 정본 진입점이다. 실제 도구
이름·입력 스키마·응답은 [`crates/fleet-mcp/src/schema.rs`](../../crates/fleet-mcp/src/schema.rs)와
handler 테스트를 기준으로 한다.

## 전송과 오류

현재 구현은 MCP protocol `2024-11-05`의 stdio JSON-RPC 2.0을 사용한다. `initialize`,
`tools/list`, `tools/call`을 지원하며, `tools/list`의 JSON Schema가 필드의 기계 판독
정본이다. JSON-RPC 수준에서는 parse `-32700`, invalid request `-32600`, method not found
`-32601`, invalid params `-32602`, internal `-32603`을 사용한다. 도구의 논리적 실패는
정상 JSON-RPC 응답 안에서 `isError: true`로 반환된다.

`tools/call`에서 **존재하지 않는 도구와 권한 없는 도구는 서로 다른 코드로 갈린다.**
카탈로그(`all_tools()`)에 없는 이름은 `-32601`(method not found), 카탈로그에는 있지만
이 launcher의 `FLEET_MCP_CAPABILITIES`가 허용하지 않은 도구는 `-32600`(invalid request)
이다. 둘을 하나로 뭉치면 호출자가 오타와 권한 부족을 구분할 수 없어 어느 쪽을 고쳐야
하는지 알 수 없다. 존재 여부가 드러나는 것은 의도된 것이다 — 근거는 이 문서가 카탈로그를
싣고 있다는 사실이 아니라, `tools/list`가 **같은 비인증 채널로** 이미 그 launcher의 전체 부여
집합을 열거한다는 것이다. 따라서 `-32600`이 추가로 흘리는 정보는 "받지 못한 capability에 속한
도구가 카탈로그에 있다"뿐이며, 이를 감추려면 `tools/list`부터 바꿔야 한다.

**대상이 없을 때의 코드는 도구마다 갈려 있으며, 이것은 아직 정리되지 않은 상태다.**
`fleet_stop_agent`와 `fleet_start_agent`(`#67` 4b)는 없는 Agent를 `isError: true`인 정상
응답(tool-level)으로 돌려주고, `fleet_place_agent`(`#67` 4a)는 `-32602 invalid_params`
(protocol-level)로 돌려준다. `start`가 `stop`을 따른 것은 그것이 `stop`의 거울 표면이고,
"그런 Agent가 없다"는 프로토콜 위반이 아니라 도구가 관측한 결과이기 때문이다 —
인자의 **형태**는 멀쩡하고 지시한 **대상**만 없으므로 `invalid_params`의 뜻과 맞지 않는다.
`place`를 이번에 함께 바꾸지 않은 것은 그것이 `#67` 4b의 범위 밖이고, 그 계약에 의존하는
테스트가 이미 있기 때문이다. 정리 방향은 tool-level로 통일하는 쪽이며, 그때 이 문단과
`place`의 테스트를 함께 고친다.

존재 판정의 정본은 `all_tools()` 카탈로그이지 `required_permission()`이 아니다. 후자는
`fleet_transition_issue`처럼 **존재하지만** 요구 capability가 인자에 따라 달라지는 도구에도
`None`을 반환하므로, `None`을 "없는 도구"로 읽으면 오판한다.

**응답 envelope는 MCP 사양이 정본이며 camelCase다.** `tools/list`의 `result`는
`{ "tools": [ { "name", "description", "inputSchema" } ] }` 객체이고, `tools/call`의
`result`는 `{ "content": [ { "type": "text", "text": … } ] }`다. 이 형태는 취향 문제가
아니라 상호운용의 전제다 — 표준 MCP 클라이언트(SDK)는 응답을 `ListToolsResult`
스키마로 검증하고, 통과하지 못한 응답은 **자기가 기다리던 응답으로 인정하지 않는다**.
그래서 형태가 틀리면 오류가 아니라 요청 타임아웃으로 나타나고, client 쪽에는 "연결은
됐는데 도구가 0개"로 보인다(2026-08-25에 실제로 그 상태였다 — `result`를 배열로,
필드를 `input_schema`로 내보내고 있었고, `cross_client.rs`의 형태 검증 테스트가 그
잘못된 형태를 "사양"으로 단언하고 있어 드러나지 않았다).

## 범위

- MCP stdio transport와 `tools/list`, `tools/call`의 Fleet 구현
- Task, Worker, Host, bootstrap token, Project, Issue 관련 도구의 입력·출력 스키마
- MCP 호출의 인증·권한 경계

HTTP `/v1`이나 Dashboard `/api/*`는 이 문서의 범위 밖이다. 이전 MCP 상세 문서는 삭제됐으며,
도구의 현재 wire schema는 코드와 이 문서의 연결된 정본을 따른다.

## 현재 도구 표면

| 도구 | 주요 입력 | 결과 범주 |
|---|---|---|
| `fleet_dispatch_task` | `prompt`, **필수** `cwd`, 선택 `model`, `server_hint`, `agent_id`, labels, turn·timeout | 비동기 `task_id` |
| `fleet_get_task_status` | `task_id` | Task 상태·결과 |
| `fleet_list_tasks` | 선택 status, limit, offset | Task 목록 |
| `fleet_cancel_task` | `task_id`, 선택 reason | 취소 결과 |
| `fleet_wait_for_task` | `task_id`, 선택 timeout | terminal Task 또는 timeout 오류 |
| `fleet_stream_task_output` | `task_id`, `from_offset`, polling 설정 | 출력 chunk와 현재 상태 |
| `fleet_collect_results` | `task_ids` | 여러 Task 결과 |
| `fleet_list_workers` | 선택 status, labels, limit | Worker 목록 |
| `fleet_list_hosts` | 선택 status | Host 목록 |
| `fleet_reset_worker_breaker` | Worker 식별자 | circuit breaker reset 결과 |
| `fleet_list_bootstrap_tokens` | 입력 없음 | token 메타데이터 목록 |
| `fleet_revoke_bootstrap_token` | `token_id` | revoke 결과 |
| `fleet_create_project` | `name`, 선택 `description` | 생성된 Project |
| `fleet_list_projects` | 선택 limit, offset | Project 목록 |
| `fleet_delete_project` | `project_id` | archive 진행 결과 — Project 상태와, `draining`에 머물렀다면 `archive_blocked_by`(`tasks`/`agents`) |
| `fleet_create_agent` | `project_id`, `name`, 선택 `description` | 생성된 Agent |
| `fleet_list_agents` | 선택 `project_id`, limit, offset | Agent 목록 |
| `fleet_start_agent` | `agent_id` | start 결과(`desired_status: running`과 그 `generation`) |
| `fleet_stop_agent` | `agent_id` | 회수 결과(`stopped`) |
| `fleet_place_agent` | `agent_id`, 선택 `worker_id` | 배정 결과(선택된 Worker). 상한 도달·다른 Worker가 `running`으로 보고한 Agent의 이동은 `-32602`. 회수된 Agent 배정은 2026-09-01부터 거절하지 않는다 — [Dashboard 계약](dashboard-api.md)의 같은 표가 정본이다 |
| `fleet_list_issues` | 선택 `project_id`, `status`, `open_only` | Issue 목록 |
| `fleet_create_issue` | `project_id`, `title`, 선택 `body`, `severity`, labels | 생성된 Issue |
| `fleet_transition_issue` | `issue_id`, `status`, 선택 `close_reason` | 전이 결과 |
| `fleet_comment_issue` | `issue_id`, `body` | 등록된 코멘트 |

이 표는 `all_tools()`의 **전체 카탈로그**다. 특정 launcher가 실제로 `tools/list`에
내보내는 것은 그 launcher의 `FLEET_MCP_CAPABILITIES`가 허용한 부분집합이다 — 예를 들어
Project·Issue 도구는 `project:*`/`issue:*` capability를 부여하지 않은 launcher에서는 보이지
않는다. 도구별 요구 capability의 정본은 **둘**이다: 대부분은 `server.rs`의
`required_permission`이 정하지만, `fleet_transition_issue`만은 요구 capability가 목표 상태에
따라 달라지므로 `fleet-core`의 `required_capability_for_transition`이 정본이고
`required_permission`은 그 도구에 `None`을 반환한다(위 "전송과 오류" 절 참고).
Agent 도구는 `#49` 1단계 범위다 — Agent를 **정의**로 만들고 회수할 뿐이며, 뒤에 프로세스가
없으므로 시작·attach·Task 배정이 없다. 수명주기는 `ready → stopped` 둘뿐이고, `project_id`를
바꾸는 도구는 만들지 않았다(불변). 명령/ACK 도구는 워커 제어 스트림(`#67` 4단계)이 선행이다.
AgentTemplate 관리 도구는 아직 제안 계약일 뿐 구현돼 있지 않다(로드맵 `#92`, `#86` 선행).
새 도구는 여기 표, `tools/list` schema, handler 테스트를 한 변경으로 갱신한다.

`fleet_dispatch_task`의 `cwd`는 **2026-08-28(로드맵 `#69` 전제)부터 필수다** — 기존 클라이언트에
대한 파괴적 변경이며, `cwd` 없이 호출하면 `-32602 invalid_params`를 받는다. 선택 인자였을 때
오케스트레이터는 값이 없으면 `/`를 지어내 파일시스템 루트에서 ACP 세션을 열었고, 그것이
실행 격리 정본을 정면으로 어겼다. ACP `NewSessionRequest.cwd`는 프로토콜 양쪽 버전에서 필수라
"보내지 않는다"는 선택지가 없으므로, 지어내지 않으려면 요청을 거절하는 수밖에 없다. 검증은
어휘적이다 — 절대 경로여야 하고, `..` 세그먼트와 interior NUL을 금지하며, `/` 자체는 거절한다.
경로가 실제로 워커의 workspace 안인지(canonical containment)는 **검사하지 않는다**: 그 판정은
워커의 파일시스템에서만 가능하다. 정본 규칙은 `fleet-core`의 `validate_workspace_cwd`이며
Dashboard `POST /api/tasks`와 `fleet tasks submit`도 같은 규칙을 쓴다.

`fleet_dispatch_task`의 `agent_id`는 **2026-09-01(로드맵 `#49` 2단계)부터 선택 인자다** — 이
Task를 특정 Agent가 처리하게 지목한다. 제출 시점에 검증하는 것은 셋이다: (1) 그 Agent가
실재하는가, (2) `server_hint`와 함께 오지 않았는가, (3) 명시된 `project_id`가 Agent의 Project와
같은가. 셋 다 `-32602 invalid_params`로 거절되며, **Task 행은 만들어지지 않는다** — 존재하지
않는 Agent를 지목한 요청은 그 자체로 틀렸고, dispatch까지 미루면 요청자가 이미 `task_id`를
받고 떠난 뒤에 실패하기 때문이다. `project_id`를 생략하면 Agent의 Project를 **물려받는다**
(`Agent::project_id`는 항상 값이 있으므로 지목한 순간 이미 정해진 것이나 다름없다). 물려받은
Project도 `Draining`/`Archived`면 거절된다 — 그러지 않으면 보관된 Project의 Agent를 지목하는
것만으로 새 Task를 밀어 넣는 우회로가 된다. `server_hint`와의 동시 사용을 일치 검사로
통과시키지 않고 항상 거절하는 이유는, Agent가 아직 배정되지 않았으면(`worker_id IS NULL`,
회복 가능한 정상 상태) 일치를 판정할 수 없어 같은 요청이 Agent의 배정 상태에 따라 통과했다
거절됐다 하기 때문이다.

반대로 **가용성**(Agent가 지금 돌고 있는가, 그 Worker가 살아 있는가)은 제출 시점에 검사하지
않는다 — 그때 참이어도 dispatch 시점에 거짓일 수 있다. 그 판정은 dispatch가 하고, 실패하면
Task는 만들어진 채 `Failed`가 된다. 규칙 정본은 `fleet-store`의 `apply_agent_pin`이며
Dashboard `POST /api/tasks`도 같은 함수를 쓴다.

## 보안 상태

현재 launcher는 `FLEET_MCP_CAPABILITIES` allow-list로 노출 도구 자체를 제한하며, 값이 없거나
비어 있거나 알 수 없으면 stdio 서버가 기동하지 않는다. 이것이 첫 경계다.

그러나 stdio MCP `ToolContext`에는 여전히 호출 principal, Project scope, 요청 감사 주체가 없다.
따라서 도구 노출 여부는 통제되지만 **호출자별 권한 판정과 감사는 없다**. destructive 도구의
`request_id`, 멱등성, precondition도 계약되어 있지 않다.

또한 capability 이름이 transport마다 같은 의미를 갖지 않는다. `fleet_reset_worker_breaker`는
`worker:delete`를 요구하는데, 같은 capability는 HTTP에서 워커 삭제 권한이다. breaker reset만
허용하려 해도 삭제 권한이 딸려오므로 `worker:operate` 신설이 필요하다.

목표 capability, Project scope, fail-closed 정책은
[Authorization·Project Scope·감사](../security/authorization-and-audit.md)를 따른다. 구현 전에는
도구가 그 정책을 보장한다고 서술하지 않는다. `fleet_list_tasks`의 offset 기반 조회는 snapshot
consistency, `has_more` 또는 next cursor를 보장하지 않는다.
