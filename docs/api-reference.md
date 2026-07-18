# API 레퍼런스

이 문서는 오케스트레이터가 노출하는 두 가지 인터페이스 — **HTTP REST API**와
**MCP 도구** — 를 모두 다룹니다.

## HTTP REST API

기본 경로: `http://<api-bind>/v1/...`
인증: `Authorization: Bearer <token>` (또는 Cloudflare Access 활성화 시
`CF-Access-Jwt-Assertion` 헤더).

### 헬스

#### `GET /v1/health`

인증 불필요. 로드밸런서 프로브용.

**응답** (200):
```json
{
  "status": "ok",
  "version": "0.1.0",
  "heartbeat_interval_secs": 15
}
```

### 워커

#### `POST /v1/workers/register`

새 워커 등록 또는 재연결.

**요청 바디**:
```json
{
  "name": "build-farm-1",
  "endpoint": "wss://worker-a.fleet.example/ws",
  "labels": { "arch": "arm64", "gpu": "false" },
  "max_concurrent": 4
}
```

**응답** (201):
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "heartbeat_interval_secs": 15
}
```

#### `POST /v1/workers/heartbeat`

주기적 상태 업데이트. 등록 시 받은 `id` 사용.

**요청 바디**:
```json
{
  "worker_id": "550e8400-...",
  "active_tasks": 2,
  "load_avg": [0.5, 0.4, 0.3],
  "mem_available_mb": 8192,
  "disk_free_mb": 102400,
  "agent_healthy": true
}
```

**응답** (200): `{}` (빈 객체)

#### `GET /v1/workers`

워커 목록 조회.

**쿼리 파라미터**:
- `status` — `online|offline|degraded|circuit_open`
- `label_<key>=<value>` — 라벨 필터 (예: `label_arch=arm64`)

**응답** (200):
```json
[
  {
    "id": "550e8400-...",
    "name": "build-farm-1",
    "endpoint": "wss://...",
    "status": "online",
    "labels": { "arch": "arm64" },
    "active_tasks": 2,
    "max_concurrent": 4,
    "circuit_state": "closed",
    "last_seen": "2026-07-19T01:23:45Z",
    "registered_at": "2026-07-19T00:00:00Z"
  }
]
```

#### `GET /v1/workers/:id`

단일 워커 상세. 응답 구조는 위와 동일.

#### `DELETE /v1/workers/:id`

워커 등록 해제. 응답 204 No Content.

### 메트릭

#### `GET /metrics`

Prometheus 표준 텍스트 포맷. 인증 불필요.

**노출 메트릭**:

| 이름                               | 유형  | 라벨       |
|------------------------------------|-------|------------|
| `fleet_up`                         | gauge | —          |
| `fleet_workers_total`              | gauge | status     |
| `fleet_workers_capacity_total`     | gauge | —          |
| `fleet_workers_active_tasks_total` | gauge | —          |
| `fleet_tasks_total`                | gauge | phase      |
| `fleet_events_written_total`       | gauge | —          |

**예시 출력**:
```text
# HELP fleet_up Liveness indicator (always 1 if scrape succeeded).
# TYPE fleet_up gauge
fleet_up 1

# HELP fleet_workers_total Number of workers by status.
# TYPE fleet_workers_total gauge
fleet_workers_total{status="online"} 3
fleet_workers_total{status="offline"} 1
fleet_workers_total{status="total"} 4
...
```

### 대시보드 API (`--dashboard-bind`로 활성화 시)

대시보드는 별도의 HTTP 서버로 동작하며 API와는 다른 포트를 사용합니다.
기본 경로: `http://<dashboard-bind>/api/...`

#### `GET /api/overview`

```json
{
  "workers": { "online": 3, "offline": 1, "total": 4 },
  "tasks":   { "pending": 2, "completed": 18, "total": 20 },
  "generated_at": "2026-07-19T01:23:45Z"
}
```

#### `GET /api/workers?status=online&limit=100`

워커 요약 배열.

#### `GET /api/tasks?limit=100`

작업 요약 배열.

#### `GET /api/events?after_seq=0&limit=100`

이벤트 로그 페이지네이션.

#### `GET /api/events/stream` (SSE)

