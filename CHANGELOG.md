# Changelog

이 프로젝트의 주요 변경사항을 기록합니다.
형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르며,
버전 번호는 [Semantic Versioning](https://semver.org/lang/ko/)을 준수합니다.

## [Unreleased]

### Added — Phase 7: ACP Transport

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
