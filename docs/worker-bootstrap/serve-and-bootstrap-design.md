# fleet serve & 대시보드 상세 설계 및 워커 부트스트랩/운영 절차서

이 설계서는 **Grok Fleet Orchestrator**의 핵심 서버 엔진인 `fleet serve`와 **웹 대시보드(Web Dashboard)**의 모듈 및 통신 설계를 다루며, 워커 노드의 **최초 등록(Bootstrap) 및 일상 운영 절차(Operational Lifecycle)**를 Mermaid 시퀀스 다이어그램으로 정의합니다.

---

## 1. `fleet serve` 상세 설계 (Server Engine Design)

`fleet serve`는 단일 Rust 바이너리로 기동하며, 내부적으로 멀티스레드 비동기 런타임(Tokio) 상에서 다중 프로토콜 엔드포인트를 병렬 실행합니다.

![fleet serve 모듈 맵 — HTTP API Server / MCP stdio Server / Background Loops 3분기 아키텍처](../assets/diagrams/worker-bootstrap/fleet-serve-module-map.svg)

> 이 다이어그램은 [`bootstrap-release-v0.2.md §1`](./bootstrap-release-v0.2.md)과 공유합니다 — 갱신 시 두 문서 모두 확인하세요. 아래 §1.1 서술은 2026-08-12 코드 대조로 정정되었습니다(원래 "7개 MCP 도구", "1초 폴링 디스패처", "`/dashboard` 정적 자산" 서술이었으나 실제 코드와 달랐습니다 — 상세 근거는 `bootstrap-release-v0.2.md §1` 표 참고).

### 1.1 주요 모듈 구성
1. **Axum HTTP Router**:
   * **API 엔드포인트**: 워커의 등록/해제 및 하트비트 통신 처리, 마스터 키 기반의 credentials 암호화 보관/조회 API 제공.
   * **정적 자산 서빙**: 대시보드는 별도 크레이트 `fleet-dashboard`가 자체 라우터(`/`, `/tasks`, `/hosts`, `/admin/*` 등)로 서빙하며, `fleet serve`가 HTTP API 서버와 함께 기동합니다.
2. **MCP stdio JSON-RPC 엔진**:
   * AI 코딩 클라이언트(Cursor, Claude Code 등)가 실행한 서브프로세스 표준 입출력(stdin/stdout) 채널을 통해 JSON-RPC 2.0 규격으로 **12개**(2026-08-13 기준, 최초 8개 + 호스트/브레이커/토큰 관리 4개)의 MCP 도구(`fleet_dispatch_task`, `fleet_get_task_status`, `fleet_list_workers`, `fleet_list_tasks`, `fleet_cancel_task`, `fleet_wait_for_task`, `fleet_stream_task_output`, `fleet_collect_results`, `fleet_list_hosts`, `fleet_reset_worker_breaker`, `fleet_list_bootstrap_tokens`, `fleet_revoke_bootstrap_token`)를 노출 (`crates/fleet-mcp/src/schema.rs`).
3. **태스크 디스패처 (Dispatcher Loop)**:
   * **이벤트 기반**으로 동작합니다 — `mpsc` 채널로 전달되는 태스크를 즉시 소비해 위임합니다(`crates/fleet-scheduler/src/dispatcher.rs`). 별도로 정체된(`Pending`/`Dispatched`) 태스크를 쓸어가는 **Reconciler**가 **30초** 주기 안전망으로 동작합니다(`crates/fleet-scheduler/src/reconcile.rs`). 테이블명은 `fleet_tasks`가 아니라 `tasks`입니다.
   * `WorkerSelector` 모듈을 통해 `Closed` 상태의 회로차단기를 가졌고 CPU/Memory 여유 용량이 남은 워커를 찾아 ACP(Agent Client Protocol) over WebSocket 채널로 작업을 위임.
4. **헬스체커 루프 (Health Checker Loop)**:
   * 15초 간격으로 가동되며, 최근 45초(3회 이상 미수신) 동안 하트비트가 수집되지 않은 온라인 워커 노드들의 상태를 `Offline`으로 변경하고, 해당 워커의 회로차단기를 즉시 강제 개방(Open)하여 태스크 할당을 차단. (`crates/fleet-scheduler/src/health.rs`, 코드와 일치 확인됨)

---

## 2. 웹 대시보드 상세 설계 (Web Dashboard Design)

대시보드는 관리자와 일반 사용자가 인프라의 상태를 실시간 파악하고 제어할 수 있는 모니터링 콘솔입니다.

### 2.1 보안 및 인증 모델 (Security & Session)
* **비밀번호 해싱**: Argon2id 알고리즘을 사용해 비밀번호를 단방향 암호화하여 DB에 안전히 보관.
* **쿠키 기반 세션**: 로그인 성공 시 암호화 서명된 Stateful 세션 쿠키를 발급하며, 세션 만료 시간 및 무효화 처리는 오케스트레이터 메모리/DB 세션 테이블을 통해 중앙 통제.
* **역할 기반 권한 제어 (RBAC)**:
  * `Admin`: 부트스트랩 토큰 발급/삭제, 유저 계정 비활성화, 워커 제거 및 CA 인증서 제어.
  * `User`: 작업 목록 보기, 작업 강제 취소, 워커 메트릭 조회만 허용.
  * ⚠️ **정정 (2026-08-12)**: 위 Admin/User 2역할 서술은 단순화된 예시입니다. 실제로는 **22종으로 세분화된 `PermissionKind`**(`crates/fleet-core/src/auth.rs`) 기반 RBAC이며, 기본 제공 역할도 `Admin`/`Operator`/`Viewer` 3종입니다. 역할은 `RoleCreate`로 확장 가능해 커스텀 역할에 특정 권한만 부여할 수 있습니다. 상세는 [`bootstrap-release-v0.2.md §3.2.2`](./bootstrap-release-v0.2.md)를 참조하세요.

### 2.2 실시간 데이터 스트리밍 (Server-Sent Events)
* 웹 브라우저 대시보드의 실시간성(로그 실시간 출력, 시스템 이벤트 타임라인 업데이트)을 확보하기 위해, 폴링(Polling) 대신 **SSE(Server-Sent Events)** 엔드포인트(`/api/events/stream`)를 활용합니다.
* PostgreSQL의 `LISTEN / NOTIFY` 트리거를 리스닝하고 있는 오케스트레이터 커넥션이 알림을 수신하는 즉시 SSE 채널을 통해 모든 접속 중인 관리자 브라우저로 1ms 이내로 전송합니다.

---

## 3. 워커 부트스트랩(Bootstrap) 절차 다이어그램

워커가 최초로 오케스트레이터에 인프라 노드로 조인하고 자동 구성 설정(worker.toml)을 받아 가동되는 절차입니다.

![Worker Bootstrap Sequence Diagram](../assets/diagrams/worker-bootstrap/bootstrap-sequence.mermaid)

---

## 4. 워커 운영 수명 주기 절차 다이어그램 (Operational Lifecycle)

워커가 구동된 후 일상적인 하트비트 송신, 작업 수신/실행, 로그 청크 스트리밍 및 장애 감지 시의 회로차단기 동작을 보여주는 종합 운영 시퀀스입니다.

![Worker Lifecycle Sequence Diagram](../assets/diagrams/worker-bootstrap/operational-lifecycle-sequence.mermaid)
