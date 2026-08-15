---
type: wiki
status: canonical
source: "docs/log.md"
last_verified: "2026-08-15"
---

# Docs — 변경 로그 (Log)

> **Append-only.** 새 항목은 파일 맨 아래에 추가한다(시간순). 과거 항목은 수정하지 않는다(오탈자
> 수정 제외). 각 항목은 `ingest`(신규 결정/소스 반영) / `query`(질문에 대한 답을 새 페이지로 파일링)
> / `lint`(모순·오래된 정보·고아 페이지 점검 및 수정) 중 하나로 분류한다. 스키마는
> [`docs/llm-wiki/README.md`](./llm-wiki/README.md) 참고. `docs/llm-wiki/`와
> `docs/credentials/`는 각자의 `log.md`/`registry.md` 변경이력을 별도로 갖는다.
>
> 이하 2026-08-11 이전 항목은 이 로그 도입 시점에 git 히스토리와 문서 내용을 근거로
> 소급 작성한 것이다.

---

## 2026-07-18 — ingest — 초기 설계 문서군 최초 작성 (역사적 소급 기록)

- `reuse-patterns.md`(2026-07-18), `deployment-log-arm2-arm1.md`(2026-07-20),
  `ui-design.md`(2026-07-20, Notion 테마 원안), 루트 `DESIGN-notion.md`(2026-07-20)가
  이 시기 작성됨. 당시 리버스 프록시는 Caddy, 대시보드 디자인 시스템은 Notion 테마 안이
  채택 상태였음.

## 2026-07-27~28 — ingest — Nginx 마이그레이션 및 Apple Design System 채택

- 루트 `DESIGN-apple.md` 신설(2026-07-27), 커밋 `837c41c`(2026-07-28)로 Caddy → nginx +
  certbot 교체 및 대시보드 전체 Apple Design System(parchment 캔버스, Action Blue, SF Pro)
  적용이 CHANGELOG에 기록됨. `docs/ui-design.md`(Notion 이중 테마 안)와 당시 아직 작성되지
  않은 배포 문서군이 이 결정을 소급 반영하지 못한 채 남아 향후 lint 대상이 됨(아래
  2026-08-11 lint 항목 참고).

## 2026-08-06 — ingest — Nginx 전환 결정 및 배포/토폴로지 문서군 확정

