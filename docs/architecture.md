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

- ~~동시 다중 세션 per worker (현재는 직렬 prompt 처리; Phase 8.4)~~ → **Phase 8.4에서 per-worker 동시 다중 세션 구현** (아래 "동시 다중 세션 (Phase 8.4)" 절 참조)
- ~~mTLS for orchestrator↔worker ACP 트래픽 (Phase 8.5)~~ → **Phase 8.5.1/8.5.2에서 클라이언트/서버 mTLS 구현** (아래 "mTLS for Orchestrator↔Worker ACP 트래픽 (Phase 8.5)" 절 참조). Phase 8.5.3 CLI 통합은 후속 진행.
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

### 제한 (Phase 7 → 8.2 현재)

- ~~단일 WebSocket 연결 (재연결 없음)~~ → **Phase 8.2에서 supervisor + 지수 백오프 재연결 구현** (아래 "WebSocket Reconnection" 절 참조)
- ~~단일 세션 per worker (다중 세션은 Phase 8.4)~~ → **Phase 8.4에서 per-worker 동시 다중 세션 구현** (아래 "동시 다중 세션 (Phase 8.4)" 절 참조)
- ~~mTLS 없음 (Cloudflare Tunnel에 위임, Phase 8.5)~~ → **Phase 8.5에서 사설 CA 기반 mTLS 구현** (아래 "mTLS for Orchestrator↔Worker ACP 트래픽 (Phase 8.5)" 절 참조)
- ACP의 `session/load`, `authorize`, `x.ai/*` 확장 미구현

## Worker Daemon (Phase 8.1)

`fleet-worker`는 워커 머신에서 상주하는 데몬으로, `grok agent serve` 서브프로세스를 관리하고
오케스트레이터에 자신을 등록합니다. Phase 8은 5개 하위 항목으로 분할되며, 8.1은
바이너리 자체와 설정/수명 주기를 다룹니다.

### 모듈 구조

```text
crates/fleet-worker/
  ├── src/main.rs           ← CLI 진입 (--config /etc/fleet/worker.toml, --check)
  ├── src/config.rs         ← worker.toml 파서 + WorkerConfigBuilder
  ├── src/grok_process.rs   ← GrokRunner: spawn / health_check / restart loop
  ├── src/registration.rs   ← RegistrationClient: register / heartbeat / deregister
  ├── src/runner.rs         ← WorkerRunner: 위 두 모듈 조립 + 신호 처리
  └── src/error.rs          ← WorkerError enum
```

### `worker.toml` 형식

```toml
[worker]
name = "build-farm-1"
orchestrator_url = "https://fleet.example.com"
heartbeat_interval_secs = 15
bootstrap_token = "fleet-xxx"        # bearer auth (선택)
labels = { arch = "arm64", gpu = "false" }
existing_worker_id = "550e8400-..."  # 재등록 시 ID 유지 (선택)

[grok]
bin = "/usr/local/bin/grok"
bind_addr = "127.0.0.1:2419"
secret = "<서버 키 시크릿>"
max_concurrent_tasks = 4
restart_delay_secs = 5
cwd = "/var/lib/fleet-worker"        # 선택
```

### 시작 시퀀스

```text
[WorkerRunner::run]
  │
  ├── 1. GrokRunner::new(config) → (runner, shutdown_tx)
  │      tokio::spawn(runner.run())
  │        └─ spawn grok agent serve --bind ... --secret ...
  │           (exit 시 restart_delay_secs 후 재시작, 최대 10회)
  │
  ├── 2. RegistrationClient::register_with_retry()
  │      POST /v1/workers/register  (5초 간격 무한 재시도)
  │      → worker_id, heartbeat_interval_secs 반환
  │
  ├── 3. tokio::spawn(run_heartbeat_loop)
  │      주기마다:
  │        ├─ TCP health_check(bind_addr)  (1초 타임아웃)
  │        ├─ collect_system_metrics (sysinfo: load/mem/disk)
  │        └─ POST /v1/workers/heartbeat   { worker_id, agent_healthy, ... }
  │
  └── 4. wait_for_signal()  (SIGINT/SIGTERM)
           ↓
         shutdown_tx.send(true)  +  grok_shutdown_tx.send(true)
           ├─ grok 서브프로세스 SIGTERM (5s timeout → SIGKILL)
           ├─ heartbeat 루프 종료 (5s timeout)
           └─ POST DELETE /v1/workers/:id  (best-effort deregister)
```

