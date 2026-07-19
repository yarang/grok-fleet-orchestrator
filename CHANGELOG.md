# Changelog

이 프로젝트의 주요 변경사항을 기록합니다.
형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르며,
버전 번호는 [Semantic Versioning](https://semver.org/lang/ko/)을 준수합니다.

## [Unreleased]

### Added — Phase 8.5.1: mTLS 클라이언트 구성 (orchestrator→worker wss://)

- **`fleet-transport` `mtls` feature**: rustls 0.23 + rustls-pemfile 2 + tokio-rustls 0.26
  을 옵션 의존성으로 추가. `acp` feature에 의존하므로 `--features "acp mtls"` 로 활성화.
- **`ClientTlsConfig`** (`fleet-transport/src/tls.rs`): 사설 CA PEM + 클라이언트
  인증서 PEM + 클라이언트 키 PEM 경로를 보관. `build_connector()` / `build_client_config()`
  으로 rustls `TlsConnector` / `ClientConfig` 생성. 매 호출마다 파일을 다시 읽어
  인증서 갱신 시 프로세스 재시작 불필요.
- **`TlsError`** 에러 타입: 파일 읽기 / 파싱 / 빌드 단계 구분.
- **`WsConn::connect_mtls(url, &ClientTlsConfig)`** (`acp/transport.rs`): tokio-tungstenite
  의 `Connector::Rustls(Arc<ClientConfig>)` 를 주입해 mTLS 핸드셰이크 수행.
  `wss://` URL만 허용.
- 기존 `WsConn::connect` 는 `into_client_request()` 기반으로 리팩터. Host /
  Sec-WebSocket-* 헤더 자동 설정.

### Tests — Phase 8.5.1

- **`crates/fleet-transport/tests/mtls_handshake.rs`** 신규 3개 통합 테스트:
  - `wsconn_connect_mtls_roundtrips_text` — rcgen 으로 ephemeral CA + 서버/클라이언트
    인증서 발급 → 클라이언트 인증서를 요구하는 TLS WebSocket echo 서버 구동 →
    `WsConn::connect_mtls` 로 접속해 텍스트 프레임 라운드트립.
  - `wsconn_connect_mtls_rejects_ws_url` — `ws://` URL은 `connect_mtls`에서 거부.
  - `wsconn_connect_mtls_fails_with_untrusted_client_cert` — 서버가 신뢰하지 않는
    CA로 서명한 클라이언트 인증서는 핸드셰이크 단계에서 거부됨을 검증.
- 총 **334개 통과**, 3개 `#[ignore]` (Phase 8.4의 331 대비 +3). clippy `-D warnings`
  통과 (`--features "acp mtls"`).

### Changed — Phase 8.5.1

- **Workspace deps**: `rustls = "0.23"` (default-features = false, ring + std +
  logging), `rustls-pemfile = "2"`, `tokio-rustls = "0.26"`, `rcgen = "0.13"` 추가.
- **`fleet-transport` 의존성 변경**: `tokio-tungstenite` 의 TLS feature를
  `rustls-tls-native-roots` → `rustls-tls-webpki-roots` 로 교체 (이식성 향상,
  system root 의존 제거).

### Notes — Phase 8.5.1

- `ClientTlsConfig` 는 rustls 0.23 의 CryptoProvider 문제를 피하기 위해
  `builder_with_provider(Arc<ring::default_provider()>)` 를 사용.
- `AcpTransport` 에 mTLS 구성을 plumbed하는 것은 Phase 8.5.3에서 수행
  (orchestrator CLI 플래그와 함께).

### Added — Phase 8.4: 동시 다중 세션 per worker

- **`WorkerTransport::register` 시그니처 변경**: `max_concurrent_tasks: u32` 인수
  추가. transport는 dispatch 시 이 값을 검사하여 `WorkerAtCapacity` 에러를
  반환. 0은 1로 정규화 (최소 1개 슬롯 보장).
- **`TransportError::WorkerAtCapacity(WorkerId)`**: 새 에러 variant.
  dispatcher는 이 에러를 일반 WorkerError와 동일하게 처리 (task를 Failed로
  마킹). 사용자는 다시 submit하거나 다른 worker를 hint로 지정 가능.

- **`AcpTransport` 동시성 모델 전면 개편** (`acp_transport.rs`):
  - `WorkerSession.active_task: Option<TaskId>` (단일) →
    `in_flight: HashMap<TaskId, InFlightTask>` + `prompt_index: HashMap<PromptId, TaskId>` (역색인).
  - `dispatch(req)`: 용량 검증 → `try_acquire(task_id)`로 슬롯 확보 →
    백그라운드에서 `session/prompt` 호출 → 응답 도착 시 `set_prompt_id`로
    prompt_id 등록.
  - **`pending_events` 버퍼**: `session/update` (Output) notification이
    `session/prompt` 응답보다 먼저 도착하는 레이스 윈도우 커버. prompt_id가
    등록되기 전의 Output/Failed 이벤트를 버퍼에 저장, `set_prompt_id` 호출
    시 drain하여 emit.
  - `run_reader_loop`: 모든 이벤트를 `prompt_id` 기반으로 라우팅. 동시에
    진행 중인 N개의 task의 Output/Completed/Failed가 정확히 해당 task로 전달.
  - `fail_all` (Phase 8.2의 `fail_active_task` 확장): reader 종료 시
    in_flight에 있는 **모든** task에 대해 `WorkerEvent::Failed` emit.
  - `AcpTransport::in_flight_count(worker_id)` / `max_concurrent(worker_id)`:
    관측/디버그용 public 메서드.
  - **`AcpClient::reader_loop` drain 개선**: WebSocket 종료 시 pending
    요청에 빈 응답 대신 특수 에러 코드 -32001 ("ACP connection closed")를
    보내어 상위 dispatch가 supervisor의 `fail_all`에 위임 가능하게 함.

- **`MockTransport` 동시성 지원**: `register`에서 `max_concurrent_tasks`를
  받아 `capacities` 맵에 저장. `dispatch`에서 `active >= cap` 검사 후
  `WorkerAtCapacity` 반환.

- **`WorkerSelector` 용량 필터** (`selector.rs`): `candidates.retain(|w|
  w.has_capacity())` 추가. 단, `Worker.active_tasks`는 heartbeat 기반으로
  갱신되므로 eventual consistent — 최종 강제는 transport가 담당.

### Tests — Phase 8.4

- **`crates/fleet-transport/tests/acp_concurrent.rs`** (신규 4개 통합 테스트):
  - `concurrent_dispatches_within_capacity_all_complete` — max=3 워커에 3개
    동시 dispatch → 모두 Completed.
  - `dispatch_beyond_capacity_returns_worker_at_capacity` — max=1 워커에 2개
    연속 dispatch → 두 번째는 `WorkerAtCapacity`.
  - `output_events_routed_to_correct_task_by_prompt_id` — 2개 동시 dispatch,
    각 task에 도착하는 Output이 서로 다른 promptId로 식별되는지 검증.
  - `in_flight_count_reflects_active_dispatches` — `in_flight_count` /
    `max_concurrent` 조회 메서드 검증.

- **`acp_reconnect.rs::failed_event_emitted_for_in_flight_task_on_close`**:
  다중 in-flight task 시나리오로 재작성. mock 서버에 `close_now` AtomicBool
  추가 — 메시지 dispatch 없이 WebSocket Close를 트리거. 두 task 모두 Failed
  수신 검증.

- 총 **331개 테스트 통과**, 3개 `#[ignore]` (Phase 7 E2E 2 + doctest 1).
  Phase 8.3 (327) 대비 +4.

### Changed — Phase 8.4 (Breaking)

- `WorkerTransport::register` 시그니처 변경: 모든 caller가 `max_concurrent_tasks`
  인수를 전달해야 함. 영향받는 호출부:
  - `fleet-api::handlers::upsert_and_register` (worker.max_concurrent 전달)
  - 모든 테스트의 `register` 호출 (`4` 또는 `1`을 명시적으로 전달)
  - `RecordingTransportShared` / `FailingTransport` 등 테스트용 mock trait 구현

### Documentation — Phase 8.4

- `docs/architecture.md`: "동시 다중 세션 (Phase 8.4)" 섹션 추가.
- 로드맵에서 Phase 8.4 제거, 8.5 (mTLS)를 다음 항목으로 표시.

## Phase 8.3: Bootstrap 토큰 + Worker 자유 가입 (`fleet-worker join`)

- **Bootstrap 토큰 저장 모델** (`crates/fleet-store/migrations/003_bootstrap_tokens.sql`):
  - `bootstrap_tokens` 테이블 — `token` (PK), `created_at`, `created_by`,
    `expires_at` (NULL = 무기한), `max_uses` (기본 1), `use_count`, `notes`,
    `last_used_by`, `last_used_at`.
  - `expires_at` 부분 인덱스 + `created_at DESC` 인덱스.
- **`BootstrapToken` 도메인** (`fleet-core::bootstrap_token`): `is_usable()` (use_count <
  max_uses AND not expired), `remaining_uses()` 계산. 직렬화에 `status` 파생 필드
  (`active | exhausted | expired`) 포함.
- **Store trait 확장** — 4개 메서드 추가:
  - `create_bootstrap_token` / `consume_bootstrap_token` /
    `list_bootstrap_tokens` / `revoke_bootstrap_token`
  - **atomic UPDATE...RETURNING** 로 토큰 소비를 단일 SQL로 수행 — 레이스 컨디션
    불가능 (두 클라이언트가 동시에 같은 단일-사용 토큰을 사용해도 DB가 한 쪽만
    성공시킴).
- **새 API 엔드포인트** (`fleet-api`):
  - `POST /v1/workers/join` — 토큰 검증 → 워커 upsert → `worker.toml` 본문 렌더링.
    성공 시 orchestrator가 워커를 위해 사용할 config를 응답 바디에 포함.
  - `POST /v1/bootstrap-tokens` — CSPRNG(`/dev/urandom`, Windows UUID 폴백) +
    base64url-no-pad 인코딩으로 토큰 생성.
  - `GET /v1/bootstrap-tokens` — 활성/사용량/만료 상태 포함 표 형식 목록.
  - `DELETE /v1/bootstrap-tokens/:token` — 폐기 (use_count를 max_uses로 올림).
- **CLI 확장** (`fleet token ...`):
  - `fleet token issue [--max-uses N] [--expires-in-secs S] [--prefix PREFIX]
    [--bytes 32] [--notes "..."`
  - `fleet token list [--json]` — 테이블 또는 JSON 출력.
  - `fleet token revoke <TOKEN>`
- **`fleet-worker join` 서브커맨드** (`crates/fleet-worker/src/join.rs`):
  - DNS-safe 워커 이름 검증 → `/dev/urandom` 기반 grok_secret 자동 생성
    (32바이트) → orchestrator의 `/v1/workers/join` 호출 → `worker.toml`을
    tmp 파일 + rename으로 원자적 저장 → (옵션) `--start` 시 현 프로세스를
    daemon으로 `exec` (Unix) / spawn+wait (Windows).
  - `JoinArgs { orchestrator_url, token, name, labels, agent_endpoint,
    grok_secret, config_out, start, max_concurrent_tasks }`.

### Tests — Phase 8.3

- **`crates/fleet-api/tests/bootstrap_tokens.rs`** (신규 13개 통합 테스트):
  - 토큰 생성/목록/폐기 round-trip
  - 유효/무효/소진/만료 토큰으로 join 시도 시 각각 200/401/401/401
  - 중복 이름 충돌 시 409
  - join 응답의 `worker_config_toml`이 필수 필드 포함 검증
  - multi-use 토큰으로 여러 워커 연속 가입
- **`crates/fleet-worker/tests/join.rs`** (신규 6개 통합 테스트):
  - config 파일 디스크 저장, grok_secret 자동 생성, 라벨 직렬화, 이름 검증,
    서버 에러 전파, 부모 디렉토리 자동 생성
- `BootstrapToken` 단위 테스트 5개 추가.
- 총 **327개 테스트 통과**, 3개 `#[ignore]` (Phase 7 E2E 2 + doctest 1).

### Documentation — Phase 8.3

- `docs/architecture.md`: "Bootstrap Token & Worker Join (Phase 8.3)" 섹션 추가
  (데이터 모델, atomic UPDATE 쿼리 설명, join 흐름, CLI 예시). 로드맵에서
  Phase 8.3 제거하고 8.4(동시 다중 세션)를 다음 항목으로 표시.
- `docs/deployment.md`: 3.4절 — `fleet-worker join` 셀프 서비스 가입 워크플로,
  `fleet token issue` 예시, 자동 생성된 `worker.toml` 구조.
- `docs/api-reference.md`: `/v1/workers/join` + `/v1/bootstrap-tokens` 엔드포인트
  스키마 추가.

## Phase 8.2: WebSocket 재연결 (supervisor + 지수 백오프)

### Added

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
