# Changelog

이 프로젝트의 주요 변경사항을 기록합니다.
형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르며,
버전 번호는 [Semantic Versioning](https://semver.org/lang/ko/)을 준수합니다.

## [Unreleased]

### Added — Phase 8.2: WebSocket 재연결 (supervisor + 지수 백오프)

- **per-worker supervisor 태스크**: `register()` 시점에 각 워커마다 전용
  `tokio::spawn` 태스크를 생성. WebSocket 수명 주기, reader 종료 감지, 자동
  재연결을 단일 책임으로 캡슐화.
  - 첫 번째 연결 시도 결과는 `oneshot` 채널로 `register()` 호출자에게 전달 —
    실패하면 `TransportError::Connection` 반환 (supervisor는 백그라운드에서 계속 재시도).
  - `unregister()` 또는 `AcpTransport::drop` 시 `SupervisorCmd::Shutdown` 전송 +
    `JoinHandle::abort()` 로 정리.

- **지수 백오프**: `RECONNECT_INITIAL` (1s) → 2s → 4s → ... → `RECONNECT_MAX` (30s).
  연결이 한 번 성공하면 `backoff`를 1s로 리셋. `ReconnectConfig { initial, max }`로
  테스트에서 임의 값 주입 가능 (`AcpTransport::with_reconnect`).

- **`ConnState` enum**: `Connecting | Connected | Disconnected`. supervisor가
  락으로 갱신하며, `dispatch` / `ping` 은 `state != Connected` 인 경우 즉시
  `TransportError::Connection` 반환. `AcpTransport::conn_state(worker_id)` 로
  외부 조회 가능.

- **`WorkerEvent::Failed` 자동 방출**: reader가 종료되면 (Close 프레임, I/O 에러,
  `grok agent serve` 종료 등) 현재 `active_task`를 take 하고
  `Failed { error: "ACP reader exited (connection lost)" }` 이벤트를 broadcast.
  이후 재연결 시 동일 task가 자동 재실행되지는 않음 (재시도 정책은 상위 레이어 담당).

- **`ClientInner.event_tx` 재설계** (`fleet-transport::acp`): 기존
  `UnboundedSender<AcpEvent>` 를 `std::sync::Mutex<Option<...>>` 로 래핑.
  `reader_loop()` 종료 시 `close_event_channel()` 로 내부 sender를 drop하여
  supervisor의 `event_rx.recv()` 가 `None` 을 반환하게 만듦. 이로써 WebSocket
  Close 프레임에만 의존하지 않고 AcpClient의 모든 종료 경로를 감지.

### Tests — Phase 8.2

- **`crates/fleet-transport/tests/acp_reconnect.rs`** (신규 7개 통합 테스트):
  - `connection_failure_during_register_returns_error` — 첫 연결 실패 시 `register()` 가 `Err` 반환.
  - `close_frame_marks_disconnected` — WebSocket Close 프레임 수신 후 `conn_state` 가 `Disconnected` 로 전환.
  - `reconnect_after_close_frame` — Close 이후 백오프를 거쳐 자동 재연결 (`Connected` 복귀).
  - `failed_event_emitted_for_in_flight_task_on_close` — 진행 중 task가 reader 종료 시 `Failed` 로 전환됨.
  - `unregister_during_backoff_exits_cleanly` — 백오프 대기 중 `unregister` 호출 시 supervisor가 즉시 종료됨.
  - `multiple_workers_reconnect_independently` — 워커 A 의 연결이 끊겨도 B 는 정상 유지.
  - `exponential_backoff_increases_between_failures` — 연속 실패 시 백오프가 2배씩 증가.
- `acp_transport.rs` 단위 테스트 6개 추가 (conn_state, ping, wait_with_shutdown 변형 3종).
- 총 **291개 테스트 통과**, 3개 `#[ignore]` (Phase 7 E2E 2 + doctest 1). 전체 294개.

### Documentation — Phase 8.2

- `docs/architecture.md`: "WebSocket Reconnection (Phase 8.2)" 섹션 추가
  (supervisor 패턴, 상태 머신, 백오프 시퀀스, shutdown 시퀀스, reader 종료 감지).
  "제한 (Phase 7/8.1 MVP)" 업데이트로 재연결 완료 표시. 로드맵에서 Phase 8.2 제거하고
  Phase 8.3 (worker join CLI) 를 다음 항목으로 표시.

## Phase 8.1: fleet-worker 바이너리

- **`fleet-worker` 크레이트** (`crates/fleet-worker/`): 워커 머신에서 실행되는
  standalone 데몬. CLI 진입 (`--config /etc/fleet/worker.toml`, `--check`).
  - `WorkerConfig`: 확장된 worker.toml 파서 (`[worker]` + `[grok]` 섹션).
    DNS-safe 워커 이름 검증, http(s) URL 검증, 범위 기반 필드 검증.
    `std::str::FromStr` trait 구현으로 `toml.parse::<WorkerConfig>()` 지원.
  - `GrokRunner`: `grok agent serve --bind <addr> --secret <s>` 서브프로세스를
    `tokio::process::Command`로 관리. exit 시 `restart_delay_secs` 후 재시작
    (최대 10회). `watch::Sender<bool>`로 외부 shutdown 제어.
    `kill_on_drop(true)` + SIGTERM(5s timeout) → SIGKILL 안전 종료.
  - `RegistrationClient`: orchestrator HTTP API 클라이언트.
    `register_with_retry()` (5초 간격 무한 재시도), `run_heartbeat_loop()`
    (TCP health_check + sysinfo 메트릭 수집), `deregister()`.
  - `WorkerRunner`: 위 두 모듈 조립. SIGINT/SIGTERM 수신 시 graceful shutdown
    (grok 10s timeout, heartbeat 5s timeout, deregister best-effort).

- **`worker.toml` 템플릿 확장** (`fleet-provisioner::templates`):
  - `TemplateContext`에 `grok_secret` (필수), `grok_bin`, `grok_bind_addr`,
    `max_concurrent_tasks`, `restart_delay_secs`, `grok_cwd`, `bootstrap_token`,
    `labels` 필드 추가.
  - `render_worker_config()`가 `[worker]` + `[grok]` 섹션 생성.
    라벨은 key 기준 정렬하여 결정론적 직렬화.
  - 기존 `[cloudflared]` 섹션은 worker.toml에서 제거 (별도 config.yml로 분리).

- **`StepContext` 확장** (`fleet-provisioner`): `grok_secret`, `grok_bind_addr`,
  `max_concurrent_tasks`, `bootstrap_token` 필드 추가.

- **CLI 플래그**: `fleet provision`에 `--grok-secret` / `FLEET_GROK_SECRET` 및
  `--bootstrap-token` / `FLEET_BOOTSTRAP_TOKEN` 추가.

- **인벤토리 YAML**: 각 워커 항목에 `grok_secret:` (per-worker) 및
  `options.bootstrap_token:` (전역) 필드 추가.

### Tests

- fleet-worker 단위 테스트 19개 (config 9 + grok_process 5 + registration 6 + runner).
- fleet-worker 통합 테스트 3개 (`worker_lifecycle.rs`): 가짜 grok TCP 리스너 +
  mock orchestrator로 register → heartbeat ≥ 2회 (agent_healthy=true) → deregister
  전체 라이프사이클 검증. grok 다운 시 agent_healthy=false 전파 확인.
- fleet-provisioner 템플릿 테스트 5개 추가/수정.
- 총 282개 테스트 통과, 3개 `#[ignore]` (Phase 7 E2E 2 + doctest 1).

### Documentation

- `docs/architecture.md`: "Worker Daemon (Phase 8.1)" 섹션 추가, 로드맵 업데이트
- `docs/deployment.md`: 3.3절 worker.toml 형식 + `--check` 검증 모드 문서화

## Phase 7: ACP Transport

### Added

- **`AcpTransport`** (`fleet-transport::AcpTransport`): `WorkerTransport` trait의
  실제 구현체. 각 워커의 `grok agent serve` (기본 포트 2419)와 WebSocket으로 통신.
  - `register(worker_id, "ws://.../ws?server-key=...")` 시 `AcpClient`가 연결되고
    `session/new` 로 세션을 엶.
  - `dispatch(req)` 는 백그라운드에서 `session/prompt`를 전송하고,
    도착하는 `session/update` notification을 `WorkerEvent::Output`로 변환.
    응답은 `WorkerEvent::Completed`로 방출 (`TaskResult`에 토큰 사용량 포함).
  - `cancel(task_id)` 는 활성 프롬프트를 `session/cancel` 로 취소.

- **ACP 클라이언트 모듈** (`fleet-transport::acp`):
  - `AcpClient::connect / open_session / prompt / cancel / close`
  - JSON-RPC 2.0 envelope (`messages::RpcRequest`, `RpcMessage`, `RpcError`)
  - 5개 ACP 메서드 매핑: `initialize`, `session/new`, `session/prompt`,
    `session/cancel`, `session/update`
  - `tokio-tungstenite` 0.24 + `rustls-tls-native-roots` feature 사용

- **`WorkerTransport::subscribe()` trait 메서드**: 이제 모든 transport 구현체가
  이벤트 수신기를 반환하는 동일한 인터페이스를 가짐. 기존 `MockTransport::new()`
  튜플 반환 패턴 제거 (호환성 손상 — 런타임/테스트만 영향).

- **HTTP API ↔ Transport 통합** (`AppState.with_transport(...)`):
  - `POST /v1/workers/register` 가 Store upsert 후 `transport.register()` 호출
  - `DELETE /v1/workers/:id` 가 `transport.unregister()` 후 Store delete
  - transport 실패는 warn 로그만 (Store는 정상 — HealthChecker가 offline으로 강등)

- **CLI `--transport acp` 모드**: `fleet serve --transport acp` 로
  실제 grok agent와 통신하는 모드 활성화. 기본값은 `mock` (하위 호환).

### Tests

- ACP 클라이언트 통합 테스트 7개 (mock WebSocket 서버로 initialize/session/prompt/cancel round-trip 검증)
- AcpTransport 통합 테스트 9개 (register/dispatch/cancel/multi-worker 시나리오)
- HTTP API ↔ Transport 통합 테스트 3개 (register/deregister가 transport 메서드를 호출하는지 검증)
- `AcpTransport` 단위 테스트 6개
- 총 256개 테스트 통과, 2개 E2E 테스트 (`#[ignore]`, `GROK_BIN` 설정 시에만 실행)

### Changed

- `MockTransport::new()` 시그니처 변경: `(Self, Receiver)` 튜플 → `Self`.
  이벤트는 `subscribe()` 로 획득. 영향받는 호출부:
  - `fleet-cli/src/runtime.rs`
  - `fleet-scheduler/src/sync.rs`, `health.rs`
  - `fleet-scheduler/tests/dispatch_e2e.rs`
  - `fleet-transport/src/mock.rs` (자체 테스트)

### Documentation

- `docs/architecture.md`: "ACP Transport" 섹션 추가, 로드맵 업데이트
- `docs/api-reference.md`: register 엔드포인트 문서에 Phase 7 변경사항 반영
- `README.md`: ACP transport를 주요 특징에 추가

## Phase 1–6 (이전 릴리스)

- Phase 1–3: 핵심 도메인, Store, Transport trait, Scheduler, MCP 서버
- Phase 4: SSH 자동 프로비저닝 + Cloudflare Access 미들웨어
- Phase 5: 웹 대시보드 (rust-embed + SSE)
- Phase 6: CLI 종합 (workers/tasks/token/events/doctor), Prometheus /metrics,
  감사 로그, 운영 문서

자세한 내용은 `docs/` 디렉토리와 git history를 참조.