### 프로비저닝 통합

`fleet-provisioner`의 `InstallFleetWorker` 스텝이 worker.toml을 렌더링하여
원격 서버의 `/etc/fleet/worker.toml`로 배포. `TemplateContext`에 추가된 필드:

- `grok_secret` — 필수 (`[grok] secret`)
- `grok_bin`, `grok_bind_addr`, `max_concurrent_tasks`, `restart_delay_secs`, `grok_cwd` — 선택 (기본값 존재)
- `bootstrap_token` — 선택 (`[worker] bootstrap_token`)
- `labels` — TOML inline table로 정렬 직렬화

`StepContext`에 추가된 필드:
- `grok_secret: Option<String>`
- `grok_bind_addr: Option<String>`
- `max_concurrent_tasks: Option<u32>`
- `bootstrap_token: Option<String>`

CLI (`fleet provision`)는 `--grok-secret`, `--bootstrap-token` 플래그와 환경변수
(`FLEET_GROK_SECRET`, `FLEET_BOOTSTRAP_TOKEN`)를 지원. 인벤토리 YAML의 각 워커에
`grok_secret:` 필드를 per-worker로 지정 가능.

### 제한 (Phase 8.1 → 8.2 현재)

- ~~WebSocket 재연결 미구현 — orchestrator→worker ACP 연결이 끊기면 task 실패~~ → **Phase 8.2에서 supervisor 기반 자동 재연결 구현**
- ~~단일 세션 per worker — 동시 task 처리 불가 (Phase 8.4)~~ → **Phase 8.4에서 per-worker 동시 다중 세션 구현** (아래 "동시 다중 세션 (Phase 8.4)" 절 참조)
- ~~mTLS 미지원 — Cloudflare Tunnel에 위임 (Phase 8.5)~~ → **Phase 8.5에서 사설 CA 기반 mTLS 구현** (아래 "mTLS for Orchestrator↔Worker ACP 트래픽 (Phase 8.5)" 절 참조)
- 시스템 메트릭의 `active_tasks`는 항상 0 — Phase 8.4에서 동시성 도입 시 실제 카운트 (개선 후보)

## WebSocket Reconnection (Phase 8.2)

Phase 7의 `AcpTransport`는 단일 WebSocket 연결만 유지했기 때문에, 네트워크 끊김,
워커 재시작, `grok agent serve` 크래시 등이 발생하면 진행 중인 태스크가 실패하고
해당 워커는 수동으로 다시 `register` 해야 했습니다. Phase 8.2는 **per-worker
supervisor 태스크**를 도입해 자동 복구를 제공합니다.

### Supervisor 패턴

각 워커는 `register()` 시점에 전용 supervisor `tokio::task`를 얻습니다. supervisor는
다음 루프를 반복합니다:

```text
loop {
  set state = Connecting
  establish_session() ─► (AcpClient, SessionId, event_rx)
    │ 실패:
    │    set state = Disconnected
    │    if 첫 연결: register()에 Err 전파 (caller가 인지)
    │    wait_with_shutdown(backoff)  ← shutdown 신호면 루프 탈출
    │    backoff = min(backoff * 2, 30s)
    │    continue
    │ 성공:
    │    set state = Connected, backoff = 1s 리셋
    │    if 첫 연결: register()에 Ok 전파
    │    spawn reader_loop (event_rx → WorkerEvent broadcast)
    │
    tokio::select! {
      cmd = cmd_rx.recv()  ─► Shutdown / Other → 루프 탈출
      _ = reader_handle    ─► ReaderExited → 재연결 루프로
    }
}
```

### 상태 머신 (`ConnState`)