- `nginx_transition_proposal.md`(비교표+nginx.conf+전환 절차), `proposed_server_architecture.md`
  (Nginx 게이트웨이 반영 최종 토폴로지), `deployment.md` §2.3(Nginx를 "권장 게이트웨이
  표준"으로 명문화)를 같은 날 작성/갱신하여 Caddy→Nginx 결정을 공식화.
- 같은 날 `roadmap_conflict_analysis.md`가 이 결정 및 S1~S6 보안 수정 완료를 전제로 백로그
  우선순위(#25 P3→P2, #15 검증 항목 추가)를 재조정.
- 같은 날 워커 부트스트랩/조인 인증 문서군(`worker_join_authentication_design.md`,
  `bootstrap_token_delivery_methods.md`, `ssh_provisioning_implementation_spec.md`,
  `fleet_serve_dashboard_and_worker_bootstrap_design.md`, `bootstrap-and-worker-features.md`) 및
  서버 관리 자가치유 제안서군(`advanced_server_management_proposals.md`,
  `linux_package_management_design.md`, `cloud_and_baremetal_hardware_healing_design.md`),
  `security-findings.md`(S1~S6 해결 보고)가 함께 작성됨.

## 2026-08-07 — ingest — single_server_deployment_plan.md liteLLM 정합화 (부분 수정)

- 로드맵 #34 작업의 일환으로 `single_server_deployment_plan.md`의 liteLLM/One API 모순을
  `llm-wiki/` 정본 기준으로 수정 완료(`llm-wiki/log.md` 2026-08-06 lint 항목 참고). 단, 같은
  문서 내 Caddy 리버스 프록시 섹션(§2.1, §3 Step 2)은 이 수정에서 누락되어 8/6일자 Nginx
  결정과 여전히 모순된 상태로 남음 — 아래 2026-08-11 lint 항목에서 발견 및 수정.

## 2026-08-09 — ingest — roadmap.md #34 liteLLM 게이트웨이 구현 완료 반영

- `crates/fleet-core/src/config.rs` URL 검증, `docker-compose.yml` litellm 컨테이너,
  `examples/litellm-config.yaml`을 반영해 roadmap.md #34를 "해결됨"으로 갱신.

## 2026-08-11 — lint — docs/ 루트 정본/사본 부기 체계 도입, 모순 2건 발견 및 수정

- **발견 1 (Caddy/Nginx)**: `single_server_deployment_plan.md`(2026-08-07 최종 수정)가 8/6에
  확정된 Nginx 결정 이후에도 Caddy 리버스 프록시 예시(Caddyfile)를 그대로 보유하고 있음을
  발견. `deployment.md` §2.3, `nginx_transition_proposal.md`, 루트 `CHANGELOG.md`(커밋
  `837c41c`, "Caddy를 nginx + certbot으로 교체")와 직접 모순. liteLLM/One API 케이스와 동일한
  실패 패턴(정본 갱신 후 사본 미동기화)이 같은 문서의 다른 섹션에서 재발한 것.
  **조치**: §1 개념도, §2.1, §3 Step 2를 Nginx 기준으로 재작성(사본임을 명시하고
  `deployment.md` §2.3을 정본으로 링크, 동기화 순서 주석 추가).
- **발견 2 (디자인 시스템)**: `docs/ui-design.md` §2.1 "이중 테마 전략"(Dark + Notion)이 루트
  `CHANGELOG.md`(커밋 `837c41c`, 2026-07-28)에 기록된 "대시보드 전체 Apple Design System
  적용" 및 루트 `DESIGN-apple.md`(정본 디자인 토큰)와 모순. 실제 대시보드 CSS
  (`crates/fleet-dashboard/assets/*.css`)에서 Notion 흔적 없음을 확인. **조치**: §2 상단에
  배너를 추가해 이 절이 미채택 원안임을 명시하고 `DESIGN-apple.md`를 정본으로 안내. 토큰
  테이블 자체는 역사적 기록으로 보존(§3 이후 페이지 설계가 옛 토큰명을 인용할 가능성이
  있어 전체 재작성은 보류 — 후속 lint 대상으로 `index.md` 고아 페이지 절에 남기지 않고
  본 항목에 기록).
- **발견 3 (고아 문서)**: `reuse-patterns.md`, `deployment-log-arm2-arm1.md`,
  `bootstrap-and-worker-features.md`가 어느 문서에서도 인용되지 않고 있음을 확인(구
  `WIKI.md` 포함). `bootstrap-and-worker-features.md`는 `architecture.md`의 Worker
  Daemon(Phase 8.1) 절 및 부트스트랩 문서군과 내용이 중복되는 사본으로 판정. **조치**: 병합/
  폐기는 후속 작업으로 `index.md` "고아 페이지" 절에 등재만 하고 미조치.
- **발견 4 (WIKI.md 불완전)**: 기존 `docs/WIKI.md`가 `architecture.md`/`api-reference.md`/
  `ui-design.md`/`security-findings.md` 등 8개 문서를 누락하고 있으며 정본/사본 상태나
  최종개정일 표기가 없었음. **조치**: `docs/index.md`(카탈로그, `llm-wiki/index.md`와 동일
  스키마) + 본 `log.md` 신설로 대체. `WIKI.md`의 5개 카테고리 프로즈는 `index.md`의 주제
  그룹 헤더로 이관, `WIKI.md` 자체는 리다이렉트 스텁으로 축소.
- 근거 조사는 `docs-inventory` 서브에이전트가 수행(전체 21개 문서 정독 + git log 대조), 본
  항목은 그 조사 결과를 실제 반영한 기록.

## 2026-08-11 — lint — docs/ 도메인별 하위 디렉토리 전면 재배치

- 위 항목의 평평한(flat) 구조 정합화에 이어, 사용자 확인 후 20개 문서를 8개 도메인
  하위 디렉토리(`architecture/`, `deployment/`, `worker-bootstrap/`, `server-management/`,
  `roadmap/`, `security/`, `ui-dashboard/`, `engineering-patterns/`)로 `git mv` 재배치.
  `docs/WIKI.md`는 인바운드 링크 0건을 재확인 후 완전 삭제(스텁 단계 생략).
- 이동 전 전체 저장소(`grep -rl`)를 대상으로 각 파일명의 인바운드 참조를 전수 조사 —
  이동 대상 20개 문서 상호간 교차링크, 루트 `README.md`/`agent.md`, `docs/credentials/registry.md`,
  `docs/llm-wiki/{README,index,litellm_integration_plan,multi_provider_llm_proxy_analysis}.md`의
  실제 마크다운 링크(`](...)′)를 모두 새 경로로 갱신. `CHANGELOG.md`와 `docs/log.md`/
  `docs/llm-wiki/log.md`의 과거 항목은 append-only 원칙에 따라 **의도적으로 미수정**
  (당시 실제 경로를 정확히 기록한 역사적 사실이므로).
- 부수적으로 발견한 결함 2건도 함께 수정: `docs/roadmap/conflict-analysis.md`가 로컬 머신
  절대경로(`file:///Users/yarang/...`)로 `roadmap.md`를 링크하고 있던 것을 상대경로로 교정;
  `docs/architecture/overview.md`의 `deployment.md` 링크가 이동 후 다른 디렉토리를 가리키게
  되어 `../deployment/deployment.md`로 교정.
- 4개 도메인(`architecture/`, `deployment/`, `worker-bootstrap/`, `server-management/`)에
  `README.md` 신설 — 해당 도메인 문서 간 관계, 정본/사본 지위, (worker-bootstrap·
  server-management는) mermaid 다이어그램 포함. `roadmap/`, `security/`, `ui-dashboard/`,
  `engineering-patterns/`는 문서 1~2건뿐이라 `README.md` 생략, `../index.md`에서 직접 안내.
- `docs/index.md`를 새 경로 기준으로 전면 재작성, 루트 `README.md`의 문서 링크도 동기화.
- 제안(디렉토리 구조, 초안 index.md/log.md, 패치, 다이어그램)은 `docs-inventory`
  서브에이전트가 작성했으나 해당 에이전트는 read-only라 파일을 직접 옮기거나 고칠 수 없어
  실제 적용은 본 세션이 수행. 초안의 개별 파일명(kebab-case로 재명명, 예:
  `worker_join_authentication_design.md` → `join-authentication.md`)과 디렉토리 배치는
  그대로 채택했으나, `ui-design.md` §2 색상/타이포 토큰 전면 재작성(초안 §C.2 후반부 제안)은
  범위가 커 이번 패스에서 보류 — 배너 정정만 유지.

## 2026-08-11 — lint — UI docs를 Apple Design System 기준으로 재정렬

- `docs/ui-dashboard/ui-design.md`의 레이아웃/컴포넌트/페이지 설명을 루트
  `DESIGN-apple.md` 정본과 일치하도록 재작성했다. 기존의 Notion/이중 테마 표현을 제거하고,
  Action Blue, parchment, black global nav, pill CTA, SF Pro 토큰과 tile/utility-card
  언어로 통일했다.
- `docs/index.md`의 UI/대시보드 참조 문서 설명을 현재 정본 기준으로 보완해,
  `DESIGN-apple.md`가 우선 정본임을 명시했다.

## 2026-08-13 — ingest — 문서화 지속 관리를 위한 지침서(documentation-policy.md) 신설

- `docs/engineering-patterns/documentation-policy.md`를 신설하여 현재 문서화 시스템의 아키텍처 상태를 분석하고, 이를 지속 관리하기 위한 핵심 지침(정본-사본 원칙, 코드 실측 검증 원칙, 다이어그램/SVG 리소스 관리 규약, Ingest-Query-Lint 3단계 워크플로우 및 YAML 프론트매터 표준 양식)을 정립했습니다.
- `docs/index.md`에 해당 문서를 도메인 8 (Engineering Patterns) 아래의 🟢 정본으로 등재했습니다.

## 2026-08-13 — ingest — MCP 도구 4종 추가(로드맵 #28) 반영 문서 갱신

- `crates/fleet-mcp`에 `fleet_list_hosts`/`fleet_reset_worker_breaker`/
  `fleet_list_bootstrap_tokens`/`fleet_revoke_bootstrap_token` 4종을 추가해
  MCP 도구가 8개 → 12개로 늘어난 것을 반영해 `architecture/api-reference.md`
  (§MCP 도구, 4종 전체 입출력 스펙 추가), `architecture/mcp-specification.md`
  (§4, 9~12번 항목 추가), `architecture/README.md`, `ui-dashboard/ui-design.md`
  (§3.8 목업 정정 배너), `worker-bootstrap/{bootstrap-release-v0.2,
  serve-and-bootstrap-design}.md`, `assets/diagrams/architecture/
  system-architecture-flow.mermaid`, `assets/diagrams/worker-bootstrap/
  fleet-serve-module-map.svg`를 일괄 갱신했다.
- 같은 패스에서 `docs/index.md`의 `ui-design.md` 행이 "8개 페이지"라는 이미
  정정된(2026-08-13 이전 세션) 오기재를 그대로 요약에 남기고 있던 것을 발견해
  "18개 라우트"로 정정하고 상태를 🟡→🟢로 올렸다.

## 2026-08-14~15 — ingest — 에이전트 및 프로젝트 격리 고도화 신규 설계 문서 5종 작성 (로드맵 #48~#52)

- 로드맵 `#48`~`#52`에 해당하는 에이전트 인프라 및 프로젝트 하드 격리 아키텍처 설계 문서 5종(`project-feature-design.md`, `agent-provisioning-design.md`, `agent-terminal-access-design.md`, `agent-harness-composition-design.md`, `agent-runtime-vendor-design.md`)을 작성했다.
- 이 과정에서 13개 관점 다중 에이전트 리뷰 및 3표 적대적 검증(총 9차 개정)을 통해, 프로젝트 배타적 소유 가드(409), StdioBridge JSON-RPC 파싱 경계 보완, tmux remain-on-exit 기반 폴링 수명주기 등 다수의 보안/아키텍처 갭을 사전에 도출하여 반영했다.
- 세부 설계 이력은 `docs/architecture/log.md`에 정본으로 append-only 기록했다.

## 2026-08-15 — lint — 문서 정합성 점검 및 정책 준수 규격 교정

- **인덱싱 누락 복구**: 신규 설계 문서 5종 및 아키텍처 로그(`architecture/log.md`)를 루트 `docs/index.md` 카탈로그와 `docs/architecture/README.md` 도메인 색인에 🟢 정본으로 일괄 등록했다.
- **다이어그램 규약 교정**: `docs/deployment/single-server.md`에서 정책을 위반해 사용 중이던 텍스트 기반 ASCII-art 다이어그램을 표준 `mermaid` 순서도 블록으로 대체했다.
- **깨진 참조 및 통계 갱신**: `docs/llm-wiki/index.md` 내 `ci_monitor_report.md` 링크의 프로젝트 외부 임시 brain 경로(`.gemini/...`)를 리포지토리 내부로 이관 복사하고 상대경로로 정정했다. `docs/assets/diagrams/README.md` 내 다이어그램 리소스 개수 통계(architecture 12➔26, ui-dashboard 7➔8)를 실제와 일치시켰다.
- **로드맵 메타데이터 정정**: `docs/roadmap/roadmap.md` 본문 헤더의 날짜 표기("2026-08-11 기준")를 "2026-08-15 기준"으로 수정했다.

## 2026-08-15 — ingest — 시스템 엔티티 관계 및 매핑 규정 명세서(system-entities-mapping.md) 신설

- 다중 에이전트 분석 토론(system_analyst, agent_analyst, task_analyst)의 설계 분석을 수렴하여, Project, Host, Worker, Agent, Custom Prompt, Skill, Tool, Task 간의 통합적 관계 모델링을 정리한 `docs/architecture/system-entities-mapping.md` 정본을 신설했습니다.
- 물리적 배치(WHERE 축), 행동 명세(WHAT 축), 스코프 체인(WHEN 축)의 3축 구조를 정의하고, 배타적 격리 불변식, 프롬프트 합성 파이프라인, 그리고 스케줄러(`WorkerSelector`) 필터 파이프라인 단계를 명문화했습니다.
- 신설 문서를 `docs/index.md` 및 `docs/architecture/README.md`에 🟢 정본으로 색인 등록했습니다.

## 2026-08-15 — ingest — 설계 관계 비판적 대안 보고서(system-entities-critique.md) 신설

- 비판적 설계 분석 전문가 에이전트(`critical_auditor`)를 소환하여 `system-entities-mapping.md`에 설정된 구조를 독립적으로 감사하고, `docs/architecture/system-entities-critique.md` 정본 보고서를 추가했습니다.
- 주요 감사 포인트인 (1) 물리/행동 레이어 혼선 및 비직교성, (2) 동시 에이전트 생성 행 잠금 및 Dispatcher TOCTOU 경쟁 상태, (3) 재배정 시 이종 테넌트 작업 물리적 오염 가능성, (4) 프롬프트 캐시(KV-Cache) 무효화 및 스킬 텍스트 인플레이션을 도출했습니다.
- 이에 대한 2차원 직교 재편, 원자적 갱신 예약 쿼리, Draining/Workspace Purge 규격화, 캐시 분리형 프롬프트 조립 등 구체적이고 실질적인 토큰/성능 최적화 대안을 제시했습니다.
- 신설 문서를 `docs/index.md` 및 `docs/architecture/README.md`에 🟢 정본으로 색인 등록했습니다.

## 2026-08-15 — ingest — 다중 도메인 설계 교차 조정 및 의사결정 보고서(multi-agent-realignment-report.md) 신설

- 코어, 인프라, 운영 도메인을 전담하는 3개 분야 설계 에이전트(`core_architect`, `infra_architect`, `operations_architect`)를 기동하여 `system-entities-critique.md`에 근거해 `docs/` 하위의 모든 설계 사양들을 교차 정밀 감사하도록 지시했습니다.
- 에이전트 간 가상 기술 합의 토론을 촉진하여 (1) unprivileged `fleet` 권한 하에서의 안전한 워크스페이스 소거 방안, (2) Nginx 리버스 프록시 WebSocket `/ws` 업스트림 600초 단일 규격 정비, (3) 워커 가입인증 API Whitelisting 및 트랜잭션 내 원자적 토큰 검증, (4) Ephemeral NOTIFY를 보완하는 데이터베이스 시퀀스 이벤트 저널링 등의 도메인 간 모순과 경쟁 문제 해결책을 수립했습니다.
- 사용자의 최종 설계 판단을 구하기 위한 네 가지 트레이드오프 선택지(Wipe 권한, 샌드박스 격리 수준, 스킬 온디맨드 로딩, 분산 동기화 토폴로지)로 구성된 아키텍처 체크리스트를 포함한 종합 보고서 `docs/architecture/multi-agent-realignment-report.md`를 신설 및 색인 등록했습니다.
- 추가로 프로젝트 로컬 설정(git config 및 openapi.yaml)에서 실제 사용 중인 Gitea 환경 정보(Web: `https://git.agentthread.dev`, SSH: `git-ssh.agentthread.dev`)를 확인하여, 이를 데이터 평면의 정본으로 삼는 분산 데이터 흐름을 명세서에 정식 병합했습니다.

## 2026-08-15 — ingest — 핵심 기능 기술 타당성 및 로컬 검증 규격서(feature-feasibility-testing.md) 신설

- 기술 타당성 평가 에이전트(`tech_evaluator`)를 기동하여 Gitea 기반 이관, 드레인, 동적 스킬, 멀티 에이전트 연동의 구현 가능성 및 로컬 통합 검증 방안을 구체화한 `docs/architecture/feature-feasibility-testing.md` 정본을 신설했습니다.
- 비동기 POSIX `git` CLI 제어, Gitea REST API 명세, SSH Deploy Key 인메모리 격리, sysinfo/NVML을 통한 동적 로드 감시 등의 필요 소요 기술을 규정했습니다.
- 실제 분산 환경이나 네트워크 Git 없이 로컬에서 wiremock(HTTP 모킹 API), Local Bare Repository(로컬 파일 시스템 간 push/pull), metrics injection 및 stress 도구(부하 강제 유발), Mock Worker Actor(DAG E2E 테스트)를 활용한 100% 자가 검증 통합 테스트 시나리오를 설계했습니다.
- 신설 문서를 `docs/index.md` 및 `docs/architecture/README.md`에 🟢 정본으로 색인 등록했습니다.

## 2026-08-16 — ingest — fleet tasks submit --skill CLI 구현 및 스킬 예시 파일 생성

- **`fleet tasks submit <prompt>` 명령 추가** (`crates/fleet-cli/src/main.rs`, `crates/fleet-cli/src/runtime.rs`)
  - `--skill <SKILL_NAME>` (반복 허용): `fleet_scheduler::skill_loader::inject_skills()` 호출로 지정 스킬을 프롬프트에 XML 블록으로 인젝션한 뒤 DB에 Pending 태스크로 생성.
  - 추가 플래그: `--model`, `--server-hint`, `--priority`, `--max-turns`, `--timeout-secs`, `--label`, `--cwd`, `--created-by`, `--json`.
  - `cargo check -p fleet-cli` 경고 0 통과. `fleet tasks submit --help` 출력 검증 완료.
  - 커밋: `0a51b12` (feat(cli): fleet tasks submit --skill 명령 추가)

- **기본 제공 스킬 파일 5종 생성** (`~/.config/grok-fleet/skills/`, git 미추적):
  - `rust-expert.md`, `security-audit.md`, `code-reviewer.md`, `doc-writer.md`, `data-analyst.md`

- **`docs/skills.md` 신설** (5.7 KB): 스킬 시스템 전체 사용 가이드 — CLI 사용법, 스킬 파일 위치/형식, 체이닝 패턴, 커스텀 스킬 작성 방법.
  - `docs/index.md` 도메인 9 표에 🟢 정본으로 색인 등록.
  - 커밋: `804dc5b` (docs: 스킬 시스템 사용 가이드 추가)
