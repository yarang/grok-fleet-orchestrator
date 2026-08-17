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

## 도메인 1. 🏛️ Core Reference

| 진입점 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`architecture/README.md`](./architecture/README.md) | Architecture 정본·Derived·review의 읽기 순서와 책임 경계 | 🟢 정본 | 2026-08-17 |

## 도메인 2. 🚀 Deployment & Infra — [`deployment/README.md`](./deployment/README.md)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`deployment/README.md`](./deployment/README.md) | 설치·구성·운영·복구·네트워크 경계의 도메인 진입점 | 🟢 정본 | 2026-08-17 |
| [`deployment/historical/2026-07-20-arm2-arm1-deploy-log.md`](./deployment/historical/2026-07-20-arm2-arm1-deploy-log.md) | 실제 arm1/arm2 배포 시점 기록(Caddy 시절, ACP 인증 디버깅) | ⚪ 역사적 기록 | 2026-07-20 |

## 도메인 3. 🔑 Worker Bootstrap & Join Auth — [`worker-bootstrap/README.md`](./worker-bootstrap/README.md)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`contracts/worker-enrollment.md`](./contracts/worker-enrollment.md) | join/register/heartbeat의 현재 구현·목표 보안 계약과 검증 게이트 | 🟢 정본 | 2026-08-17 |
| [`worker-bootstrap/join-authentication.md`](./worker-bootstrap/join-authentication.md) | Worker-scoped credential·edge 방어의 목표 가입 보안 모델 | 🔵 사본 — enrollment 계약 참조 | 2026-08-17 |
| [`worker-bootstrap/token-delivery.md`](./worker-bootstrap/token-delivery.md) | SSH·수동·cloud-init 전달 채널의 미구현 제안 비교 | 🟡 제안 | 2026-08-17 |
| [`worker-bootstrap/ssh-provisioning.md`](./worker-bootstrap/ssh-provisioning.md) | SSH host-key·provisioner 배경을 보존한 부분 구현 참조 | 🟡 부분 구현 | 2026-08-17 |
| [`worker-bootstrap/serve-and-bootstrap-design.md`](./worker-bootstrap/serve-and-bootstrap-design.md) | server/Dashboard/bootstrap 책임이 섞인 보존 참조 | ⚫ 폐기 | 2026-08-17 |
| [`worker-bootstrap/bootstrap-release-v0.2.md`](./worker-bootstrap/bootstrap-release-v0.2.md) | v0.2 시점 코드 대조 기반 구현 스냅샷 | 🔵 사본 | 2026-08-17 |

## 도메인 4. 🛠️ Operations Proposals (미구현) — [`operations/README.md`](./operations/README.md)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`operations/proposals/advanced-management-proposals.md`](./operations/proposals/advanced-management-proposals.md) | SSH 키 회수, UFW/Fail2ban, 설정 드리프트 감지, 네트워크 지연 진단, SMART/하드웨어 헬스체크 제안 | 🔵 미구현 제안 | 2026-08-16 |
| [`operations/proposals/linux-package-management.md`](./operations/proposals/linux-package-management.md) | APT/DNF 래퍼와 제한 sudo 권한 위임 제안 | 🔵 미구현 제안 | 2026-08-16 |
| [`operations/proposals/hardware-healing.md`](./operations/proposals/hardware-healing.md) | GPU 스로틀/스톨 감지와 클라우드/베어메탈 자가치유 제안 | 🔵 미구현 제안 | 2026-08-16 |

## 도메인 5. 📋 Roadmap & Planning

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`roadmap/roadmap.md`](./roadmap/roadmap.md) | P0~P3 백로그/상태 마스터 트래커 — 가장 권위 있는 "지금 실제로 무엇이 참인가" 문서 | 🟢 정본 | 2026-08-09 |
| [`roadmap/conflict-analysis.md`](./roadmap/conflict-analysis.md) | 2026-08-06 시점의 Caddy→Nginx 및 다중 운영 전제를 기록한 우선순위 분석 | ⚪ 역사적 분석 — 현재 roadmap/availability 정본 참조 | 2026-08-16 |

## 도메인 6. 🔒 Security

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`security/findings.md`](./security/findings.md) | S1~S6 보안 결함(로그인 락아웃 증폭, JWT/JWKS 검증, Real IP 추출 등) 해결 보고서 | 🟢 정본 | 2026-08-06 |
| [`security/control-plane-security-model.md`](./security/control-plane-security-model.md) | HTTP/MCP 공통 principal·capability, Worker 신원, bootstrap token 및 secret 경계 정본 | 🟢 정본 — 설계 확정·구현 대기 | 2026-08-16 |