| 상태           | 의미                                             | dispatch 동작        |
|----------------|--------------------------------------------------|----------------------|
| `Connecting`   | supervisor가 초기/재 연결을 시도 중               | `Err(Connection)`    |
| `Connected`    | WebSocket이 열려 있고 `session/new` 까지 완료됨  | 정상 dispatch 가능    |
| `Disconnected` | reader가 종료됨 — 백오프 대기 또는 곧 재시도      | `Err(Connection)`    |

`AcpTransport::conn_state(worker_id)` 로 외부에서 조회 가능. `is_connected()`는
`state == Connected` 인지 여부만 반환.

### 지수 백오프

| 시도     | 대기 시간 |
|----------|-----------|
| 1        | 1s        |
| 2        | 2s        |
| 3        | 4s        |
| 4        | 8s        |
| 5        | 16s       |
| 6+       | 30s (상한) |

연결이 한 번이라도 성공하면 `backoff`는 다시 1s로 리셋됩니다. 상수
`RECONNECT_INITIAL` (1s), `RECONNECT_MAX` (30s) 와 `ReconnectConfig` 구조체로
테스트에서 임의 값을 주입 가능 (`AcpTransport::with_reconnect`).

### 진행 중 태스크 처리

reader가 종료되면 (WebSocket Close 프레임, I/O 에러, `grok agent serve` 종료 등)
supervisor는 `fail_active_task()`를 호출합니다:

- 현재 `active_task: Option<TaskId>` 를 take.
- `WorkerEvent::Failed { task_id, error: "ACP reader exited (connection lost)" }` 를 broadcast.
- `active_prompt`도 초기화.

이후 재연결 시 동일한 `task_id`가 재실행되지는 않습니다 — dispatcher/사용자가 새로
`submit_task`를 해야 합니다 (idempotent 재시도는 상위 레이어에서 담당).

### Shutdown 시퀀스

`unregister(worker_id)` → `WorkerSession::drop` → `cmd_tx.send(Shutdown)` +
`supervisor.abort()`. 백오프 도중에도 `cmd_rx.recv()`를 `tokio::time::timeout`으로
경쟁시키기 때문에 최대 `backoff` 이내로 종료됩니다 (테스트 `unregister_during_backoff_exits_cleanly`).

### Reader 종료 감지 핵심

`ClientInner.event_tx`를 `std::sync::Mutex<Option<UnboundedSender<AcpEvent>>>`로
변경했습니다. `reader_loop()` 종료 시점에 `close_event_channel()`을 호출해 내부
sender를 drop하면, supervisor가 소유한 외부 `event_rx`의 `recv()`가 `None`을
반환하며 reader 태스크가 자연스럽게 끝납니다. 이렇게 하면 WebSocket Close
프레임 감지에만 의존하지 않고, AcpClient 내부의 어떤 종료 경로(`close()` 호출,
에러 전파, drop)에도 supervisor가 반응할 수 있습니다.

## Bootstrap Token & Worker Join (Phase 8.3)

Phase 7/8.1의 등록 흐름은 bearer 토큰을 `--api-tokens`로 정적 설정해야 했고,
워커 머신에는 미리 렌더링된 `worker.toml`을 SSH로 배포해야 했습니다. Phase 8.3는
**상태 저장형 부트스트랩 토큰**과 **`fleet-worker join` CLI**를 도입하여 셀프
서비스 등록 경로를 추가합니다.

### 데이터 모델

```sql
CREATE TABLE bootstrap_tokens (
    token           TEXT PRIMARY KEY,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by      TEXT,
    expires_at      TIMESTAMPTZ,
    max_uses        INTEGER NOT NULL DEFAULT 1,
    use_count       INTEGER NOT NULL DEFAULT 0,
    notes           TEXT,
    last_used_by    TEXT,
    last_used_at    TIMESTAMPTZ
);
```

- `max_uses` — 다회용 토큰 지원 (기본 1 = 일회성).
- `use_count` — atomic UPDATE 로 증가.
- `expires_at` — 선택적 만료.

### API 엔드포인트

