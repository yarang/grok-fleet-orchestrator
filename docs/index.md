---
type: wiki
status: canonical
source: "docs/index.md"
last_verified: "2026-08-15"
---

# Docs — 인덱스 (Index)

> `docs/`의 모든 설계·운영 문서를 도메인별 하위 디렉토리로 재배치한 뒤의 콘텐츠 지향
> 목록이다. 새 문서를 추가하거나 기존 문서의 상태(정본/사본/폐기)가 바뀌면 이 표를 함께
> 갱신하고 [`log.md`](./log.md)에 항목을 남긴다. `docs/llm-wiki/`와 `docs/credentials/`는
> 각자의 `README.md`/`index.md`(또는 `registry.md`)를 이미 갖추고 있으므로 이 인덱스에서는
> 디렉토리 단위로만 참조한다.
>
> **운영 규칙**은 [`docs/llm-wiki/README.md`](./llm-wiki/README.md)의 스키마(정본/사본 구분,
> ingest/query/lint 워크플로우, 필수 부기 파일)를 그대로 따른다. 각 도메인 디렉토리에는
> 그 도메인 문서 간 관계를 설명하는 `README.md`가 있다.

## 상태 범례

| 기호 | 의미 |
|---|---|
| 🟢 | 정본 — 해당 주제의 현재 신뢰할 수 있는 출처 |
| 🔵 | 사본 — 다른 정본을 인용/발췌. 값이 어긋나면 정본이 우선, 정본을 먼저 고친 뒤 동기화 |
| 🟡 | 부분 수정됨 — 과거 모순이 발견되어 일부만 정정, 전체 재검토 전 |
| ⚪ | 역사적 기록 — 특정 시점의 스냅샷. 현재 지침으로 사용하지 않음 |
| ⚫ | 고아/폐기 — 다른 문서에서 참조되지 않거나 폐기 판정. 병합/삭제 검토 대상 |