## 도메인 7. 🎨 UI / Dashboard

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`ui-dashboard/ui-design.md`](./ui-dashboard/ui-design.md) | 18개 대시보드 라우트 IA·플로우·컴포넌트 카탈로그(원안 "8개 페이지" 오기재 정정됨). SSH Config 자동 임포트 UI 흐름 추가 | 🟢 정본 — 2026-08-13 전체 절 코드 대조 완료(호스트 인벤토리 재분류, StatusPill/HostStatus enum, MCP 도구 12개 반영) | 2026-08-13 |
| [`../DESIGN-apple.md`](../DESIGN-apple.md) | 실제 적용된 Apple Design System 토큰(Action Blue, SF Pro, parchment, pill CTA) | 🟢 정본 — 루트 문서 | 2026-07-27 |
| [`../DESIGN-notion.md`](../DESIGN-notion.md) | 이전 Notion 테마 원안 분석 (폐기, 현재는 Apple 정본으로 대체) | ⚫ 폐기(미채택) — 루트 문서 | 2026-07-20 |

## 도메인 8. 🧩 Engineering Patterns

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`engineering-patterns/reuse-patterns.md`](./engineering-patterns/reuse-patterns.md) | `grok-build`/`xai-*` 코드베이스에서 채굴한 재사용 패턴 10종(RAII PendingGuard, 워커별 CircuitBreaker 등) — 10종 중 3종(#1,#3,#6)이 `fleet-scheduler` 코드 주석에 명시적으로 채택 근거로 인용됨을 확인 | 🔵 사본 — 실제 채택 근거는 `crates/fleet-scheduler/src/dispatcher.rs`/`breaker.rs` 코드 주석 | 2026-08-12 |

## 도메인 9. 🧭 Governance & Contributor Guidance — [`governance/README.md`](./governance/README.md)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`governance/documentation-policy.md`](./governance/documentation-policy.md) | 정본/보존 문서 지위, 코드 실측, Mermaid/SVG, Ingest/Query/Lint 규약 | 🟢 정본 | 2026-08-16 |
| [`governance/documentation-rewrite-guide.md`](./governance/documentation-rewrite-guide.md) | 도메인 진입점, 기능상 책임, 폐기 삭제, review 부기, Git 기록 규약 | 🟢 정본 | 2026-08-17 |
| [`governance/skills.md`](./governance/skills.md) | Fleet Skill 시스템 사용 및 작성 가이드 | 🟢 정본 | 2026-08-16 |
| [`reviews/README.md`](./reviews/README.md) | 비교·감사·대안·논의 부기 문서의 별도 진입점 | 🔵 사본 — 정본 근거 보관 | 2026-08-17 |

## 도메인 10. 📖 Agent & Assistant Entry Points

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`../agent.md`](../agent.md) | Git 정책·로드맵·품질/CI 게이트·LLM-Wiki 규약·다이어그램 및 SVG 리소스 관리 규약(§6) — 에이전트 협업 가이드 전문 | 🟢 정본 | 2026-08-12 |
| [`../CLAUDE.md`](../CLAUDE.md) | Claude Code 진입점. `@agent.md`를 임포트하고 문서 작성 지침(§6)만 요약 재기술 | 🔵 사본 — `agent.md`의 진입점/요약. 내용이 어긋나면 `agent.md`가 우선 | 2026-08-12 |
| [`../GEMINI.md`](../GEMINI.md) | Gemini / Google Antigravity (agy) 진입점. `@agent.md`를 임포트하고 Mermaid/SVG 문서 작성 및 에셋 지침 요약 | 🔵 사본 — `agent.md`의 진입점/요약. 내용이 어긋나면 `agent.md`가 우선 | 2026-08-16 |

## 하위 디렉토리 (자체 부기 체계 보유)

| 디렉토리 | 한 줄 요약 |
|---|---|
| [`llm-wiki/`](./llm-wiki/README.md) | LLM 게이트웨이(liteLLM) 채택 결정 및 인프라 스펙 위키 — 자체 `index.md`/`log.md` 보유 |
| [`credentials/`](./credentials/README.md) | 시크릿·크리덴셜 관리 지침 및 레지스트리 — 자체 `registry.md`(스냅샷+변경이력) 보유 |
| [`operations/`](./operations/README.md) | 배포·Worker enrollment의 운영 경계와 미구현 운영 자동화 제안의 탐색 지도 |
| [`contracts/`](./contracts/README.md) | HTTP, MCP, Dashboard, Worker enrollment 외부 계약의 정본 탐색 지도 |
| [`governance/`](./governance/README.md) | 문서 정책 및 Skill/기여자 협업 지침 |

## 고아 페이지 / 미해결 교차참조

- [`deployment/historical/2026-07-20-arm2-arm1-deploy-log.md`](./deployment/historical/2026-07-20-arm2-arm1-deploy-log.md) — 역사적 기록으로 유효, 현재
  지침 문서와는 명확히 분리되어 있어 조치 불필요.

_(2026-08-12: `engineering-patterns/reuse-patterns.md`의 고아 판정을 철회함 — 코드
주석에서 실제 채택 근거를 확인, 위 도메인 8 표 참조.)_

_(마지막 점검: 2026-08-11, [`log.md`](./log.md) 참고.)_