| Method   | Path                              | 용도                              |
|----------|-----------------------------------|-----------------------------------|
| POST     | `/v1/bootstrap-tokens`            | 토큰 발급 (어드민 전용)            |
| GET      | `/v1/bootstrap-tokens`            | 발급된 토큰 목록 조회              |
| DELETE   | `/v1/bootstrap-tokens/:token`     | 토큰 회수                         |
| POST     | `/v1/workers/join`                | 토큰으로 워커 자동 등록 + config 반환 |

`/v1/workers/join` 은 `/v1/workers/register` 와 달리:

1. **요청 본문의 `token`** 을 `Store::consume_bootstrap_token`으로 atomic 검증
   (인증 미들웨어의 bearer 와 별개).
2. 동일 name 이 존재하면 **409 Conflict** — join은 항상 신규. 재등록은 `/register`.
3. 응답에 **`worker_config_toml`** 문자열 포함. 클라이언트가 디스크에 바로 기록.
4. 부트스트랩 토큰은 성공 시 `use_count += 1`. `last_used_by` 에 worker name 기록.

### Atomic 소비

`consume_bootstrap_token(token, used_by)` 는 단일 UPDATE 문으로 race condition
방지:

```sql
UPDATE bootstrap_tokens
   SET use_count = use_count + 1,
       last_used_by = $2,
       last_used_at = NOW()
 WHERE token = $1
   AND use_count < max_uses
   AND (expires_at IS NULL OR expires_at > NOW())
RETURNING token;
```

영향받은 행이 0이면 토큰이 (a) 존재하지 않거나, (b) 소진되었거나, (c) 만료된 것.
핸들러는 이를 `401 Unauthorized` 로 매핑.

### CLI (fleet)

```bash
# 1. 어드민이 토큰 발급 (DB 저장).
fleet token issue --api-url https://fleet.example.com \
                  --api-token $ADMIN_TOKEN \
                  --max-uses 1 \
                  --expires-in-secs 3600
# → fleet_ABCD...

# 2. 발급된 토큰 목록.
fleet token list --api-url https://fleet.example.com --api-token $ADMIN_TOKEN

# 3. 회수.
fleet token revoke fleet_ABCD... --api-url ... --api-token $ADMIN_TOKEN
```

기존 `fleet token new` (로컬 난수 생성, DB 미사용)는 하위 호환을 위해 유지.
신규 배포에서는 추적/회수 기능이 있는 `token issue` 권장.

### fleet-worker join

워커 머신에서 실행하는 셀프 서비스 등록:

```bash
fleet-worker join \
  --orchestrator-url https://fleet.example.com \
  --token fleet_ABCD... \
  --name build-farm-1 \
  --labels arch=arm64,gpu=false \
  --config-out /etc/fleet/worker.toml \
  --start
```

흐름:
1. `validate_worker_name` 로 DNS-safe 검증.
2. `--grok-secret` 미지정 시 32바이트 CSPRNG 난수 생성.
3. `--agent-endpoint` 미지정 시 orchestrator 호스트 기반으로 자동 유도
   (`ws://<orchestrator-host>/ws?server-key=<secret>`).
4. `POST /v1/workers/join` 호출.
5. 응답의 `worker_config_toml` 을 `--config-out` 경로에 **atomic** 으로 기록
   (tmp 파일 작성 후 rename).
6. `--start` 시 현재 프로세스를 `fleet-worker --config <path>` 로 **exec**.

### worker.toml 자동 생성

`/v1/workers/join` 응답의 `worker_config_toml`은 다음 필드를 포함합니다:

- `[worker] name`, `orchestrator_url` (플레이스홀더), `heartbeat_interval_secs`,
  `bootstrap_token`, `existing_worker_id` (이후 재시작 시 동일 ID 유지),
  `labels`.
- `[grok] bin`, `bind_addr` (endpoint에서 추출), `secret` (endpoint에서 추출),
  `max_concurrent_tasks`, `restart_delay_secs`.

이로써 어드민은 `worker.toml`을 미리 렌더링해서 SSH로 배포할 필요 없이, 토큰
하나만 전달하면 워커 운영자가 직접 `fleet-worker join` 한 줄로 등록 완료.