## 도메인 1. 🏛️ Core Reference — [`architecture/README.md`](./architecture/README.md)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`architecture/overview.md`](./architecture/overview.md) | Store trait·CircuitBreaker·WorkerSelector·ACP 전송·워커 데몬·mTLS·부트스트랩 토큰 등 내부 설계 마스터 문서 | 🟢 정본 | 2026-08-10 |
| [`architecture/api-reference.md`](./architecture/api-reference.md) | HTTP REST + MCP 12종 도구 API 레퍼런스 (2026-08-12 코드 대조로 도구명/개수 정정, 2026-08-13 호스트/브레이커/토큰 4종 추가) | 🟢 정본 | 2026-08-13 |
| [`architecture/mcp-specification.md`](./architecture/mcp-specification.md) | Model Context Protocol(MCP) JSON-RPC over stdio 표준 사양 및 12종 도구 연동 스펙 | 🟢 정본 — §4 도구 목록 정정 완료(2026-08-12), 4종 추가 반영(2026-08-13) | 2026-08-13 |
| [`architecture/project-feature-design.md`](./architecture/project-feature-design.md) | 워커와 호스트를 배타적(1:N)으로 격리·배정하는 프로젝트 격리 단위 및 하드 디스패치 설계 마스터 문서 | 🟢 정본 | 2026-08-15 |
| [`architecture/agent-provisioning-design.md`](./architecture/agent-provisioning-design.md) | 에이전트를 Worker와 분리하여 custom prompt, 메모리, MCP 도구를 동적으로 프로비저닝하는 수명주기 및 데이터 모델 설계 문서 | 🟢 정본 | 2026-08-15 |
| [`architecture/agent-terminal-access-design.md`](./architecture/agent-terminal-access-design.md) | grok 에이전트를 tmux 내 실행하고 스냅샷 폴링 및 russh PTY WebSocket 중계를 통한 대화형 attach 기능 설계 문서 | 🟢 정본 | 2026-08-15 |
| [`architecture/agent-harness-composition-design.md`](./architecture/agent-harness-composition-design.md) | prompt ➔ skill ➔ tool의 3계층 하네스 구조 및 프로젝트 헌법(constitution) 주입 규칙 설계 문서 | 🟢 정본 | 2026-08-15 |
| [`architecture/agent-runtime-vendor-design.md`](./architecture/agent-runtime-vendor-design.md) | AgentRunner 트레잇 기반 NetworkBind(grok) 및 StdioBridge(Gemini CLI) 다중 벤더 수용 설계 문서 | 🟢 정본 | 2026-08-15 |
| [`architecture/system-entities-mapping.md`](./architecture/system-entities-mapping.md) | Project, Host, Worker, Agent, Task 간의 물리 배치(WHERE), 행동 구성(WHAT), 스코프 결정(WHEN) 관계 맵 및 매핑 불변식 명세 | 🟢 정본 | 2026-08-15 |
| [`architecture/system-entities-critique.md`](./architecture/system-entities-critique.md) | 위 관계 맵의 동시성 병목(FOR UPDATE), pg_notify 유실, 격리 우회, 토큰 인플레이션에 대한 비판적 분석 및 대안 제안서 | 🟢 정본 | 2026-08-15 |
| [`architecture/multi-agent-realignment-report.md`](./architecture/multi-agent-realignment-report.md) | 코어/인프라/운영 도메인 간의 재배정 드레인, Nginx WebSocket, 토큰 데드락, 유실 방지 동기화 논의 및 의사결정 선택지 | 🟢 정본 | 2026-08-15 |
| [`architecture/feature-feasibility-testing.md`](./architecture/feature-feasibility-testing.md) | 드레인, Git 이관, 동적 스킬, 다중 에이전트 체이닝 구현에 필요한 기술 분석 및 로컬 테스트/검증 시나리오 명세서 | 🟢 정본 | 2026-08-15 |
| [`architecture/host-integrity-and-security-monitoring-design.md`](./architecture/host-integrity-and-security-monitoring-design.md) | 워커 비관리 파일/패키지 변경 실시간 커널 감시, 3계층 필터링 및 온디맨드 LLM 위험성 분석 아키텍처 보고서 | 🟢 정본 | 2026-08-16 |
| [`architecture/intelligent-task-routing-and-budget-control-design.md`](./architecture/intelligent-task-routing-and-budget-control-design.md) | FreeRouter 흡수 2단계 라우팅, 3단계 소프트 예산 차단기, Compact 엔진 및 무비용 텔레메트리 설계서 | 🟢 정본 | 2026-08-16 |
| [`architecture/log.md`](./architecture/log.md) | 아키텍처 도메인 설계 개정 이력 기록용 append-only 로그 파일 | ⚪ 부기 문서 | 2026-08-15 |

## 도메인 2. 🚀 Deployment & Infra — [`deployment/README.md`](./deployment/README.md)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`deployment/deployment.md`](./deployment/deployment.md) | 설치부터 프로덕션까지 전 과정 배포 가이드. §2.3이 Nginx 리버스 프록시의 정본 (2026-08-12 코드 대조로 install.sh 기본 경로·systemd stdin 워크어라운드 등 정정) | 🟢 정본 | 2026-08-12 |
| [`deployment/server-topology.md`](./deployment/server-topology.md) | 오케스트레이터-대시보드-워커 물리/논리 망 구성도 (2026-08-12 코드 대조로 liteLLM 폐기된 Docker 설계 서술 정정) | 🟢 정본 — 토폴로지 | 2026-08-12 |
| [`deployment/nginx-gateway.md`](./deployment/nginx-gateway.md) | Caddy→Nginx 전환 결정서 (비교표, nginx.conf, 전환 절차) — 2026-08-12에 단일 서버 예시의 누락된 `/v1/` API 라우팅 블록 추가 | 🟢 정본 — 결정 기록 | 2026-08-12 |
| [`deployment/single-server.md`](./deployment/single-server.md) | 단일 VM Docker Compose 배포 가이드 | 🔵 사본 — liteLLM 절은 `llm-wiki/`, 리버스 프록시 절은 `nginx-gateway.md` 인용 (2026-08-12에 `FLEET_LLM_GATEWAY_URL` 필수 여부 정정) | 2026-08-12 |
| [`deployment/historical/2026-07-20-arm2-arm1-deploy-log.md`](./deployment/historical/2026-07-20-arm2-arm1-deploy-log.md) | 실제 arm1/arm2 배포 시점 기록(Caddy 시절, ACP 인증 디버깅) | ⚪ 역사적 기록 | 2026-07-20 |

