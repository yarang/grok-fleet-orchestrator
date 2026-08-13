# Model Context Protocol (MCP) 표준 준수 명세서 (MCP Specification)

본 문서는 **Grok Fleet Orchestrator**가 외부 AI 클라이언트(Cursor, Claude Code, VSCode 등)와의 연동을 위해 채택하여 준수한 **Model Context Protocol (MCP)** 표준의 기술적 의미, 프로토콜 구조, 그리고 노출된 도구(Tools)의 와이어 규격(Wire Spec)을 정의합니다.

---

## 1. Model Context Protocol (MCP) 개요

**Model Context Protocol (MCP)**은 Anthropic사에서 제창한 오픈소스 개방형 표준 프로토콜로, LLM 클라이언트(AI 코딩 에이전트 등)와 외부 도구(데이터베이스, 웹 검색기, 빌드 시스템 등) 간의 통신을 일관되게 규격화합니다.

Grok Fleet Orchestrator는 특정 상용 에이전트의 내부 API에 얽매이지 않고, 독자적인 **MCP stdio 서버** 역할을 자처하여 시스템 가용성과 상호운용성(Interoperability)을 확보합니다.

![MCP Architecture Diagram](../assets/diagrams/architecture/mcp-architecture.mermaid)

---

## 2. 프로토콜 통신 규격 (Wire Protocol)

오케스트레이터는 MCP 사양 중 **stdio 전송 계층 (Newline-delimited stdio Transport)**을 준수합니다.

* **통신 채널**: 클라이언트가 `fleet serve` 프로세스를 서브프로세스로 실행하고, 표준 입력(stdin)과 표준 출력(stdout) 채널을 통해 줄바꿈(`\n`)으로 구분된 JSON-RPC 2.0 패킷을 교환합니다.
* **프로토콜 프레임 형식 (JSON-RPC 2.0)**:
  * **요청 (Request)**:
    ```json
    {
      "jsonrpc": "2.0",
      "method": "tools/call",
      "params": {
        "name": "fleet_dispatch_task",
        "arguments": {
          "prompt": "refactor database schema"
        }
      },
      "id": 1
    }
    ```
  * **응답 (Response)**: `content` 배열의 `text`는 도구별 JSON 결과를 문자열로 직렬화한 값이며, 봉투에는 항상 `isError`가 포함됩니다.
    ```json
    {
      "jsonrpc": "2.0",
      "result": {
        "content": [
          {
            "type": "text",
            "text": "{\"task_id\":\"550e8400-e29b-41d4-a716-446655440000\",\"status\":\"dispatched\",\"hint\":\"Poll fleet_get_task_status with the task_id to observe completion.\"}"
          }
        ],
        "isError": false
      },
      "id": 1
    }
    ```

---

## 3. 핵심 표준 메소드 핸들링 (Standard Methods)

`fleet-mcp` 크레이트는 다음 세 가지 필수 MCP 표준 메소드를 가로채어 매핑을 수행합니다.

### 1) `initialize`

클라이언트와 서버가 서로의 케이퍼빌리티(Capabilities)와 프로토콜 버전을 확인하는 최초의 핸드셰이크입니다.

* **서버 응답**: `{"protocolVersion": "2024-11-05", "capabilities": {"tools": {"listChanged": false}}, "serverInfo": {"name": "grok-fleet-orchestrator", "version": "<cargo 패키지 버전>"}}` (`crates/fleet-mcp/src/schema.rs`).

### 2) `tools/list`

오케스트레이터가 지원하는 도구(Tools)의 전체 명세 목록과 입력 스키마(JSON Schema)를 클라이언트에 응답합니다.

* **서버 응답**: `fleet_dispatch_task`, `fleet_get_task_status` 등 **12종** 도구의 JSON Schema 목록을 반환합니다 (`crates/fleet-mcp/src/schema.rs`의 `all_tools()`; 개수는 `handlers.rs`의 유닛 테스트로 고정되어 있습니다). 모든 도구 이름에는 `fleet_` 접두사가 붙습니다.

### 3) `tools/call`

클라이언트가 특정 도구를 실행하도록 요청할 때 발생합니다. `params.name`에 해당하는 내부 핸들러(`crates/fleet-mcp/src/handlers.rs`)로 분기되어 실제 워커 노드 디스패치 루프를 트리거합니다. `initialized`/`notifications/initialized`(무동작 ack)와 `ping`(`null` 반환)도 함께 처리됩니다(`server.rs`).