## 동시 다중 세션 (Phase 8.4)

Phase 7/8.2의 `AcpTransport`는 워커당 **단일 활성 세션**만 유지했습니다. 즉,
워커에서 처리 중인 `session/prompt` 응답이 도착하기 전에는 두 번째 dispatch를
시도할 수 없었고, 캐퍼시티가 큰 워커의 자원을 활용할 수 없었습니다. Phase 8.4는
**per-worker 동시 다중 세션**을 추가합니다.

### WorkerSession 데이터 모델

```rust
struct WorkerSession {
    worker_id: WorkerId,
    endpoint: String,
    max_concurrent: u32,
    in_flight: Mutex<HashMap<TaskId, InFlightTask>>,
    prompt_index: Mutex<HashMap<PromptId, TaskId>>,   // 역방향 조회
    pending_events: Mutex<HashMap<PromptId, Vec<BufferedEvent>>>,
    // supervisor, AcpClient, ...
}

struct InFlightTask {
    prompt_id: Option<PromptId>,   // session/prompt 응답 도착 전까지 None
    started: Instant,
}

enum BufferedEvent {
    Output { seq: u64, chunk: String },
    Failed { error: String },
}
```

- `in_flight` — 워커에서 진행 중인 모든 task. `prompt_id`는 세션 생성 직후
  `None`이며, `session/prompt` 응답이 도착한 후 `set_prompt_id`로 채워집니다.
- `prompt_index` — 수신 이벤트의 `promptId` → `TaskId` 역방향 조회.
- `pending_events` — 드물지만 `session/update` notification이 `session/prompt`
  응답보다 먼저 도착하는 race를 흡수. `set_prompt_id` 호출 시점에 drain되어
  dispatch가 처리합니다.

### dispatch 흐름

```text
dispatch(req)
 ├─ session = sessions.get(worker_id)
 ├─ session.try_acquire(task_id)           ← max_concurrent 검사
 │    └ Err(WorkerAtCapacity) → return
 ├─ spawn {
 │     match acp.prompt(...) {
 │       Ok(prompt_id) =>
 │         let buffered = session.set_prompt_id(task_id, prompt_id);
 │         for ev in buffered { emit(ev); }   ← 레이스로 밀린 이벤트 처리
 │       Err(e) =>
 │         if e.contains("ACP connection closed") {
 │           // reader_loop 드레인으로 발생한 에러 → supervisor의 fail_all에 위임
 │         } else {
 │           if session.complete(task_id).is_some() {
 │             emit(Failed { error: e });     ← 내가 emit 소유권
 │           } // else: 이미 fail_all이 처리함
 │         }
 │     }
 │   }
 └─ Ok(())
```

- `try_acquire`가 `max_concurrent` 검사 후 `TaskId` 슬롯을 사전에 점유합니다.
- `complete()`가 `Option<InFlightTask>`를 반환하는 atomic 패턴으로 dispatch와
  supervisor `fail_all` 사이의 Failed emit 소유권 경쟁을 해소합니다.

### 용량 강제 (WorkerAtCapacity)

`WorkerTransport::register`가 세 번째 인자 `max_concurrent_tasks: u32`를 받도록
**breaking change**되었습니다 (`fleet-api/src/handlers.rs::upsert_and_register`는
`worker.max_concurrent`를 전달).

```rust
#[error("worker {0} is at capacity (max_concurrent_tasks reached)")]
WorkerAtCapacity(String),
```

- dispatch는 `try_acquire` 실패 시 즉시 `Err(WorkerAtCapacity)`를 반환하고,
  핸들러는 503 또는 retry를 선택할 수 있습니다.
- `MockTransport`도 동일한 `max_concurrent_tasks` 시맨틱을 흉내내어 테스트가
  실제 transport와 동일한 계약을 검증합니다.

### Reader 라우팅 (promptId 기반)

`run_reader_loop`는 WebSocket에서 읽은 각 메시지의 `promptId`를 `prompt_index`로
역조회하여 대상 `TaskId`를 찾습니다.

