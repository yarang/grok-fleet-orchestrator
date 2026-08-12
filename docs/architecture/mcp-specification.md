# Model Context Protocol (MCP) 표준 준수 명세서 (MCP Specification)

본 문서는 **Grok Fleet Orchestrator**가 외부 AI 클라이언트(Cursor, Claude Code, VSCode 등)와의 연동을 위해 채택하여 준수한 **Model Context Protocol (MCP)** 표준의 기술적 의미, 프로토콜 구조, 그리고 노출된 도구(Tools)의 와이어 규격(Wire Spec)을 정의합니다.

---

## 1. Model Context Protocol (MCP) 개요

**Model Context Protocol (MCP)**은 Anthropic사에서 제창한 오픈소스 개방형 표준 프로토콜로, LLM 클라이언트(AI 코딩 에이전트 등)와 외부 도구(데이터베이스, 웹 검색기, 빌드 시스템 등) 간의 통신을 일관되게 규격화합니다.

Grok Fleet Orchestrator는 특정 상용 에이전트의 내부 API에 얽매이지 않고, 독자적인 **MCP stdio 서버** 역할을 자처하여 시스템 가용성과 상호운용성(Interoperability)을 확보합니다.

![MCP Architecture Diagram](../assets/diagrams/architecture/mcp-architecture.mmd)

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
        "name": "submit_task",
        "arguments": {
          "prompt": "refactor database schema"
        }
      },
      "id": 1
    }
    ```
  * **응답 (Response)**:
    ```json
    {
      "jsonrpc": "2.0",
      "result": {
        "content": [
          {
            "type": "text",
            "text": "{\"task_id\":\"550e8400-e29b-41d4-a716-446655440000\",\"status\":\"pending\"}"
          }
        ]
      },
      "id": 1
    }
    ```

---

## 3. 핵심 표준 메소드 핸들링 (Standard Methods)

`fleet-mcp` 크레이트는 다음 세 가지 필수 MCP 표준 메소드를 가로채어 매핑을 수행합니다.

### 1) `initialize`
클라이언트와 서버가 서로의 케이퍼빌리티(Capabilities)와 프로토콜 버전을 확인하는 최초의 핸드셰이크입니다.
* **서버 응답**: `tools` 케이퍼빌리티를 활성화하여 응답합니다.

### 2) `tools/list`
오케스트레이터가 지원하는 도구(Tools)의 전체 명세 목록과 입력 스키마(JSON Schema)를 클라이언트에 응답합니다.
* **서버 응답**: `submit_task`, `get_task_status` 등 6종(또는 7종) 도구의 JSON Schema 목록을 반환합니다.

### 3) `tools/call`
클라이언트가 특정 도구를 실행하도록 요청할 때 발생합니다. `params.name`에 해당하는 내부 핸들러(Dispatcher)로 분기되어 실제 워커 노드 디스패치 루프를 트리거합니다.

---

## 4. 오케스트레이터 MCP 도구(Tools) 상세 사양

모든 도구는 표준 JSON Schema를 준수하며, 결과는 MCP 규격에 맞게 `content` 배열 내의 text 객체로 래핑되어 직렬화됩니다.

### 1) `submit_task` (작업 제출)
프롬프트를 비동기 작업으로 큐에 등록합니다.
* **Arguments**:
  * `prompt` (String, Required): 실행할 자연어 지시 사항.
  * `cwd` (String, Optional): 작업을 실행할 원격 디렉토리 절대 경로.
  * `model` (String, Optional): 타겟 LLM 모델 (예: `gemini-2.5-flash`).
  * `required_labels` (Array of Strings, Optional): 타겟 워커가 반드시 만족해야 할 라벨 목록.
* **Response**: `task_id` 및 최초 상태 (`pending`).

### 2) `get_task_status` (작업 상태 조회)
제출된 작업의 현재 진행 단계를 룩업합니다.
* **Arguments**:
  * `task_id` (UUID, Required): 상태를 조회할 작업 ID.
* **Response**: `id`, `phase` (`pending` | `dispatched` | `completed` | `failed` | `cancelled`), `worker_id`, `created_at`.

### 3) `wait_for_task` (동기식 종료 대기)
작업이 완료(`completed`/`failed`/`cancelled`) 상태가 될 때까지 커넥션을 점유하고 동기식으로 대기합니다.
* **Arguments**:
  * `task_id` (UUID, Required): 대기할 작업 ID.
  * `timeout_secs` (Integer, Optional, Default=60): 최대 대기 시간.
* **Response**: 작업 최종 결과 및 상태.

### 4) `cancel_task` (작업 취소)
실행 중이거나 대기 중인 작업을 강제로 취소합니다.
* **Arguments**:
  * `task_id` (UUID, Required): 취소할 작업 ID.
  * `reason` (String, Optional): 취소 사유.

### 5) `list_workers` (워커 상태 조회)
현재 스케줄러가 통제 중인 모든 워커 노드의 실시간 상태를 가져옵니다.
* **Arguments**:
  * `status` (String, Optional): `online` | `degraded` | `offline`.
  * `labels` (Object, Optional): 필터링할 라벨 쌍.
* **Response**: 워커 배열 목록 (각 워커의 CPU/VRAM 로드율 메트릭 포함).

### 6) `stream_task_output` (로그 스트리밍)
특정 오프셋부터 발생한 stdout/stderr 출력 로그 청크를 수집하여 청크 단위로 리턴합니다.
* **Arguments**:
  * `task_id` (UUID, Required): 로그를 조회할 작업 ID.
  * `from_offset` (Integer, Required): 수집을 시작할 로그 바이트 오프셋.
* **Response**: `chunks` 배열, `next_offset` 정수, `task_terminal` 여부.

---

## 5. MCP 표준 준수의 의의

1. **상호운용성 (Interoperability)**: Cursor, Claude Code, VSCode Cline 등 MCP 클라이언트 프로토콜만 준수하는 범용 코딩 에이전트라면 복잡한 설정 없이 `mcpServer` 설정에 `fleet serve` 경로와 환경변수만 입력하면 클러스터 전체 자원을 즉시 도구로 활용 가능합니다.
2. **에이전트 인프라 격리**: AI 모델이 구동되는 호스트(AI Client Host)와 실제 빌드/쉘 명령어가 실행되는 워커(Worker Host)가 오케스트레이터의 ACP 채널 및 mTLS 경계를 통해 철저하게 물리적/네트워크적으로 격리되어, 클라이언트의 로컬 시스템 오염이나 임의의 보안 탈취 위험을 완벽하게 차단합니다.
