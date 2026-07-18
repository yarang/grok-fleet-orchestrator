# Grok Fleet Orchestrator

다수의 Linux 서버에 분산된 [Grok Build](https://github.com/xai-org/grok-build) 에이전트들을
통합 관리하는 독립적인 Rust 오케스트레이터입니다.

표준 **MCP(Model Context Protocol)** 서버로 노출되어, grok build를 비롯해
Claude Code, Gemini CLI, Codex, Cursor 등 MCP를 지원하는 모든 AI 코딩 도구에서
원격 워커 풀을 동일한 인터페이스로 사용할 수 있습니다.

> **상태**: 0.1.0 개발 중 — Phase 1~6 완료. Phase 7(통합 테스트, docs 최종화) 진행 예정.

## 주요 특징

- **비동기 작업 디스패치**: 장기 실행 작업을 원격 워커에 제출하고 `task_id`로 추적
- **다중 워커 관리**: 사용자 지정 (`server_hint`) 또는 least-loaded 자동 선택
- **장애 격리**: 워커별 CircuitBreaker로 연속 실패 시 자동 차단
- **PostgreSQL 백엔드**: 다중 admin 동시 쓰기 + LISTEN/NOTIFY 실시간 동기화
- **Cloudflare Zero Trust**: 인터넷 노출 서버도 인바운드 포트 없이 운영
- **SSH 자동 프로비저닝**: SSH 키만 있으면 grok + cloudflared + fleet-worker 원클릭 설치
- **웹 대시보드**: 실시간 현황 + SSE 이벤트 스트리밍 (단일 바이너리 임베드)
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
```

각 명령에 `--help`를 붙여 상세 옵션을 확인하세요.

## MCP 도구

AI 클라이언트에 노출되는 7개 MCP 도구:

| 도구                    | 용도                                            |
|-------------------------|-------------------------------------------------|
| `submit_task`           | 프롬프트를 작업으로 큐에 등록                   |
| `get_task_status`       | 작업 상태 조회                                  |
| `wait_for_task`         | 작업 완료까지 대기 (타임아웃 옵션)              |
| `cancel_task`           | 실행 중인 작업 취소                             |
| `list_workers`          | 등록된 워커 조회                                |
| `stream_task_output`    | 작업 stdout/stderr 폴링 스트리밍                |
| `collect_results`       | 다수 작업 결과를 병렬 수집                      |

## 크레이트 구조

| 크레이트              | 역할                                            |
|-----------------------|-------------------------------------------------|
| `fleet-core`          | 도메인 모델 (Task, Worker, FleetEvent) — leaf    |
| `fleet-store`         | `Store` trait + PostgreSQL 구현 + LISTEN/NOTIFY |
| `fleet-transport`     | `WorkerTransport` trait + Mock 구현              |
| `fleet-scheduler`     | WorkerSelector, CircuitBreaker, Dispatcher       |
| `fleet-mcp`           | MCP JSON-RPC 서버 (7개 도구)                     |
| `fleet-api`           | HTTP API 서버 (워커 등록, 하트비트, /metrics)    |
| `fleet-provisioner`   | russh 기반 SSH 자동화 + Playbook                 |
| `fleet-dashboard`     | 웹 대시보드 (rust-embed 임베드, 순수 HTML)        |
| `fleet-cli`           | CLI 바이너리 (`fleet` 명령)                      |

> **설계 결정**: [Grok Build](https://github.com/xai-org/grok-build)를 포크하지 않고
> 독립 프로젝트로 구축했습니다. Fleet은 MCP 표준을 통해 어떤 AI 코딩 도구와도
> 연동되며, Grok Build는 워커로서 사용할 수 있습니다.

## 문서

- [`docs/architecture.md`](docs/architecture.md) — 시스템 아키텍처, 데이터 흐름, 핵심 추상화
- [`docs/api-reference.md`](docs/api-reference.md) — HTTP API + MCP 도구 레퍼런스
- [`docs/deployment.md`](docs/deployment.md) — 단일 서버 및 분산 배포 가이드 (Cloudflare Tunnel 포함)

## 라이선스

MIT OR Apache-2.0