- 알려진 `promptId` → 해당 task로 Output/Failed 이벤트 emit.
- 알려지지 않은 `promptId` → `pending_events`에 buffer. `set_prompt_id` 호출
  시점에 drain.
- `complete()`는 `in_flight`에서 제거하면서 동시에 `prompt_index`에서도
  `prompt_id` 매핑을 정리합니다.

### fail_all 시맨틱 (연결 손실)

`AcpClient::reader_loop`가 종료되면 supervisor는 모든 in-flight task를 실패로
처리해야 합니다. 이 과정에서 두 가지 경쟁 상태가 발생합니다.

1. **dispatch의 prompt() 에러 vs supervisor의 fail_all** — reader_loop가
   pending request를 `RpcError { code: ACP_ERR_CONNECTION_CLOSED (-32001),
   message: "ACP connection closed" }`로 drain하므로, dispatch는 이 에러 코드를
   감지하고 supervisor의 `fail_all`에 위임합니다 (자체 Failed emit 생략).
2. **dispatch의 Failed emit vs supervisor의 fail_all (일반 실패)** —
   `complete(task_id)`가 `Some`을 반환하면 dispatch가 emit을 소유, `None`이면
   supervisor가 이미 처리.

`WorkerSession::fail_all(self: &Arc<Self>, broadcaster, reason)`는
`in_flight.drain()` 후 각 task에 대해 `Failed { reason }`을 emit합니다.

### Selector 용량 필터

`WorkerSelector`는 후보 워커 집계 후 `candidates.retain(|w| w.has_capacity())`
로 용량이 남은 워커만 선택합니다. heartbeat 기반 eventual consistency이며,
transport의 `try_acquire`가 최종 권위를 가집니다 (필터가 통과시킨 후에도
`WorkerAtCapacity`가 발생할 수 있음 — 그 경우 상위 핸들러가 재시도).

### API/관측 지원

- `AcpTransport::in_flight_count(worker_id)` / `max_concurrent(worker_id)` —
  디버그/대시보드용 노출.
- `Worker` 레코드의 `max_concurrent_tasks`가 `register`로 흘러 들어가므로
  DB → API → transport까지 일관된 단일 진실 공급원.

## mTLS for Orchestrator↔Worker ACP 트래픽 (Phase 8.5)

Phase 7/8.1의 ACP 연결은 평문 WebSocket (`ws://`) + URL 쿼리로 전달되는
`server-key` 만으로 보호되었다. Cloudflare Tunnel을 거치는 구간은 전송 구간
암호화가 되지만, 직접 노출된 네트워크(LAN, VPC peering, 온프렘)에서는 ACP
트래픽이 스니핑/변조 가능했다. Phase 8.5는 **사설 CA 기반 mTLS**로 이 구간을
보호한다.

### 아키텍처

```text
   orchestrator                                   worker machine
   ────────────                                   ─────────────
                                                   ┌────────────────────────────┐
   ┌──────────────┐    wss:// (mTLS, 사설 CA)      │ fleet-worker               │
   │ AcpTransport ├──────────────────────────────► │  └─ MtlsProxy (0.0.0.0:2420)│
   │  (ClientTLS) │   클라이언트 인증서 제출        │       │ TLS 종단 + 검증    │
   └──────────────┘                                │       ▼ 평문 TCP 복사      │
                                                   │  grok agent serve (:2419)  │
                                                   │  (loopback only)           │
                                                   └────────────────────────────┘
```

`grok agent serve`는 외부 바이너리라 mTLS를 직접 지원할 수 없다. 그래서
`fleet-worker`가 proxy 모드로 동작해 **TLS 종단 + 클라이언트 인증서 검증**을
수행하고, 통과한 연결을 loopback의 grok 으로 평문 TCP 복사한다.

### 클라이언트 측 (orchestrator)

- **`ClientTlsConfig`** (`fleet-transport/src/tls.rs`) — 사설 CA PEM + 클라이언트
  인증서 PEM + 클라이언트 키 PEM.
- **`WsConn::connect_mtls(url, &ClientTlsConfig)`** — tokio-tungstenite 의
  `Connector::Rustls(Arc<ClientConfig>)` 주입. 핸드셰이크 시 클라이언트 인증서
  제출, 사설 CA로 서명된 서버 인증서만 신뢰.
