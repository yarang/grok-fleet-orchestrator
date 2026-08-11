# Grok Fleet Orchestrator

다수의 Linux 서버에 분산된 [Grok Build](https://github.com/xai-org/grok-build) 에이전트들을
통합 관리하는 독립적인 Rust 오케스트레이터입니다.

표준 **MCP(Model Context Protocol)** 서버로 노출되어, grok build를 비롯해
Claude Code, Gemini CLI, Codex, Cursor 등 MCP를 지원하는 모든 AI 코딩 도구에서
원격 워커 풀을 동일한 인터페이스로 사용할 수 있습니다.

> **상태**: 0.1.0 — Phase 1~9(P2) 완료. 호스트 인벤토리, RBAC 대시보드 인증, 토큰 추적 구현됨.

## 설치

세 가지 방법. 운영 환경에서는 (A) 가 기본, cargo 가 있다면 (B) 도 가능.

### (A) install.sh (curl | bash)

```bash
curl -fsSL https://github.com/yarang/grok-fleet-orchestrator/releases/latest/download/install.sh \
  | bash
```

`fleet` + `fleet-worker` 바이너리를 `~/.local/bin` 에 설치하고 PATH 에 추가.
`--bin-dir`, `--version`, `--build`, `--uninstall` 등의 플래그 지원 (`install.sh --help`).

### (B) cargo-binstall

```bash
cargo binstall --git https://github.com/yarang/grok-fleet-orchestrator fleet-cli fleet-worker
```

### (C) 소스 빌드

```bash
git clone https://github.com/yarang/grok-fleet-orchestrator
cd grok-fleet-orchestrator
cargo build --release --features "acp mtls"
# target/release/{fleet, fleet-worker}
```

운영용 샘플 설정은 `examples/` 디렉토리 참조 (`worker.toml`, `workers.yaml`,
`fleet.service`, `fleet-worker.service`, `fleet.env`, `mcp-clients.json`).
자세한 설치 절차는 [`docs/deployment.md`](docs/deployment/deployment.md#0-설치) 의 §0.

## 주요 특징

- **ACP transport**: 표준 [Agent Client Protocol](https://github.com/Zed-Industries/agent-client-protocol) over WebSocket으로 각 워커의 `grok agent serve`와 통신 (`--transport acp`)
- **fleet-worker 데몬**: 워커 머신에서 `grok agent serve` 서브프로세스 관리 + orchestrator 자동 등록/하트비트 (graceful shutdown, systemd 통합)
- **비동기 작업 디스패치**: 장기 실행 작업을 원격 워커에 제출하고 `task_id`로 추적
- **다중 워커 관리**: 사용자 지정 (`server_hint`) 또는 least-loaded 자동 선택
- **장애 격리**: 워커별 CircuitBreaker로 연속 실패 시 자동 차단
- **PostgreSQL 백엔드**: 다중 admin 동시 쓰기 + LISTEN/NOTIFY 실시간 동기화
- **Cloudflare Zero Trust**: 인터넷 노출 서버도 인바운드 포트 없이 운영
- **SSH 자동 프로비저닝**: SSH 키만 있으면 grok + cloudflared + fleet-worker 원클릭 설치
- **호스트 인벤토리**: 등록 여부와 무관하게 인프라 전체 호스트 추적 (grok 버전, OS, 상태, 이벤트 타임라인)
- **웹 대시보드**: 8개 페이지 — Overview, Task Queue, Worker Detail, Hosts, Host Detail, User Management, Audit Log, MCP Tools (Apple Design System, RBAC + 쿠키 세션 인증)
- **LLM 토큰 추적**: ACP 프로토콜에서 토큰 사용량 수집 → Prometheus 메트릭 + 대시보드 표시
- **RBAC + 세션 인증**: Argon2id 비밀번호, stateful 쿠키 세션, 역할/권한 기반 접근 제어
- **감사 로그**: 모든 상태 변화가 append-only 이벤트 로그에 기록
- **Prometheus 메트릭**: `/metrics` 엔드포인트로 스크랩

## 빠른 시작

```bash
# 1. Postgres 준비 (로컬 docker 또는 brew install postgresql@16)
createdb fleet_dev
export DATABASE_URL=postgres://yarang@localhost/fleet_dev

# 2. 빌드
cargo build --release

# 3. 마이그레이션
./target/release/fleet migrate

# 4. 서버 시작 (MCP stdio + HTTP API + 대시보드)
./target/release/fleet serve \
  --http-bind 127.0.0.1:8081 \
  --dashboard-bind 127.0.0.1:8082

# 5. 다른 터미널에서 grok build 등 MCP 클라이언트에 fleet 연결
#    (예: ~/.config/grok/mcp.json 또는 claude_desktop_config.json)
{
  "mcpServers": {
    "fleet": { "command": "/path/to/fleet", "args": ["serve"] }
  }
}
```

## CLI 명령

```
fleet serve          # MCP stdio + HTTP API + 대시보드 (메인 서버)
fleet migrate        # DB 마이그레이션만 실행
fleet workers list   # 등록된 워커 목록 (--json 지원)
fleet workers show <name>
fleet tasks list     # 작업 목록 (--status, --limit, --json)
fleet tasks show <id>
fleet tasks cancel <id> [--reason "..."]
fleet events list    # 감사 로그 (--after-seq, --limit, --json)
fleet token new      # 부트스트랩 토큰 생성
fleet doctor         # 인프라 진단 (DB, 마이그레이션, 워커, API, 대시보드)
fleet provision ...  # SSH 자동 프로비저닝 (단일/인벤토리)
fleet mtls init-ca/issue-server/issue-client  # 사설 CA + 인증서 발급 (--features mtls)
fleet credentials issue/revoke/list  # 워커 API 키 관리 (--features push-credentials)
fleet users create/list/delete/enable/disable/role  # 사용자 + RBAC 관리
```

각 명령에 `--help`를 붙여 상세 옵션을 확인하세요.

## MCP 도구

AI 클라이언트에 노출되는 7개 MCP 도구:

| 도구                       | 용도                                            |
|----------------------------|-------------------------------------------------|
| `fleet_dispatch_task`      | 프롬프트를 작업으로 큐에 등록                   |
| `fleet_get_task_status`    | 작업 상태 조회                                  |
| `fleet_wait_for_task`      | 작업 완료까지 대기 (타임아웃 옵션)              |
| `fleet_cancel_task`        | 실행 중인 작업 취소                             |
| `fleet_list_workers`       | 등록된 워커 조회                                |
| `fleet_stream_task_output` | 작업 stdout/stderr 폴링 스트리밍                |
| `fleet_collect_results`    | 다수 작업 결과를 병렬 수집                      |

## 크레이트 구조

| 크레이트              | 역할                                            |
|-----------------------|-------------------------------------------------|
| `fleet-core`          | 도메인 모델 (Task, Worker, Host, FleetEvent) — leaf |
| `fleet-store`         | `Store` trait + PostgreSQL 구현 + LISTEN/NOTIFY |
| `fleet-transport`     | `WorkerTransport` trait + ACP 구현 + Mock        |
| `fleet-scheduler`     | WorkerSelector, CircuitBreaker, Dispatcher, Health |
| `fleet-mcp`           | MCP JSON-RPC 서버 (7개 도구)                     |
| `fleet-api`           | HTTP API 서버 (워커 등록, 하트비트, 호스트 등록, /metrics) |
| `fleet-provisioner`   | russh 기반 SSH 자동화 + Playbook                 |
| `fleet-dashboard`     | 웹 대시보드 (8페이지, rust-embed 임베드, RBAC 인증) |
| `fleet-credentials`   | 워커 API 키 AES-256-GCM 암호화 저장              |
| `fleet-cli`           | CLI 바이너리 (`fleet` 명령)                      |
| `fleet-worker`        | 워커 데몬 (`fleet-worker` 명령)                  |

> **설계 결정**: [Grok Build](https://github.com/xai-org/grok-build)를 포크하지 않고
> 독립 프로젝트로 구축했습니다. Fleet은 MCP 표준을 통해 어떤 AI 코딩 도구와도
> 연동되며, Grok Build는 워커로서 사용할 수 있습니다.

## 문서

- [`docs/index.md`](docs/index.md) — 전체 설계·운영 문서 카탈로그 (정본/사본 상태, 최종 개정일)
- [`docs/architecture/overview.md`](docs/architecture/overview.md) — 시스템 아키텍처, 데이터 흐름, 핵심 추상화
- [`docs/architecture/api-reference.md`](docs/architecture/api-reference.md) — HTTP API + MCP 도구 레퍼런스
- [`docs/deployment/deployment.md`](docs/deployment/deployment.md) — 단일 서버 및 분산 배포 가이드 (Cloudflare Tunnel 포함)
- [`docs/ui-dashboard/ui-design.md`](docs/ui-dashboard/ui-design.md) — 웹 대시보드 화면 설계서 (8개 페이지, 사용자 흐름, 디자인 시스템, 구현 우선순위)
- [`DESIGN-apple.md`](DESIGN-apple.md) — Apple Design System 스펙 (색상, 타이포그래피, 컴포넌트)
- [`examples/`](examples/) — 운영용 샘플 설정 (`worker.toml`, `workers.yaml`, systemd units, MCP 클라이언트 예시)

## 라이선스

MIT OR Apache-2.0
