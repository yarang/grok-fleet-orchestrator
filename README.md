# Grok Fleet Orchestrator

다수의 Linux 서버에 분산된 [Grok Build](https://github.com/xai-org/grok-build) 에이전트들을
통합 관리하는 오케스트레이터입니다.

표준 **MCP(Model Context Protocol)** 서버로 노출되어, grok build를 비롯해
Claude Code, Gemini CLI, Codex, Cursor 등 MCP를 지원하는 모든 AI 코딩 도구에서
원격 워커 풀을 동일한 인터페이스로 사용할 수 있습니다.

## 주요 특징

- **비동기 작업 디스패치**: 장기 실행 작업을 원격 워커에 제출하고 `task_id`로 추적
- **다중 워커 관리**: 사용자 지정 (`server_hint`) 또는 least-loaded 자동 선택
- **장애 격리**: 워커별 CircuitBreaker로 연속 실패 시 자동 차단
- **PostgreSQL 백엔드**: 다중 admin 동시 쓰기 + LISTEN/NOTIFY 실시간 동기화
- **Cloudflare Zero Trust**: 인터넷 노출 서버도 인바운드 포트 없이 운영
- **SSH 자동 프로비저닝**: SSH 키만 있으면 grok + cloudflared + fleet-worker 원클릭 설치
- **웹 대시보드**: 실시간 현황 + SSE 이벤트 스트리밍 (단일 바이너리 임베드)

## 크레이트 구조

| 크레이트 | 역할 |
|----------|------|
| `fleet-core` | 도메인 모델 (Task, Worker, FleetEvent) — 의존성 없는 leaf |
| `fleet-store` | `Store` trait + PostgreSQL 구현 |
| `fleet-transport` | `HubConnectionPool` 래핑 + OIDC 통합 |
| `fleet-scheduler` | WorkerSelector, CircuitBreaker, Dispatcher |
| `fleet-mcp` | MCP 서버 (9개 도구) |
| `fleet-api` | HTTP API 서버 (워커 등록/하트비트) |
| `fleet-worker-sidecar` | 워커 머신 데몬 |
| `fleet-provisioner` | russh 기반 SSH 자동화 |
| `fleet-dashboard` | React + Vite 프론트엔드 (rust-embed 임베드) |
| `fleet-cli` | CLI 바이너리 |

## 상태

🚧 **개발 중** — 현재 `fleet-core` 도메인 모델 구현 단계.

전체 설계 문서는 [`docs/architecture.md`](docs/architecture.md)를 참조하세요.

## 라이선스

MIT OR Apache-2.0
