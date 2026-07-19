# 아키텍처

이 문서는 Grok Fleet Orchestrator의 내부 구조, 데이터 흐름, 핵심 설계 결정을
설명합니다. 배포 가이드는 [`deployment.md`](deployment.md), API 레퍼런스는
[`api-reference.md`](api-reference.md)를 참조하세요.

## TL;DR

```text
┌──────────────────────────────────────────────────────────────────┐
│                        AI 코딩 클라이언트                          │
│  grok build │ Claude Code │ Cursor │ Codex │ Gemini CLI │ ...     │
└───────────────────────┬──────────────────────────────────────────┘
                        │  MCP JSON-RPC (stdio) 또는 HTTP
                        ▼
┌──────────────────────────────────────────────────────────────────┐
│                    Orchestrator (단일 Rust 바이너리)               │
│  ┌─────────────┐ ┌─────────────┐ ┌──────────────┐ ┌───────────┐ │
│  │ fleet-mcp   │ │ fleet-api   │ │ fleet-dash   │ │ fleet-cli │ │
│  │ 7 tools     │ │ REST + auth │ │ HTML + SSE   │ │ workers/  │ │
│  │             │ │ /metrics    │ │              │ │ tasks/... │ │
│  └──────┬──────┘ └──────┬──────┘ └──────┬───────┘ └─────┬─────┘ │
│         │               │              │                │       │
│         └───────────────┼──────────────┼────────────────┘       │
│                         ▼              ▼                        │
│              ┌──────────────────────────────────┐               │
│              │   fleet-scheduler                 │               │
│              │   • WorkerSelector (hint+labels)  │               │
│              │   • CircuitBreaker (3-state)      │               │
│              │   • Dispatcher (event loop)       │               │
│              │   • HealthChecker                 │               │
│              └────────────────┬─────────────────┘               │
│                               ▼                                  │
│              ┌──────────────────────────────────┐               │
│              │   fleet-store (Store trait)       │               │
│              │   • PgStore                       │               │
│              │   • migrations (sqlx)             │               │
│              │   • LISTEN/NOTIFY                 │               │
│              └────────────────┬─────────────────┘               │
└───────────────────────────────┼──────────────────────────────────┘
                                ▼
                       ┌─────────────────┐
                       │   PostgreSQL    │
                       │  fleet_dev DB   │
                       └─────────────────┘
                                ▲
                                │  HTTP heartbeat
                                │
              ┌─────────────────┴────────────────┐
              ▼                                  ▼
     ┌──────────────────┐              ┌──────────────────┐
     │ Worker A (Linux) │   ...        │ Worker N (Linux) │
     │ grok serve       │              │ grok serve       │
     │ + cloudflared    │              │ + cloudflared    │
     └──────────────────┘              └──────────────────┘
```

## 핵심 설계 결정

### 1. MCP 표준 준수 (독립 프로젝트)

Grok Build를 포크하거나 내부 API에 의존하지 않습니다. 대신 MCP(Model Context Protocol)
JSON-RPC 2.0 over newline-delimited stdio를 구현하여, **어떤 AI 코딩 도구든** 동일한
인터페이스로 접근할 수 있습니다. Grok Build는 워커 중 하나로 소비됩니다.

### 2. 단일 Rust 바이너리

`fleet serve` 하나로 MCP stdio 서버, HTTP API, 대시보드, 헬스체커가 모두 실행됩니다.
정적 자산은 `rust-embed`로 바이너리에 임베드되어 별도의 프론트엔드 빌드 파이프라인이나
정적 파일 서버가 필요 없습니다.

### 3. Store trait 추상화

`fleet-store::Store` trait이 모든 영속화를 추상화합니다:
- 현재 구현: `PgStore` (PostgreSQL + sqlx)
- 테스트용: `MemStore` (crate-private)
- 향후 가능: SQLite (싱글노드), DynamoDB (AWS)

이 추상화 덕분에 (a) 단위 테스트가 Postgres 없이 동작하고 (b) 다른 백엔드로의
교체가 비교적 쉽습니다.

### 4. PostgreSQL LISTEN/NOTIFY로 다중 admin 동기화

여러 관리자가 동시에 `fleet serve`를 띄운 경우, 한 admin이 회로를 열면
다른 admin에게도 즉시 전파되어야 합니다. 트랜잭션 범위 밖에서 pub/sub 채널을
사용하면 이 동기화를 최소 지연으로 달성할 수 있습니다.

