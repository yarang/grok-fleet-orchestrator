# 아키텍처

이 문서는 Grok Fleet Orchestrator의 내부 구조, 데이터 흐름, 핵심 설계 결정을
설명합니다. 배포 가이드는 [`deployment.md`](../deployment/deployment.md), API 레퍼런스는
[`api-reference.md`](api-reference.md)를 참조하세요.

## TL;DR

![System Architecture Flowchart](../assets/diagrams/architecture/system-architecture-flow.mermaid)

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
- 현재 구현: `PgStore` (PostgreSQL + sqlx) — `fleet-store` 크레이트 내 유일한 `impl Store`.
- 테스트용: `MemStore` — ⚠️ **정정 (2026-08-12)**: "crate-private"라는 서술은 부정확합니다.
  `fleet-store` 크레이트 안에 정식으로 사는 단일 타입이 아니라, `fleet-api`/
  `fleet-dashboard` 등 여러 크레이트의 `#[cfg(test)]` 전용 테스트 코드에 **독립적으로
  중복 정의된 6개 이상의 각기 다른 `struct MemStore`**입니다(예: `fleet-api/src/
  test_support.rs`, `fleet-api/tests/*.rs`, `fleet-dashboard/src/app.rs` 등). 프로덕션
  빌드에는 전혀 포함되지 않습니다.
- 향후 가능: SQLite (싱글노드), DynamoDB (AWS) — 코드 전수 검색 결과 관련 구현 0건,
  순수 아이디어 단계입니다.

이 추상화 덕분에 (a) 단위 테스트가 Postgres 없이 동작하고 (b) 다른 백엔드로의
교체가 비교적 쉽습니다.

### 4. PostgreSQL LISTEN/NOTIFY로 다중 admin 동기화

여러 관리자가 동시에 `fleet serve`를 띄운 경우, 한 admin이 회로를 열면
다른 admin에게도 즉시 전파되어야 합니다. 트랜잭션 범위 밖에서 pub/sub 채널을
사용하면 이 동기화를 최소 지연으로 달성할 수 있습니다.

구체적 메커니즘 (⚠️ 2026-08-12 정정 — 핵심 주장은 정확하지만 세부 명칭이 틀렸습니다):

1. Admin A가 `worker_circuit_changed` 이벤트(⚠️ `circuit_opened`가 아닙니다 — 실제
   `FleetEvent` variant의 `event_type()`은 `"worker_circuit_changed"`)를 `events`
   테이블(⚠️ `fleet_events`가 아닙니다 — `fleet_events`는 테이블명이 아니라 NOTIFY
   **채널명**입니다)에 INSERT
2. `events` 테이블의 `AFTER INSERT` 트리거가 `pg_notify('fleet_events', NEW.seq::text)`
   실행 (`migrations/001_init.sql`)
3. Admin B의 `PgListener::listen("fleet_events")`가 즉시 알림 수신
4. Admin B의 로컬 `CircuitBreaker::force_open()`이 호출되어 강제 open 전환
   (`crates/fleet-scheduler/src/sync.rs`의 `MultiAdminSync::apply_one_to()` —
   테스트 `circuit_open_event_forces_local_breaker_open`로 검증됨)

