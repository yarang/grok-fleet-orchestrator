---
type: wiki
status: canonical
source: "docs/index.md"
last_verified: "2026-08-17"
---

# Docs — 인덱스 (Index)

> `docs/`의 모든 설계·운영 문서를 도메인별 하위 디렉토리로 재배치한 뒤의 콘텐츠 지향
> 목록이다. 새 문서를 추가하거나 기존 문서의 상태(정본/사본/폐기)가 바뀌면 이 표를 함께
> 갱신하고 [`log.md`](./log.md)에 항목을 남긴다. `docs/credentials/`는 자체 `README.md`와
> `registry.md`로 secret 메타데이터를 관리한다.
>
> **운영 규칙**은 [문서 관리 정책](./governance/documentation-policy.md)과
> [문서 재작성 가이드](./governance/documentation-rewrite-guide.md)를 따른다. 각 기능 도메인의
> 세부 문서 관계는 해당 `README.md` 또는 `index.md`가 소유한다.

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
| [`deployment/install.md`](./deployment/install.md) | release artifact 검증·바이너리 설치·서비스 등록 준비 Runbook | 🟢 정본·부분 구현 | 2026-08-17 |
| [`deployment/configuration.md`](./deployment/configuration.md) | env/TOML 구성 경계·secret 권한·production preflight | 🟢 정본·부분 구현 | 2026-08-17 |
| [`deployment/worker-provisioning.md`](./deployment/worker-provisioning.md) | SSH로 Worker binary·설정·service를 배포하고 실패를 검증하는 Runbook | 🟢 정본·부분 구현 | 2026-08-17 |
| [`deployment/operations.md`](./deployment/operations.md) | 시작·상태 확인·안전 중단·수동 Primary 승격 Runbook | 🟢 정본·부분 구현 | 2026-08-17 |
| [`deployment/backup-recovery.md`](./deployment/backup-recovery.md) | PostgreSQL 백업·새 DB 복원·in-place 복구 게이트 | 🟢 정본·부분 구현 | 2026-08-17 |
| [`deployment/troubleshooting.md`](./deployment/troubleshooting.md) | 배포 증상별 증거 수집과 복구 경로 Runbook | 🟢 정본·부분 구현 | 2026-08-17 |
| [`deployment/reverse-proxy.md`](./deployment/reverse-proxy.md) | Nginx TLS·trusted proxy·공개 endpoint 경계 | 🟡 정본·부분 구현 | 2026-08-17 |
| [`deployment/topology.md`](./deployment/topology.md) | Single Active Primary·Cold Standby 정본의 배포 관점 요약 | 🔵 사본·부분 구현 | 2026-08-17 |
| [`deployment/litellm-gateway.md`](./deployment/litellm-gateway.md) | liteLLM gateway 준비·기동·검증·rollback Runbook | 🟢 정본·부분 구현 | 2026-08-25 |

## 도메인 3. 🔑 Worker Bootstrap & Join Auth — [`worker-bootstrap/README.md`](./worker-bootstrap/README.md)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`worker-bootstrap/README.md`](./worker-bootstrap/README.md) | 현재 수동 가입 절차와 계약·보안·SSH 프로비저닝 정본 탐색 진입점 | 🟢 정본 | 2026-08-17 |

## 도메인 4. 🛠️ Operations — [`operations/README.md`](./operations/README.md)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`operations/README.md`](./operations/README.md) | 배포·가입·장애 전환 정본과 운영 문서 승격 규칙의 진입점 | 🟢 정본 | 2026-08-17 |

## 도메인 5. 📋 Roadmap & Planning — [`roadmap/README.md`](./roadmap/README.md)

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`roadmap/README.md`](./roadmap/README.md) | 구현 순서·상태·완료 게이트를 관리하는 Roadmap 도메인 진입점 | 🟢 정본 | 2026-08-17 |

## 도메인 6. 🔒 Security

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [Security](./security/README.md) | 신원·권한·Worker credential·secret 경계의 도메인 진입점 | 🟢 정본 | 2026-08-17 |

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
| [`governance/documentation-policy.md`](./governance/documentation-policy.md) | 문서 도메인·정본 관계·메타데이터·링크·부기 원칙 | 🟢 정본 | 2026-08-17 |
| [`governance/documentation-rewrite-guide.md`](./governance/documentation-rewrite-guide.md) | 대규모 문서 재작성·이동·폐기 절차와 완료 게이트 | 🟢 정본 | 2026-08-17 |
| [`governance/design-document-reading-and-judgment.md`](./governance/design-document-reading-and-judgment.md) | 설계 문서의 권위·현재 구현·실패 경로를 구분해 판단하는 공통 방법 | 🟢 정본 | 2026-08-18 |
| [`governance/skills.md`](./governance/skills.md) | Fleet Skill 시스템 사용 및 작성 가이드 | 🟢 정본 | 2026-08-16 |
| [`reviews/README.md`](./reviews/README.md) | 비교·감사·대안·논의 부기 문서의 별도 진입점 | 🔵 사본 — 정본 근거 보관 | 2026-08-17 |

## 도메인 10. 📖 Agent & Assistant Entry Points

| 문서 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`../AGENTS.md`](../AGENTS.md) | Codex가 공통 에이전트·문서 정책을 적용하도록 하는 최소 실행 진입점 | 🔵 사본 — `agent.md`와 Governance 정본 참조 | 2026-08-17 |
| [`../agent.md`](../agent.md) | Git·로드맵·보안·품질/CI의 공통 에이전트 규칙과 문서 정책 진입점 | 🟢 정본 | 2026-08-17 |
| [`../CLAUDE.md`](../CLAUDE.md) | Claude Code에서 공통 에이전트·Governance 정본으로 연결하는 최소 진입점 | 🔵 사본 | 2026-08-17 |
| [`../GEMINI.md`](../GEMINI.md) | Gemini에서 공통 에이전트·Governance 정본으로 연결하는 최소 진입점 | 🔵 사본 | 2026-08-17 |

## 하위 디렉토리 (자체 부기 체계 보유)

| 디렉토리 | 한 줄 요약 |
|---|---|
| [`credentials/`](./credentials/README.md) | 시크릿·크리덴셜 관리 지침 및 레지스트리 — 자체 `registry.md`(스냅샷+변경이력) 보유 |
| [`operations/`](./operations/README.md) | 배포·Worker enrollment·장애 전환 정본과 운영 문서 승격 규칙의 탐색 지도 |
| [`contracts/`](./contracts/README.md) | HTTP·MCP·Dashboard·Worker enrollment 현재 계약과 Project·Agent 제안 계약의 탐색 지도 |
| [`governance/`](./governance/README.md) | 문서 정책 및 Skill/기여자 협업 지침 |

## 고아 페이지 / 미해결 교차참조

_(2026-08-12: `engineering-patterns/reuse-patterns.md`의 고아 판정을 철회함 — 코드
주석에서 실제 채택 근거를 확인, 위 도메인 8 표 참조.)_

_(마지막 점검: 2026-08-11, [`log.md`](./log.md) 참고.)_