- 워커 엔드포인트 URL이 `wss://` 인 경우에만 사용. `ws://` 엔드포인트는 기존
  평문 경로 유지 (하위 호환).

### 워커 측 (fleet-worker)

- **`ServerTlsConfig`** — 사설 CA + 서버 인증서/키 PEM 으로 rustls `ServerConfig`
  빌드. `WebPkiClientVerifier` 로 사설 CA로 서명된 클라이언트 인증서만 통과.
- **`MtlsProxy`** (`fleet-transport/src/mtls_proxy.rs`):
  - `bind(addr, upstream, server_config).await` — 미리 bind. `local_addr()` 로
    바인딩된 주소 조회 가능 (라우팅/테스트).
  - `run(self, shutdown_rx)` — `watch::Receiver<bool>` 로 graceful shutdown.
    단일 연결 실패는 격리.
  - 양방향 복사: `tokio::io::copy_bidirectional` (내부적으로 `io::split` + 2 copy).
- **`[mtls]` worker.toml 섹션** (`MtlsSection`):
  - `enabled` / `listen_addr` / `server_cert_path` / `server_key_path` /
    `client_ca_path` / `advertised_host` / `advertised_port`.
  - `advertised_host` 가 없으면 orchestrator_url 의 호스트 사용.
- **`WorkerConfig::agent_endpoint()`** — mTLS 활성 시 `wss://<host>:<port>/ws?...`
  반환. 등록 응답이 이 URL을 노출하면 orchestrator의 `AcpTransport`가 자동으로
  mTLS 경로를 사용.
- **`WorkerRunner::run`**: grok이 bind_addr에 바인딩될 때까지 폴링 후 proxy spawn.
  shutdown 시 heartbeat/grok 과 함께 cleanup.

### 인증서 발급 흐름 (Phase 8.5.3)

```bash
# 1. 사설 CA 발급 (1회성).
fleet mtls init-ca --common-name "Fleet Internal CA" --out /etc/fleet/ca/

# 2. 각 워커의 서버 인증서 발급.
fleet mtls issue-server --ca /etc/fleet/ca \
                       --common-name worker-1 \
                       --dns worker-1.fleet.local,localhost \
                       --out /etc/fleet/worker-1/

# 3. orchestrator의 클라이언트 인증서 발급.
fleet mtls issue-client --ca /etc/fleet/ca \
                       --common-name orchestrator \
                       --out /etc/fleet/orchestrator/
```

### 보안 속성

- **기밀성**: TLS 1.3 (ring provider, AES-256-GCM). ACP 패킷이 중간자에 의해
  스니핑되지 않음.
- **서버 신원**: 사설 CA로 서명된 서버 인증서만 orchestrator가 신뢰. 공용 CA나
  self-signed는 거부.
- **클라이언트 신원**: 사설 CA로 서명된 클라이언트 인증서만 worker proxy가
  통과시킴. 인증서 없거나 다른 CA로 서명된 연결은 핸드셰이크 단계에서 거부.
- **server-key는 여전히 유효**: mTLS로 인증된 연결도 `?server-key=` URL 쿼리가
  필요 (grok agent serve 자체의 인증). mTLS는 전송 보호 + 클라이언트 신원
  증명일 뿐, ACP 애플리케이션 계층 인증은 별개.

### 제한

- 인증서 자동 회전 미지원. CA/서버 인증서 만료 전 수동 갱신 필요
  (`ClientTlsConfig` 는 매 핸드셰이크마다 파일을 다시 읽으므로 갱신 후 프로세스
  재시작 불필요, 하지만 fleet-worker의 proxy는 시작 시 한 번만 읽음).
- CRL/OCSP 미지원. 폐기된 클라이언트 인증서는 CA 자체를 교체하지 않는 한
  유효. (대안: mTLS + Cloudflare Access 동시 사용.)
- 인증서 발급 CLI (`fleet mtls`)는 Phase 8.5.3에서 추가.

