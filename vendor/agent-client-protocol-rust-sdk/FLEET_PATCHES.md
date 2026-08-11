# Fleet 벤더 패치 기록

이 디렉토리는 [`agentclientprotocol/rust-sdk`](https://github.com/agentclientprotocol/rust-sdk)의
로컬 벤더 사본이다. 출처 커밋은 `VENDORED_COMMIT.txt` 참고
(`07926d7f9468e149e4fb676ab531b410aa8143cb`, 2026-08-10).

## 벤더링 이유

Fleet의 `fleet-transport`가 손으로 짠 ACP JSON-RPC 클라이언트를 유지보수하다가
2026-08-11 세션에서 실제 wire-format과 다르게 가정한 버그를 3건 연달아 발견·수정한
뒤, 공식 SDK로 전환하기로 결정했다(배경: `docs/architecture/overview.md`,
대화 기록 참고). 단, 이 SDK의 WebSocket 클라이언트(`agent-client-protocol-http`)는
mTLS connector 주입 경로가 없어(`agent-client-protocol-http/src/client.rs::run_ws`가
`async_tungstenite::tokio::connect_async(endpoint.as_str())`를 인자 없이 호출),
Fleet의 Phase 8.5 mTLS 요구사항을 만족하지 못했다. 업스트림에 PR을 낼 수도 있지만
그동안 로컬 패치로 막고 있다.

## 패치 내용 (`src/agent-client-protocol-http` 한정)

- `src/client.rs`:
  - `HttpClient`에 `tls_connector: Option<tokio_rustls::TlsConnector>` 필드 추가.
  - `HttpClient::with_tls_connector(connector)` 빌더 메서드 추가.
  - `run_ws()`가 `connect_async` 대신 `connect_async_with_tls_connector_and_config`를
    호출하도록 변경 — `connector: None`일 때는 기존과 완전히 동일한 기본(공용 CA) 동작.
  - 관련 구조체 분해(destructure) 두 곳(`run()`, `run_ws()`)에 `..`/새 필드 추가.
- `Cargo.toml` (workspace root) + `src/agent-client-protocol-http/Cargo.toml`:
  - `tokio-rustls = "0.26"` 의존성 추가 (Fleet 워크스페이스와 동일 버전 — 의존성
    트리에서 rustls/tokio-rustls 중복 버전 방지).
  - `client` feature에 `dep:tokio-rustls` 추가.

모든 변경에 `// FLEET PATCH (2026-08-11):` 주석을 달아 upstream diff와 구분되게 했다.

## 업스트림 동기화 정책

- 이 디렉토리는 **Fleet 저장소에 통째로 커밋된다** (git submodule 아님) — 재현성 우선,
  `.git` 메타데이터는 제거했다.
- 업스트림을 다시 당겨올 때는: 새 커밋을 별도로 clone → 이 파일에 적힌 패치들을
  수동으로 재적용 → `VENDORED_COMMIT.txt` 갱신 → 회귀 테스트 재실행.
- 자동 diff 스크립트는 아직 없다 (패치 3곳뿐이라 수동 관리가 더 안전하다고 판단).

## 관련 파일

- `/Users/yarang/working/tools/grok-fleet-orchestrator/crates/fleet-transport/` — 이
  벤더 crate를 소비하는 쪽.
- `docs/architecture/overview.md` §"왜 xai-computer-hub-sdk가 아닌가" 및 ACP Transport
  섹션 — 마이그레이션 배경 문서(갱신 예정).