구체적 메커니즘:
1. Admin A가 `circuit_opened` 이벤트를 `fleet_events`에 INSERT
2. Postgres 트리거가 `NOTIFY fleet_events` 실행
3. Admin B의 `LISTEN fleet_events`가 즉시 알림 수신
4. Admin B의 로컬 CircuitBreakerRegistry가 강제 open 전환

### 5. CircuitBreaker 3-state 머신

각 워커마다 독립적인 회로차단기:
- **Closed**: 정상. 모든 dispatch 시도 통과.
- **Open**: 최근 N회 연속 실패. dispatch 즉시 거부 (`CircuitOpen` 에러).
- **HalfOpen**: 쿨다운(기본 30초) 후 1회 프로브 허용. 성공 시 Closed, 실패 시 Open 복귀.

이 패턴은 [Grok Build의 `CircuitBreakerRegistry`](https://github.com/xai-org/grok-build)에서
차용했지만, 워커별로 키를 관리하도록 재구현했습니다.

### 6. WorkerSelector: hint + label + least-loaded

```text
submit_task(server_hint="gpu-box", required_labels=["gpu"])
   │
   ├─ server_hint가 지정된 경우:
   │    일치하는 워커만 후보. 없으면 에러 (fallback 없음).
   │
   ├─ server_hint 없는 경우:
   │    labels를 만족하는 모든 dispatchable 워커에서
   │    active_tasks가 가장 적은 것을 선택.
   │
   └─ dispatchable = online && 회로 닫힘 && active < max_concurrent
```

### 7. 비동기 장기 실행 작업 모델

작업은 4단계 상태머신을 따릅니다:

```text
Pending → Dispatched → Completed
                    ↘             ↘
                     Failed      Cancelled
```

- `submit_task`는 `TaskId`만 반환하고 즉시 리턴 (non-blocking)
- `wait_for_task` (또는 `stream_task_output`)로 결과를 폴링
- `cancel_task`로 사용자 주도 취소
- 워커 장애 시 Dispatcher가 `Failed(kind=WorkerUnavailable)`로 표시

## 데이터 모델

### 테이블 (PostgreSQL 스키마)

| 테이블          | 용도                                            |
|-----------------|-------------------------------------------------|
| `fleet_workers` | 등록된 워커 (id, name, endpoint, status, labels)|
| `fleet_tasks`   | 작업 (id, prompt, status, server_hint)          |
| `fleet_events`  | append-only 이벤트 로그 (seq, event JSONB)      |
| `fleet_output`  | 작업 stdout/stderr 청크 (task_id, seq, chunk)   |

모든 마이그레이션은 `crates/fleet-store/migrations/`에 idempotent SQL 파일로
존재하며, `fleet migrate` 또는 서버 시작 시 자동 적용됩니다.

### 이벤트 로그가 곧 감사 로그

`fleet_events` 테이블이 감사 로그 역할을 동시에 수행합니다:
- 모든 상태 변화는 트랜잭션과 함께 이벤트로 기록
- `fleet events list` CLI로 조회
- 대시보드의 `/api/events/stream` SSE가 LISTEN/NOTIFY로 실시간 푸시
- Prometheus `fleet_events_written_total` 메트릭이 단조 증가 카운터 노출

## 인증 모델 (3계층)

| 레이어                | 매커니즘                          | 적용 대상                  |
|-----------------------|-----------------------------------|----------------------------|
| Cloudflare Access     | CF-Access-Jwt-Assertion (JWT)     | 외부망 → 오케스트레이터     |
| Bearer Token (API)    | `Authorization: Bearer <token>`   | 오케스트레이터 내부 API     |
| No-auth (dev mode)    | 없음                              | `--allow-no-auth` 시        |

운영 환경에서는 Cloudflare Access가 1차 방어선이고, bearer 토큰은
대시보드/모니터링 등 내부 시스템을 위한 2차 인증입니다.
자세한 내용은 [`deployment.md`](deployment.md)를 참조하세요.

## 크로스 클라이언트 호환성

MCP 표준을 준수하므로, 동일한 `fleet serve` 인스턴스에 여러 AI 클라이언트가
동시에 연결할 수 있습니다:

```text
~/.config/grok/mcp.json          → fleet serve (stdio)
~/.claude/claude_desktop.json    → fleet serve (stdio)  [동일 바이너리]
~/.cursor/mcp.json               → fleet serve (stdio)
```

각 클라이언트 세션은 독립적이지만, 같은 워커 풀과 작업 큐를 공유합니다.
한 클라이언트가 제출한 작업을 다른 클라이언트가 `get_task_status`로 조회할 수도 있습니다.

## 성능 특성

- **동시 워커**: 이론적으로 PostgreSQL 커넥션 풀 크기(~100)까지 확장
- **작업 처리량**: 워커 당 `max_concurrent`(기본 4) × 워커 수
- **이벤트 로그**: LISTEN/NOTIFY로 ~1ms 전파 지연
- **바이너리 크기**: release LTO + strip 시 ~15MB (모든 정적 자산 포함)

## 향후 로드맵

- `fleet-worker` 바이너리 (원격 서버에서 실행되는 데몬 — `grok agent serve` 래핑 + 자동 등록/하트비트)
- WebSocket 재연결 로직 (현재 AcpTransport는 첫 연결 실패 시 에러; Phase 8에서 지수 백오프 추가)
- 동시 다중 세션 per worker (현재는 직렬 prompt 처리)
- OIDC/JWKS 검증 (현재는 Cloudflare Access에 위임)
- 작업 우선순위 큐 +抢占 스케줄링
- 워커 오토스케일링 (로드 기반)
- 다중 리전 페더레이션

## ACP Transport (Phase 7)

`AcpTransport`는 `WorkerTransport` trait의 실제 구현체로, [Agent Client Protocol](https://github.com/Zed-Industries/agent-client-protocol) (ACP) over WebSocket을 사용해 각 워커의 `grok agent serve`와 통신합니다.

### 아키텍처

```text
[fleet serve --transport acp]
   │
   ├─ HTTP API /v1/workers/register
   │      └─► AcpTransport::register(worker_id, "ws://worker:2419/ws?server-key=...")
   │              ├─ AcpClient::connect(endpoint)     ← WebSocket handshake
   │              ├─ client.open_session(None)         ← session/new JSON-RPC
   │              └─ spawn reader task                 ← AcpEvent → WorkerEvent 변환
   │
   ├─ MCP submit_task
   │      └─► Dispatcher::dispatch
   │              └─► AcpTransport::dispatch(req)
   │                      ├─ session.active_task = task_id
   │                      └─ spawn: client.prompt(session, &prompt)
   │
   └─ Transport event stream (broadcast)
           ├─ WorkerEvent::Output   (agent_message_chunk 스트리밍)
           ├─ WorkerEvent::Completed (end_of_turn 응답)
           └─ WorkerEvent::Failed    (오류)
```

### ACP 메서드 지원

| Method           | 방향     | 용도                                   |
|------------------|----------|----------------------------------------|
| `initialize`     | req→res  | capabilities 교환 (protocolVersion=1)  |
| `session/new`    | req→res  | cwd로 세션 생성, sessionId 반환        |
| `session/prompt` | req→res  | 프롬프트 전송 + end_of_turn 시 결과    |
| `session/cancel` | req→res  | 진행 중 프롬프트 취소                  |
| `session/update` | notif    | 스트리밍 출력 (agent_message_chunk 등) |

### 동시성 모델

grok agent serve의 MvpAgent는 직렬 프롬프트 처리를 가정합니다. 따라서 `AcpTransport`는 워커당 동시에 1개의 진행 중 task를 추적 (`active_task: RwLock<Option<TaskId>>`). Phase 8에서 세션 풀링으로 동시성을 늘릴 예정.

### 왜 `xai-computer-hub-sdk`가 아닌가

`xai-computer-hub-sdk`는 *tool routing* 프로토콜 (에이전트가 외부 도구를 호출하는 용도)이며, 작업 디스패치 용도가 아닙니다. 따라서 fleet은 표준 ACP를 직접 구현했습니다.

### 제한 (Phase 7 MVP)

- 단일 WebSocket 연결 (재연결 없음 — Phase 8에서 지수 백오프 추가 예정)
- 단일 세션 per worker (다중 세션은 Phase 8)
- mTLS 없음 (Cloudflare Tunnel에 위임)
- ACP의 `session/load`, `authorize`, `x.ai/*` 확장 미구현