Server-Sent Events. 브라우저 `EventSource` API로 소비:

```javascript
const es = new EventSource("/api/events/stream");
es.addEventListener("fleet_event", (e) => {
  const entry = JSON.parse(e.data);
  console.log("seq", entry.seq, "type", entry.event.type);
});
```

연결이 끊어지면 1초 후 자동 재연결을 시도합니다. 15초마다 `keep-alive` 프레임이 발송됩니다.

#### `GET /` / `GET /static/*`

임베드된 대시보드 HTML 및 정적 자산 (CSS/JS).

---

## MCP 도구

MCP(JSON-RPC 2.0 over newline-delimited stdio) 인터페이스. AI 코딩 클라이언트가
`fleet serve`를 MCP 서버로 등록하면 아래 도구들이 자동으로 노출됩니다.

### `submit_task`

프롬프트를 비동기 작업으로 큐에 등록.

**입력 스키마**:
```json
{
  "prompt": "refactor the auth module",
  "cwd": "/workspace/project",
  "model": "grok-4",
  "server_hint": "gpu-box-1",
  "required_labels": ["gpu"],
  "max_turns": 30,
  "timeout_secs": 1800,
  "priority": "normal"
}
```

모든 필드는 `prompt`를 제외하고 선택사항.

**출력**:
```json
{
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "pending"
}
```

### `get_task_status`

**입력**: `{ "task_id": "..." }`

**출력**:
```json
{
  "id": "550e8400-...",
  "phase": "dispatched",
  "worker_id": "660e...",
  "created_at": "2026-07-19T..."
}
```

`phase` 값: `pending` / `dispatched` / `completed` / `failed` / `cancelled`.

### `wait_for_task`

작업이 종료 상태가 될 때까지 대기.

**입력**:
```json
{
  "task_id": "...",
  "timeout_secs": 60,
  "poll_interval_secs": 2
}
```

**출력**: 종료 시 `get_task_status`와 동일한 형태 + 결과 데이터.

### `cancel_task`

**입력**:
```json
{
  "task_id": "...",
  "reason": "user requested"
}
```

이미 종료된 작업에 대해 호출하면 에러 반환.

### `list_workers`

**입력**:
```json
{
  "status": "online",
  "labels": { "gpu": "true" }
}
```

모든 필드 선택사항.

**출력**: 워커 요약 배열 (HTTP API `/v1/workers`와 동일).

### `stream_task_output`

작업의 stdout/stderr를 폴링하며 새 청크를 반환.

**입력**:
```json
{
  "task_id": "...",
  "from_offset": 0,
  "max_polls": 10,
  "poll_interval_secs": 2
}
```

종료 조건: 작업이 terminal 상태가 되거나 `max_polls`에 도달할 때까지.

**출력**:
```json
{
  "chunks": [
    { "seq": 1, "chunk": "Compiling...\n", "written_at": "..." },
    { "seq": 2, "chunk": "warning: unused variable\n", "written_at": "..." }
  ],
  "next_offset": 2,
  "task_terminal": false
}
```

### `collect_results`

다수 작업의 결과를 병렬로 수집.

**입력**:
```json
{
  "task_ids": ["id1", "id2", "id3"],
  "timeout_secs": 120
}
```

**출력**: 각 작업별 상태 + 결과 (있는 경우) 매핑.

---

## 에러 형식

### HTTP API

HTTP 상태 코드 + JSON 에러 바디:
```json
{
  "error": {
    "code": "worker_not_found",
    "message": "no worker with id 550e8400-..."
  }
}
```

주요 코드:
- 400 — 잘못된 요청 (검증 실패)
- 401 — 인증 필요/실패
- 404 — 리소스 없음
- 409 — 상태 충돌 (예: 이미 종료된 작업 취소 시도)
- 500 — 서버 내부 오류

### MCP

JSON-RPC 2.0 에러 형식:
```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "error": {
    "code": -32603,
    "message": "task not found: 550e8400-...",
    "data": { "code": "task_not_found" }
  }
}
```

표준 JSON-RPC 코드 사용 (`-32700` parse error, `-32600` invalid request,
`-32601` method not found, `-32602` invalid params, `-32603` internal error).