> ✅ **정정 (2026-08-13)**: 위 메커니즘은 유닛 테스트(`sync.rs`)와 스케일아웃
> 통합 테스트(`fleet-scheduler/tests/scaleout_sync.rs`)로는 오래전부터 검증돼
> 있었지만, **실제 `fleet serve` 기동 경로(`crates/fleet-cli/src/runtime.rs`)에는
> 한 번도 연결된 적이 없었습니다** — `MultiAdminSync`가 어디서도 `spawn`되지
> 않아, 스케일아웃 배포에서 한 인스턴스가 워커를 CircuitOpen 시켜도 다른
> 인스턴스는 스스로 실패를 겪기 전까지 이를 몰랐습니다(로드맵 #25). `run_serve`가
> `HealthChecker`/`Reconciler`/`SessionCleanup`을 기동할 때와 같은 자리에서
> `MultiAdminSync`도 함께 `spawn`하도록 연결했고, 신규 CLI 플래그
> `--no-circuit-sync`로 끌 수 있습니다(기본값: 활성 — 형제 플래그
> `--no-health-check`/`--no-cleanup`/`--no-reconcile`와 동일하게 env var는 없음).

### 5. CircuitBreaker 3-state 머신

각 워커마다 독립적인 회로차단기:
- **Closed**: 정상. 모든 dispatch 시도 통과.
- **Open**: 최근 N회 연속 실패. dispatch 즉시 거부 (`CircuitOpen` 에러).
- **HalfOpen**: 쿨다운(⚠️ 정정: 기본 30초가 아니라 **10초** —
  `CircuitBreakerConfig::default().open_duration_secs`, `crates/fleet-core/src/config.rs`)
  후 프로브 허용. ⚠️ **정정 (2026-08-12)**: "1회 프로브만 허용"은 사실이 아닙니다 —
  `breaker.rs`의 `check()`는 `HalfOpen` 상태에서 **무조건 허용**합니다(코드 주석: "단순화:
  HalfOpen에서는 항상 허용"). `half_open_max_probes` 설정 필드(기본값 1)는 존재하지만
  `breaker.rs` 어디에서도 실제로 읽히지 않아 — HalfOpen 동안 동시 요청이 와도 1개로
  제한되지 않습니다. 성공 시 Closed, 실패 시 Open 복귀는 정확합니다.

이 패턴은 [Grok Build의 `CircuitBreakerRegistry`](https://github.com/xai-org/grok-build)에서
차용했지만, 워커별로 키를 관리하도록 재구현했습니다.

### 6. WorkerSelector: hint + label + model + least-loaded

![Worker Selector Logic Diagram](../assets/diagrams/architecture/worker-selector-logic.mermaid)

grok의 ACP 프로토콜은 실행 중인 세션의 모델을 동적으로 바꿀 수 없으므로
(`session/new`/`session/prompt`에 `model` 파라미터 없음), 모델 선택은 오직
"어느 워커로 보낼 것인가"로만 구현됩니다. 워커를 `model=<slug>` 라벨로
등록해 그 워커의 `grok agent serve` 프로세스가 실제로 어떤 백엔드로
설정되어 있는지 표시하면(예: `worker.toml`의
`[worker] labels = { model = "gemini" }` 또는
`fleet-worker join --labels model=gemini,...`), `fleet_dispatch_task`의
`model` 파라미터가 그 라벨과 정확히 일치하는 워커로만 라우팅합니다.

### 7. 비동기 장기 실행 작업 모델

작업은 `TaskStatus` 5개 상태(`Pending`/`Dispatched`/`Completed`/`Failed`/`Cancelled`,
`crates/fleet-core/src/task.rs`)를 따르는 상태머신입니다(⚠️ 2026-08-12 정정: 이전
판은 "4단계"라 서술했으나 실제로는 5개 상태입니다):

![Scheduler Task State Diagram](../assets/diagrams/architecture/scheduler-task-state.mermaid)

⚠️ 위 다이어그램은 2026-08-12에 정정되었습니다. 이전 판은 `Completed → Cancelled`
전이를 표시했으나, `Dispatcher::cancel()`은 이미 종료 상태(`is_terminal()` —
`Completed`/`Failed`/`Cancelled` 포함)인 작업에 대해 `CancelError::AlreadyTerminal`을
반환해 이 전이를 명시적으로 거부합니다(`dispatcher.rs`). 반대로 `Pending → Failed`
(제출 시점 워커 선택/dispatch 자체가 실패하는 경우)와 `Pending → Cancelled`
(디스패치 전 취소)는 실제로 발생하는 전이인데 이전 판 다이어그램에는 없었습니다.

- `fleet_dispatch_task`는 `task_id`만 반환하고 즉시 리턴 (non-blocking)
- `fleet_wait_for_task` (또는 `fleet_stream_task_output`)로 결과를 폴링
- `fleet_cancel_task`로 사용자 주도 취소
- 워커 장애 시 Dispatcher가 `Failed(kind=WorkerUnavailable)`로 표시
- `submit()`은 제출 시점에 딱 한 번만 워커 선택/dispatch를 시도하므로, 그
  시도가 터미널 상태에 도달하기 전에 오케스트레이터가 재시작되면 작업이
  `Pending`에 고아로 남을 수 있다 — 백그라운드 재조정(reconciliation) 루프
  (`fleet-scheduler::reconcile::Reconciler`, `--reconcile-*` CLI 플래그)가
  주기적으로 stale `Pending` 작업을 다시 훑어 재dispatch를 시도한다.

## 데이터 모델

### 테이블 (PostgreSQL 스키마)

⚠️ **정정 (2026-08-12)**: 아래 표는 실제 테이블명과 다르고(`fleet_` 접두사 없음이 코드
전반의 컨벤션 — 예: `workers`, `tasks`, `hosts`, `ssh_keys`, `bootstrap_tokens`),
개수도 크게 과소 서술되어 있습니다. `crates/fleet-store/migrations/`에는
`001`~`013`까지 13개 마이그레이션이 존재하며 RBAC(역할/권한), 호스트 인벤토리,
이메일 인증, 비밀번호 재설정, SSH 키 금고, 감사 로그, 태스크 스레드 등을 추가로
다룹니다. 아래 표는 핵심 4종만 남겨두되 이름을 정정합니다 — `events`/`output` 계열의
정확한 테이블명은 재검증이 필요합니다.

| 테이블(추정 — 재검증 필요)          | 용도                                            |
|-----------------|-------------------------------------------------|
| `workers`       | 등록된 워커 (id, name, endpoint, status, labels, circuit_state) |
| `tasks`         | 작업 (id, prompt, status, server_hint)          |
| (이벤트 로그 테이블) | append-only 이벤트 로그 (seq, event JSONB)  — 정확한 테이블명 재검증 필요 |
| (출력 청크 테이블)   | 작업 stdout/stderr 청크 (task_id, seq, chunk) — 정확한 테이블명 재검증 필요 |

모든 마이그레이션은 `crates/fleet-store/migrations/`에 idempotent SQL 파일로
존재하며, `fleet migrate` 또는 서버 시작 시 자동 적용됩니다.

### 이벤트 로그가 곧 감사 로그

`events` 테이블(⚠️ 정정: `fleet_events`가 아닙니다 — 그건 NOTIFY 채널명, 위 §4 참조)이
감사 로그 역할을 동시에 수행합니다:
- 모든 상태 변화는 트랜잭션과 함께 이벤트로 기록
- `fleet events list --after-seq <N> --limit <N> [--json]` CLI로 조회 (확인됨,
  `crates/fleet-cli/src/main.rs`)
- 대시보드의 `/api/events/stream` SSE가 LISTEN/NOTIFY로 실시간 푸시
- Prometheus `fleet_events_written_total` 메트릭 노출. ⚠️ 정정: "단조 증가 카운터"라고
  서술했지만 Prometheus **타입은 `gauge`**입니다(`# TYPE fleet_events_written_total
  gauge`, `crates/fleet-api/src/metrics.rs`) — 값 자체(관측된 최대 seq)는 단조 증가
  하는 게 맞지만, 메트릭 타입 라벨은 counter가 아닙니다.

## 인증 모델 (3계층)

| 레이어                | 매커니즘                          | 적용 대상                  |
|-----------------------|-----------------------------------|----------------------------|
| Cloudflare Access     | CF-Access-Jwt-Assertion (JWT)     | 외부망 → 오케스트레이터     |
| Bearer Token (API)    | `Authorization: Bearer <token>`   | 오케스트레이터 내부 API     |
| No-auth (dev mode)    | 없음                              | `--api-tokens`/`--cf-audience` 둘 다 미지정 시 |

⚠️ **정정 (2026-08-12)**: `--allow-no-auth`라는 CLI 플래그는 **존재하지 않습니다.**
no-auth는 명시적으로 켜는 플래그가 아니라, `--api-tokens`와 `--cf-audience`를 둘 다
지정하지 않았을 때의 **암묵적 기본값**입니다(`AppState.allow_no_auth`가 기본 `true`이며
`.with_tokens()`/`.with_cf_audience()` 호출 시에만 `false`로 전환, `runtime.rs`에서
이 경우 "NO-AUTH mode (dev only)" 경고 로그를 남김).

Cloudflare Access의 JWT 서명/체인 검증은 실제로 구현되어 있습니다 —
`crates/fleet-api/src/cloudflare.rs`가 Cloudflare JWKS를 가져와 캐싱하고
`jsonwebtoken::decode`로 RS256 서명과 audience를 검증합니다. Bearer 토큰은 CLI
플래그명 `--api-tokens`(환경변수 `FLEET_API_TOKENS`)로 지정합니다.

운영 환경에서는 Cloudflare Access가 1차 방어선이고, bearer 토큰은
대시보드/모니터링 등 내부 시스템을 위한 2차 인증입니다.
자세한 내용은 [`deployment.md`](../deployment/deployment.md)를 참조하세요.

## 크로스 클라이언트 호환성

MCP 표준을 준수하므로, 동일한 `fleet serve` 인스턴스에 여러 AI 클라이언트가
동시에 연결할 수 있습니다:

```text
~/.config/grok/mcp.json          → fleet serve (stdio)
~/.claude/claude_desktop.json    → fleet serve (stdio)  [동일 바이너리]
~/.cursor/mcp.json               → fleet serve (stdio)
```

각 클라이언트 세션은 독립적이지만, 같은 워커 풀과 작업 큐를 공유합니다.
한 클라이언트가 제출한 작업을 다른 클라이언트가 `fleet_get_task_status`로 조회할
수도 있습니다.

## 성능 특성

- **동시 워커**: ⚠️ **정정 (2026-08-12)**: "PostgreSQL 커넥션 풀 크기(~100)까지 확장"은
  틀렸습니다 — 실제 기본 풀 크기는 **10**입니다(`PoolConfig::default().max_connections`,
  `crates/fleet-store/src/postgres.rs`; CLI `--db-max-conn` 플래그 기본값도 10,
  `fleet-cli/src/main.rs`). ~100이라는 수치의 근거는 코드 어디에도 없습니다.
- **작업 처리량**: 워커 당 `max_concurrent`(기본 4) × 워커 수 — 별도의 글로벌 세마포어가
  없으므로(§동시 실행 참조) 이 곱셈 프레이밍 자체는 타당합니다.
- **이벤트 로그**: LISTEN/NOTIFY 전파 지연 — ⚠️ "~1ms"라는 수치를 뒷받침하는 벤치마크나
  테스트를 코드에서 찾지 못했습니다. 검증되지 않은/근거 없는 수치로 표시합니다.
- **바이너리 크기**: release 프로필에 LTO(`lto = "thin"`)와 `strip = true`,
  `codegen-units = 1` 설정은 실제로 존재합니다(`Cargo.toml`). 다만 "~15MB"라는 구체
  수치를 측정/기록한 CI 단계나 문서는 찾지 못했습니다 — 검증되지 않은 수치로 표시합니다.

## 향후 로드맵

- ~~동시 다중 세션 per worker (현재는 직렬 prompt 처리; Phase 8.4)~~ → **Phase 8.4에서 per-worker 동시 다중 세션 구현** (아래 "동시 다중 세션 (Phase 8.4)" 절 참조)
- ~~mTLS for orchestrator↔worker ACP 트래픽 (Phase 8.5)~~ → **Phase 8.5.1/8.5.2/8.5.3에서 클라이언트/서버 mTLS + CLI 통합 구현** (아래 "mTLS for Orchestrator↔Worker ACP 트래픽 (Phase 8.5)" 절 참조).
- ~~SSH 프로비저닝의 호스트 키 검증 (기존 accept-all)~~ → **known_hosts 기반 TOFU/Strict 검증 구현** (아래 "SSH 호스트 키 검증" 절 참조)
- OIDC/JWKS 검증 (현재는 Cloudflare Access에 위임)
- 작업 우선순위 큐 +抢占 스케줄링
- 워커 오토스케일링 (로드 기반)
- 다중 리전 페더레이션
- **Autonomic Self-Healing Engine (MAPE-K) 재연결** — `crates/fleet-scheduler/src/autonomic.rs`가
  현재 타입과 어긋나 컴파일되지 않아 `lib.rs`/`runtime.rs`에서 비활성화된 상태(아래
  "Autonomic Self-Healing Engine" 절 참조). 타입 정합 후 재배선 필요.

## ACP Transport (Phase 7)

> ⚠️ **정정 (2026-08-12)**: 이 절과 이어지는 [§WebSocket Reconnection (Phase 8.2)](#websocket-reconnection-phase-82), [§동시 다중 세션 (Phase 8.4)](#동시-다중-세션-phase-84)는 **2026-08-11 이전** 설계를 서술합니다. 그날 `crates/fleet-transport/src/acp_transport.rs`가 손수 작성한 JSON-RPC/WebSocket 클라이언트에서 공식 [`agent-client-protocol`](https://github.com/Zed-Industries/agent-client-protocol) Rust SDK 기반으로 **전면 마이그레이션**되었습니다. 아래 세 절에 등장하는 `AcpClient`, `ClientInner`, `active_task`, `reader_loop()`, `AcpEvent`, promptId 기반 라우팅, `WorkerSession{in_flight, prompt_index, pending_events}` 구조체는 **더 이상 코드에 존재하지 않습니다.**
>
> **현재 구현 요약** (`crates/fleet-transport/src/acp_transport.rs`, 정본):
> - 세션은 **워커당 1개가 아니라 태스크당 1개**입니다 — `register()`는 WebSocket 연결 + `initialize` 핸드셰이크 + supervisor 태스크 기동(`spawn_supervisor()`)만 수행하고, `session/new`는 각 태스크가 `dispatch()`될 때마다 개별적으로 발급됩니다.
> - 동시성은 `capacity: Arc<Semaphore>`(워커의 `max_concurrent_tasks`로 사이징)로 제어하고, 진행 중 세션은 `sessions: Arc<Mutex<HashMap<SessionId, InFlightSession>>>`로 추적합니다.
> - 스트리밍 이벤트 라우팅은 promptId가 아니라 **`SessionId` 기반**입니다(`handle_session_notification`).
> - 연결 손실 시 `WorkerSession::fail_all()`이 모든 in-flight 세션을 드레인하며 `WorkerEvent::Failed`(메시지: `"ACP connection lost — will reconnect"`)를 브로드캐스트합니다 — 이전 판이 서술한 `fail_active_task()`/`active_task: Option<TaskId>`는 존재하지 않습니다.
> - 종료(`unregister()`)는 `tokio::sync::watch::Sender<bool>`로 신호를 보내고 최대 5초 `timeout`으로 대기합니다 — `supervisor.abort()` 호출은 없습니다.
> - 지수 백오프는 **연결 성공 후에도 초기값으로 리셋되지 않습니다** — 실패할 때만 2배씩 증가하는 단조 증가 카운터입니다(아래 §WebSocket Reconnection의 서술과 반대).
>
> 세 절 모두 "Phase N에서 왜 이렇게 설계했는가"를 보여주는 역사적 기록으로서 원문을 보존하되, 개별 오류는 아래에 인라인으로 표시합니다.

`AcpTransport`는 `WorkerTransport` trait의 실제 구현체로, [Agent Client Protocol](https://github.com/Zed-Industries/agent-client-protocol) (ACP) over WebSocket을 사용해 각 워커의 `grok agent serve`와 통신합니다.

### 아키텍처

![ACP Transport Lifecycle Sequence Diagram](../assets/diagrams/architecture/acp-transport-lifecycle.mermaid)

### ACP 메서드 지원

| Method           | 방향     | 용도                                   |
|------------------|----------|----------------------------------------|
| `initialize`     | req→res  | capabilities 교환 (protocolVersion=1)  |
| `session/new`    | req→res  | cwd로 세션 생성, sessionId 반환        |
| `session/prompt` | req→res  | 프롬프트 전송 + end_of_turn 시 결과    |
| `session/cancel` | req→res  | 진행 중 프롬프트 취소                  |
| `session/update` | notif    | 스트리밍 출력 (agent_message_chunk 등) |

### 동시성 모델

⚠️ **정정 (2026-08-12)**: 이 절은 Phase 7 시점(단일 세션/워커) 서술입니다. 2026-08-11 SDK
마이그레이션 이후 실제 동시성 모델은 `capacity: Arc<Semaphore>`(워커의
`max_concurrent_tasks`로 사이징) + 태스크당 1개 세션(`sessions: Arc<Mutex<HashMap<SessionId,
InFlightSession>>>`)입니다. `active_task: RwLock<Option<TaskId>>` 필드는 존재하지
않습니다. 상세는 위 배너와 [§동시 다중 세션](#동시-다중-세션-phase-84)의 정정 내용을
참조하세요.

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
  ├── src/join.rs           ← fleet-worker join: 셀프 서비스 등록 (Phase 8.3, 아래 절 참조)
  ├── src/runner.rs         ← WorkerRunner: 위 모듈 조립 + (선택)mTLS 프록시 기동 + 신호 처리
  └── src/error.rs          ← WorkerError enum
```

⚠️ **정정 (2026-08-12)**: 위 목록은 Phase 8.1 시점 기준이라 이후 추가된 `src/join.rs`(Phase 8.3)가
빠져 있었고, `runner.rs`가 Phase 8.5에서 mTLS 프록시(`MtlsProxy`) 기동/종료 책임까지
맡게 된 것도 반영돼 있지 않았습니다.

### `worker.toml` 형식

```toml
[worker]
name = "build-farm-1"
orchestrator_url = "https://fleet.agentthread.dev"
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

![Worker Daemon Execution Flowchart](../assets/diagrams/architecture/worker-daemon-execution.mermaid)

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

![Supervisor Backoff Logic Flowchart](../assets/diagrams/architecture/supervisor-backoff-logic.mermaid)

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

⚠️ **정정 (2026-08-12)**: 이 문단의 "연결 성공 시 backoff 1s 리셋" 서술은 **사실이 아닙니다.**
`spawn_supervisor()` 루프에서 `backoff` 변수는 오직 실패할 때만 2배씩 증가하며(`(backoff *
2).min(reconnect.max)`), 재연결에 성공해도 초기값으로 되돌리는 코드는 없습니다
(`crates/fleet-transport/src/acp_transport.rs` 전체 검색으로 확인). 즉 여러 번 실패 후
재연결에 성공해 한동안 잘 붙어 있다가 다시 끊기면, 1s가 아니라 이전에 도달했던 backoff
값(예: 8s/16s)부터 재시작합니다. 상수 `RECONNECT_INITIAL` (1s), `RECONNECT_MAX` (30s) 와
`ReconnectConfig` 구조체로 테스트에서 임의 값을 주입 가능 (`AcpTransport::with_reconnect`) —
이 부분은 정확합니다.

### 진행 중 태스크 처리

⚠️ **정정 (2026-08-12)**: 아래 3개 하위 절(진행 중 태스크 처리 / Shutdown 시퀀스 /
Reader 종료 감지 핵심)은 2026-08-11 SDK 마이그레이션 이전의 손수 작성 클라이언트
내부 구현을 서술합니다. `ClientInner`, `reader_loop()`, `close_event_channel()`,
`AcpEvent`, `active_task`, `active_prompt`, `fail_active_task()`, `cmd_tx`/`cmd_rx`는
현재 `crates/fleet-transport/src/acp_transport.rs`에 **존재하지 않습니다**(전체 검색
결과 0건). 원문은 설계 이력 보존을 위해 남겨두되, 실제 동작은 다음과 같습니다:

- 연결 손실 시 워커당 1개가 아니라 **`WorkerSession::fail_all()`이 모든 in-flight
  세션을 `sessions: Arc<Mutex<HashMap<SessionId, InFlightSession>>>`에서 드레인**하며,
  각각에 대해 `WorkerEvent::Failed`(메시지: `"ACP connection lost — will reconnect"`)를
  브로드캐스트합니다.
- 종료(`unregister()`)는 `tokio::sync::watch::Sender<bool>`로 신호를 보내고
  `tokio::time::timeout(Duration::from_secs(5), handle)`으로 최대 5초 대기합니다.
  `.abort()` 호출은 없습니다.
- Reader 종료 감지는 SDK가 제공하는 연결 상태 관리에 위임되며, `ClientInner`/
  `AcpEvent` 기반의 수동 채널 종료 메커니즘은 SDK 마이그레이션과 함께 제거되었습니다.

이후 재연결 시 동일한 `task_id`가 재실행되지는 않습니다 — dispatcher/사용자가 새로
`fleet_dispatch_task`를 해야 합니다 (idempotent 재시도는 상위 레이어에서 담당) —
이 결론 자체는 여전히 유효합니다.

<details>
<summary>원문 (Phase 8.2 시점, 2026-08-11 이전 구현 기준 — 참고용, 현재와 다름)</summary>

reader가 종료되면 (WebSocket Close 프레임, I/O 에러, `grok agent serve` 종료 등)
supervisor는 `fail_active_task()`를 호출합니다:

- 현재 `active_task: Option<TaskId>` 를 take.
- `WorkerEvent::Failed { task_id, error: "ACP reader exited (connection lost)" }` 를 broadcast.
- `active_prompt`도 초기화.

`unregister(worker_id)` → `WorkerSession::drop` → `cmd_tx.send(Shutdown)` +
`supervisor.abort()`. 백오프 도중에도 `cmd_rx.recv()`를 `tokio::time::timeout`으로
경쟁시키기 때문에 최대 `backoff` 이내로 종료됩니다 (테스트 `unregister_during_backoff_exits_cleanly`).

`ClientInner.event_tx`를 `std::sync::Mutex<Option<UnboundedSender<AcpEvent>>>`로
변경했습니다. `reader_loop()` 종료 시점에 `close_event_channel()`을 호출해 내부
sender를 drop하면, supervisor가 소유한 외부 `event_rx`의 `recv()`가 `None`을
반환하며 reader 태스크가 자연스럽게 끝납니다. 이렇게 하면 WebSocket Close
프레임 감지에만 의존하지 않고, AcpClient 내부의 어떤 종료 경로(`close()` 호출,
에러 전파, drop)에도 supervisor가 반응할 수 있습니다.

</details>

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
fleet token issue --api-url https://fleet.agentthread.dev \
                  --api-token $ADMIN_TOKEN \
                  --max-uses 1 \
                  --expires-in-secs 3600
# → fleet_ABCD...

# 2. 발급된 토큰 목록.
fleet token list --api-url https://fleet.agentthread.dev --api-token $ADMIN_TOKEN

# 3. 회수.
fleet token revoke fleet_ABCD... --api-url ... --api-token $ADMIN_TOKEN
```

기존 `fleet token new` (로컬 난수 생성, DB 미사용)는 하위 호환을 위해 유지.
신규 배포에서는 추적/회수 기능이 있는 `token issue` 권장.

### fleet-worker join

워커 머신에서 실행하는 셀프 서비스 등록:

```bash
fleet-worker join \
  --orchestrator-url https://fleet.agentthread.dev \
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
   (⚠️ 정정: `ws://<orchestrator-host>/ws?server-key=<secret>`가 아니라
   **`{scheme}://<orchestrator-host>/ws/<worker-name>?server-key=<secret>`** —
   경로에 워커 이름이 들어가 같은 터널 뒤 여러 워커의 엔드포인트 충돌을 방지하고,
   scheme도 orchestrator 자체의 ws/wss를 따라갑니다. `derive_agent_endpoint`,
   `crates/fleet-worker/src/join.rs`).
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

> ⚠️ **정정 (2026-08-12)**: 아래 "WorkerSession 데이터 모델"·"dispatch 흐름"·"Reader
> 라우팅"·"fail_all 시맨틱" 절은 Phase 8.4 시점(2026-08-11 SDK 마이그레이션 이전) 구현을
> 서술합니다. 이 구조체·필드·에러 코드는 현재 코드에 **존재하지 않습니다.** "용량 강제",
> "Selector 용량 필터", "API/관측 지원" 절은 여전히 유효합니다(아래 계속).
>
> **현재 구현**: `WorkerSession { capacity: Arc<Semaphore>, sessions: Arc<Mutex<HashMap<SessionId,
> InFlightSession>>>, ... }` — `in_flight`/`prompt_index`/`pending_events`/`InFlightTask`/
> `BufferedEvent` 타입은 사라졌습니다. 라우팅은 `promptId`가 아니라 **`SessionId`** 기준이고
> (`handle_session_notification`), `fail_all`이 사용하는 에러 코드로 서술된 `RpcError {
> code: ACP_ERR_CONNECTION_CLOSED (-32001) }`는 코드 전체 검색 결과 **어디에도 없습니다**
> (완전히 지어낸 값입니다).

Phase 7/8.2의 `AcpTransport`는 워커당 **단일 활성 세션**만 유지했습니다. 즉,
워커에서 처리 중인 `session/prompt` 응답이 도착하기 전에는 두 번째 dispatch를
시도할 수 없었고, 캐퍼시티가 큰 워커의 자원을 활용할 수 없었습니다. Phase 8.4는
**per-worker 동시 다중 세션**을 추가했고, 이후 SDK 마이그레이션이 아래 데이터 모델을
`Semaphore` 기반으로 다시 단순화했습니다.

<details>
<summary>원문 (Phase 8.4 시점 데이터 모델 — 참고용, 현재와 다름)</summary>

### WorkerSession 데이터 모델 (Phase 8.4, 폐기됨)

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

</details>

### dispatch 흐름

![Multi-Session Task Dispatch Flowchart](../assets/diagrams/architecture/multi-session-dispatch.mermaid)

- 현재 구현: `capacity.try_acquire_owned()`(세마포어)가 `max_concurrent` 검사 후 슬롯을
  사전에 점유합니다. 아래 "atomic `complete()`" 서술은 Phase 8.4 시점의 `HashMap` 기반
  구현에 대한 것으로, 세마포어 기반 구현에서는 `Semaphore` 자체가 이 경쟁을 흡수합니다.

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

### Reader 라우팅 — ⚠️ 정정: promptId 아닌 SessionId 기반

⚠️ **정정 (2026-08-12)**: 아래는 Phase 8.4 시점 서술입니다(원문 보존). 현재 코드는
`promptId`가 아니라 **`session_id`**로 라우팅합니다 — `handle_session_notification`이
수신 알림의 `notification.session_id`로 `sessions: HashMap<SessionId, InFlightSession>`을
직접 조회합니다. `prompt_index`/`pending_events`/`run_reader_loop`라는 이름의 함수·필드는
존재하지 않습니다.

<details>
<summary>원문 (Phase 8.4 시점 — 참고용, 현재와 다름)</summary>

`run_reader_loop`는 WebSocket에서 읽은 각 메시지의 `promptId`를 `prompt_index`로
역조회하여 대상 `TaskId`를 찾습니다.

- 알려진 `promptId` → 해당 task로 Output/Failed 이벤트 emit.
- 알려지지 않은 `promptId` → `pending_events`에 buffer. `set_prompt_id` 호출
  시점에 drain.
- `complete()`는 `in_flight`에서 제거하면서 동시에 `prompt_index`에서도
  `prompt_id` 매핑을 정리합니다.

</details>

### fail_all 시맨틱 (연결 손실)

⚠️ **정정 (2026-08-12)**: 아래 원문이 인용하는 에러 코드 `RpcError { code:
ACP_ERR_CONNECTION_CLOSED (-32001), message: "ACP connection closed" }`는 **코드
전체 검색 결과 어디에도 존재하지 않습니다** — 완전히 지어낸 값입니다. 실제로는
`WorkerSession::fail_all(self: &Arc<Self>, broadcaster, reason)`가 `sessions`
맵 전체를 drain하며 각 세션에 `WorkerEvent::Failed`(메시지: `"ACP connection
lost — will reconnect"`)를 브로드캐스트합니다. 아래 두 경쟁 상태 서술 자체의
구조(에러 코드로 소유권을 구분한다는 아이디어)가 현재 구현과 대응되는지는
검증되지 않았습니다.

<details>
<summary>원문 (Phase 8.4 시점 — 참고용, 현재와 다름)</summary>

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

</details>

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

![mTLS Proxy Architecture Diagram](../assets/diagrams/architecture/mtls-proxy-architecture.mermaid)

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

⚠️ **정정 (2026-08-12)**: 아래 `fleet mtls` 서브커맨드 전체와 `--mtls-*` CLI 플래그는
**기본 빌드에 포함되지 않는 opt-in Cargo feature**입니다 — `crates/fleet-cli/Cargo.toml`의
`default = ["acp"]`에 `mtls`는 없고, `crates/fleet-cli/src/mtls.rs`도
`#![cfg(feature = "mtls")]`로 가드되어 있습니다. `cargo build --release`(기본 피처만)로는
`fleet mtls init-ca` 등이 빌드되지 않으며 `cargo build --release --features mtls`가
필요합니다. 반대로 `fleet-worker`는 `Cargo.toml`에서 `fleet-transport`를 항상 `mtls`
피처와 함께 의존하므로, 워커 측 프록시 코드는 항상 컴파일됩니다.

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

발급된 인증서는 각 프로세스에 배포한 뒤 아래처럼 사용한다:

```bash
# 4. orchestrator 구동 (클라이언트 인증서로 worker mTLS proxy 에 인증).
fleet serve --transport acp \
            --mtls-ca /etc/fleet/ca/ca.pem \
            --mtls-cert /etc/fleet/orchestrator/client.pem \
            --mtls-key  /etc/fleet/orchestrator/client.key

# 5. 워커 프로비저닝 시 [mtls] 섹션 활성화 (fleet-worker 가 자체적으로 proxy 구동).
fleet provision --host 10.0.0.5 --name worker-1 \
               --mtls-enabled \
               --mtls-server-cert /etc/fleet/worker-1/server.pem \
               --mtls-server-key  /etc/fleet/worker-1/server.key \
               --mtls-client-ca   /etc/fleet/ca/ca.pem \
               --mtls-advertised-host worker-1.fleet.local
```

`AcpTransport::with_client_tls(ClientTlsConfig)` 로 transport 가 구성된 경우,
supervisor 의 `establish_session` 은 endpoint 스킴에 따라 다음과 같이 분기한다:

- `wss://` → `AcpClient::connect_mtls` → `WsConn::connect_mtls` (사설 CA + 클라이언트 인증서).
- `ws://`  → `AcpClient::connect` (일반 TCP, `client_tls` 무시 — 경고 로그).

즉 orchestrator 는 동일한 바이너리로 mTLS 활성/비활성 워커를 모두 다룰 수 있다.

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
- ~~인벤토리 모드(`--inventory`)의 mTLS 미지원~~ → ✅ **해결 (2026-08-13, 로드맵 #37)**.
  `InventoryDefaults`/`InventoryWorker`에 mTLS 필드를 추가했다 — 여러
  워커가 공유할 만한 값(`mtls_enabled`, `mtls_listen_addr`, `mtls_client_ca`,
  `mtls_advertised_port`)은 `defaults:`에, 워커마다 고유해야 하는 서버
  인증서/키(`mtls_server_cert`/`mtls_server_key`, `fleet mtls issue-server`로
  사전 발급)는 개별 `workers:` 항목에만 둔다. `mtls_advertised_host`를
  생략하면 워커 `name`으로 자동 폴백한다. 예시는
  [`examples/workers.yaml`](../../examples/workers.yaml) 참고:
  ```yaml
  defaults:
    mtls_enabled: true
    mtls_listen_addr: "0.0.0.0:2420"
    mtls_client_ca: /etc/fleet/ca.pem
  workers:
    - host: 10.0.2.20
      name: gpu-x86-01
      mtls_server_cert: /etc/fleet/gpu-x86-01.pem
      mtls_server_key: /etc/fleet/gpu-x86-01.key
      # mtls_advertised_host 생략 → "gpu-x86-01"로 자동 폴백
  ```
  필드가 채워졌는데 `mtls_enabled: true`인 워커에 cert/key가 빠진 경우의
  검증은 새로 만들지 않고 기존 템플릿 렌더링 단계
  (`templates.rs`의 `StepError::Template("mtls_enabled=true requires ...")`)를
  그대로 재사용한다 — 단일 호스트 모드와 동일한 에러 경로.

## SSH 호스트 키 검증 (프로비저너)

`fleet provision`은 SSH로 원격 워커에 접속한다. 최초 구현(Phase 4)은
`russh`의 `check_server_key`에서 서버 공개키를 **무조건 수용(accept-all)**했기
때문에, DNS 스푸핑이나 라우팅 하이재킹 같은 MITM(중간자 공격)에 취약했다.
이제 `~/.ssh/known_hosts` 기반 검증을 도입해 OpenSSH의 `StrictHostKeyChecking`
동작과 동등한 보안을 제공한다.

### 정책 (`HostKeyPolicy`)

`fleet-provisioner::ssh::HostKeyPolicy` 세 가지 모드. OpenSSH 설정값과 대응:

| 정책 | 동작 | OpenSSH 대응 |
|------|------|--------------|
| `accept-all` | 검증 없이 수용. **위험** — 테스트/일회성 전용 | `StrictHostKeyChecking=no` |
| `tofu` (기본) | 첫 연결에서 `known_hosts`에 키 자동 추가, 이후 일치 검사 | `StrictHostKeyChecking=accept-new` |
| `strict` | `known_hosts`에 호스트가 **반드시** 있어야 함. 자동 추가 없음 | `StrictHostKeyChecking=yes` |

검증은 `russh_keys::check_known_hosts_path(host, port, pubkey, path)`로 수행:
- `Ok(true)` → 호스트가 있고 키 일치 → 통과
- `Ok(false)` → 호스트가 `known_hosts`에 없음 → `tofu`면 `learn_known_hosts_path`로 추가, `strict`면 거부
- `Err(_)` → 키 불일치(또는 파일 읽기 실패) → MITM 의심, 즉시 거부

거부 사유는 `SshHandler.reject_reason` 공유 슬롯을 통해 `SshClient::connect`
호출자에게 `SshError::HostKeyVerification { host, reason }`로 전달되어, 단순
"connection failed"가 아닌 "host key mismatch (possible MITM)" 같은 명확한
진단 메시지를 제공한다.

### 설정 경로 (우선순위)

정책과 `known_hosts` 경로는 각각 독립적으로 아래 순서로 결정된다:

1. CLI 플래그 — `--host-key-policy <POLICY>`, `--known-hosts <PATH>`
   (환경변수 `FLEET_HOST_KEY_POLICY`, `FLEET_KNOWN_HOSTS` 도 지원)
2. 인벤토리 `defaults:` — `host_key_policy:`, `known_hosts:`
3. 기본값 — 정책 `tofu`, 경로 `~/.ssh/known_hosts` (`HOME` 기반)

### 사용 예시

```bash
# 운영 권장: strict 모드 + 사전에 fleet scan-host-keys 로 known_hosts 채우기
fleet scan-host-keys --host 10.0.1.10
# 출력된 SHA256 지문을 클라우드 콘솔 등 대역 밖(out-of-band) 채널로 검증한 뒤:
fleet scan-host-keys --host 10.0.1.10 --write
fleet provision --host 10.0.1.10 --ssh-key ~/.ssh/id_ed25519 \
    --name build-arm64-01 --host-key-policy strict

# 신규 인프라 자동화: TOFU 로 첫 키 자동 학습 (기본값)
fleet provision --inventory workers.yaml --ssh-key ~/.ssh/id_ed25519

# CI/일회성: 검증 생략 (명시적 opt-in)
fleet provision --host 10.0.1.10 --host-key-policy accept-all ...
```

### 대규모 배포용 사전 키 수집 — `fleet scan-host-keys` (2026-08-13 추가, 로드맵 #39)

`strict` 정책은 `known_hosts`에 없는 호스트의 **모든** 첫 연결을 거부한다 —
대규모 인프라를 처음부터 `strict`로 배포하려면 각 호스트의 키가 미리
채워져 있어야 한다는 뜻이다. 기존에는 외부 `ssh-keyscan` 바이너리에
의존해야 했는데(위 예시), 이제 `fleet` 바이너리 자체에 동등한 기능이
내장되어 있다.

* **구현**: `fleet-provisioner::ssh::scan_host_key(host, port)`가
  `russh::client::connect`로 handshake만 수행하고 `check_server_key`
  콜백에서 서버 키를 캡처한 뒤 `Ok(false)`를 반환해 즉시 종료시킨다 —
  개인키나 사용자 계정 없이도 키만 얻을 수 있다(`ssh-keyscan`과 동일한
  원리). 지문은 `PublicKey::fingerprint()`(SHA-256)로 계산.
* **`--host <addr>` 또는 `--inventory <file>`**: 단일 호스트 또는 인벤토리
  전체를 일괄 스캔.
* **기본값은 출력만, `--write`로 명시해야 파일에 반영**: 지문을 대역 밖으로
  검증하지 않고 바로 `--write`하는 것은 TOFU와 신뢰 모델이 동일해 실질적인
  MITM 방어 효과가 없다 — 그래서 기본 동작을 "출력만"으로 두어 검증 단계를
  건너뛰기 어렵게 설계했다.
* 파일 append 로직(`append_known_hosts_line`)은 `ssh` 카고 피처와 무관하게
  항상 컴파일되는 순수 파일 I/O이며, `russh_keys::learn_known_hosts_path`와
  동일한 줄 형식(`host algo base64` 또는 비표준 포트면 `[host]:port algo base64`)을
  만든다 — 기존 known_hosts 파싱 경로(`check_known_hosts_path`)와 100% 호환.

### 제한

- `check_known_hosts_path` / `learn_known_hosts_path` 는 동기 파일 I/O.
  `known_hosts` 파일이 작고(수 KB) 연결당 1회만 읽히므로 블로킹 영향은 미미.
- 해시된 호스트명(`|1|<salt>|<hash> ...` 형식)은 `russh-keys`가 처리하므로
  그대로 지원되지만, TOFU `learn`과 `fleet scan-host-keys --write`는 모두
  평문 호스트명으로 추가한다.

## Autonomic Self-Healing Engine (Autonomy) — 🔴 미구현·비연결 상태 (설계 초안)

> ⚠️ **정정 (2026-08-12)**: 이 절 전체가 "탑재되어 있습니다" 등 현재형으로 서술되어
> 있지만, **실제로는 컴파일조차 되지 않는 미완성 코드**이며 어떤 바이너리에도
> 연결되어 있지 않습니다.
>
> - `crates/fleet-scheduler/src/lib.rs`에 다음과 같은 날짜가 박힌 코드 주석이 있습니다:
>   *"FLEET NOTE (2026-08-12): `autonomic` 모듈은 커밋되지 않은 미완성 상태로 발견됨 —
>   `Worker.metrics`(존재하지 않는 필드), `FleetEvent::WorkerLeft`의 `id`/`name`(실제
>   변형은 `worker_id`/`at`만 가짐), `BreakerRegistry::get` 시그니처... 가 모두 현재
>   타입과 어긋나 컴파일이 안 된다."*
> - `pub mod autonomic;`과 그 재노출(`pub use`)이 `lib.rs`에서 **주석 처리**되어 있습니다.
> - `crates/fleet-cli/src/runtime.rs`에서 `fleet serve`가 `AutonomicEngine`을 기동하는
>   배선 코드도 동일한 날짜의 동일한 사유로 **주석 처리**되어 있습니다.
> - 즉 `crates/fleet-scheduler/src/autonomic.rs`(172줄)는 어떤 릴리스 바이너리에도
>   포함되지 않습니다.
>
> 아래는 **설계 의도(초안)**로 읽어야 하며, "지금 이렇게 동작한다"가 아니라
> "구현이 완료되고 재연결되면 이렇게 동작할 예정이었다"로 해석하세요. `WorkerStatus::
> Degraded`/`Offline`과 `BreakerRegistry`/`CircuitBreaker` 강제 개방 자체는 실제
> 코드에 존재하는 타입이므로, 이 설계가 완전히 근거 없는 것은 아닙니다 — 다만
> `AutonomicEngine`이 그것들을 실제로 구동하는 경로는 현재 끊겨 있습니다.

오케스트레이터의 안정적이고 지속적인 자율 운영을 보장하기 위해, 실시간으로 하드웨어 상태 및 메트릭을 감시하고 자가치유를 수행하는 **Autonomic Engine**을 설계했습니다(미구현).

### MAPE-K 제어 루프 아키텍처 (설계)

![MAPE-K Self-Healing Control Loop Diagram](../assets/diagrams/architecture/mape-k-control-loop.mermaid)

### 작동 메커니즘 (설계 — 미구현)

1. **상태 감시 (Monitor)**: `AutonomicEngine`이 백그라운드 스레드에서 설정된 interval 마다 가동되어, `Store`로부터 활성 워커들의 메트릭 정보 및 동작 데이터를 폴링하여 관측하도록 설계되었습니다.
2. **동적 분석 (Analyze)**: 워커가 전달한 하드웨어 로드 지표를 바탕으로 오작동 여부를 판단하도록 설계되었습니다:
   - CPU 로드율 과부하 지속 (>95%)
   - VRAM 메모리 부족 또는 GPU 스톨(Stall) 누적
   - API 응답 실패율 누적 및 지연 시간 임계치 초과
3. **치유 계획 (Plan)**: 진단된 위반 사항에 대해 자율 복구 계획을 결정하도록 설계되었습니다. 경미한 과부하 시 워커를 `Degraded` 상태로 하향하여 스케줄러 분배 비중을 조절하고, 심각한 고장이나 순단 시 `CircuitBreaker`를 강제 개방하여 유입되는 작업을 원천 차단하는 것이 목표입니다.
4. **치유 실행 (Execute)**: `Store` 상태에 `Degraded`/`Offline` 등의 상태를 자율 커밋하고, 메모리 내의 `BreakerRegistry`와 연동하여 해당 워커로 향하는 dispatch 요청을 즉각 정지시키는 것이 목표입니다.

## 태스크 모니터링 및 종료 감지 파이프라인

MCP로 `fleet_dispatch_task`를 호출한 뒤, 오케스트레이터가 내부적으로 어떻게 진행 상황을
추적하고 종료를 감지하는지, 그리고 MCP 클라이언트가 실제로 쓸 수 있는 모니터링 수단은
무엇인지 정리합니다(2026-08-12 코드 대조).

![Task Monitoring & Completion Detection Pipeline](../assets/diagrams/architecture/task-monitoring-pipeline.mermaid)

### 디스패치는 논블로킹이다

`Dispatcher::submit()`(`crates/fleet-scheduler/src/dispatcher.rs`)은 워커 선택,
회로차단기 확인, `Dispatched` 상태 기록, `session/new`+`session/prompt` 전송까지만
동기적으로 수행하고 **즉시 반환**합니다. 실제 에이전트 턴 실행은
`AcpTransport::dispatch()`가 `tokio::spawn`한 백그라운드 태스크에서 별도로 진행됩니다
(`crates/fleet-transport/src/acp_transport.rs:418`). `fleet_dispatch_task`의 응답
`hint` 필드 자체가 "`fleet_get_task_status`로 폴링하라"고 명시합니다.

### 내부는 이벤트 기반, MCP 외부 인터페이스는 폴링 전용

| 단계 | 메커니즘 |
|---|---|
| 진행 중 출력 감지 | `session/update`(`agent_message_chunk`) 수신마다 `WorkerEvent::Output`을 broadcast 채널에 emit (`acp_transport.rs:750-774`) |
| 종료 감지 | `session/prompt` 응답 수신 시 `WorkerEvent::Completed` 또는 `Failed`를 emit (`acp_transport.rs:485-516`). ⚠️ `stop_reason` 값으로 분기하지 않습니다 — 정상 응답이면 무조건 완료 처리, 로그만 남깁니다. |
| 상태 커밋 | `Dispatcher::run_event_loop()`이 `mpsc` 채널로 이 이벤트를 **구독**해 `Store::update_task_status()`를 호출 (`dispatcher.rs:59-189`) — **폴링이 아니라 채널 수신**입니다. |

즉 **오케스트레이터 내부의 종료 확인은 이벤트 구독**이지만, **MCP로 접근하는 클라이언트가
쓸 수 있는 도구는 전부 폴링/블로킹폴링**입니다:

- `fleet_get_task_status` — 1회성 스냅샷 조회.
- `fleet_stream_task_output` — `max_polls`(기본 60)회까지 `poll_interval_secs`(기본 1초)
  간격으로 반복 조회하는 유한 폴링 루프.
- `fleet_wait_for_task` — **50ms 고정 간격**으로 `is_terminal()`을 반복 확인하는 블로킹
  폴링(`dispatcher.rs:565`), 기본 타임아웃 300초.

**MCP를 통한 push/알림 경로는 존재하지 않습니다.** `fleet-mcp`의 stdio 루프는 "단일
스레드, 한 번에 하나의 요청만 처리"하는 단방향(클라이언트→서버) 구조이며(`server.rs:9`),
서버가 클라이언트에 알림을 먼저 보내는 코드는 없습니다. (참고: 대시보드의
`/api/events/stream`는 Postgres `LISTEN/NOTIFY` 기반 SSE로 실시간 push가 되지만, MCP
경로가 아니라 웹 대시보드 전용입니다.)

### HealthChecker의 Offline 처리와 태스크 실패 연동

✅ **해결됨 (2026-08-13)**. `HealthChecker`가 워커를 `Offline`으로 표시하는 것(45초/3회
누락)과 그 워커에 배정된 진행 중 태스크를 실패 처리하는 것은 원래 **서로 연결되어
있지 않았습니다** — `HealthChecker::scan_once()`(`crates/fleet-scheduler/src/health.rs`)는
`Worker.status`만 갱신할 뿐 Task 테이블은 건드리지 않았고, `Reconciler`의 orphan 회수
경로도 워커 row가 완전히 사라진 경우만 다뤘습니다. 워커가 heartbeat만 끊기고
WebSocket 연결은 살아있는 애매한 상태라면, 배정된 태스크가 무기한 `Dispatched`로
남을 수 있었습니다.

`Reconciler`에 세 번째 스윕을 추가해 이 빈틈을 메웠습니다
(`crates/fleet-scheduler/src/reconcile.rs::reap_stale_dispatched`): 담당 워커가
`workers` 테이블에 여전히 존재하되 `status == Offline`이고, 마지막 하트비트
(`last_seen`)로부터 `offline_worker_grace`(기본 **5분**, `--reconcile-offline-worker-
grace-secs` / `FLEET_RECONCILE_OFFLINE_WORKER_GRACE_SECS`) 이상 지났다면
`Failed(WorkerUnavailable)`로 전이합니다. 기존 "워커 row가 아예 사라진" 경로
(`dispatched_worker_check_after`, 기본 30초)보다 훨씬 긴 유예를 두는 이유는
`Offline`이 되돌릴 수 있는 상태이기 때문입니다 — 워커가 곧 재연결될 수 있는 상황에서
성급하게 작업을 실패 처리하지 않기 위함입니다.

⚠️ **남은 한계**: `update_task_status`가 현재 상태를 조건으로 거는 낙관적 잠금을 하지
않으므로, 이 스윕이 태스크를 `Failed`로 마킹한 직후 워커가 실제로는 재연결에
성공해서 뒤늦게 `WorkerEvent::Completed`가 도착하면 상태가 다시 덮어써질 수 있는
이론적 경쟁 상태가 여전히 남아 있습니다. 5분이라는 유예 자체가 이 창을 매우 좁게
만들 뿐, 완전히 없애지는 못합니다.

## 동시 실행(멀티 에이전트) 능력

**결론: 있습니다 — 단, 범위가 "오케스트레이터의 세션 북키핑 계층"으로 한정됩니다.**

![Concurrency Scope Diagram](../assets/diagrams/architecture/concurrency-scope.mermaid)

### 워커 1대 내부의 동시성

`WorkerSession.capacity: Arc<Semaphore>`가 `max_concurrent_tasks`(기본값 4 — worker
config·`fleet_core::Worker`·DB 스키마 `max_concurrent INTEGER NOT NULL DEFAULT 4` 세
군데 모두 일치)로 사이징되어 있고, 태스크마다 별도 ACP 세션(`session/new`)을 발급해
`session_id` 기준으로 라우팅합니다(§동시 다중 세션 참조). **초과 시 큐잉 없이 즉시
`WorkerAtCapacity` 에러**를 반환합니다 — 대기열이 아니라 즉시 거부입니다. 이는
`crates/fleet-transport/tests/acp_concurrent.rs`의 `concurrent_dispatches_within_
capacity_all_complete`(워커 1대에 태스크 3개 동시 디스패치, 독립 완료 확인),
`dispatch_beyond_capacity_returns_worker_at_capacity`(용량 초과 시 즉시 거부 확인),
`concurrent_tasks_stream_output_via_correct_session_routing`(태스크 5개 동시 디스패치,
출력 미혼선 확인) 세 테스트로 검증돼 있습니다.

### 워커 여러 대 사이의 동시성

디스패치 경로 전체를 잠그는 전역 락이 없습니다 — `FleetState`(`crates/fleet-scheduler/
src/state.rs`)는 `Mutex`/`RwLock`으로 감싸여 있지 않고, `Dispatcher::submit()`/
`dispatch_existing()`은 모두 `&self`라 `Arc<Dispatcher>`를 여러 tokio 태스크에서
동시에 호출할 수 있습니다. 회로차단기도 워커별 개별 `Mutex`(`breaker.rs`)라 워커 A/B
간 경합이 없습니다. 구조적으로 여러 워커에 동시 디스패치가 가능하지만, ⚠️ **"서로 다른
워커 N대에 동시에 디스패치해서 독립적으로 완료되는지"를 직접 검증하는 전용 테스트는
찾지 못했습니다** — 아키텍처상 지원되는 것은 확실하나, 같은-워커 케이스만큼 테스트로
못박혀 있진 않습니다.

### ⚠️ 확인 불가 영역: `grok agent serve` 자체의 진짜 병렬성

위 동시성은 전부 **오케스트레이터 쪽 세션 관리 계층**의 이야기입니다. 실제
`grok agent serve` 서브프로세스가 여러 세션의 추론 요청을 진짜 병렬로 처리하는지,
아니면 내부적으로 직렬화하는지는 **이 저장소 코드만으로는 확인할 수 없습니다.**
`fleet-worker`(`grok_process.rs`)는 `grok agent serve`를 외부 바이너리로 spawn/재시작
할 뿐 내부 동작에 관여하지 않고, `acp_concurrent.rs`의 테스트들도 실제 grok이 아니라
mock WebSocket 서버를 상대로 돌기 때문에 증명하는 것은 "오케스트레이터가 여러 세션을
올바르게 라우팅한다"는 것이지 "grok이 진짜 병렬 추론을 한다"는 것이 아닙니다.

### 전체 fleet 상한

별도의 글로벌 세마포어는 없습니다. 순수하게 Σ(온라인이고 회로차단기 Closed인 워커의
`max_concurrent`)가 실질 상한입니다. MCP stdio 루프 자체는 한 커넥션당 요청을 하나씩
순차 처리하지만(파이프라이닝 불가, `server.rs:9`), `fleet_dispatch_task`가 프롬프트를
"넘기기만 하고" 즉시 반환하므로 여러 태스크를 연달아 빠르게 dispatch 호출하는 것 자체는
문제없고, 실제 실행은 백그라운드에서 병렬로 진행됩니다.