---

## 4. 오케스트레이터 MCP 도구(Tools) 상세 사양

모든 도구는 표준 JSON Schema를 준수하며, 결과는 MCP 규격에 맞게 `content` 배열 내의 text 객체로 래핑되어 직렬화됩니다(`isError` 불리언 포함). 아래는 12종 도구의 요약이며, 전체 입출력 JSON 예시는 [`api-reference.md` §MCP 도구](./api-reference.md)를 정본으로 참조하세요 — 이름·인자·응답 필드는 `crates/fleet-mcp/src/schema.rs`/`handlers.rs` 기준으로 2026-08-12에 전면 정정되었습니다(이전 판은 `fleet_` 접두사 없는 도구명과 존재하지 않는 인자를 다수 포함하고 있었습니다). 2026-08-13에 호스트 인벤토리/브레이커 리셋/부트스트랩 토큰 관리 4종(9~12번)이 추가되어 8종에서 늘었습니다(로드맵 #28).

### 1) `fleet_dispatch_task` (작업 제출)

프롬프트를 비동기 작업으로 큐에 등록합니다.

* **Arguments**: `prompt`(String, Required), `cwd`/`model`/`server_hint`(String, Optional), `required_labels`(Array of Strings, Optional), `max_turns`/`timeout_secs`(Integer, Optional). `priority` 인자는 존재하지 않습니다.
* **Response**: `task_id`, `status`(`"dispatched"` — `"pending"`이 아님), `hint`.

### 2) `fleet_get_task_status` (작업 상태 조회)

제출된 작업의 현재 진행 단계를 룩업합니다.

* **Arguments**: `task_id`(String, Required).
* **Response**: `task_id`(필드명 `id` 아님), `phase`(`pending`|`dispatched`|`completed`|`failed`|`cancelled`), `prompt`, `created_at`, `created_by`, phase별 부가 필드(`worker_id`/`output`/`error` 등).

### 3) `fleet_list_tasks` (작업 목록 조회)

상태 필터와 페이지네이션으로 작업 목록을 조회합니다.

* **Arguments**: `status`(String, Optional, `pending`|`dispatched`|`completed`|`failed`|`cancelled`|`terminal`|`active`), `limit`(Integer, Optional, 기본 50, 1~200), `offset`(Integer, Optional, 기본 0).
* **Response**: `tasks` 배열(`count` 포함) — `output`은 항상 생략되고 완료 작업은 `output_bytes`만 포함.

### 4) `fleet_cancel_task` (작업 취소)

실행 중이거나 대기 중인 작업을 취소합니다.

* **Arguments**: `task_id`(String, Required), `reason`(String, Optional, 기본 `"cancelled by user"`).
* **Response**: 성공 시 `task_id`, `status: "cancelled"`. 이미 종료 상태인 작업에 호출하면 도구 에러(`isError:true`).

### 5) `fleet_list_workers` (워커 상태 조회)

현재 스케줄러가 통제 중인 모든 워커 노드의 상태를 가져옵니다.

* **Arguments**: `status`(String, Optional, `online`|`degraded`|`offline`|`circuit_open`), `labels`(Object, Optional), `limit`(Integer, Optional, 기본 100, 1~500).
* **Response**: `workers` 배열(`id`/`name`/`endpoint`/`status`/`labels`/`active_tasks`/`max_concurrent`/`circuit_state`/`last_seen`/`registered_at`) + `count`. CPU/VRAM 등 리소스 메트릭은 포함되지 않습니다.

### 6) `fleet_wait_for_task` (동기식 종료 대기)

작업이 종료 상태(`completed`/`failed`/`cancelled`)가 될 때까지 커넥션을 점유하고 동기식으로 대기합니다.

* **Arguments**: `task_id`(String, Required), `timeout_secs`(Integer, Optional, **기본 300**, 1~3600). `poll_interval_secs` 인자는 이 도구에 없습니다.
* **Response**: 종료 시 `fleet_get_task_status`와 동일한 형태.

### 7) `fleet_stream_task_output` (로그 스트리밍)

특정 오프셋부터 발생한 stdout/stderr 출력을 폴링하며 누적된 문자열로 반환합니다.

* **Arguments**: `task_id`(String, Required), `from_offset`(Integer, Optional, 기본 0), `poll_interval_secs`(Integer, Optional, 기본 1, 1~30), `max_polls`(Integer, Optional, 기본 60, 1~600).
* **Response**: `output`(누적 문자열 — 청크 배열이 아님), `chunks_seen`, `next_offset`, `polls_used`, `stopped_reason`(`"terminal"`|`"max_polls_reached"`). `task_terminal` 불리언은 존재하지 않습니다.

### 8) `fleet_collect_results` (배치 결과 수집)

다수 작업의 최종 상태를 병렬로 수집합니다.

* **Arguments**: `task_ids`(Array of Strings, Required, 1~200개), `include_output`(Boolean, Optional, 기본 `true`). `timeout_secs` 인자는 존재하지 않습니다.
* **Response**: `results` 배열, `count`, `summary`(`terminal`/`not_found`/`total`).

### 9) `fleet_list_hosts` (호스트 인벤토리 조회, 2026-08-13 추가)

`hosts` 테이블 전체를 조회합니다 — `fleet_list_workers`와 달리 아직 워커로
조인하지 않은 프로비저닝된 호스트, 오프라인/장애 호스트까지 포함합니다.

* **Arguments**: `status`(String, Optional, `provisioned`|`online`|`offline`|`failed`).
* **Response**: `hosts` 배열(`id`/`hostname`/`worker_id`/`status`/`ssh_host`/`ssh_port`/`grok_version`/`fleet_worker_version`/`load_avg`/`mem_available_mb`/`disk_free_mb`/`last_heartbeat_at`/`provisioned_at`) + `count`.

### 10) `fleet_reset_worker_breaker` (CircuitBreaker 강제 리셋, 2026-08-13 추가)

워커의 CircuitBreaker를 `Closed`로 강제 리셋합니다.

* **Arguments**: `worker_id`(String, Optional) 또는 `worker_name`(String, Optional) — 둘 중 정확히 하나만.
* **Response**: `worker_id`, `previous_state`, `new_state: "closed"`. 존재하지 않는 `worker_name`은 `isError:true`.
* 리셋은 store에 영속화되고 `WorkerCircuitChanged` 이벤트로 발행되어 `MultiAdminSync`(로드맵 #25)를 통해 다른 인스턴스에도 전파됩니다.

### 11) `fleet_list_bootstrap_tokens` (부트스트랩 토큰 조회, 2026-08-13 추가)

워커 조인 토큰 목록을 최신순으로 조회합니다(원문 토큰 값 포함, 마스킹 없음).

* **Arguments**: 없음.
* **Response**: `tokens` 배열(`token`/`created_at`/`expires_at`/`max_uses`/`use_count`/`usable`/`notes`/`last_used_by`/`last_used_at`) + `count`.

### 12) `fleet_revoke_bootstrap_token` (부트스트랩 토큰 폐기, 2026-08-13 추가)

부트스트랩 토큰을 즉시 폐기합니다(되돌릴 수 없음, 이미 조인한 워커는 무영향).

* **Arguments**: `token`(String, Required).
* **Response**: `token`, `revoked: true`. 존재하지 않는 토큰은 `isError:true`.
* ⚠️ 토큰 **발급**(create)은 의도적으로 MCP에 노출하지 않았습니다 — fleet-cli/HTTP API 전용.

---

## 5. MCP 표준 준수의 의의

1. **상호운용성 (Interoperability)**: Cursor, Claude Code, VSCode Cline 등 MCP 클라이언트 프로토콜만 준수하는 범용 코딩 에이전트라면 복잡한 설정 없이 `mcpServer` 설정에 `fleet serve` 경로와 환경변수만 입력하면 클러스터 전체 자원을 즉시 도구로 활용 가능합니다.
2. **에이전트 인프라 격리**: AI 모델이 구동되는 호스트(AI Client Host)와 실제 빌드/쉘 명령어가 실행되는 워커(Worker Host)가 오케스트레이터의 ACP 채널 및 mTLS 경계를 통해 철저하게 물리적/네트워크적으로 격리되어, 클라이언트의 로컬 시스템 오염이나 임의의 보안 탈취 위험을 완벽하게 차단합니다.