## 도메인 3. 🔑 Worker Bootstrap & Join Auth — [`worker-bootstrap/README.md`](./worker-bootstrap/README.md)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`worker-bootstrap/join-authentication.md`](./worker-bootstrap/join-authentication.md) | 부트스트랩 토큰 인증 + Cloudflare Access 이중 방어 설계 | 🟢 정본 | 2026-08-06 |
| [`worker-bootstrap/token-delivery.md`](./worker-bootstrap/token-delivery.md) | 토큰 전달 3가지 방식 비교(SSH 자동주입/수동 CLI/cloud-init), SSH 자동주입 권장 | 🟢 정본 | 2026-08-06 |
| [`worker-bootstrap/ssh-provisioning.md`](./worker-bootstrap/ssh-provisioning.md) | SSH 자동 프로비저닝 구현 명세 (시퀀스 다이어그램 + Rust 의사코드) | 🟢 정본 | 2026-08-06 |
| [`worker-bootstrap/serve-and-bootstrap-design.md`](./worker-bootstrap/serve-and-bootstrap-design.md) | `fleet serve` 모듈 설계(Axum/MCP/디스패처/헬스체커) + 대시보드 RBAC/SSE + 부트스트랩 시퀀스 | 🟢 정본 | 2026-08-06 |
| [`worker-bootstrap/bootstrap-release-v0.2.md`](./worker-bootstrap/bootstrap-release-v0.2.md) | 워커 설치/조인/프로비저닝 구현 현황 요약·색인 (코드 대비 검증, 상세는 위 4개 문서로 위임) | 🟢 정본 | 2026-08-12 |

## 도메인 4. 🛠️ Server Management & Self-Healing (로드맵 제안서, 미구현) — [`server-management/README.md`](./server-management/README.md)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`server-management/advanced-management-proposals.md`](./server-management/advanced-management-proposals.md) | SSH 키 회수, UFW/Fail2ban 관리, 설정 드리프트 감지, 네트워크 지연 진단, SMART/하드웨어 헬스체크 제안 | 🟢 정본 — 제안 | 2026-08-06 |
| [`server-management/linux-package-management.md`](./server-management/linux-package-management.md) | APT/DNF 래퍼 설계, PackageKit D-Bus vs sudoers 화이트리스트 권한 위임 | 🟢 정본 — 제안 | 2026-08-06 |
| [`server-management/hardware-healing.md`](./server-management/hardware-healing.md) | GPU 스로틀/스톨 감지(NVML) + 클라우드/베어메탈 차등 자가치유 + 서킷브레이커 DB 공유 스펙 | 🟢 정본 — 제안, 로드맵 #25 연계 | 2026-08-06 |

## 도메인 5. 📋 Roadmap & Planning

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`roadmap/roadmap.md`](./roadmap/roadmap.md) | P0~P3 백로그/상태 마스터 트래커 — 가장 권위 있는 "지금 실제로 무엇이 참인가" 문서 | 🟢 정본 | 2026-08-09 |
| [`roadmap/conflict-analysis.md`](./roadmap/conflict-analysis.md) | Caddy→Nginx 결정 + S1~S6 수정이 백로그 우선순위에 미치는 영향 분석 | 🟢 정본 — roadmap.md 위성 문서 | 2026-08-06 |

## 도메인 6. 🔒 Security

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`security/findings.md`](./security/findings.md) | S1~S6 보안 결함(로그인 락아웃 증폭, JWT/JWKS 검증, Real IP 추출 등) 해결 보고서 | 🟢 정본 | 2026-08-06 |

## 도메인 7. 🎨 UI / Dashboard

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`ui-dashboard/ui-design.md`](./ui-dashboard/ui-design.md) | 18개 대시보드 라우트 IA·플로우·컴포넌트 카탈로그(원안 "8개 페이지" 오기재 정정됨). SSH Config 자동 임포트 UI 흐름 추가 | 🟢 정본 — 2026-08-13 전체 절 코드 대조 완료(호스트 인벤토리 재분류, StatusPill/HostStatus enum, MCP 도구 12개 반영) | 2026-08-13 |
| [`../DESIGN-apple.md`](../DESIGN-apple.md) | 실제 적용된 Apple Design System 토큰(Action Blue, SF Pro, parchment, pill CTA) | 🟢 정본 — 루트 문서 | 2026-07-27 |
| [`../DESIGN-notion.md`](../DESIGN-notion.md) | 이전 Notion 테마 원안 분석 (폐기, 현재는 Apple 정본으로 대체) | ⚫ 폐기(미채택) — 루트 문서 | 2026-07-20 |

## 도메인 8. 🧩 Engineering Patterns (기타)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`engineering-patterns/reuse-patterns.md`](./engineering-patterns/reuse-patterns.md) | `grok-build`/`xai-*` 코드베이스에서 채굴한 재사용 패턴 10종(RAII PendingGuard, 워커별 CircuitBreaker 등) — 10종 중 3종(#1,#3,#6)이 `fleet-scheduler` 코드 주석에 명시적으로 채택 근거로 인용됨을 확인 | 🔵 사본 — 실제 채택 근거는 `crates/fleet-scheduler/src/dispatcher.rs`/`breaker.rs` 코드 주석 | 2026-08-12 |
| [`engineering-patterns/documentation-policy.md`](./engineering-patterns/documentation-policy.md) | 문서의 정본/사본 정합성 관리 정책, 코드 실측 검증 원칙, 다이어그램/SVG 리소스 관리 규약 및 Ingest/Query/Lint 3단계 워크플로우 명세서 | 🟢 정본 | 2026-08-13 |

## 도메인 9. 📖 Agent & Assistant Guidelines

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`../agent.md`](../agent.md) | Git 정책·로드맵·품질/CI 게이트·LLM-Wiki 규약·다이어그램 및 SVG 리소스 관리 규약(§6) — 에이전트 협업 가이드 전문 | 🟢 정본 | 2026-08-12 |
| [`../CLAUDE.md`](../CLAUDE.md) | Claude Code 진입점. `@agent.md`를 임포트하고 문서 작성 지침(§6)만 요약 재기술 | 🔵 사본 — `agent.md`의 진입점/요약. 내용이 어긋나면 `agent.md`가 우선 | 2026-08-12 |
| [`../GEMINI.md`](../GEMINI.md) | Gemini / Google Antigravity (agy) 진입점. `@agent.md`를 임포트하고 Mermaid/SVG 문서 작성 및 에셋 지침 요약 | 🔵 사본 — `agent.md`의 진입점/요약. 내용이 어긋나면 `agent.md`가 우선 | 2026-08-16 |
| [`skills.md`](./skills.md) | 에이전트 스킬 시스템 사용 가이드 — `fleet tasks submit --skill <name>` CLI 사용법, `FLEET_SKILLS_DIR` 우선순위, 기본 제공 스킬 목록, 커스텀 스킬 작성 방법 | 🟢 정본 | 2026-08-16 |

## 하위 디렉토리 (자체 부기 체계 보유)

| 디렉토리 | 한 줄 요약 |
|---|---|
| [`llm-wiki/`](./llm-wiki/README.md) | LLM 게이트웨이(liteLLM) 채택 결정 및 인프라 스펙 위키 — 자체 `index.md`/`log.md` 보유 |
| [`credentials/`](./credentials/README.md) | 시크릿·크리덴셜 관리 지침 및 레지스트리 — 자체 `registry.md`(스냅샷+변경이력) 보유 |

## 고아 페이지 / 미해결 교차참조

- [`deployment/historical/2026-07-20-arm2-arm1-deploy-log.md`](./deployment/historical/2026-07-20-arm2-arm1-deploy-log.md) — 역사적 기록으로 유효, 현재
  지침 문서와는 명확히 분리되어 있어 조치 불필요.

_(2026-08-12: `engineering-patterns/reuse-patterns.md`의 고아 판정을 철회함 — 코드
주석에서 실제 채택 근거를 확인, 위 도메인 8 표 참조.)_

_(마지막 점검: 2026-08-11, [`log.md`](./log.md) 참고.)_
