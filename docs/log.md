---
type: wiki
status: canonical
source: "docs/log.md"
last_verified: "2026-08-30"
---

# Docs — 변경 로그 (Log)

> **Append-only.** 새 항목은 파일 맨 아래에 추가한다(시간순). 과거 항목은 수정하지 않는다(오탈자
> 수정 제외). 각 항목은 `ingest`(신규 결정/소스 반영) / `query`(질문에 대한 답을 새 페이지로 파일링)
> / `lint`(모순·오래된 정보·고아 페이지 점검 및 수정) 중 하나로 분류한다. 스키마는
> [문서 관리 정책](./governance/documentation-policy.md)을 따른다. `docs/credentials/`는 secret
> 메타데이터 스냅샷과 변경 이력을 `registry.md`에서 별도로 관리한다.
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

- **`docs/architecture/host-integrity-and-security-monitoring-design.md` 신설**:
  - 워커 노드의 비인가 패키지 및 임의 생성 파일 감시를 위한 3계층 하이브리드 파이프라인(커널 inotify ➔ 5분 윈도우 배칭 ➔ L1/L2 필터링 ➔ L3 온디맨드 LLM 위험성 분석) 설계.
  - cgroups v2/systemd 리소스 격리(`Nice=19`, `IOSchedulingClass=idle`, `CPUQuota=5%`, `MemoryMax=64M`)를 통한 무결성 데몬의 오버헤드 최소화 기법 수립.
  - `TaskResult` 사이드이펙트 매니페스트 필드 추가 및 점진적 로드맵 제안.
  - `docs/architecture/README.md` 및 `docs/index.md`에 🟢 정본으로 색인 등록.

- **`fleet-worker` grok & agy(Antigravity CLI) 프록시 연동 지원**:
  - `WorkerConfig`에 `[llm_proxy]` (`gateway_url`, `api_key`) 섹션 추가.
  - 에이전트 CLI 프로세스 스폰 시 `OPENAI_BASE_URL`, `GEMINI_BASE_URL`, `ANTIGRAVITY_BASE_URL`, `ANTHROPIC_BASE_URL`, `FLEET_LLM_GATEWAY_URL` 환경변수 자동 주입.
  - `grok_process.rs` 단위 테스트 `apply_llm_proxy_envs_sets_grok_and_agy_envs` 추가 및 52개 테스트 전체 통과 검증 완료.
  - `docs/llm-wiki/litellm_integration_plan.md` 갱신.

- **`docs/architecture/intelligent-task-routing-and-budget-control-design.md` 신설**:
  - FreeRouter 분류/정책의 Rust 흡수 2단계 라우팅(`TaskRouter`), 3단계 소프트 예산 제어(80% 경보/Compact, 100% Grace Turn, 120% Hard Abort & Partial Diff), 3계층 Compact 엔진(L1 Truncate, L2 Summary, L3 State), 무비용 결정론적 텔레메트리 DB 및 MAB(UCB1) 공평성 탐색 알고리즘, 멀티 CLI(grok, agy) 하이브리드 매핑 통합 설계서.
  - `docs/architecture/README.md` 및 `docs/index.md`에 🟢 정본으로 색인 등록.

- **`markdown-visual-expert.md` 스킬 신설 및 `doc-writer.md` 보강**:
  - 마크다운 기술 문서 작성 시 Mermaid 다이어그램 및 벡터 SVG 적극 활용 지침 수립.
  - 대형/재사용 시각 에셋을 `docs/assets/diagrams/<domain>/` 디렉토리에 체계적으로 저장 및 상대경로로 참조하는 규약 반영.
  - `~/.config/grok-fleet/skills/markdown-visual-expert.md` 및 `.agents/skills/markdown-visual-expert/SKILL.md` 생성.
  - Gemini/Antigravity 에이전트 지침 진입점 [`GEMINI.md`](../GEMINI.md) 신설 및 `docs/index.md` 색인 등록.

- **`fleet-scheduler` 지능형 TaskRouter 및 HeuristicTaskClassifier 구현 (Step 3)**:
  - `crates/fleet-scheduler/src/router.rs` 신설: `TaskRouter` 트레잇 및 14차원 결정론적 휴리스틱 분류기(`HeuristicTaskRouter`) 구현 (비용 $0, 레이턴시 0ms).
  - 4단계 논리 프로파일(`Economy`, `Balanced`, `Complex`, `Reasoning`) 및 기본 모델/소프트 예산 매핑.
  - `Dispatcher::submit` 파이프라인에서 지능형 라우팅 결정 자동 주입.
  - `docs/assets/diagrams/architecture/task-router-flow.{svg,mermaid}` 다이어그램 작성 및 아키텍처 사양서 연동.
  - `cargo test --workspace` 100% 통과 검증 완료.

- **대규모 분산 호스트 클러스터 그룹핑 & 호스트 카드 UI 디자인 정식 반영**:
  - 30여 대 이상의 멀티 클라우드(OCI, GCP, Local) 분산 인프라를 위한 상단 KPI, 아코디언 클러스터 그룹핑, 실시간 호스트 카드(CPU/메모리/활성 태스크 슬롯/서킷브레이커) UI 명세 수립.
  - `docs/assets/diagrams/ui-dashboard/host-cluster-grouped-view.{svg,mermaid}` 다이어그램 작성 및 `docs/ui-dashboard/ui-design.md` §3.2.5 반영.

## 2026-08-16 — Control-plane 운영·보안·일관성 재정렬

- 유형: `ingest` + `lint`
- Single Active Primary와 Cold Standby를 공식 운영 모델로 확정했다.
- 가용성, TaskAttempt 일관성, control-plane 보안, 설정 배치 정본을 추가했다.
- Active-Active 및 완전 stateless 주장을 제거하고 배포·복구 절차를 동기화했다.
- 지능형 라우팅을 부분 구현으로 재분류하고 하드코딩 모델과 미구현 단계를 명시했다.
- Cloudflare token 원문을 credential registry에서 제거하고 Worker 설정 경로를
  `/etc/fleet/worker.toml`로 통일했다.
## 2026-08-16 — Agent 격리·Skill 전달 정책 확정

- 신뢰된 단일 프로젝트에는 `host_trusted`, 다중 프로젝트 또는 신뢰되지 않은 입력에는 `container_required`를 적용하는 혼합 실행 격리를 확정했다.
- 필수 Skill은 revision/hash를 고정해 static prefix로 inline 주입하고, 선택 Skill은 카탈로그 후 읽기 전용 동적 조회로 전환했다.
- `agent-execution-isolation.md`를 정본으로 추가하고, terminal cleanup에서 host 전체 `tmux kill-server` 사용을 금지했다.

## 2026-08-16 — Worker 최소 권한·제한 sudo 소거 확정

- Worker와 Agent를 비권한 `fleet` 계정으로 실행하고, root 권한은 root 소유 `fleet-worker-wipe`의 정확한 sudo allow-list에만 위임하도록 확정했다.
- 범용 shell·`rm`·와일드카드 sudo 권한을 금지하고, opaque workspace id 검증·descriptor 기반 정리·감사 로그를 소거 도구의 구현 게이트로 기록했다.

## 2026-08-16 — Bootstrap token API/MCP 원문 노출 1차 제거

- `BootstrapToken::public_id()`를 도입해 `GET /v1/bootstrap-tokens`와 `fleet_list_bootstrap_tokens`가 원문 대신 `token_id`를 반환하도록 변경했다.
- HTTP와 MCP의 revoke 입력 및 HTTP URL path를 `token_id`로 바꾸고, 발급 응답에서만 원문을 1회 반환하도록 계약·OpenAPI·회귀 테스트를 동기화했다.
- DB 원문을 digest로 전환하는 후속 마이그레이션은 S9 미해결 범위로 남겼다.

## 2026-08-16 — 선택적 Worker heartbeat 정책 확정

- Worker별 `liveness_mode`를 `periodic`(기본)과 `on_demand`(idle 시 무트래픽)로 분리했다.
- on-demand Worker는 heartbeat timeout 대신 dispatch 직전 ACP probe로 확인하며, `heartbeat_interval_secs = 0` 같은 모호한 비활성화 값은 지원하지 않도록 정본 설계를 추가했다.

## 2026-08-16 — Project·Task·Agent lifecycle 정합성 검토

- Project에 운영 lifecycle과 Draining/Delete 계약이 없고, `ON DELETE SET NULL`이 실행 중 Agent/Task의 정책 문맥을 제거할 수 있음을 기록했다.
- on-demand liveness가 현재 heartbeat 기반 Agent command polling을 멈출 수 있어, 별도 control stream/poll 구현 전에는 Agent host에 적용하지 않는 안전 제약을 추가했다.
- Task 현재 상태와 목표 TaskAttempt 상태의 계층 경계, project snapshot 보존을 사용자 결정 후 정본 lifecycle 문서로 확정하도록 제안했다.
- `system-entities-mapping.md`의 ASCII 관계/프롬프트 도식을 Mermaid로 교체하고, 선택 Skill inline 주입이라는 오래된 서술을 확정된 혼합 Skill 전달 계약으로 정렬했다.

## 2026-08-16 — 연속 개발 lifecycle 확정

- 개발 지속성은 Project와 Agent가 소유하고, Task는 완료 조건을 만족하면 반드시 terminal 상태가 된다는 모델을 정본으로 추가했다.
- Project `Active → Draining → Archived` 전이에서 신규 Task/Agent를 중단하고, 이미 실행 중인 Attempt는 제출 시점 snapshot으로 마무리한 뒤 Agent를 정리하도록 정의했다.

## 2026-08-16 — 아키텍처 정본·보존 문서 경계 정리

- `architecture/canonical-map.md`를 설계 진입점으로 추가해 주제별 단일 정본과 Derived/Historical 보존 문서를 구분했다.
- 구현 참조, 검토, 대안, 의사결정 기록은 삭제하지 않고 정본 지위에서 내려 보존했다.
- 아키텍처 README와 전체 인덱스를 같은 지위 체계로 동기화하고, API/MCP 계약과 로그에 명시적 metadata를 추가했다.
- 긴 Agent 정본에는 내용을 삭제하지 않고 현재 결정만 빠르게 읽을 수 있는 요약 진입점을 추가했다.

## 2026-08-16 — 문서 도메인 경계 재구성 1차

- 유형: `ingest` + `lint`
- 미구현 서버 관리 제안을 `operations/proposals/`로 이동하고 이전 `server-management/` 경로에는 레거시 포인터만 남겼다.
- 문서화 정책과 Skill 가이드를 `governance/`로 분리했으며, `contracts/`를 HTTP·MCP·Worker enrollment 계약의 탐색 진입점으로 추가했다.
- 현재 정본과 과거 전제가 섞여 있던 roadmap 충돌 분석을 Historical로 재분류했다. 문서 내용을 삭제하지 않고 정본 링크와 지위를 명시했다.

## 2026-08-16 — Project·Task·Agent 설계 책임 재분배

- 유형: `ingest` + `lint`
- `task-management-design.md`를 새 정본으로 추가해 Project 귀속, Task 제출·의존성·취소·결과·감사와 TaskAttempt의 경계를 분리했다.
- lifecycle 문서를 Project·Task·Attempt·Agent의 교차 전이, drain/archive 순서, snapshot 보존 계약으로 확장했다.
- Project는 정책·격리·자원 소유, Agent Provisioning은 Harness·Runtime·Terminal·Isolation을 조합하는 운영 계층으로 책임 경계를 명시했다.
- overview와 canonical map에 새 계층·탐색 지도를 추가했다. 이후 긴 기존 문단의 중복 규범은 각 정본 이관 검증 후 삭제한다.

## 2026-08-17 — 레거시 사본 제거와 정본 경로 확정

- 유형: `lint`
- `server-management/`의 README와 세 개의 레거시 포인터를 삭제했다. 포인터는 설계 근거나 독자적 운영 절차를 갖지 않고 `operations/proposals/`의 정본 위치만 반복했다.
- 다이어그램 색인, 문서화 정책, roadmap의 경로를 `operations/proposals/`로 정정했다. 과거 log 항목은 당시의 경로 기록으로 보존했다.

## 2026-08-17 — Worker·Agent process·맥락 보존 계약 확정

- 유형: `ingest` + `lint`
- Worker daemon은 Host에서 지속 실행하고, Agent는 Project의 durable context를 가진 논리 엔티티,
  Agent process는 TaskAttempt마다 필요 시 실행하는 ephemeral 자원으로 분리했다.
- 기본 Task 완료 경로를 process 종료·`Hibernated`로 정하고 `WarmIdle`은 TTL·slot 상한이 있는
  명시적 최적화로 제한했다.
- Project archive는 모든 Agent process cleanup ACK 뒤에만 완료하되 일반 풀 Worker는 종료하지 않고,
  Project reservation Worker만 해제·반납 또는 deprovision하도록 정했다.
- Tool/Skill catalog → Project grant → Agent binding → Task 요청 → Attempt snapshot의 deny 우선순위와
  immutable revision/hash 규칙을 추가했다.

## 2026-08-17 — 설계문서 품질·정본성 합동 감사

- 유형: `lint`
- `docs/**/*.md` 64개를 코드 정합성, 보안·실패 경로, 문서 정책·정보 구조 관점에서 병렬 감사했다.
- 구현 상태 오표기, bootstrap token 계약 모순, Operator 자동 Agent 생성 권한 우회, HTTP/MCP/Dashboard 정본 혼재를 P0/P1로 분류했다.
- frontmatter 미적용·구형 스키마, 정본 지도·색인 누락, 절대 로컬 링크, 고아 문서와 대형 책임 혼합 문서를 정량화했다.
- 합의된 해결 순서와 체크리스트를 `governance/documentation-quality-review-2026-08-17.md`에 기록했다. 기존 문서의 대규모 마이그레이션은 스키마 의미와 P0 계약 승인 뒤 수행한다.

## 2026-08-17 — `overview.md` 재작성 경계 확정

- 유형: `lint`
- `architecture/overview.md`는 삭제하지 않고, 시스템 경계·현재 구현 상태·정본 탐색표를 제공하는 100~150줄의 Derived 입문 지도로 축소하기로 했다.
- ACP/Worker/WebSocket/CircuitBreaker/WorkerSelector/mTLS/SSH 등 코드 대조 고유 내용은 신규 Derived `implementation-reference.md`로 이관한다.
- Worker join·bootstrap은 `worker-bootstrap/`으로, 미구현 Self-Healing과 제거·보류 이력은 `operations/proposals/` 또는 Historical 문서로 이관한다.
- 실제 파일 분할은 안정 링크와 정본 지도 갱신을 포함하는 후속 재작성 작업에서 수행한다.

## 2026-08-17 — 문서 재작성 운영 지침 반입

- 유형: `ingest` + `lint`
- 프로젝트 `agent.md`에 문서 정본성·재작성 규약을 추가하고, 구형 LLM-Wiki 단독 색인 지침을 중앙 `docs/index.md` 및 Governance 체계로 교체했다.
- `governance/documentation-rewrite-guide.md`에 문서 책임 분리, 문체, 검토·반입 게이트, `overview.md` 분리 청사진과 완료 체크리스트를 정본으로 추가했다.
- 임시 조사 자료는 `.tmp/`에만 두며 최종 문서와 섞지 않는 규칙을 명문화했다.

## 2026-08-17 — 문서 재작성 1차: 탐색·계약·가입 경계 분리

- 유형: `ingest` + `lint`
- `architecture/overview.md`를 시스템 경계·현재 구현 요약·정본 탐색을 제공하는 Derived 입문 지도로 재작성하고, 기존 코드 대조와 구현 이력은 `architecture/implementation-reference.md`로 보존했다.
- HTTP, MCP, Dashboard, Worker enrollment 계약을 `contracts/`의 개별 정본으로 분리했다. 기존 `architecture/api-reference.md`와 `mcp-specification.md`는 inbound link를 보존하는 Deprecated 참조로 하향했다.
- Worker enrollment의 현재 raw bootstrap token 재사용·API-token 보호 모드 충돌과 목표 scoped credential 계약을 명시적으로 분리했다. bootstrap 문서의 token-file/SFTP 절차는 Proposed 또는 Deprecated로 표시했다.
- 코드 정합성·보안·정책 감사의 토론, 조치, 다음 배치는 `governance/document-rewrite-discussion-report-2026-08-17.md`에 기록했다.

## 2026-08-17 — 문서 재작성 지침 재정립

- 유형: `lint`
- 모든 도메인은 파일 책임과 읽기 순서를 관리하는 `README.md` 또는 `index.md` 진입점을 둔다.
- 정본 문서는 기능상 계약 하나만 소유하며, 중복 내용을 정본 링크로 교체한다.
- 폐기 문서는 포인터로 보존하지 않고 inbound link와 색인을 정리한 뒤 삭제한다. 삭제 이유와 대체 경로는 이 로그에 기록한다.
- 논의·감사·대안 비교는 설계 도메인에서 분리해 `docs/reviews/`에 부기한다. 정본에는 확정된 결과만 남긴다.
- 문서 재작성 완료 후에는 검증을 거쳐 코드 변경과 분리한 `docs:` Conventional Commit을 남긴다.

## 2026-08-17 — Review 부기 분리와 폐기 문서 삭제

- 유형: `lint`
- 문서 품질 감사와 재작성 논의 보고서를 `governance/`에서 `reviews/`로 옮기고, `reviews/README.md`를 부기 문서의 진입점으로 추가했다.
- `architecture/api-reference.md`와 `mcp-specification.md`는 새 contracts 정본으로의 inbound link를 갱신한 뒤 삭제했다.
- 문서 정책과 Agent 지침에서 Deprecated 포인터 보존을 제거하고, 폐기 문서 삭제·review 분리·문서 전용 Git 기록을 표준으로 확정했다.

## 2026-08-17 — Deployment Runbook 분리

- 유형: `ingest` + `lint`
- `deployment/`을 install, configuration, operations, backup-recovery, troubleshooting, reverse-proxy, topology의 책임별 문서와 도메인 진입점으로 재작성했다.
- production preflight에 no-auth 외부 bind 차단, trusted proxy 확인, secret 권한, Worker enrollment의 현재 blocked 조건을 추가했다.
- 기존 `deployment.md`, `configuration-files.md`, `server-topology.md`, `nginx-gateway.md`, `single-server.md`, 과거 배포 로그를 대체 정본과 inbound link를 정리한 뒤 삭제했다.
- 비교·감사·삭제 근거는 `reviews/deployment-rewrite-review-2026-08-17.md`에 부기했다.

## 2026-08-17 — Agent 실행 플랫폼 기능별 분리

- 유형: `ingest` + `lint`
- `architecture/agents/README.md`를 Agent 도메인 진입점으로 만들고, 격리·프로비저닝·runtime·harness·tool·memory·terminal 책임을 별도 정본으로 분리했다.
- 외부 Agent 관리 표면은 `contracts/agent-management.md`로 분리했고, 현재 구현되지 않았음을 명시했다.
- 기존 혼합 Agent 설계문서 다섯 개는 활성 링크와 색인을 새 정본으로 바꾼 뒤 삭제했다. 비교 근거와 후속 보안 게이트는 `reviews/agent-rewrite-review-2026-08-17.md`에 기록했다.

## 2026-08-17 — 문서 재작성 검증 게이트 보강

- 유형: `lint`
- 재작성 지침에 현재 사실·목표 계약 분리, 보안상 중요한 인증·secret·token 소모 경로의 코드 대조, 인증 조합 표기를 추가했다.
- 루트 색인의 도메인 진입점 전용 원칙, 폐기·역사 문서의 정상 탐색 경로 제외, 빈 도메인 디렉터리 정리, 상대 링크·삭제 경로 검사를 완료 게이트로 명시했다.

## 2026-08-17 — Architecture 정본·참조·검토 분리

- 유형: `ingest` + `lint`
- Architecture 진입점과 정본 지도를 기능별 현재 정본만 보이도록 재작성하고, 구현 참조를 코드 구조·제약 중심의 Derived 문서로 축소했다.
- 비교·feasibility 문서 세 개는 `reviews/`로, host integrity 운영 제안은 `operations/proposals/`로 이관했다.
- 엔티티 관계와 routing 문서는 현재 구현과 목표 계약을 분리해 재작성했다.

## 2026-08-17 — Architecture 정본 지도 통합

- 유형: `lint`
- Architecture README에 질문별 단일 정본 선택표를 통합하고, 중복된 `architecture/canonical-map.md`를 inbound link 정리 뒤 삭제했다.

## 2026-08-17 — Control Plane 권한과 장애 전환 절차 분리

- 유형: `ingest` + `lint`
- 단일 제어 권한, lease, epoch, fencing 계약은 Architecture에 남기고, 수동 Primary 승격·Standby 준비 점검은 Deployment Runbook으로 분리했다.

## 2026-08-17 — Routing·예산 계약과 Architecture 로그 정리

- 유형: `ingest` + `lint`
- `task-routing-policy.md`와 `task-budget-control.md`로 Worker 선택 정책과 usage 예산 제어를 분리했다.
- `architecture/log.md`는 중앙 `docs/log.md`, `docs/reviews/`, Git history와 역할이 겹치므로 inbound link를 정리한 뒤 삭제했다.

## 2026-08-17 — Project 모델·외부 계약·검토 분리

- 유형: `ingest` + `lint`
- `architecture/project-feature-design.md`를 Project 데이터·소유·디스패치 자격·권한 차단 조건만 보유하는 정본으로 축소했다.
- 제안된 Dashboard HTTP·MCP 호출 표면은 `contracts/project-management.md`로 옮기고, 화면 책임은 UI Dashboard 정본 링크로 남겼다.
- 권한 우회 가능성 등 미결 비교 근거는 `reviews/project-model-review-2026-08-17.md`로 격리했다.

## 2026-08-17 — Task 아키텍처 도메인 진입점

- 유형: `ingest` + `lint`
- Task 관리, 실행 일관성, routing 정책, 예산 제어 정본을 `architecture/tasks/`로 묶고 `README.md`를 단일 진입점으로 추가했다.
- Project·Agent와의 교차 lifecycle 및 전 시스템 엔티티 관계 지도는 Architecture 최상위에 유지했다.

## 2026-08-17 — 외부 계약의 설계 우선성과 현재 사실 보강

- 유형: `ingest` + `lint`
- Contracts는 목표 설계를 우선 정본으로 유지하되, 구현과 다른 경우 현재 상태를 명시하도록 진입점 규칙을 정리했다.
- HTTP host 범위·인증 조합, MCP 전송·도구 표면, Dashboard route 표면, Worker enrollment의 secret·token 소비 제한과 활성화 게이트를 보강했다.

## 2026-08-17 — Security 도메인 진입점과 기록 분리

- 유형: `ingest` + `lint`
- `security/README.md`를 신원·권한·Worker credential·secret 경계의 단일 도메인 진입점으로 추가했다.
- historical 보안 발견·해결 기록은 `security/reports/`로 이관하고, Security 정본과 root index는 진입점만 가리키도록 정리했다.

## 2026-08-17 — Deployment 책임·색인 정합성 정리

- 유형: `lint`
- `deployment/topology.md`를 Control Plane 권한·장애 전환 Architecture 정본의 배포 관점 사본으로 재분류했다.
- 중앙 색인에 현재 Deployment Runbook·구성·네트워크 문서를 등록하고, 이미 삭제된 historical 배포 기록의 링크와 고아 항목을 제거했다.

## 2026-08-17 — 문서 정책과 재작성 가이드 책임 분리

- 유형: `lint`
- `documentation-policy.md`는 도메인·정본·메타데이터·부기 원칙만 소유하도록 재작성하고, 과거 감사 기록과 절대 경로를 제거했다.
- `documentation-rewrite-guide.md`는 정책의 적용 절차·완료 게이트만 소유하도록 정리하고, Canonical·Runbook의 실패·복구 경계와 조건부 Git 기록 규칙을 명확히 했다.

## 2026-08-17 — 에이전트 문서 지침을 Governance 정본으로 연결

- 유형: `ingest` + `lint`
- 루트 `AGENTS.md`를 Codex 실행 진입점으로 추가하고 `agent.md`와 Governance 정본을 필수 작업 순서에 연결했다.
- `agent.md`에서 문서 정책·다이어그램 전문과 무조건 커밋 규칙을 제거하고, 정책 링크와 에이전트 필수 행동만 남겼다.
- `CLAUDE.md`와 `GEMINI.md`를 공통 에이전트·문서 정책을 중복하지 않는 최소 진입점으로 축소했다.
- 다이어그램 자산 진입점의 삭제된 `agent.md` 절 참조를 Governance 정본 링크로 교체했다.

## 2026-08-17 — LLM Wiki를 기능 책임별로 재배치

- 유형: `ingest` + `lint`
- liteLLM 채택·요청 경계는 `architecture/llm-gateway.md`, 준비·기동·검증·rollback은 `deployment/litellm-gateway.md`의 현재 정본으로 분리했다.
- 게이트웨이 채택 비교, arm2 배포 실측, 무료 공급자 조사, 기존 위키 변경 로그와 무관한 CI 보고서는 `reviews/`의 historical 근거로 이관했다.
- LLM 다이어그램을 `assets/diagrams/architecture/`와 `assets/diagrams/deployment/`로 나누고 코드 현행 경계와 미구현 budget·fallback을 분리했다.
- 독립 `llm-wiki` README·index·log 체계를 제거하고 Architecture·Deployment·Reviews 진입점, Credential registry, Roadmap, 예제 참조와 중앙 색인을 갱신했다.

## 2026-08-17 — LLM 역사 자료를 Git 이력으로 단일화

- 유형: `lint`
- liteLLM 채택 비교의 현재 필요 근거를 `architecture/llm-gateway.md`에 흡수했다.
- 과거 채택 분석, arm2 배포 원문, 무료 공급자 조사, LLM Wiki 변경 로그와 무관한 CI 보고서는 현재 탐색 경로에서 삭제했다.
- 삭제한 원문은 Git 이력을 복원 원천으로 사용하며, Reviews 색인·Roadmap·Runbook·예제의 활성 참조를 제거했다.

## 2026-08-17 — Roadmap을 구현 상태 트래커로 축소

- 유형: `ingest` + `lint`
- `roadmap/README.md`를 추가해 구현 순서·상태·완료 게이트만 소유하는 책임과 ID lifecycle을 정의했다.
- `roadmap.md`의 설계·감사·테스트 이력 복제를 제거하고 기존 `#1`~`#52`를 보존한 축약형 레지스트리와 활성 대기열로 재작성했다.
- 번호 없이 분리돼 있던 보안·실행 신뢰성 작업과 LLM gateway 후속 보강에 `#53`~`#65`를 부여했다.
- 2026-08-06 전제의 `conflict-analysis.md`는 현재 설계와 충돌하고 Git 이력으로 복원 가능해 삭제했다.

## 2026-08-17 — Worker 가입 문서와 SSH 프로비저닝 책임 분리

- 유형: `ingest` + `lint`
- `worker-bootstrap/`은 현재 수동 가입 여정의 진입점과 최소 Runbook만 남기고, 가입 계약은 Contracts, 보안 목표는 Security를 우선하도록 축소했다.
- 실제 `fleet provision`이 join API를 호출하지 않는 코드 경계에 맞춰 `deployment/worker-provisioning.md`를 신설했다.
- 중복 목표 설계, 미구현 token-file/SFTP/shred 절차, v0.2 스냅샷과 혼합 server 문서 다섯 개를 삭제하고 Git 이력을 복원 원천으로 삼았다.
- 현행과 다른 Worker bootstrap 다이어그램 여덟 개를 삭제하고, 현재 흐름은 두 Runbook의 인라인 Mermaid로 다시 작성했다.
- Credential registry, UI 설계, 중앙 색인, 다이어그램 색인과 Roadmap #57의 잔여 참조·상태를 동기화했다.

## 2026-08-17 — Server Management 미승인 제안 정리

- 유형: `lint`
- 비어 있던 `server-management/` 도메인과 승인·구현 책임이 없는 `operations/proposals/` 문서 다섯 개를 제거했다.
- Roadmap #43의 폐기 결정과 충돌하는 자가치유 설계, 위험한 패키지·SSH·방화벽 실행 예시, 구현되지 않은 무결성 감시 계약은 현재 문서에서 제거하고 Git 이력을 복원 원천으로 삼았다.
- 폐기된 흐름을 구현 사실처럼 보이게 하던 전용 Mermaid 네 개와 잔여 색인·검토 링크를 함께 정리했다.
- 향후 운영 자동화는 Roadmap ID 부여, Architecture 또는 Security 설계 승인, 구현 검증을 거친 뒤 Deployment Runbook으로 승격한다.

## 2026-08-17 — Consumer-facing 계약의 현재·제안 상태 분리

- 유형: `ingest` + `lint`
- `contracts/` 진입점을 현재·부분 구현 계약과 Roadmap에 연결된 제안 계약으로 나누고 코드·wire schema·목표 prose의 우선관계를 명시했다.
- HTTP의 Worker credential 민감 표면과 pagination 한계, Dashboard capability·오류 envelope, MCP의 `from_offset`과 현재 무권한 실행 경계를 코드에 맞춰 보강했다.
- Worker enrollment의 HTTP 오류 응답 원문 token 노출을 현재 위험에 추가하고, Agent·Project 문서를 `proposed-contract`로 분류해 transport·권한·동시성 활성화 게이트를 명시했다.
- OpenAPI의 삭제된 Architecture 문서 참조를 Contracts 정본으로 교체하고 Roadmap #49와 중앙 색인을 동기화했다.

## 2026-08-18 — 공유 Worker pool과 Project Task 경계 기반 추가

- 유형: `ingest` + `lint`
- Project가 Host/Worker를 예약하는 모델을 폐기하고, Project-owned Agent의 일시적 Worker execution lease와 agent 수 상한 모델로 정정했다.
- 기본 admission은 Project active agent 1, warm agent 0, Agent당 동시 Attempt 1이며 Worker의 process 상한을 함께 적용한다. 이는 자원 격리가 아닌 동시성 제어다.
- `TaskRequest.project_id`를 core, CLI `fleet tasks submit --project-id`, MCP `fleet_dispatch_task.project_id`에 연결해 UUID 검증 후 `tasks.project_id`로 영속한다. Project 엔터티·정책 enforcement 이전에는 스케줄링이나 권한을 바꾸지 않는다.

## 2026-08-18 — Fleet 내부 Security Manager credential authority 확정

- 유형: `ingest`
- Project·Agent·TaskAttempt credential의 원문 authority를 Fleet Security Manager로 정했다. Orchestrator는 policy/reference만 다루고 암호문 backend를 직접 복호화·export하지 않는다.
- 초기 encrypted backend는 Postgres를 허용하되, Security Manager 뒤에 캡슐화해 KMS/HSM·외부 backend로 교체 가능하게 한다.
- Worker에는 Attempt·Worker identity에 묶인 one-time short-lived delivery grant로만 전달하며, tmpfs/file descriptor 전달·fail-closed revision reconcile·break-glass export를 원칙으로 정했다.

## 2026-08-18 — Agent execution lease의 self-fencing 보강

- 유형: `ingest`
- 단순 TTL/ACK 모델의 network partition, ACK 유실, Worker 재기동 뒤 중복 Agent process 위험을 확인하고 `worker_execution_lease`에 Worker incarnation·control epoch·단조 fencing token을 추가했다.
- slot claim은 DB CAS 단일 writer로 계산하고, Worker는 stale token을 거절하며 control lease 상실 시 새 실행을 멈추고 grace 뒤 self-fence/drain한다.
- Start/stop 결과 불명확은 `OutcomeUnknown`으로 유지해 inventory 관측 전 새 lease·중복 start를 금지한다.

## 2026-08-18 — Host당 단일 Worker daemon 기본 확정

- 유형: `ingest`
- 기본 운영 모델을 Host 1 : Worker daemon 1로 정했다. 단일 Worker가 다수 Agent process를 slot·container isolation·cleanup 아래 관리한다.
- 다중 Worker는 `multi_worker_enabled`, capability partition, 독립 namespace, 별도 capacity accounting이 모두 갖춰진 예외 운영으로만 허용하며 Project 배정 수단으로 쓰지 않는다.

## 2026-08-18 — TaskAttempt effect ledger와 부분 적용 규칙 확정

- 유형: `ingest`
- Task 의도와 TaskAttempt 실행을 분리하고 외부 부작용은 durable effect ledger에 `Planned`부터 receipt/보상 상태까지 기록하도록 정했다.
- 전달 결과가 불명확하거나 보상이 실패한 effect는 `PartiallyApplied`이며, 현재 외부 Task API에는 `Failed + failure_disposition`으로 호환 투영한다. 자동 retry는 금지한다.
- 외부 idempotency key는 Attempt generation이 아니라 Task 의도와 effect scope에서 안정적으로 파생하고, 완료는 checkpoint·effect 증거·credential/lease 정리까지 확인한 뒤에만 확정한다.

## 2026-08-18 — 공유 Host Agent 격리·privileged helper 규칙 확정

- 유형: `ingest`
- 공유 Host는 rootless·비특권 `container_required`를 기본으로 하고, read-only rootfs·Agent 전용 worktree·tmpfs secret·deny-by-default egress·고정 image digest를 Attempt snapshot으로 확정했다.
- Git/secret/workspace 경계를 명시하고 다른 Agent worktree, host socket, global credential store, metadata/private network 접근을 기본 거부했다.
- Fleet의 sudo 허용은 Agent의 arbitrary sudo가 아니라 fencing·scope·typed schema를 재검증하는 root-owned `fleet-privileged-helper`의 allow-listed operation으로 제한했다.

## 2026-08-18 — Hibernated 기본과 WarmIdle bounded optimization 확정

- 유형: `ingest`
- 기본 `max_warm_agents = 0`에서 Task 종료 뒤 Agent process는 Hibernated로 전이한다. WarmIdle은 별도 Project 상한과 Worker process slot을 소비하는 opt-in 최적화다.
- WarmIdle에는 credential·attach grant를 남기지 않고 runtime/image·isolation·workspace·Tool/Skill·egress/privileged 정책이 모두 호환될 때만 재사용한다.
- TTL, drain/revoke, control lease 상실, policy 변경, Worker 압박에서 WarmIdle을 종료하며 만료→drain/revoked→LRU 순서의 eviction과 cleanup/lease release를 확정했다.

## 2026-08-18 — Project archive·hold·retention 규칙 확정

- 유형: `ingest`
- archive를 idempotent drain workflow로 정하고, Attempt terminal뿐 아니라 Worker inventory의 process 부재, lease/credential grant cleanup, effect/security/legal hold 해소를 모두 `Archived` 전이 조건으로 했다.
- 미해결 `PartiallyApplied` effect나 cleanup 실패는 `ArchiveBlocked`와 durable hold로 보존하며, 보상·risk acceptance·hold 전환 없이는 archive하지 않는다.
- archive 뒤 context는 read-only로 봉인하고 retention 이후의 영구 삭제는 Git·audit 증거를 각각 남기는 별도 관리 작업으로 분리했다. reopen은 새 policy/Agent/lease/grant만 만든다.

## 2026-08-18 — 관측성·재조정·장애 복구 계약 확정

- 유형: `ingest`
- desired/observed/reconciliation result를 분리하고 control plane, Worker, Agent lease, TaskAttempt, effect, credential grant, archive hold별 불일치 상태를 정했다.
- Reconciler의 자동 범위를 safe cleanup·quarantine·증거 수집으로 제한하고, external redrive·risk acceptance·hold 해제·durable data 삭제는 운영자 권한으로 남겼다.
- inventory-first recovery, metric/audit 상관관계와 고카디널리티·secret 금지, alert별 초기 운영 action과 구현 게이트를 새 정본에 기록했다.

## 2026-08-18 — Principal·capability·Project scope·감사 계약 확정

- 유형: `ingest`
- HTTP, Dashboard, MCP, Worker control에 공통 AuthorizationContext와 fail-closed evaluator를 적용하는 목표 계약을 만들었다.
- Human/Automation/Worker/AgentProcess/SecurityManager/Bootstrap principal을 분리하고, Agent process는 일반 control-plane principal을 갖지 않도록 정했다.
- Project scope resolver, capability matrix, non-enumerating 오류, MCP authenticated launcher, break-glass dual approval, append-only secret-free audit의 규칙과 구현 게이트를 추가했다.

## 2026-08-18 — Project·Agent 운영 모델 구현 increment 재정렬

- 유형: `ingest`
- Roadmap #66~#70을 Security Manager, Agent execution lease, archive/retention/hold, Git checkpoint recovery, observability/reconciliation으로 등록했다.
- 구현 순서를 production trust → Worker identity/credential authority → execution consistency/control authority → workspace/isolation → 최소 Project/Agent → 운영 완결성 → 확장 기능으로 재구성했다.
- 첫 착수 단위를 #58 AuthorizationContext/fail-closed middleware와 #59 bootstrap digest migration으로 고정하고, 그 전에는 새 Project/Agent/Security Manager 외부 API를 활성화하지 않도록 했다.

## 2026-08-18 — Production trust foundation 초기 구현

- 유형: `implementation` + `verification`
- HTTP는 no-auth provider가 없으면 fail-closed로 거절하고, non-loopback bind는 bearer token 또는 Cloudflare Access audience 없이 기동하지 않도록 했다. AuthorizationContext의 최소 기반을 추가했으며 capability/scope와 Cloudflare claim principal 추출은 후속 #58 범위다.
- BootstrapToken은 원문 대신 SHA-256 digest만 보관하도록 core·memory/PostgreSQL store·API/MCP 회수 경로를 전환했다. PostgreSQL migration은 기존 원문 primary key를 digest로 원자 치환하며 발급 응답에서만 원문을 반환한다.
- `cargo check --workspace`, bootstrap API 통합 테스트, MCP/core/store/dashboard 단위 테스트와 `git diff --check`를 통과했다. 실제 PostgreSQL migration/DB dump 검증은 별도 환경에서 수행해야 한다.

## 2026-08-18 — MCP launcher capability 경계 초기 구현

- 유형: `implementation` + `verification`
- MCP stdio는 `FLEET_MCP_CAPABILITIES`의 명시 capability allow-list가 없으면 기동하지 않으며, 허용된 도구만 목록과 호출 경로에 노출하도록 했다. 자연어·tool argument로 권한을 확장할 수 없다.
- 이는 authenticated launcher assertion/local peer identity, Project scope evaluator, audit actor를 대체하지 않는 초기 경계다. 해당 identity 전파와 HTTP endpoint별 enforcement는 #58의 남은 범위로 유지한다.

## 2026-08-18 — HTTP scoped bearer credential 전환

- 유형: `implementation` + `verification`
- `FLEET_API_TOKENS`를 쉼표 구분 평면 allow-list에서 `principal_id`·`token`·`capabilities`를 가진 JSON manifest로 전환했다. 빈/unknown/권한 없는 manifest는 fail-closed하며, token 원문은 context·로그에 남기지 않는다.
- Worker 목록/등록·삭제 및 bootstrap token 관리 route에 최소 capability 행렬을 적용했다. Project scope, credential endpoint, Cloudflare principal, Worker self binding, audit은 아직 후속 단계다.
- `cargo test -p fleet-api --tests`, `cargo check --workspace`, `git diff --check`로 검증했다.

## 2026-08-18 — Worker operational credential enrollment 계약 확정

- 유형: `design`
- bootstrap token은 join 단 한 번의 승인에만 사용하고, 성공 뒤 worker_id 결합 operational credential을 1회 발급·digest 저장하는 흐름을 Worker enrollment 정본에 추가했다.
- `worker:self` principal은 register/heartbeat/deregister의 자기 worker_id만 조작하며, old `bootstrap_token`을 worker.toml의 장기 bearer로 보관하지 않는다. 구현은 새 credential store schema, atomic enrollment transaction, worker config migration, self-binding middleware를 한 increment로 진행한다.
- `worker_operational_credentials`의 digest-only PostgreSQL schema migration을 추가했다. 아직 실행 경로가 새 table을 읽거나 credential을 발급하지 않으므로 migration만 적용한 상태에서 Worker 동작은 바뀌지 않는다.

## 2026-08-18 — Worker operational credential join 경로 초기 구현

- 유형: `implementation` + `verification`
- join 성공 시 `fwo_` operational token을 새로 생성해 worker.toml에만 1회 넣고, DB/memory store에는 SHA-256 digest·worker_id·lifecycle metadata만 기록하도록 연결했다. Worker daemon은 `operational_token`으로 register/heartbeat/deregister bearer를 보낸다.
- HTTP middleware는 active digest를 `worker:<worker_id>` principal과 `worker:self` 최소 capability로 해석한다. register의 `existing_worker_id`와 heartbeat의 body worker_id가 credential binding과 다르면 거절한다. join route는 bootstrap body를 자체 인증 수단으로 처리한다.
- enrollment의 bootstrap consume·Worker 생성·credential 발급이 아직 하나의 DB transaction은 아니며, credential rotation/revoke·deregister binding·mTLS·agent endpoint query secret 제거는 남은 #60 범위다. `cargo test -p fleet-api --test bootstrap_tokens`, `cargo test -p fleet-worker --lib`, `cargo check --workspace`, `git diff --check`를 통과했다.

## 2026-08-18 — Worker credential 10단계 점진 전환 계획

- 유형: `implementation-plan`
- 새 Worker operational credential 경로를 먼저 완결하고 검증한 뒤, bootstrap_token·평면 bearer·fallback을 삭제하는 10개 단계와 각 완료 증거를 Roadmap 정본으로 고정했다.

## 2026-08-18 — 설계 문서 판단 방법 정본화

- 유형: `governance`
- 문서 권위·현재 구현·목표 계약을 분리하고, entity/authority/state/effect/failure를 기준으로 설계를 읽어 Roadmap·log·test까지 연결하는 공통 판단 방법을 Governance 정본으로 추가했다.

## 2026-08-18 — Worker credential 전환 상태 표기 정정 (lint)

- 유형: `lint`
- [설계 문서 판단 방법](governance/design-document-reading-and-judgment.md)을 적용해 [Worker credential 전환](roadmap/worker-credential-migration.md)과 `roadmap.md` #60을 코드와 대조한 결과, 앞선 "join 경로 초기 구현" 로그 항목과 두 정본이 실제 코드보다 앞서 있음을 확인했다.
- 사실 확인: `crates/fleet-store/src/postgres.rs`에 트랜잭션 경계(`.begin()`)가 없어 join의 bootstrap 소비·Worker 생성·credential 발급은 atomic하지 않다(2·3·4단계 미착수). `find_active_worker_operational_credential`을 호출하는 코드가 없고 `AuthorizationContext`에 worker 식별자 필드도 없어, register/heartbeat/deregister의 `worker:self` binding은 부분 구현이 아니라 미착수다(5단계) — `app.rs`의 `authorize_http_endpoint` 자체 주석도 이를 명시한다. 따라서 임의의 `worker:register`/`worker:delete` capability 보유 principal이 다른 worker_id를 조작할 수 있는 상태다.
- `worker-credential-migration.md`의 2~5단계 상태·완료 증거·"현재 경계" 절과 `roadmap.md` #60 설명을 코드에 맞게 정정했다. 코드 변경은 없으며, 다음 증분(atomic enrollment + self-binding 미들웨어를 한 단위로 구현)은 별도로 진행한다.

## 2026-08-18 — Worker credential atomic enrollment·self-binding 구현 (2~5단계)

- 유형: `implementation`
- `Store` 트레이트에 `enroll_worker` 기본 메서드(bootstrap 소비·Worker 생성·credential 저장을 순차 호출하는 fallback)를 추가하고, `PgStore`는 `pool.begin()` 단일 트랜잭션으로, `MemStore`는 `bootstrap_tokens`/`workers`/`worker_operational_credentials` 세 `Mutex`를 한 스코프에서 보유하는 방식으로 각각 오버라이드해 all-or-nothing을 보장했다. `join_worker` 핸들러를 기존 3개 독립 store 호출에서 이 단일 `enroll_worker` 호출로 재작성했다.
- `AuthorizationContext`에 `worker_id: Option<WorkerId>` 필드와 `AuthenticationMethod::WorkerOperational` variant를 추가하고, `auth_middleware`가 operational credential digest를 조회해 인증된 요청에 자신의 worker_id를 실어 보내도록 했다. `register_worker`/`heartbeat`/`deregister_worker`는 `enforce_worker_self_binding` 헬퍼로 `ctx.worker_id`와 요청 대상 worker_id를 비교해 불일치 시 `ApiError::Forbidden`(403)을 반환한다. admin bearer/Cloudflare Access/no-auth 경로(`ctx.worker_id == None`)는 이 제약을 받지 않는다.

## 2026-08-18 — Worker credential atomic enrollment·self-binding 검증

- 유형: `verification`
- `crates/fleet-store/src/mem.rs`의 `enroll_worker_tests`(`enroll_worker_commits_all_three_on_success`/`enroll_worker_rolls_back_on_credential_digest_conflict`/`enroll_worker_rolls_back_on_name_conflict`)와 `crates/fleet-api/tests/worker_self_binding.rs`(`heartbeat_for_self_succeeds_but_for_other_worker_is_forbidden`/`register_for_self_succeeds_but_impersonating_other_worker_is_forbidden`/`deregister_self_succeeds_but_deregistering_other_worker_is_forbidden`)를 포함해 `cargo test --workspace`가 전부 통과했다.
- 로컬 PostgreSQL(`postgres://$(whoami)@localhost/fleet_test`)에 `DATABASE_URL`을 주입해 `crates/fleet-store/tests/enroll_worker.rs`(PgStore 트랜잭션 rollback/commit 3종)와 기존 `integration.rs`/`auth_integration.rs`/`audit_integration.rs` 스위트를 직렬(`--test-threads=1`)로 재실행해 모두 통과함을 확인했다.
- `cargo check --no-default-features`, `cargo clippy --all-targets --all-features`(fleet-api/fleet-store 대상 파일에는 경고 0 — fleet-worker/fleet-scheduler/fleet-mcp/vendor 예제의 기존 경고는 이번 작업과 무관한 범위), `git diff --check`를 통과했다.

## 2026-08-19 — Worker credential rotate/revoke·안전한 bootstrap 전달·legacy 삭제 (6~8단계)

- 유형: `implementation` + `verification`
- 6단계: `Store`에 `rotate_worker_operational_credential`/`revoke_worker_operational_credential`을 추가하고 PgStore(`UPDATE ... RETURNING`, `rotation_generation` 증가)와 MemStore에 구현했다. `find_active_worker_operational_credential`은 이미 `revoked_at IS NULL AND (expires_at IS NULL OR expires_at > NOW())` 필터를 갖고 있어 추가 변경이 필요 없었다. `POST /v1/workers/:id/credential/rotate`/`DELETE /v1/workers/:id/credential`을 새 `PermissionKind::WorkerCredentialManage` capability로 fail-closed 등록했다 — worker operational credential 인증(`worker:self`)에는 이 capability를 부여하지 않아 worker가 스스로 자기 credential을 회전시킬 수 없다(관리자 전용). `fleet workers credential rotate/revoke` CLI를 추가했다.
- 7단계: `fleet-worker join`의 `fleet_worker::join::resolve_bootstrap_token`이 `--token-file <path>`(경로가 `-`면 stdin)를 지원한다. 기존 `--token`(argv/env)은 deprecated로 유지하되 원문을 절대 보간하지 않는 고정 경고 로그만 남긴다.
- 8단계 조사 중 실제 배선 버그를 발견했다: `crates/fleet-worker/src/handlers.rs`(`fleet-api`)의 join 응답은 `operational_token` 필드로 worker.toml을 렌더링하지만, `fleet-worker`의 `WorkerSection`은 그 필드를 `bootstrap_token`이라는 다른 이름으로 갖고 있어 join 이후 생성된 config는 조용히 무시되는 필드를 가진 채 `bootstrap_token`이 항상 `None`이었다 — register/heartbeat/deregister가 Authorization 헤더 없이 나가고 있었다(보호된 orchestrator에서는 매 요청이 401). `WorkerSection.bootstrap_token`을 `operational_token`으로 교체하고 `registration.rs`의 세 호출부(register/heartbeat/deregister)가 실제로 그 값을 bearer로 보내도록 배선했다. 남은 `[worker] bootstrap_token` 키는 `WorkerConfig::from_str`에서 명시적으로 거부한다(자동 마이그레이션 없음 — bootstrap token은 원문이 이미 소진됐거나 digest 기반이 아니므로 재사용 금지).
- fleet-api의 평면 쉼표 구분 `FLEET_API_TOKENS` bearer 목록은 이번 6~8단계 작업 이전에 이미 principal·capability를 가진 JSON manifest(`ApiTokenCredential`)로 전환되어 있었다 — 이번 커밋 범위에는 포함하지 않았다(별도 작업 경계).
- **알려진 갭**: `fleet provision`(SSH 자동 프로비저닝)은 여전히 `ProvisionOptions.bootstrap_token`을 worker.toml의 `[worker] bootstrap_token`으로 직접 기록한다 — 8단계의 fail-closed 검사 덕분에 이렇게 생성된 config는 daemon 기동 시점에 명확히 거부되므로 인증 없이 조용히 동작하는 위험은 없지만, `fleet provision`을 `/v1/workers/join`과 자동 연동하는 배선 자체는 아직 없다. 프로비저닝된 워커는 `fleet-worker join` 재실행 또는 `fleet workers credential rotate` 결과를 `operational_token`에 수동 반영해야 한다.
- 9·10단계(mTLS, staging rehearsal)는 실제 PKI/staging 인프라가 없어 착수하지 않았다 — 코드 스캐폴드도 만들지 않았다. 9단계 설계 방향만 `worker-credential-migration.md`에 짧게 기록했다.
- 신규/변경 테스트: `crates/fleet-api/tests/worker_credential_rotation.rs`(5종), `crates/fleet-store/tests/worker_credential_rotation.rs`(4종, PostgreSQL 대상), `crates/fleet-worker/src/join.rs`의 `resolve_token_*`(6종, 로그 redaction 포함), `crates/fleet-worker/src/config.rs`의 `legacy_bootstrap_token_field_is_explicitly_rejected`/`operational_token_field_parses_correctly`/`config_without_any_token_field_still_parses`, `crates/fleet-worker/src/registration.rs`의 `operational_token_is_sent_as_register_bearer`/`no_operational_token_sends_no_authorization_header`.
- `cargo check --workspace`(`--all-features`/`--no-default-features` 둘 다), `cargo clippy --all-targets --all-features`(이번에 건드린 파일 기준 경고 0), `cargo test --workspace`(전부 통과), `DATABASE_URL=postgres://$(whoami)@localhost/fleet_test`로 PgStore 통합 테스트 포함 재실행(전부 통과), `git diff --check`를 통과했다. `DATABASE_URL` 설정 시 `crates/fleet-mcp/tests/cross_client.rs` 12개 테스트가 실패하는 것을 발견했는데, 원인은 이번 작업과 무관한 별도 미커밋 변경(`FLEET_MCP_CAPABILITIES` 필수화)이 이 통합 테스트의 subprocess 기동 인자를 갱신하지 않은 상태로 남아있기 때문이다(`fleet serve`가 `FLEET_MCP_CAPABILITIES is required for MCP stdio`로 즉시 종료) — fleet-mcp 파일은 이번 커밋에 포함하지 않았으므로 별도로 추적해야 한다.

## 2026-08-19 — Task dispatch credential precondition을 #71로 등록

- 유형: `ingest`
- 사용자 질의("orchestrator가 일을 요청받으면 credential이 없어서 진행 못 하는 경우가 있는가")에 답하며 코드를 추적한 결과, `fleet-scheduler`의 worker 선택 로직(`crates/fleet-scheduler/src/selector.rs`)이 `worker_credentials`를 전혀 참조하지 않고, LLM credential은 `fleet provision`의 `PushCredentials` 스텝(Task 흐름과 완전히 분리된 수동 프로비저닝 단계)을 통해서만 워커 호스트의 `~/.grok/config.toml`에 반영되며, credential이 비어 있으면 그 스텝조차 조용히 no-op함을 확인했다. `crates/fleet-core/src/task.rs`의 `FailureKind`에도 credential 관련 분류가 없어, LLM 인증 실패가 발생해도 일반 `WorkerError`로만 남는다.
- 이 갭을 `#71`(Task dispatch credential precondition)로 로드맵에 등록했다. 설계는 이미 코드 관례로 확정된다: `Task.resolved_model`이 있으면 worker 후보 필터에 해당 model의 활성 credential 보유 여부를 추가하고(label 필터와 동일 자리), 후보가 전부 제외되면 기존 `DispatchError::NoWorker` 재시도/dead-letter 경로를 재사용하되 최종 실패 사유를 신설 `FailureKind::CredentialMissing`으로 구분한다. `model`이 없는 task는 검사 대상이 아니다.
- 별도 architecture 문서를 새로 만들지 않고 `roadmap.md` 행 자체에 설계 요약을 담았다 — 범위가 스케줄러 한 곳의 필터 조건 추가와 실패 사유 taxonomy 확장으로 좁고 자기완결적이기 때문이다.

## 2026-08-20 — `#71` 로드맵 상태 회귀 정정 (lint)

- 유형: `lint`
- `#72`를 등록하려고 `roadmap.md`를 열었다가, `#71`(완료로 기록됐어야 할 항목, 커밋 `5ca8d16`)이 최신 커밋(`df0be43`, `#61` 작업)에서 "설계 확정·구현 대기"로 되돌아가 있는 것을 발견했다. `git grep`으로 `crates/fleet-scheduler/src/selector.rs`의 `SelectionError::NoWorkerForCredential`, `crates/fleet-core/src/task.rs`의 `FailureKind::CredentialMissing`이 여전히 존재함을 확인 — **코드가 아니라 문서 상태만 회귀**했다. 원인은 여러 백그라운드 에이전트가 순차적으로 `docs/roadmap/roadmap.md`에 checkout-HEAD → 편집 → stage → WIP 복원 절차를 반복하는 과정에서, 특정 시점의 "HEAD" 스냅샷이 그보다 앞선 커밋의 완료 기록을 반영하지 못한 것으로 추정된다(정확한 재현은 하지 않았다). `#71` 행을 `5ca8d16` 커밋의 원문으로 복원했다.
- **교훈**: 여러 세션/에이전트가 같은 세션 안에서 순차적으로 `roadmap.md`/`log.md`에 대해 checkout-dance를 반복할 때는, 각 단계가 끝난 뒤 이전에 완료로 기록한 항목들이 여전히 완료로 남아있는지 다음 단계 시작 전에 재확인해야 한다. 이번처럼 우연히 발견하지 못하면 조용히 누락될 수 있다.

## 2026-08-20 — Admin API bearer token DB 기반 rotate/revoke를 #72로 등록

- 유형: `ingest`
- 사용자 요청("credential 회전 건을 먼저 정리")에 따라 `FLEET_API_TOKENS`/`FLEET_GMAIL_APP_PASS` 회전 경로를 검토했다. `FLEET_GMAIL_APP_PASS`는 Google이 발급하는 제3자 시크릿이라 orchestrator가 자동 회전할 수 없음을 확인 — 이 항목은 로드맵 대상이 아니다. `FLEET_API_TOKENS`는 orchestrator 자신이 값을 발급하는 admin bearer 토큰이라 `#60`(Worker operational credential)과 동일한 패턴(DB digest 저장, rotate API, 즉시 이전 값 무효화)을 적용할 수 있다고 판단해 `#72`로 등록했다. 사용자가 "orchestrator가 SSH로 자기 호스트를 직접 고치는 자동화"와 "DB 기반 rotate API" 두 방향 중 후자를 선택했다(자기 자신을 SSH로 재기동하는 자동화는 실패 시 서비스가 먹통이 될 위험이 커 배제).
- 설계는 `roadmap.md` 행에 요약했다: 신규 `admin_api_tokens` 테이블, rotate/revoke/create/list API, 기존 `token:*` capability(bootstrap token 전용)와 겹치지 않는 새 capability, `FLEET_API_TOKENS` env 값을 최초 기동 시 DB로 1회 자동 가져오는 무중단 전환 경로. 아직 코드 변경은 없다.

## 2026-08-20 — 로드맵 상태 회귀 2차 발견·전면 복구 (lint)

- 유형: `lint`
- `#72` 문서화를 위해 `roadmap.md`를 열었다가, `1053160`(직전 `#71` 정정 커밋)에서도 여전히 `#57`·`#58`·`#59`·`#66`이 `df0be43`(`#61`) 커밋 당시의 회귀를 그대로 안고 있었음을 발견했다 — 지난번엔 `#71` 한 줄만 확인하고 넘어가 같은 커밋이 망가뜨린 나머지 네 줄을 놓쳤다. `902d8f8`(`#66` 커밋, `df0be43` 직전 마지막 정상 상태)의 `#57`~`#65` 블록 전체를 근거 삼아 `#57`(완료)·`#58`(부분 구현)·`#59`(완료)·`#66`(부분 구현, Worker LLM credential 인가·감사)을 원문 그대로 복구했다. `#61`의 올바른 문구(`부분 구현`, 1~2단계 완료)는 어떤 커밋에도 남아있지 않아(`git log -S`로 전체 이력 확인) 이 세션 앞선 turn에서 실제로 작성했던 문구를 그대로 재입력했다. `#67`~`#70`은 `902d8f8`에는 아예 없던 행(그 시점 이후 별도로 추가됨)이라 비교 대상에서 제외하고 `1053160`과의 일치만 확인했다 — 이번 정정에서 손대지 않았다.
- **재발 방지 확인 절차를 실제로 실행**: `#48`~`#70` 전 ID를 `1053160`과 diff해 이번에 의도적으로 바꾼 `#57`/`#58`/`#59`/`#61`/`#66` 다섯 줄 외에는 단 한 글자도 달라지지 않았음을 스크립트로 확인한 뒤에만 커밋했다. `docs/log.md`의 이전 lint 항목(2026-08-20, `#71` 단독 정정)에 적어둔 "다음 checkout-dance 전에 이전 완료 항목이 여전히 완료로 남아있는지 재확인" 절차를 처음으로 실제 적용한 사례다.
- 근본 원인은 여전히 재현하지 못했다(추정: 여러 백그라운드 에이전트가 겹치는 시간대에 `/tmp`의 백업 파일명을 재사용했을 가능성). 반복 재발을 막으려면 파일명에 타임스탬프나 PID를 섞는 등 근본적인 조치가 필요할 수 있다 — 다음 세션 과제로 남긴다.

## 2026-08-21 — `#53` Worker LLM proxy 설정 원자성 구현 완료

- 유형: `ingest`
- `crates/fleet-worker/src/config.rs`의 `LlmProxySection { gateway_url, api_key }`가 서로 완전히 독립적으로 검증돼 온 문제를 수정했다. 이전에는 `gateway_url`만 있으면 `grok`/`agy` 하위 프로세스가 인증 없이 게이트웨이를 호출했고, `api_key`만 있으면 **liteLLM master key가 BASE_URL 오버라이드 없이 실제 OpenAI/Anthropic 등 provider 엔드포인트로 그대로 전송**되어 게이트웨이 master key가 제3자 서비스로 유출되는 것과 동일한 심각도의 오설정이 가능했다. `gateway_url`이 스킴 없는 문자열이어도 검증 없이 그대로 subprocess env로 주입됐다.
- `WorkerConfig::validate()`(`worker.orchestrator_url` 검증과 동일한 관례를 따름)에 `[llm_proxy]` 검증을 추가: `gateway_url`/`api_key` 중 정확히 하나만 있으면 어느 필드가 빠졌는지 명시한 `WorkerError::Config`로 거부, `gateway_url`은 `http://`/`https://` 스킴을 강제. `llm_proxy` 섹션 자체가 없거나 두 필드 다 있는 정상 조합은 그대로 통과 — 기존 배포(게이트웨이 미사용 워커)는 100% 호환. 이 검증은 `WorkerConfig::from_str`(`validate()`를 이미 호출) 경로를 통해 `worker.toml` 파싱 시점에 걸리므로, 잘못된 조합의 설정은 데몬이 기동조차 못 하고 즉시 명확한 에러로 종료된다.
- `apply_llm_proxy_envs`(`grok_process.rs`) 자체는 변경하지 않았다 — `validate()`가 이미 반쪽짜리 조합을 막아준 뒤에만 호출된다는 전제가 성립하므로, subprocess env 주입 로직은 그대로 두고 상위 검증만 추가하는 편이 변경 범위를 좁히고 기존 회귀 테스트(`apply_llm_proxy_envs_sets_grok_and_agy_envs`)를 그대로 재사용할 수 있어 더 안전하다고 판단했다.
- `WorkerConfig::from_str` 통합 테스트 5건(`llm_proxy_gateway_url_only_is_rejected`, `llm_proxy_api_key_only_is_rejected`, `llm_proxy_gateway_url_without_scheme_is_rejected`, `llm_proxy_valid_combination_parses_ok`, `llm_proxy_section_absent_parses_ok`)을 `config.rs`에 추가 — `validate()`를 직접 호출하지 않고 실제 `worker.toml` 파싱 경로 전체를 거치게 해서, 로드맵 `#53` 완료 게이트(URL-only, key-only, invalid URL, 정상 조합, subprocess 환경변수 회귀 테스트)를 실제 기동 경로 기준으로 충족시켰다.
- `cargo check --workspace`(`--all-features`, `--no-default-features`), `cargo clippy --all-targets --all-features`(변경 파일 기준 신규 경고 0 — 기존 경고는 `fleet-scheduler`/`fleet-mcp`/`fleet-worker`의 다른 파일에 이미 있던 것과 동일), `cargo test --workspace`(신규 5건 포함 전부 통과) 모두 통과를 확인했다.

## 2026-08-22 — 멀티 에이전트 설계 검토와 로드맵 상태 회귀 3차 복구

- 유형: `lint` + `ingest`
- 커밋되지 않은 워킹트리(문서 31건 수정 + 신규 4건)를 3인 병렬 감사(코드 대조·적대적 보안·규약)로 검토하고 결과를 [멀티 에이전트 설계 검토 보고서](reviews/multi-agent-design-review-2026-08-22.md)에 남겼다.
- **회귀 3차 재발 확인·복구**: 워킹트리의 `docs/log.md`·`docs/roadmap/roadmap.md`가 커밋 `df0be43`과 바이트 단위로 동일했다(`git show df0be43:… | diff -`). 즉 `1053160`·`cae6492`·`f31eaee` 세 커밋이 통째로 되돌려진 상태였고, `#53`·`#57`·`#58`·`#59`·`#61`·`#66`·`#71` 일곱 행이 후퇴하고 `#72` 행이 소실됐으며 append-only log 항목 4건(그중 2건이 1·2차 회귀 정정 기록)이 삭제될 상황이었다. `docs/deployment/README.md`도 `af23538` 스냅샷으로 되돌아가 `mcp-clients.md`를 고아화하고 있었다. 세 파일을 HEAD로 복원했다.
- HEAD에 선재하던 결함도 정리했다: `roadmap.md`의 `#72` 중복 행(완료/대기 2행) 병합, `roadmap/README.md`의 `worker-credential-migration.md` 중복 행 제거, 섹션 헤더 범위 동기화.
- **신규 영구 ID 4건 등록**: `#73`(capability 행렬 기본 deny 전환 — 미등록 route가 검사 없이 통과하며 현재 `GET /v1/workers/{id}`·`POST /v1/hosts/register`가 누락), `#74`(Cloudflare principal 매핑 fail-closed — 매핑 부재 시 `PermissionKind::all()`을 부여하는데 `fleet-cli`에 매핑 설정 경로가 없어 운영 배포에서 끌 수 없음), `#75`(worker `endpoint`의 `server-key` 평문 전파 차단), `#76`(감사 범위 확장과 상관관계 필드 — 현재 `AuditEvent`는 LLM credential 3개 route에만 존재).
- 설계 공백 보완: `CancelUnconfirmed`에 출구 전이(`Cancelled`/`PartiallyApplied`/`OutcomeUnknown`)와 재조정 규칙을 정의해 Attempt가 영구 비terminal로 남아 Project archive가 정지하는 경로를 닫았다. external idempotency key 파생식의 "정책 revision"을 제출 시 snapshot 값으로 못 박아 같은 문서 완료 게이트와의 모순을 해소하고 HMAC 키 회전 규칙을 추가했다. `worker-liveness-policy.md`에 "3단계 완료 전까지 `on_demand` 등록을 API가 거절한다"는 중간 상태 fail-closed 규칙을 명문화했다.
- 코드 대조 정정: `worker-enrollment.md`의 "현재 구현" 절에서 이미 해소된 3개 항목(원문 token 재기록, 오류 문자열의 원문 포함, token 선소비)을 해소 표시로 옮기되 여전히 사실인 `server-key` 평문 전파는 "남은 노출 경계"로 강조했다. `control-plane-security-model.md`의 상태 표, `http-api.md`의 route 표(누락 6건 추가, 단수 `/credential`과 복수 `/credentials` 구분 명시), `mcp-tools.md`의 보안 상태 절을 코드 기준으로 갱신했다.
- 남은 사실 확인: `config/inventory-from-ssh.yaml`도 같은 되돌림에 포함되어 arm2 호스트 6대를 제거하고 삭제된 `oci-ajou-arm1`을 되살린다. 실제 인스턴스 상태는 저장소 밖 사실이라 판정하지 않고 스테이징에서 보류했다.

## 2026-08-22 — 무인 부트스트랩 가능성 검토와 transport 정본 결정

- 유형: `design` + `ingest`
- orchestrator·seed worker·추가 호스트가 사용자 간섭 없이 bootstrap될 수 있는지를 3인 병렬 감사(부트스트랩 체인·프로토콜/절차·운영 현실)와 교차검증 1라운드로 검토하고 [무인 부트스트랩 검토](reviews/bootstrap-automation-review-2026-08-22.md)에 정리했다. **결론: 현재 무인 부트스트랩은 불가능하며, 무인화 이전에 이미 운영 중인 fleet을 망가뜨리는 결함이 있다.**
- **C1(최우선)**: `runner.rs`가 SIGTERM에서 `client.deregister()`를 호출해 `DELETE FROM workers`가 실행되고, `018`의 operational credential과 `005`의 암호화된 LLM 키가 CASCADE로 함께 삭제된다. 재기동 시 영구 401(5초 고정 간격 무한 재시도, 영구 실패 미구분)이며 인증이 필요한 모든 배포가 해당된다 — `runtime.rs`가 무인증 비-loopback bind를 거부하므로 원격 워커가 존재할 수 있는 구성은 전부 인증 모드다. 직관과 반대로 SIGKILL·전원 상실은 무사하고 `systemctl stop`이 파괴적이다. `mem.rs`는 cascade하지 않아 인메모리 테스트가 이 결함을 영원히 통과시킨다.
- 그 밖의 합의된 차단 지점: `fleet provision`이 만든 worker.toml을 데몬이 fail-closed 거부(프로비저너에 `join` 참조 0건), 인벤토리 모드의 `fleet_worker_bin: None` 하드코딩, `check_prereqs`의 arch 결과 폐기(커밋된 25대 중 7~8대가 arm64), `ssh.rs`의 원격 exit code 무시와 광범위한 `let _ =`/`|| true`, `examples/fleet.env`의 `FLEET_API_TOKENS` 형식이 현재 파서에 거부됨, 최초 admin 토큰의 닭-달걀.
- **Transport 실태**: 실운영은 리버스 SSH 터널 + orchestrator측 nginx 워커별 라우팅에 의존하는데(`config.rs` 주석이 2026-08-11 24시간 장애까지 기록) 저장소에 터널 자산이 0건이고 `fleet-api`에 `/ws` 라우트도 없으며, 정본 `topology.md`는 오히려 그 방식을 배제하고 있었다. 반면 mTLS 직접 다이얼은 런타임(`MtlsProxy` 배선, 인증서 무중단 회전, 발급 CLI)이 이미 완성되어 있고 PEM 업로드 스텝과 SAN 일관성 규칙만 빠져 있다.
- **결정 4건**: (1) transport를 mTLS 직접 다이얼로 확정하고 Cloudflare Tunnel·reverse SSH를 지원 토폴로지에서 제외, (2) C1은 자기 deregister 제거로 수정, (3) `fleet provision`이 join을 대행(완전 무인), (4) 이기종 fleet 지원. `topology.md`와 `worker-provisioning.md`를 결정에 맞게 갱신하고 `#77`~`#85`를 순서 근거와 함께 등록했다.
- 미결로 남긴 것: SSH host-key 정책(`tofu` 기본값 vs runbook의 `strict` 요구), Worker 신뢰 등급(`User=root` 배포본 vs `execution-isolation.md` 정본, LLM 키 평문 배포 vs gateway 경유), 릴리스 태그 정책.

## 2026-08-22 — `#78` graceful shutdown hard-delete 중단, `#77` 배포 예시 정합

- 유형: `implementation` + `verification`
- **`#78`**: `crates/fleet-worker/src/runner.rs`의 shutdown 경로에서 `client.deregister()` 호출을 제거했다. 이 한 줄이 `DELETE /v1/workers/{id}` → `DELETE FROM workers`를 유발했고, `worker_operational_credentials`(018 CASCADE)와 `worker_credentials`(005 CASCADE, 암호화된 LLM 프로바이더 키)가 함께 삭제됐다. 인증이 구성된 모든 배포에서 `systemctl restart`나 재부팅 한 번으로 워커가 신원과 LLM credential을 영구히 잃고 register가 영구 401(5초 고정 간격 무한 재시도)이 됐다. 역설적으로 SIGKILL·전원 상실은 deregister에 도달하지 않아 무사했다.
- 대체 수단을 새로 만들지 않았다 — "이 워커는 이제 없다"는 신호는 `fleet-scheduler`의 HealthChecker가 heartbeat timeout으로 Offline 전이와 `WorkerLeft` 이벤트를 내며 비파괴적으로 이미 담당한다. 영구 제거는 관리자의 `DELETE /v1/workers/{id}`만 수행하며, `WorkerClient::deregister` 메서드는 그 관리 경로용으로 유지했다(`delete_worker`의 프로덕션 호출부는 `handlers.rs`의 핸들러 하나뿐).
- `MemStore::delete_worker`를 PgStore와 동작 일치시켰다: 두 credential 테이블을 함께 제거하고 존재하지 않는 id에 `NotFound`를 반환한다. 이 divergence 때문에 결함이 모든 인메모리 테스트를 통과했으므로, 파리티 자체를 테스트로 고정했다. `config.rs`의 legacy `bootstrap_token` 거부 에러가 안내하던 복구 절차(`fleet workers credential rotate`)는 join을 거치지 않은 워커에 404를 반환하는 막다른 길이어서, 재-join을 안내하도록 문구를 정정했다.
- **`#77`**: `examples/fleet.env`의 `FLEET_API_TOKENS`가 평면 문자열이라 `parse_scoped_api_tokens`가 거부했다 — 저장소 예시를 그대로 따르면 `fleet serve`가 기동조차 하지 못했다. JSON manifest 형식으로 교체하고 capability 최소권한 안내를 붙였다(`worker:llm_credential:export`는 기본 예시에서 제외). `examples/fleet.service` 주석과, `examples/fleet-worker.service`의 "`fleet provision`이 자동 배포한다"는 잘못된 서술도 정정했다(실제 배포본은 `templates.rs`의 `User=root` 유닛이며 두 형상 일치는 별도 항목).
- 신규 테스트: `crates/fleet-store/src/mem.rs`의 `delete_worker_cascade_tests`(CASCADE 2종·무관 워커 자산 보존·`NotFound`), `crates/fleet-store/tests/worker_delete_cascade.rs`(동일 계약을 실제 PostgreSQL에서 검증), `crates/fleet-api/tests/verify_env_example.rs`(예시 manifest가 파서와 동일 조건으로 파싱되는지, export capability 미부여).
- 검증: `DATABASE_URL=postgres://$(whoami)@localhost/fleet_test`로 `worker_delete_cascade` 통과, `cargo check --no-default-features` 통과, `cargo clippy -p fleet-store -p fleet-worker -p fleet-api --all-targets --all-features` 경고 0.
- 부수 발견(미해결): `worker_operational_credentials.rotation_generation`은 PostgreSQL에 `CHECK (>= 1)`이 있으나 MemStore는 강제하지 않는다. 이번 테스트 작성 중 실제 DB에서만 제약 위반이 나 발견했다 — `#78` 행에 후속으로 기록했다.

## 2026-08-22 — 프로비저닝 인벤토리 사실 확인과 정정

- 유형: `lint` + `ingest`
- 앞선 항목("멀티 에이전트 설계 검토와 로드맵 상태 회귀 3차 복구")에서 `config/inventory-from-ssh.yaml`이 로드맵·log와 같은 스냅샷 되돌림에 포함된 것으로 보고 보류했으나, `df0be43`과 대조한 결과 **다른 파일이었다** — 되돌림이 아니라 별개 편집이었고 그 내용이 옳았다. 사용자 확인으로 두 사실을 확정했다.
- **arm2 6대(`oci-yarang-arm2`·`oci-ajou-arm2`·`oci-fcoinfup-arm2`·`oci-cyrus-arm2`·`oci-boom-arm2`·`oci-bok-arm2`)는 리소스 사유로 영구 삭제**됐다. 재생성 계획이 없으므로 인벤토리에서 제거하고 파일 상단 이력에 남겼다. HEAD 주석의 "arm2는 swuniv-chatbot을 서비스 중이라 유지"는 더 이상 유효하지 않다.
- **`oci-ajou-arm1`은 운영 중**이다. 커밋 `50ce018`("remove terminated oci-ajou-arm1")의 terminate 판단이 사실과 달랐으므로 항목을 되돌리고 주석을 정정했다.
- **`oci-yarang-arm1`은 현재 중지 상태이며 2026-09에 복구 예정**이다. 인벤토리에 남기되 복구 전에는 프로비저닝이 SSH 연결 실패로 끝난다는 주석을 달았다. `#79`(원격 실패 관측)와 `#81`(arch 감지) 이전에는 이런 호스트의 실패가 원인 불명으로만 남는다.
- 결과: 워커 30대 → 25대, arm64 7대(`oci-*-arm1` 계열). `#81`의 arm64 수치를 "7~8대"에서 실측 7대로 정정했다. `docs/credentials/registry.md`의 2026-08-20 `oci-yarangdev-arm2` terminate 기록은 이번 6대와 무관한 별개 사건이라 그대로 둔다.

## 2026-08-22 — UI 관리 대상 재해석과 Issue 추적 명세 설계

- 유형: `design` + `ingest`
- multi-agent-spec-designer 스킬로 Main Architect + Protocol Specialist + Workflow Specialist 체계를 구성해 UI 관리 대상과 Issue 기능을 설계하고 [명세 검토](reviews/ui-management-and-issue-spec-2026-08-22.md)에 정리했다.
- **목표 재해석**: 사용자가 제시한 목록(host·project·task·agent·agent_template·issue)이 균질하지 않았다. host/worker/task UI는 이미 구현됐고 project/agent는 `#48`/`#49`가 소유한다. 실제 신규는 둘이다 — `agent_template`은 UI 라우트(`/admin/agent-templates`)·필드(`default_agent_template_id`)·capability(`AgentTemplateManage`)가 여러 문서에 흩어져 있으면서 **소유하는 정본도 로드맵 ID도 없었고**, `issue`는 저장소 전체에 개념이 없었다.
- 두 전문가 모두 초안을 비준하지 않고 정정했다. 핵심 정정 5건: (1) Agent는 control plane principal이 아니므로 Issue를 직접 열 수 없다 — Worker control stream 보고 → control plane 대리 생성, `project_id`는 저장된 Attempt 행에서 유도(요청 본문 불신), (2) 템플릿을 정체성/불변 revision 2계층으로 분리, (3) Attempt는 revision을 참조하지 않고 본문·hash를 materialize(참조만 두면 retention purge가 `#65` 재현성을 깨뜨림), (4) tool 상승 차단의 판정 시점은 저장이 아니라 Attempt admission(저장 후 Project grant가 좁아지는 경우), (5) 열린 Issue는 archive를 막지 않음(막으면 Agent 생성 Issue 하나로 Project archive 무기한 교착).
- Issue/Task 경계를 확정했다: 부모-자식이 아닌 join 테이블 연관이며 `tasks`에 `issue_id`를 두지 않는다. `InProgress` Issue 상태를 두지 않는다(비터미널 연관 Task에서 유도 가능하며, 상태로 승격하면 Task 상태를 복제해 두 상태 머신이 경쟁한다). 교착 없음은 불변식 두 개로 강제한다 — Task/Attempt 전이 조건이 `issue.status`를 읽지 않고, Issue close에 Task 상태 선행 조건이 없다. **Task/Attempt 상태 머신은 한 글자도 바뀌지 않는다.**
- Agent 폭주 방지는 부분 유니크 인덱스(`(project_id, dedup_key) WHERE status IN ('open','triaged')`)를 주 방어선으로 5층을 둔다 — 같은 blocker 10,000회 보고가 Issue 1건 + `occurrence_count`가 된다. dead-letter 자동 생성은 Task당 1건이 아니라 원인별 집계로 "노이즈 대 유실" 딜레마를 해소한다.
- **신규 결함 3건 발견**(코드로 재확인): `crates/fleet-dashboard`에 중앙 capability 행렬이 없고 핸들러에 `PermissionKind` 검사가 29곳 산재한다 — `#73`은 `/v1`만 고치는데 신규 관리 화면은 대부분 Dashboard 표면에 놓이므로 `#73` 범위에 반영했다. MCP 표면도 동형이다. `ApiError`에 422/428/429가 없어 낙관적 동시성과 rate limit을 표현할 수 없다(`#92` 선행 조건).
- 신규 정본 2건([AgentTemplate](architecture/agents/agent-template.md), [Issue 추적](architecture/issues.md))을 추가하고 `#86`~`#92`를 순서 근거와 함께 등록했다. 기존 정본 6건에 필요한 변경은 각 항목 착수 시점에 반영하도록 검토 문서에 목록으로 남겼다.
- 사람 결정 8건(H1~H8)은 추측하지 않고 권고와 함께 남겼다. 가장 시급한 것은 H1 — 템플릿 편집이 사실상 tool 부여이므로 `project-feature-design.md`가 이미 `AgentCreate` 우회 우려로 걸어둔 차단 조건과 같은 질문이다.

## 2026-08-22 — AgentTemplate 편집 권한 결정 (H1)

- 유형: `design`
- `#86` 착수를 막고 있던 H1(템플릿 편집 권한을 `#48`의 구현 차단 조건 아래 둘 것인가)을 자료 확인 후 결정했다. 확인 과정에서 문제의 성격이 처음 제기됐을 때와 달랐다.
- **사실 1**: tool 권한 상승은 이미 정본상 불가능하다. `entity-placement-and-context.md`의 우선순위 사슬(`catalog → Project grant → Agent template(subset only) → Task request → snapshot`)과 "Project deny 또는 capability 부족은 Agent template으로 다시 허용할 수 없다"가 이미 canonical이다. Protocol Specialist가 "핵심 보안 불변식"으로 제시한 상승 차단 정리는 새 제안이 아니라 기존 정본의 재확인이었다.
- **사실 2**: 그러나 `tool-catalog.md`가 "tool binding 변경은 `AgentManage` 권한과 Project 범위 검사를 요구한다"고 이미 정한다. 템플릿의 tool 집합이 Agent tool binding의 출처이므로 무시할 수 없다.
- **사실 3**: `#48`의 차단 조건은 "자동 Agent provisioning을 통한 `AgentCreate` 우회"를 겨냥한다. 템플릿 편집은 Agent를 만들지 않으므로 다른 메커니즘이며, 같은 차단을 적용하면 과잉이고 `#86`이 무기한 지연된다.
- **사실 4**: 남는 위험은 prompt authorship인데, `TaskCreate` 보유자가 이미 같은 종류의 힘을 갖는다(Operator 역할이 기본 보유). 다만 템플릿은 지속적이고 다른 사람의 Task에도 적용된다는 비대칭이 있다.
- **결정 1 — 필드별 게이팅**: `role_prompt`·메타데이터 편집은 `agent_template:update`, `tools`/`skills`/`isolation_class` 편집은 거기에 Agent tool-binding 권한을 추가 요구한다. 정본 충돌 없이 `#86`을 `#48` 승인 없이 진행할 수 있다.
- **결정 2 — Operator는 `read` + `update`**: tool-binding 권한을 주지 않으므로 실질적으로 prompt 편집만 가능하다. `BuiltinRole::Operator`의 고정 목록에 두 항목을 추가해야 하며, 추가하지 않으면 operator는 아무것도 받지 못한다. admin은 `PermissionKind::all()`로 자동 보유하고 `builtin_roles_cover_all_permissions` 테스트가 이를 강제한다.
- `agent-template.md`에 "편집 권한의 필드별 게이팅"과 "기본 역할 배정" 절을 추가하고, `#86`·`#92` 완료 게이트와 검토 문서 §8에 반영했다.

## 2026-08-23 — Issue 기능 프레임 정정과 Agent 착수 경로 결정

- 유형: `design` + `lint`
- H2(dead-letter 자동 Issue 생성의 kind별 기본값)를 검토하다 **더 근본적인 오해**가 드러났다. 조정자가 Issue를 orchestrator의 인프라 장애 추적으로 해석해 서브 에이전트에게 위임했으나, 사용자 의도는 **프로젝트가 해결해야 할 일감을 관리하는 이슈 트래커**였다. 두 전문가는 주어진 프레임 안에서 정확히 작업했고, 프레임 자체가 틀렸다.
- 정정이 바꾸지 않은 것: Issue/Task가 부모-자식이 아닌 연관, Task 성공이 Issue를 닫지 않음, `InProgress` 부재, 교착 없음 불변식 I1·I2, Task/Attempt 상태 머신 불변. 이 경계들은 이슈 트래커에도 그대로 유효하다.
- **H2 조사에서 나온 사실**: 명세의 kind별 기본값 표가 실제 코드와 어긋나 있었다. `reconcile.rs`의 dead-letter 경로는 `CredentialMissing`과 `WorkerUnavailable` 두 kind만 붙이며, `FailureKind`의 `Timeout`·`AuthFailed`·`Cancelled` 세 variant는 **저장소 어디서도 생성되지 않는 죽은 코드**다. 제안된 "15분 hysteresis"도 대부분 중복이었다 — `interval=30s`·`stale_after=60s`·`max_dispatch_retries=20`이라 dead-letter까지 최소 약 11분이 걸린다.
- **`#90`(dead-letter → Issue 자동 생성) 취소**. 인프라 장애는 alert이지 프로젝트 일감이 아니다. 관측 요구(`/metrics`에 kind별 분해 부재)와 죽은 variant 정리는 `#70`이 흡수했다. ID는 참조 안정성을 위해 재사용하지 않는다.
- **`#93`(Agent backlog claim) 신설**. Agent가 Issue를 읽고 착수한다는 결정에 따라 인가·동시성·예산 문제가 새로 들어왔다. **인가는 상태가 소유한다** — `Triaged → ReadyForAgent` 전이는 사람만 하며 신설 `issue:approve_agent_work` capability를 요구하므로, Agent가 스스로 연 Issue를 스스로 착수하려면 반드시 사람의 승인을 거친다. `project-feature-design.md`가 자동 provisioning의 `AgentCreate` 우회를 이유로 건 차단 조건과 같은 종류의 위험이라 같은 방식으로 명시적 승인 지점을 뒀다. claim은 `#62`의 lease 관례를 재사용한 CAS + 만료 lease이며 Issue 상태를 바꾸지 않는다(상태를 더 만들면 `InProgress`를 금지한 것과 같은 문제). Project는 claim 예산을 가지며, 없으면 무한 생성-소비 루프가 성립한다.
- `issues.md`를 이슈 트래커 프레임으로 다시 썼다 — 범위 절 신설, `ReadyForAgent` 상태 추가, Agent 착수 절 신설, `Draining` 중 claim 거절, capability에 `issue:approve_agent_work` 추가.
- **미결**: `docs/roadmap/roadmap.md`와의 소유권 관계. 영구 ID 원장을 트래커가 대체할지, roadmap이 계획 정본으로 남을지는 양쪽을 설계해 비교한 뒤 결정하기로 했다. roadmap.md가 이번 세션에서 동시 편집으로 세 번 회귀한 이력이 있어 실질적 동기가 있으나, 이주 비용과 DB 장애 시 계획 가시성을 함께 봐야 한다.

## 2026-08-23 — Roadmap 원장과 Issue Tracker의 소유권 비교

- 유형: `design`
- [Issue 추적](architecture/issues.md)이 미결로 남긴 "영구 ID 원장을 트래커가 대체하는가"를 [비교 설계](reviews/roadmap-vs-issue-tracker-2026-08-23.md)로 다뤘다.
- **실측**: `roadmap.md`는 194행/41KB에 93개 행(활성 46, 보존 레지스트리 47), 이 파일을 건드린 커밋 73개, 기록된 상태 회귀 3회. 행 길이 중앙값 104자에 **최대 2,506자**이고, 41개 행이 정본을 링크하는 반면 **7개 행은 `이 행`을 정본으로 선언**한다.
- **관측된 문제 3가지**: (1) 동시 편집 회귀 — 세 번 모두 파일 전체를 다시 쓰는 편집 단위가 원인이며 행 단위 갱신이었다면 발생하지 않았다, (2) 감사 부재 — roadmap을 고치는 데 필요한 권한이 파일 쓰기뿐이고 capability 검사도 구조화된 기록도 없다, (3) 행이 설계 문서가 되어간다 — 정책은 "설계 정본을 먼저 수정한 뒤 순서·게이트만 동기화"라고 이미 옳게 정하는데 형식이 그것을 강제하지 못한다.
- **현재 형식의 강점도 기록했다**: 오프라인·git에서 읽히고, diff로 리뷰되며, 절 산문이 순서 근거를 담고, 부트스트랩 문제가 없다.
- 세 모델을 비교했다. **Model A(전면 대체)의 치명적 약점은 DB 장애 시 계획 상실**이다 — Cold Standby 승격이 수동이고(`#63` 미구현) 이번 세션에 "DB를 잃으면 워커 fleet 전체를 수동 재발급해야 한다"는 결함을 발견한 시스템에서, 복구 순서를 담은 계획이 같은 DB에 있으면 안 된다. **Model B(분리)는 관측된 문제를 하나도 풀지 않는다** — 회귀 3회의 원인인 파일 편집이 그대로 남는다.
- **권고: 단계적 Model C** (트래커가 상태를 소유하고 roadmap.md는 생성물). 결정적 논거는 **정책이 이미 Model C를 요구한다**는 것이다 — 갱신 규약이 roadmap 행의 역할을 "포인터와 게이트"로 정한 순간 2,506자 행과 `이 행` 정본 7건은 형식이 정책을 강제하지 못해 생긴 드리프트이며, 정본 링크를 필수 필드로 만들면 구조적으로 막힌다. 생성 파일을 git에 커밋하므로 오프라인 가시성과 diff 리뷰도 유지된다.
- 4단계 이행 순서를 정했다: (1) `#88`·`#93`을 Model B 형태로 먼저 구현해 부트스트랩 공백을 정직하게 인정, (2) 스키마 안정 후 roadmap 행 스키마의 표현 가능성 검증(특히 정본 링크 필수화로 `이 행` 관행 차단), (3) 렌더러 도입과 생성물 전환, (4) 보존 레지스트리(`#1`~`#47`)는 스키마가 다르고 변경되지 않으므로 이주하지 않는다.
- `issues.md`의 미결 절을 결론으로 교체했다 — `#88`·`#93` 범위에서 트래커는 roadmap을 대체하지 않고 실무 단위만 다룬다.

## 2026-08-23 — `#73` HTTP capability 행렬 기본 deny 전환

- 유형: `implementation` + `verification`
- `crates/fleet-api/src/app.rs`의 `authorize_http_endpoint`가 `required_capability`의 `None`을 통과가 아니라 `403`으로 처리하도록 기본값을 뒤집었다. `/health`와 `POST /workers/join`(body의 bootstrap token이 자체 인증 수단)만 함수 안에서 명시적으로 허용한다. `#58`·`#66`에서 두 번 반복된 "행렬 미등록 = 허용" 결함의 근본 원인을 개별 route 추가가 아니라 기본값 전환으로 닫았다.
- 누락돼 있던 두 route를 행렬에 등록했다. `GET /v1/workers/{id}`는 신설 `is_worker_by_id_route` 헬퍼(단일 세그먼트만 매칭, LLM credential 하위 경로와 혼동 없음)로 `worker:list`를 요구하게 했다 — 이전에는 응답 `endpoint` 필드에 담긴 워커의 ACP `server-key`가 인증만 통과하면(워커 자신의 operational credential 포함) 무권한 노출됐다. `POST /hosts/register`는 기존 `host:provision` capability를 그대로 매핑했다 — 이전에는 `upsert_host`의 `ON CONFLICT DO UPDATE`가 기존 Host의 `ssh_host`/`ssh_user`/`status`/`worker_id`를 무권한으로 덮어썼다.
- **구현 중 발견한 회귀**: 기본값을 뒤집자 `allow_no_auth`(개발/테스트 기본값) 경로에서 join 테스트 10건이 403으로 실패했다. `auth_middleware`의 join 우회는 `allow_no_auth == false`일 때만 도달하는 별도 분기라 `allow_no_auth == true`에서는 `authorize_http_endpoint`를 그대로 타는데, 그 안에는 join 예외가 없었다. `POST /workers/join`을 `authorize_http_endpoint` 안에도 명시 허용해 해소했다.
- **테스트 작성 중 발견한 axum 동작**: 완전히 존재하지 않는 경로(`nest()` 트리에 매칭되는 route가 하나도 없는 경우)는 미들웨어 자체를 타지 않고 axum이 직접 404를 반환한다 — `eprintln` 계측으로 `authorize_http_endpoint`가 호출조차 되지 않음을 확인했다. 보안 문제는 아니다(404든 403이든 막힌 것은 막힌 것). 회귀 가드는 실제로 등록된 route가 행렬에서 빠지는 경우를 잡아야 하므로, 통합 테스트를 "존재하지 않는 깊은 경로"에서 "등록된 경로의 잘못된 메서드"(`PUT /workers`)로 교체했다.
- 신규 테스트: `crates/fleet-api/src/app.rs`의 `capability_matrix_covers_router_routes`(router에 실제 등록된 모든 (method, path)가 capability를 가짐을 확인하는 병행 유지 목록 — `openapi_yaml_is_valid_and_covers_known_paths`와 같은 관례), `authorize_http_endpoint_denies_by_default_for_any_unmatched_route`(임의의 미등록 조합이 함수 수준에서 항상 403임을 고정), `get_worker_by_id_and_host_register_now_require_capability`, `is_worker_by_id_route_matches_single_segment_only`. `crates/fleet-api/tests/capability_matrix_default_deny.rs`(8건, 실제 HTTP 스택) — 두 route의 403/200 정확성, 워커 자신의 operational credential로 다른 워커·Host를 건드릴 수 없음, `allow_no_auth` 모드에서도 join이 여전히 동작함.
- 병행 유지 목록의 한계를 문서에 명시했다: `capability_matrix_covers_router_routes`는 `build_app`의 route 목록을 손으로 병행 유지하므로, 새 route를 추가하면서 이 목록에 반영하지 않으면 테스트는 통과하지만 실제로는 놓친다. 진짜 안전장치는 기본값 deny 전환 자체다 — 목록 갱신을 잊어도 새 route는 자동으로 403이지 열려 있지 않다.
- `http-api.md`와 `authorization-and-audit.md`의 "현재 구현" 서술을 코드 기준으로 갱신하고, Dashboard `/api`(중앙 행렬 부재, 핸들러에 29곳 산재)에는 같은 불변식이 아직 없다는 점과 `#92`가 그 범위를 다룬다는 점을 명시했다.
- 검증: `cargo test --workspace`(진행 중, 현재까지 실패 0), `cargo check --no-default-features` 통과, `cargo clippy -p fleet-api --all-targets --all-features` 경고 0.

## 2026-08-23 — `#79` 원격 실행 실패 관측 가능화

- 유형: `implementation` + `verification`
- `crates/fleet-provisioner/src/ssh.rs`의 `RemoteExecutor`에 `exec_checked` 기본 메서드를 추가했다. `exec_streaming`을 위임해 exit code를 얻고, 비0이면 `StepError::RemoteExit`로 승격한다. `exec()` 자체는 그대로 두었다 — `test -f ... && echo yes`처럼 비0 종료가 정상적인 "아니오" 응답인 조회 명령에는 exit code 무시가 필요하기 때문이다.
- `install_fleet_worker.rs`(디렉토리 생성, config/유닛 이동, daemon-reload), `push_credentials.rs`(config.toml atomic write), `install_cloudflared.rs`(자격증명 생성, config.yml 이동, 재시작)의 mutation 명령에서 `let _ =`/`|| true`로 버려지던 실패를 `exec_checked`로 교체했다. cloudflared enable/restart는 `#85`에서 표준 playbook 제거가 예정된 과도기 스텝이라 best-effort로 남기되, 실패를 로그로 관측 가능하게 했다(조용히 삼키지 않음).
- **구현 중 실제 잠복 버그를 발견했다**: `install_cloudflared.rs`에 `/etc/cloudflared` 디렉토리를 만드는 스텝이 없어, 새 호스트에서 `config.yml`의 `sudo mv`가 "디렉토리 없음"으로 조용히 실패하고 있었다(스텝은 "Applied"로 보고됨 — 정확히 이번 항목이 막으려던 사고 유형). `sudo mkdir -p /etc/cloudflared`를 추가해 해소했다.
- `start_services.rs`의 죽은 `wait_timeout_secs`(전 저장소 참조 0건)를 실제 폴링으로 구현했다. 로컬 `systemctl is-active`는 즉시 1회 확인 대신 타임아웃까지 재시도한다. `orchestrator_api_token`이 있으면 `GET /v1/workers`를 폴링해 워커가 `online`으로 보고될 때까지 기다린다 — 토큰이 없으면(하위 호환) 로컬 상태만 확인했다고 경고 로그를 남기고 진행한다.
- 이 하트비트 폴링을 구현하다 **두 번째 관측 결함을 스스로 만들 뻔했다**: 401/403(capability 부족)을 아무 처리 없이 "아직 못 찾음"과 똑같이 취급하면, 남은 시간 내내 폴링하다 결국 "워커가 등록 안 됨" 타임아웃으로 오인시킨다. 상태 코드 분류를 `classify_status` 순수 함수로 분리해 401/403은 즉시 별도 원인으로 실패하도록 정정했다 — `#79`의 취지를 그 안에서 한 번 더 확인한 사례다. `docs/deployment/worker-provisioning.md`에 `worker:list` capability 요구사항을 문서화했다.
- `PlaybookError::StepFailed`에 `completed_steps: Vec<StepReport>`를 추가했다. 이전에는 이 정보가 에러에 실리지 않아 `fleet-cli`의 실패 처리가 매번 `steps: vec![]`로 리포트를 만들었고, 20대 중 7번째가 어느 스텝에서 멈췄는지 실패 리포트만으로는 알 수 없었다. `fleet-cli`에 `recover_completed_steps` 헬퍼(anyhow로 소거된 에러에서 `downcast_ref`로 복원)를 추가해 두 catch 지점(dry-run/실제 SSH 병렬 실행)에 적용했다.
- 신규 테스트 12건: `playbook.rs`의 `completed_steps_includes_earlier_skipped_and_applied_steps`(+ 기존 `stops_on_step_failure` 확장), `start_services.rs`의 `apply_fails_when_daemon_reload_fails`/`apply_continues_when_only_cloudflared_enable_fails`/`apply_without_orchestrator_token_skips_heartbeat_check_but_succeeds`/`classify_status_*`(3종), `runtime.rs`의 `recover_completed_steps_tests`(2종).
- 검증: `cargo test --workspace`(1090 passed), `DATABASE_URL` 주입 `fleet-store`/`fleet-api` 직렬 재실행(226 passed), `cargo check --no-default-features`, `cargo clippy -p fleet-provisioner -p fleet-cli --all-targets --all-features`(경고 0).

## 2026-08-23 — `#80` 최초 admin 토큰 발급 경로

- 유형: `implementation` + `verification`
- `crates/fleet-api/src/app.rs`에 `issue_admin_bootstrap_token_if_needed`를 추가했다. `admin_api_tokens`가 비어 있으면 `principal_id: "bootstrap"`, full capability로 1개 발급하고 원문을 반환한다. `sync_env_admin_tokens_to_store`(로드맵 `#72`)와 나란히 두되 **그 함수 이후에 호출해야 하는 순서 제약**을 문서화했다 — env로 admin을 이미 구성한 배포에 최소 권한을 벗어난 전체-권한 토큰을 추가로 심지 않기 위해서다. 이 함수 자체는 파일을 쓰지 않는다 — store 조작과 원문 반환만 하는 순수 함수로 남겨 테스트가 쉽고, 파일 I/O는 호출자 책임으로 분리했다.
- `fleet-cli`가 `--http-bind` 기동 시 이를 호출하고, 반환된 원문을 `write_admin_bootstrap_token_file`로 `0600` 파일(`/etc/fleet/bootstrap-admin-token`, `FLEET_ADMIN_BOOTSTRAP_TOKEN_FILE`로 오버라이드 가능)에 1회 쓴다. 저널/표준출력에는 원문을 남기지 않는다 — dashboard OTP(`tracing::info!`로 출력)와 의도적으로 다른 경로다: 로그는 대개 영구 보존되므로 admin 토큰 같은 지속 credential에는 부적합하다는 로드맵 초안의 판단을 그대로 반영했다. 파일이 이미 존재하면 덮어쓰지 않고 에러를 반환한다 — `issue_admin_bootstrap_token_if_needed`가 정상 동작하는 한 도달하지 않는 경로지만, 도달하면 "토큰은 DB에 발급됐으나 파일화 실패"를 명확한 로그로 남기고 principal_id `bootstrap` 회수 후 재발급을 안내한다.
- **구현 전 게이트 재확인에서 로드맵 초안의 전제가 틀렸음을 발견했다**: "발급 경로가 다른 토큰과 동일한 `AuditEvent`를 남김"이라는 완료 게이트를 그대로 구현하려 했으나, `create_admin_token` 핸들러 자신도 `AuditEvent`를 남기지 않고 `tracing::info!`만 쓴다는 것을 코드로 확인했다(구조화된 감사 기록은 `#66`의 LLM credential 3개 route에만 존재). 존재하지 않는 관례에 맞추는 대신 같은 관측 수준(`tracing::info!`)으로 구현하고, 광범위한 감사 확장은 `#76`에 그대로 남겼다.
- `crates/fleet-api/src/handlers.rs`의 `generate_random_bytes`/`base64url`을 `pub(crate)`로 바꿔 `app.rs`에서 재사용했다(새 CSPRNG/인코딩 로직을 중복 작성하지 않음).
- 신규 테스트: `crates/fleet-api/tests/admin_bootstrap_token.rs`(4건) — 빈 store에서 1회 발급, 발급된 토큰이 `/v1/workers`와 `/v1/admin/tokens` 양쪽에 실제로 인증됨(전체 capability 확인), 재기동 시뮬레이션(두 번째 호출)에서 재발급 없음, env sync 이후에는 미발급. `crates/fleet-cli/src/runtime.rs`의 `admin_bootstrap_token_file_tests`(3건) — `0600` 권한 확인, 부모 디렉토리 자동 생성, 기존 파일 보존(덮어쓰지 않음). 후자는 env var 전역 상태를 건드리지 않도록 경로 해석(`resolve_admin_bootstrap_token_path`)과 쓰기 로직(`write_admin_bootstrap_token_to_path`)을 분리해 병렬 테스트 레이스를 피했다.
- `docs/security/control-plane-security-model.md`의 "현재 구현과 목표의 차이" 표를 갱신했다 — `#80` 행 추가와 함께, `capability 행렬 커버리지` 행이 `#73` 완료를 반영하지 않고 있던 것도 함께 정정했다(별도 발견).

## 2026-08-23 — `#81` 이기종 fleet 지원 (arch·OS 감지 배선)

- 유형: `implementation` + `verification`
- `crates/fleet-provisioner/src/steps/check_prereqs.rs`에 `detect_prereq`(원격에서 `CheckPrereqs`를 실제로 실행해 `PrereqReport`를 얻음)와 `assumed_prereq_from_labels`(연결 없는 미리보기용, 인벤토리 라벨에서 가정값 구성)를 추가했다. `fleet-cli::run_playbook`과 `fleet-dashboard::run_provisioning` 두 곳에 있던 `PrereqReport` 하드코딩(`ubuntu`/`x86_64` 고정)을 실제 호출로 교체했다 — dry-run은 라벨 기반 가정값, 실 연결은 `detect_prereq`(검증 실패를 조용히 덮지 않고 그대로 전파, `#79`의 관측 가능성 원칙과 동일).
- `Playbook::standard(prereq)`가 이제 `InstallFleetWorker`/`InstallCloudflared`에도 `target_arch`를 주입한다 — 이전에는 `InstallDeps`만 감지된 값을 받았다.
- **구현 중 실제 CLI 실행으로 재현한 회귀**: `CheckPrereqs::apply`는 원래 `ctx.dry_run`과 무관하게 항상 진짜 조회를 시도했다. 인벤토리 dry-run은 아무 것도 프로그래밍되지 않은 `MockExecutor`를 쓰므로 모든 질의가 빈 문자열로 돌아와 "could not detect OS"로 **매번 실패**했다 — `fleet provision --inventory ... --dry-run`을 직접 실행해 확인했다. 이 버그는 `#81`의 하드코딩 문제와 별개로 이미 존재했었고, 로드맵이 스스로 정한 완료 게이트("arm64 호스트 dry-run이 올바른 타깃을 선택하는 테스트")를 직접 가로막고 있었다. `CheckPrereqs::apply`가 `ctx.dry_run`을 확인해 그 경로에서 `assumed_prereq_from_labels`를 쓰도록 고쳐 해소했다 — 실 연결(`dry_run == false`) 검증 로직은 그대로 유지했다(`non_dry_run_against_bare_mock_executor_still_fails_validation` 테스트로 고정).
- `InstallFleetWorker`에 `target_arch`와 `StepContext.fleet_worker_bin_by_arch`(아키텍처별 로컬 바이너리 경로 맵)를 추가했다. 선택 우선순위는 명시 오버라이드 → 아키텍처 매칭 → 아키텍처 무관 단일 폴백(기존 단일 아키텍처 배포와 완전 하위 호환). dry-run 미리보기 메시지도 실제 배포와 같은 선택 로직(`resolve_local_bin`으로 통합)을 쓰도록 고쳤다 — 처음에는 dry-run 메시지가 이 로직을 타지 않아 미리보기가 실제 배포와 다른 결과를 보여주는 걸 뒤늦게 발견했다. **이 맵을 CLI/인벤토리 YAML에서 채우는 배선은 `#83` 범위로 명시적으로 남겼다** — `#81`은 스텝이 아키텍처를 인식하는 메커니즘까지, 실제 데이터 소스 연결은 `#83`의 일.
- `InstallCloudflared`에 `target_arch`와 `cloudflared_arch_suffix`(`uname -m`의 `aarch64`/`x86_64`를 cloudflared 자산 명명 `arm64`/`amd64`로 변환)를 추가했다. 이전에는 `cloudflared-linux-amd64`가 고정돼 있어 arm64 호스트에서 잘못된 아키텍처 바이너리를 받았다. `#85`에서 이 스텝이 표준 playbook에서 제거될 예정이라, 미인식 아키텍처는 하드 실패 대신 경고 로그 후 amd64로 폴백하는 선에서 마무리했다.
- `crates/fleet-dashboard/src/provisioning.rs`에도 같은 하드코딩이 독립적으로 존재했다 — 대시보드에서 트리거하는 프로비저닝 흐름이 CLI와 별개로 같은 버그를 복제하고 있었다. `detect_prereq(&ssh)`로 교체하고 실패를 `ApiError::Unavailable`로 변환했다.
- 신규/변경 테스트: `check_prereqs.rs`(11건, dry-run 라벨 기반 가정값·비dry-run fail-closed 유지·detect_prereq 실패 전파 포함), `install_fleet_worker.rs`(11건, 아키텍처 매칭·폴백·명시 오버라이드 우선순위·arch 명시 에러 메시지·dry-run 메시지 일치), `install_cloudflared.rs`(10건, arch suffix 매핑·미인식 폴백·다운로드 URL 아키텍처 선택), `playbook.rs`(구성 안정성 확인). `fleet provision --inventory ... --dry-run`을 arm64 라벨 인벤토리로 직접 실행해 회귀 해소를 재확인했다.
- `docs/deployment/worker-provisioning.md`에 `fleet_worker_bin_by_arch` 사용법을 추가했다. 부수적으로 같은 파일의 기존 서술이 `#82`(provision→join 배선)를 가리켜야 할 곳에서 `#81`을 잘못 링크하고 있던 것을 발견해 함께 정정했다.
- 검증 진행 중: `cargo test -p fleet-provisioner -p fleet-cli -p fleet-dashboard`(120+34+27+17+5+5+1 = 209 passed), `cargo check --no-default-features` 통과, `cargo clippy -p fleet-provisioner -p fleet-cli -p fleet-dashboard --all-targets --all-features`(초기 `unused import: PrereqReport` 1건 발견·제거 후 경고 0). 전체 워크스페이스 회귀와 PostgreSQL 검증은 진행 중.

## 2026-08-23 — `#82` provision→join 배선

- 유형: `implementation` + `verification`
- `crates/fleet-provisioner/src/ssh.rs`의 `RemoteExecutor`에 `exec_with_stdin`을 신설했다. `channel.exec()` 직후 `stdin_data`를 채널에 쓰고 `channel.eof()`로 마감한 뒤 stdout/stderr(합침)와 exit code를 반환한다 — bootstrap token처럼 명령행 인자·환경변수·디스크 파일 어디에도 남기고 싶지 않은 값을 원격 프로세스의 표준 입력으로 직접 전달하기 위한 용도다. `SshClient`(russh, `channel.data`/`channel.eof`/`channel.wait` 조합)와 `MockExecutor`(신규 `stdin_writes` 기록 필드 + `recorded_stdin_writes()`) 양쪽에 구현했다.
- `crates/fleet-provisioner/src/steps/join_worker.rs`에 `JoinWorker` 스텝을 신설하고 `Playbook::standard`의 `InstallFleetWorker`와 `PushCredentials` 사이에 배선했다(표준 playbook 7단계 → 8단계). 동작: (1) `ctx.orchestrator_api_token`으로 `POST /v1/bootstrap-tokens`를 호출해 `max_uses: 1`, TTL 300초의 워커 전용 1회용 토큰을 발급받는다(`token:issue` capability 필요), (2) 원격에서 `sudo fleet-worker join --token-file - --orchestrator-url … --name … --config-out /etc/fleet/worker.toml`을 `exec_with_stdin`으로 실행하며 토큰은 stdin으로만 흘려보낸다(커맨드라인에는 등장하지 않음 — `join_command_never_contains_the_token` 테스트로 고정), (3) 성공하면 `chmod 600`으로 권한만 보정한다. `is_applied`는 원격 `/etc/fleet/worker.toml`에 `operational_token` 필드가 이미 있는지 `grep`으로 확인한다.
- 이로써 worker.toml 렌더링 경로가 하나로 합쳐졌다 — 원격 호스트 자신이 실행하는 `fleet-worker join`이 오케스트레이터의 `POST /v1/workers/join` 응답(`fleet-api::handlers::render_worker_config_toml`)을 그대로 디스크에 쓴다. 프로비저너가 로컬에서 중복 렌더링하던 `templates.rs::render_worker_config`(legacy `[worker] bootstrap_token` 필드를 방출했고, `fleet-worker`가 이를 fail-closed로 거부해 기동 불가능한 워커를 만들고 있었다)를 완전히 삭제했다. `TemplateContext`는 cloudflared config.yml 전용 3필드(`tunnel_name`/`hostname`/`credentials_path`)로 축소됐다.
- `bootstrap_token` 필드·플래그를 배선 전체에서 제거했다 — `StepContext`(`steps.rs`), `ProvisionOptions`(`inventory.rs`), `--bootstrap-token`/`ProvisionArgs`(`fleet-cli`의 `main.rs`/`runtime.rs`), `ProvisionRequest`(`fleet-dashboard`의 `provisioning.rs`). 대시보드 프로비저닝 폼(`provision.html`/`provision.js`)의 "Bootstrap Token" 입력은 삭제하고 그 자리에 실제로 소비되는 "Orchestrator API Token"(`api_token`) 입력을 추가했다 — 발견: 이 폼에는 애초에 `api_token` 입력이 없어 기존 `PushCredentials` 스텝도 이미 조용히 실패하고 있었다(대시보드 트리거 프로비저닝에서 credential push가 한 번도 성공한 적 없었다는 뜻). `JoinWorker` 추가로 실패 지점이 앞당겨지므로 방치할 수 없어 함께 고쳤다.
- **로드맵 초안의 게이트와 다르게 구현한 지점**: 초안은 `is_applied`가 `existing_worker_id`로 `GET /v1/workers/{id}`를 호출해 원격 신원을 검사해야 한다고 요구했다. 로컬 `operational_token` 필드 존재 여부만 grep하는 더 단순한 방식으로 구현했다 — 그 필드는 join 성공 시에만 원자적으로 기록되므로 "이전에 성공적으로 join했다"를 판별하는 데 충분하고, idempotency 검사만을 위해 `worker:read`류의 새 capability를 추가로 요구하지 않아도 된다. `/v1/workers/join`은 동일 이름에 대해 **재등록을 지원하지 않고 항상 `409 Conflict`로 거부**한다(`register`와 다른 실제 동작) — `JoinWorker`는 이 경우를 조용히 성공 처리하거나 기존 워커를 지우지 않고, 출력에 `409`가 있으면 운영자가 판단할 수 있는 안내 문구를 덧붙여 명확한 에러로 전파한다(`agent.md`의 "실패 보상은 항상 전진, 워커 삭제로 후퇴하지 않는다" 원칙).
- worker_name/orchestrator_url/labels는 인벤토리 YAML에서 올 수 있는 신뢰할 수 없는 입력이므로 원격 커맨드라인에 보간하기 전 항상 POSIX 작은따옴표로 quote한다(`shell_quote`) — 이 경로가 원격 셸 명령 문자열에 변수를 직접 보간하는 이 크레이트의 첫 사례라 새로 추가했다(`join_command_quotes_untrusted_worker_name` 테스트로 고정).
- 신규 테스트: `join_worker.rs` 19건(prereq 검증 3, dry-run, is_applied 2, 커맨드 구성·quoting 3, 409 판별 2, HTTP 발급을 분리한 `perform_join`의 stdin-only 토큰 전달·chmod·에러 매핑 3건 포함). `install_fleet_worker.rs`의 기존 테스트를 worker.toml 미작성 확인으로 갱신(`apply_uploads_binary_and_writes_config` → `apply_uploads_binary_and_installs_unit`, `apply_fails_without_grok_secret` 삭제 — 그 요구사항은 이제 `JoinWorker`가 원격에서 처리). `playbook.rs`/`tests/playbook_dry_run.rs`의 7단계 순서 고정 테스트를 8단계로 갱신.
- 검증: `cargo test --workspace`(전체 통과, 실패 0), `DATABASE_URL` 주입 `fleet-store`/`fleet-api` 직렬 재실행(전체 통과), `cargo check --no-default-features` 통과, `cargo clippy --all-targets --all-features`(초기 `clippy::needless_update` 1건 — `install_cloudflared.rs`가 3필드로 줄어든 `TemplateContext`에 여전히 `..Default::default()`를 남겨 발생 — 발견·제거 후 경고 0).
- `docs/deployment/worker-provisioning.md`(implementation: partial → complete, 시퀀스 다이어그램과 capability 요구사항 갱신)와 `docs/contracts/worker-enrollment.md`("남은 노출 경계"의 provisioner 서술을 legacy bootstrap_token 상태에서 실제 join 배선으로 정정)를 갱신했다.

## 2026-08-23 — `#83` 인벤토리 모드 완결성

- 유형: `implementation` + `verification`
- `crates/fleet-cli/src/runtime.rs::build_inventory_step_context`의 `fleet_worker_bin: None` 하드코딩을 제거했다. `crates/fleet-provisioner/src/inventory.rs`에 `InventoryDefaults.fleet_worker_bin`(단일 폴백)/`fleet_worker_bin_by_arch`(아키텍처별 맵 — `#81`이 메커니즘만 만들고 실제 배선은 이번에 채웠다)와 `InventoryWorker.fleet_worker_bin`(워커별 오버라이드, `effective_fleet_worker_bin`로 defaults보다 우선) 필드를 추가했다.
- `grok_secret`은 "선택 필드 유지 vs 완전 제거"를 유지로 결정했다 — 타입은 이미 `Option<String>`이었으므로 스키마 변경은 필요 없었고, `#82`가 미지정 시 원격 `fleet-worker join`이 무작위 생성하도록 이미 바꿔 놨으므로 필드 자체를 없앨 이유가 사라졌다. 오래된 "미설정 시 프로비저닝 단계에서 실패함 — caller가 반드시 채워야 함" doc comment(더 이상 사실이 아님)를 정정했다.
- `run_provision_inventory`에 `api_token` 필수 검증을 추가했다 — dry-run이 아닐 때만 검사하고(`JoinWorker` 자신도 dry-run이면 이 값을 보지 않아 대칭적이다), CLI `--api-token`이 인벤토리 `options.api_token`을 오버라이드하는 기존 병합 **이후**에 검사한다(`Inventory::validate()`에 두면 비밀을 YAML에 커밋하지 않고 `--api-token`으로 주입하는 기존 운영 패턴을 깨뜨린다). 이전에는 이 누락이 실제 SSH로 `CheckPrereqs`~`InstallFleetWorker`까지 실행한 뒤 `JoinWorker`에서야 드러났다 — 인벤토리에 20여 대가 있으면 매 호스트마다 이 낭비가 반복됐다.
- 선언만 있고 참조 0건이던 `ProvisionOptions.retry_failed`는 구현 대신 제거를 택했다 — `run_provision_inventory`(dry-run/실제 SSH 두 branch 모두)를 직접 읽어 실제 재시도 로직이 전혀 없음을 확인했고, 지금 새로 설계하는 것은 이 항목의 게이트("커밋된 25노드 인벤토리 dry-run 완주")를 벗어난다. `examples/workers.yaml`과 `config/inventory-from-ssh.yaml`의 `retry_failed: true`도 함께 정리했다 — `deny_unknown_fields`를 쓰지 않으므로 필드 제거 후에도 남아 있는 YAML은 계속 파싱되지만(`unknown_legacy_retry_failed_field_is_ignored_not_rejected` 테스트로 고정), 죽은 설정을 계속 노출하는 것 자체가 오해를 낳으므로 정리했다.
- `crates/fleet-dashboard/src/provisioning.rs`의 오래된 주석이 "이 호출부의 배선은 `#83` 범위"라고 잘못 약속하고 있던 것을 발견해 정정했다 — `ProvisionRequest`는 YAML 인벤토리가 아닌 단건 JSON 요청이라 "인벤토리 모드 완결성"의 범위 밖이다. `#83`은 `fleet provision --inventory`만 다뤘다.
- **게이트 검증**: `./target/debug/fleet provision --inventory config/inventory-from-ssh.yaml --dry-run`을 실제로 실행해 커밋된 25노드 전원이 신설 8단계(`#82`의 `join_worker` 포함)를 전부 완주함을 확인했다(`25 succeeded, 0 failed`). `api_token` 검증도 별도로 확인했다 — 임시 1노드 인벤토리(스크래치 디렉터리, 커밋 안 함)로 `--api-token` 없이 비dry-run 실행하면 SSH 연결을 하나도 시도하기 전에 즉시 실패하고, `--api-token`을 주면 그 검사를 통과해 (예상대로) 존재하지 않는 SSH 키 파일에서 실패한다 — 검증이 정확히 의도한 지점에서만 막는지 확인했다.
- 신규 테스트: `inventory.rs` 6건 — `fleet_worker_bin_by_arch_defaults_to_empty_map`, `parses_fleet_worker_bin_by_arch_shared_map`, `effective_fleet_worker_bin_prefers_worker_override_over_defaults`, `effective_fleet_worker_bin_none_when_neither_set`, `unknown_legacy_retry_failed_field_is_ignored_not_rejected`(하위 호환 고정).
- 검증: `cargo test --workspace`(전체 통과), `DATABASE_URL` 주입 `fleet-store`/`fleet-api` 직렬 재실행(전체 통과), `cargo check --no-default-features` 통과, `cargo clippy --all-targets --all-features` 경고 0.
- `docs/deployment/worker-provisioning.md`의 "사전 조건" 절을 갱신했다 — 인벤토리 모드가 이제 아키텍처별 바이너리 맵을 실제로 지원한다는 점(단일 호스트 CLI 모드는 여전히 단일 경로만 지원), api_token 사전 검증이 실 프로비저닝에서 즉시 실패를 만든다는 점을 명시했다.

## 2026-08-23 — `#84` 원격 파일 전송 방식 교체

- 유형: `implementation` + `verification`
- `crates/fleet-provisioner/src/ssh.rs`의 `SshClient::upload_file`/`write_file`이 base64를 단일 셸 명령행에 보간하던 것(`echo '<b64>' | base64 -d > path && chmod ...`)을 제거하고, `#82`가 만든 `exec_with_stdin`(SSH 채널 데이터 메시지로 청크 전송)으로 교체했다. russh 벤더링 소스(`Channel::data` → `send_data` → `tokio::io::copy`를 `ChannelTx`에)를 확인해 `max_packet_size`/`window_size`를 라이브러리가 알아서 지킨다는 것을 확인한 뒤, SFTP 서브시스템을 새로 얹는 대신 이미 검증된 이 primitive를 재사용하기로 했다.
- 파일 생성 명령 자체를 `umask 077 && cat > path && chmod {mode}`로 바꿨다 — 이전에는 `> path`가 SSH 세션 사용자의 기본 umask(보통 0644)로 파일을 먼저 만들고 그 뒤에야 `chmod`가 따라붙어, 그 사이 비밀이 담긴 파일이 world-readable 상태로 잠깐 존재했다. 지금은 생성 시점부터 0600이고 필요하면 그 다음에만 목표 mode로 넓힌다(좁혔다 넓히는 순서라 창이 없다). `write_file`은 여전히 `/tmp` 스테이징 파일에 쓰고 호출자가 별도로 `sudo mv`+`sudo chmod`하는 기존 흐름은 그대로다 — 이번 변경은 그 스테이징 파일 자체가 잠깐이라도 world-readable이 되는 문제만 없앴다.
- 원격 경로 문자열은 `steps/join_worker.rs`의 `shell_quote`와 같은 로직을 인라인 중복했다(`push_credentials.rs::urlencode`와 같은 기존 관례 — 의존성 추가 방지).
- `MockExecutor`는 애초에 셸을 거치지 않는 synthetic 구현이라 변경이 필요 없었다 — 기존 테스트가 매칭하는 합성 호출 문자열(`"write /tmp/..."`, `"upload ... → ..."`)도 그대로 유지된다.
- `base64` 크레이트 의존성을 `fleet-provisioner`에서 완전히 제거했다(다른 사용처 0건을 grep으로 확인 후) — `Cargo.toml`/`Cargo.lock` 갱신.
- **게이트 검증을 실제 SSH로 수행했다**: 로컬 macOS `sshd`를 임시 설정(ephemeral ed25519 host key + client key, `127.0.0.1:2222`, 스크래치 디렉터리에서 생성·검증 후 완전 삭제, 커밋 안 함)으로 띄우고, 실제 `SshClient::connect`로 (1) 5MB 랜덤 바이너리 `upload_file`이 성공하고 로컬·원격 바이트가 완전히 동일함(`cmp`로 확인)을, (2) `write_file`로 쓴 비밀 콘텐츠가 생성 직후 `stat`으로 확인한 결과 정확히 `0600`(world-readable 아님)임을 직접 확인했다. 검증에 쓴 임시 예제 바이너리(`crates/fleet-provisioner/examples/verify_84.rs`)는 검증 후 삭제했다 — 커밋된 코드에는 남지 않는다.
- **구현 중 발견**: `--no-default-features`(ssh feature 꺼짐) crate 단독 clippy에서 신설 `shell_quote`가 `dead_code`로 잡혔다 — 실제 사용처(`upload_file`/`write_file`)가 전부 `russh_impl` 모듈(ssh feature 전용) 안에 있기 때문이다. 워크스페이스 전체 `cargo check --no-default-features`(agent.md가 요구하는 게이트)는 feature unification 때문에 이 경고를 가리고 있었다 — `fleet-cli`/`fleet-dashboard`가 다른 경로로 `ssh` feature를 이미 켜고 있어서다. `shell_quote`와 그 단위 테스트 3건을 `#[cfg(feature = "ssh")]`로 게이팅해 크레이트 단독 빌드에서도 경고 0이 되도록 정정했다.
- 신규 테스트: `ssh.rs`의 `shell_quote_wraps_plain_paths`/`shell_quote_escapes_embedded_single_quotes`/`shell_quote_neutralizes_shell_metacharacters`(3건, `ssh` feature 게이팅).
- `RemoteExecutor` 트레이트의 `upload_file`/`write_file`/`exec_with_stdin` doc comment가 서로를 가리키며 "write_file은 base64를 명령행에 보간하므로"라고 설명하던 부분이 이번 변경으로 사실이 아니게 돼 함께 정정했다 — `exec_with_stdin`이 별도로 존재하는 진짜 이유(고정된 "이 바이트를 이 경로에 써라" 의미가 아니라 호출자가 임의 명령을 지정하는 범용 primitive라는 점)를 명확히 했다.
- 검증: `cargo test --workspace`(전체 통과), `DATABASE_URL` 주입 `fleet-store`/`fleet-api` 직렬 재실행(전체 통과), `cargo check --no-default-features` 통과, `cargo clippy -p fleet-provisioner --no-default-features`(경고 0, 위 dead_code 수정 후), `cargo clippy --all-targets --all-features`(경고 0).

## 2026-08-23 — `#85` mTLS 자산 배포와 SAN 일관성

- 유형: `implementation` + `verification`
- 표준 playbook을 8→9단계로 확장했다: `IssueMtlsAssets`(신설, `InstallFleetWorker` 뒤·`JoinWorker` 앞)와 `ConfigureMtls`(신설, `JoinWorker` 뒤)를 추가하고 `InstallCloudflared`를 제거했다(타입은 남겨 커스텀 playbook에서 여전히 구성 가능 — `Playbook::new(vec![...])`).
- **인증서 발급**: `fleet-cli/src/mtls.rs`의 `run_issue_server`가 이미 `pub fn(...) -> Result<()>`로 CLI 파싱과 분리돼 있어 리팩터링 없이 재사용했다 — `fleet-provisioner`나 orchestrator API는 rcgen/서명 로직을 전혀 모른다. `run_provision_single`/`run_provision_inventory`(둘 다 `fleet-cli/src/runtime.rs`)가 mTLS 워커마다 `tempfile::TempDir`에 서버 인증서를 발급한다(SAN=`mtls_advertised_host`, 미지정 시 워커 이름). `mtls` feature가 꺼진 빌드에서는 `issue_local_mtls_assets`가 명확한 에러를 반환한다(`#[cfg(feature = "mtls")]`/`#[cfg(not(...))]` 분기, `build_acp_transport`의 기존 패턴을 그대로 따름).
- **`IssueMtlsAssets`**: 로컬 3파일(cert/key/ca)을 `/tmp` 스테이징 후 `sudo mv`로 원격 `/etc/fleet/mtls/{server.pem,server.key,ca.pem}`(고정 경로 — 설정 가능하게 만들면 `ConfigureMtls`가 worker.toml에 채우는 경로와 어긋날 여지가 생긴다)에 옮기고 권한을 보정한다(cert/ca `0644`, key `0600`, `root:root`). `#84`의 `upload_file`을 직접 쓰지 않은 이유: `upload_file`은 SSH 로그인 사용자 권한으로 곧바로 목적지에 쓰는데, `/etc/fleet/mtls`는 `sudo mkdir -p`로 root 소유가 되므로 비root SSH 사용자가 직접 쓸 수 없다 — 이 크레이트의 다른 특권 파일 쓰기가 전부 쓰는 스테이징+sudo mv 패턴을 그대로 따랐다.
- **`JoinWorker` mTLS 확장**: mTLS 활성화 시 `grok_secret` 미지정이면 여기서 32바이트 무작위 hex를 직접 생성해 `--grok-secret`과 `--agent-endpoint wss://{advertised_host}:{advertised_port}/ws?server-key={secret}` 양쪽에 동일하게 넘긴다. 그러지 않으면(즉 `--agent-endpoint` 없이 `fleet-worker join`에 맡기면) `derive_agent_endpoint`가 리버스 SSH 터널 시절의 `{scheme}://{host}/ws/{name}` 형태로 자동 유도해, `topology.md`가 지원 대상에서 제외한 토폴로지를 광고하게 된다.
- **`ConfigureMtls`**: 구현 전 조사로 `fleet-api`의 `JoinRequest`/`JoinResponse`/`render_worker_config_toml`에 mTLS 관련 필드가 정말 0건임을 grep으로 재확인했다 — 즉 join 응답 경로로는 애초에 `[mtls]`를 렌더링할 방법이 없다. 오케스트레이터 스키마를 확장하는 대신, `push_credentials.rs`가 `/root/.grok/config.toml`을 다루는 것과 같은 원격 read-append-atomic write 패턴으로 `JoinWorker`가 만든 worker.toml에 `[mtls]`를 직접 덧붙인다. `[worker]` 섹션 존재를 sanity check해 `--tags mtls`로 `JoinWorker` 없이 이 스텝만 단독 실행되는 경로(태그 필터링이 실제로 만들 수 있는 시나리오)를 조용히 깨진 파일로 만들지 않는다.
- **SAN 일관성**: `advertised_host`가 인증서 SAN과 항상 같은 값이 되는 것은 별도 검증 로직이 아니라, `IssueMtlsAssets`(발급)와 `ConfigureMtls`(worker.toml 기록) 둘 다 `ctx.mtls_advertised_host` 하나만을 출처로 쓰는 구조로 보장된다 — 두 값이 서로 다른 필드에서 나올 수 있는 경로 자체가 없다.
- **인벤토리 스키마 단순화**: `InventoryDefaults.mtls_client_ca`, `InventoryWorker.mtls_server_cert`/`mtls_server_key`(전부 참조 0건이던 "미리 발급해 경로만 채운다" 옛 모델)를 제거하고 `ProvisionOptions.mtls_ca_dir`(fleet 전체가 공유하는 로컬 CA 디렉토리 — `api_token`과 같은 options-level 패턴, CLI `--mtls-ca-dir`로 오버라이드 가능)로 대체했다. 단일 호스트 CLI의 `--mtls-server-cert`/`--mtls-server-key`/`--mtls-client-ca`도 같은 이유로 제거하고 `--mtls-ca-dir`로 통합했다.
- **`InstallCloudflared` 제거**: `topology.md`가 이미 Cloudflare Tunnel을 지원 대상에서 제외했고, 조사 중 `sudo systemctl enable cloudflared 2>&1 || true`가 설치 실패를 무조건 exit 0으로 만들어 뒤이은 `if install_code != 0 { warn!(...) }` 검사를 실질적으로 죽은 코드로 만드는 것도 재확인했다(이전 감사에서 이미 알려진 결함, 이번에 실제 제거로 정리).
- **게이트 검증(실측)**:
  1. **인증서 없이 mTLS 활성화 → 명확한 실패**: 단일 호스트/인벤토리 모드 둘 다 실제 CLI로 실행해, `mtls_ca_dir` 없이 `mtls_enabled: true`이면 SSH 연결을 하나도 시도하기 전에 즉시 에러로 종료됨을 확인했다.
  2. **SAN 불일치 감지**: `fleet mtls init-ca`/`issue-server`/`issue-client`(실제 CLI, 스텁 아님)로 인증서를 발급하고, `fleet-transport`의 실제 `ServerTlsConfig`/`ClientTlsConfig`/`MtlsProxy`(`runner.rs`/`acp_transport.rs`가 프로덕션에서 쓰는 바로 그 타입)로 로드해 진짜 TCP+TLS handshake를 실행하는 임시 예제(`crates/fleet-transport/examples/verify_85_*.rs`, 검증 후 삭제·미커밋)를 작성했다. SNI를 인증서 SAN과 다르게 주면 `rustls`가 `InvalidCertificate(NotValidForNameContext { expected: ..., presented: [...] })`로 정확히 거부함을 확인했다 — SAN 일관성은 애플리케이션 코드가 아니라 TLS 자체의 고유 동작으로 보장된다.
  3. **정상 경로 성공**: 같은 임시 예제로 올바른 SAN(SNI="localhost", 발급 시 `--dns localhost`)을 쓰면 실제 handshake가 성공하고, `MtlsProxy` 경유로 페이로드가 정확히 왕복함을 확인했다(`MTLS_HANDSHAKE_OK`). ACP JSON-RPC 레이어 자체는 `fleet-transport/tests/acp_transport_mtls.rs`가 이미 별도로 검증하므로 여기서 재검증하지 않았다 — 이번 검증의 목적은 "내가 새로 배선한 발급·업로드·설정 기록 결과물이 기존 mTLS 런타임과 아무 수정 없이 맞물리는가"였다.
  4. **회귀 없음**: 커밋된 25노드 인벤토리(`config/inventory-from-ssh.yaml`, 전원 mTLS 비활성)로 `--dry-run`을 재실행해 신설 9단계 전체가 여전히 완주함을 확인했다(`25 succeeded, 0 failed`).
- 신규 테스트: `steps/issue_mtls_assets.rs` 6건(비활성 no-op, dry-run, 로컬 경로 누락 에러, 업로드 커맨드 구성, is_applied 2종), `steps/configure_mtls.rs` 9건(비활성/dry-run no-op, `[worker]` 누락 sanity check, 섹션 append, advertised_host/port 기본값 폴백, is_applied 2종), `steps/join_worker.rs`에 mTLS 관련 7건(agent-endpoint 구성·폴백, grok_secret 해석 3종) 추가, `inventory.rs` 스키마 갱신 관련 2건.
- 문서: `docs/deployment/topology.md`(mTLS 배포 결손이 이제 닫혔음을 반영, 잘못된 `#84` 귀속을 정정), `docs/deployment/worker-provisioning.md`("mTLS 프로비저닝" 절 신설 — 사전 조건·각 스텝 동작·실패 모드), `examples/workers.yaml`(옛 필드 예시 정리, `mtls_ca_dir` 예시 추가).
- 검증: `cargo test --workspace`(전체 통과), `DATABASE_URL` 주입 `fleet-store`/`fleet-api` 직렬 재실행(전체 통과), `cargo check --no-default-features` 통과, `cargo clippy --all-targets --all-features` 경고 0(초기 `needless_borrows_for_generic_args` 5건 — 신설 스텝 테스트의 `&format!(...)` — `cargo clippy --fix`로 정정), `cargo clippy -p fleet-provisioner -p fleet-cli --no-default-features`(mtls feature 꺼진 빌드) 경고 0(초기 `shell_quote` 스타일과 같은 이유로 `run_provision_inventory`의 `Path` import가 dead_code였던 것을 `#[cfg(feature = "mtls")]`로 정정).

이로써 `#77`~`#85`까지 무인 부트스트랩 시리즈(2026-08-22 [무인 부트스트랩 검토](reviews/bootstrap-automation-review-2026-08-22.md)에서 도출)가 전부 완료됐다.

## 2026-08-23 — `#74` Cloudflare principal capability 매핑 fail-closed

- 유형: `implementation` + `verification`
- 로드맵의 유일한 P0 항목. `crates/fleet-api/src/app.rs::cf_access_capabilities`가 매핑 부재(`state.cf_principal_capabilities == None`) 시 `PermissionKind::all()`을 반환하던 것을 빈 `Vec`으로 고쳤다 — 매핑이 설정된 경우의 fail-closed 동작(열거되지 않은 이메일은 빈 capability)은 이미 정상이었지만, 매핑을 아예 설정하지 않은 배포는 CF Access application 정책만 통과하면 principal이 누구든 전체 capability(`worker:llm_credential:export`, `admin_token:manage`, `token:issue`, `worker:delete` 포함)를 받는 실제 결함이었다 — `docs/security/authorization-and-audit.md`/`control-plane-security-model.md`가 이미 정확히 이 문제를 문서화해 두고 있었다.
- 구현 전 조사로 `with_cf_principal_capabilities`(매핑을 설정하는 유일한 빌더)의 호출부가 `crates/fleet-api/tests/`와 `app.rs`의 `#[cfg(test)]` 안에만 있고 `fleet-cli`에는 전혀 없음을 재확인했다 — 즉 운영 배포에는 이 fail-open을 끌 방법 자체가 없었다.
- `fleet-cli`에 매핑 설정 경로를 신설했다: `Command::Serve`에 `--cf-principal-capabilities`/`FLEET_CF_PRINCIPAL_CAPABILITIES`(JSON 배열, `[{"email":...,"capabilities":[...]}]`)를 추가하고, `runtime.rs`에 `parse_cf_principal_capabilities`(`ApiTokenCredential`/`FLEET_API_TOKENS`의 `parse_scoped_api_tokens`와 같은 파싱·검증 스타일 — 빈 배열·빈 email·빈 capabilities 거부)를 구현했다. CF 세션은 이미 검증된 JWT의 `email` 클레임으로 식별되므로 `ApiTokenCredential`과 달리 별도 `token`(bearer secret) 필드가 없다 — 전용 `CfPrincipalCapabilityEntry` 구조체로 분리했다.
- `run_serve`에 `FLEET_CF_AUDIENCE`는 설정됐는데 매핑이 비어 있으면 거부하는 검사를 추가했다 — 기존 "non-loopback 무인증 bind 거부"(`runtime.rs:420-424`)와 같은 원칙: 안전하지 않거나 무의미한 조합으로 조용히 기동하지 않는다. 매핑 없이 기동하면 이제 서버 자체가 뜨지 않으므로, `cf_access_capabilities`의 `None` 분기(빈 `Vec`)는 정상 배포에서는 도달하지 않는 방어적 이중 안전장치가 됐다.
- **기존 fail-open 테스트 정정**: `cf_capabilities_default_to_all_when_unmapped_deployment`(`app.rs` 단위 테스트)와 `cf_access_without_capability_mapping_keeps_full_access`(`cloudflare_access.rs` 통합 테스트)는 fail-open을 직접 assert하고 있었다 — fail-closed를 검증하도록 뒤집었다(`/health`는 인증 예외 경로라 capability 없이도 여전히 200임을 함께 고정). `cf_access_accepts_valid_jwt`/`cf_access_case_insensitive_header`도 매핑 없이 `/v1/workers` 200을 기대하고 있어 우연히 fail-open에 편승하고 있었던 것을 발견했다 — 각각 목적(JWT 수용 여부, 헤더 대소문자 처리)과 무관한 capability 게이트를 분리하기 위해 명시적 매핑(`spawn_with_cf_capabilities`)을 쓰도록 고쳤다.
- **게이트 검증(실측)**: 실제 빌드한 `fleet` 바이너리로 (1) `DATABASE_URL`+`FLEET_HTTP_BIND`+`FLEET_CF_AUDIENCE`만 주고 매핑 없이 `fleet serve`를 실행하면 어떤 SSH/HTTP 요청 처리도 시작하기 전에 정확한 에러 메시지와 함께 즉시 종료됨을 확인했다(`exit: 1`). (2) `FLEET_CF_PRINCIPAL_CAPABILITIES`를 함께 주면 `"HTTP API server with Cloudflare Access auth (fail-closed capability mapping)"` 로그와 함께 정상 기동해 `principals=1`이 찍힘을 확인했다 — 이후 HTTP 요청 왕복 자체는 `fleet serve`가 부트스트랩하는 MCP stdio 서브시스템이 `FLEET_MCP_CAPABILITIES` 없이는 stdin EOF에서 프로세스 전체를 종료시키는(내 변경과 무관한 기존 동작) 제약 때문에 서브프로세스로는 실측을 마무리하지 못했다 — 대신 `cloudflare_access.rs` 통합 테스트(같은 `AppState`/`auth_middleware`/`authorize_http_endpoint`/`cf_access_capabilities` 코드 경로를 `axum::serve`로 직접 구동)로 매핑 없는 principal의 403/매핑된 principal의 200/`/health` 200을 확인했다.
- 문서: `docs/security/authorization-and-audit.md`("매핑되지 않은 principal의 capability" 절을 완료로 갱신, 존재하지 않는 권한명 `credential:break_glass_export`를 실제 이름 `worker:llm_credential:export`로 정정), `docs/security/control-plane-security-model.md`(상태 표의 "Cloudflare 전용 배포의 권한" 행을 완료로 갱신), `docs/credentials/registry.md`(`FLEET_CF_PRINCIPAL_CAPABILITIES` 행 추가), `examples/fleet.env`(새 env var 예시 추가).
- 신규/변경 테스트: `crates/fleet-api/src/app.rs` 단위 1건 반전, `crates/fleet-api/tests/cloudflare_access.rs` 1건 반전 + 2건 명시 매핑으로 전환, `crates/fleet-cli/src/runtime.rs::cf_principal_capabilities_tests` 8건 신설.
- 검증: `cargo test --workspace`(전체 통과), `DATABASE_URL` 주입 `fleet-store`/`fleet-api` 직렬 재실행(전체 통과), `cargo check --no-default-features` 통과, `cargo clippy --all-targets --all-features`와 `cargo clippy --no-default-features` 둘 다 이 저장소 코드에서는 경고 0(벤더링된 `agent-client-protocol-test`의 기존 무관 경고 1건만 관찰, 손대지 않음).

## 2026-08-23 — `#75` Worker endpoint secret 마스킹(1단계), `#94` 등록

- 유형: `implementation` + `verification`
- `agent_endpoint`(예: `wss://host:port/ws?server-key=<secret>`)의 `server-key` 값은 워커의 grok 서브프로세스 ACP 인증 토큰 원문이다. 조사로 `workers.endpoint` 컬럼에 저장된 이 값이 네 경로 — `GET /v1/workers/{id}`, Dashboard `/api/workers`·`/api/events`, MCP `fleet_list_workers` — 로 그대로 새어 나가고 있음을 확인했다(전부 문자 그대로 `endpoint`라는 이름의 필드로). `GET /v1/workers/{id}`의 **인가**(누가 호출할 수 있는가)는 이미 `#73`이 닫아 뒀지만, **노출**(호출할 수 있는 사람에게 무엇이 보이는가)은 별개 계층으로 열려 있었다.
- `fleet-transport::acp_transport`에 로깅 전용으로만 있던 `sanitize_endpoint`(server-key 값을 `<redacted>`로 치환)를 `fleet_core::worker::mask_server_key`로 승격했다 — `fleet-core`가 의존성 없는 leaf 크레이트라 `fleet-api`/`fleet-dashboard`/`fleet-mcp`/`fleet-transport` 전부가 재사용할 수 있는 유일한 공통 지점이다. `fleet-transport`의 기존 함수는 이 새 함수를 호출하도록 바꾸고 중복 로직(같은 `server-key=` 경계 탐지를 두 곳에서 손코딩하고 있던 것)을 없앴다.
- 네 응답 경로(`fleet-api`·`fleet-dashboard`의 `worker_to_summary`, `fleet-mcp`의 `worker_summary`)에서 `w.endpoint.clone()`을 `mask_server_key(&w.endpoint)`로 바꿨다.
- 이벤트 로그(`FleetEvent::WorkerJoined`)는 append-only라 **쓰기 시점**(fleet-api의 `register_worker`/`join_worker` 핸들러가 이벤트를 구성하는 순간)에 마스킹하기로 결정했다 — 읽기 시점 필터링(`event_view.rs`가 task output에 쓰는 방식)만 하면 DB에는 원문이 영구히 남아, 나중에 필터링 로직에 구멍이 생기거나 DB에 직접 접근하면 그대로 노출된다. `event_view.rs`의 기존 redaction은 `task:output` capability로 조건부 노출(정당한 뷰어가 있다)인 반면, `server-key`는 정당한 외부 뷰어가 아예 없으므로(원문이 필요한 유일한 소비자는 `fleet-transport`가 실제로 다이얼할 때뿐) 무조건 마스킹이 맞는 설계다.
- 조사 중 로드맵 문구에 없던 두 곳도 함께 발견해 고쳤다: `transport.register` 실패 시의 `tracing::warn!` 로그 2곳(`handlers.rs`)이 마스킹 없이 원문을 로그에 남기고 있었다.
- `fleet-transport::acp_transport::register`/`build_ws_client`(워커에 실제로 다이얼하는 유일한 정당한 소비자)는 검토만 하고 손대지 않았다 — 여기서 원문이 필요한 게 맞다.
- **로드맵 원문의 2단계("최종적으로 ACP 인증을 URL query 밖(헤더 또는 mTLS)으로 이전")는 `#94`로 분리했다.** `#85`로 mTLS가 이미 canonical transport가 됐음에도 `join_worker.rs::mtls_agent_endpoint`/`WorkerConfig::agent_endpoint`의 mTLS 분기 둘 다 여전히 `server-key`를 URL에 넣는 것을 확인했다 — client cert가 채널을 이미 인증한 뒤에도 URL의 secret이 중복된 두 번째 인증 인자로 남아 있다. 이걸 지금 제거하지 않은 이유: `grok agent serve`(이 저장소 밖 외부 프로세스)가 실제로 어떤 인증을 강제하는지(헤더 기반 대안을 지원하는지, cert만으로 충분한지)를 이 저장소 코드만으로는 확인할 수 없다 — 확인 없이 URL에서 secret을 빼면 실제 배포에서 조용히 연결이 끊길 위험이 있다. `#94`(P1·설계 필요)로 등록해 그 확인부터 시작하도록 남겨 뒀다.
- 신규 테스트: `fleet-core::worker::tests`의 `mask_server_key_*` 6건(쿼리 값 치환, 스킴/호스트/경로 보존, 뒤따르는 다른 쿼리 파라미터 보존, fragment 보존, secret 없는 값은 그대로, 빈 문자열), `fleet-api/tests/capability_matrix_default_deny.rs`의 `get_worker_by_id_never_leaks_raw_server_key_even_with_capability`(인가된 호출자에게도 원문 미노출 — `#73`의 인가 테스트와 다른 계층임을 주석으로 명시)와 `register_worker_writes_masked_endpoint_into_the_event_log`(HTTP 응답이 아니라 저장소를 직접 읽어 이벤트 컬럼 자체가 마스킹돼 있음을 확인 — 읽기 시점이 아니라 쓰기 시점 마스킹임을 실증), `fleet-api/tests/transport_integration.rs`의 `registered_worker_endpoint_is_masked_on_read_even_though_transport_got_the_raw_value`(같은 register 요청 안에서 transport 호출은 원문을, 뒤이은 GET 응답은 마스킹된 값을 받음을 동시에 확인 — "원문이 필요한 유일한 소비자"라는 설계 주장을 하나의 테스트로 실증), `fleet-dashboard/tests/dashboard_api.rs`의 `workers_list_never_leaks_raw_server_key`, `fleet-mcp/src/handlers.rs`의 `worker_summary_never_leaks_raw_server_key`.
- 문서: `docs/contracts/worker-enrollment.md`의 "남은 노출 경계" 절을 갱신했다 — `#73`(인가)과 `#75`(노출 마스킹) 둘 다 닫혔음을 반영하고, `#94`로 분리된 남은 절반(URL의 secret 자체)을 명시했다.
- 검증: `cargo test --workspace`(전체 통과), `DATABASE_URL` 주입 `fleet-store`/`fleet-api` 직렬 재실행(전체 통과), `cargo check --no-default-features` 통과, `cargo clippy --all-targets --all-features`와 `cargo clippy --no-default-features` 둘 다 이 저장소 코드 기준 경고 0(초기 `needless_borrows_for_generic_args` 2건 — `mask_server_key` 호출부의 불필요한 `&` — `cargo clippy --fix`로 정정).

## 2026-08-23 — `#76` 감사 범위 확장(mutation·capability 거절, 1단계), `#95` 등록

- 유형: `implementation` + `verification`
- `docs/security/authorization-and-audit.md`의 "구현 게이트 6"(모든 mutation과 sensitive deny가 상관관계 필드·secret-free audit record를 남긴다)을 조사하니, 실제로 `AuditEvent`를 남기는 경로는 `#66`이 닫은 LLM credential export/put/delete 세 곳뿐이었다 — bootstrap token 발급·회수, admin API token 생성·회전·회수, Worker 등록·등록해제, Host 등록, 그리고 모든 capability 거절이 `tracing::warn!`으로만 남아 사후 조회가 불가능했다.
- 새 감사 액션 7개(`token.bootstrap.issue`/`.revoke`, `admin_token.create`/`.rotate`/`.revoke`, `worker.register`/`.deregister`, `host.register`, `http.capability_denied`)를 `fleet_core::audit::action`에 추가하고 해당 핸들러 7곳 + `auth_middleware`의 인증 분기 5곳에 감사 기록을 배선했다. heartbeat(초당 다건 호출)는 identity 변경이 아니라서 의도적으로 제외했다.
- **fail-closed를 발급(mint) 쪽에도 적용**: `#66`의 "감사 실패 시 평문을 반환하지 않는다"는 export 전용 원칙이었는데, bootstrap/admin token 발급도 같은 위험 프로파일이다 — 누가 새 자격을 발급받았는지 기록하지 못하면 그 자격이 감사되지 않은 채 살아있게 된다. `create_bootstrap_token`/`create_admin_token`/`rotate_admin_token`은 감사 기록이 실패하면 방금 만든(또는 회전된) 토큰을 즉시 회수하고 500을 반환한다. `rotate_admin_token`은 이전 토큰을 되살릴 방법이 없으므로(store 계층에서 이미 무효화됨) 새 토큰마저 회수해 principal을 무자격 상태로 안전하게 실패시키는 쪽을 택했다 — 권한이 남는 쪽보다 없는 쪽으로 실패하는 게 안전하다는 원칙을 rotate에도 일관 적용한 것.
- 반대로 이미 반영돼 되돌릴 필요가 없는 mutation(bootstrap/admin token 회수, Worker 등록·등록해제, Host 등록, capability 거절)은 감사 실패로 이미 결정된 응답을 뒤집지 않는다(log-only) — `delete_worker_credential`(`#66`)이 세운 "삭제는 이미 반영됐으니 200을 500으로 뒤집지 않는다" 원칙을 그대로 확장했다. 두 원칙(발급은 fail-closed, 이미 반영된 변경은 log-only)을 구분한 이유를 로드맵/문서에 명시했다 — 나중에 새 mutation을 추가할 때 어느 쪽을 따를지 판단 기준이 되도록.
- **capability 거절 감사 배선 중 발견한 컴파일 제약**: `authorize_http_endpoint(&req)`가 반환하는 `Err`를 감사하려고 `record_capability_denial(state, req: &Request)`를 만들었더니 `future cannot be sent between threads safely` 에러가 났다 — `axum::http::Request<Body>`는 `Sync`가 아니고, async fn은 파라미터 타입 전체를 자신의 상태 머신에 담기 때문에 함수 내부에서 실제로 `&Request`를 await 경계 너머로 들고 있지 않아도 시그니처에 `&Request`가 있는 것만으로 반환 future가 `Send`를 잃는다(`from_fn` 미들웨어는 Send future를 요구). 필요한 값(`method`/`path`/`AuthorizationContext`)만 owned로 뽑아 `record_capability_denial(state, method: Method, path: String, ctx: Option<AuthorizationContext>)`로 시그니처를 바꿔 해결했다 — `auth_middleware`의 5개 인증 분기(개발 무인증·worker operational credential·admin DB 토큰·Cloudflare Access·env bearer allow-list) 전부에 동일 패턴을 적용했다.
- **기존 테스트 인프라 확장**: `MemStore::with_failing`(회복성 테스트 전용 실패 주입 빌더, 기존엔 `list_tasks`/`delete_expired_sessions`/`delete_old_login_attempts` 3개만 지원)에 `"record_audit_event"`를 추가해, 이번 fail-closed 경로를 실제로 감사 실패 상황에서 검증할 수 있게 했다. 이 과정에서 `crates/fleet-api/tests/bootstrap_tokens.rs`의 기존 minimal `BsStore`(`record_audit_event`를 구현하지 않아 `Store` 트레이트 기본값인 `Err(StoreError::Unsupported)`로 폴백)가 새 fail-closed 로직 때문에 발급 관련 테스트 전부를 500으로 깨뜨리는 것을 발견해, `audit_events: Mutex<Vec<AuditEvent>>` 필드와 실제 `record_audit_event`/`list_audit_events` 구현을 추가했다. **운영 함의**: 감사를 구현하지 않은 `Store`로 배포하면 이제 bootstrap/admin token 발급·회전이 전부 500이 된다 — 프로덕션 백엔드(Postgres)는 처음부터 구현돼 있어 영향 없지만, 커스텀 `Store` 구현체가 있다면 확인이 필요하다.
- 신규 테스트: `crates/fleet-api/tests/audit_coverage.rs` 9건 — `bootstrap_token_issue_and_revoke_are_audited`, `bootstrap_token_issuance_fails_closed_when_audit_recording_fails`(감사 실패 시 발급된 토큰이 즉시 회수돼 살아있는 토큰이 0개임을 확인), `admin_token_create_rotate_revoke_are_audited`, `admin_token_creation_fails_closed_when_audit_recording_fails`, `admin_token_rotation_fails_closed_when_audit_recording_fails`(store trait을 직접 호출해 시딩 → HTTP로 rotate 시도 → 새 토큰도 즉시 회수됨을 확인), `worker_register_and_deregister_are_audited`(`agent_endpoint`의 `server-key` 값이 감사 detail에 실리지 않음도 함께 확인 — `#75`와의 경계 재확인), `host_register_is_audited`, `capability_denial_is_audited`, `capability_denial_audit_failure_does_not_change_the_403_response`(거절 감사가 log-only임을 대칭적으로 확인).
- **분리한 범위 → `#95`**: `AuditEvent`에 `request_id`/`project_id`/`attempt_id`/`policy_revision` 상관관계 필드를 추가하는 것과, Dashboard·MCP 표면의 동일 감사는 이번에 포함하지 않았다. `project_id`/`attempt_id`는 대응하는 Project/Attempt 엔티티가 이 저장소에 아직 없어(`#48`·`#62` 계열 선행) 상관시킬 대상 자체가 없다 — 지금 필드만 추가하면 항상 `NULL`인 죽은 컬럼이 된다. Dashboard는 `#73`이 지적한 대로 중앙 capability 행렬 자체가 없어 감사를 걸 지점이 불명확하고(`#92`가 다룸), MCP tool별 감사도 별도 설계가 필요하다.
- 검증: `cargo test -p fleet-api --all-features`(새 스위트 포함 전체 통과), `cargo test --workspace`(96 스위트 전체 `test result: ok`, 0 FAILED), `DATABASE_URL` 주입 `fleet-store`/`fleet-api` 직렬 재실행(26 스위트 전체 통과 — mem.rs 변경이 PgStore 경로에 영향 없음을 확인), `cargo check --no-default-features` 클린, `cargo clippy --all-targets --all-features` 경고 0.

## 2026-08-23 — `#54` liteLLM 배포 hardening

- 유형: `implementation` + `verification(limited — no docker daemon in this environment)`
- 로드맵이 요구한 네 항목(secret 외부 주입, 고정 image version, healthcheck, 비공개 기본 bind)을 저장소가 직접 증명하는 유일한 자산 — `docker-compose.yml`의 `litellm` service와 `examples/litellm-config.yaml` — 에 전부 적용했다.
- **고정 image version**: `ghcr.io/berriai/litellm:main-latest`(floating)를 `ghcr.io/berriai/litellm:v1.98.0`으로 바꿨다. GitHub releases 페이지(2026-08-23 릴리즈된 최신 stable)와 GHCR 패키지의 실제 태그 목록(`v1.98.0`/`1.98.0` 확인) 둘 다로 태그가 실재함을 교차 확인했다.
- **secret 외부 주입**: `LITELLM_MASTER_KEY: sk-litellm-master-key`가 compose 파일에 원문으로 박혀 있어, 셸에서 같은 이름의 env var를 export해도 아예 무시되는 버그였다(env var 참조 자체가 없었다) — `${LITELLM_MASTER_KEY:-sk-litellm-master-key-change-me}`로 바꿔 외부 주입이 실제로 override 가능하게 했다. `examples/litellm-config.yaml`의 `general_settings.master_key`도 하드코딩 대신 이미 provider key들(`GEMINI_API_KEY`/`GROQ_API_KEY`)이 쓰던 `os.environ/...` 관례로 통일해 `os.environ/LITELLM_MASTER_KEY`로 바꿨다 — 값을 compose 파일의 env var 한 곳에서만 관리하고 yaml에 중복 하드코딩하지 않는다.
- **healthcheck**: `postgres`/`orchestrator`엔 있었는데 `litellm`엔 없었다. 이미지의 실제 Dockerfile을 조회해 최종 런타임이 Chainguard `wolfi-base` 기반이고 `curl`/`wget`이 설치돼 있지 않음을 먼저 확인했다 — 관례적인 `curl` 기반 healthcheck를 그대로 썼다면 이미지에 없는 바이너리를 호출해 항상 unhealthy로 빠졌을 것이다. 대신 이미지에 이미 있는 `python3`(stdlib `urllib`)로 liteLLM 공식 문서로 확인한 `/health/readiness`(인증 불필요, DB 연결까지 확인)를 호출한다. `orchestrator`의 `depends_on.litellm.condition`도 `service_started`(컨테이너 기동만 확인)에서 `service_healthy`(healthcheck 통과까지 대기)로 바꿔, 오케스트레이터의 첫 dispatch가 gateway 준비 전 connection-refused로 실패할 여지를 없앴다.
- **비공개 기본 bind**: `"4000:4000"`(모든 호스트 인터페이스에 publish)을 `"127.0.0.1:4000:4000"`으로 바꿨다 — liteLLM master key가 유출되면 등록된 모든 LLM 공급자 API 키를 대신 소비당할 수 있어, 위험도가 orchestrator API 노출보다 크다고 판단했다.
- **검증 한계를 명시적으로 기록한다**: 이 작업 환경에는 docker/docker compose가 설치돼 있지 않아, 실제 컨테이너를 기동해 healthcheck 통과·포트 bind·`depends_on` 순서를 동적으로 재현하는 검증은 하지 못했다. 대신 `python3 -c "import yaml; yaml.safe_load(...)"`로 두 YAML 파일의 문법 오류가 없음을 정적으로 확인했고, image 태그(GitHub releases + GHCR 태그 목록)·health 엔드포인트 경로와 인증 요구사항(공식 문서)·base 이미지의 바이너리 구성(실제 Dockerfile)은 각각 업스트림을 직접 조회해 교차 확인했다 — 이 저장소 코드를 실제로 실행해 확인한 `#82`~`#85`(실제 SSH 서버·TLS handshake)와는 확증 등급이 다르다는 점을 그대로 남겨 둔다. docker가 있는 환경에서 `docker compose up -d litellm` → `docker compose ps`로 healthy 전이와 `orchestrator`의 대기를 실제로 재검증하는 걸 권장한다.
- 문서: [litellm-gateway.md](deployment/litellm-gateway.md)의 "저장소에서 검증된 자산" 표와 "시작 전"/"기동과 검증" 절을 갱신해 네 항목이 기본값으로 이미 반영돼 있음을 명시했다.

## 2026-08-23 — `#70`(부분) 실패 원인별 metric 분해와 `FailureKind` 죽은 코드 제거

- 유형: `implementation` + `verification`
- `#70`(Observability·reconciliation·recovery) 전체는 아직 존재하지 않는 하부 구조(Reconciler, `worker_execution_lease`, effect ledger — `#63`·`#67` 선행)에 의존해 착수 전이다. 이번엔 `#90` 취소 조사에서 이미 좁게 carve-out된 조각만 완료했다: `/metrics`가 `fleet_tasks_total{phase="failed"}`로 실패 총량만 알려주고 원인을 분해하지 않아, 운영자가 "credential 미프로비저닝으로 몇 건 실패했나" 같은 질문에 답할 수 없던 gap.
- 신설 `fleet_tasks_failed_total{kind}` gauge를 `crates/fleet-api/src/metrics.rs`에 추가했다. 스크랩마다 store의 전체 task 목록을 순회하며 `TaskStatus::Failed(failure).kind`별로 집계한다(다른 metric들과 같은 "매 스크랩 즉시 집계" 패턴 — 별도 백그라운드 카운터 없음). 관측되지 않은 kind도 `FailureKind::ALL`을 순회해 0으로 명시적으로 찍는다 — 라인이 아예 없는 것과 0건인 것을 대시보드·alert가 구분할 수 있어야 하기 때문.
- **`FailureKind::Timeout`/`AuthFailed`/`Cancelled` 세 variant를 제거했다.** 로드맵 `#90`이 "저장소 어디서도 생성되지 않는 죽은 코드"라고 지적한 것을 이번에 직접 재확인(`FailureKind::` 전체 사용처 grep)하고, 각각 왜 죽어 있었는지 원인을 추적했다:
  - `Cancelled`: 애초에 도달 불가능한 설계 중복이었다 — `TaskStatus`에 이미 별도 top-level `Cancelled { reason }` variant가 있어, 취소된 작업은 `TaskStatus::Cancelled`로 표현되지 `TaskStatus::Failed(TaskFailure { kind: Cancelled })` 경로를 절대 타지 않는다.
  - `Timeout`: `fleet-transport::WorkerEvent::Failed`가 지금도 구조화되지 않은 `error: String` 하나뿐이라(4개 생성 지점 모두 확인) 타임아웃인지 다른 워커 에러인지 애초에 구분할 방법이 없다 — 진짜 구현하려면 transport 계층에 구조화된 에러 kind를 추가하는 별도 작업이 필요하고(로드맵 `#67`의 fencing/lease 작업과 자연스럽게 묶인다), 그 전에 variant만 살려 두면 항상 미사용 상태로 남는다.
  - `AuthFailed`: OIDC 토큰 검증이라는 doc comment의 전제 자체가 task 실행 경로에 아직 존재하지 않는다(`grep -rl oidc`로 확인 — Dashboard 세션 인증 목표 계약에서만 언급될 뿐 구현 없음).
  - 셋 다 "지금 구현"이 아니라 "제거"를 선택했다 — Timeout/AuthFailed는 선행 작업(transport 구조화 에러, task-level OIDC) 없이는 만들어도 항상 미사용이고, Cancelled는애초에 틀린 설계다.
- `FailureKind::ALL`(4-variant 고정 배열)과 `as_str()`을 추가했다 — metric label 문자열이 `#[serde(rename_all = "snake_case")]` 직렬화 표현과 갈라지지 않도록 값 일치를 테스트로 강제하고(`ALL`을 순회하는 테스트), 새 variant를 추가할 때 `ALL` 갱신을 잊으면 metric에서 그 kind가 조용히 빠지는 문제를 컴파일러가 완전히 막지는 못하지만 최소한 테스트가 값 불일치를 잡는다.
- 스키마 변경 없음을 확인했다 — `TaskStatus`(그 안의 `TaskFailure.kind`)는 Postgres `tasks.status` 컬럼에 JSONB로 통째로 저장되며 CHECK 제약이 없다(`crates/fleet-store/migrations/001_init.sql`). **다만 주의**: 이론상 과거에 이 3개 variant로 직렬화된 row가 실제 production DB에 남아 있다면, `serde_json::from_value::<TaskStatus>`가 알 수 없는 variant로 그 row에서 하드 에러를 낸다(조용한 손상이 아니라 명확한 실패) — 이번 조사로는 코드상 생성 지점이 전혀 없음을 확인했을 뿐, 실제 production DB를 조회해 과거 row까지 확인하지는 않았다.
- 신규 테스트: `fleet-core::task::failure_kind_as_str_matches_serde_snake_case_for_every_variant`, `fleet-api::metrics::failed_tasks_are_broken_down_by_failure_kind`(같은 kind 중복 집계, 다른 kind 별도 집계, 미관측 kind의 명시적 0 라인 모두 확인).
- 검증: `cargo check --workspace --all-features`, `cargo test --workspace`(96 스위트 전체 `ok`), `DATABASE_URL` 주입 `fleet-store`/`fleet-api`/`fleet-scheduler` 직렬 재실행(30 스위트 전체 통과 — JSONB 왕복이 실제로 깨지지 않았음을 확인), `cargo check --no-default-features`, `cargo clippy --all-targets --all-features` 경고 0.

## 2026-08-23 — `#63`(1단계) Control plane lease/epoch primitive

- 유형: `implementation` + `verification`
- `#63`(Cold Standby fencing) 전체는 여러 세션에 걸친 작업이다 — 이번엔 `docs/architecture/control-plane-authority-and-failover.md`의 "Lease와 상태 전이" 절이 정의하는 lease/epoch primitive만 세웠다. 이 위에 실제 dispatch/cancel/Agent command 핸들러를 배선하는 것과 Worker 안의 `worker_execution_lease`(fencing token, `#67`)는 다음 단계로 남긴다.
- `fleet-store`: migration `021_control_plane_lease.sql`(`cluster_id` PK, `active_instance_id`, `epoch`, `acquired_at`, `expires_at`, `last_renewed_at`), `Store` trait에 `acquire_control_lease`/`renew_control_lease`/`release_control_lease`/`get_control_lease` 4개 추가(기본 구현은 다른 신규 trait method들과 같은 관례로 `Unsupported` — 기존 minimal Store 테스트 double을 깨지 않기 위함). `PgStore`/`MemStore` 둘 다 구현.
  - **획득**: `INSERT ... ON CONFLICT (cluster_id) DO UPDATE ... WHERE expires_at < NOW() RETURNING ...` 단일 원자적 statement. Postgres는 `DO UPDATE ... WHERE` 조건이 거짓이면 그 행을 갱신하지도 RETURNING에 포함하지도 않는다는 문서화된 동작을 그대로 이용해, "유효한 lease가 있으면 거절(빈 결과)"과 "만료됐으면 가로챔(갱신된 행 반환)"을 별도 조회 없이 한 SQL로 표현했다.
  - **갱신**: `WHERE cluster_id=$1 AND active_instance_id=$2 AND epoch=$3 AND expires_at > NOW()` — 3중 CAS. `expires_at > NOW()`를 빼면 이미 만료된(그러나 아직 아무도 가로채지 않은) lease를 instance_id·epoch만 맞다는 이유로 "갱신"이라는 이름으로 되살릴 수 있고, 그 사이 다른 instance가 막 가로챘다면 두 instance가 동시에 유효하다고 믿는 경합이 생긴다 — 이 세 번째 조건이 그 경합을 막는다.
  - 모든 시간 비교는 `NOW()`(DB 서버 시각)만 쓴다. 애플리케이션 서버 시각을 신뢰하면 클럭 스큐만으로 안전성이 깨질 수 있어서다.
  - TTL은 `chrono::Duration`이 아니라 `std::time::Duration`으로 받는다 — sqlx가 `std::time::Duration`을 Postgres `INTERVAL`로 직접 bind 지원해서(`sqlx-postgres`의 `Encode`/`Type` impl 확인), 앱에서 `NOW() + ttl`을 계산하지 않고 그대로 SQL에 넘길 수 있다. 앱에서 미리 `expires_at`을 계산해 바인딩하는 방식은 그 자체가 다시 앱 서버 시각에 의존하게 돼 피했다.
- `fleet-scheduler::lease::LeaseManager`(신설, `HealthChecker`와 동일한 `new`/`spawn`/`status()` 패턴): `LeaseStatus::{Stopped, Active{epoch}, Fenced}` 상태를 `Arc<Mutex<_>>`로 관측 가능하게 노출. `try_acquire`/`try_renew`/`release`를 개별 호출 가능한 public 메서드로 둬 테스트에서 루프 없이 직접 검증할 수 있게 했다(`scan_once` 관례). 백그라운드 루프는 획득 실패(거절 또는 store 에러) 시 `poll_interval` 뒤 재시도, 갱신 실패 시 `Fenced`로 전이하고 다시 획득 루프로 돌아간다 — 상태 다이어그램의 `Refused`/`Fenced → Stopped`을 "재시도 중단"이 아니라 "계속 폴링"으로 구현했다. 안전성은 재시도를 막는 게 아니라 매 획득마다 증가하는 epoch가 담당한다(이전 epoch의 늦은 쓰기는 renew CAS에서 자연히 거절된다).
- **아직 안 한 것**(다음 단계로 명시): 이 lease를 dispatch/cancel/Agent command/breaker 변경 핸들러가 실제로 검사하도록 배선(불변식 2 강제), `fleet-cli::run_serve`에서 `LeaseManager::spawn` 호출, Worker 쪽 fencing token(`#67`), 수동 승격 Runbook.
- 신규 테스트: `crates/fleet-store/tests/control_plane_lease.rs`(10건, 실제 Postgres) — 핵심은 `concurrent_acquire_attempts_only_one_wins`(구현 게이트 1: 실제 커넥션 풀로 8개 instance가 동시에 획득을 시도해도 정확히 1개만 성공함을 mock이 아니라 실제 DB로 확인)와 `renew_after_expiry_fails_even_with_correct_instance_and_epoch`(만료된 lease는 instance_id·epoch가 맞아도 갱신으로 되살아나지 않음). `crates/fleet-scheduler/src/lease.rs`의 단위 테스트 9건(MemStore 기반, 백그라운드 루프의 실제 타이밍 동작 2건 포함 — flaky 여부를 3회 반복 실행으로 확인, 전부 안정적으로 통과).
- 검증: `cargo check --workspace --all-features`, `cargo test --workspace`(97 스위트 전체 `ok`), `DATABASE_URL` 주입 `fleet-store`/`fleet-api`/`fleet-scheduler` 재실행(31 스위트 전체 통과, `control_plane_lease.rs` 10건 포함), `cargo check --no-default-features`, `cargo clippy --all-targets --all-features` 경고 0.

## 2026-08-24 — `#63`(2단계) lease를 dispatch/cancel/reconcile 핸들러에 배선

- 유형: `implementation` + `verification`
- 1단계(어제)에서 만든 control plane lease/epoch primitive는 그 자체로는 아무것도 강제하지 않는 "관측 가능한 상태"였다. 이번엔 실제로 그 상태를 검사해 신규 제어 동작을 거절하는 배선을 완성했다 — `docs/architecture/control-plane-authority-and-failover.md`의 불변식 2("lease가 없는 인스턴스는 조회를 제공할 수 있어도 dispatch, cancel, Agent command, breaker 변경을 수행하지 않는다").
- `fleet-scheduler::state::FleetState`에 `lease: Option<LeaseObserver>`(`with_lease` builder)와 `lease_allows_control()`을 추가했다. `None`(기본값)이면 항상 `true`를 반환한다 — HA lease를 켜지 않은 배포와 기존 수십 개의 `FleetState::new(...)` 테스트 호출부를 전혀 건드리지 않고도 도입할 수 있었다.
- 거절 지점 4곳:
  - `Dispatcher::dispatch_existing` — 워커 선택, breaker 상태 변경, `Dispatched` 전이, transport dispatch까지 전부 담당하는 단일 choke point라 여기 하나만 막아도 `submit()`(최초 제출)과 `Reconciler`(stale-pending 재시도) 양쪽 경로가 동시에 커버된다.
  - `Dispatcher::cancel` — task 조회보다 lease 확인을 먼저 한다. 존재하지 않는 task_id로도 `ControlPlaneFenced`가 먼저 나와야, "lease 없는 인스턴스는 애초에 어떤 task도 취소할 권한이 없다"는 사실이 task 존재 여부와 무관하게 성립한다.
  - `Dispatcher::handle_worker_event` — `Completed`/`Failed`만 대상이다. `Output`(stdout/stderr 버퍼링)은 권위 있는 "제어" 결정이 아니라 순수 append-only 관측 데이터 전달이므로 fenced 상태에서도 계속 흘려보낸다 — 막으면 사용자가 보던 실시간 출력만 끊기고 얻는 안전 이득이 없다. 이 구분을 놓치지 않기 위해 `matches!` guard로 명시했고, 대칭 테스트(`output_event_is_still_processed_when_control_plane_lease_is_fenced`)로 고정했다.
  - `Reconciler::reconcile_once` — 개별 `dispatch_existing` 호출 하나하나가 아니라 **sweep 전체**를 건너뛴다. 문서가 "Reconciler는 Active Orchestrator epoch에서만 동작한다"고 명시적으로 규정하기 때문에, dead-letter 확정·재dispatch·stale Dispatched reap을 개별적으로 막는 것보다 sweep 진입 자체를 막는 게 문서의 의도와 정확히 일치한다. 이 top-level 가드 덕분에 `dispatch_existing`에서 도달하는 `ControlPlaneFenced` 분기는 "sweep 시작 시점엔 유효했는데 그 사이 fenced된" 드문 경합에서만 실행되는 방어적 코드가 됐다 — 그 경우도 별도 match arm으로 명시해 `Failed`로 잘못 마킹됐다고 오해하는 로그가 나오지 않게 했다(기존 catch-all `Err(e)` 분기의 주석이 "dispatch_existing이 이미 Failed로 마킹했다"고 가정하고 있었는데, `ControlPlaneFenced`는 그 가정을 깬다).
  - 로그에 `event`를 통째로 남기지 않는다 — `WorkerEvent::Completed.result.output`은 워커 실행 결과 원문이라, 거절 로그에 `?event`를 그대로 찍으면 이 세션 내내 지켜온 secret/output-free 로그 원칙을 이 지점에서만 어기게 된다. task_id만 추출해 로깅한다.
- `fleet-cli::run_serve`가 이제 **모든** 배포(단일 인스턴스 포함)에서 `LeaseManager`를 무조건 spawn한다. 옵트인 플래그를 만들지 않은 이유: 경쟁자가 없는 lease 획득은 사실상 즉시 성공해 실질적인 지연이 없고(빈 테이블에 대한 `acquire`는 항상 성공하는 upsert 한 번), 옵트인으로 두면 이 메커니즘 전체가 실제 배포 어디서도 실행되지 않아 검증되지 않은 채로 남는다 — 이 세션 내내 지켜온 "무인 운영, 불필요한 새 설정 노브를 만들지 않는다" 원칙과도 맞는다. `cluster_id`는 `"default"` 고정값(이 저장소가 아직 단일 클러스터만 지원). `instance_id`는 매 기동마다 새 UUID(재기동 사이 안정적일 필요 없음 — epoch가 진짜 신원이다). migration은 이 지점보다 앞서 항상 끝나 있으므로(`store.migrate()`가 `run_serve` 앞부분에서 실행됨) `control_plane_lease` 테이블 부재 걱정은 없다.
- **의도적으로 미룬 것**: graceful shutdown 시 lease를 즉시 release하지 않는다 — `LeaseManagerHandle::abort()`는 갱신 루프만 멈추고, lease는 TTL(기본 15초)만큼 자연 만료를 기다린 뒤 다른 인스턴스가 가져간다. 즉시 release(빠른 failover)는 후속 개선으로 남겼다 — `LeaseManagerHandle`이 release에 필요한 `(cluster_id, instance_id, epoch)`를 갖고 있지 않아 별도 설계가 필요했다.
- **검증 한계를 명시적으로 남긴다**: `fleet-cli::run_serve`의 실제 배선(LeaseManager를 실제로 spawn해 FleetState에 주입하는 부분)은 컴파일과 코드 리뷰로만 확인했다. `fleet serve`의 MCP stdio 서브시스템이 stdin EOF에서 프로세스 전체를 종료시키는 특성(이 세션에서 `#75`/`#76` 검증 때도 마주친 동일한 제약) 때문에, 실제로 2개의 `fleet serve` 프로세스를 같은 DB에 띄워 라이브로 "하나가 Active, 하나가 Refused"를 관찰하는 종단간 검증은 이번에도 하지 못했다. 대신 각 레이어를 개별적으로 실제/현실적인 조건에서 검증했다 — Store 수준 CAS는 `#63` 1단계에서 이미 실제 Postgres 동시성으로, Dispatcher/Reconciler의 거절 로직은 이번에 수동으로 구성한 fenced `LeaseObserver`로. 이 조합이 "층별로는 각각 실증됐지만 전체 조립은 코드 리뷰 수준"이라는 걸 정직하게 기록해 둔다.
- 신규 테스트: `crates/fleet-scheduler/src/dispatcher.rs` 6건(`submit`/`cancel`/`Completed`/`Failed` 거절 4건, `Output`은 계속 처리됨을 확인하는 대칭 테스트 1건, 그리고 이 전부를 세팅하는 `setup_fenced` 헬퍼), `crates/fleet-scheduler/src/reconcile.rs` 1건(`ReconcileSummary::default()`와 정확히 일치 — sweep 전체가 진입조차 안 했음을 확인).
- 검증: `cargo check --workspace --all-features`, `cargo test --workspace`(97 스위트 전체 `ok`, 0 FAILED), `DATABASE_URL` 주입 `fleet-store`/`fleet-api`/`fleet-scheduler` 재실행(31 스위트 전체 통과), `cargo check --no-default-features`, `cargo clippy --all-targets --all-features` 경고 0.

## 2026-08-24 — `#48`(1단계) Project 엔티티와 CRUD

- 유형: `implementation` + `verification`
- 로드맵 대부분(`#64`/`#65`/`#67`/`#68`/`#69`/`#86`~`#93`)이 아직 없는 Project/Agent 엔티티에 막혀 있다는 걸 확인한 뒤, 사용자가 `#48` 착수를 선택했다. `docs/architecture/project-feature-design.md`가 이미 "구현 상태는 부분 구현"이라고 밝혀 뒀듯, `ProjectId`와 `tasks.project_id`는 있었지만 참조 대상인 `projects` 테이블 자체가 없어 순수 미검증 메타데이터였다 — 이번이 처음으로 실제 엔티티를 만든 작업이다.
- **의도적으로 좁힌 범위**: 목표 계약 전체(Agent admission, `max_active_agents`/`max_warm_agents` 강제, Worker eligibility selector, 정책 revision, 5-상태 lifecycle)는 Agent·AgentTemplate·effect ledger·`worker_execution_lease`(`#67`) 같은 하부 구조가 이 저장소에 전혀 없어 지금 만들 수 없다. `docs/contracts/project-management.md`의 "승인 전 차단 후보"(정책 변경 API, host/worker 배정)도 보안 모델 승인이라는 명시된 선행 조건이 있어 손대지 않았다. 1단계는 그 경계 안에서 실제로 만들 수 있는 것만 골랐다: Project 엔티티 자체, CRUD, 그리고 지금 이미 존재하는 데이터(Task 상태)만으로 확인 가능한 유일한 archive 게이트.
  - `ProjectStatus`를 목표 5-상태(`Draft`/`Active`/`Draining`/`ArchiveBlocked`/`Archived`)가 아니라 3-상태(`Active`/`Draining`/`Archived`)로 줄였다 — `Draft`는 검증할 AgentTemplate이 없고, `ArchiveBlocked`는 근거가 될 Agent process/lease/credential grant cleanup 증거가 없다. `#70`에서 `FailureKind`의 죽은 variant 세 개를 제거하며 세운 원칙("실제로 채울 방법이 없는 variant는 만들지 않는다")을 여기도 그대로 적용했다.
  - archive(`DELETE`/`fleet_delete_project`) 게이트는 목표 계약의 전체 절차(Attempt 관찰 → effect ledger 재평가 → Agent process/lease/credential grant cleanup 확인) 대신 "이 Project를 참조하는 비종료 Task가 없다" 하나뿐이다 — `Store::project_has_active_tasks`가 `tasks.status_phase IN ('pending','dispatched')`를 확인한다. 이 게이트를 통과하면 `request_id`도, `202`+비동기 progress도 없이 같은 요청 안에서 곧바로 `Draining → Archived`까지 진행한다 — 배경에서 기다릴 대상(Agent 등)이 없으니 즉시 판정 가능해서다. 재호출은 안전하다(같은 게이트를 다시 평가할 뿐인 idempotent 동작).
- **`fleet-store`**: migration `022_projects.sql`(`projects` 테이블), `Store::{create,get,get_by_name,list,update_status}_project`/`project_has_active_tasks`. `tasks.project_id`에 FK를 걸지 않았다 — 이 컬럼은 migration 이전부터 존재해 실제 배포 DB에 이미 검증 안 된 값이 저장돼 있을 가능성을 배제할 수 없고, FK를 강제하면 그런 배포에서 migration 자체가 실패한다. 참조 무결성은 애플리케이션 계층(Task 제출 시 존재·상태 확인)이 담당한다. `project_has_active_tasks`는 `tasks.status_phase`(001_init.sql의 생성 칼럼, `TaskStatus`의 `#[serde(tag = "phase")]` 덕에 `'pending'`/`'dispatched'`/... 평문)를 그대로 써서 새 인덱스 없이 구현했다.
- **`fleet-core`**: `PermissionKind::{ProjectRead,ProjectCreate,ProjectDelete}` 신설(`project:policy_manage`/`project:assign`은 문서가 명시한 차단 조건 때문에 만들지 않음). `Operator`/`Viewer` 기본 역할에 `ProjectRead`를 추가했다(둘 다 이미 다른 읽기 전용 조회 권한을 가지고 있어 자연스러운 확장).
- **`fleet-dashboard`**: `GET/POST /api/projects`, `GET/DELETE /api/projects/{id}`. JSON body를 받는 API라 CSRF는 `logout`과 동일한 `X-CSRF-Token` 헤더 variant를 썼다(기존 task 제출 폼처럼 HTML form 필드에 심는 방식은 JSON 엔드포인트에 맞지 않는다). 생성·archive 성공은 `project.create`/`project.archive_requested`/`project.archived` 감사 액션으로 기록한다.
- **`fleet-mcp`**: `fleet_create_project`/`fleet_list_projects`/`fleet_delete_project` 신설. `fleet_dispatch_task`의 `project_id` 처리를 "파싱만"에서 "존재·상태 검증"으로 바꿨다 — 이게 이번 작업에서 project_id가 처음으로 실제 의미를 갖게 된 지점이다. 검증 로직은 Dashboard의 `delete_project_api`와 정확히 같은 archive 절차를 `fleet_delete_project`에도 그대로 복제했다(두 표면이 같은 규칙을 써야 한다는 계약 문서의 요구와 일치).
- **사전 존재 버그 발견 (별도 태스크로 분리)**: DB 검증 중 `fleet-mcp/tests/cross_client.rs`의 `unknown_tool_returns_error`가 clean main에서도 항상 실패하는 걸 확인했다 — 등록되지 않은 MCP tool 이름 호출 시 JSON-RPC `-32601`(method not found)이 아니라 `-32600`(invalid request)을 반환한다. `git stash`로 내 변경을 걷어내고 재현해 이 작업과 무관함을 확인한 뒤 `spawn_task`로 분리했다(`task_78be5260`).
- **검증 중 발견한 별개 이슈**: `fleet-mcp/tests/cross_client.rs`는 `target/debug/fleet` 바이너리를 subprocess로 띄우는데, migration 022 추가 전에 빌드해 둔 바이너리로 테스트를 돌리면 "migration 22 was previously applied but is missing in the resolved migrations" 에러가 난다 — sqlx가 (DB가 아는 migration 집합)과 (이 바이너리에 컴파일 시점에 embed된 migration 집합)의 불일치를 정확히 감지해 거부한 것으로, 실제 버그가 아니라 오래된 로컬 바이너리 때문이었다. `cargo build -p fleet-cli --bin fleet`로 재빌드 후 정상 통과했다.
- 신규 테스트: `crates/fleet-core/src/project.rs` 4건(상태 문자열 round-trip, `accepts_new_tasks`, 생성자 기본값, builder 메서드), `crates/fleet-store/tests/projects.rs` 8건(실제 Postgres — round-trip, 중복 이름 충돌, pagination/정렬, 상태 필터, 상태 전이, 미존재 id 처리, active task 유무에 따른 게이트), `crates/fleet-dashboard/tests/dashboard_api.rs`에 11건(권한 403 2건, CSRF 거절, 빈 이름 거절, 중복 이름 409, 404, archive가 즉시 완료되는 경우/task가 남아 draining 유지되는 경우/이미 archived인 경우의 idempotent 재호출), `crates/fleet-mcp/src/handlers.rs`에 7건(round-trip, 빈/중복 이름 거절, 미존재 project 삭제, active task 있으면 draining 유지, dispatch_task의 미존재/archived project 거절).
- 검증: `cargo check --workspace --all-features`, `cargo test --workspace`(98 스위트 전체 `ok`, 0 FAILED), `DATABASE_URL`+`FLEET_MCP_CAPABILITIES` 주입 `fleet-store`/`fleet-api`/`fleet-scheduler`/`fleet-mcp`/`fleet-dashboard` 재실행(사전 존재 버그 1건 제외 전체 통과), `cargo check --no-default-features`, `cargo clippy --all-targets --all-features` 경고 0(도중 발견한 2건 — `sort_by`→`sort_by_key`, `min().max()`→`clamp()` — 수정 완료).
- 문서: `docs/architecture/project-feature-design.md`, `docs/contracts/project-management.md`, `docs/architecture/project-task-agent-lifecycle.md`의 "현재 구현" 관련 절을 1단계 실제 범위에 맞게 갱신(`implementation: proposed → partial`, 각 표에 실제 구현 여부 명시).

## 2026-08-24 — `#48`(2단계) 두 표면의 project_id 비대칭 해소와 규칙 단일화

- 유형: `implementation` + `verification(browser + real Postgres)`
- 1단계를 마치며 문서에 "HTTP `POST /api/tasks`는 아직 project_id 입력을 받지 않고, MCP `fleet_dispatch_task`만 이 검증을 거친다"고 적어 뒀는데, 이건 계약 문서(`docs/contracts/project-management.md`)의 활성화 게이트 "Dashboard와 MCP의 동일한 권한·오류 응답 검증"을 정면으로 어기는 상태다. 2단계는 그 비대칭을 닫는 것부터 시작했다.
- **규칙 단일화 (`fleet_store::project_rules` 신설)**: Dashboard에 검증 로직을 그대로 복사하려다, 1단계에서 이미 archive 절차를 Dashboard와 MCP에 복제해 뒀다는 걸 떠올려 멈췄다 — 세 번째 복제를 만들기 전에 추출하는 게 맞다고 판단했다. `ensure_project_accepts_new_tasks`(제출 시 존재·상태 검증)와 `advance_project_archive`(idempotent `Active → Draining → Archived` 진행)를 `fleet-store`에 한 번만 두고, 두 표면은 결과를 각자의 에러 타입(`ApiError` / `JsonRpcError`)으로 옮기기만 한다. `fleet-core`가 아니라 `fleet-store`에 둔 이유는 둘 다 `Store` 조회가 필요한데 `fleet-core`는 의존성 없는 leaf 크레이트이기 때문이다. `advance_project_archive`는 상태 전이를 콜백으로 알려 준다 — Dashboard는 그걸 감사 이벤트로 기록하고 MCP는 (아직 감사 파이프라인이 없어) 무시한다. 1단계에 복제해 뒀던 두 벌의 archive 코드는 이 공유 구현 호출로 되돌렸다.
- **이어가기의 Project 상속 (`Task::inherit_from_parent`)**: 이어가기가 부모의 `project_id`를 물려받지 않으면 한 thread가 절반은 Project 안, 절반은 일반 풀에 걸쳐 Project 경계 자체가 의미를 잃는다. `server_hint`/`cwd`/`model`과 같은 우선순위(명시 값 > 부모 값 > None)로 상속하도록 했다. **중요한 건 상속된 값도 검증 대상이라는 점** — 부모가 속한 Project가 그 사이 `Draining`/`Archived`가 됐으면 이어가기도 거절돼야 한다(닫힌 Project는 새 Task를 받지 않는다). 그래서 검증을 `Task::from_request` 직후가 아니라 `inherit_from_parent` 이후에 배치했고, 이 순서 의존성을 `inherit_from_parent`의 doc comment에 "호출부 주의"로 명시했다(이 메서드는 값을 채우기만 하고 검증하지 않는다).
- **UI**: `TaskSummary`에 `project_id`를 노출하고(대시보드가 설정할 수 있게 됐으니 보여줄 수도 있어야 한다), New Task 폼에 Project 드롭다운을 추가했다. 드롭다운은 `api/projects`에서 `status === 'active'`만 채운다 — draining/archived를 고르면 서버가 어차피 400으로 거절하므로 애초에 고를 수 없게 하는 편이 낫다. `project:read` 권한이 없으면 403을 받고 "None"만 남으며 hint가 그 사실을 알려 준다. 기존 JS가 빈 optional 필드를 payload에서 제거하므로 "None" 선택은 그대로 `project_id: None`이 된다.
- **실제 브라우저 검증 (이 세션에서 반복해 막혔던 제약의 해법)**: `fleet serve`의 MCP stdio가 stdin EOF에서 프로세스 전체를 종료시키는 문제 때문에 `#75`/`#76`/`#63` 검증 때 실제 기동 확인을 계속 포기했었는데, 이번에 `tail -f /dev/null | exec ./target/debug/fleet serve ...`로 우회했다 — stdin이 열린 채 데이터가 오지 않으므로 MCP 루프가 살아 있고 대시보드도 계속 뜬다. 이걸로 실제 Postgres(`fleet_ui_test`) + 대시보드를 띄워 bootstrap → 로그인 → New Task 페이지까지 브라우저로 진행했다:
  - 드롭다운이 active 2건(`acme-web`, `billing-service`)만 알파벳순으로 표시하고 archived 1건(`legacy-shut`)을 제외함을 DOM에서 직접 확인
  - 실제 폼 제출로 task가 생성되고 `project_id`가 올바르게 저장됨을 DB `JOIN`으로 확인(`UI verification task | 67499d41-... | acme-web`), `/api/tasks` 응답에도 그 값이 실림
  - archived project(`400`, "project 'legacy-shut' is archived and does not accept new tasks")와 미존재 project(`400`, "no such project: ...") 제출이 거절되고 task 행이 생성되지 않음(`count = 1` 유지)을 실측
  - 검증 후 서버 종료·`fleet_ui_test` DB 삭제까지 정리 완료
- 신규 테스트: `crates/fleet-store/src/project_rules.rs` 6건(admit/not-found/draining·archived 거절, archive 즉시 완료·draining 유지·idempotent 재호출 — 감사 콜백이 no-op 재호출 때 발생하지 않는 것까지 확인), `crates/fleet-core/src/task.rs` 3건(상속, 명시 값 우선, 부모가 없으면 None 유지), `crates/fleet-dashboard/tests/dashboard_api.rs` 6건(project_id 기록 + 응답 노출, 미존재/archived/malformed 거절, 이어가기 상속, 닫힌 Project로의 이어가기 거절).
- 검증: `cargo check --workspace --all-features`, `cargo test --workspace`(98 스위트 전체 `ok`, 0 FAILED), `DATABASE_URL`+`FLEET_MCP_CAPABILITIES` 주입 재실행(사전 존재 버그 `unknown_tool_returns_error` 1건 제외 전체 통과 — `task_78be5260`으로 이미 분리됨), `cargo check --no-default-features`, `cargo clippy --all-targets --all-features` 경고 0.

## 2026-08-24 — `#88` Issue 엔티티와 상태 머신

- 유형: `implementation` + `verification`
- `#48`(1·2단계)를 마치면서 선행 조건(`#48`·`#73`·`#76`)이 모두 충족돼 착수했다. `docs/architecture/issues.md`가 상세한 설계 정본을 이미 갖고 있어, 이번 작업은 "설계 결정"이 아니라 "그 설계 중 지금 채울 수 있는 부분을 정확히 골라 구현"하는 일이었다.
- **범위 선정**: 설계 문서는 dedup key, `occurrence_count`, claim lease, Project 예산, 계보 깊이 상한까지 다루지만 그 대부분은 Agent(`#89`/`#93`)와 archive hold(`#91`)에 달려 있다. 로드맵이 `#88`에 배정한 것은 엔티티·상태 머신·연관까지이고, 표면 노출은 `#92`가 따로 소유한다. 그 경계를 그대로 지켰다 — 채울 방법이 없는 컬럼을 미리 만들지 않는 원칙(`#48`/`#70`)을 적용해 `author_kind`/`dedup_key`/`origin_attempt_id`도 만들지 않았고, `issue:archive_hold_manage` capability도 토글 대상 테이블이 없어 제외했다. 무엇을 왜 미뤘는지는 설계 문서에 "구현 상태" 절을 새로 만들어 표로 남겼다.
- **`tasks`에 `issue_id`를 두지 않았다** — 설계의 핵심 결정이다. 넣는 순간 Task 상태 머신이 Issue를 읽어야 하는 압력이 생기고 두 상태 머신이 경쟁한다. 연관은 `issue_task_links` join 테이블이 소유하며, `task_id`는 `SET NULL` + `task_label` 보존이다(`011_audit_log.sql`의 `actor_label`과 같은 패턴 — 어떤 Task와 엮여 있었는지는 Issue 이력의 일부라 Task와 함께 사라지면 안 된다). `fleet-core/src/issue.rs`가 `crate::task`를 import하지 않는 것이 I1의 구조적 근거이고, 이 사실을 모듈 문서에 명시했다.
- **`InProgress` 부재를 세 겹으로 고정**: enum에 없고(당연), `IssueStatus::ALL`을 순회해 `"in_progress"`가 어떤 variant의 문자열도 아니며 `parse_str`이 거절함을 테스트하고, DB CHECK 제약이 `in_progress` INSERT를 거절한다. 세 번째가 필요한 이유는 애플리케이션 enum에 없어도 DB에 직접 쓰는 경로가 있으면 값이 들어올 수 있어서다 — 실제 Postgres에 수동 INSERT를 시도해 `issues_status_check` 위반으로 거절되는 것을 확인한 뒤 테스트로 고정했다.
- **`close_reason` 정합성도 양쪽에**: `Closed`일 때만·그때는 반드시 있어야 한다는 불변식을 `Issue::transition_to`(애플리케이션)와 `issues_close_reason_matches_status` CHECK(DB) 양쪽에 뒀다. reopen 시 이전 종결 사유를 지우는 것도 같은 이유다 — 끌고 다니면 "왜 닫혔었나"와 "지금 왜 열려 있나"가 뒤섞인다.
- **capability 분리를 저장소 API에서도 유지**: 계약이 `issue:update`(오탈자 수정)와 `issue:close`(문제 종결)를 분리한 만큼, `Store`도 `update_issue_fields`(상태 제외)와 `transition_issue`(상태 전용)로 갈랐다. 호출부가 실수로 한 메서드에 둘 다 태우지 못하게 하기 위함이고, 로컬 사본의 `status`를 `Closed`로 조작해 `update_issue_fields`를 호출해도 저장된 상태가 바뀌지 않는 것을 테스트로 고정했다.
- **"진행 중"은 파생 값**: `issue_has_active_tasks`를 Store에 뒀지만 문서에 "이 값을 Issue 상태로 저장하지 않는다"고 명시했다 — 저장하는 순간 Task 상태의 복제본이 생기고, 그게 `InProgress`를 금지한 이유 그 자체다. I2를 깨지 않는다는 것도(close 경로가 이 메서드를 호출하지 않는다) doc에 적었다.
- **교착 없음(게이트 2)을 두 방향 모두 실증**: `ReadyForAgent` 상태의 Issue에 연관된 Task가 아무 방해 없이 터미널까지 도달하고(I1), 비터미널 Task가 연관돼 있어도 Issue close가 성공하며 그 Task는 여전히 살아 있다(I2). 두 테스트 다 "선행 조건이 실제로 성립하는지"를 먼저 assert한 뒤 본 검증을 한다 — 그러지 않으면 조건이 안 갖춰진 채 통과하는 공허한 테스트가 된다.
- **게이트 10(MemStore/PgStore 공유 행동)이 테스트 파일의 구조를 정했다**: 모든 시나리오를 `Arc<dyn Store>`를 받는 클로저로 쓰고 `both_backends!` 매크로로 두 백엔드에 각각 돌린다. `DATABASE_URL`이 없으면 PgStore 쪽만 skip되고 MemStore 쪽은 항상 실행되므로, 두 구현이 갈라지는 것을 DB 없는 CI에서도 절반은 잡는다. PgStore가 실제로 실행됐는지는 테스트 후 남은 행을 psql로 직접 조회해 확인했다(조용히 skip되어 "통과"로 보이는 위험이 이 저장소에 전례가 있다 — `integration.rs`의 `try_connect` 주석 참고).
- 신규 테스트: `fleet-core::issue` 13건(전이표 전체를 훑어 허용 간선 집합이 계약 다이어그램과 정확히 일치하는지 확인하는 것 포함 — 간선을 하나 더 열거나 지우면 잡힌다), `crates/fleet-store/tests/issues.rs` 14건(11건은 두 백엔드 공유, 3건은 DB CHECK 전용).
- 검증: `cargo check --workspace --all-features`, `cargo test --workspace`(99 스위트 전체 `ok`, 0 FAILED), `DATABASE_URL`+`FLEET_MCP_CAPABILITIES` 주입 재실행(사전 존재 버그 `unknown_tool_returns_error` 1건 제외 전체 통과 — `task_78be5260`으로 이미 분리됨), `cargo check --no-default-features`, `cargo clippy --all-targets --all-features` 경고 0.

## 2026-08-24 — `#92`(Issue 표면) Issue 관리 HTTP API

- 유형: `implementation` + `verification`
- `#88`로 Issue 엔티티는 생겼지만 만들거나 볼 방법이 없었다 — `#48` 1단계 직후와 같은 상황이라 표면을 붙였다.
- **`#92`를 갈라 진행한 근거**: 이 항목은 서로 독립인 두 도메인(AgentTemplate, Issue)을 "관리 표면"이라는 주제로만 묶은 것이다. Issue 쪽 선행(`#73`·`#88`)은 모두 완료됐고 AgentTemplate 쪽만 `#86`에 막혀 있어, 그 경계로 갈라 Issue만 먼저 노출했다. 항목 설명이 선행 조건으로 적어 둔 `ApiError`의 422/428/429 확장은 **필요하지 않았다** — 그 요구는 AgentTemplate revision의 낙관적 동시성(`If-Match` → 409/428)과 rate limit(429)에서 오는 것이고, Issue 표면은 400/403/404/409로 충분한데 기존 `ApiError`가 넷 다 갖고 있다. 확장을 "선행"이라고 적어 둔 것을 그대로 따랐다면 필요 없는 작업을 먼저 했을 것이다.
- **상태 전이를 `PATCH`와 분리한 별도 endpoint로 만든 것이 이 표면의 핵심 설계다.** 목표 상태마다 요구 capability가 다르기 때문이며, 저장소 계층에서 이미 `update_issue_fields`/`transition_issue`로 갈라 둔 것(`#88`)과 같은 이유다. 매핑은 `handlers::required_capability_for_transition` 단일 함수가 소유하고 `pub`으로 노출했다 — MCP 표면이 생기면 그대로 재사용해 계약의 "두 표면 동일 동작" 요구를 구조적으로 만족시키기 위함이다(Project에서 `fleet_store::project_rules`로 했던 것과 같은 접근).
- **전이별 capability 매핑에서 계약 해석이 갈린 두 지점**:
  - **승인 철회(`ReadyForAgent → Triaged`)에 `issue:approve_agent_work`를 요구하지 않기로 했다.** 계약이 그 capability를 `Triaged → ReadyForAgent` 한 방향으로만 정의했고, 더 중요하게는 권한을 회수하는 쪽이 부여하는 쪽보다 어려우면 잘못된 승인을 되돌리기가 더 힘들어진다 — 안전한 방향으로 실패하지 않는 설계가 된다. `issue:update`로 매핑했다.
  - **`→ Resolved`도 `issue:close`로 매핑했다.** capability 목록에 "resolve"가 없어 `update`와 `close` 중 골라야 했는데, `Resolved`는 텍스트 편집이 아니라 "이 문제가 처리됐다"는 판정이다. 계약이 close를 update에서 분리한 이유가 "오탈자 수정 권한이 문제 종결 권한을 함께 주면 안 된다"이고, 그 논리가 Resolved에도 그대로 적용된다.
  - 두 결정 모두 테스트에 `assert_ne!`로 명시해 뒀다 — 나중에 누가 "당연히 approve겠지"/"당연히 update겠지"로 바꾸면 잡힌다.
- `assignee` 변경만 `issue:assign`을 **추가로** 요구한다 — 다른 필드 수정에까지 요구하면 계약이 assign을 별도 capability로 분리한 의미가 없어진다. 요청 본문에 `assignee` 키가 있을 때만 검사한다.
- `has_active_tasks`는 저장하지 않고 조회 시점에 계산해 응답에 싣는다 — `InProgress` 상태를 두지 않은 이유가 정확히 그것이며(`#88`), 표면에서도 그 원칙을 지켰다. Task를 연관지어도 Issue의 `status`는 그대로이고 배지만 켜지는 것을 테스트로 고정했다.
- `Draining` Project에도 Issue를 만들 수 있게 했다 — 계약의 "`Draining` 중에도 Issue 쓰기는 허용하고 claim과 Issue→Task 생성만 막는다"를 따른다. 생성 시 Project **존재**만 확인하고 상태는 보지 않는다(Task 제출이 `active`를 요구하는 것과 의도적으로 다르다).
- 신규 감사 액션 `issue.create`/`issue.transition`. 전이는 `detail.to`에 목표 상태를 남긴다 — `ready_for_agent` 승인은 Agent 자동 착수의 인가 지점이므로 누가 승인했는지가 감사에 남아야 한다.
- 신규 테스트 16건: 전이별 capability 매핑을 계약과 대조하는 단위 테스트, 전체 lifecycle 왕복(Open→Triaged→ReadyForAgent→Resolved→Closed→reopen), 허용되지 않는 간선 409, 사유 없는 close 409, `issue:update`만 가진 principal이 승인·종결에 403을 받고 **저장된 상태도 안 바뀌는 것**, PATCH 본문에 `status`/`close_reason`을 끼워 넣어도 상태가 안 바뀌는 것, assignee만 추가 권한, 코멘트 왕복, Task 연관이 파생 배지를 켜되 status는 그대로, 연관 해제, read 권한 403, CSRF 403.
- 검증: `cargo check --workspace --all-features`, `cargo test --workspace`(99 스위트 전체 `ok`, 0 FAILED), `DATABASE_URL`+`FLEET_MCP_CAPABILITIES` 주입 재실행(사전 존재 버그 `unknown_tool_returns_error` 1건 제외 전체 통과 — `task_78be5260`으로 이미 분리됨), `cargo check --no-default-features`, `cargo clippy --all-targets --all-features` 경고 0.
- **남은 것**: AgentTemplate 표면(`#86` 선행), Issue의 MCP 표면, `issue:archive_hold_manage`(`#91` 선행), Dashboard UI 화면(`docs/ui-dashboard/ui-design.md` 소유 — 이번엔 JSON wire 표면만 냈다).

## 2026-08-24 — `#48`(3단계) Project Dashboard 화면과 UI 문서 정정

- 유형: `implementation` + `verification(browser + real Postgres)` + `documentation`
- 1·2단계로 Project API를, `#88`/`#92`로 Issue 엔티티와 API를 쌓았지만 대시보드에서는 아무것도 보이지 않았다 — 사용자 입장에서 그 작업 전체가 없는 것과 같았다. UI 설계 문서(§3.9·§3.10)에 화면 명세가 있어 그걸 따라 붙였다.
- **구현하려다 문서 모순을 발견했다.** `ui-design.md` §3.9(2026-08-14)는 목록에 "Host/Worker 배정 수"를 표시하라며 `list_project_hosts`/`list_project_worker_ids` 카운트를 데이터 소스로 지정한다. 그런데 모델 정본 `project-feature-design.md`의 공유 실행 풀 불변식은 **"Host와 Worker에는 `project_id`를 두지 않는다. 하나의 Worker는 시간에 따라 여러 Project의 Agent를 실행할 수 있다"**고 못 박는다. 화면 명세가 모델 정본이 금지한 데이터를 요구하는 상태였다 — UI 문서가 공유 실행 풀 결정(문서상 2026-08-17)보다 앞서 작성돼 갱신되지 않은 것으로 보인다. 명세를 그대로 구현하면 정본과 충돌하므로 **화면은 모델을 따르고 UI 문서의 해당 행을 폐기 표시**했다(정정 사유와 근거 문서를 인용해 남겼다). 문서 정책상 화면 정본이라도 모델이 금지한 데이터를 요구할 수는 없다.
  - 같은 이유로 명세가 생성 폼에 요구하는 `agent_provisioning_mode`/`workdir_template`/`default_agent_template_id`/`agent_idle_timeout_secs`도 넣지 않았다 — 1단계가 Agent·AgentTemplate 부재를 이유로 의도적으로 제외한 정책 필드들이다. 무엇을 왜 뺐는지 UI 문서에 "구현 상태" 절을 만들어 남겼고, 그 필드들이 생기면 폼에 추가하라고 적었다.
- 화면 셋: `/projects`(목록 — Name/Status/Description/Created by/Created, 기존 `/hosts`·`/tasks`의 컬럼 정렬·10초 폴링 관례 재사용), `/projects/new`(name+description 단일 폼 — 명세의 "마법사 아님, `/tasks/new` 컨벤션 재사용"을 따름), `/projects/:id`(메타데이터 + 이 Project의 Issue 목록 + Task 목록 + archive 액션).
- **archive의 부분 진행 상태를 화면에 그대로 드러냈다**: 비종료 Task가 남아 있으면 archive는 `draining`에 머무는데(1단계 게이트), 그 경우 "Draining — tasks still running; archive completes once they finish."를 표시하고 버튼을 "Retry archive"로 바꾼다. 성공/실패 이분법으로 뭉개면 사용자가 왜 안 끝났는지 알 수 없다.
- **권한을 페이지 수준에서 막았다**: `project:create`가 없으면 `/projects/new` 페이지 자체가 403이다 — 폼을 다 채우게 한 뒤 제출에서 거절하는 것보다 낫다. Issue 목록도 `issue:read`가 없으면 빈 목록이 아니라 "Not permitted"를 표시한다(권한 없음과 데이터 없음을 구분).
- **사이드바 동기화 문제**: 12개 HTML 파일에 사이드바가 손으로 복제돼 있고 `<!-- sidebar:start -->` 마커는 있지만 **그걸 읽는 코드가 없다**(자동화 의도만 남고 구현은 없는 상태). 링크 하나를 추가할 때 일부 파일을 빠뜨리기 쉬워서, 스크립트로 일괄 추가한 뒤 **모든 페이지가 Projects 링크를 갖는지 강제하는 테스트**를 넣었다 — 다음에 누가 링크를 추가할 때도 같은 실수를 잡는다.
- **실제 브라우저 검증** (`tail -f /dev/null | fleet serve` 우회 재사용): 실제 Postgres + 대시보드를 띄워 bootstrap→로그인 후 — 목록이 두 Project를 배지·설명과 함께 렌더링하는 것, 상세가 그 Project의 Issue와 Task를 정확히 필터링해 보여주는 것, archive 클릭이 pending task 때문에 `draining`에 머물며 사유를 표시하고 버튼이 "Retry archive"로 바뀌는 것, task를 종결시킨 뒤 재시도하니 `archived`로 넘어가고 버튼이 사라지는 것, 생성 폼으로 실제 Project가 DB에 저장되며 `created_by`가 세션 사용자로 기록되는 것까지 전부 확인했다. 검증 후 서버·테스트 DB 정리 완료.
- 신규 테스트 4건: 권한 충분 시 세 페이지 200, `project:read` 없으면 목록 403, `project:create` 없으면 생성 폼만 403(목록은 200), 모든 사이드바 페이지의 Projects 링크 존재.
- 검증: `cargo test --workspace`(99 스위트 전체 `ok`, 0 FAILED), `cargo check --no-default-features`, `cargo clippy --all-targets --all-features` 경고 0. JS는 `node -e "new Function(...)"`으로 파싱 확인.

## 2026-08-24 — `#92`(Issue MCP 표면)과 공유 규칙의 층위 정정

- 유형: `implementation` + `refactor` + `verification`
- `#92` HTTP 표면을 내면서 "MCP는 남았다"고 적어 뒀고, 계약의 활성화 게이트가 "Dashboard와 MCP의 동일한 권한·오류 응답"을 요구하므로 그 비대칭을 닫았다.
- **층위 오류를 발견해 바로잡았다**: HTTP 표면 작업에서 `required_capability_for_transition`을 `fleet-dashboard`에 두고 "MCP가 재사용할 수 있게 `pub`으로 노출했다"고 적었는데, **`fleet-mcp`는 `fleet-dashboard`에 의존하지 않는다**(대시보드는 leaf crate다). 재사용이 애초에 불가능한 배치였다. `IssueStatus`와 `PermissionKind` 둘 다 `fleet-core`에 있고 Store 조회가 전혀 필요 없는 순수 함수이므로 `fleet-core::issue`로 옮겼다 — surface 크레이트에 공유 도메인 정책을 두는 것이 처음부터 잘못이었다. 이제 두 표면이 같은 구현을 참조해 계약의 "동일 동작"이 구조적으로 보장된다.
- **인자 의존 인가**라는 새 문제를 다뤘다: MCP 서버의 인가는 `required_permission(tool_name) -> Option<PermissionKind>` 행렬인데, `fleet_transition_issue`는 요구 capability가 **목표 상태(인자)에 따라 달라** 도구 이름만으로 판정할 수 없다. 두 단계로 나눴다 —
  - `permits_tool`은 "전이 권한을 **하나라도** 가졌는가"로 도구 **노출**만 결정한다. 하나도 없으면 도구가 `tools/list`에 나오지 않고 호출도 거절된다(fail-closed).
  - **정확한 판정은 핸들러**가 한다. 이를 위해 `ToolContext`에 `capabilities`를 실었고, 기본값을 **빈 집합**으로 뒀다 — 명시적으로 부여하지 않으면 인자 의존 도구가 전부 거절되므로, 나중에 다른 곳에서 `ToolContext::new`를 쓰다 인가를 빠뜨려도 열리는 방향으로 실패하지 않는다.
  - 거절 메시지에 어떤 capability가 없는지 명시한다 — "권한 없음"만 던지면 launcher 설정을 고칠 수 없다.
- 도구 넷: `fleet_list_issues`(파생 `has_active_tasks` 포함), `fleet_create_issue`, `fleet_transition_issue`, `fleet_comment_issue`. 도구 설명에 "인프라 alert이 아니라 프로젝트 일감"이라는 구분과 "`in progress` 상태가 의도적으로 없다"는 사실을 적었다 — MCP 클라이언트(AI 어시스턴트)가 그 구분을 모르면 워커 장애를 Issue로 열려 할 수 있다.
- `Draining` Project의 Issue 생성은 Dashboard와 동일하게 허용한다(Project 존재만 확인, 상태는 보지 않음) — 계약의 "`Draining` 중에도 Issue 쓰기는 허용"을 따른다.
- 신규 테스트 6건(fleet-mcp): 생성·목록·코멘트 왕복, 미존재 project 거절, 상태 기계 준수(허용 안 되는 간선·사유 없는 close), **전이별 capability 판정**(`issue:update`만 가진 launcher가 triage는 되고 승인·종결은 거절되며 저장된 상태도 안 바뀜 + 거절 메시지가 없는 capability를 명시), 미존재 issue 처리. 추가로 server 단위 테스트 1건: 전이 권한이 하나도 없으면 도구 자체가 노출되지 않고, 넷 중 하나라도 있으면 노출됨.
- 검증: `cargo test --workspace`(99 스위트 전체 `ok`, 0 FAILED), `cargo check --no-default-features`, `cargo clippy --all-targets --all-features` 경고 0.

## 2026-08-25 — `#58` Project scope: `issue:link` confused-deputy(리소스 간 경계) 차단

- 유형: `implementation` + `verification`
- **범위를 좁힌 근거**: `#58`이 원래 요구하는 "principal이 자기 Project만 본다"는 지금 착수할 수 없다 — `project-management.md`의 활성화 게이트가 이미 "Project scope는 아직 모든 인증된 호출자가 전체 Project를 본다(RBAC의 Project 단위 scope는 미구현)"고 명시하며, `fleet-core::auth`에 User를 특정 Project로 한정하는 멤버십 개념 자체가 없다. 지금 "principal이 이 Project에 속하는가" 검사를 추가해도 비교 대상이 없어 항상 통과하는 죽은 분기가 될 뿐이다(원칙: 채울 방법이 없는 것은 미리 만들지 않는다). 대신 `#58`이 같은 표제 아래 명시한 confused-deputy 중, 멤버십 모델 없이도 지금 닫을 수 있는 **리소스 간 경계** 문제 하나로 좁혔다.
- **발견한 구멍**: `POST /api/issues/:id/links`(`link_issue_task_api`)는 요청받은 `task_id`가 존재하기만 하면 그 Task가 어느 Project 소속인지 검사하지 않고 그대로 연결한다. 호출자가 Issue A(Project P1)에 Project P2 소속 Task의 id를 추측·나열해 넣으면, 링크 성공 여부와 이후 그 Task의 label 노출을 통해 자신이 볼 권한이 없는 Project의 Task 존재를 알아낼 수 있다 — 전형적인 confused-deputy다(대상 계약: `docs/security/authorization-and-audit.md` 게이트 3 "Task ID만으로 Project scope를 우회하거나 존재를 열거하지 못한다").
- **고친 위치와 이유**: `fleet-core::issue`가 아니라 `fleet-dashboard::handlers::link_issue_task_api`에 뒀다. `fleet-core/src/issue.rs`는 `crate::task`를 import하지 않는 것 자체가 I1("Task/Attempt 전이가 `issue.status`를 읽지 않는다")의 구조적 증거라고 모듈 문서에 명시돼 있다(`#88`) — 여기에 Task 타입을 끌어들이는 검사를 추가하면 그 증거가 무너진다. 두 Store 구현(`mem.rs`/`postgres.rs`)의 `link_issue_task`도 검사가 없지만, 호출자가 이 핸들러 하나뿐이라 핸들러가 유일하고 충분한 강제 지점이다.
- **일반 풀 Task(`project_id: None`)는 대상에서 제외했다** — 애초에 어느 Project에도 속하지 않으므로 경계를 넘을 대상이 없고, 기존 동작·기존 테스트와도 일치한다.
- **거절 응답을 "Task 없음"과 완전히 동일하게 만들었다**(`ApiError::BadRequest("no such task: {task_id}")` 재사용) — "다른 Project 소속"이라는 사실 자체가 노출이므로, 존재하지 않는 Task와 다른 Project 소속 Task를 호출자가 구분할 수 없어야 한다. 신규 테스트는 **같은 `task_id`를 두 세계 상태(없음 → 다른 Project에 존재함)에 순서대로 재사용**해 두 응답을 바이트 단위로 비교한다 — 처음에 서로 다른 id로 비교했다가 두 UUID가 메시지에 반향돼 실패한 것을 발견하고(정상 동작, 정보 노출 아님) 같은 id 재사용으로 바로잡았다.
- **DB 주입 품질 게이트 진행 중 이번 변경과 무관한 기존 특성을 실증했다**: `cargo test -p fleet-store --all-features`를 한 번에 돌리면 `audit_integration`(6~7건) 또는 `admin_token_rotation`(2건) 등 실행 순서에 따라 매번 다른 조합이 FK 위반·행 오염으로 실패한다. 원인은 이 크레이트의 DB 통합 테스트 11개 바이너리가 전부 같은 `fleet_test` Postgres를 `TRUNCATE ... CASCADE`로 초기화하는데, cargo가 여러 바이너리를 동시에(그리고 바이너리 안에서도 여러 테스트 함수를 동시에) 실행해 TRUNCATE와 INSERT가 서로 경합하기 때문이다 — 내 변경과 무관한, 이 저장소 DB 테스트 하네스의 기존 특성이다(`audit_integration.rs`는 자체 doc comment로 `--test-threads=1`을 이미 요구하지만 다른 10개 파일에는 그 경고가 없었다). **11개 DB-gated 바이너리를 각각 별도 프로세스로, `--test-threads=1`로 격리 실행**해서야 전부 `ok`를 확인했다 — `cargo test -p fleet-store --all-features` 한 번의 결과만으로는 이 크레이트의 DB 게이트를 신뢰할 수 없다는 뜻이므로, 다음에 이 크레이트를 검증할 에이전트를 위해 여기 남긴다.
- 신규 테스트 1건(`fleet-dashboard`): `linking_a_task_from_another_project_is_rejected_like_a_missing_task` — 위 존재 은닉 비교에 더해, 거절된 링크가 실제로 저장되지 않았음을 `GET /api/issues/:id/links`로 확인한다.
- **남은 것**: principal 단위 Project scope(호출자가 자기 Project만 보는 것)는 여전히 미착수 — 멤버십 모델의 설계 승인이 선행돼야 한다(`docs/roadmap/roadmap.md` `#58` 행에 근거를 남겼다).
- 검증: `cargo test --workspace`(전체 `ok`, 0 FAILED), `DATABASE_URL`+`FLEET_MCP_CAPABILITIES` 주입 재실행 — `fleet-store`(DB-gated 11개 바이너리 각각 격리 실행 전부 `ok`), `fleet-api`(`bootstrap_token_dump` 격리 실행 `ok` + 나머지 일괄 `ok`, 다른 바이너리는 DB 미사용), `fleet-scheduler`(`scaleout_sync` `--test-threads=1` `ok`), `fleet-mcp`(`fleet` 바이너리 재빌드 후 `cross_client` — 사전 존재 버그 `unknown_tool_returns_error` 1건 제외 전체 `ok`, `task_78be5260`으로 이미 분리됨), `fleet-dashboard`(DB 불필요·MemStore만 사용, 신규 테스트 포함 전체 `ok`) — `cargo check --no-default-features`, `cargo clippy --all-targets --all-features` 경고 0.

## 2026-08-25 — `#58` 후속: Project 경계 술어 추출, 그리고 `main`의 CI fmt 게이트 실패 발견

- 유형: `refactor` + `verification`
- **후속 리팩터**: `6fd45d6`이 `link_issue_task_api` 안에 인라인으로 넣었던 Project 경계 비교를 `fleet_store::project_rules::task_project_matches_issue_project` 순수 술어로 올리고 단위 테스트 3건(일반 풀 Task 허용 / 같은 Project 일치 / 다른 Project 불일치)을 붙였다. 동작 변화는 없다.
- **추출 근거를 정직하게 다시 썼다**: 처음 작성한 주석은 "Dashboard와 (링크 도구가 생기면) MCP가 같은 판정을 공유해야 하므로"라고 적었지만, **오늘 MCP에는 링크 도구가 없다**(`fleet-mcp/src/schema.rs`의 issue 도구는 `list`/`create`/`transition`/`comment` 넷뿐). 존재하지 않는 소비자를 근거로 드는 것은 "채울 방법이 없는 것은 미리 만들지 않는다" 원칙과 충돌하므로, 주석을 **"오늘 호출부는 Dashboard 하나뿐이며 MCP에는 링크 도구가 없다"**를 명시하는 문구로 고쳤다. 그럼에도 surface 크레이트에 두지 않는 이유는 `#92`에서 Dashboard에 넣어 둔 Issue 전이 규칙을 MCP 표면이 생긴 뒤에야 `fleet-core`로 옮겨야 했던 비용을 반복하지 않기 위해서라고 근거를 바꿔 적었다 — 이건 오늘 참인 진술이다.
- **`Task::project_id` 주석 정정**: "project 그룹화 도입 전까지 항상 `None`"이라는 낡은 서술을 지웠다. `#48` 이후 `submit_task_api`가 실제로 채우므로 거짓이었다. 아울러 `inherit_from_parent`가 정말 `project_id`를 상속하는지 코드와 기존 테스트(`inherit_from_parent_adopts_project_id`)로 확인했다 — 상속한다. 상속하지 않았다면 Project 소속 Task의 이어가기가 `None`으로 떨어져 `#58`이 막 닫은 경계에 구멍이 남았을 것이므로, 주석을 믿지 않고 확인한 값이 있었다.
- **`main`이 CI 첫 단계에서 이미 실패하는 상태임을 발견했다**: 이 변경에 `cargo fmt`를 돌렸다가 33개 파일 / 112곳이 재포맷되는 것을 보고 확인했다. 깨끗한 HEAD(`6fd45d6`)에서 `cargo fmt --all -- --check`가 exit 1이며, `.github/workflows/ci.yml:57`은 이 명령을 clippy보다 **먼저** 실행한다. 위반은 `#48`/`#92` 시기 코드에 퍼져 있다(`fleet-dashboard/src/handlers.rs` 7곳, `fleet-mcp/src/handlers.rs` 7곳, `fleet-api/src/handlers.rs` 4곳 등). 로컬 rustfmt·CI 모두 `stable`이라 버전 차이가 아니다.
  - **원인 추정**: 이 저장소에서 관행적으로 돌려 온 품질 게이트 4종(`cargo test --workspace` / DB 주입 재실행 / `cargo check --no-default-features` / `cargo clippy --all-targets --all-features`)에 **`cargo fmt --check`가 빠져 있다**. CI는 검사하는데 로컬 게이트는 안 하니 위반이 누적됐다.
  - **이 커밋에 섞지 않았다**: 로직 변경과 33개 파일 대량 재포맷이 한 커밋에 들어가면 리뷰가 불가능해진다. `cargo fmt`가 건드린 파일을 전부 되돌리고 내 편집만 손으로 다시 적용했으며, 위반 개수를 변경 전후로 비교해 **내가 추가한 코드가 새 fmt 위반을 0개 추가**함을 확인했다. 별도 `style:` 커밋으로 분리하는 것은 사용자 승인 대기 중이다.
- 검증: `cargo test --workspace`(exit 0, 전체 `ok`), `DATABASE_URL`(`postgres://yarang@localhost:5432/fleet_test`)+`FLEET_MCP_CAPABILITIES` 주입 재실행 — `fleet-store` DB-gated 11개 바이너리 각각 별도 프로세스 `--test-threads=1` 전부 `ok`(116건), `fleet-api`/`fleet-scheduler`/`fleet-dashboard` 전부 `ok`, `fleet-mcp`는 사전 존재 버그 `unknown_tool_returns_error` 1건만 FAILED(`task_78be5260`, 이번 변경과 무관) — `cargo check --no-default-features` 통과, `cargo clippy --all-targets --all-features` 경고 0.
  - **검증 한계**: 로컬 Postgres가 내려가 있어 `pg_ctl`로 직접 기동했는데 `postmaster became multithreaded during startup`으로 한 번 실패했다(macOS 로케일 문제). `LC_ALL=C LANG=C`로 기동해 해결했다 — `brew services start`는 `launchctl bootstrap` 오류로 실패하므로, 이 환경에서 DB 게이트를 돌릴 다음 에이전트는 `env LC_ALL=C LANG=C pg_ctl -D /opt/homebrew/var/postgresql@16 start`를 쓰는 편이 빠르다.

## 2026-08-25 — `main`의 CI fmt 게이트 복구, 그리고 게이트 목록을 CI와 일치시킴

- 유형: `lint` + `verification`
- **무엇을 했나**: `cargo fmt --all`로 31개 파일을 재포맷해(`691 insertions(+), 481 deletions(-)`) `main`이 CI 첫 단계(`.github/workflows/ci.yml:57`)에서 실패하던 상태를 해소했다. 직전 항목에서 발견해 사용자 승인 후 **로직 변경과 분리한 순수 포맷 커밋**이다.
- **순수 포맷임을 증명했다**: `git diff -w`로는 부족하다 — rustfmt는 줄 자체를 재구성하므로 공백 무시 비교로도 차이가 남는다. 그래서 변경된 31개 파일 전부에 대해 **공백·후행 쉼표·중괄호를 제거한 토큰열**을 `HEAD`와 비교했고, 31개 모두 바이트 단위로 동일했다. 실제로 rustfmt가 한 일은 두 종류뿐이다: (1) 후행 쉼표 삽입, (2) 긴 match arm을 블록으로 감싸기. 예(`crates/fleet-store/src/rbac.rs`):
  ```rust
  -        PermissionKind::ProjectDelete => "Archive projects (request draining, not permanent delete)",
  +        PermissionKind::ProjectDelete => {
  +            "Archive projects (request draining, not permanent delete)"
  +        }
  ```
- **근본 원인을 같이 고쳤다(`agent.md` §4.3)**: 재포맷만 하면 다음 에이전트가 같은 드리프트를 다시 만든다. 로컬 게이트 목록이 CI보다 약했던 지점이 **셋**이라 전부 CI와 동일한 형태로 바꿔 적었다.

  | 항목 | 기존 `agent.md` | CI 실제 | 문제 |
  |---|---|---|---|
  | 포맷 | (없음) | `cargo fmt --all -- --check` | 검사 자체가 누락 → 112건 누적 |
  | 기본 피처 lint | `cargo clippy --all-targets --all-features` | `cargo clippy --workspace --features "acp mtls" --all-targets -- -D warnings` | `-D warnings`가 없어 **경고가 있어도 exit 0** |
  | 최소 빌드 | `cargo check --no-default-features` | `cargo clippy --workspace --no-default-features --all-targets -- -D warnings` | `check`는 컴파일 오류만 봄, lint 미검사 |

  특히 두 번째는 "clippy 경고 0을 확인했다"는 그동안의 로그 진술이 **명령만 놓고 보면 성립하지 않았다**는 뜻이다(경고가 있었더라도 exit 0이므로). 이번에 CI와 동일한 `-D warnings` 형태로 실행해 실제로 경고 0임을 확인했다.
- **작업 중 드러난 별개 결함 2건은 이 커밋에 섞지 않고 분리했다**(순수 포맷 커밋을 유지하기 위해):
  - `collect_disk_free_mb()`(`fleet-worker/src/registration.rs:536`)가 `total_space() - available_space()`, 즉 **사용 중 용량**을 계산하면서 `disk_free_mb`(여유 용량)라는 이름으로 `WorkerHost`·`fleet-api` 스키마·공개 `openapi.yaml`까지 흘려보낸다 — 모든 소비자가 뒤집힌 값을 본다. 덤으로 이 뺄셈은 debug 빌드에서 언더플로 panic이 가능하고, panic 시 `DiskCacheState`가 `Refreshing`에 영구히 갇혀 디스크 통계가 조용히 멈춘다.
  - `disk_cache_get_or_schedule_refresh_populates_background` 테스트가 flaky다(아래).
- **Flaky 테스트 1건을 규명했다**(`agent.md` §3.3 "방치 금지"): `cargo test --workspace`에서 `fleet-worker --lib`가 71 passed / 1 failed로 한 번 깨졌다. 포맷 때문인지 확인하려고 12회 반복 실행해 **3회차에 재현**했다 — `registration::tests::disk_cache_get_or_schedule_refresh_populates_background`(`registration.rs:1040`).
  - **분류: 환경적 타이밍 예산 문제이지 런타임 버그가 아니다.** 테스트는 `spawn_blocking`이 `sysinfo::Disks::new_with_refreshed_list()`를 끝내기를 100ms×100회 = **10초** 동안 기다린다. 그런데 구현 자신의 doc comment(`registration.rs` ~535, ~555)가 이 호출을 "환경에 따라 **수 초가 소요될 수 있음**(예: macOS autofs 마운트 타임아웃)"이라고 명시한다. 즉 테스트가 **구현이 명시적으로 보장하지 않는 벽시계 상한**을 단언하고 있고, 병렬 cargo 부하나 느린 마운트에서 예산을 넘긴다. 프로덕션 코드의 계약("캐시가 비면 `None` 반환, heartbeat를 블록하지 않음")은 위반되지 않았다.
  - 타임아웃을 늘리는 것은 해법이 아니다(빈도만 낮추고 스위트를 느리게 함) — 수집기를 주입 가능하게 만들어 실제 디스크 I/O 의존을 끊어야 한다.
- 검증(전부 이번 포맷 트리에서 실행): `cargo fmt --all -- --check` 통과 · `cargo clippy --workspace --features "acp mtls" --all-targets -- -D warnings` **exit 0** · `cargo clippy --workspace --no-default-features --all-targets -- -D warnings` **exit 0** · `cargo test --workspace` 통과(위 flaky 1건 제외) · `DATABASE_URL` 주입 재실행 — `fleet-store` DB-gated 11개 바이너리 각각 별도 프로세스 `--test-threads=1` 전부 `ok`(116건), `fleet-api`(16개 스위트 `ok`), `fleet-scheduler`(98건 `ok`), `fleet-dashboard`(104건 `ok`), `fleet-mcp`는 사전 존재 버그 `unknown_tool_returns_error` 1건만 FAILED(`task_78be5260`, 이번 변경과 무관).
- **검증 한계**:
  - **로컬 rustfmt는 1.9.0-stable(8bab26f4f6, 2026-07-14)이고, CI는 `dtolnay/rust-toolchain@stable`을 실행 시점에 해석한다.** CI의 stable이 더 최신이면 내 rustfmt가 만들지 않는 포맷을 요구해 fmt 검사가 그래도 실패할 수 있다. 로컬 통과를 CI 통과로 읽으면 안 된다.
  - **DB 게이트 실행 중 하네스 오류 2건을 스스로 만들었다가 잡았고, 다음 에이전트를 위해 남긴다**: (1) `cargo test -p fleet-store --test issues`는 `--all-features` 없이는 **컴파일되지 않는다** — `MemStore`가 `test-support` 피처 뒤에 있다(`lib.rs:22`). 피처를 빼면 이 바이너리만 조용히 결과 없이 지나가므로 11개 중 1개를 검증하지 않은 채 "전부 ok"로 오독하기 쉽다. (2) `FLEET_MCP_CAPABILITIES`에 존재하지 않는 이름(`task:submit`, `project:update`)을 넣었더니 fail-closed 파싱이 서버 기동을 막아 `cross_client` **12건이 전부 `initialize` 응답 없음으로 실패**했다 — 트리 회귀처럼 보였지만 전적으로 환경변수 오류였다. 정본 이름은 `fleet-core/src/auth.rs:466`의 `as_str()`이며 `task:create`/`project:read` 등이다.

## 2026-08-25 — OpenRouter 적용 가능성 판정과 gateway dialect 계약의 문서화

- 유형: `ingest`(정본 대규모 재작성) + `design` + `verification`
- **질문을 다시 정의했다**: "OpenRouter를 쓸 수 있게 고칠 수 있나"로 시작했지만, 코드를 읽어 보니 Fleet에는 LLM 설정 경로가 **둘** 있었고 둘의 성격이 달랐다. 경로 A는 worker 전역 `[llm_proxy]`가 subprocess 환경변수로 팬아웃되는 gateway 경로이고, 경로 B는 `worker_credentials` 한 행이 grok `config.toml`의 `[model.<id>]`로 렌더링되는 per-model 경로다(`fleet-credentials/src/lib.rs`, `migrations/005_worker_credentials.sql`). **경로 B로는 OpenRouter가 오늘 이미 동작한다 — Rust 변경이 0줄이다**(`base_url = "https://openrouter.ai/api/v1"`, `api_backend`는 `chat_completions` 또는 `responses`, `model`은 `vendor/model`). 즉 이건 공급자 추가 문제가 아니었다.
- **진짜 설계 결함은 따로 있었다**: `apply_llm_proxy_envs`(`fleet-worker/src/grok_process.rs:115`)는 **하나의** `gateway_url`을 OpenAI·Anthropic·Gemini·Antigravity 네 계열 환경변수로 팬아웃하면서 **그 gateway가 네 dialect를 전부 서비스한다고 가정**한다. liteLLM에는 참이지만 일반적으로 거짓이고, 이 가정은 코드에만 있고 문서 어디에도 없었다. 부분 구현 gateway를 넣으면 설정 시점에는 아무 신호가 없고 해당 dialect를 쓰는 Agent만 **요청 시점에** 실패한다. 이번에 이것을 불변식으로 승격하고, 정본에서 "liteLLM gateway"를 "dialect 계약을 만족하는 gateway"로 일반화했다(liteLLM은 기준 구현으로 강등).
- **기억을 근거로 쓰려다 멈추고 프로브로 확인했고, 기억이 틀렸다**: 나는 "OpenRouter는 OpenAI 호환 전용"이라고 알고 있었다. 그 말이 참이면 Fleet의 `ANTHROPIC_BASE_URL` 팬아웃이 깨진 것으로 문서에 적혔을 것이다. 문서끼리도 엇갈려(API 개요는 Anthropic 지원을 언급하지 않고, 특정 문서 URL은 404) 인증 없는 POST 프로브로 라우트 존재 여부를 직접 쟀다(`401` = 존재·인증 거부, `404` = 없음). **`/api/v1/messages`는 `401`을 Anthropic 형식 error 봉투로 돌려준다 — OpenRouter는 Anthropic native dialect를 서비스한다.** Gemini `generateContent`는 `v1`·`v1beta` 모두 `404`로 실제로 없다. `authority: canonical` 문서에 기억을 적었다면 거짓을 정본으로 굳힐 뻔했다.
- **한 번 더 걸렀다 — 긍정 주장 하나가 여전히 기억 기반이었다**: 초고는 "`gateway_url = "https://openrouter.ai/api"`로 두면 `OPENAI_*`와 `ANTHROPIC_BASE_URL`이 둘 다 올바른 경로로 해석된다"고 단정했는데, 이 중 Anthropic 쪽 근거는 "Anthropic SDK가 `/v1/messages`를 이어붙인다"는 내 기억뿐이었고 검색 결과는 오히려 `…/api/v1`을 설정하라고 말하고 있었다. 같은 프로브 기법으로 갈랐다.
  - `/api/v1/v1/messages` → `404`
  - `/api/messages` → `200`이지만 `content-type: text/html`, `x-matched-path: /[maker-id]/[slug]` — **API 라우트가 아니라 Next.js 마케팅 페이지**다. 상태코드만 봤다면 "존재함"으로 오독했을 자리다.
  - 결론: OpenRouter는 `/api/v1/messages` **한 형태만** 서비스하므로 두 base 모양을 함께 허용하지 않는다. 하위 CLI가 `{base}/v1/messages`로 이으면 하나의 `gateway_url`이 두 dialect를 만족하고, `{base}/messages`로 이으면 **어떤 값으로도 동시 만족이 불가능**하다(그 경우 `OPENAI_BASE_URL`이 `…/api/v1/v1`이 되어 404). 어느 쪽인지는 외부 CLI 구현이라 이 저장소가 증명할 수 없으므로, 정본의 해당 행을 단정에서 **확인 필요**로 내리고 두 갈래를 표로 남겼다.
- **채울 방법이 없는 것은 만들지 않았다**: 커스텀 HTTP 헤더(`HTTP-Referer`/`X-Title`), provider routing 선호, usage·cost 수집, `api_backend` 값 검증 네 가지를 유예하고 각각의 이유와 완료 조건을 정본의 유예 표에 적었다. 앞의 둘은 렌더링 대상인 grok `[model.<id>]`에 실을 슬롯이 없어 지금 칼럼을 만들면 **아무도 읽지 않는 값**이 된다. `api_backend` 검증은 별도 배경 작업(`task_e963ac6b`)으로 분리했다 — 기존 행에 집합 밖 값이 이미 들어 있을 수 있어(테스트가 `"openai"`를 통과시킨다) 엄격한 enum으로 조이면 읽기 시점 역직렬화가 깨질 수 있다.
- **`docs/credentials/registry.md`는 일부러 건드리지 않았다.** 이 파일은 실제 배포된 secret의 메타데이터 대장인데 OpenRouter credential은 존재하지 않는다. 행을 추가하면 없는 secret을 있는 것처럼 적는 셈이다.
- 변경 파일: [`docs/architecture/llm-gateway.md`](architecture/llm-gateway.md)(59→약 150줄로 재작성, 정본), [`docs/deployment/litellm-gateway.md`](deployment/litellm-gateway.md)(계약 참조 문단 추가), [`docs/index.md`](index.md)(날짜 동기화).
- 검증: 두 문서의 상대 링크 전부 해석됨, 인용한 소스 경로 7개 전부 존재 확인, 문서 색인 날짜 동기화. 코드 변경이 없으므로 빌드·테스트 게이트는 이번 항목의 대상이 아니다.
- **검증 한계**:
  - 프로브는 **라우트 존재 여부만** 증명한다. 유효한 API key로 실제 completion을 받는 것, grok·agy가 이 endpoint로 정상 동작하는 것은 검증하지 않았다.
  - 하위 CLI의 base URL 이어붙이기 규칙은 외부 구현이라 위 두 갈래 중 어느 쪽인지 정하지 못했다. **경로 A 채택 전 대상 CLI로 1회 실요청 검증이 필수다.**
  - `llm-gateway.md`의 `verification: code-checked`는 **Fleet 쪽 진술에만** 해당한다. OpenRouter 라우트 표는 코드가 아니라 2026-08-25의 외부 네트워크 관측이며, 정책의 `verification` 어휘에는 외부 관측을 가리키는 값이 없다. 공급자가 라우트를 바꾸면 무효가 된다.

## 2026-08-25 — lint — MCP `tools/list` wire 포맷이 사양과 달라 도구가 0개로 보이던 결함

- 유형: `lint`(정합성 수정) + `verification`
- **증상은 "연결됨, 도구 0개"였다.** 로컬 `.mcp.json`으로 orchestrator 호스트에 SSH stdio로
  붙였을 때 `initialize`는 성공하는데 도구가 하나도 보이지 않았다. raw JSON-RPC를 파이프로
  넣어 보면 서버는 멀쩡히 응답을 돌려주므로 **이 방식으로는 결함이 보이지 않는다**.
  표준 MCP SDK(`@modelcontextprotocol/sdk`) client로 같은 명령을 물려 `listTools()`를
  호출해서야 정체가 드러났다 — `McpError -32001: Request timed out`.
- **원인은 두 군데였고 둘 다 MCP `2024-11-05` 사양 위반이다**:
  1. `tools/list`의 `result`를 `ListToolsResult` 객체(`{ "tools": [...] }`)가 아니라
     **배열 그대로** 내보내고 있었다(`fleet-mcp/src/server.rs`).
  2. 도구 메타데이터 필드를 `inputSchema`가 아니라 Rust 필드명 그대로 **`input_schema`**로
     내보내고 있었다(`fleet-mcp/src/schema.rs`의 `ToolInfo`).
- **왜 오류가 아니라 타임아웃으로 나타나는가**가 이 항목의 핵심이다. 표준 SDK는 응답을
  result 스키마로 검증하고, **통과하지 못한 응답을 자기가 기다리던 응답으로 인정하지
  않는다**. 그래서 서버는 정상 응답을 보내고 client는 영원히 기다린다. 서버 로그에도,
  JSON-RPC error 코드에도 아무 흔적이 없다.
- **회귀 테스트가 있었는데 잘못된 형태를 "사양"으로 단언하고 있었다.**
  `cross_client.rs::gemini_cli_tools_list_shape`가 `result`가 배열이고 필드가
  `input_schema`인 것을 assert하고 있었다 — 즉 이 테스트는 결함을 잡는 대신 **고정**하고
  있었다. 사양 쪽으로 뒤집고, `input_schema` 키가 wire에 나오지 않는다는 negative
  assertion을 추가했다.
- **그 테스트는 어디서도 실행된 적이 없다.** 두 겹의 이유가 겹쳐 있었다:
  - 로컬에서는 `DATABASE_URL`이 없으면 `spawn_server()`가 `None`을 반환해 **조용히 skip**
    한다. `cargo test -p fleet-mcp`가 `0.00s`에 초록으로 끝나므로 skip과 pass가 종료
    코드로 구분되지 않는다(`--nocapture | grep -c skipping` = 13).
  - CI는 `DATABASE_URL`을 주지만 `FLEET_MCP_CAPABILITIES`를 주지 않는다. stdio MCP는 이
    값 없이 기동을 거부하므로(fail-closed) 12건이 전부 "no initialize response"로 실패할
    상태였다. 다만 CI가 그 앞 clippy 단계에서 먼저 실패해 test 단계까지 가지 않아
    드러나지 않았다(`main` 최근 5회 전부 red).
  → 이번에 fixture가 직접 `FLEET_MCP_CAPABILITIES`를 주도록 고쳤다. **13 skip → 12 pass**로
  바뀌었고, 이제 이 테스트는 실제로 형태를 검증한다.
- **`GET /api/tools`(dashboard)는 일부러 건드리지 않았다.** `fleet-dashboard/src/handlers.rs`
  는 `all_tools()`를 읽되 `"input_schema"` 키를 직접 조립한다 — serde rename의 영향을 받지
  않으며, 이쪽은 MCP가 아니라 dashboard 자신의 HTTP 계약이다.
- 변경 파일: [`crates/fleet-mcp/src/schema.rs`](../crates/fleet-mcp/src/schema.rs),
  [`crates/fleet-mcp/src/server.rs`](../crates/fleet-mcp/src/server.rs),
  [`crates/fleet-mcp/tests/cross_client.rs`](../crates/fleet-mcp/tests/cross_client.rs),
  [`docs/contracts/mcp-tools.md`](contracts/mcp-tools.md)(응답 envelope 정본 명문화 +
  도구 표를 `all_tools()` 전체 카탈로그 19개로 동기화 — Project·Issue 도구 7개가 표에서
  누락돼 있었고 "제안 계약일 뿐"이라는 낡은 문장이 남아 있었다),
  [`docs/deployment/mcp-clients.md`](deployment/mcp-clients.md)(연결 검증을 "등록 여부"가
  아니라 "`tools/list`가 실제로 도구를 반환하는지"로 재정의, 호스트 바이너리 최소 빌드
  요구 명시, Antigravity CLI 항목의 과잉 주장 정정).
- 검증: `cargo fmt --all -- --check` 통과 · clippy 2종(`acp mtls` / `--no-default-features`)
  각각 `-D warnings`로 exit 0 · `cross_client` 12 pass · 로컬 빌드 바이너리에 SDK client를
  물려 `LIST OK, count = 9`(launcher capability allow-list가 허용한 부분집합)와
  `inputSchema.type = "object"` 확인.
- **검증 한계**:
  - **수정된 바이너리는 아직 orchestrator 호스트에 배포되지 않았다.**
    `/usr/local/bin/fleet`은 여전히 2026-08-22 빌드이므로, `.mcp.json`을 넣고 Claude Code를
    재시작해도 client에는 **여전히 도구가 0개로 보인다.** 실제 수용 검증은 SSH 전 경로
    (`ssh … sudo -u fleet /usr/local/bin/fleet-mcp-launch.sh`, `--transport acp`)로 다시
    해야 하며, 로컬 `--transport mock` 결과가 이를 대신하지 못한다.
  - `unknown_tool_returns_error` 1건은 여전히 FAILED다. **이번 변경과 무관한 사전 존재
    결함**으로, `required_permission()`이 모르는 이름에 `None`을 돌려주면 `permits_tool`이
    false가 되어 unknown-tool 분기(`-32601`)에 닿기 전에 `-32600`으로 끊긴다. 위
    2026-08-25 포맷 항목에서도 같은 건이 사전 존재로 기록돼 있다(`task_78be5260`).

## 2026-08-25 — ingest — MCP 호스트 배포, capability 전체 개방, `tools/call` 오류 코드 계약

- 유형: `ingest`(배포·계약 정본화) + `verification`
- 위 `lint` 항목이 남긴 검증 한계 중 **`unknown_tool_returns_error` 실패를 해소**했다.
  Antigravity CLI(`agy`) 경로는 **여전히 미확인**이다(아래 검증 한계 참조).
- **`tools/call`의 오류 코드를 갈랐다(`-32601` vs `-32600`).** 사전 존재 결함을 "테스트
  기대값을 현실에 맞춘다"가 아니라 **구현을 사양 쪽으로 고치는** 방향으로 결정했다.
  `handle_tools_call`이 권한 게이트보다 **존재 여부를 먼저** 판정한다 — 카탈로그에 없는
  이름은 `dispatch_tool`의 fallback이 `-32601`로 답하고, 있지만 이 launcher의 capability가
  허용하지 않는 도구만 `-32600`으로 끊는다. 둘을 뭉치면 호출자가 오타와 권한 부족을
  구분할 수 없어 어느 쪽을 고쳐야 하는지 알 수 없다.
  - **존재 판정의 오라클을 `all_tools()` 카탈로그로 잡은 것이 핵심이다.** 더 직관적인
    선택인 `required_permission()`은 **틀린다** — `fleet_transition_issue`처럼 존재하지만
    요구 capability가 인자(목표 상태)에 따라 달라지는 도구에도 `None`을 반환하므로,
    `None`을 "없는 도구"로 읽는 순간 오판한다. 실제로 기존 코드가 그 경로였다.
  - **짝 테스트를 함께 넣었다**(`known_but_unpermitted_tool_returns_invalid_request`).
    `-32601` 테스트만 있으면 **권한 게이트를 통째로 삭제해도 초록불**이 된다. 두 케이스가
    서로 다른 코드로 갈린다는 것이 실제 계약이고, 그건 테스트 두 개로만 표현된다.
  - 정보 노출 판단: 존재 여부가 드러나지만 전체 카탈로그는 이미
    [MCP 도구 계약](contracts/mcp-tools.md)에 공개돼 있고 노출 통제는 `tools/list`가
    하므로, 이 변경으로 실제로 넓어지는 권한은 없다.
- **호스트 바이너리를 재빌드·교체했다(`oci-yarangdev-arm1`).** 공개 릴리스를 만들지 않는
  경로를 골랐다.
  - `release.yml`은 aarch64 tarball을 만들지만 `release` job이 `draft: false`로 **공개
    GitHub Release를 게시**한다(저장소가 public). 호스트 바이너리 하나를 갈아끼우는 데
    공개 게시를 끼워 넣을 이유가 없어 배제했다.
  - 호스트에는 cargo도 소스도 없다. 로컬 macOS에서 `cargo-zigbuild`로
    `aarch64-unknown-linux-gnu.2.35` 크로스 빌드(2분 2초). glibc 하한을 target triple에
    박는 이유는 호스트가 2.39이기 때문이다.
  - **이 경로가 성립하는 전제는 "컴파일타임 sqlx 매크로 0건"이다** — `query!` 계열을 쓰지
    않아 빌드에 살아 있는 `DATABASE_URL`이 필요 없다. 매크로를 도입하는 순간 이 경로는
    막히고 호스트나 CI에서 빌드해야 한다. 절차와 함께 정본에 적었다.
  - **교체 전에 스테이징 검증을 했다**: `/tmp/fleet-new`로 올리고 `/usr/local/bin`을 전혀
    건드리지 않는 임시 launcher로 먼저 `tools/list`를 확인한 뒤에야 교체했다. 실행 중인
    바이너리는 덮어쓸 수 없으므로(`Text file busy`) `install`로 새 inode를 만든다.
  - 백업: `/usr/local/bin/fleet.bak-20260825T065838Z`,
    `/etc/fleet/fleet.env.bak-20260825T065441Z`.
- **`FLEET_MCP_CAPABILITIES`를 19개 도구 전체로 개방했다**(운영자 결정). 다만 값만 늘리지
  않고 **같은 파일에서 권한을 제어할 수 있게** 만들었다 — 각 capability가 어떤 도구를
  여는지, 그리고 주의가 필요한 항목 4개와 **그 사유**(`token:revoke`·`project:delete`는
  되돌릴 수 없음, `worker:delete`는 HTTP에서 같은 이름이 워커 삭제 권한이라는 transport 간
  의미 충돌, `issue:approve_agent_work`는 Agent 자동 착수의 승인 관문)를 주석으로 함께
  적었다. 네 항목을 "되돌릴 수 없음" 하나로 묶지 않은 것은 의도다 — 사유가 다르면 좁힐 때의
  우선순위도 다르기 때문이다. 권한을 좁히는 일이
  "코드를 읽어야 아는 일"이 아니라 **그 줄에서 항목을 지우는 편집**이 되도록 한 것이 의도다.
  - `fleet_transition_issue`는 요구 capability가 목표 상태마다 다르므로(`issue:update` /
    `issue:close` / `issue:reopen` / `issue:approve_agent_work`) 단일 스위치가 아니다.
    4종을 전부 지우면 도구가 사라지고, 일부만 남기면 남긴 전이만 수행된다.
  - **서비스 재기동이 필요 없다.** launcher가 MCP 세션마다 `fleet serve`를 새로 띄우므로
    다음 세션부터 적용된다. `fleet.service`는 HTTP·dashboard 쪽 프로세스다.
- 검증(전부 이번 트리·이번 호스트에서 실행):
  - 게이트 3종 통과 — `cargo fmt --all -- --check`, clippy `--features "acp mtls"`,
    clippy `--no-default-features`(둘 다 `-D warnings`, exit 0).
  - `cross_client` **14 passed / 0 failed**(사전 존재 실패 해소 + 짝 테스트 1건 추가).
  - env 편집 후 키 개수 13→13, 모드·소유(`0640 root:fleet`) 보존 확인.
  - 재기동 후 헬스가 교체 전과 **동일**(`8081 → 404`, `8082 → 303`), `NRestarts=0`,
    `journalctl`에 warning 이상 없음.
  - **최종 수용은 `.mcp.json`이 쓰는 실제 경로 그대로 했다** — `ssh … sudo -u fleet
    /usr/local/bin/fleet-mcp-launch.sh`에 표준 MCP SDK client를 물려 `LIST OK, count = 19`.
    이어서 `tools/call`도 확인했다(`fleet_list_projects`, `fleet_list_workers` 둘 다
    `isError=false`에 실데이터). `tools/list`만으로는 `CallToolResult` 형태를 검증하지
    못하기 때문이다 — 위 `lint` 항목이 정확히 그런 종류의 형태 결함이었다.
  - 호스트 임시 파일(`/tmp/fleet-new`, `/tmp/fleet-mcp-launch-test.sh`) 정리 완료.
- 변경 파일: [`crates/fleet-mcp/src/server.rs`](../crates/fleet-mcp/src/server.rs),
  [`crates/fleet-mcp/tests/cross_client.rs`](../crates/fleet-mcp/tests/cross_client.rs),
  [MCP 도구 계약](contracts/mcp-tools.md)(오류 코드 계약 명문화),
  [MCP 클라이언트 연결](deployment/mcp-clients.md)(재빌드·교체 절차와 capability 운영 절
  신설). 호스트 측 변경(`/etc/fleet/fleet.env`, `/usr/local/bin/fleet`)은 저장소 추적
  대상이 아니다.
- **검증 한계**:
  - **Antigravity CLI(`agy`) 경로는 여전히 `tools/list` 성공을 확인하지 않았다.** 서버 쪽
    결함이 사라졌으므로 이제 성공할 것으로 보이지만, 실제로 재확인한 것은 표준 MCP SDK
    client의 SSH stdio 경로뿐이다.
  - **개방한 19개 중 실제로 호출해 본 것은 2개다.** 파괴적 도구(`fleet_delete_project`,
    `fleet_revoke_bootstrap_token`)는 의도적으로 호출하지 않았으므로, 그 핸들러들이 정상
    동작한다는 증거는 이번에 얻지 못했다. 개방했다는 것과 검증했다는 것은 다르다.
  - **CI는 여전히 red다.** clippy 드리프트(`fleet-api`의 `chunks_exact`,
    `agent-client-protocol-test`의 `unused_async`, `fleet-dashboard`의 `large_err` ×4)로
    최근 5개 run이 전부 실패다. 로컬 stable(1.97.1)에서는 재현되지 않으므로 CI의 toolchain
    해석 차이로 보이며, 정확한 원인은 이번 작업에서 규명하지 않았다 — 별건이다.
  - 작업 중 `oci-yarang-arm2`에서 **SSH host key 변경 경고**를 받았다. 우회하지 않았고
    원인도 확인하지 않았다. 이 저장소와 무관하지만 확인이 필요하다.

## 2026-08-25 — lint — 도메인 에이전트 4종 판정으로 잡은 정본 결함과 `issue:approve_agent_work` 예외의 명문화

- 유형: `lint` + `verification`
- **배경**: 세션이 단독 소유가 되면서 미커밋 트리 10개 파일을 커밋 가능한 상태로 만드는
  작업을 시작했다. 혼자 훑는 대신 도메인별 판정자를 `.claude/agents/`에 정의해
  (`expert-rust-gate`, `expert-git-history`, `expert-docs-canon`, `expert-security`) 각자
  자기 정본만 근거로 판정하게 했다. 이 항목은 그중 문서·보안 판정의 결과다.
- **가장 값진 결과는 "에이전트가 옳았다"가 아니라 "에이전트의 진단은 옳고 처방은 틀렸다"였다.**
  문서 판정자가 `deployment/mcp-clients.md`(되돌릴 수 없는 항목 3개)와 `docs/log.md`(4개)의
  불일치를 잡아냈고, 긴 쪽을 참으로 가정해 "짧은 쪽에 `worker:delete`를 추가"를 권고했다.
  세 번째 소스인 호스트 `/etc/fleet/fleet.env` 주석을 실제로 열어 보니 **둘 다 틀렸다** —
  호스트는 주의 항목 4개를 **두 축**으로 나눠 적고 있었고(`token:revoke`·`project:delete`만
  되돌릴 수 없음, `worker:delete`는 transport 간 이름 충돌, `issue:approve_agent_work`는 승인
  관문), `fleet_reset_worker_breaker`는 되돌릴 수 있으므로 권고대로 고쳤다면 정본에 **거짓을**
  심을 뻔했다. 불일치가 발견된 지점과 정답이 있는 지점은 대개 다르다.
- **차단 결함으로 판정해 고친 문서 결함**(전부 이번 diff가 만들었거나 이번 diff가 드러낸 것):
  - `contracts/mcp-tools.md`가 자기모순이었다. 33~35행은 "존재 판정의 정본은 `all_tools()`이지
    `required_permission()`이 아니다"라고 적으면서, 81~82행은 "도구별 요구 capability는
    `required_permission`이 정본"이라고 단정했다. 코드가 33~35행 편이다 — `permits_tool`이
    `fleet_transition_issue`만 `required_capability_for_transition`으로 특례 처리한다. 요구
    capability의 정본이 **둘**임을 명시하도록 고쳤다. 같은 문서의 범위 줄도 Project·Issue
    도구 7개를 정본화해 놓고 "Task, Worker, Host, bootstrap token"에 머물러 있어 보정했다.
  - `deployment/litellm-gateway.md`의 `last_verified`를 되돌렸다. 이번 변경은 교차참조 3줄이
    전부인데 날짜를 옮겨 **Runbook 본문(docker-compose 구성·포트·버전 핀·기동/rollback 절차)을
    재검증한 것처럼** 표시하고 있었다. 이 저장소 정본 체계에서 가장 잘 깨지는 것은 본문이
    아니라 **문서가 자기 자신에 대해 하는 메타 주장**이다.
  - `deployment/mcp-clients.md`의 호스트 바이너리 교체 절차가 `deployment/install.md`(정본)의
    "버전 고정 릴리스 artifact와 checksum" 정책을 **설계상 우회**하는데 양쪽에 교차참조가
    없었다. `authority: canonical` 둘이 같은 작업을 다르게 말하는 상태였다. 예외임을 명시하고
    양방향 링크를 걸었다.
  - `contracts/README.md`가 "아래 문서는 현재 호출 가능한 기능이 아니다"로 Project 계약을
    덮는데, 같은 diff가 `fleet_create_project`/`fleet_list_projects`/`fleet_delete_project`를
    현재 도구 표면으로 정본화해 그 문장을 거짓으로 만들었다. 부분 구현을 표현하도록 한정했다.
- **보안 판정이 커밋 차단 하나를 냈고, 그것이 이 항목의 핵심이다.**
  `FLEET_MCP_CAPABILITIES` 전체 개방에 `issue:approve_agent_work`가 포함됐는데, 이 capability는
  `fleet-core/src/auth.rs`와 [Issue 추적](architecture/issues.md)이 **"사람만 가질 수 있으며
  Agent/Worker에게는 어떤 경로로도 부여하지 않는다"**고 못박은 것이다. MCP stdio `ToolContext`에는
  호출 principal이 없으므로 부여하면 사실상 LLM이 승인 전이를 수행한다.
  - **오늘 악용 가능한 경로는 없다.** `#93`(Agent backlog claim)이 미구현이라 `ReadyForAgent`는
    표식에 그치고, claim lease·`origin_issue_id` 같은 보상 통제는 아직 대상이 없다.
    **위험은 `#93`이 구현되는 순간 무장된다.**
  - 운영자 결정은 **유지 + 재검토 게이트 등재**였다. 권한을 유지하되, 세 정본(코드 doc
    comment, 아키텍처 문서, 호스트 주석)에 예외를 명시하고 Roadmap `#93`에 "착수 전 MCP
    stdio의 이 capability 재검토"를 **선행 게이트로 등재**했다. 문서를 사실과 일치시키는 것과
    위험이 무장되는 시점에 강제로 다시 걸리게 하는 것을 동시에 만족시키는 선택이다.
  - 대안으로 검토한 "이 항목만 제거"는 비용이 작다는 것을 코드로 확인해 두었다 —
    `permits_tool`이 `.any()`로 노출을 판정하므로 `fleet_transition_issue`는 계속 보이고
    나머지 3종 전이도 그대로 동작하며, 잃는 것은 `ReadyForAgent` 전이 하나뿐이다. 게이트에서
    재검토할 때 이 사실을 다시 조사하지 않아도 되도록 남긴다.
- **자기 코드에서 그럴듯하지만 틀린 논증을 하나 잡았다.** `server.rs`의 새 권한 분기가 존재
  여부 노출을 정당화하며 "전체 카탈로그는 `docs/contracts/mcp-tools.md`에 공개돼 있다"를 근거로
  들고 있었다. 이것은 **보안 속성을 문서 파일의 내용과 저장소 공개 여부에 의존시킨다** — 저장소를
  private로 바꾸면 근거가 사라진다. 견고한 근거는 따로 있다: `tools/list`가 **같은 비인증
  채널로** 이미 그 launcher의 전체 부여 집합을 열거하므로, `-32600`이 추가로 흘리는 정보는
  "받지 못한 capability에 속한 도구가 카탈로그에 있다"뿐이다. 코드와 문서 양쪽을 고쳤다.
- 변경 파일: `crates/fleet-mcp/src/server.rs`(주석), `crates/fleet-core/src/auth.rs`(doc
  comment), `docs/contracts/mcp-tools.md`, `docs/contracts/README.md`,
  `docs/deployment/mcp-clients.md`, `docs/deployment/install.md`,
  `docs/deployment/litellm-gateway.md`, `docs/architecture/issues.md`,
  `docs/roadmap/roadmap.md`, `docs/credentials/registry.md`, 이 로그.
  호스트: `oci-yarangdev-arm1:/etc/fleet/fleet.env`(주석만, 백업 후 `0640 root:fleet`·키 13개
  불변 확인).
- **검증 한계**:
  - 이 항목의 대부분은 **문서·주석 변경**이며 런타임 동작을 바꾸지 않는다. 유일한 예외인
    `server.rs`도 주석만 고쳤다. 따라서 여기서 3종 게이트를 다시 돌려 얻는 신호는 없다 —
    게이트 결과는 같은 트리의 코드 변경(MCP wire 포맷, `TaskPhase`)에 대해 별도로 기록한다.
  - **`issue:approve_agent_work`를 유지하기로 한 결정은 "안전함을 확인했다"가 아니다.**
    `#93` 미구현이라는 **시점 의존적 전제** 위에서만 무해하며, 그 전제가 깨지는 시점을
    게이트로 표시했을 뿐이다. 게이트가 실제로 지켜지는지는 이 항목이 보장하지 못한다.
  - 호스트 `.bak` 파일들에 회수 기한이 없다. 시크릿을 담은 사본이 무기한 남는 문제는
    `credentials/registry.md`에 미조치로 등재했고 이번에 해소하지 않았다.

## 2026-08-25 — lint — `main`의 CI 8연속 실패: 증상은 clippy lint 3계열, 원인은 게이트 정합성 결함 2건

- 유형: `lint` + `verification`
- **무엇을 했나**: `main`의 CI가 8회 연속 실패하는 동안 `agent.md` §4의 로컬 게이트 3개는 계속 `exit 0`을 반환했다. 게이트 명령은 CI와 같았고, **게이트가 도는 환경**이 달랐다. 증상(clippy lint)이 아니라 그 비대칭을 먼저 고쳤다.
- **근본 원인 2건**:

  | 결함 | 상태 | CI 실제 | 결과 |
  |---|---|---|---|
  | 툴체인 부동 | `rust-toolchain.toml`이 `channel = "stable"`, CI도 `dtolnay/rust-toolchain@stable` | 실행 시점 해석 → 1.98.0 | 로컬 1.97.1 / CI 1.98.0. 1.98.0이 도입한 lint는 **CI에서만** 나타나고 로컬에서 재현 불가 |
  | RUSTFLAGS 비대칭 | `agent.md` 게이트가 걸지 않음 | `ci.yml:14`의 workflow-level `env`가 **전 잡**에 `-D warnings` | `-- -D warnings`와 겹치지 않는다 — 후자는 clippy lint에만, 전자는 rustc 경고 전체에 걸리고 `test`·`coverage` 잡에도 적용된다 |

  양쪽을 `1.98.0`에 고정했다. `rust-toolchain.toml`만으로는 부족하다 — dtolnay 액션은 브랜치명을 기본 toolchain으로 쓰고 이 파일을 읽지 않으므로 `ci.yml` 3곳과 `release.yml` 1곳에 `toolchain` 입력을 명시했다. 액션 정의를 직접 확인해 근거를 세웠다 — `stable` ref의 `action.yml`에는 `toolchain` 입력이 `required: false, default: stable`로 선언돼 있고, 브랜치명은 그 **기본값으로만** 들어간다. 즉 입력을 명시하면 브랜치 태그를 그대로 두어도 해석 결과가 고정된다. §4.3에는 `rustc --version`과 `export RUSTFLAGS="-D warnings"`를 게이트의 일부로 명시했다.

  이 파일에 2026-08-25 자로 같은 결함이 두 번 기록된다. 앞의 것은 목록에서 `cargo fmt`가 빠져서(목록이 **짧아서**), 이번 것은 명령이 같아도 **실행 환경이 달라서**다. 드러난 자리만 다를 뿐 같은 결함이다.
- **벤더 경계**: clippy는 member(primary) 패키지만 채점한다. 벤더링한 공식 ACP Rust SDK가 워크스페이스 멤버라서, 우리가 유지보수하지 않는 코드의 lint(`unused_async_trait_impl`, `agent-client-protocol-test/src/lib.rs:14`)가 게이트를 즉사시켰다. 명령줄 `--exclude`는 이걸 풀지 못한다 — 멤버 **선택 단계 필터**일 뿐 멤버십을 바꾸지 않아 clippy가 여전히 primary로 채점한다(합성 워크스페이스 실측: `--exclude` exit 101, `[workspace] exclude` exit 0). 매니페스트 쪽 수정이라 §4의 게이트 명령 3개가 바이트 단위로 유지되는 이점도 있다.
  - 멤버 **15 → 11**(정확히 `crates/` 11개). `agent-client-protocol`·`-derive`·`-http`·`-schema`는 `Cargo.lock`에 남아 의존성으로 계속 컴파일된다. 빠지는 것은 아무도 의존하지 않는 doctest 헬퍼 `-test`뿐이다.
  - **부수효과를 미리 기록한다**: `cargo test --workspace`(`ci.yml:66,103`)와 `cargo llvm-cov --workspace`(`ci.yml:177`)가 더 이상 이 SDK의 테스트를 포함하지 않으므로 **커버리지 수치가 한 번 내려간다.** 나중에 원인 불명의 변동으로 보이지 않게 남긴다.
  - 부수 발견: `Cargo.lock`이 413줄 줄었고, 그 과정에서 워크스페이스가 **axum을 두 버전(우리 0.7.9, 벤더 0.8.9) 동시에** 잠그고 있었다는 사실이 드러났다. 0.8.9 계열(`axum-core` 0.5.6, `axum-macros`, `tower-http`, `tokio-tungstenite`, `windows-*`)이 함께 빠졌다. 우리 크레이트는 전부 0.7.9를 쓴다.
- **lint 3계열은 그 다음에 고쳤다**:
  - `chunks_exact_to_as_chunks` — **사이트가 2곳이었다.** `fleet-api/src/handlers.rs:1167`과 `fleet-cli/src/token.rs:289`. clippy는 첫 실패 크레이트에서 abort하므로 red gate는 `fleet-api`만 보여준다. 워크스페이스를 직접 grep하지 않았으면 하나를 남긴 채 "고쳤다"고 판단했을 것이다.
  - `result_large_err` ×4 (`fleet-dashboard/src/auth.rs:101`, `handlers.rs:1973/2148/3031`, Err 크기 128/224/232/232B) — 함수별 `#[allow(..., reason = ...)]`.
    - **이 저장소의 기존 대응은 억제가 아니라 수정이었다**: `fleet-provisioner/src/error.rs:63`은 같은 lint를 `source: Box<StepError>`로 해소하고 그 이유를 doc comment에 적어 둔다. 그래서 여기서 `#[allow]`을 고르려면 **그 선례가 왜 적용되지 않는지**를 먼저 보여야 한다: provisioner의 Err는 평범한 `enum`이라 Box로 감싸도 되지만, axum 핸들러의 반환 타입은 `IntoResponse` 바운드에 묶여 있고 axum-core 0.4.5에는 `impl IntoResponse for Box<T>` 제네릭 구현이 없다(`Box<str>`·`Box<[u8]>`만 있다). 게다가 이 `Result`는 요청당 최대 한 번 구성되어 곧바로 `IntoResponse`로 소비되므로, lint가 겨냥하는 비용(큰 Err를 여러 스택 프레임에 걸쳐 이동시키는 것) 자체가 발생하지 않는다. Err 구성 지점도 18곳이다. `clippy.toml`의 임계값 조정은 **우리 코드 채점을 약화**시키므로 택하지 않았다 — 남의 코드를 대상에서 빼는 벤더 경계와는 성격이 다르다. 관례 변경이다: 직전까지 `crates/` 아래 bare `#[allow]`이 45개, lint 어트리뷰트의 `reason =`은 0개였다.
  - `unused_async_trait_impl` — 벤더 경계로 해소.
- **판정 중 정정한 것 2건**(도메인 에이전트 판정을 액면 그대로 받지 않은 결과):
  - 고쳐야 할 사이트 총계가 6이 아니라 **7**이었다 — `chunks_exact`가 1곳이 아니라 2곳(위 `fleet-cli`)이어서, 2+4+1이다. 판정 자신이 "red gate는 첫 실패 계열만 보여준다"고 적어 놓고 그 함정에 걸렸다. 계열이 3종으로 닫힌다는 증명은 `-A` 억제 후 `EXIT=0` 방식이라 그대로 유효하다.
  - "`as_chunks`는 MSRV 여유가 0이므로 신중해야 한다"는 경고는 사실이지만, **이 변경이 그 상태를 만드는 것이 아니다.** `fleet-worker/src/join.rs:271`이 이미 `as_chunks::<3>()`를 쓰고 있어 MSRV는 이전부터 이 기능에 묶여 있었다. 즉 같은 함수 세 사본 사이의 드리프트를 되돌린 것이다.
- **테스트 없는 프로덕션 경로 1건을 메웠다**: `fleet-api`의 `base64url`은 관리자 부트스트랩 토큰(`app.rs:1056`의 `fat_` 토큰)을 만드는 경로에 있는데 단위 테스트가 0개였다(같은 함수의 다른 두 사본에는 RFC 4648 벡터 테스트가 있었다). 무테스트 상태로 인코딩 함수를 고치지 않기 위해 **테스트를 먼저 넣고 통과를 확인한 뒤** 재작성하고 다시 확인했다. 순서를 뒤집었다면 테스트는 "새 구현이 스스로에 대해 일관적"임만 증명했을 것이다. 추가한 것: RFC 4648 §10 벡터 7개(나머지 길이 0/1/2 전 경로), URL-safe 알파벳(`-_` 사용·`+/` 미사용), `0..=48`바이트 길이 `= ceil(4n/3)`과 32바이트 → 43자.
- **같은 함수가 세 벌 존재한다는 사실 자체는 후속 작업으로 분리했다**(`fleet-api`/`fleet-cli`/`fleet-worker`). 이번 사건이 그 중복의 비용을 이미 청구했다 — 세 사본이 드리프트해서 lint를 두 번 고쳐야 했고, 테스트 보유도 갈렸다.
- **`agent.md` §3의 `cargo test`를 돌리다가 flake 하나의 기전을 다시 규명했다**(커밋 `d93c7e0`): `fleet-worker`의 `registration::tests::disk_cache_get_or_schedule_refresh_populates_background`. 이 파일의 "2026-08-25 — `main`의 CI fmt 게이트 복구, 그리고 게이트 목록을 CI와 일치시킴" 항목이 "환경적 타이밍 예산" 문제로 분류해 둔 그 테스트인데, **분류는 맞았고 기전은 틀렸다.**
  - 격리·단일 스레드에서 5회 중 2회 실패했다. 간헐적 경합이라면 이 재현율이 나오지 않는다.
  - 계측 결과: 이 테스트가 프로세스 안에서 유발하는 `Disks::new_with_refreshed_list()`의 **첫** 호출이 12.9초, 2회차 이후는 30ms였다. 테스트 예산은 100회 × 100ms = **정확히 10초**였다. 즉 경합이 아니라 **예산 < 비용**의 결정적 구조이고, 그래서 성공한 실행도 전부 10.27초가 걸렸다(값이 예산 경계 직전에 도착).
  - 프로덕션 코드의 doc comment(`collect_disk_free_mb`)가 이미 "수 초가 소요될 수 있음(macOS autofs 마운트 타임아웃)"이라고 예고하고 있었다. 테스트가 **자기 모듈이 문서화한 기대치보다 좁은 예산**을 쓰고 있었던 것이다.
  - 예산을 30초로 올리고 근거를 테스트 주석에 적었다. 값이 도착하면 즉시 break하므로 빠른 환경에서 이 상수는 비용이 아니다. 같은 조건 5/5 통과.
  - 이 규명은 별도 크레이트에서 같은 호출을 재보다가 **한 번 기각됐다** — 스탠드얼론 프로브에서는 메인 스레드·`spawn_blocking`·`std::thread` 전부 30~76ms였다. "느린 건 blocking 풀 스레드 때문"이라는 가설이 여기서 죽었고, 계측을 테스트 바이너리 **안으로** 옮기고 나서야 "첫 호출만 느리다"가 드러났다. 바깥에서 잰 수치로 안을 추론하지 않는다.
- 검증(전부 CI와 같은 형태 — `rustc 1.98.0 (88d9e12ae 2026-08-18)` + `RUSTFLAGS="-D warnings"` — 로 실행하고 exit 코드를 변수로 직접 받았다):
  `cargo fmt --all -- --check` **exit 0** · `cargo clippy --workspace --features "acp mtls" --all-targets -- -D warnings` **exit 0** · `cargo clippy --workspace --no-default-features --all-targets -- -D warnings` **exit 0**, 세 출력 모두 `^error|^warning` 0줄.
  **캐시가 더운 exit 0은 증적으로 쓰지 않았다.** 첫 실행에서 clippy 두 게이트가 0.89초·0.30초에 끝난 것이 눈에 걸렸다 — cargo가 fingerprint를 fresh로 판정해 진단을 **다시 내보내지 않은** 상태였다. exit 0 자체는 건전하지만(cargo는 같은 플래그로 성공한 유닛만 fresh로 표시한다) "lint를 돌렸다"는 주장은 성립하지 않는다. `crates/` 아래 `.rs` 143개의 mtime을 무효화하고 다시 돌려, 두 게이트 모두 멤버 11개에 `Checking`이 찍히는 것을 확인한 뒤의 결과가 위 숫자다.
  `cargo test --workspace --features "acp mtls" -- --test-threads=1` **exit 0** — 테스트 바이너리 55개 + doc-test 10회, `test result` 65줄 전부 `ok`, **982 passed / 0 failed / 4 ignored**. 위 flake 수정 후의 수치다(수정 전에는 `fleet-worker --lib`가 71 passed / 1 failed, 이후 72 passed). `0 passed`로 지나간 바이너리는 `e2e_with_real_grok.rs` 하나뿐이고 2건 모두 `#[ignore]`(실제 Grok API 필요)로, **피처가 빠져 조용히 건너뛴 것이 아니다** — 이 파일의 앞선 항목이 경고한 함정이라 명시적으로 확인했다. `fleet-store`의 DB-gated 스위트도 `issues.rs` 14건 · `projects.rs` 8건으로 실제 채점됐다(워크스페이스 피처 통합으로 `test-support`가 켜진다).
  `cargo metadata --no-deps`로 멤버가 정확히 `crates/` 11개(`fleet-api`, `fleet-cli`, `fleet-core`, `fleet-credentials`, `fleet-dashboard`, `fleet-mcp`, `fleet-provisioner`, `fleet-scheduler`, `fleet-store`, `fleet-transport`, `fleet-worker`)임을 확인했다. 툴체인 고정이 실제로 먹었는지도 확인했다 — 인자 없는 `rustc --version`이 1.97.1 → 1.98.0으로 바뀌었다.
- **검증 한계**:
  - **`${PIPESTATUS[0]}`로 exit 코드를 찍던 첫 시도는 빈 값을 남겼다.** 이 셸은 zsh이고 `PIPESTATUS`는 bash-ism이다(zsh는 소문자 `$pipestatus`). 그래서 게이트 로그가 **초록으로 보이면서 exit 코드는 기록하지 않은** 상태였다 — 이 항목의 주제(게이트가 자기 자신에 대해 거짓 신호를 낸다)와 정확히 같은 종류의 사고다. 파이프 없이 `$?`를 변수로 받아 다시 돌렸다. 다음 에이전트는 게이트 스크립트에 `PIPESTATUS`를 쓰지 않는다.
  - **게이트를 감싸는 셸 배관에서 같은 종류의 거짓 신호가 두 번 더 나왔다.** (1) `until ! pgrep -f "cargo test --workspace"`로 만든 대기 루프가 **자기 자신의 명령줄**과 매치돼 영원히 끝나지 않았다 — `pgrep -f`에 넘기는 패턴이 그 패턴을 담은 스크립트와 겹치면 안 된다. `pgrep -x cargo`처럼 실행 파일 이름으로 좁힌다. (2) `cargo test … 2>&1 | grep "^PROBE"`가 컴파일 에러를 통째로 삼켜, 출력 0줄이 "아직 실행 중"으로 오독됐다. 게이트 출력은 파일로 받고 **그 다음에** 필터한다. 위 `PIPESTATUS` 건과 함께, 이 항목의 주제는 게이트 명령 자체만이 아니라 **게이트를 관찰하는 배관**까지다.
  - **DB 게이트(`DATABASE_URL` 주입)는 다시 돌리지 않았다.** 이번 변경에 SQL 쿼리나 DB 트레이트 수정이 없어 `agent.md` §3.2의 발동 조건에 해당하지 않는다. 해당 경로의 최신 증적은 이 파일의 "2026-08-25 — `#58` 후속: Project 경계 술어 추출, 그리고 `main`의 CI fmt 게이트 실패 발견"과 "2026-08-25 — `main`의 CI fmt 게이트 복구, 그리고 게이트 목록을 CI와 일치시킴" 두 항목이다.
  - **벤더 SDK의 테스트는 이제 `cargo test --workspace`·`cargo llvm-cov --workspace`가 채점하지 않는다.** 우리가 그 코드를 유지보수하지 않으므로 의도된 것이지만, upstream을 당겨올 때 회귀를 우리 스위트가 잡아 주지 않는다는 뜻이기도 하다. 이 SDK를 유지보수하는 fork로 바꾸는 순간 경계를 `-test` 하나로 좁혀야 한다(그 신호를 `Cargo.toml` 주석에 적어 뒀다).
  - **CI 러너에서 1.98.0이 실제로 설치되는 것은 아직 관측하지 못했다.** 액션이 `toolchain` 입력을 존중한다는 것은 `action.yml`에서 확인했지만(위), 실제 워크플로 실행 로그의 `rustc --version`은 다음 CI 실행에서만 보인다. 로컬에서 직접 확인한 것은 `rust-toolchain.toml` 쪽 효과뿐이다.

## 2026-08-25 — `#62` 1단계 — 상태 전이 compare-and-set: 늦은 완료와 취소 경쟁을 스토어에서 닫음

- 유형: `feature` + `correctness`
- **무엇을 했나**: 작업 상태를 옮기는 모든 경로가 `update_task_status`로 **무조건 덮어쓰기**를 하고 있었다. 그래서 reconciler가 orphan을 `Failed`로 확정한 뒤 워커의 늦은 `Completed`가 도착하면 그대로 덮어썼고, 두 개의 취소가 동시에 들어오면 둘 다 성공했다. `reconcile.rs`의 모듈 주석 자신이 "이 스윕도 이론적 경쟁 상태를 완전히 막지는 못한다 — `update_task_status`가 현재 상태를 조건으로 거는 낙관적 잠금을 하지 않기 때문"이라고 적어 두고 있었다. 그 문장을 참으로 만들던 원인을 제거했다.
- **설계**: `Store::compare_and_set_task_status(id, expected: &[TaskPhase], new)`를 추가하고, 결과를 `TransitionOutcome{Applied, Rejected{current}}`로 돌려준다.
  - **거절을 `Err`이 아니라 `Ok`로 표현한다.** 다른 writer가 먼저 상태를 옮긴 것은 장애가 아니라 **정상적인 관측 결과**이고, 호출자는 대개 자기 쓰기를 포기하는 것이 옳다. `Err`은 DB 장애나 역직렬화 실패처럼 관측 자체가 불가능했던 경우로 남긴다. `Err`로 표현했다면 모든 호출 지점이 "진짜 실패"와 "정상적으로 진 경쟁"을 문자열이나 variant 매칭으로 다시 갈라야 했다.
  - **트레이트에 기본 구현을 두지 않았다.** 기본 구현을 주면 새 backend가 CAS를 조용히 잊고도 컴파일된다 — 이 저장소의 mock들이 `update_task_status`에 이미 `unimplemented!()`를 쓰는 것과 같은 이유다. 그 대가로 `Store` 구현 6곳(PgStore, MemStore, mock 4개)을 전부 손대야 했고, 실제로 그게 census를 강제해 아래 "놓칠 뻔한 것"을 잡아냈다.
  - **마이그레이션이 없다.** PgStore는 `WHERE id = $1 AND status_phase = ANY($3)` 단일 statement로 구현하는데, `status_phase`는 `001_init.sql:43`이 이미 만들어 둔 생성 칼럼(`status->>'phase'` STORED)이고 `002_indexes.sql`이 인덱스까지 걸어 두었다. 낙관적 잠금에 필요한 것이 이미 스키마에 있었고 아무도 쓰지 않고 있었다. MemStore는 단일 lock 안에서 read-compare-write이라 원자적이다.
  - `TaskPhase` 어휘는 `fec2371`에서 **소비자 0인 채로** 커밋돼 있었다. 이번이 그 첫 소비자다.
- **`expected`를 호출 지점별로 좁게 넘긴다** — 이게 이 변경에서 가장 틀리기 쉬웠던 지점이다. `TaskPhase::allowed_predecessors(Failed)`는 `[Pending, Dispatched]`를 준다. `mark_failed`에 그 합집합을 하드코딩하면 컴파일도 되고 테스트도 통과하지만, orphan sweep이 방금 `Pending → Dispatched`로 옮겨간 작업을 `Failed`로 죽일 수 있다 — **닫으려던 경쟁이 그대로 남는다.** 그래서 `mark_failed`는 `expected`를 인자로 받고, 호출 지점이 각자 자기가 실제로 뒤따를 수 있는 상태만 선언한다: 선택 실패·breaker open·dead-letter는 `[Pending]`, transport 실패·orphan·offline은 `[Dispatched]`. `allowed_predecessors`의 doc comment가 이미 이 함정을 경고하고 있었다.
- **전이 결과에 무엇을 종속시킬지 두 방향으로 갈렸다** (grep으로 결정했고, 가정으로 결정하지 않았다):

  | 부수효과 | 게이트 | 이유 |
  |---|---|---|
  | `append_event` | `Applied`일 때만 | 이벤트 로그는 스토어 사실의 파생물이다. 거절된 전이가 `task_completed`를 남기면 로그가 **일어나지 않은 일**을 주장한다 |
  | `dec_running()` | 결과와 무관하게 | 이 게이지는 스토어 상태가 아니라 **디스패처 자신의 `inc_running()`**과 짝지어져 있다. `inc_running()`은 워크스페이스 전체에서 정확히 1곳(`dispatcher.rs:530`, dispatch 경로)뿐이고 reconciler는 게이지를 건드리지 않는다. `Applied`일 때만 감소시키면 늦은 완료가 거절될 때마다 증가분이 영구히 남는다 |

- **dispatch 경로는 transport로 보내기 _전에_ 중단한다.** CAS가 거절되면 `DispatchError::NotPending`으로 즉시 반환한다 — 상태를 차지하지 못한 채 dispatch하면 아무도 소유하지 않는 실행이 생긴다. 이 에러는 재시도 대상이 아니다(작업은 이미 누군가의 소유다). 부수적으로 `dispatch_ready_tasks`에 있던 실제 이중 dispatch 창이 닫혔다 — 두 스윕이 같은 pending 작업을 집어도 하나만 `Pending`을 차지한다.
- **`Dispatcher::cancel`의 TOCTOU를 닫았다.** 기존 코드는 `get_task` → `is_terminal()` 검사 → 쓰기 3단계였다. 이제 `[Pending, Dispatched]` CAS 한 번이고, 거절은 곧 `CancelError::AlreadyTerminal`이다. `fleet tasks cancel`(CLI)도 같은 형태로 바꿨다.
- **중복 매핑 하나를 제거했다**: `phase_label`이 `TaskStatus` → 문자열 5-arm match를 손으로 다시 적고 있었다. CAS의 SQL 조건절이 이 문자열 집합에 의존하게 됐으므로 사본이 늘수록 조용히 어긋날 자리가 늘어난다. `status.phase().as_str()` 위임으로 바꿨다.
- **놓칠 뻔한 것**: `Store` 구현이 5개인 줄 알았는데 6개였다. `crates/fleet-scheduler/src/sync.rs:148`이 `impl fleet_store::Store for NoopStore`라고 **경로 한정 이름**으로 적혀 있어 `grep "impl Store for"`에 걸리지 않았고, `#[cfg(test)]` 안이라 평범한 `cargo check`도 잡지 못했다. `--all-targets` clippy 게이트가 잡아 줬다. 반대 방향의 오판도 한 번 있었다 — `project_rules.rs:261`을 프로덕션 writer로 세었는데 `#[cfg(test)] mod tests`(137행부터) 안이었다. 전환했다면 아무도 도달할 수 없는 `Rejected` 분기를 만들었을 것이다.
- **테스트**: `crates/fleet-store/tests/task_cas.rs` 9건, `both_backends!`로 MemStore와 실제 PostgreSQL 양쪽에서 같은 단언을 돌린다. 정본 [실행 일관성](architecture/tasks/execution-consistency.md)의 검증 게이트 대응은 `a_late_completion_cannot_overwrite_a_reconciled_failure`(게이트 1), `only_one_of_two_racing_cancels_applies`·`a_completion_cannot_land_on_a_cancelled_task`(게이트 2). 거절 케이스는 반환값만이 아니라 **저장된 상태가 바뀌지 않았음**까지 확인한다 — 그러지 않으면 "`Rejected`를 반환하면서 실제로는 쓴" 구현이 통과한다. `cas_reports_not_found_for_an_unknown_task`는 0행의 두 원인(행 없음 / 위상 불일치)이 갈리는 것을 고정한다.
- **검증 게이트**: `rustc 1.98.0 (88d9e12ae 2026-08-18)`로 `rust-toolchain.toml`의 고정과 일치. `RUSTFLAGS="-D warnings"` 아래에서 `cargo fmt --all -- --check`, `cargo clippy --workspace --features "acp mtls" --all-targets -- -D warnings`, `cargo clippy --workspace --no-default-features --all-targets -- -D warnings` 모두 종료 코드 0. `cargo test --workspace --all-features`는 66개 스위트 991건 통과(`FAILED`·`panicked`·`error` 라인 0, exit 0). DB 게이트는 새로 만든 빈 DB에 `DATABASE_URL`을 주입해 `--test-threads=1`로 직렬 실행 — `task_cas` 9건(0.19s, PgStore가 실제로 돈 신호. `DATABASE_URL` 없이는 MemStore만 돌아 0.00s가 나온다), `fleet-scheduler` 98건 통과.

- **검증 한계와 이번에 밟은 함정**:

  | 무엇 | 내용 |
  |---|---|
  | `cargo test --workspace`에 `DATABASE_URL`을 물리면 안 된다 | 처음 워크스페이스 실행에서 `fleet-store/tests/audit_integration.rs` 7건이 전부 깨졌다. #62 회귀가 **아니다** — 그 파일과 `fleet-store/src/audit.rs`는 이번 변경과 diff가 0이고, `DATABASE_URL` 없이는 7건 통과, 직렬 실행에서도 7건 통과한다. 원인은 이 스위트가 격리(truncate)를 하지 않고 깨끗한 DB를 가정하는데 `cargo test`가 테스트 함수를 **병렬**로 돌린다는 것 — 같은 DB를 물린 채 재현하면 `users_username_key` 중복과 잔존 감사 행(`left: 14, right: 1`)으로 똑같이 깨진다. DB 게이트 크레이트를 `--test-threads=1`로 직렬 실행하라는 기존 방법이 정확히 이 이유 때문이며, 워크스페이스 실행은 `DATABASE_URL`을 **빼고** 돌려야 한다 |
  | 게이트 출력은 파일로 받고 그 다음에 필터한다 | 위 실패를 처음 관측했을 때 백그라운드 파이프 안에서 `grep \| sort \| uniq -c`를 걸어 둔 탓에 패닉 메시지가 사라지고 개수만 남아 있었다. 원인 규명이 불가능해 원본 출력을 파일로 다시 받아야 했다 |
  | 간헐 실패 2종 | `fleet-worker`의 `disk_cache_get_or_schedule_refresh_populates_background`와 `fleet-mcp`의 `cross_client` 타이밍 flake는 이번 clean 실행에서 나타나지 않았다. "이번에 안 났다"가 "없다"는 뜻은 아니다 — 둘 다 기존 flake이고 해당 크레이트는 이번 변경과 diff가 0이다 |
  | CAS가 **동시성 부하 아래**에서 검증되지는 않았다 | `task_cas.rs`는 전이를 순차로 겹쳐서 경쟁의 **결과**를 고정한다(늦은 완료가 거부되는지, 두 취소 중 하나만 적용되는지). 실제 다중 writer를 동시에 때리는 부하 테스트는 없다. 단일 `UPDATE ... WHERE status_phase = ANY(...)` statement의 원자성은 Postgres가 보장하는 것이지 이 테스트가 증명하는 것이 아니다 |

- **미룬 것과 이유**:

  | 미룬 것 | 왜 |
  |---|---|
  | `TaskAttempt` 엔티티, effect ledger, external idempotency key (정본 게이트 3·6·7·9·10) | 이번 변경은 **Task의 상태 칸 하나**에 대한 잠금이다. Attempt는 별도 테이블·수명주기·재시도 계약을 요구하며 CAS 위에 얹히는 층이지 그 일부가 아니다. 로드맵 `#62`의 2단계 이후로 남긴다 |
  | 이전 control epoch 이벤트 거부 (게이트 4) | 전이 시점에 epoch를 읽을 경로가 없다. `#63` 2단계가 `FleetState.lease`를 배선했지만 그건 **호출 진입점**에서의 fence이고, 스토어 statement에 epoch를 조건으로 함께 거는 것은 `control_plane_lease`와 `tasks`를 한 트랜잭션으로 묶는 별개 설계다 |
  | 비가역 tool effect의 자동 재실행 금지 (게이트 5) | effect 분류 자체가 아직 코드에 없다. 분류 없이 "비가역이면 재실행 금지" 분기를 넣으면 항상 거짓인 죽은 조건이 된다 |
  | `Rejected{current}`를 UPDATE와 한 트랜잭션으로 읽기 | PgStore는 0행을 받은 뒤 **별도 SELECT**로 현재 위상을 읽으므로 그 사이에 또 다른 writer가 상태를 바꿀 수 있다. 그래서 `current`를 타입 doc에서 **로깅·에러 메시지 전용**으로 규정하고 제어 흐름의 근거로 삼지 않기로 했다. 정확성이 필요한 판단은 전부 `Applied`/`Rejected` 이분법만 쓴다. 그 대가로 모든 전이에 트랜잭션 비용을 치르지 않는다 |
  | `RUNNING_GAUGE` underflow | `Dispatcher::cancel`은 `Pending` 작업에 대해서도 `dec_running()`을 호출하는데, `inc_running()`은 dispatch 시점에만 실행되므로 0에서 `fetch_sub(1)`이 `usize::MAX`로 wrap한다. **이번 변경이 만든 것이 아니다**(기존 `cancel`도 무조건 감소시켰다). 고치지 않은 이유는 따로 있다 — **`running_count()`의 소비자가 워크스페이스에 0개**다. 이 게이지는 쓰기 전용이고 아무도 읽지 않아 결함이 관측되지 않는다. 밴드에이드(`checked_sub`)로 덮으면 "고쳤다"는 인상만 남고 회계는 여전히 틀린다. 정확한 회계에는 `Applied{previous: TaskPhase}`가 필요하고, 그건 이 게이지를 실제로 메트릭에 배선할 때 같이 결정할 일이다. 별도 항목으로 분리한다 |

- **문서**: `reconcile.rs`의 모듈 주석에서 "낙관적 잠금을 하지 않기 때문에 경쟁을 막지 못한다"는 서술을 걷어내고, 5분 유예가 이제 **정합성의 근거가 아니라 되돌릴 수 있는 `Offline` 상태에서 성급하게 실패 처리하지 않기 위한 정책**으로만 남는다고 고쳐 적었다. 동시에 CAS가 닫지 **않는** 것도 명시했다 — 워커에서 실제로 완료된 작업이 여기서 `Failed`로 확정되면 그 결과물은 여전히 버려진다. 상태는 일관되지만 일은 낭비되며, 그것을 줄이는 것은 유예 시간 조정의 몫이다. 같은 파일의 dispatch 결과 `match`에 있던 catch-all 주석도 고쳤다 — "dispatch_existing이 이미 Failed로 마킹했으므로 로깅만 한다"는 서술은 새로 생긴 `DispatchError::NotPending`에 대해 **거짓**이다. NotPending은 상태를 전혀 건들지 않은 경우이며, 여기서 실패 마킹을 하면 방금 다른 writer가 정당하게 가져간 작업을 죽여 이번 변경이 닫으려던 것과 같은 종류의 경쟁을 **새로** 만든다. 코드는 이미 로깅만 하고 있었으므로 동작 변경은 없고, 주석이 그 이유를 잘못 말하고 있던 것을 바로잡았다.

## 2026-08-25 — verification — 게이트 정합성 수정을 CI에서 관측: lint는 초록, 그 뒤에 가려져 있던 테스트 실패 4건

- 유형: `verification`
- **무엇을 했나**: 게이트 정합성 커밋 11개를 `origin/main`에 푸시하고(`22d9e50..3238c65`) CI 실행을 관측했다. 목적은 이 파일의 "2026-08-25 — lint — `main`의 CI 8연속 실패" 항목이 남긴 마지막 검증 한계를 닫는 것이었다.
- **검증 한계가 닫혔다**: 러너 로그에 `Run dtolnay/rust-toolchain@stable → toolchain: 1.98.0`, `rustup toolchain install 1.98.0 --component clippy --component rustfmt`가 찍혔고, 실제 실행 경로도 `/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo`였다. 액션이 명시 `toolchain` 입력을 존중한다는 것을 정적 근거(`action.yml`)가 아니라 실측으로 확인했다.
- **lint 게이트는 목적을 달성했다**: `cargo fmt --check`, `cargo clippy (acp+mtls)`, `cargo clippy (no-default-features)` 모두 CI에서 통과했다. 8연속 실패의 직접 원인이던 clippy 3계열은 해소됐다.
- **그러자 test 단계가 처음으로 실행되면서 실패 4건이 드러났다.** clippy가 먼저 죽던 동안 이 단계는 도달조차 하지 않았으므로, **게이트를 고치면 빨간 것이 늘어나는 것이 정상이다** — 가려져 있던 것이 드러나기 때문이다. 4건 모두 `DATABASE_URL`이 있어야만 실제로 실행되는 경로이고, 로컬 전체 스위트가 982 passed / 0 failed였던 것은 그 경로들이 조용히 통과했기 때문이다.

  | 실패 | 도입 커밋 | 성격 |
  |---|---|---|
  | `fleet-store` `create_and_get_project_roundtrip` (`projects.rs:73`) | `1f4f913`(2026-08-24, `#48` 1단계) — 이번 푸시 **이전** | PgStore 왕복에서 `created_at`이 `…622808887Z` → `…622808Z`. Postgres `timestamptz`가 마이크로초로 절삭하는데 테스트가 나노초 값과 `assert_eq!`로 구조체 전체를 비교한다 |
  | `fleet-scheduler` `test_circuit_breaker_sync_between_scaleout_nodes` (`scaleout_sync.rs:143`) | `df0be43`(2026-08-21, `#61`) — 이번 푸시 **이전** | LISTEN/NOTIFY 전파를 `sleep(200ms)`로 기다린 뒤 `Open`을 단언하는데 `Closed`가 나온다. 예산 기반 대기라 이 파일의 disk-cache flake와 같은 부류다 |
  | `fleet-mcp` `gemini_cli_tools_list_shape` (`cross_client.rs:283`) | `315e3d6`(이번 푸시에 포함) | 서버가 `tools/list`를 **배열 + `input_schema`**로 반환 — `315e3d6`이 고치기 **이전**의 형태 |
  | `fleet-mcp` `known_but_unpermitted_tool_returns_invalid_request` (`cross_client.rs:460`) | `315e3d6`(이번 푸시에 포함) | `-32600`을 기대하는데 `-32601` — 역시 `315e3d6` 이전 동작 |

- **뒤 두 건은 stale 바이너리 가설이 유력하나 아직 미확정이다.** 근거 셋: (1) `cross_client.rs:42`가 `CARGO_BIN_EXE_*` 대신 `target/debug/fleet` 경로를 하드코딩한다. (2) `fleet-cli`에 `tests/`가 없다 — cargo는 **해당 패키지의 integration test나 bench가 선택될 때만** bin 타깃을 실제 바이너리로 빌드하므로, `cargo test --workspace`는 `src/main.rs`를 `--test`로만 컴파일하고 `target/debug/fleet`은 만들지 않는다. (3) CI의 `actions/cache@v6`가 `restore-keys: ${{ runner.os }}-cargo-`라는 **접두사 매칭**으로 `target/`을 복원한다. 즉 이 테스트가 실행하는 바이너리는 **아무도 갱신을 보장하지 않는 산출물**이다. 관측된 출력이 두 항목 모두 `315e3d6` 이전 동작과 정확히 일치하는 것도 이 가설과 맞는다. 로컬 재현으로 확정하려 했으나 아래 사유로 중단했다.
- **`spawn_server()`의 조용한 skip이 이 4건을 로컬에서 감췄다**: `cross_client.rs:31`의 `database_url()`이 `None`이면 `spawn_server()`가 `None`을 반환하고 **테스트는 그대로 통과**한다. 이 파일 1176행이 이미 같은 함정을, 1028행이 같은 부류의 위험("조용히 skip되어 통과로 보이는")을 기록해 뒀다. 이번에 "SQL·DB 트레이트 변경이 없으니 `agent.md` §3.2의 DB 게이트 발동 조건이 아니다"라고 판단한 것이 이 4건을 통과시켰다 — **발동 조건을 변경 종류로 좁게 읽으면, 변경과 무관하게 이미 깨져 있던 것은 영원히 보이지 않는다.**
- **작업 트리 동시 편집 사고**: 진단 도중 `git status`가 9개 파일 `+348/-48`로 더러워졌고 `fleet-scheduler`가 E0061 6건으로 컴파일되지 않았다. 원인은 결함이 아니라 **다른 세션(`[macstudio]grok-fleet-orchestrator 로드맵 진행`)이 같은 작업 트리에 `#62` 리팩터를 쓰고 있었던 것**이다(mtime 19:30~19:37, 당시 19:37). 아무것도 되돌리지 않고 빌드를 중단했다 — 공유 `target/`을 통해 서로의 증분 빌드를 무효화하기 때문이다. 그 세션은 이후 `76e4f81`로 커밋·푸시를 마쳤다. **한 작업 트리에 두 세션이 붙으면 빌드 산출물과 `git status`가 공유 가변 상태가 된다.**
- 검증: CI run `32837011249`(`3238c65`)와 `32841344564`(`76e4f81`) 두 실행 모두 같은 4건으로 실패 — flaky가 아니라 재현되는 실패다. `Shellcheck install/uninstall` 잡은 두 실행 모두 성공.
- **다음 작업으로 넘긴다**: 4건의 수정. 순서와 근거는 인계 프롬프트에 있다.

## 2026-08-26 — feat — `#62` 2단계: 클라이언트 멱등성(정본 게이트 3), 그리고 "행이 중복되지 않음"과 "실행이 중복되지 않음"의 차이

- 유형: `feat`
- **무엇을 했나**: [실행 일관성](architecture/tasks/execution-consistency.md) 정본 게이트 10개 중 **3번("같은 idempotency key로 재제출하면 새 실행을 만들지 않고 기존 결과를 돌려준다")** 을 닫았다. 1단계(`76e4f81`)가 Task의 상태 칸을 CAS로 잠갔다면, 2단계는 **제출 자체**를 잠근다.
- **왜 게이트 3만 골랐나**: 남은 게이트 4~10은 전부 `TaskAttempt` 엔티티나 effect ledger를 전제한다. 게이트 3만이 그 기계장치 없이 닫힌다 — 필요한 것은 요청에 붙는 키 하나와 그것을 강제하는 유니크 제약뿐이다.

- **설계 결정과 근거**:

  | 결정 | 왜 |
  |---|---|
  | 유니크 범위를 `(created_by, idempotency_key)`로 잡았다 | 정본은 "동일 principal"이라고 쓰지만 **MCP 표면에는 principal이 없다** — stdio JSON-RPC 루프이고 인가는 `FLEET_MCP_CAPABILITIES`만 본다. 그래서 `created_by`가 `"mcp"`로 하드코딩된다. 채울 수 없는 `principal_id` 칼럼을 미리 만드는 대신 `created_by`로 범위를 잡고, 한계를 세 곳(마이그레이션 024 헤더, `Store::insert_task_idempotent` doc, MCP 도구 스키마 설명)에 적어 두었다. **같은 오케스트레이터의 모든 MCP 제출이 하나의 키 네임스페이스를 공유한다**는 뜻이다. `#93`의 게이트가 같은 표면에서 **같은 principal 부재 문제**를 기록하고 있다 — 하나로 묶어 푸는 것이 맞다 |
  | 페이로드 해시를 길이 접두·필드 명시·버전 태그로 직접 만들었다 | `serde` 직렬화 바이트를 해싱하면 필드 순서·표현 변경이 조용히 해시를 바꿔 과거 키를 전부 충돌로 만든다. 길이 접두가 없으면 `prompt="ab"`와 `prompt="a", cwd="b"`가 같은 해시를 갖는다 — 이 경계 사례를 테스트로 고정했다 |
  | 라우터가 채우는 필드(`resolved_model`, `token_budget`, `routing_profile`)를 해시에서 뺐다 | 해시가 식별하는 것은 **클라이언트가 보낸 제출**이다. 라우팅은 그 제출로부터 결정되는 파생값이므로 포함하면 라우터 휴리스틱이 바뀔 때마다 재시도가 충돌로 변한다. 대시보드의 `inherit_from_parent`도 같은 이유로 해시 계산 **이후**에 적용된다 |
  | 거절을 `Ok(Conflict)`로 모델링했다 | 1단계 `TransitionOutcome`과 같은 근거 — 재제출은 장애가 아니라 정상적인 클라이언트 동작이다 |
  | `IdempotentInsert`에 `PartialEq`를 파생하지 않았다 | `Task`가 `PartialEq`가 아니라 `Box<Task>` 비교에서 E0369가 난다. 대신 `inserted() -> bool`을 뒀다 |
  | 빈 문자열을 키에서 접었다 | HTML 폼은 비어 있는 입력도 `""`로 보낸다. 접지 않으면 **키를 쓰지 않는 모든 제출이 `""` 하나를 공유해 서로를 중복으로 판정한다.** 조용하고 치명적인 회귀라 테스트로 명시 고정했다 |
  | `fleet-cli`는 비멱등으로 남겼다 | 로컬 CLI에는 timeout 후 재시도하는 클라이언트가 없다. `runtime.rs:1493`에 주석만 남기고 `insert_task`를 계속 호출한다 |

- **핵심 교훈 — store 테스트 6건은 게이트를 증명하지 못한다**: `fleet-store/tests/task_idempotency.rs`가 증명하는 것은 **행이 중복되지 않음**이다. 게이트가 요구하는 것은 **실행이 중복되지 않음**이고, 그 계약은 `Dispatcher::submit`의 조기 반환에만 존재한다. 처음에는 store 테스트만 있었는데, 그 상태에서 누군가 `IdempotentInsert` match를 `append_event` **아래로** 옮겨도 6건이 전부 초록으로 남는다. 그래서 디스패처 테스트 2건을 추가하고 **뮤테이션으로 값을 증명했다**:

  | 뮤테이션 | 결과 |
  |---|---|
  | `return Ok(existing.id)` 제거(아래로 흘림) | `duplicate_submit_...`이 `NoWorker`로 실패 — 워커 없는 픽스처(`setup_no_workers(0)`)를 쓴 것이 의도적이다. 중복 경로가 새면 반환값만으로 드러난다 |
  | 조기 반환은 유지하되 `append_event`를 그 **앞**에 삽입 | **이벤트 수 단언만이** 잡았다(`left: 0, right: 1`). 다른 모든 단언과 store 테스트 6건은 이 변형에서 초록이다 — 행은 여전히 하나이므로 |

- **대시보드 응답에 `deduplicated`를 추가했다**: MCP에는 이미 있었지만 대시보드에는 없었다. 없으면 **신호가 거짓이 된다** — 이미 `Completed`인 중복을 돌려주면 핸들러의 `actually_dispatched`가 false가 되고 `warning` 필드는 없는데, 같은 함수의 doc 주석이 **바로 그 조합을 "재시도 예약됨"으로 정의한다**(`#38`). 끝난 작업을 "재시도 대기 중"으로 보고하는 셈이었다. Dispatcher API를 넓히지 않고 `id != task_id`로 판정한다 — `submit()`이 중복을 흡수하면 최초 id를 돌려주므로 그 차이 자체가 신호다. 제출 **전** 단계(권한·CSRF·project admission)는 전부 읽기 전용임을 확인했다 — 중복 판정 전에 감사 행이나 카운터가 남는 경로는 없다.

- **검증 한계**:

  | 무엇 | 내용 |
  |---|---|
  | 대시보드 UI에는 이 필드의 입력 컨트롤이 없다 | `idempotency_key`는 JSON/폼 API 소비자를 위한 것이라 **브라우저 클릭으로는 이 코드에 도달할 수 없다.** 그래서 브라우저 왕복 대신 HTTP 수준에서 검증했다 — `spawn_dispatcher_server_with_store`가 실제 axum 서버와 실제 `Dispatcher`를 띄우므로 라우팅·CSRF·핸들러·디스패처·store를 관통하는 진짜 왕복이다. curl 수동 확인보다 강하다(CI에서 반복된다) |
  | 동시 제출 부하에서 검증되지 않았다 | 테스트는 재제출을 **순차로** 겹친다. 두 요청이 정확히 같은 순간 같은 키로 들어오는 경우의 원자성은 Postgres 부분 유니크 인덱스 + `ON CONFLICT DO NOTHING`이 보장하는 것이지 이 테스트가 증명하는 것이 아니다. MemStore는 단일 lock 아래 O(n) 스캔으로 같은 의미를 흉내내며, 이쪽은 부하 특성이 다르다 |
  | `ON CONFLICT`의 부분 인덱스 추론 | Postgres는 부분 유니크 인덱스를 추론하려면 `ON CONFLICT` 절에 **술어를 그대로 반복**해야 한다. 빠뜨리면 런타임에 "no unique or exclusion constraint matching"으로 죽는다 — 실제 PostgreSQL 실행으로만 잡히고 MemStore 통과로는 보이지 않는다 |

- **환경 결함 2건(다음 에이전트가 그대로 밟는다)**:

  | 증상 | 원인과 조치 |
  |---|---|
  | `brew services start postgresql@16` → `Bootstrap failed: 5: Input/output error`, `pg_ctl start` → `FATAL: postmaster became multithreaded during startup` / `HINT: Set the LC_ALL environment variable to a valid locale.` | 로케일 미설정. **`export LC_ALL=C`** 후 `pg_ctl -D /opt/homebrew/var/postgresql@16 start`로 기동된다 |
  | `fleet-mcp/tests/cross_client.rs` 14건 중 13건이 `DATABASE_URL` 아래에서 실패, 표면 패닉은 `no initialize response`(무관한 메시지) | 단일 테스트로 좁혀서 진짜 원인을 봤다: `migration 24 was previously applied but is missing in the resolved migrations`. 이 스위트는 미리 빌드된 `target/debug/fleet`을 spawn하는데 `sqlx::migrate!`는 마이그레이션 파일을 **컴파일 시점에** 박아 넣는다 — 낡은 바이너리는 023까지만 알고 DB에는 024가 적용돼 있었다. **`cargo build -p fleet-cli --all-features`** 로 해소. 제품 결함이 아니다. 마이그레이션을 추가하는 사람은 누구나 이걸 밟는다 |

- **검증**: `rustc 1.98.0`(고정과 일치), `cargo fmt --all -- --check` 통과, clippy 2종(`acp mtls` / `--no-default-features`, 둘 다 `RUSTFLAGS="-D warnings"` + `-- -D warnings`) 경고 0, `env -u DATABASE_URL cargo test --workspace` 실패 0, 그리고 `DATABASE_URL`을 물린 5개 크레이트(`fleet-store`·`fleet-api`·`fleet-scheduler`·`fleet-mcp`·`fleet-dashboard`) `--test-threads=1` 직렬 재실행 전부 통과. 마이그레이션 024가 실제로 적용되는 것과 스키마 객체를 직접 확인했다 — `idx_tasks_idempotency UNIQUE, btree (created_by, idempotency_key) WHERE idempotency_key IS NOT NULL`, `CHECK ((idempotency_key IS NULL) = (idempotency_payload_hash IS NULL))`.
- **미룬 것**: 정본 게이트 4~10(`TaskAttempt` 엔티티, effect ledger, 이전 control epoch 이벤트 거부, 비가역 tool effect 재실행 금지). 이유는 2026-08-25 1단계 항목의 표와 동일하며 그대로 유효하다. principal 부재는 `#93`과 함께 풀어야 한다.

## 2026-08-26 — ingest — `#96` Task 삭제와 스레드 그룹 목록 설계 확정

사용자 요청("Task 삭제를 지원하고 Task 리스트를 최초 Task의 하위 목록으로 grouping")을 설계로 확정했다.
구현은 착수하지 않았다 — 요청이 "설계하자"였고, 아래 결정들이 코드보다 먼저 정본에 있어야 한다.

- **스키마가 이미 삭제 정책을 갖고 있었다.** 새로 정할 캐스케이드가 없다: `task_outputs`(001:49)·
  `task_telemetry`(016:13)는 CASCADE, `events`(001:61)·`tasks.parent_task_id`(013:17)·
  `issue_task_links`(023:74)는 SET NULL. `023_issues.sql`이 그 선택 근거까지 남겨 뒀다. 이것이
  **soft delete를 배제한 이유**다 — `deleted_at` 컬럼을 두면 이 다섯 절이 전부 죽은 코드가 된다.

- **삭제 판정은 선검사가 아니라 SQL 술어다.** `DELETE FROM tasks WHERE id = $1 AND status_phase =
  ANY($2)`. 읽고-나서-지우면 그 사이에 `Dispatched → Completed`가 끼어들 수 있고(두 writer, 공유
  트랜잭션 없음) 이는 `#62` 1단계가 CAS로 없앤 것과 **같은 종류의 TOCTOU**다. CAS를 집안 관례로
  문서화한 바로 다음 작업에서 그 관례를 어기는 코드를 낼 수는 없다.

- **`Dispatched` 삭제를 막는 이유는 정합성이 아니라 침묵이다.** 1단계 CAS 덕분에 저장소는 손상되지
  않는다 — 워커의 늦은 완료 이벤트는 `UPDATE` 0행 → **이어지는 재조회**가 `None` → `Err(NotFound)`로
  귀결된다(0행이면 재조회는 항상 일어난다. 거절과 부재를 구분하는 것이 그 조회의 존재 이유다).
  문제는 디스패처의 `Err` 갈래가 `warn!` 한 줄만 남기고 이벤트를 발행하지 않는다는 것이다. 워커는
  머신을 끝까지 태우고 그 결과는 어떤 기록도 없이 사라진다.

- **삭제를 막는 유일한 것은 `Pending` 의존자다.** `dependency_ids`는 `UUID[]`이고 FK가 없어 DB가 막아
  주지 않는다. dispatch 준비 판정은 선행 Task 조회가 `Ok(None)`이면 `ready = false`로 끝내는데, 없는
  행은 영영 생기지 않으므로 그 의존자는 dead-letter도 timeout도 없이 `Pending`에 **영구히 갇힌다**.
  terminal 의존자는 검사하지 않는다 — 이미 실행이 끝나 ready 판정을 다시 지나지 않고, 전부 검사하면
  DAG가 조금만 깊어져도 삭제가 사실상 불가능해진다.

- **멱등성 키는 삭제 시 해제된다.** 게이트 3의 보장은 "중복 제출은 *기존 Task를 반환한다*"이다.
  반환할 Task가 없어진 뒤에 tombstone을 남기면 클라이언트에게 조회하면 404가 되는 id를 건네게 된다 —
  보장을 지키는 게 아니라 더 나쁘게 깨뜨리는 것이다.

- **그룹의 정체성은 루트 행이 아니라 `thread_id` 값이다.** `parent_task_id`가 `ON DELETE SET NULL`
  이므로 "루트가 삭제된 스레드"는 가정이 아니라 도달 가능한 정상 상태다. 루트 기준으로 그룹을 잡으면
  그 자식들은 `id != thread_id`라 루트 목록에도 없고 어느 그룹에도 못 붙어 **목록에서 통째로 증발**
  한다. 값 기준으로 잡으면 그저 "헤더 없는 그룹"이고 특수 질의 경로가 필요 없다.

- **필요한 마이그레이션은 하나뿐이고, 없어도 되는 것 셋을 확인했다.**

  | 항목 | 마이그레이션 | 이유 |
  |---|---|---|
  | `dependency_ids` 부분 GIN | **필요** | 의존자 검사가 `@>` 포함 질의를 요구하는데 이 컬럼엔 인덱스가 없다. 저장소 전체 GIN은 `workers.labels` 하나뿐. `WHERE dependency_ids <> '{}'`로 좁힌다 — 대부분 `DEFAULT '{}'`이고, `idx_tasks_parent_task_id`가 쓰는 관례와 같다 |
  | `PermissionKind::TaskDelete` | 불필요 | permission은 코드 정의이고 `seed_permissions`/`seed_builtin_roles`가 **매 기동** 실행되어 기존 역할까지 역채움한다 |
  | 그룹 질의 | 불필요 | `idx_tasks_thread_id ON tasks (thread_id, created_at)`가 013에 이미 있다 — 정렬까지 맞는 복합 인덱스다 |
  | `Deleted` 상태 | 불필요 | 삭제는 상태 전이가 아니라 레코드 제거다. 상태 다이어그램에 칸을 늘리지 않는다 |

- **`task.*` 감사 액션이 하나도 없었다.** auth·user·worker·credential만 있다. `task.delete`를 새로
  만든다 — hard delete는 행 자체를 없애므로, 분리되어 남는 `events` 행을 빼면 감사 로그가 "이 Task가
  존재했고 누가 지웠다"를 증언하는 유일한 기록이 된다.

- **의도적으로 남긴 것**:

  | 무엇 | 왜 |
  |---|---|
  | 스레드 통째 삭제 버튼 | 삭제마다 의존자 선검사가 개별적으로 실패할 수 있다. 부분 실패 의미론을 먼저 정하지 않은 일괄 버튼은 "몇 건은 지워지고 몇 건은 남은" 상태를 사용자에게 설명하지 못한다 |
  | `delete_task`의 `Store` 기본 구현 | `list_thread_tasks`는 `list_tasks`로부터 **유도 가능**하므로 기본 구현이 정당하고 목 스토어 4개가 재정의 없이 정확하다. 삭제는 유도 불가능하다 — 조용히 아무것도 안 하는 기본 구현은 목 스토어를 거짓말하게 만든다. 필수 메서드로 두고 6개 impl 전부가 명시적으로 답하게 한다 |
  | 깊이 있는 들여쓰기 | `thread_id`는 평평한 키라 손자 세대도 루트와 같은 값을 갖는다. 깊이를 그리려면 `parent_task_id`를 재귀로 훑어야 하는데, 그 재귀를 피하려고 `thread_id`를 도입했다(`013_task_threads.sql`) |

- **검증 한계**: 이 항목은 **설계만** 확정했다. 코드 변경이 없으므로 실행으로 증명된 것은 없다. 위
  주장들의 근거는 전부 현재 저장소의 코드·마이그레이션 실측이다(FK 절 5개, `dispatch_ready_tasks`의
  `ready = false` 경로, `compare_and_set_task_status`의 0행 처리, `seed_builtin_roles`의 재시드,
  `idx_tasks_thread_id` 존재, `dependency_ids` 인덱스 부재, `audit.rs`의 `task.*` 부재). 구현 시
  게이트는 `management.md` 삭제 계약 절 하단의 6개다.

- **문서 갱신**: `architecture/tasks/management.md`(삭제 계약 절 신설, API·감사 절과 게이트 반영),
  `architecture/tasks/execution-consistency.md`(멱등성 키 해제, 게이트 1건 추가),
  `ui-dashboard/ui-design.md` §3.3(태스크 큐를 스레드 그룹 구조로 재설계),
  `contracts/dashboard-api.md`(계획된 표면 절 신설 — "현재 route 표면" 표에는 넣지 않았다. 미구현
  route를 그 표에 적으면 표 제목이 거짓이 된다), `roadmap/roadmap.md`(`#96` 등록).

## 2026-08-26 — lint — `#96` 설계의 근거 두 곳을 실측과 대조해 정정

이전 항목("`#96` Task 삭제와 스레드 그룹 목록 설계 확정")의 두 주장을 코드·스키마 재실측으로
점검한 결과 둘 다 과장이었다. 커밋 없이 문서만 고친다.

- **`events` 보존 주장이 근거보다 강했다.** `001_init.sql`을 다시 읽으니 `events.task_id`는
  `ON DELETE SET NULL`만 있고 `task_label` 같은 라벨 컬럼이 없다 — `issue_task_links.task_label`,
  `audit_log.actor_label` 관례는 011·023에서 생겼고 001에는 소급되지 않았다. 그래서 삭제된 Task의
  `events` 행은 "보존되어 분리"가 아니라 **익명으로 남는다**: 사건의 존재는 남지만 어느 Task의
  사건이었는지는 잃는다. `management.md`의 캐스케이드 표·설명과 `#96` 감사 논거("`events`를 빼면
  감사가 유일한 기록")를 이 사실에 맞춰 고쳤다 — 오히려 주장은 더 강해진다: `events`를 뺀 나머지가
  아니라, **예외 없이** `task.delete` 감사 이벤트가 유일한 식별 가능 기록이다. 라벨 컬럼 추가는
  범위 밖으로 남긴다(append-only 로그의 과거 행을 채울 방법이 없고, 새 라벨은 쓰기 경로 변경을
  요구하는 별도 결정이다 — 채울 방법이 없는 컬럼을 미리 만들지 않는다는 원칙과 같다).

- **부분 GIN 인덱스가 계획한 질의에서 실제로 쓰이는지 확인하지 않았다.** `idx_tasks_parent_task_id
  WHERE parent_task_id IS NOT NULL`을 전례로 들었지만, 그 인덱스가 쓰이는 이유는 `parent_task_id =
  $1`이 `IS NOT NULL`을 **동등 비교로** 함의한다는 걸 planner가 증명할 수 있어서다. 배열 포함
  (`dependency_ids @> ARRAY[$1]`)은 이 추론 규칙에 없다 — `dependency_ids <> '{}'` 조건을 질의에
  같이 쓰지 않으면 planner가 인덱스 조건을 증명하지 못해 시퀀셜 스캔으로 떨어지고, 인덱스를 추가한
  목적 자체가 무효화된다. `management.md`에 의존자 조회 질의를 `WHERE status_phase = 'pending' AND
  dependency_ids <> '{}' AND dependency_ids @> ARRAY[$1]::uuid[]` 형태로 명시해, 구현자가 두 번째
  조건만 쓰고 인덱스를 조용히 잃는 경로를 막았다.

- **부기 정정**: `management.md`의 `verification` 필드가 `design-reviewed`였는데, 근거가 전부
  코드·스키마 실측(FK 절, CAS 0행 경로, 인덱스 존재 여부, `audit.rs`)이라 `dashboard-api.md`와 같은
  등급인 `code-checked`로 고쳤다. `roadmap.md` `#96`의 상태 토큰 "설계 완료"는
  [Roadmap 도메인](./roadmap/README.md)이 정의한 값 집합에 없어 "설계 확정·구현 대기"로 바꿨고,
  `roadmap.md` 자체의 `last_verified`가 `#96` 등록 이전 날짜(2026-08-18)로 남아 있던 것도 갱신했다.
  `dashboard-api.md`의 `last_verified_commit`은 `working-tree`였는데 이미 `0b641c7`로 커밋된
  내용이므로 실제 커밋 해시로 채웠다.

- **검증 한계**: 이번에도 코드 변경은 없다. 두 정정 모두 마이그레이션·쿼리를 실행해 planner의 실제
  계획(`EXPLAIN`)을 확인한 것이 아니라 Postgres 문서화된 추론 규칙에 근거한 서면 판단이다. 구현
  단계에서 `EXPLAIN (ANALYZE)`로 인덱스가 실제로 쓰이는지 확인하는 항목을 완료 게이트에 아직
  추가하지 않았다 — `management.md` 게이트 목록은 결과(거부/허용) 테스트만 요구하고 실행 계획
  검증은 요구하지 않는다.

## 2026-08-26 — lint — `#96` 구현 착수 중 `events` 보존 판정을 다시 정정

바로 위 항목("`#96` 설계의 근거 두 곳을 실측과 대조해 정정")에서 "삭제된 Task의 `events` 행은
익명으로 남는다"고 고쳤는데, 이번엔 그 정정 자체가 틀렸다. `delete_task`를 구현하며
`list_events`(`crates/fleet-store/src/postgres.rs`)를 실제로 읽어 보니, 이 메서드는
`SELECT seq, payload FROM events ...`만 실행하고 `payload` JSONB를 역직렬화해 `FleetEvent`를
복원한다 — `task_id` **컬럼**은 아예 조회하지 않는다. `FleetEvent`의 `TaskCreated`/`TaskDispatched`/
`TaskProgress`/`TaskCompleted`/`TaskFailed`/`TaskCancelled` variant는 모두 `task_id: TaskId`를 필수
직렬화 필드로 갖고 있고(`crates/fleet-core/src/events.rs`), 이 값은 `payload` JSONB 안에 그대로
박혀 있다. Task를 삭제해도 `ON DELETE SET NULL`이 건드리는 것은 `events.task_id` **컬럼**뿐이고,
`payload` JSONB 내용은 어떤 FK 액션으로도 바뀌지 않는다. 그리고 `task_id` 컬럼을 조회 조건으로
쓰는 코드는 워크스페이스 어디에도 없다(grep으로 확인) — 그 컬럼은 사실상 write-only다.

즉 실제 동작은: **신원은 잃지 않는다.** SET NULL이 지우는 것은 인덱싱/조인 전용 컬럼 하나뿐이고,
`payload`를 읽으면 어느 Task의 사건이었는지 그대로 복원된다. 다만 `task_id` 컬럼이 NULL이라
인덱스를 타는 조회는 불가능하므로, "그 정보를 실용적으로 조회할 수 있는가"라는 관점에서는 여전히
감사 로그(`actor`/`target`이 인덱스가 있는 자리에 남는다)가 유일한 경로다 — "정보가 아예 없다"와
"정보는 있지만 인덱스 없이는 스캔해야 찾는다"는 서로 다른 주장이며, `management.md`·`roadmap.md`의
캐스케이드 표·감사 논거·완료 게이트 문구를 이 구분에 맞춰 다시 고쳤다. `#96` 설계를 위해 작성한
`crates/fleet-core/src/audit.rs`의 `action::TASK_DELETE` 문서 주석과
`crates/fleet-store/src/postgres.rs`의 `delete_task` 구현 주석도 커밋 전에 같이 고쳤다 — 잘못된
주장이 코드 주석으로 굳어지기 전에 잡았다.

**이 항목 자체가 기록해 둘 교훈이다**: 같은 하루에 같은 주장을 두 번 고쳤고, 두 번째 교정이 첫 번째
교정을 도로 뒤집었다. 첫 정정(스키마 DDL만 읽음)은 컬럼 존재 여부만 확인하고 그 컬럼이 실제로 읽히는
경로가 있는지는 확인하지 않은 채 "익명"이라고 결론 내렸다 — 스키마 레벨 사실(라벨 컬럼 없음)에서
런타임 레벨 결론(신원 손실)으로 건너뛴 것이 오류였다. 스키마만 보고 데이터 보존 여부를 판정하지
않고, 그 컬럼을 실제로 쓰는 읽기 경로(`list_events`)까지 확인해야 한다는 것이 이번에 다시 확인된
교훈이다.

**검증 한계**: 여전히 코드는 실행하지 않았다 — `list_events`의 SQL과 `FleetEvent`의 필드 정의를
읽고 정적으로 추론한 결론이다. `delete_task` 구현이 끝나면 완료 게이트의 통합 테스트가 실제로
Task를 지운 뒤 `events`를 읽어 `payload.task_id`가 보존되는지 실행으로 확인해야 이 결론이
최종적으로 검증된다.

## 2026-08-26 — verification — `#96` 실행 검증: 게이트 4종 전부 통과, `events.payload` 보존 결론이 실행으로 확정됨

- 유형: `verification`
- **무엇을 했나**: 바로 위 항목이 정적 추론으로 남긴 결론("`delete_task` 구현이 끝나면 완료 게이트의
  통합 테스트가 실제로 Task를 지운 뒤 `events`를 읽어 확인해야 한다")을 실행으로 닫았다. 이미 작업
  트리에 구현돼 있던 `#96`의 backend(`Store::delete_task`/`list_task_threads`, 대시보드 핸들러·라우트,
  migration 025)와 frontend(스레드 그룹 목록 UI)를 대상으로 표준 4종 게이트를 실제로 실행했다.
- **DB 게이트 하네스 함정을 다시 밟지 않았다**: 새로 `createdb`한 빈 DB로 시작해 `PgStore::migrate()`가
  001부터 025까지 전부를 처음부터 적용하게 했고(스키마가 이미 025까지 존재하는 DB를 재사용했다면
  이 신규 마이그레이션이 실제로 처음부터 실행되는지 확인하지 못했을 것이다), 2026-08-24 항목이 기록한
  "이 저장소 DB 테스트 하네스의 기존 특성"(11~이제 14개 DB-gated 바이너리가 같은 Postgres를
  `TRUNCATE ... CASCADE`로 공유해 병렬 실행 시 경합)을 그대로 따라 매 크레이트의 모든 테스트 바이너리를
  `--test-threads=1`로, 바이너리별 별도 프로세스로 격리 실행했다. 격리 없이 `cargo test -p fleet-store
  --features test-support`를 한 번에 돌렸을 때는 실제로 `admin_token_rotation`의
  `rotate_invalidates_previous_digest_and_activates_new_one`이 `NotFound` panic으로 실패했다 — `#96`과
  무관한, 문서화된 하네스 특성의 재현이다. 격리 실행에서는 fleet-store 14개 바이너리 전부 `ok`.
- **결과 — 4종 게이트 전부 초록**:
  - `rustc --version` → `1.98.0`, `rust-toolchain.toml`의 고정 채널과 일치.
  - `RUSTFLAGS="-D warnings"` 아래 `cargo fmt --all -- --check` → 통과.
  - `cargo clippy --workspace --features "acp mtls" --all-targets -- -D warnings` → 경고 0.
  - `cargo clippy --workspace --no-default-features --all-targets -- -D warnings` → 경고 0.
  - `cargo test --workspace`(DB 미주입) → 68개 테스트 스위트 전부 `ok`, 실패 0.
  - `DATABASE_URL` 주입 후 `--test-threads=1` 격리 실행: `fleet-store` 14 바이너리, `fleet-api` 14,
    `fleet-scheduler` 2, `fleet-mcp` 1(`cargo build -p fleet-cli --all-features`로 먼저 재빌드 —
    2026-08-25 항목이 남긴 "낡은 `target/debug/fleet` 바이너리" 함정 재확인·회피), `fleet-dashboard` 2 —
    전부 `ok`. 2026-08-25 항목이 CI에서 관측한 4건의 실패 중 `scaleout_sync`(LISTEN/NOTIFY 타이밍)와
    `cross_client.rs` 2건(stale-binary 가설)은 이번 로컬 실행에서 **재현되지 않았다** — 둘 다 `ok`였다.
    stale-binary 가설이 맞았다는 뜻일 수도, 다른 조건(로컬 macOS vs CI 컨테이너, 매번 새로 빌드한
    바이너리)이 달라 우연히 안 걸렸다는 뜻일 수도 있다 — 이번 실행은 그 3건이 `#96`과 무관함만
    확인했을 뿐, CI에서 실제로 고쳐졌는지는 검증하지 않았다. 나머지 1건(`projects.rs`의
    `create_and_get_project_roundtrip`, 이번 실행 범위인 fleet-store 게이트에 포함됨)은 위 두 건과
    같은 급으로 취급하면 안 된다 — 이건 원인 불명의 flake가 아니라 이 파일 1468행이 이미 근본 원인을
    특정해 둔 결함이다: `project.rs:87`의 `Utc::now()`는 나노초 정밀도를 만드는데 Postgres
    `timestamptz`는 마이크로초로 저장하고, 테스트는 왕복값과 원본을 `assert_eq!`로 구조체 전체
    비교한다. 이번 통과는 그 나노초 하위 자리가 이번엔 우연히 0이었거나(혹은 이 머신의 시계 해상도가
    마이크로초 단위라 애초에 그 자리가 항상 0이거나) — 둘 중 어느 쪽인지 이번 실행에서 확인하지
    않았다. 어느 쪽이든 코드(`project.rs:87`, `projects.rs:73`)는 그대로이므로 이 결함 자체가 고쳐진
    것은 아니고, 로컬에서 통과했다는 사실이 CI 재발 여부에 대해 증거가 되지도 않는다.
- **`events.payload` 보존 결론이 실행으로 확정됐다**: `crates/fleet-store/tests/task_delete.rs`의
  `delete_task_cascades_outputs_and_telemetry_and_nulls_event_task_id_column`이 실제 PgStore로 Task를
  지운 뒤 `SELECT task_id, payload FROM events WHERE payload->>'task_id' = $1`로 재조회해
  `task_id` 컬럼은 `None`(`ON DELETE SET NULL`), `payload.task_id`는 원본 UUID 그대로임을 함께
  단언하며 통과했다. 이로써 이 파일의 두 항목 전(0826 초판 → 정정)이 정적 추론으로 도달한 결론이
  이번에 실제 실행으로 재확인됐다 — 세 번째 판정이 아니라 앞선 정정을 실행이 뒷받침한 것이다.
- **완료 게이트 6개와 테스트의 대응을 직접 확인했다**(`docs/architecture/tasks/management.md` "구현
  순서와 검증 게이트" 절): ① 비terminal 거부가 0행 판정임 → `delete_task_rejects_a_non_terminal_task`
  + PgStore `delete_task`가 선검사 없이 단일 `DELETE ... AND status_phase = ANY($2)`로 구현된 것을
  코드로 확인. ② Pending 의존자 차단/terminal 허용 → `delete_task_blocked_by_pending_dependents` +
  `delete_task_removes_a_terminal_task`. ③ 루트 삭제 후 자식 `parent_task_id` NULL·`thread_id` 유지·
  스레드 생존 → `task_delete.rs`의 게이트 3 테스트 + `dashboard_api.rs`의
  `list_task_threads_survives_a_deleted_root`. ④ `events`/`task_outputs`/`task_telemetry` 캐스케이드 →
  위 문단. ⑤ 비admin 403 → `delete_task_requires_task_delete_permission`. ⑥ 성공·거부 감사 →
  `delete_task_records_a_task_delete_audit_event_on_success_and_rejection`. 여섯 게이트 모두 이름이
  일치하는 테스트가 있고 전부 통과했다.
- **`fleet serve`를 백그라운드로 띄울 때의 운영 함정 2건을 새로 발견해 기록한다** (제품 결함이 아니라
  이 실행 환경 한정 함정이지만, 다음에 브라우저로 대시보드를 검증할 에이전트가 반복해서 밟을 것이다):
  1. `FLEET_MCP_CAPABILITIES`가 비어 있거나 인식 못 하는 토큰을 담고 있으면(`McpAuthorization::
     from_environment`가 fail-closed) MCP 컴포넌트가 즉시 죽는데, `fleet serve`는 MCP와 대시보드를
     같은 join된 future로 묶어 실행해서 **대시보드까지 함께 죽는다** — 대시보드 로그에 "listening"이
     찍힌 뒤에도. 동작하는 값은 `crates/fleet-mcp/tests/cross_client.rs:67`에 있다.
  2. 유효한 capability 값을 줘도, `nohup cmd &`로 백그라운드 실행하면 자식 프로세스의 stdin이
     `/dev/null`이 되어 MCP stdio 리더가 즉시 EOF를 보고 "정상" 종료하는데, 이 정상 종료도 같은 이유로
     대시보드를 함께 끌고 내려간다. `nohup sh -c 'tail -f /dev/null | ./target/debug/fleet serve ...'
     &`처럼 stdin을 무한히 열어 둬야 살아남는다.
  두 함정 모두 `fleet serve`가 MCP와 대시보드를 분리 불가능하게 묶은 설계에서 나온다 — 이번 범위
  밖이지만, MCP 없이 대시보드만 띄우고 싶은 다음 상황(예: 대시보드 전용 헬스체크·로컬 UI 검증)에
  반복해서 부딪힐 문제라 여기 남긴다.
- **검증 한계**: CI에서 아직 이 실행을 재현하지 않았다 — 로컬(macOS, 1.98.0)에서만 확인했다. 위에서
  언급했듯 2026-08-25 CI 관측 4건 실패가 이번엔 로컬에서 재현되지 않았는데, 그게 CI에서도 고쳐졌다는
  뜻인지는 다음 CI 실행으로만 확인된다.

## 2026-08-26 — ingest — #63 3단계: graceful lease 반납, 라이브 failover 검증, 승격 Runbook

- **구현**: `LeaseManagerConfig.shutdown_grace`(기본 5초)와 `CancellationToken` 기반
  `LeaseManagerHandle::shutdown()`을 추가했다. 반납을 핸들이 직접 하지 않고 **갱신 루프 안에서**
  한다 — `release`는 현재 Active epoch로 CAS하는데 핸들에서 부르면 두 태스크가 같은 상태를 읽고,
  읽은 직후 루프가 재획득해 epoch가 바뀌면 무효한 epoch로 반납하게 된다. 루프 안에서 하면 epoch를
  소유한 주체가 유일해 그 경쟁 자체가 성립하지 않는다. 종료 신호는 **대기 지점**(`interval.tick()`,
  `sleep`)에서만 관측하고 `try_acquire`/`try_renew` 주변에서는 보지 않는다 — DB future를 중간에
  버리면 커밋됐는지 알 수 없는 lease가 남는다. `abort()`의 의미(비정상 종료 흉내)는 손대지 않았고,
  두 의미가 뭉개지지 않도록 대칭 테스트 2건으로 잠갔다.
- **`shutdown()`은 그전까지 도달 불가능한 코드였다.** `fleet-cli::run_serve`에 신호 핸들러가
  없어서 stdin EOF 경로만 존재했다. `wait_for_shutdown_signal()`(unix는 `SIGTERM`/`SIGINT`,
  그 외는 `ctrl_c`)을 넣고 MCP 서버 실행을 `tokio::select!`로 감쌌다.
- **라이브 검증에서만 드러난 결함 — blocking stdin 때문에 프로세스가 종료되지 않는다.**
  신호 처리를 넣은 직후의 실행에서 lease는 반납됐는데(`control plane lease released on graceful
  shutdown`) 프로세스가 살아남았다(`ps` 상태 `SN`). 원인은 `crates/fleet-mcp/src/server.rs`의
  `tokio::io::stdin()`이다 — 이것은 전용 **blocking 스레드**에서 `read(2)`를 호출하므로
  `select!`가 future를 버려도 이미 커널에 들어간 read는 취소되지 않고, tokio 런타임의 종료는 그
  blocking 태스크가 끝나기를 기다린다. stdin이 닫히지 않는 배포(systemd 파이프, 터미널,
  `tail -f`)에서는 그 스레드가 영원히 끝나지 않는다. 운영자 입장에서 이것은 `systemctl stop`이
  타임아웃 뒤 `SIGKILL`로 승격되는 형태로 나타난다. 모든 정리와 lease 반납이 끝난 신호 경로 끝에
  `std::process::exit(0)`을 두어 해소했다.
  **이 부류의 결함은 단위 테스트로 잡히지 않는다** — 테스트 하네스는 프로세스를 새로 띄우지
  않는다. 라이브 2-프로세스 검증을 하지 않았다면 그대로 배포됐을 것이다.
- **`shutdown_grace` 초과 분기가 약속과 다르게 동작하고 있었다.** 주석은 "포기하고 abort한다"고
  했지만 `timeout(grace, self.inner)`는 만료 시 `JoinHandle`을 drop할 뿐이고, tokio에서
  `JoinHandle`의 drop은 취소가 아니라 **detach**다. 그대로 두면 종료 중인 프로세스의 갱신 루프가
  계속 살아 lease를 갱신하고, "반납도 못 했고 TTL 만료도 오지 않는" 상태가 된다 — 주석이 약속한
  fallback 자체가 성립하지 않는다. `abort_handle()`을 미리 잡아 timeout 분기에서 명시적으로
  abort하도록 고쳤다. 오늘은 위의 `process::exit(0)`이 이 결함을 가리고 있었지만 `shutdown()`은
  공개 API이므로 `exit`하지 않는 호출자에게는 그대로 노출된다.
- **라이브 failover 검증** (실제 PostgreSQL, `fleet serve` 프로세스 2개, `fleet_failover_63`):

  | 시각 (UTC) | 인스턴스 | 관측 |
  | --- | --- | --- |
  | 01:38:59.017 | A | lease acquired epoch=1 (ttl=15s) |
  | 01:39:01.079 | B | lease manager 기동 — 획득하지 못함 (구현 게이트 1) |
  | 01:39:04.028 | A | renewed epoch=1, expires_at=01:39:19.024 |
  | 01:39:05.169 | A | `SIGTERM` 수신 |
  | 01:39:05.171 | A | lease released (신호로부터 1.7ms), 프로세스 실제 종료 |
  | 01:39:07.094 | B | lease acquired epoch=2 |

  B의 승격은 `SIGTERM`으로부터 1.93초, 그 시점 TTL이 13.85초 남아 있었다 — 승격이 TTL 만료로는
  설명되지 않으므로 명시적 반납이 실제로 인수인계를 앞당겼다는 증거다.
- **문서를 코드에 맞춰 정정했다.** 정본이 "자동 failover는 지원하지 않는다"고 못박고 있었지만,
  `PgStore::acquire_control_lease`의 CAS 술어는 `expires_at < NOW()` **하나**뿐이라 lease가 왜
  만료됐는지를 구분하지 못한다. 그래서 실제 인수인계는 성격이 다른 두 경로로 갈린다 — 정상 반납은
  "전 소유자가 스스로 놓았다"는 명시적 증거를 남기지만, TTL 만료는 **시계만이 증거**다. 후자가
  정확히 "지원하지 않는 모델"의 *Primary fencing 없는 자동 failover*이고 불변식 3("Standby는 기존
  Primary의 종료 또는 fencing을 확인하기 전 제어 권한을 얻지 않는다")을 위반하는데, **코드가 그
  경로를 막지 않는다**. 계약 문장을 약화하지 않고, 정본에 [구현 상태와 유예](architecture/control-plane-authority-and-failover.md)
  절을 신설해 두 경로와 게이트 7개의 상태를 표로 남겼다. Runbook에는 "TTL 만료는 승격의 근거가
  아니다"와 `expires_at - last_renewed_at`으로 종료 유형을 판별하는 SQL을 넣었다(갱신은 두 값을
  `NOW()`/`NOW()+TTL`로 함께 쓰지만 반납은 `expires_at`만 당기므로, gap이 TTL이면 비정상 종료,
  `renew_interval` 이하면 명시적 반납 — 두 구간이 겹치지 않는다).
- **그 판별식은 전제 없이는 틀린다 — 리뷰에서 잡아 Runbook을 고쳤다.** 처음 쓴 문장은 gap만으로
  종료 유형을 읽게 했는데, 두 가지가 그것을 무너뜨린다. (1) 정상적으로 갱신 중인 **살아 있는**
  lease도 gap이 항상 정확히 TTL이다(갱신이 두 값을 같은 `NOW()` 기준으로 함께 쓰므로) — 만료
  여부를 함께 보지 않으면 건강한 Primary를 비정상 종료로 오독한다. (2) `control_plane_lease`는
  cluster당 한 행이고 획득의 `ON CONFLICT DO UPDATE`가 `active_instance_id`·`acquired_at`·
  `expires_at`·`last_renewed_at`을 전부 덮어쓰므로, **승격이 자신을 판정할 증거를 파괴한다** —
  Standby가 이미 lease를 얻은 뒤 운영자가 이 쿼리를 실행하면 새 소유자의 갓 얻은 lease를 보게
  되고, 그것도 gap == TTL이다. warm standby 배포에서 증거가 남아 있는 창은 `poll_interval`(기본
  3초) 수준이라 실무에서는 이미 덮어써져 있는 쪽이 흔하다. 그래서 쿼리에 `expires_at < NOW()`를
  추가하고, gap을 읽어도 되는 전제(소유자가 아직 죽은 Primary + 만료됨)와 그 밖의 경우에는 1단계
  fencing 증거가 유일한 입력이라는 것을 명시했다. 이 테이블은 소유권의 **현재 상태**이지 이력이
  아니다 — 종료 유형을 사후에 확실히 알려면 lease 이력을 따로 남겨야 하는데, 그것은 지금 없다.
- **정본의 `verification`은 `code-checked`로 둔다.** 라이브 2-프로세스 검증을 실제로 했으므로
  `integration-tested`를 붙일 유혹이 있었지만, 이 필드는 문서 **전체**의 주장에 대한 척도이고
  구현 게이트 7개 중 5개는 미착수다 — 그 등급을 붙이면 미착수 게이트까지 통합 검증된 것으로
  읽힌다. 저장소 관행과도 어긋난다(`implementation: partial`인 architecture 문서는 예외 없이
  `code-checked`이고, `integration-tested`를 쓴 문서는 아직 하나도 없다). 실제로 관찰한 것은
  본문 "라이브 검증 기록" 절이 게이트 번호와 함께 정확히 담고 있으므로, 등급을 올려서 얻을
  정보가 없다.
- **epoch 강제(불변식 4·5 / 구현 게이트 3 = `#62` 검증 게이트 4)는 `#67`에 귀속시켰다.**
  `epoch` 컬럼은 있고 승격마다 증가하지만 그것을 읽어 쓰기를 거르는 코드는 저장소에 하나도 없다
  (`dispatcher.rs`·`state.rs`·이벤트 경로 어디에도 epoch 술어가 없다). 창을 닫으려면 Worker
  이벤트가 자신이 어느 epoch에서 dispatch됐는지 싣고 돌아와야 하고, 그것은 `worker_execution_lease`의
  `fencing_token`을 요구한다 — `#67` 범위이고 아직 없다. 바인딩할 대상이 없는 채로 epoch 술어만
  넣으면 항상 참인 술어가 되어 게이트를 통과한 것처럼 보이는 죽은 검사가 된다.
- **검증 한계**: 라이브 실행이 관찰한 것은 **정상 종료 경로뿐**이다 — TTL 만료 인수인계, 수동 승격
  절차, Worker 재연결, reconciliation(구현 게이트 4)은 어느 것도 실행하지 않았다. Standby는 이미
  기동해 polling 중인 **warm** 상태였으므로 1.93초는 `poll_interval`(3초) 안의 수치이며, 계약이
  기술하는 Cold Standby(운영자가 승격 시점에 기동)의 수치가 아니다. `fleet-cli`의 신호 처리 경로를
  덮는 테스트가 없어 이 경로를 통째로 지워도 모든 게이트가 초록으로 남는다. `shutdown_grace` 초과
  분기도 DB 무응답을 재현할 하네스가 없어 테스트가 없다.

## 2026-08-26 — fix — `main` CI 적색 3건: 절삭·stale 바이너리·예산 대기, 그리고 "조용히 통과한 것"이 남긴 피해

- 유형: `fix`
- **무엇을 했나**: 위 "2026-08-25 — verification" 항목이 넘긴 4건(파일 기준 3건)을 모두 고쳤다.
  로드맵 항목이 아니므로 `#N`은 없다.
- **`projects.rs:73` — Postgres `timestamptz`의 마이크로초 절삭.** CI 로그가 값 차이까지 보여줬다:
  `…475167Z` vs `…475167661Z`, 나노초 `661`만 다르고 나머지 필드는 전부 같다. 구조체 전체를
  `assert_eq!`하는 이 왕복 테스트는 저장소 안에서 **유일한 예외**였다 — `auth_integration`과
  `issues`의 왕복 테스트는 예외 없이 필드별로 비교한다. 그래서 테스트를 관행에 맞추고, 시각은
  저장소가 실제로 보장하는 정밀도(`timestamp_micros()`)로 비교했다.
- **모델을 고치지 않은 이유**: `Project::new()`가 마이크로초로 절삭하게 만들면 왕복 불변식이
  실제로 성립하지만, 마이크로초 해상도는 **Postgres의 성질이지 `Project`의 성질이 아니다** —
  도메인 타입을 저장소 한계에 맞추는 것은 의존 방향이 거꾸로다. 게다가 저장소 전체에 시각 절삭
  처리는 하나도 없고(`trunc_subsecs`/`timestamp_micros`/`duration_trunc`/`with_nanosecond` 0건)
  `Utc::now()` 호출부는 20곳이 넘는다. 한 곳만 절삭하면 일관성이 없고, 전부 고치면 새 규약이
  생기는데 그것을 강제할 수단이 없다. 되읽은 시각을 자신이 쓴 값과 비교하는 코드(낙관적 잠금 등)가
  있으면 절삭은 제품 결함이 되므로 먼저 확인했다 — 없다. 검색에 걸린 셋은 전부 도메인 내부
  비교(`created_at == updated_at` 불변식, 조상 순서 비교)이지 저장-되읽기 대조가 아니다.
- **이 결함은 macOS 로컬 게이트로 원리상 잡을 수 없다.** macOS의 `Utc::now()`는 마이크로초
  해상도라 나노초 성분이 **항상 0**이다(실측: 5회 연속 `sub-micro nanos = 0`). 그래서 절삭이
  무손실이고 원래 코드도 로컬에서는 통과한다 — 수정 직후 로컬 8/8이 초록이었던 것도 수정의
  유효성을 증명하지 못한다. `agent.md` §4.3이 기록한 드리프트 두 사례(fmt 명령 누락, 툴체인
  부동)에 이은 **세 번째 계열**이고, 이번엔 게이트 목록이나 명령이 아니라 **OS 시계 해상도**라서
  목록을 CI와 아무리 똑같이 맞춰도 재현되지 않는다. 그래서 테스트가 나노초를 명시적으로
  주입하도록 고쳤다 — 이제 플랫폼과 무관하게 절삭 경로를 지난다. 주입 상태에서 구조체 전체
  비교를 임시로 되살리자 macOS에서 CI와 **동일한 실패**가 재현됐다(`…516204Z` vs `…516204661Z`).
- **`cross_client.rs:283`/`:460` — stale 바이너리. 가설이 실측으로 확정됐다.** `rm -f
  target/debug/fleet` 후 `cargo test --workspace --features "acp mtls" --no-run`을 끝까지 돌려도
  그 파일은 **생기지 않는다**. 즉 이 테스트가 실행하는 바이너리는 아무도 갱신을 보장하지 않는
  산출물이라는 앞 항목의 (2)가 사실이다. CI에는 `coverage` 잡에만 `cargo build -p fleet-cli`가
  있었으므로, `test`·`test-no-default` 두 잡에 각자의 피처로 같은 단계를 추가했다.
- **하네스의 조용한 skip을 없앴다 — 이것이 더 큰 결함이었다.** `spawn_server()`는 바이너리가
  없으면 `canonicalize().ok()?`로 `None`을 돌려 **테스트를 통과시켰다**(그 뒤의 `exists()` 검사는
  도달 불가능한 죽은 코드였다). `DATABASE_URL`이 주어졌다는 것은 이 환경에서 통합 테스트를
  돌리겠다는 뜻이므로, 그 상태의 바이너리 부재는 skip 사유가 아니라 **환경 결함**이다. 이제
  안내 메시지와 함께 panic한다.
- **잡별 실패 목록이 서로소였던 이유가 여기서 나온다.** `cargo test`는 실패한 테스트 바이너리에서
  멈추고, 잡마다 캐시 설정이 다르다.

  | 잡 | 캐시 키 | `target/debug/fleet` | 관측된 실패 |
  |---|---|---|---|
  | `test` (acp+mtls) | `restore-keys` 접두사 매칭 | 낡은 것이 복원됨 | `cross_client` 2건 — `projects.rs`는 **실행되지 않음** |
  | `test-no-default` | exact key만 | 없음 | `projects.rs` 1건 — `cross_client`은 **조용히 skip** |
  | `coverage` | 접두사 + 명시적 build | 최신 | `projects.rs` 1건 |

  가설 하나가 세 잡을 모두 설명한다. 그리고 `test-no-default`의 "통과"는 **한 건도 실행하지 않은
  통과**였다. 이 마지막 문장은 처음에 추론으로 적었다가 — 통과 **개수**는 skip과 실행을 구분하지
  못하므로 `14 passed`만으로는 성립하지 않는다 — CI 로그의 **소요 시간**으로 확정했다. 그 잡의
  `cross_client`은 `finished in 0.00s`, 타임스탬프로 `Running`부터 `test result`까지 **5.4ms**였다.
  같은 14건이 로컬에서 바이너리를 실제로 띄우면 **5.12s**다. subprocess를 14번 spawn하는 테스트가
  0.00초에 끝날 수는 없다. **조용한 skip은 개수가 아니라 시간에 흔적을 남긴다** — 이 테스트들은
  `#[ignore]`가 아니라 함수 본문에서 조기 `return`하는 방식이라 `0 ignored`로 보고되고, 러스트
  테스트 하네스에는 런타임 조기 반환을 skip으로 셀 방법이 없기 때문이다.

  `cross_client`을 먼저 고치면 `test` 잡에서 `projects.rs`가 새로 드러나므로, `projects.rs`부터
  고쳤다 — "게이트를 고치면 빨간 것이 늘어난다"가 이번에도 적용된다.
- **`scaleout_sync` — 제품이 아니라 테스트의 대기 방식 문제다.** 노드 B의 동기화는 도달 경로가
  둘이고 지연이 크게 다르다: Postgres LISTEN/NOTIFY(즉시)와, NOTIFY를 놓쳤을 때의 폴백
  폴링(`fleet-store`의 `FALLBACK_POLL_INTERVAL` = **5초**). 테스트는 `tokio::spawn(sync_b.run())`
  직후 곧바로 이벤트를 발행하는데, `tokio::spawn`은 태스크를 큐에 넣을 뿐 실행을 보장하지 않는다.
  spawn된 쪽이 `listener.listen()`에 도달하기 전에 NOTIFY가 나가면 그 알림은 **유실된다** —
  Postgres는 구독 이전에 발행된 알림을 전달하지 않는다. 그러면 수렴이 폴백 주기까지 늦춰지고,
  고정 200ms 예산은 그 경로에서 반드시 깨진다(25배 차이). 제품은 폴백 폴링을 갖고 있어 어느
  쪽으로든 수렴하므로 순서 버그가 아니다. `sleep`을 늘리는 것은 폴백 주기를 우연히 덮는 값을
  찾는 것이라 여전히 예산 대기이므로, 데드라인(15초) 안에서 조건이 참이 될 때까지 폴링하도록
  바꿨다. `agent.md` §3.3이 요구하는 분류로는 **테스트 하네스 결함**이다.
- 검증: `rustc 1.98.0`(CI와 동일), `RUSTFLAGS="-D warnings"`, `cargo fmt --all -- --check` 통과,
  clippy 두 조합 모두 경고 0. DB 주입 실측 — `projects` 8/8, `scaleout_sync` 1/1, `cross_client`은
  `fleet` 바이너리를 **acp+mtls로 빌드했을 때 14/14, no-default로 빌드했을 때도 14/14**. 후자는
  그 조합에서 이 테스트가 실행된 첫 사례다(지금까지는 바이너리가 없어 전부 skip됐다).
- **검증 한계**: `scaleout_sync`가 통과한 경로는 NOTIFY 쪽이다(0.12초에 종료). 폴백 폴링 경로는
  spawn과 발행의 순서를 인위적으로 뒤집을 하네스가 없어 실행하지 못했다 — 데드라인이 5초보다
  넉넉하다는 것은 상수 비교로만 확인했다. CI의 새 build 단계가 stale을 실제로 없애는지는 다음
  실행을 관측해야 확정된다. `projects.rs` 수정이 Linux에서 통과하는 것도 아직 CI 미관측이며,
  로컬에서 확인한 것은 "나노초가 있는 상태에서 필드별 비교가 견딘다"까지다.
- **같은 드리프트를 CI에만 고치고 끝낼 뻔했다.** build 단계를 `ci.yml`에만 넣으면 로컬 게이트에는
  없는 전제가 CI에만 생긴다 — 게다가 하네스가 이제 조용히 skip하는 대신 panic하므로, 그 상태의
  로컬은 **전보다 더 시끄럽게** 깨진다. `agent.md` §4.3이 두 번 적어 둔 "게이트 목록이 CI보다
  약하면 드리프트는 반드시 재발합니다"에 정확히 해당하므로, §3.2에 잡별 피처로 `cargo build -p
  fleet-cli`를 먼저 돌리라는 전제를 명령과 함께 넣고 §4.3에 이번 사례를 (3)으로 기록했다. 앞선
  두 사례가 각각 **목록이 짧아서**(fmt 누락)와 **환경이 달라서**(툴체인 부동) 생긴 것이라면,
  이번 것은 **의존성이 빌드 시스템에 보이지 않아서** 생긴 세 번째 종류다.
- **CI 관측으로 위 세 가지가 모두 닫혔다** (run `32924661170`, `4e0ecd0`). 네 잡 전부 초록이지만,
  초록만으로는 이번에도 조용한 skip과 구분되지 않으므로 로그의 소요 시간으로 확인했다.

  | 확인 대상 | 직전 실행 | 이번 실행 |
  |---|---|---|
  | `cross_client` (no-default) | `finished in 0.00s` — 전부 skip | **1.29s / 14 passed** |
  | `cross_client` (acp+mtls) | 2건 실패(낡은 바이너리) | **1.36s / 14 passed** |
  | `projects` (3잡) | 2잡 실패, acp+mtls는 미도달 | **0.91 / 0.86 / 0.84s, 각 8 passed** |
  | `scaleout_sync` (3잡) | 1건 간헐 실패 | **0.20~0.21s / 1 passed** |

  acp+mtls 잡이 `projects.rs`에 **도달한 것 자체가 이번이 처음이다** — 직전까지는 `cross_client`이
  먼저 죽어 그 뒤 바이너리가 실행되지 않았다. 그 뒤에 네 번째 실패가 숨어 있지 않았음이 이걸로
  확인됐고, `projects.rs`부터 고친 순서 판단도 근거를 얻었다.
- **로그를 읽다가 "시간으로 skip을 가린다"는 방법의 바로 옆 함정에 빠졌다.** 처음에 acp+mtls의
  `cross_client`을 `43 passed / 0.01s`로 읽었는데, 그 줄은 **직전 테스트 바이너리의 결과**였다.
  cargo는 `Running <바이너리>`를 stderr로, 테스트 결과를 stdout으로 쓰므로 CI 로그에서 두 스트림의
  순서가 뒤집힌다. `Running X` 다음에 오는 첫 `test result`가 X의 것이라는 보장이 없다 — 짝이
  보장되는 앵커는 결과와 같은 stdout에 실리는 `running N tests` 줄이다. 소요 시간을 증거로 쓸
  때는 이 앵커로 잡아야 한다.
- **두 겹 방어가 실은 도달 불가능이었다.** 위에서 "대응은 build 단계와 panic 두 겹"이라고 적었는데,
  `spawn_server()`의 체크 순서를 확인하니 `let _ = database_url()?;`가 1행이고 panic하는
  `canonicalize()`가 19행이었다. `DATABASE_URL` 없이 돌리면 `?`가 먼저 `None`을 반환하므로 panic은
  **죽은 코드**가 된다. 실측으로 확정했다 — `target/debug/fleet`을 치우고 `env -u DATABASE_URL cargo
  test -p fleet-mcp --test cross_client`를 돌리면 여전히 `14 passed ... finished in 0.01s`다.
  **방어를 몇 겹 쌓았는지가 아니라 그 앞에 조기 반환이 있는지가 도달 가능성을 정한다.**

  코드가 아니라 게이트를 고쳤다. 조기 반환 자체는 옳다 — DB 없는 환경에서 통합 테스트를 건너뛰는
  것은 이 저장소의 규약이고, 순서를 뒤집으면 DB를 안 붙인 사람에게 바이너리 부재로 panic한다.
  결함은 `agent.md` §3.2 bullet 2가 DB 게이트를 "SQL 쿼리가 수정되었거나 DB 트레이트가 변경되었을
  경우"로 **조건부**로 적어 둔 쪽에 있었다. 이 파일 1474행이 바로 그 조건 때문에 4건을 놓쳤다고
  이미 기록해 뒀는데, 정작 그 조건문은 그대로 남아 있었다. 무조건으로 바꾸고, §4.3의 (3) 사례에
  panic이 `database_url()?` 뒤에 있다는 사실을 덧붙였다. CI는 모든 잡에 postgres 서비스를 붙여
  조건 없이 돌리므로 이 변경은 게이트를 CI 쪽으로 맞추는 것이지 범위를 넓히는 것이 아니다.

## 2026-08-26 — ingest — `#62` 3단계: control epoch fencing (게이트 4의 쓰기 절반)과 범위 축소 기록

- **착수 범위는 `TaskAttempt` 계층 전체였고, 실행 가능한 부분이 하나뿐임을 확인해 축소했다.** 정본
  [실행 일관성](architecture/tasks/execution-consistency.md)의 검증 게이트 11개 중 미구현 7개를
  코드와 대조한 결과, 게이트 5·6·9·10은 **effect ledger 부재**가 아니라 그보다 앞선 이유로 막혀
  있었다 — ledger를 채울 **생산자가 없다**. `fleet-worker`는 `grok agent serve`를 장수명 subprocess로
  띄우므로(`grok_process.rs:145-164`) 개별 tool 호출을 관측할 지점 자체가 없다. `tool_call`/`ToolCall`/
  `side_effect`/`SideEffect`를 저장소 전체에서 grep하면 0건이다. 게이트 7의 `policy_revision`도
  마찬가지로 0건이며, 게이트 8의 `CancelUnconfirmed`는 상태 자체가 없다. 지금 스키마만 만들면
  전부 항상 NULL인 컬럼과 아무도 만들지 않는 variant가 된다.
- **게이트 4를 절반만 닫았다. 나머지 절반은 지금 닫으면 회귀다.** `epoch`는 migration 021의 정의대로
  **최초 획득을 포함해** 획득마다 증가한다. 그래서 "dispatch 당시 epoch보다 현재 epoch가 크면 이벤트를
  버린다"는 규칙을 넣으면 **평범한 control plane 재시작마다 진행 중인 모든 Task의 완료가 버려진다**.
  phase CAS도 대신하지 못한다 — 시도 1의 늦은 `Completed`는 시도 2가 실행 중일 때도 여전히
  `Dispatched` 위상과 일치한다. 늦은 이벤트를 가르는 진짜 식별자는 epoch가 아니라
  `attempt_id`/generation이고, 그래서 정본이 `control_epoch`를 `tasks`가 아니라 **Attempt 행**에
  둔다는 것을 이번에 다시 읽고 확인했다. 정본의 상태 기계에는 애초에 `epoch 불일치 → 거부` 전이가
  없다 — 재시작으로 고아가 된 dispatch는 `Dispatched --> OutcomeUnknown: 전달 후 응답 유실`로 간다.
  정본의 CAS 조항이 epoch를 여섯 개 **쓰기 조건** 중 하나로 열거하는 것도 같은 이야기다.
- **구현한 쓰기 절반**: `ControlFence{cluster_id, epoch}`(fleet-store), `TransitionOutcome::Fenced`
  (fleet-core), `compare_and_set_task_status(.., fence: Option<&ControlFence>)`. PgStore는
  `EXISTS (SELECT 1 FROM control_plane_lease WHERE cluster_id = $4 AND epoch = $5)`를 **기존 UPDATE와
  같은 문장 안에** 넣는다 — 별도 SELECT로 먼저 확인하면 확인과 쓰기 사이가 그대로 TOCTOU 창이 되어
  1단계가 없앤 것을 되살린다. 0행일 때만 lease를 재조회해 `Fenced`/`Rejected`/`NotFound`를 가르며,
  그 재조회는 위상 재조회보다 **먼저** 한다(fenced 인스턴스가 읽은 Task 상태에는 권위가 없다).
- **`Fenced`를 `Rejected`와 나눈 이유는 후속 동작이 다르기 때문이다.** `Rejected`는 다른 writer가 이
  Task 하나를 먼저 옮긴 것이라 그 Task만 포기하면 되지만, `Fenced`는 이 인스턴스가 더 이상 제어
  기관이 아니라는 뜻이라 **이후의 모든 쓰기도 같은 이유로 실패한다**. 두 값을 합치면 호출자가
  "이 Task를 포기"와 "쓰기를 그만두어야 함"을 구분할 수 없다.
- **`#63` 2단계의 bool 게이트와 겹치지 않는다.** 그쪽(`lease_allows_control()`)은 이미 `Fenced`를
  **관측한 뒤**를 막는다. 이번 것은 관측이 아직 `Active`인데 저장소만 앞서 간 창 — 갱신 주기(5초)
  안에 장애 전환이 일어나면 실제로 도달하는 상태 — 을 막는다. 디스패처 테스트를 이 창 위에 세운
  이유가 그것이다: 기존 `setup_fenced` 계열은 bool 하나에서 막혀 CAS에 **도달조차 하지 않는다**.
- **뮤테이션으로 값을 증명했다.** `dispatcher.rs`의 `cancel` 호출 지점에서 `fence` 인자를 `None`으로
  바꾸면 새 테스트 1건만 정확히 실패한다. 통과 자체는 "무엇을 검증했는가"를 말해 주지 않는다 —
  §4.3(3)의 "개수가 아니라 소요 시간이 조용한 skip을 드러낸다"와 같은 계열의 확인이다.
- **`fleet tasks cancel`은 의도적으로 fence를 걸지 않는다.** `fleet serve`가 죽었거나 fenced된
  상황에서도 동작해야 하는 operator 도구라 lease를 획득하지 않으며 획득해서도 안 된다. 대가는
  실재한다 — 이 경로만 epoch 술어를 우회한다. 정본과 로드맵 양쪽에 남겼다.
- **게이트 실행 중에 내 grep이 실패를 삼켰다.** `fleet-store`의 DB 재실행을
  `cargo test -p fleet-store -- --test-threads=1`로 돌리고 결과를 `grep -E "^running|^test result"`로
  거른 탓에, 이 크레이트가 `--all-features`(=`test-support`) 없이는 `fleet_store::mem`을 못 찾아
  **컴파일 자체가 실패한 것**이 화면에서 사라졌다. 출력이 비어 있는 것을 "통과"로 읽을 뻔했다.
  §4.3의 세 사례와 같은 종류다 — 이번엔 게이트 목록이 아니라 **게이트를 읽는 필터**가 CI보다
  약했다. 뒤이어 `error|FAILED|panicked` 계수를 함께 찍는 방식으로 바꿔 확인했고, 그 계수도
  `worker_delete_nonexistent_errors`라는 테스트 **이름**에 걸리는 오탐을 냈다. 요약 필터는 그
  자체가 증거가 아니며, 0이 아닌 값이 나오면 원문을 봐야 한다.
- **검증**: `rustc 1.98.0`(rust-toolchain.toml 고정값과 일치), `RUSTFLAGS="-D warnings"`,
  `cargo fmt --all -- --check` 통과, `cargo clippy --workspace --features "acp mtls" --all-targets
  -- -D warnings` 및 `--no-default-features` 양쪽 경고 0. `cargo build -p fleet-cli --features
  "acp mtls"` 후 `cargo test --workspace --features "acp mtls"` **68 스위트 전부 `ok`, 실패 0**.
  `DATABASE_URL` 주입 직렬 재실행: `fleet-store --all-features`(16 바이너리), `fleet-api`(16),
  `fleet-scheduler`(4), `fleet-mcp`(3 — `cross_client` **14 passed / 4.86s**로 조용한 skip이 아님을
  소요 시간으로 확인), `fleet-dashboard`(4) 전부 통과. 신규 테스트는 `task_cas.rs` 4건(MemStore·실제
  PostgreSQL 양쪽)과 `dispatcher.rs` 1건.
- **검증 한계**: 실제 두 프로세스의 라이브 장애 전환 중 쓰기 거절은 **관찰하지 않았다**. 각 계층을
  개별적으로만 확인했다(저장소 CAS는 실제 Postgres, 배선은 수동 구성한 stale observer).
  READ COMMITTED 잔여 창 — 단일 `UPDATE`가 스냅샷을 한 번 잡으므로 문장 실행 **중**에 커밋되는
  epoch N+1은 보이지 않는다 — 은 `FOR SHARE` 없이 남겨 두고 정본에 기록했다. 닫으려면 모든 Task
  상태 쓰기가 5초 주기 lease 갱신 `UPDATE`와 같은 한 행에서 락 경쟁을 하게 된다. 마이크로초 규모이며
  술어가 없던 이전의 무제한 창에 비하면 유계다.
- **문서**: 정본에 [구현 상태와 유예](architecture/tasks/execution-consistency.md) 절을 신설해 게이트
  11개의 상태와 막고 있는 것을 표로 남기고, `implementation`을 `proposed → partial`로 고쳤다.
  로드맵은 `#62` 행에 3단계를 추가하고, **`#63` 행도 함께 고쳤다** — 그 행의 "아직 안 한 것"이 바로
  이번에 절반 닫은 게이트를 지목하고 있어서, 한쪽만 고치면 로드맵이 자기모순이 된다.

## 2026-08-26 — ingest — ACP 인증을 URL query 밖으로 이전 (`#94`)

- **로드맵이 걸어 둔 게이트가 틀렸다는 것이 첫 발견이다.** `#94`는 "mTLS client cert만으로
  충분한지 확인"을 착수 조건으로 걸고 있었는데, 그 조건은 **구조적으로 성립할 수 없다** —
  `fleet-transport`의 `MtlsProxy`가 TLS를 종단하고 grok에는 평문 TCP를 넘기므로
  (`copy_bidirectional`) grok은 client certificate를 애초에 볼 수 없다. 실측으로도 로컬의 grok
  세 버전(0.2.102 / 0.2.112 / 1.0.5)이 전부 인증 없는 `/ws`를 거절했다. 그래서 이 항목이 할 수
  있는 것은 secret의 **이전**이지 **제거**가 아니다. 로드맵 행과 [Enrollment 정본](contracts/worker-enrollment.md)
  양쪽의 문구를 고쳤다 — 원문대로 두면 다음 사람이 "grok을 고치면 되겠네"가 아니라 "우리가
  아직 안 했네"로 읽는다.
- **저장소를 한 줄도 바꾸지 않은 것이 이번 설계의 핵심이다.** secret은 이미 `Worker.endpoint`
  문자열 안에 있고 `#75`의 `mask_server_key`가 네 개 읽기 경로를 지키고 있다. 별도 컬럼을
  만들었다면 그 네 경로를 전부 다시 열고 새 마스킹 의무를 만드는 것 — 유출면을 줄이는 게 아니라
  **옮기는** 것이다. 대신 신설 `fleet_core::split_server_key`가 **다이얼 직전 한 곳**에서만
  문자열을 쪼갠다. 생산자(`agent_endpoint` / `mtls_agent_endpoint`)를 건드리지 않았으므로 변경
  전후에 등록된 워커가 동일하게 동작하고 혼재 상태 마이그레이션이 없다.
- **`split_server_key`는 `mask_server_key`와 반대 방향으로 보수적이다.** 마커 앞이 `?`/`&`가
  아니면 손대지 않는다. 마스킹은 과하게 가려도 안전하지만, 잘못 쪼개면 다이얼 자체가 깨진다 —
  같은 문자열을 보는 두 함수라도 실패의 대가가 다르면 판정 기준도 달라야 한다.
- **양성 테스트 하나만으로는 아무것도 증명되지 않는다.** `e2e_94_secret_travels_in_header_not_url`은
  같은 프로세스에서 **음성 대조를 먼저** 돌린다 — 인증 없는 `/ws`가 거절되는 것을 확인하지
  않으면, 헤더를 붙인 연결이 성공한 것이 "헤더가 인증됐다"인지 "grok이 인증을 강제하지 않았다"인지
  구분할 수 없다. mTLS 홉도 마찬가지로 상류에서 헤더를 **캡처해서** 확인했다("당연해 보인다"와
  "측정했다"는 다르다).
- **워커별 인증 모드 자동 협상은 만들지 않았다.** 헤더를 거절하는 grok이 하나도 관측되지 않았다.
  가정 위에 협상 기계를 지으면 트리거되지 않는 분기가 영구히 남는다. 프로세스 단위 스위치
  `FLEET_ACP_AUTH=query` 하나만 escape hatch로 남겼고, `ws_auth_parts(endpoint, mode)`를 순수
  함수로 분리해 env 읽기를 `AcpAuthMode::from_env()` 한 곳에 가뒀다(프로세스 전역 상태가 병렬
  테스트를 오염시키는 것을 피한다).
- **게이트를 읽는 방식도 한 번 더 고쳤다.** 오늘 백그라운드 게이트가 `exit code 0`을 보고했지만
  실제로는 중간의 acp+mtls clippy가 실패해 있었다 — `&&` 체인은 **마지막** 명령의 코드만 남긴다.
  이후로는 단계마다 `RC_<단계>=$?`를 개별로 찍어 판정한다. 2026-08-25의 두 사례, 08-26의
  `cross_client` 사례, 그리고 어제의 "grep이 컴파일 실패를 삼킴"과 같은 계열이다 — **요약값은
  증거가 아니다.**
- **검증**: `rustc 1.98.0`(rust-toolchain.toml 고정값과 일치), `RUSTFLAGS="-D warnings"`,
  `cargo fmt --all -- --check`(`RC=0`), `cargo clippy --workspace --features "acp mtls"
  --all-targets -- -D warnings`(`RC=0`), 같은 명령의 `--no-default-features` 판(`RC=0`) —
  셋 다 개별 종료 코드로 확인. `cargo build -p fleet-cli --features "acp mtls"` 후
  `cargo test --workspace` **68 스위트 전부 `ok`, 1042건 통과, 실패 0**(ignored 5 — `#[ignore]`가
  붙은 실 grok e2e 3건 포함). `DATABASE_URL` 주입 직렬 재실행(`--test-threads=1`):
  `fleet-store --all-features` 16 바이너리, `fleet-api` 16, `fleet-scheduler` 4, `fleet-mcp` 3,
  `fleet-dashboard` 4 — 전부 통과. `fleet-mcp`의 `cross_client`는 **14 passed / 4.99s**로,
  `target/debug/fleet`를 실제로 띄웠음을 소요 시간으로 확인했다(부재 시엔 `0.00s`에 같은 개수를
  보고한다). 실 grok e2e는 `GROK_BIN`을 주입해 0.2.102 / 0.2.112 / 1.0.5 세 버전에 각각 수동
  실행했다. 신규 테스트 15건.
- **검증 한계**: 비mTLS 워커의 `/ws/{name}` nginx 홉이 `Authorization`을 상류로 전달하는지는
  **확인하지 못했다** — 그 설정은 운영자 소유이고 이 저장소에 없어 재현할 대상 자체가 없다
  (`proxy_set_header`로 지운 배포는 조용히 401을 받는다). grok 0.2.102 미만은 로컬에 바이너리가
  없어 미테스트. 실제 프로덕션 워커가 아니라 로컬 grok 프로세스에 다이얼했다. 미룬 것 전체는
  Enrollment 정본의 표에 있다.

## 2026-08-26 — ingest — 기동 호환성 게이트: 살아 있는 lease 아래에서의 마이그레이션 거부 (`#63` 4단계)

- **정본이 근거로 적어 둔 문장이 절반은 틀렸다는 것이 첫 발견이다.** 게이트 5의 근거는 "기동 시
  호환성 검사 자체가 없다"였는데, sqlx 0.8.6은 `ignore_missing` 기본값이 `false`이고 이 저장소가
  `set_ignore_missing`을 부르지 않으므로 **DB에 적용됐는데 바이너리에 없는** 마이그레이션은 이미
  `VersionMissing`으로 거절된다. 즉 "DB보다 낡은 바이너리"는 예전부터 기동하지 못했다. 없던 것은
  메커니즘이 아니라 그 보장을 고정하는 **테스트**였고, 그래서 이번 작업은 절반이 회귀 테스트고
  절반이 신규 구현이다. **"미착수"라고 적힌 게이트를 읽을 때 코드를 먼저 보지 않으면, 이미 있는
  것을 다시 만들거나 없는 것을 있다고 믿는다.**
- **진짜 구멍은 반대 방향이었다.** 바이너리에만 있는 마이그레이션은 `Migrator::run_direct`의
  `None => conn.apply(...)` 가지에서 **말없이 적용된다**. Cold Standby는 Primary와 DB 하나를
  공유하므로, 더 새 바이너리를 든 Standby는 **기동하는 것만으로** 살아 있는 Primary 밑에서 스키마를
  바꾼다. sqlx의 advisory lock은 이것을 덮지 못한다 — 그 락은 **경쟁하는 마이그레이터끼리** 직렬화할
  뿐이고, 여기서 위험한 쪽은 마이그레이션하지 않고 **이미 돌고 있는** 낡은 프로세스다. 방어의
  존재가 아니라 **무엇을 상대로 한 방어인지**가 적용 여부를 정한다.
- **술어를 역할이 아니라 상태로 세운 것이 설계의 전환점이다.** 마이그레이션 시점에는 자기가
  Primary인지 Standby인지 알 수 없다 — `store.migrate()`는 `runtime.rs:180`이고 `LeaseManager::new`는
  그보다 한참 뒤다. 역할로 조건을 걸려 하면 막힌다. 대신 **적용할 마이그레이션이 있고 동시에
  만료되지 않은 lease가 있을 때만** 거절하도록 `guard_migration_against_live_lease`를 세웠다.
  두 조건을 **모두** 요구하는 것이 핵심이다 — 적용할 게 없어도 막으면 평범한 롤링 재기동이 통째로
  막히는 운영 함정이 되고, 마이그레이션만 보고 막으면 정당한 오프라인 업그레이드가 막힌다.
- **`release_control_lease`가 행을 지우지 않고 `expires_at = NOW()`로 만든다는 사실이 이 게이트를
  운영 가능하게 만든다.** 덕분에 "살아 있는 lease"는 `acquire_control_lease`가 쓰는 것과 **같은 단일
  술어**(`expires_at > NOW()`)이고, 정상 종료한 Primary는 TTL을 기다리지 않고 즉시 길을 비킨다.
  사람을 막는 창은 **크래시 뒤 최대 TTL 15초**뿐이며, 에러 메시지가 남은 초·멈춰야 할 인스턴스·
  클러스터 이름·조치 방법을 모두 적는다.
- **게이트를 `Store::migrate` 구현 안에 둔 것이 호출 지점 누락을 구조적으로 없앤다.**
  `fleet serve`·`fleet migrate`(둘 다 `connect_and_migrate` 경유)·`fleet users`·`fleet doctor`
  네 곳이 전부 같은 함수를 지난다. 호출자마다 가드를 부르게 했다면 다섯 번째 호출자가 생기는 날
  조용히 새는 경로가 하나 생긴다.
- **binary 버전 검사는 의도적으로 넣지 않았다.** 바이너리 버전을 DB에 쓰는 **생산자가 저장소에
  하나도 없고** `control_plane_lease`에 버전 컬럼도 없다. 지금 술어를 넣으면 항상 참인 죽은 검사가
  된다 — "채울 방법이 없는 것은 미리 만들지 않는다"는 원칙 그대로다. 그래서 게이트 5는 **닫힘이
  아니라 부분**이고, 미룬 이유를 정본의 방향별 4행 표에 남겼다.
- **테스트가 진짜 상태를 만든다.** `Migrator`의 필드가 공개돼 있다는 점(sqlx가 `migrate!()`를 위해
  열어 둔 것)을 이용해 **버전을 잘라낸 사본**을 조립하면, 체크섬을 손으로 위조하지 않고 진짜
  `_sqlx_migrations` 원장과 함께 "DB는 N-1까지, 바이너리는 N까지"인 임시 DB를 만들 수 있다.
  원장 행을 직접 INSERT하는 방식보다 훨씬 덜 부서진다.
- **뮤테이션으로 테스트의 값을 증명했다.** `migrate()`에서 가드 호출 **한 줄**을 지우면 4건 중
  정확히 2건이 실패하고, 나머지 2건은 통과한 채 남는다. 이 2:2 분할이 결함이 아니라 **증거**다 —
  남는 2건은 sqlx의 기존 보장을 고정하는 것과 과잉 발동이 없음을 확인하는 것이라, 가드를 지워도
  통과하는 것이 옳다. 전부 실패했다면 오히려 테스트들이 같은 것을 중복해 보고 있다는 뜻이다.
- **라이브 검증**: 임시 DB를 마지막 마이그레이션 직전(버전 24)까지 올리고 025의 인덱스가 없음을
  확인한 뒤 `expires_at = NOW() + 120s`인 lease 행을 넣었다. 실제 바이너리의 `fleet migrate`와
  `fleet serve`가 **둘 다 exit code 1로 거절**됐고(후자는 MCP 서버를 열기 **전에** 멈췄다),
  `_sqlx_migrations` 최대 버전은 24로 불변, 인덱스도 계속 부재였다. lease를 `expires_at = NOW()`로
  (= `release_control_lease`와 정확히 같은 형태) 반납한 직후 `fleet migrate`가 **즉시** 성공해
  버전 25와 인덱스가 생겼다 — TTL을 기다리지 않는다는 것을 실측으로 확인한 것이다. 정리: 임시 DB
  `dropdb`, 뮤테이션 실행에서 샌 임시 DB 2개도 함께 제거해 잔여 0 확인.
- **함께 정정한 것**: 정본 게이트 표의 **게이트 3 행도 낡아 있었다**. `516f85e`(`#62` 3단계)로
  쓰기 절반이 닫혔는데 "미착수"로 남아 있었고, 근거로 든 `crates/fleet-api/src/state.rs`는
  **존재하지 않는 파일**이다. 게이트 3을 부분으로 고치고 epoch 절 본문도 갱신했다. 어제 `#62`
  작업에서 "`#63` 행을 함께 고치지 않으면 로드맵이 자기모순"이라고 적었던 것의 반대 방향 사례다 —
  **한쪽만 고친 정정은 다음 사람에게 잘못된 근거를 그대로 물려준다.**
- **검증 한계**: 검사와 실제 적용 사이는 **원자적이지 않다** — 검사 직후 다른 인스턴스가 lease를
  얻으면 스키마는 여전히 바뀔 수 있다. 이 게이트는 배포 실수를 막는 것이지 분산 합의가 아니며,
  원자적으로 만들려면 마이그레이션을 lease 아래에 넣어야 하는데 `control_plane_lease`를 **만드는
  것이** 021 마이그레이션이라 부트스트랩 순환이 생긴다. 라이브 검증은 lease 행을 직접 INSERT해
  "살아 있는 Primary"를 흉내 냈을 뿐 두 번째 `fleet` 프로세스를 실제로 띄우지는 않았고, 네 호출
  지점 중 실제 바이너리로 지난 것은 2개이며 `fleet users`/`fleet doctor`는 같은 함수를 부른다는
  사실로만 덮여 있다. Primary가 크래시하고 TTL이 지난 뒤에는 이 게이트도 막지 못한다 — DB만 보고
  살아 있는 Primary와 죽은 Primary를 구분할 수 없다는 **불변식 3의 한계와 같은 뿌리**다.
  구현 게이트 4(Primary 종료 → 수동 승격 → Worker 재연결 → pending reconciliation)는 여전히
  미착수이며, `LeaseManager::spawn()`이 3초 주기로 자동 재획득하므로 계약이 말하는 "수동 승격"에
  대응하는 구현이 아직 없다는 문제도 그대로 남아 있다.
- **게이트 관측의 한계**: 새 테스트 파일도 이 저장소의 DB 테스트 14개가 모두 쓰는 관례를 그대로
  따라 `DATABASE_URL`이 없으면 조용히 skip한다. 그래서 `cargo test --workspace`(DB 미주입)는
  `migration_lease_guard`에 대해 `4 passed ... finished in 0.00s`를 보고하며, **이 게이트에서
  네 건은 실제로 돌지 않았다**. 진짜 실행은 `DATABASE_URL`을 주입한 직렬 재실행(0.67s)뿐이다.
  관례를 깨지 않는 쪽을 택했지만 — 파일 하나만 panic하면 나머지 13개와 어긋난다 — 그 대가로
  워크스페이스 통과 개수는 이 네 건에 대해 아무것도 증명하지 않는다. 뼈아픈 것은 같은 커밋에서
  `cross_client`에는 소요 시간 판별을 적용해 놓고 **방금 내가 쓴 파일에는 적용하지 않았다는**
  점이다. §4.3의 (3)이 남긴 교훈은 도구가 아니라 겨냥이었다 — **판별법은 들이대는 곳에서만
  작동하고, 새로 쓴 코드는 의심 목록의 맨 뒤로 밀린다.**

---

## 2026-08-26 — ingest — 무재시도 정책 채택과 `TaskAttempt`의 철회 (`#62` 4단계)

- **결정**: 실행 실패를 재시도하지 않는다. 실패한 Task는 터미널로 남고, 다시 하려면 **새 Task**를
  만든다. 사용자 결정이며, 논의와 대안 비교는
  [재시도 정책 결정 기록](./reviews/task-retry-policy-decision-2026-08-26.md)에 있다.
- **그 결정이 무너뜨린 것**: 4단계의 원래 범위는 `TaskAttempt` 엔티티였다. 무재시도 아래에서
  Task와 시도는 **1:0..1**이 된다 — 실측 근거는 `TaskStatus::Pending`을 **새 상태로 쓰는 코드가
  저장소에 하나도 없다**는 것이다(`expected` 슬라이스 밖의 모든 출현은 테스트 `matches!` 단언
  이거나 읽기 측 표시·메트릭 match arm). Task는 `Pending`으로 태어나 다시 돌아오지 않으므로
  `Pending→Dispatched` CAS는 Task당 영구히 최대 한 번 성공한다. 그러면 attempt 행의 `worker_id`·
  `created_at`·`generation`은 각각 `TaskStatus::Dispatched{worker_id}`·`tasks.dispatched_at`·상수 1의
  중복이고 `AttemptId`는 소비자가 없다. **새로운 사실은 `control_epoch` 하나뿐**이었다.
- **그래서 749 insertions를 되돌리고 컬럼 하나로 남겼다.** 마이그레이션 026이
  `tasks.dispatch_control_epoch BIGINT CHECK (>= 0)`을 더하고, 3단계가 만든 CAS **안에서**
  `is_dispatching && fence.is_some()`일 때만 쓴다. `control_plane_lease`가 **현재** epoch만 들고 있어
  lease가 넘어가면 "어느 제어 세대가 이 Task를 디스패치했는가"가 사후 복원 불가능해지기 때문에
  이 하나는 기록할 값이 있다. 나머지 셋은 오늘 `tasks`에서 읽힌다.
- **두 조건의 교집합인 이유**: Postgres는 자리번호 개수를 문장 텍스트의 최댓값으로 추론하므로
  `$5`가 fence 없이 텍스트에만 남으면 문장 자체가 거절된다. 그리고 `is_dispatching`을 떼면 완료·
  실패 전이가 값을 덮어써 "디스패치한 세대"가 "마지막으로 손댄 세대"로 바뀐다. 두 이유가 **서로
  다른 실패 모드**라 조건도 두 개다.
- **검증**: `task_cas.rs` 3건을 MemStore·실제 PostgreSQL 양쪽에서. 셋째(E1 디스패치 → lease 해제 →
  `instance-b`가 E2로 재획득 → E2에서 완료 → 값이 여전히 `Some(E1)`)만이 `is_dispatching` 가드를
  지키며, 뮤테이션으로 그 사실을 확인했다.
- **부수 수정**: `migration_lease_guard.rs`가 관측 지점을 마이그레이션 025의 인덱스 이름으로
  하드코딩해 두어 **어떤** 새 마이그레이션이든 이 테스트를 깨뜨렸다. `_sqlx_migrations` 원장
  조회로 일반화했다 — sqlx `Migrator`는 각 마이그레이션의 DDL과 원장 삽입을 **한 트랜잭션**에
  넣으므로 원장 부재는 스키마 객체 부재와 동등한 증거이며, 이 관측은 마이그레이션에 무관하다.
- **정본 반영**: [실행 일관성](./architecture/tasks/execution-consistency.md)의 ER·상태 모델에서
  `TASK_ATTEMPT`와 `RetryWaiting`을 제거하고, `## 재시도 계약`을 `## 실패 처리 계약`으로 교체했다.
  CAS 술어 목록에서 `attempt_id`/generation을 뺐고, `### control epoch 게이트를 절반만 닫은 이유`의
  둘째 문단(=늦은 이벤트를 가르는 진짜 식별자는 `attempt_id`라는 논증)은 **무효**가 되어 다시
  썼다 — 시도 2가 없으므로 phase CAS가 그 경쟁을 실제로 가른다.
- **정책이 만든 새 구멍(미해결, 정직하게 기록)**: 외부 멱등성 키는 **Project ID + Task ID +
  effect scope + 제출 시 정책 revision**의 HMAC이다. Task ID가 안정된 앵커였던 이유는 재시도가 한
  Task **안에** 머물렀기 때문인데, 실패를 새 Task로 옮기면 ID가 바뀌어 키가 달라지고 외부
  provider는 새 작업으로 본다 — 그 파생식이 막으려던 이중 적용이 되살아난다. 새 Task 경계를 넘는
  앵커 필드가 필요하지만 **없다.** "채울 방법이 없는 것은 미리 만들지 않는다"에 따라 필드를 만들지
  않고 정본 유예 표에 **설계 미결**로 올렸다. 이것은 미구현 게이트가 아니라 **정책과 함께 받아들인
  대가**다.
- **`#95`의 대기 사유가 유령이 됐다**: [Authorization](./security/authorization-and-audit.md)
  202-204는 `AuditEvent`에 `attempt_id`가 없는 이유를 "Project/Attempt 엔티티가 아직 없어
  (`#48`·`#62` 계열 선행)"로 적는다. 4단계가 "생산자 없음"으로 닫힌 지금 그 선행은 `#62`로는
  **영원히 충족되지 않는다.** 로드맵 `#95` 행에 이 사실과 삭제 지시를 적었고, security 정본의
  같은 문장 수정은 그 도메인의 몫으로 남겼다.
- **검증 한계**: 이 컬럼을 **읽는 코드는 없다**. HA lease가 없는 단일 인스턴스 배포에서는
  `SchedulerState::control_fence()`가 `None`이라 **항상 NULL**이며, 이는 "값을 못 구했다"가 아니라
  "제어 세대 개념이 없는 배포"라는 뜻이다. 026 이전 행은 소급 불가다. 라이브 2프로세스 장애
  전환에서의 쓰기 거절은 이번에도 관찰하지 않았다.
- **고치지 않은 용어 드리프트**: [`security/authorization-and-audit.md`](./security/authorization-and-audit.md)
  37·42·111·202-204와 [`security/control-plane-security-model.md`](./security/control-plane-security-model.md)
  72·91·97·106-112는 아직 `Attempt`를 별도 엔티티로 전제한다. 1:0..1이므로 실체는 Task이지만
  **단순 개명이 아니다** — 전자는 `#95`의 대기 사유(로드맵 상태 주장)이고 후자는 grant 수명
  의미론(보안 계약 주장)이라, 각 도메인에서 등가성을 확인한 뒤 고친다. 위치와 사유를 정본의
  "다른 문서에 남은 `Attempt` 표현" 절 표에 남겼다.

---

## 2026-08-26 — lint — 로컬 테스트 게이트가 CI와 다르다: `--test-threads=1` 누락

`#62` 4단계 커밋 전 게이트를 돌리다 `fleet-store`의 `audit_integration` 7건 중 6건이 실패했다.
원인을 규명한 결과 **코드 결함도, flaky도 아니었다 — 로컬 게이트 명령이 CI와 달랐다.**

- **증상**: `DATABASE_URL`을 주입한 `cargo test -p fleet-store --test audit_integration`이
  6건 실패. DB를 새로 만들어도 동일하게 재현됐다(누적 오염이 아니다).
- **규명**: `-- --test-threads=1`을 붙이면 **7건 전부 통과**한다. 내 변경을 `git stash`로
  제외한 HEAD에서도 병렬 실행은 똑같이 6건 실패한다 — 즉 **이번 변경과 무관한 선재 조건**이다.
- **기전**: 이 파일의 `require_db!` 매크로는 매 테스트 시작에 `TRUNCATE audit_log, sessions,
  user_roles, users CASCADE`를 실행하고, 여러 테스트가 **필터 없는 전역 목록**을 단언한다.
  병렬로 돌면 한 테스트의 TRUNCATE가 다른 테스트의 행을 지우고, 한 테스트의 삽입이 다른
  테스트의 단언을 오염시킨다. 파일 상단 doc comment는 이미 `-- --test-threads=1`을 적어
  두었으나 **강제하지는 않는다.**
- **CI가 초록인 이유**: `.github/workflows/ci.yml`의 두 test 잡은 모두
  `cargo test --workspace -- --test-threads=1`로 **직렬 실행**한다. 그래서 CI는 이 조건을
  애초에 만나지 않는다.
- **드리프트의 방향이 평소와 반대다.** §4.3이 기록한 세 사례는 전부 "로컬 게이트가 CI보다
  **약해서** CI만 실패"였다. 이번은 "로컬이 CI와 **달라서** 로컬만 실패"다. 위험은 대칭이다 —
  같은 불일치가 반대로 작동하면 **병렬에서만 통과하는 테스트**가 로컬 초록을 받고 CI에서
  깨진다. 게이트는 CI보다 강하거나 약한 것이 아니라 **같아야** 한다.
- **조치**: 로컬 테스트 게이트를 CI와 같은 형태로 적는다.
  ```bash
  cargo build -p fleet-cli --features "acp mtls"   # 잡의 피처로 먼저 (§3.2)
  cargo test --workspace -- --test-threads=1
  ```
  `audit_integration`의 테스트 격리 자체(전역 목록 단언을 테스트별 고유 actor/action으로
  좁히기)는 이 커밋의 범위가 아니라 별도 항목으로 남긴다. 직렬 실행으로 CI와 로컬이 모두
  성립하므로 지금 당장 깨지는 것은 없고, 고치면 워크스페이스 테스트를 병렬화할 수 있다는
  것이 이득이다.

## 2026-08-26 — docs — 드리프트 표가 2개라고 적었으나 실측은 30개였다 (`#62` 4단계 후속)

`#62` 4단계에서 정본에 넣은 "다른 문서에 남은 `Attempt` 표현" 표는 security 문서 **2개**만
열거했다. 전수 측정으로 실제 발자국은 **30개 파일 161행**이었다(architecture 20개 120행,
reviews 5개 22행, security 2개 13행, 기타 3개 6행).

```
grep -rn --include='*.md' 'TaskAttempt\|attempt_id\|RetryWaiting\|AttemptId\|Attempt' docs
```

**2개짜리 표는 표가 없는 것보다 나빴다.** 없으면 "조사하지 않았다"로 읽히지만, 있으면 전수
목록으로 읽히면서 28개를 감춘다. 문서가 스스로 검증됐다고 **주장하는** 상태가 검증되지 않은
상태보다 위험한 전형적 사례다 — `agent.md` §4.3이 게이트 목록에 대해 기록한 것과 같은 형태의
결함이며, 대상이 명령 목록이 아니라 인벤토리 표라는 점만 다르다.

원인은 sweep 결과를 끝까지 읽지 않고 미리보기 앞부분(security 2건)만으로 표를 만든 것이다.
41KB 출력 중 2KB만 본 상태에서 "전수"를 주장했다.

대응:

- 정본의 표를 **도메인별 실측 집계**로 교체하고, 측정 명령·측정일·제외 파일을 표 위에 적었다.
  대표 파일만 예시로 들고 나머지는 개수로 증언한다.
- 미수정 이유를 세 가지로 명시했다. ① `#62` 4단계가 확인한 1:0..1은 **현재 코드**의 성질이고,
  다른 문서의 `TaskAttempt`는 `#48` 계열이 소유한 **목표 엔티티**다. ② 개명이 아니라 계약 변경이다
  — `control-plane-security-model.md`의 "Attempt 단위 grant 수명"을 Task 단위로 바꾸면 credential이
  살아 있는 시간 구간이 달라진다. ③ reviews는 결정 당시 근거이므로 소급 수정하지 않는다.
- 교차 도메인 판정을 `#97`로 신설했다. **개명 작업이 아니라 판정 작업**임을 항목에 못박았다 —
  먼저 "재시도가 없는데도 Attempt를 별도 엔티티로 둘 이유가 남아 있는가"를 정하고, 없으면 흡수,
  있으면 생산자를 명시한다. reviews 5개 파일은 범위에서 뺐다.

같이 고친 두 곳:

- `#95` 행이 자기모순이었다. 산출물 문장은 `AuditEvent`에 `attempt_id`를 **추가한다**고 적고,
  뒤 문장은 **삭제해야 한다**고 적었다. 필드 목록에서 빼고 뒤 문장을 "뺀 이유"로 바꿨다.
- [`docs/architecture/tasks/README.md`](architecture/tasks/README.md)의 읽기 순서 표가 실행 일관성
  문서를 "Attempt, 재시도, 멱등성, 부작용"으로 소개하고 있었다. 그 문서에는 이제 재시도 계약이
  없다 — "상태 전이 CAS, 실패 처리, 멱등성, 부작용"으로 고쳤다.

숫자를 확정하기 전에 한 번 더 틀렸다. 첫 측정은 32개 파일 171행이었는데, 그 안에
`docs/roadmap/roadmap.md`가 들어 있었다. 그런데 **이번 수정 자체가 그 파일의 카운트를 바꾼다** —
`#97` 행을 추가하면 `TaskAttempt`가 적힌 줄이 늘고, `tasks/README.md`에서 그 단어를 지우면 그
파일은 집계에서 통째로 빠진다. 그래서 최종 집계에서 roadmap.md를 **사유를 적어 제외**했다. 그
파일의 언급은 남아 있는 드리프트가 아니라 결정과 후속 항목을 서술하는 원장이고, 포함하면 앞으로
로드맵을 편집할 때마다 이 숫자가 저절로 썩는다. **고치는 중에 대상이 변하는 인벤토리는 경계를
먼저 고정해야 한다** — 그러지 않으면 방금 고친 결함을 형태만 바꿔 재생산한다.

검증 한계: 앵커 유효성은 전 문서 스크립트로 확인했다(dead file 2건은 산문 속 `...`과 템플릿
경로로 기존부터 있던 것, dead anchor 0). 30개 파일 각각이 **어떤 종류의** 계약을 주장하는지는
대표 4개 파일만 실제로 읽었고 나머지는 토큰 분포로만 분류했다 — `#97`이 그것을 파일 단위로
확인해야 한다.

## 2026-08-26 — test — `cross_client` flake의 원인은 116MB 바이너리의 첫 exec 비용이었다

`cargo build -p fleet-cli` 직후의 `cargo test --workspace`에서 `fleet-mcp`의 `cross_client` 14건 중
1건이 `RESPONSE_TIMEOUT`(15초)을 넘겨 실패했다(`13 passed; 1 failed ... finished in 17.08s`). 단독
재실행은 14건 0.66초로 통과했다.

세 가설을 전부 측정했다. 콜드 DB의 26개 마이그레이션은 첫 응답 0.19초(웜 0.02초)로 **크기가 맞지
않았다**. 커넥션 고갈도 아니었다 — 서버 20개를 동시에 띄워(잠재 200 연결, 서버측
`max_connections=100`) 전부 ~0.10초였고, 풀이 lazy라 `pg_stat_activity` 실측 연결은 0이었다. 남은
것은 macOS의 첫 exec 비용이며 이것만 자릿수가 맞았다: 갓 만든 116MB 디버그 바이너리의 첫 `serve`
응답이 1.35~2.34초, 두 번째가 0.03~0.04초로 **35~78배** 차이였다.

이 비용은 프로세스의 **첫 spawn 하나만** 지불한다. 14건 중 정확히 1건, 그것도 알파벳 순 첫 테스트가
실패한 관측이 이 설명과 맞는다. 대응은 `spawn_server()`에 `std::sync::Once` 워밍업(`fleet --version`)
을 넣어 그 비용을 타이머 **밖**에서 치르는 것이다. A/B 실측으로 효과를 확인했다 — 워밍업 없음 1.35초,
`--version` 워밍업(1.08초) 후 0.04초.

**워밍업의 위치가 안전성을 정한다.** `database_url()?`와 canonicalize panic **뒤**에 둔다. 앞에 두면
DB 없는 실행이 이유 없이 1초를 물고, 존재를 증명하지 않은 경로를 exec해 명확한 panic 메시지가 흐린
exec 실패로 바뀐다 — `agent.md` §4.3 (3)이 기록한 "방어의 강도가 아니라 순서가 도달 가능성을 정한다"와
같은 함정이다. `--version`은 DB에 접속하지 않으므로(웜 0.015초) 이 워밍업은 Postgres 상태에
의존하지 않는다.

CI 워크플로는 바꾸지 않았다. 두 test 잡 모두 `cargo build -p fleet-cli` 직후 `cargo test`를 돌려
**같은 노출을 갖지만**, 수정이 테스트 파일 안에 있어 양쪽에 그대로 적용된다.

진단 자체는 `agent.md` §3.3에 정본으로 기록했다(규약이 요구하는 곳은 작업 로그가 아니라
가이드라인이다).

검증: `cargo build -p fleet-cli --features "acp mtls"` 후 `DATABASE_URL` 주입 `cargo test --workspace
--features "acp mtls" -- --test-threads=1` → 69개 스위트 1053건 통과, 0 실패. `cross_client`는 14건
6.48초로 조용한 skip이 아님을 소요 시간으로 확인했다. §4.3 게이트 4종(rustc 1.98.0, `RUSTFLAGS=-D
warnings`, fmt, clippy 2종) 전부 통과.

**검증 한계**: 유휴 시 ~1.4초가 부하에서 15초를 넘기는 **증폭 자체는 재현하지 못했다**(12개 테스트
바이너리 동시 실행이라는 정황에서 추론). 15초 실패는 한 번 관측된 뒤 재현되지 않았다. 즉 이 수정은
재현 후 수정이 아니라 **기전 실측으로 정당화**된다 — 둘은 같은 주장이 아니다. 또한 `--test-threads=1`
없이 돌리면 `audit_integration` 7건이 공유 DB 격리 문제로 실패하는데, 이는 이 변경과 무관하며 CI가
그 플래그를 쓰는 이유다(별도 항목으로 분리).

**CI 확인(사후 추가)**: `9f4ba90`의 두 test 잡 모두 `cross_client` 14건이 **실행**됐다 —
`no-default-features` 1.93초, `acp+mtls` 1.57초. 로컬 게이트는 `acp+mtls`만 돌렸고, §4.3 사례 (3)에서
`14 passed`를 `0.00s`에 보고했던 잡은 정확히 `no-default-features`였으므로 이 잡의 소요 시간은
CI 로그로만 닫힌다. 초록 배지가 아니라 시간을 읽어야 하는 이유가 그대로 남아 있다.

**게이트 실행에서 스스로 만든 결함 1건**: 이번 §4.3 게이트를 `cargo clippy ... | tail -5`로 돌리고
`$?`를 찍었는데, 그것은 clippy가 아니라 `tail`의 상태라 무조건 0이었다. 대안으로 쓴
`${PIPESTATUS[0]}`도 zsh에서는 1-기반이라 빈 문자열이었다. 즉 "게이트 통과"라는 판단이 출력의
`Finished` 줄에 대한 **추론**에 얹혀 있었다. 파이프 없이 재실행해 세 게이트의 종료 코드가 실제로 0임을
확인했고(fmt/clippy 2종), 규약은 `agent.md` §4.3에 bullet로 추가했다.

## 2026-08-27 — docs — `TaskAttempt` 존치 여부를 판정했다: 흡수 (`#97`)

`#62` 4단계가 무재시도를 채택하면서 `TaskAttempt`의 주된 존재 이유(시도 generation)가 사라졌지만,
그 엔티티는 tasks 도메인만의 것이 아니었다 — 2026-08-26 실측으로 30개 파일 161행이 그것을 목표
설계로 전제하고 있었다. `#97`은 개명 작업이 아니라 **판정 작업**이었고, 판정은 흡수다. 근거와 용어
대응표는 [Attempt 흡수 판정](architecture/project-task-agent-lifecycle.md#attempt-흡수-판정)이
정본으로 소유한다.

**판별 기준을 먼저 고정했다**: 별도 엔티티는 어떤 경로든 **하나의 Task가 정당하게 두 개의 실행
기록을 갖는** 경우가 있을 때만 정당화된다. 원래 행이 후보로 적어 둔 세 경로를 각 정본의 문언과
대조했다. (1) Agent process 교체 후 재개 — `entity-placement-and-context.md`가 "같은 Agent가 **새**
Attempt를 재개"라고 적었는데, 무재시도 아래에서 그것은 같은 Task의 둘째 실행이 아니라 **새 Task**다.
(2) WarmIdle process 재사용 — `WarmIdle --> Running: "next compatible task"`와 "WarmIdle process에는
실행 중 Task가 없고"는 **한 process가 여러 Task를 거친다**는 뜻이다. (3) 부분 적용 뒤 이어 실행 —
"Task는 terminal 이후 재개하지 않는다. 후속 작업은 새 Task로 만들고". 셋 다 "한 process, 여러
Task"였지 "한 Task, 두 실행"이 아니었다. **세 후보가 전부 현재 문언만으로 판별됐기 때문에** 원래
행이 적어 둔 선행(`#48` 계열 Project/Agent 엔티티)을 기다리지 않았다 — `#48`이 Project를 실제로
만들 때 어긋나는 사실이 나오면 그때 재개정한다.

판정 과정에서 정본 자신의 자기모순을 하나 찾았다. `project-task-agent-lifecycle.md`의 엔티티 표는
Task를 "한 번의 dispatch/attempt"로 적었는데 **바로 다음 줄** `TaskAttempt` 행은 존재 이유를
"retry"로 적고 있었다. 두 행은 같은 표에서 서로를 부정한다. 흡수 판정은 이 둘을 한 행으로 합쳐
해소했다.

**개명이 아니었던 곳은 정확히 한 군데다 — credential grant 수명.** Attempt는 실행 창에만 존재했지만
Task 행은 `Pending` 생성부터 archive 보존 기간까지 산다. "Attempt 단위 grant"를 평평하게 "Task 단위"로
옮겼다면 credential 유효 구간이 **조용히 넓어졌을** 것이다. 그래서 보안 정본 두 곳
([control plane 보안 모델](security/control-plane-security-model.md),
[credentials 진입점](credentials/README.md))에 **"dispatch부터 terminal까지의 실행 구간"**으로 명시하고,
`Pending` Task에는 발급하지 않음과 terminal 전이 시 회수를 함께 적었다. `#97` 행이 착수 전에 이
위험을 미리 지목해 뒀고, 실제로 그 지목이 맞았다.

**Mermaid 노드 삭제는 문법 오류가 아니라 조용한 오류를 만든다.** `Attempt` 노드를 지우면서
`Attempt --> Process`만 지우고 `Task --> Attempt`를 남기면 렌더러가 `Attempt`를 **라벨 없는 유령
노드로 되살린다**. 노드 삭제는 삭제가 아니라 간선 재배선으로 다뤄야 한다. 실제로 5개 다이어그램에서
간선을 다시 이었다. 부수적으로 `system-entities-mapping.md`에서는 라벨 정의가 참조보다 뒤에 오게
되어(Mermaid는 허용하지만 읽는 사람에게 `Process`가 미정의로 보인다) 순서를 바꿨다.

**작업 방식**: 26개 파일을 python3 heredoc으로 `(old, new)` 쌍 목록을 돌리되 각 쌍마다
`if s.count(old) != 1: sys.exit(...)`로 **시끄럽게 실패**하게 했다. 26개 파일 · 100건 남짓의 치환에서
단 한 건도 조용히 빗나가지 않았고, 이는 치환이 성공했다는 뜻이자 **내가 읽은 원문이 정확했다는
증거**이기도 하다. sed였다면 0건 매치와 성공이 구분되지 않는다.

범위: 실측 30개 중 reviews 5개 파일을 뺀 25개 + 집계표를 소유한
[실행 일관성](architecture/tasks/execution-consistency.md) = **26개 파일**. reviews는 결정 당시의 근거
기록이므로 나중 결정으로 소급해 덮어쓰지 않는다 — 덮어쓰면 근거로서의 가치를 잃는다. 집계표는 삭제
대신 "당시의 크기와 위치" 기록으로 과거형 전환했다: 드리프트가 2개 파일이 아니라 30개였다는 사실
자체가 보존할 가치가 있다.

`#95`와의 관계는 무모순을 넘어섰다. `authorization-and-audit.md` 202-204가 "실행 상관 필드는
`attempt_id`가 아니라 `task_id`이며 그것은 이미 있다"를 **적극 단언**하도록 고쳤으므로, `#95` 행이
"그 문장도 함께 고친다"고 적어 둔 몫은 사라졌다. `#95` 행을 그에 맞게 동기화했다.

**`#97` 집계 밖에서 함께 고친 것 1건(정직한 부기)**: `agents/agent-template.md`에서 "retry 예산"
표현 2건을 지웠다(`TemplateUnavailable` 행의 "retry 예산 미소모", 게이트 5의 retry 문구). 무재시도
아래에서 죽은 서술이지만 **`TaskAttempt`/`attempt_id`/`RetryWaiting` 집계에는 잡히지 않던 문자열**
이므로 30개 파일 161행에 포함되지 않았다. 즉 이 커밋의 변경량은 `#97` 집계보다 그만큼 넓다.

**집계 방식의 잔여 오차 1건**: 원래 집계는 대소문자 구분 `grep`으로 reviews를 5개 파일 22행으로
셌는데, 이번 확인에 쓴 `grep -i attempt`는 소문자 "attempt"까지 잡아 23행이 나왔다. 재작성한 절에는
모호하지 않은 **파일 수(5개)만** 적고 행 수를 다시 주장하지 않았다.

검증: 링크·앵커 스크립트 검사 — 27개 파일의 상대 링크에서 파손 0, 새 앵커 `#attempt-흡수-판정`을
참조하는 5개 파일 전부 원천 헤딩에 도달. Mermaid 27블록에 `Attempt` 노드 잔존 0. 잔존 문자열
sweep에서 `docs` 전체에 남은 `TaskAttempt`/`attempt_id`/`RetryWaiting`은 (i) 판정 절 자신, (ii) 의도적
부정 서술("별도 `TaskAttempt` 엔티티는 없다"), (iii) 역사 집계표, (iv) reviews·이 원장·roadmap뿐이다.
`security-findings.md`의 `clear_login_attempts`·`MAX_EMAIL_SEND_ATTEMPTS`는 로그인 rate limit의 Rust
식별자로 무관하며 제외했다.

품질 게이트는 **문서 전용 변경이라는 이유로 건너뛰지 않고** 전부 돌렸다 — `agent.md` §3.2가
"발동 조건을 변경 종류로 좁게 읽으면 내 변경과 무관하게 이미 깨져 있던 것은 영원히 보이지 않는다"고
못 박은 그대로다. rustc 1.98.0(`rust-toolchain.toml`과 일치), `RUSTFLAGS="-D warnings"`,
`cargo fmt --all -- --check` exit=0, clippy `acp mtls` exit=0, clippy `--no-default-features` exit=0
(셋 다 파이프 없이 종료 코드를 직접 읽었다). `cargo build -p fleet-cli --features "acp mtls"` 후
`DATABASE_URL` 주입 `cargo test --workspace --features "acp mtls" -- --test-threads=1` → 69개 스위트
1053건 통과, 0 실패. `cross_client`는 14건 **0.64초**로 조용한 skip(`0.00s`)이 아님을 소요 시간으로
확인했다 — 워밍업 수정 후의 단독 실행 실측 0.66초와 일치한다.

**검증 한계**: 문서 전용 변경이라 코드로 증명할 대상이 없다. `git diff --name-only`가 27개 전부
`.md`임을 확인했고 위 게이트는 이 커밋이 무엇을 깨뜨렸는지가 아니라 **트리의 현재 상태**를 말한다.
Mermaid는 노드 잔존만 스크립트로 검사했고 렌더러로 실제 파싱시키지는 않았다. 목표 설계 서술이 판정과
**의미상** 맞는지는 사람 판독으로만 확인했다 — 스크립트가 보장하는 것은 문자열 치환의 정확성이지
문장이 여전히 말이 되는지가 아니다. 판정 자체도 **현재 정본 문언**을 근거로 하며, `#48`이 Project
엔티티를 실제로 구현할 때 문언에 없던 사실이 나오면 재개정 대상이다. 세션이 자정을 넘겨 판정일은
2026-08-26, `last_verified`와 이 항목은 최종 검증을 마친 2026-08-27이다.

## 2026-08-27 — security — `#48` 차단 조건 1을 승인으로 닫고, 함께 묶여 있던 노후 문언 3건을 정정했다

사용자 승인("보안모델 승인")으로 [Project 기능 설계](architecture/project-feature-design.md)의
차단 조건 1(`ProjectPolicyManage`와 `AgentCreate`의 관계)이 닫혔다. 승인을 기록하려고 대상 문언을
찾았더니 **제안 자체가 없었다** — [Project model review](reviews/project-model-review-2026-08-17.md)는
"보안 모델과 구현 계획에서 결정한 뒤 정본 계약에 반영한다"고 미뤘을 뿐이고, 그 뒤로 아무도 규칙을
쓰지 않았다. 따라서 이 작업은 비준이 아니라 **규칙 작성**이었고, 정본은
[Authorization·Project Scope·감사](security/authorization-and-audit.md)의 "Project 정책 변경과
Agent 생성의 관계"가 소유한다. 그 문서를 고른 것은 취향이 아니다 —
[Control Plane 보안 모델](security/control-plane-security-model.md)이 "capability 카탈로그, Project
scope의 상세 계약은 authorization-and-audit.md가 정본이다. 이 문서는 분류표를 중복 보유하지 않는다"고
명시적으로 위임한다.

**승인된 규칙 3건.** (1) 필드별 게이팅 — Agent의 수·provisioning 대상을 바꾸는 정책 필드는
`project:policy_manage`에 더해 `agent:manage`를 요구하고, 나머지 정책 필드는 아니다. `#86`의 H1
결정(2026-08-22)이 템플릿 편집에 쓴 것과 같은 형태이며, 위험한 것은 표면이 아니라 어느 필드가
무엇을 만들 수 있는가라는 판단을 재사용했다. (2) 권한 확인 시점은 Task 제출이 아니라 **정책 쓰기**다.
admission은 한도를 집행하고 권한을 판정하지 않는다 — 반대로 하면 모든 contributor가 `agent:manage`를
가져야 하고, 그러면 Project 정책이 아니라 개별 권한이 상한을 정하게 되어 정책이 의미를 잃는다.
"누가 이 Agent 생성을 승인했는가"는 Task가 아니라 정책 revision의 audit event가 답한다.
(3) 메타데이터 편집은 무관 — `name`·`description`은 Agent를 만들지 않는다.

**승인을 기록하려다 발견한 노후 문언 3건.** 차단 문구가 실제로 무엇을 가리키는지 대조한 결과다.

- **`project:assign`과 `agent:create`는 목표 capability 표에 없는 이름이었다.**
  `authorization-and-audit.md`의 Agent 영역은 `agent:read`/`agent:manage`/`agent:attach`뿐이고
  Project 영역에 `assign`은 없다. 존재하지 않는 두 이름의 "관계"를 승인 조건으로 걸어 둔 셈이며,
  그대로 두면 구현 시점에 아무 이름에나 갖다 붙일 수 있다 — `#66`에서 `worker:delete`가 LLM
  credential 삭제를 흡수한 경로가 정확히 그것이다. 규칙을 `agent:manage`에 묶었다.
- **host·worker Project 배정·해제 endpoint는 승인 대기가 아니라 설계상 존재하지 않는다.**
  같은 설계 문서의 공유 실행 풀 불변식이 "Host와 Worker에는 `project_id`를 두지 않는다"로 이미
  배제했다. 차단 후보 목록에 남겨 두면 언젠가 승인되어 생길 것처럼 읽히므로 뺐다.
- **`PATCH /api/projects/{id}`는 보안 차단 대상이 아니었다.** 메타데이터 편집인데 Agent 우회 위험과
  한 문장에 묶여 있었다. H1이 템플릿 편집을 같은 이유로 뺀 것과 같은 과잉 적용이다. 남은 선행 조건은
  보안이 아니라 계약이다 — 같은 절이 구현 전에 확정하라고 정한 동시 편집 의미(revision 또는
  `If-Match`, `request_id`)가 미결이고, `projects`에 revision 컬럼이 없으므로 `updated_at` 기반
  `If-Match`로 갈지 컬럼을 신설할지가 실제 결정 대상이다.

**승인이 열지 않은 것.** 차단 조건 2·3은 그대로다. 셋 중 **1번만 사람 결정**이고 2·3은 테스트
조건이며, 그 대상인 Agent 엔티티와 `worker_execution_leases`(`#67`)가 없어 지금은 작성할 수조차
없다. 따라서 `fleet_update_project_policy`와 정책 컬럼은 여전히 차단이고, `project:policy_manage`와
`agent:manage` capability도 만들지 않았다 — 관리·검사할 대상이 없어 만들면 죽은 권한이 된다
(`issue:archive_hold_manage`를 만들지 않은 것과 같은 판정). 집행 증명은 `authorization-and-audit.md`의
구현 게이트 9로 등록해, 규칙만 문서에 남고 시험이 사라지는 경로를 막았다.

코드 변경은 노후 사유를 적고 있던 주석 2건뿐이다. `fleet-core/src/auth.rs`는 두 capability의 부재
사유를 "승인 전 차단"이라고 적고 있었고, `fleet-dashboard/src/handlers.rs`는 `PATCH`를 "policy 변경"
이라고 잘못 부르며 같은 사유를 달고 있었다. 승인 뒤에는 둘 다 틀린 설명이 되므로 실제 사유로 고쳤다.

**게이트 실행 중 같은 결함을 재현했다 — 기록해 둔다.** 처음 `cargo test`를 `| tail -60`으로 돌렸고,
읽은 `exit=0`은 `cargo test`가 아니라 `tail`의 종료 코드였다. 출력 파일에도 마지막 60줄(doc-test)만
남아 "10개 스위트, 1건 통과"라는 말이 안 되는 수치가 나와서야 드러났다. `agent.md` §4.3은 이 결함을
이미 적어 뒀지만 **clippy 예시로** 적어 뒀고, 명령이 `cargo test`로 바뀌자 적용을 놓쳤다 — 규칙을
예시에 붙은 것으로 읽으면 예시가 바뀔 때 규칙이 사라진다. 파이프 없이 파일로 리다이렉트하고
종료 코드를 직접 읽어 다시 돌렸다.

rustc 1.98.0(`rust-toolchain.toml`과 일치), `RUSTFLAGS="-D warnings"`, `cargo fmt --all -- --check`
exit=0, clippy `--features "acp mtls"` exit=0, clippy `--no-default-features` exit=0 — 넷 다 파이프
없이 종료 코드를 직접 읽었다. `cargo build -p fleet-cli --features "acp mtls"`(121MB 바이너리 생성
확인) 후 `DATABASE_URL` 주입 `cargo test --workspace --features "acp mtls" -- --test-threads=1`이
`exit=0`, 69개 스위트 1053건 통과 0 실패. `cross_client`는 14건 **0.73초**로 `0.00s` 조용한 skip이
아님을 소요 시간으로 확인했다(워밍업 수정 후 실측 0.64~0.66초와 일치).

**검증 한계**: 승인된 규칙 자체는 코드로 증명되지 않는다 — 집행 대상인 `project:policy_manage`,
`agent:manage`, Agent 엔티티, 정책 컬럼이 모두 없어서 게이트 9를 지금 쓸 수 없다. 이 커밋이 증명하는
것은 규칙의 **정확한 기록**과 노후 문언 정정이지 규칙의 **집행**이 아니다. 규칙의 내용은 기존 정본
(`authorization-and-audit.md`의 "Task 생성 권한이 Agent 생성·Project 정책 변경 권한을 암묵적으로
주지 않는다")과 `#86` H1 선례에서 도출했으나, 사용자 승인은 차단 조건 1이 **제기된 형태**에 대한
것이었고 결정 2·3(확인 시점, 메타데이터 제외)은 그 승인을 구체화하며 내가 쓴 것이다 — 의도와 다르면
이 절과 로드맵 행을 함께 고쳐야 한다. 위 게이트는 이 커밋이 무엇을 깨뜨렸는지가 아니라 트리의 현재
상태를 말한다(코드 변경이 주석 2건뿐이므로 특히 그렇다).

## 2026-08-27 — ci — `main`의 빨간 X가 코드 실패가 아니었다: 판정처럼 보이는 값의 세 번째 얼굴

`#48` 4단계(`3767283`)를 푸시하며 CI를 확인하던 중, 바로 앞 커밋 `ddd2b85`(`#97`)의 run
`32984609335`이 `conclusion: failure`로 닫혀 있는 것을 발견했다. **코드 실패가 아니다.**

```
{"conclusion":"failure","status":"completed","jobs":[
  {"name":"Coverage report","status":"queued","completedAt":"0001-01-01T00:00:00Z","conclusion":""},
  {"name":"Test + Clippy (no features)","status":"queued","completedAt":"0001-01-01T00:00:00Z","conclusion":""},
  {"name":"Test + Clippy (acp+mtls)","status":"queued","completedAt":"0001-01-01T00:00:00Z","conclusion":""},
  {"name":"Shellcheck install/uninstall","status":"queued","completedAt":"0001-01-01T00:00:00Z","conclusion":""}]}
```

네 잡 모두 `queued`에서 벗어나지 못했고 `completedAt`이 제로 타임이다. GitHub Actions
`major_outage` 중 러너가 배정되지 않은 채 run만 종료됐다. `gh run rerun`은 "This workflow run
cannot be retried"로 거부했고 `ci.yml`에는 `workflow_dispatch`가 없어, 다음 푸시가 유일한 통로였다.

**왜 기록하는가.** run의 `conclusion`은 잡 결과의 집계가 아니라 run 자체의 종료 상태다. 한 줄도
실행되지 않아도 `failure`가 될 수 있고, 그 X는 `main`에 영구히 남는다. `conclusion`만 읽으면 없는
버그를 쫓게 되고, 반대로 "저건 장애였다"고 넘기는 습관이 들면 진짜 실패도 같은 취급을 받는다.
구분은 `conclusion`이 아니라 잡의 `status`/`completedAt`에 있다.

이것은 오늘 세 번째로 같은 형태다.

| 판정처럼 보이는 값 | 그것을 무효화하는 옆의 값 |
|---|---|
| `cargo test ... \| tail -60` 뒤의 `$?` = 0 | 그 0은 `tail`의 것이다 (zsh `PIPESTATUS`는 1-기반이라 `${PIPESTATUS[0]}`도 빈 문자열) |
| `cross_client`의 `14 passed` | `finished in 0.00s` — 바이너리 부재로 전부 조용히 skip (§4.3의 (3)) |
| run의 `conclusion: failure` | 잡의 `status: queued`, `completedAt: 0001-01-01` — 실행 자체가 없었다 |

`agent.md` §4.3의 "판정을 파이프 뒤에서 읽지 않습니다" bullet에 두 항목을 붙였다. 하나는 CI
`conclusion` 사례이고, 다른 하나는 **그 규칙이 이미 있었는데도 오늘 재발한 이유**다 — 예시가
clippy로만 적혀 있어서 `cargo test`로 명령이 바뀌자 적용 대상으로 읽히지 않았다. 규칙의 사거리를
예시가 좁힌 것으로, 게이트 목록이 CI보다 약하면 드리프트가 재발한다는 §4.3의 법칙과 같은 결이다.

**이번 푸시의 CI 결과.** run `33019959927`(`3767283`) — 네 잡 전부 `success`, run `conclusion:
success`. 이번에는 잡 상태까지 확인했다. 장애 이후 처음으로 `main`이 실제 CI 신호를 받았다.

**검증 한계.** 장애 자체는 GitHub 측 상태 페이지가 아니라 관측된 증상(전 잡 `queued` 정체,
`rerun` 거부)으로 판단했다. `ddd2b85`의 내용이 CI를 통과했을 것이라는 직접 증거는 없고, 다만
그 트리를 포함한 `3767283`이 통과했으므로 회귀가 남아 있지 않다는 것은 확인됐다.

## 2026-08-27 — security — 파생 결정 2·3도 승인을 받아, 승인 범위 기록을 닫았다

바로 위 항목이 남긴 미결을 닫는다. 차단 조건 1의 1차 승인("보안모델 승인")은 **관계를 확정하라**는
제기 형태에 대한 것이었고 결정 1이 그 직접적 답이었다. 결정 2(권한 확인 시점은 정책 쓰기)와 결정
3(메타데이터 편집은 무관)은 그 승인을 집행 가능한 규칙으로 구체화하며 내가 쓴 것이라, 정본과 로그
양쪽에 "승인 범위 밖"이라고 명시하고 별도 확인을 요청했다. 사용자가 문언 그대로 승인했다
("결정 2,3은 결정한대로 진행하자"). [Authorization·Project Scope·감사](security/authorization-and-audit.md)의
"승인 범위" 문단을 2단계 승인으로 고쳤고, 세 결정 모두 지금은 승인된 규칙이다.

**규칙 문언은 한 글자도 바뀌지 않았다.** 바뀐 것은 각 결정이 누구의 판단인지에 대한 기술뿐이다.
따라서 이 커밋은 `#48`의 어떤 차단도 새로 열지 않는다 — 조건 2·3은 그대로 열려 있고,
`project:policy_manage`/`agent:manage` capability와 정책 컬럼도 여전히 만들지 않는다.

**왜 이 문단을 유지하는가.** 승인 범위를 지우고 "승인됨"으로만 남기면 다음 개정 때 어디까지가
사람 결정이고 어디부터가 파생인지 알 수 없어진다. 파생 규칙은 근거가 무너지면 함께 무너져야
하는데(결정 2의 근거는 "반대로 하면 Project 정책이 의미를 잃는다"이지 사용자의 선호가 아니다),
출처를 지우면 그 연결이 끊긴다. 그래서 승인이 완료된 뒤에도 문단을 지우지 않고 2단계 사실로
고쳐 두고, "이후 변경은 새 승인을 받는다"를 함께 남겼다.

**게이트.** 코드 변경이 없다 — `git status --porcelain`이 `*.rs`·`Cargo.toml`·`Cargo.lock`·
`crates/`·`.github/`에 대해 비어 있고 변경은 `docs/` 2개 파일뿐이다. 따라서 트리는 CI가 4잡 전부
success로 확인한 `5d56212`와 코드에 관해 바이트 동일하며, `cargo fmt --all -- --check`만 파이프
없이 다시 읽었다(exit=0).

**검증 한계**: 승인 사실 자체는 대화 기록에만 있고 저장소 안에서 증명할 수단이 없다 — 이 로그
항목과 정본의 "승인 범위" 문단이 그 기록이다. 규칙의 집행은 여전히 증명되지 않았다(게이트 9는
대상 capability와 Agent 엔티티가 없어 아직 쓸 수 없다).

## 2026-08-27 — feat — 리스를 잃었다 되찾는 창을 닫았다: 술어는 "지금 내가 기관인가"가 아니라 "이 결과가 내 세대의 것인가"를 물어야 한다 (`#67` 1단계)

`516f85e`(`#62` 3단계)가 넣은 fence 술어는 `EXISTS(control_plane_lease WHERE cluster_id = $4
AND epoch = $5)`다. 이것이 닫는 것은 **"쓰기를 보내는 이 인스턴스가 지금도 제어 기관인가"**다.
그런데 같은 프로세스가 리스를 잃었다 **되찾으면** 그 술어가 성립한 채로 낡은 결과가 통과한다.

epoch 5로 디스패치 → 리스가 넘어가 6이 어딘가에서 올라가고 그쪽이 이 작업을 재디스패치할 수
있다 → 같은 프로세스가 7로 재획득. 이제 epoch 5에 보냈던 dispatch의 결과가 도착한다. 위상은
`Dispatched`로 **맞고**, lease 술어도 epoch 7로 **성립한다** — 지금 이 인스턴스가 진짜 제어
기관이기 때문이다. 술어가 하나뿐이면 epoch 5의 결과가 epoch 6의 진행을 덮어쓴다.

**두 술어가 서로 다른 질문을 한다는 것이 이번 변경의 전부다.** 앞은 *"지금 내가 제어 기관인가"*,
새 술어는 *"이 결과가 내 세대의 것인가"*. 앞이 참이어도 뒤가 거짓일 수 있고, 그 조합이 창이다.
`#62` 4단계가 만든 `tasks.dispatch_control_epoch`(migration 026)가 이미 dispatch 시점의 세대를
행에 싣고 있으므로, 새 컬럼이나 새 테이블 없이 술어 한 줄로 닫힌다.

```sql
AND (dispatch_control_epoch IS NULL OR dispatch_control_epoch = $5)
```

### 최초 설계가 틀렸다: 발동 조건은 목표 상태가 아니라 전이의 출처다

들어올 때 계획은 "`Completed`/`Failed` 전이에 세대 술어를 건다"였다. `mark_failed`의 호출부
6개를 읽고 나서 그 계획을 버렸다. 그중 셋은 **reconciler의 고아·오프라인 스윕**이고 둘은
**dispatch 실패 확정**이다 — 전부 워커가 보고한 결과가 아니라 **현재 보유자가 지금 내리는
결정**이다. 목표 상태로 유도하면 그것들까지 술어에 걸리고, 그러면 **epoch 5에 디스패치된
고아를 epoch 7 보유자가 영원히 회수하지 못하는 라이브락**이 된다. 세대가 넘어간 뒤에 남는
고아가 정확히 reconciler가 존재하는 이유인데, 그 경로를 막는 셈이다.

저장소는 이 출처를 상태로부터 유도할 수 없다. 그래서 호출자가 선언한다 —
`TransitionOrigin{WorkerOutcome, ControlDecision}`. 술어는 `WorkerOutcome`에만 걸리고,
전체 CAS 호출 중 그 값을 넘기는 것은 `WorkerEvent::Completed`/`Failed` 핸들러 **둘뿐**이다.

### 거절을 기존 결과에 접지 않은 이유

`TransitionOutcome::StaleDispatchEpoch { dispatched_under }`를 새로 만들었다.

- `Fenced`에 접으면 → "나는 더 이상 제어 기관이 아니다, 이후 모든 쓰기도 실패한다"로 읽힌다.
  실제는 정반대다(기관이 맞고 다음 쓰기는 성공한다).
- `Rejected { current }`에 접으면 → `current: Dispatched`를 보고하게 되는데, 위상은 실제로
  **맞았다.** 없는 경합을 조사하라고 운영자를 보내는 거짓 신호다.

변형을 새로 만든 대가로 컴파일러가 영향받는 match 지점 4개를 전부 지목했다. 접었다면 그 4개는
조용히 흡수됐을 자리다. 그중 둘은 구성상 도달 불가능한데, `unreachable!()` 대신 에러 반환으로
막았다 — 스케줄러 루프 안이라 panic이 프로세스를 죽인다.

### NULL을 통과시킨 것은 026의 의미를 따른 것이다

migration 026에서 NULL은 "값을 못 구했다"가 아니라 **"제어 세대라는 개념이 없는 배포"**다
(단일 인스턴스, 또는 026 이전 행). 거절했다면 HA를 나중에 켠 배포에서 전환 이전에 디스패치된
작업이 **전부 종료 불가**가 된다.

### 기존 테스트 하나가 새 술어와 정면으로 충돌했다

`terminal_transitions_do_not_overwrite_the_dispatch_epoch`(`#62` 4단계)는 나중 fence 아래에서
워커 완료 보고를 적용시키고 `Applied`를 기대했다 — 새 술어가 정확히 거절하는 조합이다.
지키려는 성질("종료 전이는 dispatch 세대를 덮어쓰지 않는다")은 그대로 두고 전이를 **reconciler의
고아 회수**(`ControlDecision`)로 바꿨다. 거절되는 쪽은 새로 넣은 `#67` 절이 따로 다룬다.

### 게이트

- `cargo build -p fleet-cli --features "acp mtls"` → exit=0 (§3.2, `cross_client`가 subprocess로
  쓰는 `target/debug/fleet`는 `cargo test`가 만들지 않는다)
- `cargo test --workspace --features "acp mtls" -- --test-threads=1` → **exit=0, 69개 스위트,
  실패 0.** `task_cas` 21건 `finished in 0.40s`, `cross_client` 14건 `0.67s` — 둘 다 `0.00s`가
  아니므로 조용한 skip이 아니다
- `cargo build -p fleet-cli --no-default-features` exit=0 + `cargo test --workspace
  --no-default-features -- --test-threads=1` → **exit=0, 69개 스위트, 실패 0.** `task_cas`
  21건 `0.51s`, `cross_client` 14건 `12.49s`. 이 잡에서도 `task_cas`가 **실제로 채점된다는
  것을 확인했다** — 새 테스트가 한쪽 잡에서만 돌고 있었다면 게이트를 통과했다고 말할 수
  없다. `cross_client`의 12.49s는 acp+mtls 잡의 0.67s보다 훨씬 느리지만(피처가 다른 별도
  바이너리라 §3.3의 첫 exec 비용을 다시 지불한다) 개별 테스트 기준 `RESPONSE_TIMEOUT` 15초
  아래이고 14건 전부 통과했다
- §4.3 5줄: `rustc 1.98.0`(rust-toolchain.toml과 일치), `RUSTFLAGS="-D warnings"`,
  `cargo fmt --all -- --check` exit=0, clippy acp+mtls exit=0, clippy no-default-features exit=0.
  전부 파이프 없이 `> file 2>&1; echo "exit=$?"`로 판정을 직접 읽었다

**게이트를 두 번 잘못 돌렸고, 둘 다 CI 형태와 어긋나서였다.** (1) 처음에 `cargo test -p
fleet-store --test task_cas`를 병렬로 돌려 15건이 `NotFound`로 실패했다. 원인은 코드가 아니라
하네스 전제다 — `pg_backend()`가 **호출될 때마다** `TRUNCATE`를 돌리므로 병렬에서는 A의
truncate가 B의 시드를 지운다. 파일 상단 주석에만 `--test-threads=1`이 적혀 있고 코드는 이를
강제하지 않는다. (2) 그다음 `cargo test --workspace`를 피처 없이 돌렸더니 `task_cas`가 **아예
실행되지 않았다**. `Running tests/task_cas.rs` 줄이 없다는 것으로만 드러난다. CI는
`--features "acp mtls" -- --test-threads=1`을 쓰므로 두 번 다 **CI보다 약한 게이트**였다.
`agent.md` §4.3의 "게이트 목록이 CI보다 약하면 드리프트는 반드시 재발한다"의 또 다른 얼굴이다 —
이번에는 목록이 짧아서도, 툴체인이 달라서도 아니고 **같은 명령의 인자가 달라서** 생겼다.

**(2)의 원인을 처음에 틀리게 적었다가 고쳤고, 그 오진을 여기 남긴다.** 처음 쓴 설명은
"`test-support`가 켜지지 않아 타깃이 빠졌다"였다. 근거는 몇 분 전에 `cargo test -p fleet-store
--test task_cas`가 `E0432: unresolved import fleet_store::mem`으로 죽은 것이었는데, **그 경험을
워크스페이스 실행에 잘못 일반화했다** — `-p`로 크레이트 하나만 고르면 피처 통합이 일어나지
않지만, 워크스페이스 실행에서는 다른 크레이트의 dev-dependency가 `test-support`를 켜 준다.
실측 둘이 이것을 뒤집는다: 피처 없는 `cargo test --workspace --no-run`이 `task_cas` 실행
파일을 **만들고**, 최종 `--no-default-features` 게이트에서 이 파일이 **21건 실제로 채점됐다**
(0.51s). 진짜 원인은 cargo의 기본 fail-fast다 — 같은 로그에서 `audit_integration`이 실패해
그 시점에 중단됐고, fleet-store의 타깃 순서상 `task_cas`는 그 뒤라 도달하지 못했다. 실패가
아니라 **미도달**이었다.

두 진단은 증상이 같고(줄이 없다) 함의가 정반대다. "타깃이 빠졌다"면 CI도 이 테스트를 영영
돌리지 않는다는 뜻이지만, "미도달"이면 앞선 실패만 치우면 돌아간다. 전자를 믿었다면 있지도
않은 피처 게이트를 쫓았을 것이다. 게다가 옳은 사실은 **이 파일에 이미 적혀 있었다** —
2026-08-25 항목이 "워크스페이스 피처 통합으로 `test-support`가 켜진다"고 기록해 두었다.
기록이 있어도 직전 경험이 더 생생하면 그쪽을 믿는다는 것이, 이 오진이 남길 만한 이유다.

`audit_integration` 6건은 병렬 실행에서 실패했다가 직렬에서 7건 전부 통과했다 — 알려진 격리
문제(별도 작업으로 분리되어 있다)이며 이 변경과 무관하다.

### 의도적으로 만들지 않은 것

"채울 방법이 없는 것은 미리 만들지 않는다"를 그대로 적용했다. 표는
[권한과 장애 전환](architecture/control-plane-authority-and-failover.md)의 "`#67` 1단계" 절에
있다. 요지는 `worker_execution_lease`·`worker_incarnation`·ACK·self-fencing이 전부 **워커→
오케스트레이터 제어 스트림**을 전제하는데 그 채널이 없다는 것이다 — `WorkerTransport`의
오케스트레이터→워커 표면은 `dispatch`/`cancel`이 전부다. `#89`로 귀속시켰다.

인접 결함 2건은 발견만 하고 손대지 않았다(정본에 기록). ① `WorkerEvent::Failed`가 워커가
보고한 실패(확정)와 관측을 잃은 것(타임아웃, 연결 상실 시 `fail_all()`)을 한 종류로 뭉친다.
② prompt 타임아웃 경로가 `cancel`을 보내지 않은 채 transport 용량 permit만 놓는다.

### 검증 한계

- 새 테스트 5건은 두 백엔드 모두를 돌지만, **전부 단일 프로세스가 순차로 리스를 뺏고 되찾는
  재구성**이다. 두 오케스트레이터 프로세스가 동시에 떠 있는 상태에서 늦은 결과가 도착하는
  라이브 실행은 하지 않았다. 즉 **술어의 정확성은 검증됐고, 이 창이 프로덕션에서 실제로
  열린다는 관측은 없다** — 창의 존재는 코드 대조로만 확인했다.
- **같은 epoch 안의 재디스패치는 이 술어로 구분할 수 없다.** 구분하려면 세대가 아니라
  시도(attempt) 단위 신원이 필요한데 그 개념이 없다. 지금 그 창이 닫혀 있는 이유는 술어가
  아니라 `#62` 4단계의 무재시도 정책이다 — **재시도 정책이 바뀌면 이 창이 다시 열린다.**
- 라이브락(목표 상태로 유도했을 때 고아가 회수되지 않는 것)은 **실제로 발생시켜 보지 않았다.**
  `control_decision_can_reap_a_task_dispatched_under_another_epoch`가 그 반대 조건을 고정할
  뿐이다. 잘못된 설계를 구현해 재현한 것이 아니라 호출부 6개를 읽고 판정했다.

---

## 2026-08-27 — test — 커밋에 적은 원인이 맞았지만, 적을 당시에는 근거가 없었다 (`cross_client` 12.49초)

`#67` 1단계를 푸시한 뒤 게이트 기록에 설명되지 않은 숫자 하나가 남아 있었다. `cross_client`가
`--no-default-features` 게이트에서 **12.49초**, acp+mtls 게이트에서 **0.67초**였다. 같은 머신,
같은 세션이다. 여기에 "별도로 빌드된 바이너리가 §3.3의 첫 exec 비용을 다시 지불한 것"이라고
적었는데, 이것은 **추론이었고 근거를 적지 않았다.**

게다가 그 설명은 §3.3의 수정과 표면상 충돌한다. `spawn_server()`의 `std::sync::Once` 워밍업은
바로 그 비용을 없애려고 넣은 것이다. 설명이 맞다면 워밍업이 이 경로를 못 덮고 있다는 뜻이고,
그건 이 저장소가 하중을 싣고 있는 수정에 난 구멍이다. 틀렸다면 최소 피처 세트 어딘가에서
12초가 나오고 있고, 로그는 **그럴듯하지만 틀린 원인**을 싣게 된다 — 바로 두 문단 위에서 방금
고친 그 결함이다.

실측 셋으로 갈랐다.

| 조건 | 결과 |
|---|---|
| 같은 바이너리, 연속 2회 | 0.71초 / 0.62초 |
| `rm target/debug/fleet` → 재빌드 직후 1회 | **7.84초** |
| 그 직후 1회 | 0.74초 |

**콜드 바이너리 비용이 맞다** — 10.6배, 12.49초와 같은 크기대다. 첫 두 줄이 "최소 피처 세트의
구조적 비용"이라는 대안을 배제한다.

**§3.3과의 충돌은 해소된다. 그 항목의 "타이머"는 응답당 `RESPONSE_TIMEOUT`(15초)이지 suite 총
소요 시간이 아니다.** 워밍업은 비용을 첫 응답 대기 밖으로 옮길 뿐 없애지 않으므로, 콜드
바이너리로 시작한 suite는 여전히 8~12초가 걸린다. 세 번 모두 14/14 통과했고 **그것이 워밍업이
제 역할을 했다는 증거다** — 12.49초는 결함의 신호가 아니라 워밍업이 없었다면 15초 벽에
부딪혔을 실행이 통과한 흔적이다. §3.3 기록에 이 범위를 명시했다.

여기서 소요 시간은 판정이 **아니고** 통과 여부가 판정이다. §4.3의 조용한 skip 탐지에서는 정반대로
소요 시간이 판정이었다(`14 passed`를 `finished in 0.00s`가 뒤집는다). **같은 숫자가 무엇을
말하는지는 무엇을 의심하고 있느냐에 달린다.**

### 검증 한계

- **7.84초와 게이트의 12.49초 차이(1.6배)는 설명하지 못한다.** 게이트는 워크스페이스 전체 실행
  중이었고 다른 테스트 바이너리와 머신을 나눠 쓰고 있었다 — 정황이지 실측이 아니다. §3.3의
  "부하에서의 증폭"이 재현되지 않은 것과 같은 자리에 남는 미해명이다.
- 재빌드 후 `ls`의 mtime이 09:24로 **보존됐다**(캐시 hardlink로 보임). 파일이 새로 놓인 것은
  `rm`으로 확인했지만 페이지 캐시 상태를 직접 관측하지는 않았다. 7.84초라는 결과가 콜드였음을
  사후에 말해줄 뿐이다.
- 이 조사는 `#67` 1단계의 정확성과 무관하다. 세 실행 모두 14/14 통과했고 CI 네 잡도 전부
  `completed`/`success`였다. 고친 것은 **코드가 아니라 기록의 근거**다.

**남길 교훈은 "틀렸다"가 아니라 "맞았지만 몰랐다"이다.** 원인 귀속이 결과적으로 옳았다는 것은
적을 당시에 근거가 있었다는 뜻이 아니다. 근거 없이 적힌 옳은 문장과 근거 없이 적힌 틀린 문장은
**쓰는 시점에 구별할 수 없고**, 그래서 둘 다 같은 결함이다.

---

## 2026-08-28 — feat/test — Agent 엔티티 1단계, 그리고 테스트 격리가 CI 설정에 숨어 있다는 것 (`#49`)

`#49`의 1단계로 Agent 엔티티(`agents` 테이블, Store 6개 메서드, MCP 도구 3종, Dashboard API 3종,
Project 상세의 Agents 절)를 넣었다. 상태는 목표 설계의 8종이 아니라 `ready`/`stopped` **둘뿐**이고,
나머지는 각각을 막고 있는 로드맵 ID와 함께 [provisioning.md](architecture/agents/provisioning.md)의
유예 표에 남겼다 — 채울 방법이 없는 상태를 미리 만들지 않는다.

`022`가 `tasks.project_id`를 "순수 미검증 메타데이터"로 남긴 전례가 있어서, 이번에는 **읽는 쪽을
같은 커밋에 넣었다**: `ensure_project_accepts_new_agents`, archive 게이트의 두 번째 조건
(`project_has_live_agents`), 그리고 `agent:read`/`agent:manage`가 드디어 검사할 대상. FK도 `022`와
반대로 **실제로 걸었다** — 근거는 같다. `022`는 컬럼이 마이그레이션보다 먼저 존재해서 검증되지 않은
기존 데이터가 있을 수 있었고, `027`은 컬럼을 이 마이그레이션이 만들므로 그런 데이터가 존재할 수 없다.

### 새 테스트가 관례를 그대로 베꼈다가 깨진 것

`tests/agents.rs`는 `tests/projects.rs`를 본떠 매 테스트 시작 시
`TRUNCATE agents, tasks, projects CASCADE`를 돌렸다. 단독 실행(`--test-threads=1`)에서 10/10 통과했다.
스레드 수를 지정하지 않은 `cargo test --workspace`에서는 **10건 중 6건이 실패**했고 전부 같은
메시지였다:

```
Conflict("no such project for agent: insert or update on table \"agents\"
         violates foreign key constraint \"agents_project_id_fkey\"")
```

cargo는 한 바이너리 안의 테스트를 기본으로 **병렬** 실행한다. A가 만든 Project 행을 B의 TRUNCATE가
지우고, 그 사이 A가 Agent를 INSERT한다.

### 그런데 이건 내가 만든 결함이 아니라 저장소의 관례였다

원인을 그렇게만 적었으면 절반만 맞았을 것이다. 실제로 확인해 보니:

- `TRUNCATE`를 쓰는 통합 테스트가 **15개 파일**(`fleet-store` 14 + `fleet-scheduler`의 `scaleout_sync`).
- `ci.yml`은 세 잡 모두 `cargo test ... -- --test-threads=1`로 돌리며, 주석에
  "`--test-threads=1` 필수: fleet-store integration test 가 TRUNCATE ... CASCADE 로"라고 적혀 있다.
- 실측: `projects.rs`는 병렬에서 8건 중 3~4건 실패(3회), 직렬에서 8건 통과.
  `audit_integration`은 병렬에서 6~7건 실패, 직렬에서 7건 통과.

즉 **관례는 의도된 것이고 CI가 그것을 강제한다.** 내 로컬 게이트가 `--test-threads=1`을 빠뜨려
CI보다 **엄격했던** 것이다. §4.3이 기록해 온 드리프트는 늘 "로컬이 CI보다 약해서" 생겼는데,
이번에는 방향이 반대다 — 그리고 그 방향에서도 결과는 같다. **게이트와 CI가 다르면, 어느 쪽이
엄격하든 로컬의 판정은 CI의 판정이 아니다.** 판정은 CI 형태로 다시 돌려 받았다(70 스위트 전부 ok).

`audit_integration`이 직전 실행에서 안 보였던 이유도 여기 있다. `cargo test`는 기본이 fail-fast라
24번째 타깃(`agents`)에서 멈췄고, 25번째(`audit_integration`)를 **한 번도 실행하지 않았다.**
`git stash -u`로 HEAD를 복원하고 새 DB에서 돌려 이것이 내 변경과 무관함을 확인했다
(HEAD·fresh DB·병렬 2회 모두 6 failed, 직렬 7 passed).

### 그래도 `agents.rs`는 관례를 따르지 않기로 했다

TRUNCATE를 되돌리는 대신 걷어내고, 각 테스트가 UUID 접미사를 붙인 유일한 Project를 만들도록 했다
(`projects.name`이 전역 UNIQUE라 접미사가 필요하다). 이유는 취향이 아니다 — `027`의 FK 때문에 이
파일에서는 경합이 조용한 오답이 아니라 즉시 터지고, 그러면 "이 파일은 특정 스레드 수에서만
통과한다"는 전제가 **파일 밖 CI 설정에 숨는다.** 공유 상태를 지우는 대신 애초에 공유하지 않으면
그 전제 자체가 없어진다.

함께 죽는 단언이 하나 있었다. `all.len() == 3`은 "테이블이 비어 있다"에 기대는 단언이라, 비우기를
멈추면 근거를 잃는다. `우리 행만 있다` → `우리 행이 빠지지 않는다`로 약화시켰다. 결과는 스레드 수와
무관하게 10/10이고 연속 재실행에서도 통과한다.

### 검증 한계

- **나머지 14개 파일은 그대로 뒀다.** `#49`의 범위가 아니고, 한 번에 바꾸면 이 커밋의 판정이
  Agent 기능이 아니라 하네스 리팩터링에 대한 것이 된다. 그 파일들은 병렬에서 계속 깨지며,
  CI는 `--test-threads=1` 때문에 그 사실을 **구조적으로 볼 수 없다.**
- **`agents.rs`의 6건 실패는 한 번 관측하고 고쳤다.** 수정 후 통과는 여러 번 확인했지만, 수정 전
  실패율이 스레드 수·머신 부하에 따라 어떻게 변하는지는 재지 않았다.
- 1단계 Agent는 **실행되지 않는다.** archive 게이트를 붙잡는 것은 Agent *행*이지 Agent *프로세스*가
  아니다. `#48`의 미해결 조건 2·3은 이 커밋으로 풀리지 않는다 — 조건 2가 시험하는 *자동* provisioning
  경로 자체가 없고, 조건 3은 execution lease(`#67` 후속)에 걸려 있다.
- `authorization-and-audit.md`의 게이트 9는 여전히 **시험 불가**다. `agent:manage`는 생겼지만
  `project:policy_manage`도 정책 컬럼도 없어서 바꿔 볼 필드가 없다.

---

## 2026-08-28 — fix — 디스크 캐시 테스트의 예산을 다시 늘리고, `cargo test`가 바이너리를 동시에 돌린다는 기록을 실측으로 뒤집었다

- 유형: `fix` + `verification`
- 계기: `#49` 작업의 품질 게이트에서 최소 기능 세트(`--no-default-features`)만 `exit=101`로 남았다.
  단일 실패는 `fleet-worker --lib`의 `registration::tests::disk_cache_get_or_schedule_refresh_populates_background`
  (`registration.rs:1048`, "disk cache should be populated").

### 예산 ≈ 비용이 30초에서 재발했다

이 테스트는 `for _ in 0..300 { … sleep(100ms) }`, 즉 **30초 예산**으로 백그라운드 수집을 기다린다.
그 예산을 정한 주석은 스스로 근거를 남겨 뒀다 — "첫 호출은 macOS에서 실측 12.9초", "예산이 10초였을
때는 비용과 예산이 거의 같아 5회 중 2회가 실패하는 결정적 flake였다".

**그 12.9초는 지금 이 머신에서 성립하지 않는다.** 격리·무부하 실측(다른 cargo 프로세스 없음):

| 군집 | 소요(초) | 결과 |
|---|---|---|
| 느린 군집 (3회) | 27.63 / 30.20 / 33.07 | 33.07초 회차만 실패(30초 예산 초과) |
| 빠른 군집 (5회) | 20.95 / 21.04 / 21.36 / 21.46 / 21.05 | 전부 통과 |

즉 10초 → 30초 상향이 실패한 것과 **같은 구조가 30초에서 반복됐다**. 비용이 예산에 근접했기 때문이지
간헐적 경합이 아니다. 부하 가설은 죽었다 — 위 측정은 전부 유휴 상태에서 단독으로 돌렸다.

**같은 머신·같은 명령이 두 군집으로 갈린다는 사실 자체가 산정 방식을 결정한다.** 분산이 이 정도면
"측정 최대치 + 조금"은 다음 군집 이동에서 다시 깨진다 — 10 → 30이 정확히 그 방식이었다. 그래서
측정 최대치의 **배수**로 잡았다: `for _ in 0..1200`, 즉 **120초 = 33.07초의 약 3.6배**. 비율을 주석에
남겨서 다음 사람이 여유가 잠식됐는지 판별할 수 있게 했다.

**첫 호출 비용을 무는 테스트가 누구인지도 확인했다.** `--test-threads=1`에서 6개 `disk_cache*` 테스트는
알파벳 순으로 돌고, 앞선 두 개(`disk_cache_begin_refresh_after_ttl_expiry`,
`disk_cache_begin_refresh_from_initial`)는 순수 in-memory `DiskCache` 조작이라 sysinfo를 건드리지 않는다.
따라서 필터 실행이든 전체 lib 스위트든 **실패 테스트가 첫 호출 비용을 전부 문다** — 전체 스위트의
실행 순서(`g3_test_min.log`)와 필터 실행 순서가 실제로 동일함을 확인했다. 하나의 예산이 양쪽을 덮는다.

이번에는 **측정 최대치 + 약간**이 아니라 **측정 최대치의 배수**로 정했다. 10 → 30의 실패가 정확히
"비용보다 조금 큰 값"을 고른 결과였기 때문이다. 값이 도착하면 즉시 `break`하므로 빠른 환경에서 이
상수는 비용이 아니다 — 예산을 키워도 통과 경로의 소요 시간은 변하지 않는다.

### 부수 발견: `cargo test`는 테스트 바이너리를 **동시에 돌리지 않는다**

2026-08-24 항목(`docs/log.md`)은 fleet-store DB 테스트의 경합 원인을 이렇게 적었다 —
"cargo가 여러 바이너리를 **동시에**(그리고 바이너리 안에서도 여러 테스트 함수를 동시에) 실행해
TRUNCATE와 INSERT가 서로 경합". 2026-08-26 항목은 이를 근거로 인용한다.

**괄호 안(바이너리 내부 병렬)은 맞고, 바깥(바이너리 간 병렬)은 틀렸다.** `cargo test --workspace`가
도는 동안 0.5초 간격으로 60초, 총 120회 표본을 뜬 결과:

```
ps -axo command | grep -cE "[t]arget/debug/deps/[a-z_0-9]+-[0-9a-f]{16}"
→ 120회 전부 정확히 1  (분포: 120 × 1)
```

동시에 실행 중인 테스트 바이너리는 **항상 하나**였다. 바이너리 간 병렬은 `cargo-nextest`의 성질이지
`cargo test`의 성질이 아니다.

이 정정이 기존 처방을 뒤집지는 않는다. `--test-threads=1`은 여전히 필요하다 — **바이너리 내부**
병렬이 실재하고, 그것이 `ci.yml`의 세 잡이 이 플래그를 거는 이유다. 다만 "바이너리마다 별도
프로세스로 격리"는 `cargo test`에 대해서는 **불필요한 부분**이다. 처방이 들었던 것은 기록된 이유
때문이 아니라 `--test-threads=1` 때문이었다.

### 그럼 내가 본 중복 PK 실패는 무엇이었나

같은 게이트 추적 중에 `fleet-store`에서 중복 키 `Conflict`가 났고, 나는 그것을 위 하네스 특성으로
읽으려 했다. 실제 원인은 **내가 띄운 게이트 체인 두 개가 겹쳐 돈 것**이었다. 파일 mtime이 증거다 —
`g2_test_min.log`(60,858B)와 `g_min_test.log`(61,770B)가 `gate_all.exit`·`g_rest.exit`과 함께
**12:41:27 같은 초에** 끝났고, 두 실행의 실패 조합이 서로 달랐다(중복 키 `Conflict` 3건 vs 2건).
같은 DB에 두 writer가 끼어들 때 나오는 서명이다. 하네스의 문서화된 특성이 아니라 **내 절차의 결함**이다.

이 때문에 준비했던 바이너리별 격리 실행 스크립트는 폐기했다. 방어하려던 경합이 존재하지 않았고,
CI가 실제로 돌리는 형태는 워크스페이스 실행이다.

### 스스로 만든 두 번째 함정: 낡은 로그를 판정으로 읽을 뻔했다

`g_rest.exit`에는 clippy `exit=101`이 두 건 있었지만, 그 로그(`g_clippy_acp.log`/`g_clippy_min.log`,
mtime 12:16:59 / 12:18:03)는 **소스 수정 이전**의 것이었다. 지적된
`clippy::needless_borrows_for_generic_args`(`crates/fleet-store/tests/agents.rs:75`)는 이미 고쳐져
디스크의 파일에는 존재하지 않는다. 유효한 판정은 `gate_all.exit`(fmt 0 / clippy-acp 0 / clippy-min 0)다.
§4.3은 "판정을 파이프 뒤에서 읽지 마라"를 이미 적고 있는데, 여기서는 파이프가 아니라 **시간**이
판정을 무효화했다. 로그의 신선도를 mtime으로 확인하지 않으면 존재하지 않는 실패를 쫓게 된다.

### 검증 한계

- **`Disks::new_with_refreshed_list()` 자체를 직접 계측하지는 않았다.** 위 숫자는 테스트가 관측하는
  경로 전체(`spawn_blocking` + 수집 + 100ms 폴링 granularity)의 비용이다. 예산이 지켜야 하는 것도
  그 경로이므로 예산 산정에는 충분하지만, "sysinfo 호출이 N초"라고 읽으면 안 된다.
- **비용이 왜 12.9초에서 20~33초로 늘었는지는 규명하지 못했다.** 마운트 수(16개, Keybase FUSE 포함)를
  의심해 `df -k`를 재 봤지만 **0초**로 끝나 이 가설을 뒷받침하지 못했다. 가설로만 남긴다.
- **120회 표본은 `cargo test --workspace` 한 번의 실행 구간에서 떴다.** 다른 cargo 버전·다른 플래그
  조합에서도 항상 1인지는 확인하지 않았다.
- 2026-08-24·08-26 항목의 문장은 **고치지 않았다**(`docs/log.md`는 append-only). 여기 실측만 남긴다.

---

## 2026-08-28 — fix — archive가 막힌 **사유**를 서버가 말하게 했다: 화면이 없는 Task를 기다리라고 안내하고 있었다 (`#49`)

### 무엇이 잘못됐나

`#49` 1단계를 커밋한 직후 실제 브라우저로 archive 게이트 두 번째 조건을 검증하다가, **방금 커밋한
코드가 사용자에게 거짓을 말하고 있는 것**을 발견했다. Task 0건 · `Ready` Agent 1건인 Project에서
archive를 누르면 화면은 이렇게 표시했다:

> Status: `draining` · Draining — tasks still running; archive completes once they finish.
> (같은 화면의 Tasks 절: **No tasks**)

두 가지가 동시에 틀렸다. (1) 막고 있는 것은 Task가 아니라 Agent다. (2) "곧 끝난다"는 약속은 Agent에
대해서는 **성립할 수 없다** — `Ready` Agent는 저절로 끝나지 않고 사람이 Stop을 눌러야 한다. 즉
사용자는 존재하지도 않는 Task가 끝나기를 무한히 기다리게 된다.

### 왜 생겼나 — 게이트에 조건을 더하면서 그 조건을 설명할 방법을 만들지 않았다

`advance_project_archive`는 두 조건을 평가한 뒤 **사유를 버리고** 상태만 반환했다:

```rust
if store.project_has_active_tasks(...).await? || store.project_has_live_agents(...).await? {
    return Ok(ArchiveProgress::Draining);   // ← 무엇이 막았는지가 여기서 사라진다
}
```

사유가 응답에 없으니 화면은 지어낼 수밖에 없었고, `#48` 시절 조건이 하나뿐일 때 하드코딩한 문장이
`#49`가 조건을 추가한 뒤에도 그대로 남았다. `ArchiveProgress::Draining`의 doc comment("비종료 Task가
아직 남아 있어")도 같은 방식으로 낡아 있었다.

이것은 `#48` 2단계가 규칙을 `project_rules`로 단일화하며 막으려 했던 바로 그 분기(divergence)다.
**규칙만 단일화하고 사유를 표면에 맡기면 단일화는 절반만 성립한다** — 게이트의 판정은 한 곳에서
나오는데 그 판정의 *의미*는 표면마다 따로 적혀 있기 때문이다.

### 어떻게 고쳤나

사유를 게이트가 말하게 했다. `ArchiveBlockers { active_tasks, live_agents }`를 만들어
`ArchiveProgress::Draining(ArchiveBlockers)`가 실어 나르고, 두 표면은 그것을 `archive_blocked_by`
(`["tasks"]` / `["agents"]` / 둘 다)로 옮기기만 한다. 라벨 어휘도 `ArchiveBlockers::labels()` 한
곳에 둔다 — 표면마다 문자열을 지으면 같은 사유가 두 이름으로 갈린다.

**클라이언트가 `agents`/`tasks` 목록을 보고 추론하는 방식은 택하지 않았다.** 그렇게 하면 게이트가
JS에 세 번째로 구현되고, `#67`/`#89`가 blocker를 추가할 때 이번 버그가 **구조적으로 재발**한다.
사유는 게이트를 평가한 쪽만 알 수 있다.

**두 조건의 `||` 단락 평가도 제거했다.** 단락시키면 Task가 막고 있을 때 Agent 조건을 묻지 않으므로,
사용자는 Task를 전부 끝낸 **뒤에야** Agent도 막고 있었다는 사실을 알게 된다. 추가 비용은 Task가
막는 경우의 질의 한 번뿐이고, 그 대신 한 번에 조치 가능한 답이 나온다.

문장도 조치별로 갈랐다 — Task는 "기다리면 끝난다", Agent는 "stop them below". 서버가 사유를 주지
않은 경우(구버전 응답)에는 사유를 **말하지 않는다**(`Draining — archive is still blocked.`) —
틀린 사유를 지어내는 것보다 낫다.

### 실측 (실제 Postgres + 실제 브라우저)

서버는 **수정된 코드로 다시 빌드해 교체한 뒤** 검증했다(낡은 바이너리로는 검증이 성립하지 않는다).

| 시나리오 | 화면 문구 | 와이어 |
|---|---|---|
| Task 0 · `ready` Agent 1 | "live agents are still assigned; archive completes once you stop them below." | `"archive_blocked_by":["agents"]` |
| Task 1(pending) · `ready` Agent 1 | "unfinished tasks and live agents are blocking archive; wait for the tasks and stop the agents below." | `["tasks","agents"]` |
| Stop 누른 뒤 재시도 | "Archived" (버튼 사라짐, Agent `stopped`) | 필드 없음 |
| archived에 재호출(idempotent) | — | `status: archived`, 필드 없음 |

두 번째 행이 단락 평가 제거의 증거다. 이 상황은 **Active일 때 Task와 Agent를 함께 둔 뒤 archive를
요청하는 경로로만** 만들 수 있다 — `draining` Project는 `ensure_project_accepts_new_tasks`가 새
Task를 거절하므로 나중에 Task를 붙일 수 없다. 게이트의 두 조건이 서로를 가리는 구조라는 것을
검증 경로 자체가 보여준다.

### 검증 한계

- **`archive_blocked_by`가 빠졌을 때의 폴백 문구는 브라우저로 확인하지 않았다.** 현재 서버는 항상
  사유를 싣기 때문에 그 분기를 실 서버로 만들 방법이 없다. 코드 경로로만 존재한다.
- **MCP 표면은 단위 테스트로만 확인했다.** Dashboard와 달리 실제 MCP 클라이언트로 `fleet_delete_project`를
  호출해 사유를 받아 보지는 않았다.
- **컴파일러가 MCP를 강제하지 못했다.** variant에 payload를 실으면 `match` 지점이 전부 에러가 되어
  두 표면이 함께 갱신될 것으로 기대했지만, MCP 핸들러는 반환값 자체를 버리고 있어서 아무 에러도
  나지 않았다. 대칭은 사람이 맞춰야 했다 — **반환값을 무시하는 호출자에게는 타입 시스템이 아무것도
  강제하지 못한다.** 지금은 두 표면 모두 단정 테스트가 있어 다음 조건 추가 때는 테스트가 잡는다.
- 이 결함은 `#49` 1단계 커밋(`295a619`) 자체에 들어 있었다. 되돌리지 않고 별도 `fix:` 커밋으로
  남긴 것은, **브라우저 실검증이 잡아낸 결함**이라는 사실이 이력에 남아야 하기 때문이다 —
  세 계층(store/API/MCP)의 테스트는 전부 초록이었고 상태만 단정했지 사유는 단정하지 않았다.

### 부수 발견 — 게이트 로그가 트리보다 이르면 초록은 판정이 아니다

커밋 직전에 게이트 로그(`exit=0` 다섯 줄)와 소스의 mtime을 나란히 보다가, 로그가 **13:34**이고
고친 소스가 **13:51~13:57**임을 발견했다. 그 초록은 고치기 *전* 트리에 대한 것이었고, 그대로
커밋했다면 검증된 적 없는 코드가 "게이트 통과"로 기록될 뻔했다. 전부 다시 돌려 현재 트리에서
확인했다(rustc 1.98.0 / fmt·clippy(acp mtls)·clippy(no-default) 전부 `exit=0`, DB 주입 직렬
`acp mtls` 70 suites 1094 passed 0 failed, `no-default-features` 70 suites 1090 passed 0 failed,
`cross_client` 14 passed 7.16s·7.95s로 조용한 skip 아님).

agent.md §4.3에 한 줄 남겼다. 그 절이 이미 모아 둔 결함군 — 플래그가 없어서, 파이프 때문에
결과를 못 읽어서 — 의 세 번째 얼굴이기 때문이다: 여기서는 결과를 읽었지만 **대상이 이미
바뀌어 있었다**.


---

## 2026-08-28 — feat — 오케스트레이터가 워커의 작업 디렉터리를 지어내고 있었다: 그 자리에 `/`가 있었다 (`#69` 전제)

### 무엇이 있었나

`AcpTransport::dispatch`에 이런 줄이 있었다.

```rust
cwd: req.cwd.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/")),
```

`Task.cwd`는 클라이언트가 채우거나 말거나인 `Option<String>`이고, MCP·Dashboard·CLI 세 표면
어디에도 요구 사항이 없었다. 비어 있으면 에이전트 세션이 **파일시스템 루트**에서 열렸다.
mount 경계가 아직 없으므로(로드맵 `#64`·`#49` 2단계) 이는 워커의 홈, `~/.ssh`, Fleet 자신의 설정,
다른 Project의 작업물이 전부 그 에이전트의 작업 디렉터리라는 뜻이다.

정본 [실행 격리](architecture/agents/execution-isolation.md)는 "Worker는 Agent별 worktree를 만들고
해당 Task 실행에는 그 경로만 mount한다. Project root, 다른 Agent worktree, Worker 설정 디렉터리,
credential backend는 mount하지 않는다"고 정해 두었다. 구현은 그 정반대의 최대값을 기본값으로
쓰고 있었다.

### 왜 `Option`이었나 — 전제 자체가 틀렸다

"보내지 않으면 워커가 알아서 하겠지"가 성립하려면 프로토콜이 생략을 허용해야 한다. 벤더된
ACP SDK를 열어 확인했다: `NewSessionRequest.cwd`는 v1에서 `pub cwd: PathBuf`, v2에서
`pub cwd: AbsolutePath`로 **양쪽 다 필수**다. 생략은 애초에 선택지가 아니었고, 그래서 코드가
값을 지어내는 것 말고 할 수 있는 일이 없었다. `Option`은 프로토콜의 사실을 반영한 적이 없다.

덤으로 `AbsolutePath`는 이름이 주는 인상과 달리 **아무 검증도 하지 않는다** —
`#[serde(transparent)]`에 `#[from(forward)]`가 붙은 `pub struct AbsolutePath(pub PathBuf)`이라
상대 경로도 그대로 통과한다. 타입 이름을 안전 장치로 읽으면 안 된다.

### 무엇을 했나

지어내는 것을 그만두고 거절한다. `fleet-core::validate_workspace_cwd`가 정본이며 어휘적으로
네 가지만 본다: 절대 경로일 것, `..` 세그먼트가 없을 것, `/` 자체가 아닐 것, interior NUL이
없을 것. 게이트는 다섯 자리에 있다.

| 자리 | 위치 | 반환 | 그 자리여야 하는 이유 |
|---|---|---|---|
| MCP | `handle_dispatch_task`, 인자 파싱 | `invalid_params` | 아래로 흘리면 핸들러의 Err 갈래가 `dispatch failed … (task_id=…)`로 **만들어진 적 없는 task_id를 인용**한다 |
| Dashboard | `submit_task_api`, 상속 뒤 | `400` | 이어가기가 부모의 `cwd`를 물려받으므로 상속 뒤여야 한다(`project_id`가 이미 같은 규칙) |
| CLI | `run_tasks_submit`, `insert_task` 앞 | `anyhow` | 이 경로는 `Dispatcher::submit()`을 **지나지 않는다** |
| Scheduler | `Dispatcher::submit()`, 저장 앞 | `InvalidRequest` | 뒤에 두면 영원히 디스패치될 수 없는 Pending 행이 남아 재조정 루프가 매 tick 같은 실패를 반복한다 |
| Transport | `AcpTransport`/`MockTransport::dispatch()`, 워커 상태 조회 앞 | `InvalidRequest` | 기존에 저장된 행과 CLI 우회 경로의 최종 관문 |

관문이 다섯인 것은 방어의 중복이 아니라 **경로가 실제로 갈라지기 때문**이다. 워커 상태보다
앞에 두는 이유는, 뒤에 두면 같은 잘못된 요청이 워커가 온라인이냐에 따라 다른 에러를 받게
되어서다.

### 회로 차단기를 오염시키지 않기 위해 variant를 새로 만들었다

이것이 이 작업에서 가장 조심한 부분이다. transport 실패를 뭉뚱그려 `Transport`로 접으면
`dispatch_existing`이 `cb.record(Outcome::Failure)`를 부른다. 그러면 **클라이언트가 잘못된 `cwd`를
반복 제출하는 것만으로 멀쩡한 워커의 회로를 열 수 있다** — 검증을 추가하면서 자해형 DoS 경로를
만드는 셈이다. `TransportError::InvalidRequest`와 `FailureKind::InvalidRequest`를 신설하고,
`From<TransportError>`가 variant를 보존하게 하고, 이 갈래만 차단기 기록과 상태 전이를 건너뛴다.
`FailureKind`를 나눈 이유는 `CredentialMissing`과 같다 — `WorkerError`로 적으면 운영자가 워커
로그를 뒤지지만, **워커는 이 요청을 본 적이 없다.**

### 대조군이 테스트를 구했다

`invalid_request_does_not_open_the_circuit_but_a_worker_fault_does`를 처음에는 실험군만으로
썼다. "잘못된 `cwd`로 dispatch → 회로가 열리지 않는다." 초록이었다. 그런데 대조군(워커 결함이면
열린다)을 붙이자 그쪽도 열리지 않았다.

원인은 관측 수단이었다. 판정을 `store.get_worker(id).circuit_state`로 읽고 있었는데,
`Store::update_worker_circuit_state`는 트레이트에 **no-op 기본 구현**이 있고
(`crates/fleet-store/src/lib.rs:363`) `MemStore`는 그것을 재정의하지 않는다. 그 필드는 MemStore
아래에서 **영원히 `Closed`**다. 실험군만 있었다면 면제 로직을 통째로 지워도 통과하는 테스트를
초록으로 받아들이고 넘어갔을 것이다. 판정을 이벤트 로그의
`FleetEvent::WorkerCircuitChanged { to: Open }`로 옮겨 고쳤다.

같은 형태의 결함을 픽스처에서도 한 건 발견했다. `reply_into_archived_project…`는 실패하지
**않았지만**, 부모에 `cwd`가 없으면 그 400은 archive 검사가 아니라 새 `cwd` 게이트가 낸 것이라
archive 검사를 지워도 초록으로 남는다. 실패한 픽스처만 고쳤다면 통과하는 거짓말이 남았을
것이다 — **상태 코드만 단정하는 테스트는 게이트를 하나 더 얹는 것만으로 조용히 무의미해진다.**

### 검증 한계

- **containment는 판정하지 못한다.** 검증은 어휘적일 뿐이고, 경로가 Fleet workspace root 아래인지는
  보지 않는다. 오케스트레이터의 `canonicalize`는 자기 파일시스템을 보므로 워커의 경로에 대해
  아무것도 말하지 않고, symlink 해석은 그 경로가 존재하는 쪽에서만 가능하다. 워커측 relay나
  `#64`의 container mount 경계가 선행이다. 그래서 `/home/other-user/secrets`처럼 규칙을 전부
  지키면서 경계 밖인 경로는 지금도 통과한다. 이 한계를
  `cwd_validation_is_lexical_only_and_does_not_contain` 테스트가 명시적으로 고정한다.
- **CLI 게이트에는 자동화된 테스트가 없다.** `fleet-cli`는 `[[bin]]`만 있는 크레이트라 통합
  테스트 타깃이 `run_tasks_submit`에 닿지 못하고, `connect_and_migrate`가 먼저 걸려 DB 없이는
  호출도 되지 않는다. `cross_client`처럼 바이너리를 subprocess로 띄우는 방식은 가능하지만,
  agent.md §4.3 (3)이 기록한 "Cargo 의존성 그래프에 잡히지 않는 런타임 참조" 위험을 하나 더
  늘리는 선택이라 하지 않았다. 대신 실제 바이너리로 수동 확인했다(아래).
- **`#69`의 완료 게이트 4종은 하나도 닫히지 않았다.** 이번 것은 그 앞의 전제다.

### 실검증

실제 Postgres(`fleet_live69`)와 실제 바이너리로 확인했다. 자동 테스트가 없는 CLI 게이트가
이 절의 주된 이유지만, 확인하는 김에 대시보드도 브라우저로 끝까지 몰아봤다.

**CLI** — `fleet tasks submit`을 5가지 잘못된 `cwd`로 호출했고 전부 서로 다른 메시지로 거절됐다:
생략(`cwd is required: the orchestrator cannot invent a working directory for the worker`),
`relative/path`·`.`(`must be an absolute path starting with '/'`),
`/srv/../etc`(`must not contain '..' path segments`),
`/`(`must not be the filesystem root '/'`). 정상 경로 1건만 성공했고, 그 뒤
`select count(*) from tasks`가 **1**이었다 — 거절된 5건은 행을 하나도 남기지 않았다.

**이 실검증이 결함 하나를 잡았다.** 처음 구현에서 CLI 오류가
`invalid --cwd for task d3ac2286-…`처럼 **저장된 적 없는 task_id를 인용**했다. `task.id`는
`insert_task` 전에 이미 만들어지지만 이 갈래는 저장하지 않고 반환하므로, 운영자가 저 UUID로
DB를 뒤지면 아무것도 나오지 않는다. MCP 핸들러에서 게이트를 인자 파싱 자리로 올린 이유가
정확히 이것이었는데 CLI에서 같은 실수를 했다. 메시지에서 task_id를 뺐다. 코드를 읽어서는
보이지 않았고 실제로 실행해서야 보였다.

**대시보드** — bootstrap → 로그인 → `/tasks/new`까지 브라우저로 진행했다.
빈 `cwd`는 HTML5 `required`가 클라이언트에서 막았고(폼이 제출되지 않고 필드가 강조됨),
`required`를 통과하는 `/srv/../etc`와 `/`는 서버가 400으로 거절하며 그 사유를 화면에
그대로 렌더링했다(`Error: bad request: cwd: cwd must not contain '..' path segments, got
"/srv/../etc"`). **두 겹이 서로를 대체하지 않는다는 것이 여기서 갈린다** — 하나만 있었으면
각각 다른 구멍이 남는다. 또한 오류 본문이 실제로 메시지를 담는다는 사실이
`submit_task_without_a_valid_cwd_is_rejected_and_creates_nothing`의 `body.contains("cwd")`
단정을 뒷받침한다(HTML 오류 페이지로 감쌌다면 그 단정이 헛돈다).

**상속** — 정상 `cwd`로 제출한 뒤 Task 상세의 Reply 폼(=`cwd` 입력이 **없는** 화면)으로
이어가기를 보냈고 통과했다. DB 확인 결과 자식 행의 `cwd`가 부모와 같은
`/srv/fleet/workspaces/browser`였다. 게이트를 상속 **앞**에 뒀다면 이 reply가 400으로
죽었을 것이고, 이어가기 기능 전체를 망가뜨린 채 "검증을 추가했다"고 기록됐을 것이다.

최종 상태는 행 3개(CLI 정상 1, 대시보드 정상 1, 이어가기 1)였다. 확인 후 서버를 내리고
`dropdb fleet_live69`로 정리했다.

---

## 2026-08-28 — docs — 같은 함정을 하루에 두 번 밟았다: 로그에 적는 것과 게이트를 고치는 것은 다르다

`#69`의 §3.2 테스트 게이트에서 `cargo test --workspace --features "acp mtls"`가 70개 스위트 중
10개, **78건 실패**로 끝났다. 전부 `fleet-store`의 DB 통합 테스트였다.

원인은 코드가 아니라 내 명령이었다. CI(`ci.yml:78`, `ci.yml:127`)는 두 잡 모두
`-- --test-threads=1`로 돌리는데 내 명령에는 그게 없었다. 이 파일들은 각 테스트가 시작할 때
`TRUNCATE ... CASCADE`로 테이블을 비우는 방식으로 격리하므로, 병렬로 돌면 한 테스트의 TRUNCATE가
다른 테스트가 방금 넣은 행을 지운다. 플래그를 붙여 다시 돌리니 70/70, 1108건 통과했다.

### 이건 오늘 오전에 이미 규명된 것이었다

이 로그의 `#49` 항목(같은 날)이 정확히 같은 현상을 다룬다. 거기서 `tests/agents.rs`가 병렬에서
10건 중 6건 실패했고, 15개 파일이 이 관례를 쓴다는 것과 CI가 그것을 강제한다는 것까지 확인해
적어 뒀다. 결론 문장도 이미 있었다 — "게이트와 CI가 다르면, 어느 쪽이 엄격하든 로컬의 판정은
CI의 판정이 아니다."

**그런데 그 기록이 `agent.md`의 게이트 목록으로 옮겨지지 않았다.** 그래서 몇 시간 뒤 같은 함정을
다시 밟았다. 재발의 원인은 새로운 무지가 아니라 **서술만 갱신하고 실행 목록을 갱신하지 않은
것**이다. `docs/log.md`는 무슨 일이 있었는지를 남기고 `agent.md`는 다음에 무엇을 실행할지를
정한다. 다음 세션의 에이전트는 로그 3000줄이 아니라 게이트 목록을 보고 명령을 만든다.

§4.3이 기록해 온 드리프트 사례가 전부 "로컬 게이트에 무언가가 **없어서**"였던 것과 이번은
결이 다르다. 이번엔 알고 있었고 적어 두기까지 했다. 지식의 문제가 아니라 **지식이 실행되는
자리에 놓이지 않은** 문제다.

### 방향도 정정한다

처음에는 "로컬 게이트가 CI보다 약했다"고 적으려 했는데 틀렸다. 병렬 실행은 직렬보다 **엄격한**
조건이다. 그런데도 결과는 같다 — 판정이 CI의 판정이 아니게 된다. §4.3의 재발 법칙을 "약하면
재발한다"로만 읽으면 이 방향은 걸러지지 않으므로, `agent.md`에 일반형으로 다시 적었다.

### 원인을 무엇으로 가렸는지

실패 목록만 보고 코드 결함으로 판정하지 않은 근거는 **실패의 모양**이다. 코드 결함이면 특정
단정이 일관되게 깨지지만, 여기서는 `left`가 매번 다른 값(9·11·13·2)으로 나왔고
`audit_log_actor_user_id_fkey` 위반처럼 그 테스트가 시험하려던 것과 무관한 오류가 섞여 있었다.
그다음에 `ps`와 `pg_stat_activity`로 잔여 프로세스·연결이 0인지 확인하고 단독으로 재실행했다.

### `agent.md` 변경

§3.2에 bullet을 추가해 `-- --test-threads=1`을 **명령 형태 그대로** 적었다. 함께 적은 것:

- 위 bullet 2의 "직렬 가동"이 크레이트 단위가 아니라 **한 바이너리 안에서의** 직렬성이라는 것.
  이 모호함이 내가 플래그를 빠뜨린 직접 원인이다.
- `--no-fail-fast` 권장. `cargo test`는 기본이 fail-fast라 실패 타깃 뒤를 **한 건도 실행하지
  않는다** — `#49` 때 24번째에서 멈춰 25번째가 통째로 보이지 않았다. §4.3 (3)의 조용한 skip은
  `finished in 0.00s`라도 찍히지만, 이쪽은 출력 자체가 없어서 소요 시간으로도 잡히지 않는다.

### 검증 한계

- **78건 실패를 재현하지 않았다.** 플래그를 빼면 깨진다는 것은 `#49` 항목이 반복 실측
  (병렬 3회 / 직렬 1회)으로 이미 고정했고, 이번 관측은 그것과 같은 모양이라 원인 규명은
  거기에 기댄다. 이번 세션에서 따로 A/B를 돌리지는 않았다.
- **동시 실행이라는 두 번째 오염원은 분리하지 못했다.** 실패한 실행 시점에 다른 백그라운드
  `cargo test`가 같은 `fleet_test` DB에 붙어 있었다. 플래그 없는 단독 실행만으로도 깨진다는 것은
  `#49`의 실측이 보장하지만, 이번 78이라는 **숫자**에 동시 실행이 얼마나 기여했는지는 알 수 없다.
- **나머지 14개 파일의 TRUNCATE 관례는 그대로다.** `#49`가 `agents.rs`만 UUID 격리로 바꾸며
  남긴 유예이고, 이 커밋도 손대지 않는다.

---

## 2026-08-28 — feat/test — 세 파일이 각자 옳았는데 합치니 죽은 워커가 영구 dispatch 대상이었다 (`#70` 게이트 1·5)

`#70`의 구현 게이트 7개 중 **5개는 시험을 작성할 수조차 없다** — Reconciler, `worker_execution_lease`,
effect ledger, `CancelUnconfirmed`가 코드에 아예 없다(`EffectLedger|PartiallyApplied|OutcomeUnknown|
CancelUnconfirmed` grep 결과는 `lease.rs:44`의 주석 한 줄뿐). 그래서 이번에는 하부 구조에 막히지
않은 두 개만 다뤘고, 무엇이 무엇에 막혀 있는지를 설계 정본의 "구현 상태" 표로 남겼다.

### 게이트 5 — 결함은 어느 파일에도 없고 세 파일 사이에 있었다

- `crates/fleet-api/src/handlers.rs`의 `build_worker`는 `liveness_mode`와 무관하게
  `WorkerStatus::Online`을 기록한다. (등록됐으니 Online — 타당하다.)
- `crates/fleet-scheduler/src/health.rs`는 `on_demand` 워커를 **의도적으로 강등하지 않는다**.
  heartbeat을 보내지 않는 모드이니 누락으로 죽었다고 판단할 수 없다. (타당하다.)
- `crates/fleet-scheduler/src/selector.rs`에는 liveness 조건이 **하나도 없었다**
  (`grep -n liveness` 0건). Online이면 후보다. (타당하다.)

셋을 합치면 "생존 여부를 확인할 수단이 없는 워커가 한 번 등록되면 영구히 dispatch 대상"이 된다.
`worker.rs`의 `WorkerLivenessMode::OnDemand` 문서도, `handlers.rs`가 렌더링하는 worker.toml의
"아직 프로덕션에서 쓰지 말 것" 경고도 이 배정이 범위 밖이라고 **적어 왔지만**, 강제하는 코드는
없었다. 문서에 적힌 제약과 코드가 집행하는 제약이 갈라져 있었다.

`select()` 1.5단계에 필터를 넣고 `SelectionError::AllUnprobed`를 추가했다. 세 가지 판단:

- **라벨/모델 필터보다 먼저 건다.** liveness는 "이 작업에 적합한가" 이전에 "애초에 배정 가능한
  대상인가"의 문제다. 뒤로 미루면 on_demand 워커가 라벨도 안 맞을 때 운영자에게 "라벨 불일치"로
  보고된다 — 원인이 잘못 보고되는 것을 시험으로 고정했다.
- **`FailureKind`를 늘리지 않았다.** dispatcher의 기존 `_ => WorkerUnavailable` 기본 arm에 그대로
  떨어지므로 `fleet_tasks_failed_total{kind}`의 라벨 집합이 변하지 않는다. 새 variant를 만들면
  그 자체가 metric 표면 변경이 되고, 게이트 1에서 방금 고정한 허용 목록을 같은 커밋에서 넓히게
  된다.
- **`Unchecked` 워커 상태는 만들지 않았다.** 설계 문서가 그 이름을 쓰지만, ACP probe(`#67` 의존)가
  없는 한 그 상태에서 빠져나올 방법이 없다 — 도달했다가 영영 못 나오는 상태다. 상태를 늘리는
  대신 이미 있는 `liveness_mode`를 판단 근거로 썼다. 이것은 영구 규칙이 아니라 **상태 기계의 안전한
  절반**이고, probe가 들어오면 "probe 성공한 워커는 후보에 포함"으로 바뀐다.

### 게이트 1 — 지키는 코드는 있었고, 지키게 하는 것이 없었다

`/metrics`가 노출하는 라벨은 전부 정적이라(`status`/`phase`/`kind`/`type`/`le`) 금지 항목을 위반한
적이 없다. 위반할 **기회**가 없었을 뿐, 위반을 막는 것은 없었다. 두 겹으로 고정했다.

1. fixture(worker/task UUID, prompt, 리포지터리 URL, `?server-key=` secret, `created_by`)가 본문에
   나타나지 않는지.
2. 렌더링된 **모든 라벨 이름과 값이 유한 허용 목록 안**인지.

두 번째가 왜 따로 필요한지를 변이로 확인했다. `HttpMetrics::render`에 `path="/v1/healthz"` 라벨을
심자 — fixture의 어떤 값과도 겹치지 않으므로 — **1번은 통과하고 2번만 실패했다**. 부분문자열
검사는 "지금 저장소에 있는 값"만 볼 수 있고, 나중에 추가될 라벨은 원리적으로 못 본다.

이 변이가 `HttpMetrics::render` 안에 있었다는 것이 두 번째 발견이다. `metrics_handler`는
`metrics_text(store)`의 결과에 `http_metrics.render()`를 이어 붙여 응답을 만드는데, 시험이
`metrics_text`만 불렀다면 **저 변이를 한 건도 잡지 못했다**. 하필 그 절반이 고카디널리티 라벨
(`path="/api/tasks/<uuid>"` 같은)이 붙을 가능성이 가장 높은 자리다. 조립 부분을
`metrics_body(store, http)`로 추출해 핸들러와 시험이 **같은 함수**를 부르게 했다 — 시험이 조립
방식을 따라 적으면 언젠가 갈라진다.

### 통과하는 테스트는 아직 판정이 아니다

네 개의 새 selector 시험은 필터를 지우면 4건 전부 깨지고, 두 개의 metric 시험은 위 두 변이에
각각 반응한다(변이 1은 둘 다, 변이 2는 허용 목록만). 이 저장소가 §3.2와 §4.3에서 반복해 적어 온
것과 같은 형태다 — 초록은 그 자체로 무엇도 증명하지 않고, **깨뜨려 봐야 판정이 된다.**

### 검증 한계

- **게이트 5의 E2E는 없다.** selector 단위 시험(mock Store)까지이고, 실제 on_demand 워커를 등록해
  dispatch가 거절되는 것을 실제 Postgres·HTTP로 확인하지는 않았다.
- **게이트 1은 HTTP로 스크레이프하지 않았다.** 핸들러가 부르는 것과 같은 `metrics_body`를 직접
  호출한다. 인증·라우팅 계층이 본문을 바꾸지 않는다는 것은 코드로 확인했을 뿐 실측이 아니다.
- **500 경로는 손대지 않았다.** `metrics_handler`의 오류 응답은 `format!("metrics error: {e}")`인데,
  `MetricsError`가 감싸는 sqlx 오류에는 쿼리문이 들어갈 수 있다. 게이트 1의 표는 정상 본문의
  노출을 다루므로 이번 범위에 넣지 않았고, 별도로 판단할 문제로 남긴다.
- **게이트 5는 절반이다.** "probe 성공 후 dispatch 허용"은 ACP probe가 없어 미구현이고, 그동안
  `on_demand` 워커는 등록은 되지만 어떤 작업도 받지 못한다. 이 모드를 실제로 쓰려던 운영자에게는
  기능 후퇴로 보일 수 있다 — 다만 그 전에도 그 워커에 간 작업은 생존 확인 없이 보낸 것이었다.

---

## 2026-08-29 — feat — 같은 이름으로 재시작한 워커의 고아는 시간이 지나도 회수되지 않았다 (`#67` 2단계)

`#38`이 만든 reconciler는 `Dispatched` Task를 두 경우에 회수한다. (a) 배정된 worker row가 없거나,
(b) 그 워커가 300초 넘게 `Offline`이거나. 두 분기 모두 "시간이 지나면 언젠가 걸린다"는 모양이라
창이 유한해 보였다. 그런데 `register_worker`는 `--name`이 같으면 **기존 `worker_id`를 재사용**한다.
그래서 같은 이름으로 재시작한 워커는 두 경우 **어느 쪽에도 걸리지 않는다**:

- row는 그대로 남아 있으므로 (a)가 아니다.
- 재시작 직후 새 하트비트와 함께 `Online`으로 재등록되므로 300초 `Offline`이 성립할 일이 없어
  (b)도 아니다.

그 프로세스가 실행 중이던 Task는 **영원히** `Dispatched`로 남는다. 이 창은 시간으로 닫히지
않는다 — 그리고 `has_capacity()`가 워커의 자기보고 `active_tasks`를 그대로 믿는 구조라, 고아가
쌓일수록 그 워커의 가용 슬롯 계산도 함께 썩는다.

### 정본은 워커가 자기 세대를 보고하게 그렸지만, 관측이 더 강하다

[Agent lifecycle 상태 기계](assets/diagrams/architecture/agent-lifecycle-state-machine.mermaid)는 재시작
감지를 하트비트가 싣고 오는 `process_incarnation`으로 그린다. 이번에는 그렇게 하지 않았다.

`register_with_retry`는 프로세스 기동당 정확히 1회 호출되고(`crates/fleet-worker/src/runner.rs:75`,
문서 주석도 `├── register_with_retry (1회)`), `#78`이 종료 시 deregister를 **의도적으로** 없앴다.
두 사실을 합치면, **이미 존재하는 row에 register가 다시 오는 것**은 "그 워커 프로세스가
재시작했다"는 뜻 외에 다른 해석이 없다. 즉 오케스트레이터는 워커에게 묻지 않고도 같은 신호를
얻는다. 그리고 그쪽이 엄격히 강하다 — **워커의 시계도, 워커가 고른 값도 술어에 들어오지 않는다.**

### 카운터가 아니라 타임스탬프인 이유

비교 상대가 이미 존재한다. `tasks.dispatched_at`이고, 그 값은 DB의 `NOW()`로 찍힌다
(`crates/fleet-store/src/postgres.rs:440`). 새 컬럼도 DB가 찍게 하면 **술어의 양쪽이 같은 Postgres
시계**를 쓴다 — 오케스트레이터가 여럿이어도 시계 오차가 판정에 끼어들 자리가 없다. 카운터로
했다면 `tasks` 쪽에도 세대를 실어야 했고, 그건 스키마 변경이 한 번 더 붙는다는 뜻이다.

### backfill 방향이 이 마이그레이션의 위험한 부분이었다

`incarnation_started_at`을 `NOW()`로 채우면, 업그레이드하는 **그 순간** 모든 진행 중 Task가
`dispatched_at < incarnation_started_at`을 만족한다 — 즉 정상 실행 중인 작업을 일제히 고아로
죽인다. `registered_at`으로 채우면 반대가 된다. 같은 컬럼, 같은 한 줄인데 방향에 따라 무해와
전면 장애가 갈린다.

**그리고 이 주장은 실행된 적 없는 주장이었다.** `fleet_test`는 마이그레이션 시점에 `workers`가
비어 있으므로 `UPDATE ... WHERE incarnation_started_at IS NULL`이 0행을 건드리고 `SET NOT NULL`이
공짜로 성공한다 — 테스트가 전부 초록이어도 백필은 **한 번도 실행되지 않는다**. 그래서 별도 DB
(`fleet_backfill_028`)를 만들어 027까지 적용하고, 10일 전 등록된 워커와 5분 전 디스패치되어
진행 중인 Task, 그리고 `dispatched_at`이 NULL인 012 이전 행을 심은 뒤 028을 적용해 실측했다:

| 행 | `dispatched_at`(유효) | `incarnation_started_at` | 회수되는가 |
|---|---|---|---|
| 진행 중 Task | 5분 전 | 10일 전 | **아니오** |
| 012 이전 행 | 400일 전 | 10일 전 | 예 |

첫 줄이 이 마이그레이션이 안전하다는 관측이다(`NOW()` 백필이었다면 반드시 "예"가 된다).

**동시에 "오탐이 구조적으로 불가능"이라는 원래 표현이 과했다는 것도 이 실측이 보여 준다.**
둘째 줄은 회수되는데 그것은 오탐이 아니다 — `registered_at`은 *현재 row가 만들어진 시각*이고,
그보다 앞선 dispatch가 존재한다는 것은 그 사이에 row가 지워졌다(`#78` 이전의 deregister 경로)는
뜻이므로 그 Task는 실제로 고아다. 정확한 명제는 조건부다: **현재 row가 살아 있는 동안 디스패치된
것은 반드시 `registered_at` 이후다.** 무조건형과 조건부형은 같은 마이그레이션을 정당화하지만,
무조건형은 row 재생성이라는 반례를 못 보게 만든다.

### 신호는 왜 지속 컬럼이어야 했나

회수를 register 핸들러가 그 자리에서 하면 컬럼이 필요 없다. 그런데 `AppState`
(`crates/fleet-api/src/app.rs:71`)에는 dispatcher가 없다 — 핸들러는 상태 전이를 만들 수단이 없다.
그래서 회수는 reconciler가 해야 하고, reconciler가 나중에 읽으려면 **사실이 DB에 남아 있어야
한다**. `registered_at`을 재활용하는 선택지는 닫혀 있었다: 그 값은 `selector.rs:362`의 tie-break와
목록의 `ORDER BY registered_at DESC`가 이미 하중을 걸고 있고, 핸들러가 재등록 때 보존하는 것도
의도된 동작이다.

### 순서가 정하는 것들

- **bump는 upsert 앞에.** 재시작했다는 사실은 이후 upsert의 성패와 무관하게 참이다. 뒤에 두면
  upsert가 실패했을 때 사실이 기록되지 않은 채 워커는 재시도로 다시 `Online`이 되고, 고아는 다시
  영구화된다.
- **`ON CONFLICT DO UPDATE`는 이 컬럼을 제외.** 하트비트도 같은 upsert 경로를 타므로, 포함시키면
  하트비트마다 세대가 흔들려 술어가 매번 참이 된다.
- **새 분기는 `Offline` 분기 앞에.** 재시작은 현재 상태와 무관하게 되돌릴 수 없는 사실이다. 뒤에
  두면 다시 `Online`이 된 워커의 고아가 `Some(_) => continue`로 빠져나간다.

### 함께 드러난 인접 결함 2건

1. **`mem.rs::upsert_worker`가 Postgres와 갈려 있었다.** 무조건 `insert`라 `registered_at`을
   덮어썼는데, Postgres의 `ON CONFLICT DO UPDATE`는 그 컬럼을 건드리지 않는다. 파일 머리말이
   주장하는 패리티를 어긴 상태였고, 그대로 두면 **같은 균열이 회수 판정을 백엔드별로 다르게
   만든다** — MemStore 기반 테스트가 초록인데 Postgres에서 다르게 도는 형태다.
2. **reconcile 픽스처가 물리적으로 불가능한 상태를 담고 있었다.** `Worker::new`는 "지금"을 찍는데
   픽스처 Task는 120초 전에 dispatch된 것으로 되어 있었다 — 아직 존재하지 않던 워커에 배정된
   Task다. 아무도 그 관계를 읽지 않는 동안은 무해했고, 새 분기가 **바로 그 관계**를 읽기
   시작하면서 드러났다.

### 새 트레이트 메서드는 mock까지 깨뜨린다

`Store`에 메서드를 하나 추가하니 `crates/fleet-store/src/` 밖의 `impl Store for` 4곳이 전부
컴파일 에러가 됐다. 이 4건 중 3건은 통합 테스트라 `--all-targets` 없이는 clippy에 잡히지 않는다.

여기서 **한 번 틀렸고, 그 틀림의 모양이 기록할 값이 있다.** 처음에 `bootstrap_tokens.rs`의
`BsStore`만은 동작하는 구현이 필요하다고 보고 12줄짜리 mock을 넣었다. 근거는
"`join_with_duplicate_name_returns_409`가 기존 이름으로 register를 태우므로 bump 경로를 밟는다"였다.
확인해 보니 **둘 다 사실이 아니었다** — 이 파일에는 `/v1/workers/register`를 때리는 테스트가 한 건도
없고(`grep`이 0건), `join_worker`는 중복 이름을 `enroll_worker` 앞에서 409로 조기 거부하므로 애초에
bump를 호출하지 않는다. 즉 그 mock 본문은 **도달 불가능한 코드**였고, 주석은 그 도달 가능성을
사후에 정당화한 문장이었다. 네 곳 모두 `unimplemented!()`로 되돌렸다.

교훈은 "mock의 동작 필요 여부는 트레이트가 아니라 테스트가 지나는 경로가 정한다"에서 한 걸음 더
들어간다: **경로를 읽지 않고 추정하면 그 추정이 코드가 되고, 코드는 주석으로 자기를 정당화한다.**
`grep`으로 확인하는 데 든 비용은 명령 두 개였다. 그리고 이 결함은 §"검증 한계"가 아니라 이 절에
남는다 — 한계는 "확인하지 않았음을 아는 것"이고, 이것은 "확인했다고 잘못 믿은 것"이다.

### 적용된 마이그레이션은 주석 한 줄도 못 고친다

위의 "구조적으로 불가능" 표현을 028의 **주석에서** 고친 뒤 테스트를 다시 돌리니 71개 suite 중
17개가 깨졌다:

```
migration 28 was previously applied but has been modified
```

sqlx는 적용 시점에 마이그레이션 파일의 **체크섬**을 `_sqlx_migrations`에 박아 두고, 이후 파일이
한 바이트라도 달라지면 거부한다. 체크섬은 SQL 문법이 아니라 파일 전체를 덮으므로 **주석 수정도
스키마 수정과 똑같이 무효화한다.** 여기서 파일을 고칠 수 있었던 유일한 이유는 028이 아직
커밋되지 않아 어떤 배포에도 적용된 적이 없다는 것뿐이다 — 커밋 이후였다면 오타 하나를 고치는
것도 운영 DB에서 이 에러를 만들고, 정정의 형태는 파일 수정이 아니라 **029 신설**이어야 한다.
로컬 대응은 `dropdb --force fleet_test && createdb fleet_test`이고, 그 김에 028이 빈 DB에서
깨끗하게 적용되는 것도 함께 확인된다.

**그리고 실패의 모양이 원인을 가렸다.** 깨진 17개에는 `bump_*`뿐 아니라 `audit_*`, `cas_*`,
`blank_lines_ignored`처럼 이번 변경과 아무 관계 없는 것들이 섞여 있었다 — §3.2가 적어 둔
"코드 결함이면 특정 단정이 일관되게 깨지지만, 환경 문제면 의도하지도 않은 오류가 섞인다"의
전형이다. 실패 목록의 **폭**을 먼저 보면 개별 단정을 파고들기 전에 환경을 의심하게 된다.

### 검증 한계

- **두 오케스트레이터 라이브 실행은 하지 않았다.** "양쪽이 같은 Postgres 시계를 쓴다"는 논증은
  코드 대조 수준이며, 실제로 두 프로세스를 띄워 관측한 것이 아니다.
- **실제 워커 재시작 E2E는 없다.** 재시작 신호는 테스트에서 재구성한 것이다 — 실 워커 프로세스를
  죽였다 살려 고아가 회수되는 것을 끝에서 끝까지 본 적은 없다.
- **`restart_check_prefers_dispatched_at_over_started_at`은 `dispatched_at`을 손으로 심는다.** 실제
  dispatch가 그 컬럼을 채우는 경로는 이 테스트가 확인하지 않는다.
- **`has_capacity()`는 여전히 자기보고 `active_tasks`를 믿는다.** 이번 변경은 그 값을 썩게 만들던
  무한 창을 유한하게 만들 뿐, 권한 역전 자체를 되돌리지는 않았다.
- **backfill 실측은 손으로 심은 행을 쓴 별도 DB에서 했다.** 실제로 돌고 있는 배포를 업그레이드해
  본 것이 아니다 — 관측한 것은 술어의 방향이지 운영 중 마이그레이션의 소요 시간이나 락 영향이
  아니다.

## 2026-08-29 — fix — macOS에서만 통과하는 테스트를 짰다: 나노초 잔여가 0이라 절단이 보이지 않았다 (`#67` 2단계 후속)

`9aa8571`을 푸시하고 CI가 네 잡 중 세 잡에서 실패했다. 로컬 게이트는 다섯 줄 전부 `exit=0`이었고,
`--features "acp mtls"`와 `--no-default-features` 양쪽에서 71개 suite / `test result: FAILED` 0건이었다.
CI가 깬 것은 이번에 새로 넣은 `crates/fleet-store/tests/worker_incarnation.rs` **하나뿐**이고, 4건 중
3건이 같은 모양으로 죽었다:

```
assertion `left == right` failed: bump는 최초 등록 시각을 건드리면 안 된다
  left:  2026-08-28T17:43:06.212633Z      ← Postgres 왕복
 right:  2026-08-28T17:43:06.212633416Z   ← Rust Utc::now()
```

Postgres `timestamptz`는 **마이크로초 해상도**라 왕복하면 나노초가 잘린다. 그런데 macOS의
`CLOCK_REALTIME`은 나노초 잔여가 항상 0이다(실측: 10회 연속 `% 1000 == 0`). **잘릴 것이 없으니
로컬에서는 왕복 절단이라는 현상 자체가 일어나지 않았다.** 진짜 나노초를 주는 Linux CI에서만
드러난다.

깨진 단정 3건은 전부 **메모리 값과 왕복 값을 맞대는** 자리였다. 반면 `stored.incarnation_started_at
== first`처럼 **양쪽 다 왕복한** 비교는 멀쩡했다 — 절단이 대칭이기 때문이다. 진단은 정확히 이
경계에서 갈린다.

### 게이트가 약해서가 아니라 플랫폼이 달라서

§4.3은 "게이트와 CI가 다르면 어느 쪽이 엄격하든 로컬의 판정은 CI의 판정이 아니다"를 세 번
기록해 왔다 — (1) 목록이 짧아서, (2) 툴체인 버전이 달라서, (3) 런타임 전제가 어디에도 안 적혀서.
이번은 **네 번째 얼굴**이다: 명령도 같고 툴체인도 같고 전제도 갖췄는데, **커널의 시계 해상도가
달랐다.** 명령을 더 붙여서 막을 수 있는 종류가 아니다.

막을 수 있는 자리는 게이트가 아니라 테스트다. `seed()`가 나노초 잔여를 **일부러** 심도록 고쳤다:

```rust
w.registered_at += chrono::Duration::nanoseconds(416);
```

이 한 줄로 테스트는 어느 플랫폼에서든 같은 경로를 밟는다. 실제로 이 줄을 넣은 뒤 헬퍼를 원래
`assert_eq!`로 되돌리자 macOS에서 CI와 **완전히 같은 모양**으로 재현됐다
(`...445191Z` vs `...445191416Z`). 즉 이 수정은 기전 추론이 아니라 **RED/GREEN으로 검증**됐다 —
`cross_client`(§3.3 기록 1)가 재현 없이 기전 실측으로만 정당화됐던 것과 대비된다.

비교 자체는 `assert_same_instant()`가 양변을 `timestamp_micros()`로 내려서 한다. 양쪽 다 왕복한
비교에는 쓰지 않는다 — 거기서 해상도를 낮추면 단정만 약해지고 얻는 것이 없다.

### 프로덕션 판정에는 영향이 없다

회수 술어 `dispatched_at < incarnation_started_at`은 두 값이 **같은 store에서** 오므로 해상도가
항상 일치한다. 그리고 같은 마이크로초에 일어난 디스패치는 `<`가 거짓이 되어 고아로 판정되지
**않는다** — 살아 있는 태스크를 잘못 회수하는 쪽이 아니라 회수를 한 틱 미루는 쪽으로 틀린다.
절단은 fail-safe 방향이다.

### 검증 한계

- **Linux에서 직접 돌려 보지는 않았다.** 재현은 macOS에 나노초를 손으로 심어서 만든 것이고,
  수정이 Linux에서 통과한다는 것은 다음 CI 실행으로만 확인된다.
- **416ns라는 값은 CI 로그에서 관측된 잔여를 그대로 가져온 것**이고, 특정 잔여값에서만 통과하는
  경우를 배제하려면 잔여를 무작위로 돌려야 하지만 그렇게 하지 않았다. 절단은 잔여 크기와
  무관한 연산이므로 한 값으로 충분하다고 판단했다.
- **다른 테스트 파일 조사는 정적 읽기였고, Linux에서 돌려 본 것이 아니다.** `crates/*/tests`
  전체에서 시각 필드를 건드리는 단정 19곳을 뽑아 읽었다. 16곳은 `is_some`/`is_none`이거나
  허용 오차를 두고 있어 해상도와 무관하고, `task_cas.rs:284`와 `dashboard_api.rs:1826`은
  **양쪽 다 왕복한 값**이라 절단이 대칭이다. 메모리 값과 저장된 값을 섞는 곳은
  `projects.rs:218` 한 곳뿐인데, `update_project_status`가 `updated_at`을 새로 찍으므로
  `>=` 부등호에 create·update 왕복 시간만큼(밀리초 규모) 여유가 있어 999ns 절단이 이를
  뒤집을 수 없다. **다만 이것은 구조가 아니라 여유 폭으로 얻은 안전이다** — 저 줄을
  `assert_eq!`로 조이는 순간 Linux에서 깨진다.

---

## 2026-08-29 — fix — 워커 상세의 "최근 태스크"는 Postgres에서 **언제나 비어 있었다**: SQL이 다른 직렬화 형식을 보고 있었다

`#67`의 다음 게이트(자기보고 `active_tasks`를 store 파생 카운트로 교체)를 스코핑하면서
"태스크를 워커별로 어떻게 세는가"를 확인하러 `PgStore::list_tasks`를 읽다가 발견했다.
`#67` 작업 자체는 아직 시작하지 않았다.

### 무엇이 틀렸나

`WORKER_WHERE` 상수는 externally-tagged JSONB를 전제하고 있었다:

```sql
(status->'Dispatched'->>'worker_id' = $1
 OR status->'Completed'->>'worker_id' = $1
 OR status->'Failed'->>'worker_id' = $1)
```

그런데 `TaskStatus`는 `#[serde(tag = "phase", rename_all = "snake_case")]` —
**internally-tagged**다. 페이로드는 중첩 객체가 아니라 최상위로 평탄화된다. 여섯 variant를
전부 직렬화해서 실측했다:

```
{"phase":"pending"}
{"phase":"dispatched","worker_id":"...","started_at":"..."}
{"phase":"completed","output":"o","exit_code":0,...,"worker_id":"...","finished_at":"..."}
{"phase":"failed","error":"e","kind":"worker_unavailable","worker_id":"...","attempts":0}
{"phase":"failed","error":"e","kind":"worker_unavailable","attempts":0}
{"phase":"cancelled","reason":"r","cancelled_at":"..."}
```

`status->'Dispatched'`는 존재하지 않는 키라 항상 NULL이고, `NULL = $1`은 참이 되지 않는다.
즉 **`list_tasks(worker_id: Some(..))`는 조건과 무관하게 0행**이었다. 추론이 아니라
직렬화 출력으로 확정했다 — 저장된 행으로 확인하려 했으나 테스트 DB가 비어 있어 증거가
되지 못했고, serde를 직접 왕복시켰다.

주석이 틀린 것이 아니라 **주석과 코드가 같은 오해를 공유**하고 있었다. 코드만 보고는
어느 쪽이 사실인지 알 수 없어서, 판정은 코드도 주석도 아닌 직렬화 실측에서 나와야 했다.

### 두 번째 결함 — 자리 번호가 상수 안에 박혀 있었다

회귀 테스트를 붙이자 첫 단정은 통과하는데 다음 단정에서 다른 오류가 나왔다:

```
argument of LIMIT must be type bigint, not type text  (SQLSTATE 42804)
```

`WORKER_WHERE`가 `$1`을 하드코딩하는데, `created_by`와 함께 거는 갈래에서는 `created_by`도
`$1`을 쓴다. sqlx의 `bind`는 위치 기반이라 bind 넷이 `$1..$4`로 가는데 쿼리에는 `$3`까지만
있어 `wid`(text)가 그대로 `LIMIT`에 꽂혔다. 자리 번호를 상수에서 빼내 갈래마다 직접 쓰게
고쳤다.

**두 결함의 증상이 정반대다.** 워커만 거는 갈래는 조용히 빈 목록(사용자는 "이 워커는 아직
아무것도 안 했구나"로 읽는다), 둘 다 거는 갈래는 500 에러. 조용한 쪽이 오래 살아남은 이유가
바로 그것이다 — 시끄러운 쪽은 프로덕션에서 호출자가 없어서 살아남았다.

### 왜 아무 테스트도 잡지 못했나

저장소 전체에서 `TaskFilter.worker_id`에 `Some(..)`을 넣는 테스트가 **한 건도 없었다**.
그리고 `MemStore`는 같은 필터를 Rust `match`로 올바르게 구현한다(`mem.rs:398-411`) —
`Dispatched`/`Completed`/`Failed(Some)` 세 위상을 `created_by`와 AND로 묶는 것까지 Pg와
같다. 이 필터를 쓰는 유일한 프로덕션 소비자는 대시보드 워커 상세의 "최근 태스크"
목록(`fleet-dashboard/src/handlers.rs:877`)인데, 대시보드 테스트는 MemStore를 쓴다.

즉 **저장 표현을 검증하는 단정이 MemStore 쪽에만 있으면 그것은 검증이 아니다.** MemStore는
JSONB를 거치지 않으므로 직렬화 형식에 대해 아무것도 말하지 않는다. 로드맵이 `upsert_worker`
에서 기록한 mem/pg 드리프트와 같은 계열이고, 이번 것은 그중에서도 **양쪽 다 통과하는데
프로덕션만 틀린** 모양이다.

새 테스트 `task_list_filters_by_worker_id`는 여섯 위상을 전부 넣고 세 개만 매치하는지,
`created_by`와 AND가 되는지, 무관한 워커는 빈 목록인지를 Postgres에 대고 확인한다.
RED를 먼저 봤다 — 수정 전 `left: []`.

### 적용된 마이그레이션 파일은 주석조차 고치면 안 된다

`026_task_dispatch_control_epoch.sql:10`과 `docs/reviews/task-retry-policy-decision-2026-08-26.md`,
`CHANGELOG.md:23`이 같은 잘못된 경로를 인용하고 있다. 이 중 `026`을 고치려다 멈췄다 —
`sqlx::migrate!`는 파일 **전체 바이트**의 체크섬을 `_sqlx_migrations`에 저장하고, SQL 파서를
거치지 않으므로 주석과 공백도 체크섬에 들어간다. 실측했다: 임시 DB에 마이그레이션을 적용한
뒤 `026`의 주석 한 줄만 바꾸고 다시 `migrate()`를 부르면

```
migration error: migration 26 was previously applied but has been modified
```

로 기동이 거부된다. 그대로 커밋했다면 배포된 DB가 전부 기동 불가가 됐을 것이다.
`docs/reviews/`와 `CHANGELOG.md`는 그 시점의 판단을 보존하는 역사 기록이라 손대지 않았다.
정정은 `postgres.rs`의 주석에 남겼다.

### 인덱스를 추가하지 않은 이유

`002_indexes.sql`은 `status_phase`만 인덱싱하고 status 페이로드 필드는 다루지 않는다.
`status->>'worker_id'`에는 인덱스가 없어 seq scan이 된다. 유일한 호출자가 `limit: 20`의
워커 상세 페이지라 GIN 인덱스는 지금 필요하지 않다고 보고 추가하지 않았다. `#67`의
store 파생 카운트는 `status_phase = 'dispatched'`로 좁힌 뒤 집계하므로 `idx_tasks_phase`를
탄다 — 그쪽도 새 인덱스를 요구하지 않는다.

### 검증 한계

- **`worker_id` 값이 UUID 문자열로 저장된다는 것에 의존한다.** `->>`가 text를 주므로
  `WorkerId`의 직렬화 표현이 바뀌면 조용히 다시 깨진다. 새 테스트가 이를 잡지만,
  그것은 테스트가 있을 때의 이야기다.
- **`Completed`/`Failed`의 페이로드 필드 이름이 `worker_id`로 유지되는 것에 의존한다.**
  세 구조체가 우연히 같은 이름을 쓰고 있어 술어 하나로 묶이는 것이고, 이 등가성은
  타입 시스템이 아니라 새 테스트가 지킨다.
- **`created_by` 단독 갈래와 무필터 갈래는 이번 변경으로 바뀌지 않았고, 기존 테스트가
  그대로 덮는다.**

### 브라우저 실검증 — 같은 DB, 같은 데이터에서 0행 → 3행

임시 Postgres(`fleet_wd_test`)에 워커 1개와 태스크 5개를 심고 대시보드를 띄워
bootstrap → 로그인 → `/workers/{id}`까지 실제로 진행했다. 심은 5개는 의도적으로
**매칭 3 / 비매칭 2**로 구성했다 — `dispatched`(created_by `alice`),
`completed`(`alice`), `failed`(`bob`)는 대상 워커를 갖고, `pending`과
`worker_id`가 **없는** `failed`는 갖지 않는다. 마지막 것이 특히 중요하다:
`TaskFailure.worker_id`는 `skip_serializing_if`가 붙어 `None`이면 키 자체가
사라지므로, 술어가 `IS NOT NULL`처럼 헐겁게 동작하면 이 행이 섞여 들어온다.

화면은 정확히 3행을 렌더링했고 2행은 나타나지 않았다. `/api/workers/{id}` 응답의
`recent_tasks`도 같은 3건이었으며, `endpoint`는 `server-key=<redacted>`로 마스킹된
채였다(`#75`의 경계가 이 경로에서도 유지됨을 확인).

**수정 전후를 같은 DB·같은 행에 대고 직접 쟀다.** 이 검증의 값은 여기에 있다 —
회귀 테스트의 RED는 수정을 되돌린 트리에서 잰 것이지만, 아래는 UI가 방금 3행을
그린 바로 그 데이터에 옛 술어를 직접 던진 결과다:

```
-- 수정 전(외부 태그 경로)
SELECT count(*) FROM tasks WHERE (status->'Dispatched'->>'worker_id' = $1
   OR status->'Completed'->>'worker_id' = $1 OR status->'Failed'->>'worker_id' = $1);
 → 0

-- 수정 후(내부 태그 경로)
SELECT count(*) FROM tasks WHERE status->>'worker_id' = $1;
 → 3
```

검증 후 서버를 내리고 `dropdb fleet_wd_test`로 정리했다.

### 이 검증이 **범위 밖의 결함 하나를 드러냈다** — 워커 상세 화면의 저장형 XSS

화면을 읽다가 `ENDPOINT` 칸이 `http://127.0.0.1:9999?server-key=`로 끝나고
마스킹 표식 `<redacted>`가 **사라진 것**을 발견했다. 값이 텍스트가 아니라 HTML로
파싱되고 있다는 뜻이다. `crates/fleet-dashboard/assets/worker-detail.js:50`의
`grid.innerHTML = details.map(...)`이 `w.endpoint`·`w.status`·`w.circuit_state`·
`w.worker_version`을 **이스케이프 없이** 문자열 보간한다. 바로 아래 `labels`(63행)와
`renderTasks`의 `prompt`(94행)는 `escapeHtml()`을 통과하므로, 빠뜨린 것이지
의도한 설계가 아니다.

실행되지 않는 무해한 표식으로 실측했다 — `worker_version`을
`<i id=ztmp-xss-probe>PARSED</i>`로 바꾸고 페이지를 다시 열자
`document.getElementById('ztmp-xss-probe')`가 **실제 DOM 요소를 반환**했다.
`worker_version`은 워커가 자기 신고하는 값이고 `endpoint`도 등록 시 워커가 준
문자열이므로(마스킹은 `server-key` 값만 지우고 나머지는 보존한다), 워커 하나를
장악하면 그 워커 상세를 여는 **관리자 세션에서 스크립트가 돈다**.

이번 커밋의 범위(`PgStore::list_tasks`) 밖이라 고치지 않고 분리했다. 확인 뒤
`worker_version`은 `0.1.0`으로 되돌렸고 DB는 통째로 삭제했다. **코드를 읽어서가
아니라 화면을 읽다가 잡혔다** — 마스킹 표식이 사라진 것은 SQL 수정과 아무 관계가
없었고, 실검증을 API 응답 확인으로만 끝냈다면 `<redacted>`가 JSON에 그대로 있어
보이지 않았을 것이다.

## 2026-08-29 — `#67` 3단계: 용량 판단을 워커 자기보고에서 오케스트레이터 원장으로

`has_capacity()`가 읽던 `Worker::active_tasks`는 **워커가 하트비트 본문에 담아
보내는 값**이다(`fleet-api`의 `handlers.rs:244`·`:284`가 유일한 쓰기 경로 —
오케스트레이터는 dispatch 시 이 값을 올리지 않는다). 세 가지가 따라온다:
갱신이 health interval(기본 15초) 주기라 그 사이 과다 dispatch가 나고, 워커가
위조할 수 있고, 0을 신고하면 필터를 통과하는 데 그치지 않고 least-loaded 정렬에서
**우대**받는다. 마지막 항목 때문에 `selector.rs:203`의 필터만 고치는 것으로는
부족했다 — 정렬 키(`:229`)도 같이 옮겨야 거짓말이 이득이 되지 않는다.

`Store::count_dispatched_tasks_by_worker()`를 추가했다. PgStore는
`WHERE status_phase = 'dispatched' AND status->>'worker_id' IS NOT NULL GROUP BY 1`
한 번이다. `status_phase`는 생성 STORED 칼럼(`001_init.sql:43`)이고 전용 인덱스
`idx_tasks_phase`(`002_indexes.sql:10`)가 이미 있어 **새 인덱스를 만들지 않았다**.

### 용량 판단은 이 저장소에 세 군데 있고, 셋 다 다른 사실을 말한다

바꾼 것은 (1)이고, (2)는 그대로 두는 것이 맞다.

| # | 위치 | 세는 것 | 범위 | 위조 가능 |
|---|------|---------|------|-----------|
| 1 | `Worker::active_tasks` → **store 파생 카운트** | 오케스트레이터가 기록한 `Dispatched` 행 | 전역 | 아니오 |
| 2 | `acp_transport.rs:178`의 `tokio::sync::Semaphore` | 이 프로세스가 실제로 연 세션 | **프로세스 내** | 아니오 |
| 3 | (없음) CAS slot claim | — | — | — |

(2)를 selector와 맞추지 않아도 되는 이유: 그것은 자기보고를 읽지 않고
`try_acquire_owned()` 실패로 `TransportError::WorkerAtCapacity`를 낸다. 즉 둘이
어긋날 여지가 애초에 없다. 다만 scale-out에서 (2)는 오케스트레이터 프로세스마다
따로 세므로, 전역으로 맞는 숫자는 (1)뿐이다.

### 함께 필요해진 변경 — 오류 메시지가 거짓말이 되는 것을 막는 쪽이 더 시급했다

`candidates.retain(...)`이 목록을 비우면 제어가 흘러 `AllOffline`("no worker is
currently online") 또는 힌트 갈래의 `HintedUnavailable`("offline or circuit-open")로
끝난다. **자기보고 시절에는 이 갈래가 사실상 도달하지 않았다** — 값이 0 쪽으로
낡아 있어 필터가 거의 걸리지 않았기 때문이다. store 파생 카운트는 dispatch 즉시
오르므로 포화가 후보를 비우는 흔한 경로가 된다. 그대로 뒀다면 운영자는 포화가
날 때마다 "워커가 오프라인"이라는 답을 받고 멀쩡히 돌고 있는 워커를 재시작했을
것이다. **정확해진 카운트가 부정확한 메시지를 처음으로 노출시킨 형태다.**
`AllAtCapacity`·`HintedAtCapacity`를 추가했다. 판단 기준은 `AllUnprobed`를
나눌 때와 같다 — 운영자가 할 일이 다르면 다른 오류다.

`Worker::has_capacity()`와 `is_dispatchable()`은 삭제했다. selector가 유일한
프로덕션 호출자였고(`is_dispatchable()`은 그 전부터 테스트에서만 불렸다),
`Worker`는 store에 접근할 수 없으므로 그 자리에서 신뢰할 수 있는 용량 판단을
**할 수가 없다**. 주석만 달고 남겨 두면 다음 기여자는 `has_capacity()`라는
이름을 보고 부르고, 방금 신뢰할 수 없다고 판정한 값을 다시 읽는다.

### 게이지: 이름 유지, 의미 교체 (사용자 결정)

`fleet_workers_active_tasks_total`을 같은 메서드로 계산하게 했다. 두 게이지로
쪼개는 안을 제안했으나 사용자가 단일 게이지 교체를 선택했고, 그대로 따랐다.
**받아들인 대가**: 자기보고와 관측값의 괴리 자체는 이제 관측할 수 없다 —
워커가 거짓 `active_tasks`를 신고해도 어떤 메트릭에도 나타나지 않는다.
기존 대시보드·알림은 이름을 바꾸지 않아도 계속 동작하지만, 측정 대상이
"워커들이 뭐라고 말했나"에서 "오케스트레이터 원장에 무엇이 남아 있나"로 바뀌었다.

`fleet_tasks_total{phase="dispatched"}`와 중복 아니냐는 의심이 들어 대조했다.
두 가지가 다르다. (a) 새 게이지는 `list_workers` 결과를 순회하며 더하므로
**등록된 워커에 배정된 것만** 센다 — 워커가 삭제된 뒤 남은 고아 `Dispatched`
행은 빠진다. (b) `fleet_tasks_total`은 `list_tasks(limit: 10_000)`을 세므로 task가
1만 건을 넘으면 과소계상되지만, 새 게이지는 SQL `COUNT(*)`라 상한이 없다.
`fleet_tasks_total`의 이 상한은 이 작업의 범위 밖이라 고치지 않았다.

### `0.16s`가 조용한 skip이 아님을 변이로 확인했다

`dispatched_load.rs` 5건이 `DATABASE_URL` 주입 상태에서 `0.16s`에 끝났다.
§4.3(3)이 기록한 조용한 skip과 같은 모양이라 SQL의 JSONB 경로를 일부러
`status->'Dispatched'->>'worker_id'`(=`3b0a846`에서 고쳤던 그 틀린 모양)로 바꿔
다시 돌렸다 — **3건이 실패했다**. Postgres 갈래가 실제로 돈다는 뜻이고, 0.16s는
진짜다. 통과한 나머지 2건은 빈 맵을 기대하는 테스트였다. 즉 그 둘은 이 결함을
**원리적으로 가릴 수 없다**. 소요 시간만으로 판정하려 했다면 여기서 막혔을
것이고, 변이가 그 자리를 대신했다.

### 기존 테스트 두 건이 정렬의 출처를 암묵적으로 단정하고 있었다

`select_model_none_is_backward_compatible`이 깨졌다. 그 테스트의 의도는
"model=None이면 model 라벨을 무시한다"인데, 부하를 `make_worker("busy", 5, ...)`
처럼 자기보고로 표현하는 바람에 **정렬이 그 필드를 읽는다는 것까지** 같이
단정하고 있었다. 출처를 옮기자 세 워커가 전부 부하 0 동률이 되어 tie-break인
이름 순으로 `busy`가 뽑혔다. 픽스처를 `with_load()`로 옮겼다.
`periodic_worker_is_preferred_over_idle_on_demand_worker`도 같은 이유로 옮겼다.
tie-break가 결정적(이름 순)이라 이 실패는 재현 가능했다 — 무작위였다면 flaky로
나타났을 자리다.

`selector.rs`의 `MockStore`에는 빈 맵을 돌려주는 스텁을 두지 않고 `with_load()`
빌더를 붙였다. 빈 맵이면 모든 테스트가 "부하 전부 0"이라는 한 경우만 밟게 되고,
그건 교체 이전 동작과 구분되지 않는다 — **스위트는 초록인데 새 로직은 한 줄도
검증되지 않는다.** 새로 넣은 `least_loaded_ignores_self_reported_active_tasks`는
자기보고와 store 값을 **정반대로** 세팅한다(자기보고 9/실제 0 대 자기보고 0/실제 4).
같은 방향으로 세팅하면 어느 쪽을 읽든 통과하므로, 반대로 두는 것이 판정을 만든다.

`Store` 구현체는 5개가 아니라 6개였다. `impl Store for`로 grep해서
`fleet-scheduler/src/sync.rs:148`의 `impl fleet_store::Store for NoopStore`를
놓쳤고, 컴파일러가 잡았다.

### 검증 한계

- **읽고-보내는 사이는 여전히 원자적이지 않다.** 동시에 도는 `select()` 둘이
  같은 N을 읽고 둘 다 dispatch할 수 있다. 이 창을 닫는 것은 store 파생 카운트가
  아니라 CAS slot claim이며, 그것은 `#67`의 남은 절반(`worker_execution_lease`)이다.
  이번 변경이 그 창을 닫았다고 읽지 말 것.
- 라이브 fleet에서 워커가 거짓 `active_tasks`를 신고하는 상황은 재현하지 않았다.
  테스트에서 두 값을 반대로 세팅해 selector가 어느 쪽을 읽는지로 대신 확인했다.
- 대시보드 UI 실검증은 하지 않았다. 이번 변경의 사용자 가시면은 Prometheus 게이지
  하나와 오류 메시지 두 개이고, 둘 다 화면 렌더링을 거치지 않는다.
- `fleet_tasks_total`의 1만 건 상한은 대조 과정에서 확인만 하고 고치지 않았다.

---

## 2026-08-29 — security — 이스케이프는 있었는데 더 약한 이스케이프가 전역을 가리고 있었다 (`#98` / S13)

대시보드 정적 자산의 저장형 XSS를 제거했다. 발견의 요점은 취약점 자체가 아니라
**한 종류의 실수가 아니었다**는 것이다.

### 네 계층

1. 약 20곳에서 `escapeHtml` 호출이 그냥 빠졌다. 12개 파일에 흩어져 있어 눈으로는 안 보인다.
2. 11개 페이지 스크립트가 각자 지역 `escapeHtml`을 정의해 **전역을 가렸다**:

   ```js
   function escapeHtml(s) {
     const d = document.createElement('div');
     d.textContent = s;
     return d.innerHTML;      // ← `"`와 `'`가 그대로 살아 나온다
   }
   ```

   전역(`app.js:292`)은 `String(s ?? '').replace(/[&<>"']/g, …)`로 5문자를 모두 바꾸는
   정상 구현이었다. 텍스트 노드 직렬화기는 `&`·`<`·`>`만 바꾸므로 지역 정의는 **속성
   문맥에서 무력**하다. 이 계층이 가장 잡기 어렵다 — 호출부는 `escapeHtml(x)`라고 적혀
   있어 **이스케이프하는 것처럼 보인다**. 호출부만 읽는 리뷰로는 원리적으로 보이지 않는다.
3. 3곳이 `onclick="fn('${escapeHtml(v)}')"` 를 문자열로 만들었다. 여기는 **강한
   이스케이프로도 막히지 않는다**: HTML 파서가 속성값의 문자 참조를 JS 파싱보다 **먼저**
   디코드하므로 `&#39;`가 `'`로 되돌아간 뒤 JS에 넘어간다. 값이 `a');alert(1);//`이면
   그대로 실행된다. 고치는 방법은 이스케이프를 더 강하게 하는 게 아니라 **데이터를
   핸들러에서 빼는 것**이다(`data-*` + `addEventListener` — `tasks.js:226`과
   `project-detail.js:153`이 이미 쓰던 관례).
4. `grokBadge()`처럼 **HTML 조각을 반환하는 헬퍼**는 호출부(`${grokBadge(h.grok_version)}`)가
   이미 조각을 받는 자리라 거기서 감쌀 수 없다. 이스케이프가 헬퍼 **안**에 있어야 한다.

`row.onclick = () => …`(4곳)은 계층 3이 **아니다**. DOM 프로퍼티에 함수를 대입하는 것이라
HTML 속성 파싱을 거치지 않는다. 위험한 것은 보간으로 만들어진 `onclick="…"` **속성
텍스트**뿐이다.

### 공격 방향

`worker_version`·`os_type`·`arch`·`grok_version`·워커 라벨은 전부 워커의 자기 신고 값이다.
침해된 워커가 **관리자 브라우저**에서 스크립트를 실행한다 — 워커 → 관리자 권한 상승.
SSH 키 이름은 업로드 시 `trim().is_empty()`만 검증하므로(`provisioning.rs`) `host:provision`
보유자 → 관리자 상승 경로다.

### 감사 방법이 결론을 바꿨다

처음 돌린 스캔은 **보간 중심**(`${…}`)이었고 48건을 전부 안전으로 판정했다. 그런데
`provision.js`는 전부 `'+ k.name +'` 문자열 결합이라 그물에 걸리지 않아 **스캔 대상에
아예 들어오지 않았다**. 그 파일에는 SSH 키 이름과 서버 에러 본문이 이스케이프 없이
`innerHTML`로 들어가는 확정 취약점이 5곳 있었다.

**싱크 중심** 스캔(`innerHTML =`·`insertAdjacentHTML`·`outerHTML`·`.href =`)으로 다시
돌려서야 드러났다. 데이터가 어떤 문법으로 조립되든 결국 통과해야 하는 문이 싱크이므로,
이스케이프 감사의 정본 스캔은 싱크 쪽이어야 한다. 최종 81건 판정, 잔여 0.

이것은 `agent.md` §4.3이 반복해 기록해 온 것의 또 다른 얼굴이다: **판정의 범위는 스캐너의
문법 가정만큼만 넓다.** "48건 전부 안전"은 참이었지만 48건이 전체가 아니었고, 그 사실은
판정 안에 적혀 있지 않았다.

`.href =` 4곳은 전부 `'tasks/' + encodeURIComponent(id)` 꼴이다. 여기서 방어는 이스케이프가
아니라 **상수 상대경로 접두사**다 — `escapeHtml`은 `javascript:` 스킴을 막지 못한다.

### 회귀 테스트에서 일부러 하지 않은 것

`crates/fleet-dashboard/tests/asset_xss_invariants.rs`는 계층 2와 3의 **구조적 불변식만**
잠근다. 계층 1("모든 보간이 이스케이프됐는가")은 단정하려면 변수 이름 allow-list가
필요한데, 그러면 `fooLabel`이라 이름 붙인 원본 데이터가 통과하고 안전한 변수를 개명하면
실패하는 — **시간이 지날수록 틀려지는** 테스트가 된다. 잠글 수 있는 것은 이름에 의존하지
않는 사실뿐이다.

두 테스트 모두 **바닥 단정**을 함께 둔다(`app.js` 존재 + `.js` ≥ 15). `assets.rs`가 폴더
부재를 빈 디렉토리로 허용하므로, 이게 없으면 자산이 0개일 때 두 테스트가 조용히 통과한다 —
§4.3 (3)이 기록한 `14 passed / finished in 0.00s`와 같은 모양이다.

주석 제거기는 `://` 앞의 `//`를 건너뛴다. 지금 자산에 URL은 없지만, 순진한 제거기는 나중에
URL이 들어왔을 때 뒷부분을 통째로 먹어 **위반을 감추는** 쪽(거짓 음성)으로 고장 난다.
고장 방향을 고르는 것이 요점이다.

**테스트가 빨개질 수 있는지도 확인했다** — `projects.js`에 약한 지역 정의와 `onclick=`
문자열을 일시적으로 되살려 두 테스트가 모두 실패(`exit=101`)하는 것을 보고 원상복구했다.
통과만 보는 것은 판정이 아니다. 부수적으로 debug 빌드의 `rust_embed`가 디스크에서 읽는다는
것도 실측됐다(재빌드 없이 수정이 테스트에 보였다).

### 함께 정리한 것

`admin-users.js`의 `toggleUser(id, currentEnabled)`에서 죽은 인자를 지웠다. 서버가
`POST /api/users/{id}/toggle` 하나로 현재 상태를 보고 뒤집으므로 클라이언트가 알려 줄 필요가
없었고, 그 인자가 `'${u.enabled}'`로 속성에 박히던 자리였다.

### 실검증 — 브라우저에서 페이로드를 먹여 봤다

임시 DB(`fleet_xss_verify`)에 관리자 계정과 페이로드가 실린 SSH 키를 넣고 대시보드를
`127.0.0.1:18082`에 띄워 실제로 확인했다. 키 이름은 `create_ssh_key_api`가 `trim().is_empty()`만
검사하므로 DB에 직접 넣은 값과 API로 넣은 값이 표시 경로에서 구분되지 않는다.

- `/admin/ssh-keys` — 이름 `xss1');alert('L3-PWNED');//`, 타입 `ed25519"><img src=x
  onerror=alert('L1-PWNED')>`. DOM 조회 결과 `img[src="x"]` 0건, `[onerror]` 0건,
  목록 안 `[onclick]` 0건. `alert`는 한 번도 호출되지 않았다.
- **계층 3의 수정이 옳다는 증거는 `data-delete-key`의 값이다.** 속성에는 페이로드가
  **원본 그대로** 담겨 있는데(`xss1');alert('L3-PWNED');//`) 아무 일도 일어나지 않는다.
  값을 약하게 만든 게 아니라 값이 **파싱되는 자리에서 치웠기** 때문이다.
- **리스너 재바인딩을 실측했다.** 첫 행을 삭제(`confirm` → true)하면 `render()`가 다시 돌아
  목록이 `["zz-second"]`로 줄고, 그 뒤 남은 행의 Delete가 다시 `confirm`을 띄웠다. 위임을
  `render()` **안에** 둔 것이 실제로 살아 있다는 뜻이다. 두 번의 클릭 동안 `alert` 0건.
- `/hosts/provision` — 이번에 새로 고친 파일이라 가장 중요한 확인이다. `<option>`의
  `value`가 `prov"><img src=x onerror=alert('PROV-NAME')>`로 **무손실 복원**되는데도
  `img[src="x"]` 0건이다. `escapeHtml`이 `"`를 `&quot;`로 바꿔 속성이 조기에 닫히는 것을
  막았고, 파서가 그 참조를 디코드해 값은 원본으로 돌아온 것이다. `innerHTML` 쪽은
  `&gt;&lt;`로 남아 텍스트 노드가 됐다 — **같은 이스케이프가 두 컨텍스트에서 각각 옳게**
  동작한다.
- 이 마지막 관측이 계층 2의 정확한 반증이다. 삭제한 11개의 약한 헬퍼는 `textContent` →
  `innerHTML` 왕복이라 `"`를 그대로 뒀으므로, 같은 자리에서 속성이 닫히고 `<img>`가
  삽입됐을 것이다.
- 검증 후 서버를 내리고 `dropdb fleet_xss_verify`로 정리했다(포트 미점유·프로세스 부재 확인).

### 검증 한계

- **계층 1은 회귀 테스트가 없다.** 위 실검증이 `admin-ssh-keys.js`와 `provision.js`의 표시
  경로에서 계층 1 방어가 성립함을 실측했지만, 그것은 **오늘의 코드에 대한 관측**이지
  불변식이 아니다. 새로 추가되는 보간의 이스케이프 누락은 여전히 사람 눈에 의존한다.
- 자산 스캔은 `.js`만 본다. `admin-users.html`·`admin-ssh-keys.html`의 **정적** 인라인
  `onclick`(`display='none'` 토글, 보간 없음)은 검사 대상이 아니다.
- `fmtTokens`가 전역(`n: number`)과 지역 3곳(`t: object`)으로 시그니처가 갈리는 그림자를
  발견했으나 고치지 않았다. 전역의 유일한 호출부가 `index.html`에만 있는
  `metric-tokens-total`을 대상으로 하고 `setMetric`이 요소 부재 시 no-op이라 충돌이
  실현되지 않으며, 실현되더라도 표시 버그이지 보안 결함이 아니다.
- `worker_to_summary`가 `mask_server_key`를 적용하는 것을 확인했으므로 워커 상세 화면의
  `Endpoint`에 secret이 실리지는 않는다. 이번 감사에서 확인만 하고 바꾼 것은 없다.

---

## 2026-08-29 — fix — `Failed`가 "워커가 실패했다"와 "우리가 결과를 못 봤다"를 한 이름으로 부르고 있었다

`#67` 후속 게이트가 시작 가능한지 조사하다가, 게이트 자체가 아니라 그 옆에 기록만 되어
있던 인접 결함이 **오늘 고칠 수 있는 상태**임이 드러나 처리했다.

### 조사: `#67` 다음 게이트 세 항목은 서로 다른 이유로 막혀 있다

| 항목 | 판정 | 막는 것 |
| --- | --- | --- |
| `worker_execution_lease` CAS slot claim·`fencing_token` | 막힘 | lease의 **주체**가 없다. 레코드가 실을 필드는 전부 생산자를 얻었지만(`agent_id`←`#49` 1단계, `worker_incarnation`←`#67` 2단계, `control_epoch`←1단계) fencing할 대상인 *실행 중인 Agent process*가 존재하지 않는다 — `dispatch`는 Agent를 고르지 않고(`#49` 2단계), `WorkerTransport`에 프로세스를 띄울 채널이 없다(`#89`) |
| Agent self-fencing E2E | 막힘 | 같은 이유(`#89`) |
| ACK 유실 `OutcomeUnknown` | **막힘, 그러나 사유가 정본과 다르다** | 정본은 "설계가 아직 정해지지 않았다"라고 적어 뒀는데, 설계는 `execution-consistency.md`에 상태 기계로 이미 그려져 있다. 실제로 막는 것은 **해소기**다 |

`#67` 2단계가 자기 blocker를 녹인 방식(워커 자기보고 → 오케스트레이터 관측으로 재구성)을
여기에 다시 쓸 수 없다는 것도 함께 확인했다. 그때는 **신호가 이미 있었고** 해석만 바꾸면
됐다. lease는 신호가 아니라 fencing 대상 자체가 없으므로, 관측으로 재구성할 원본이 없다.

### 그럼에도 오늘 고칠 수 있었던 것

`OutcomeUnknown` 조사에서 나온 사실: `WorkerEvent::Failed`의 **여섯 생성 지점 중 셋은
워커가 실패를 보고한 적이 없다**(연결 상실 시의 `fail_all()`, `session/new` 타임아웃,
`session/prompt` 타임아웃). 이 "셋"과 아래의 "관측을 잃은 것은 둘"은 다른 집합이다 —
`session/new` 타임아웃은 보고가 없었지만 결과는 불확실하지 않다. 그런데 `dispatcher.rs`가 여섯 전부를
`FailureKind::WorkerError`로 못 박고 있었고, 그 이름의 doc은 "워커에서 **실행 중** 발생한
에러(exit ≠ 0, panic 등)"라고 주장한다. 운영자는 그 분류를 보고 워커 로그를 뒤지지만,
연결 상실이나 prompt 타임아웃에서는 그 로그에 오히려 **성공 기록**이 남아 있을 수 있다.
`control-plane-authority-and-failover.md`가 "인접 결함 1"로 기록해 둔 것이다.

`InvalidRequest`가 이미 같은 논증으로 존재한다 — "`WorkerError`는 워커가 실행을 시도했다가
실패했다고 **주장**한다". `ResultLost`는 그 거울상이다: 요청은 분명히 갔는데 답이 오지 않았다.

**바꾼 것.** `fleet_transport::FailureObservation{Reported, NotDelivered, ResultLost}`를
`WorkerEvent::Failed`에 실었고, dispatcher가 각각 `WorkerError`/`WorkerUnavailable`/
`ResultLost`로 옮긴다. 가르는 기준은 심각도가 아니라 **오케스트레이터가 아는 것의 범위**이며,
두 질문을 **순서대로** 묻는다: (1) 답이 왔는가 — 왔으면 `Reported`. (2) 안 왔으면, 프롬프트가
전달됐는가 — 전이면 `NotDelivered`, 후면 `ResultLost`.

**처음에는 이 둘을 대등한 축으로 적었고, 그 서술은 바로 아래 표와 모순됐다.** "넷째 조합
(요청이 안 갔는데 답이 왔다)은 존재하지 않는다"고 썼는데, 표의 둘째 행 `session/new` 에러
응답이 정확히 그 조합이다 — 프롬프트는 나가지 않았고 워커는 답을 줬다. 대등한 두 축으로
읽으면 그 칸이 어디로 가야 할지 정해지지 않는다. 질문 1이 먼저인 이유는 **답이 온 순간
질문 2가 무의미해지기 때문**이다: 결과가 확정된 마당에 프롬프트가 어디까지 갔는지는
오케스트레이터의 판단을 바꾸지 않는다. 질문 2는 답이 없을 때에만, 그 무지의 범위를
좁히려고 묻는다. 이 결함은 커밋 직전 diff를 다시 읽다가 잡혔다 — **이 변경 자체가
"doc이 코드에 대해 거짓말하는 것"을 고치는 작업인데, 새로 쓴 doc이 같은 결함을 갖고
있었다.** 세 variant라는 결론은 같지만, 근거가 틀리면 다음 variant를 추가할 때 잘못된
자리에 놓게 된다.

| 생성 지점 | 관측 | 저장되는 kind |
| --- | --- | --- |
| `acp_transport.rs` `fail_all()` (연결 상실) | `ResultLost` | `ResultLost` |
| `session/new` 에러 응답 | `Reported` | `WorkerError` |
| `session/new` 타임아웃 | `NotDelivered` | `WorkerUnavailable` |
| `session/prompt` 에러 응답 | `Reported` | `WorkerError` |
| `session/prompt` 타임아웃 | `ResultLost` | `ResultLost` |
| `mock.rs` (테스트 더블) | `Reported` | `WorkerError` |

### 세 지점을 빼내는 것으로는 절반만 고쳐진다

이 변경의 논거는 "`WorkerError`의 doc이 그 이름이 실제로 담는 것보다 좁다"였다. 그런데 셋을
빼낸 뒤에도 남은 셋 중 `session/new` 에러 응답은 **여전히 실행 중이 아니다** — 세션이 열리지
않았으니 실행이 시작된 적이 없다. doc을 그대로 뒀다면 이 커밋이 인용한 바로 그 문장이 커밋
후에도 거짓으로 남고, 논증을 따라온 독자가 같은 모순을 다시 발견하게 된다. 그래서
`FailureKind::WorkerError`의 doc을 **"워커가 응답으로 보고한 실패 — 실행 중 에러이거나 실행 전
세션 생성 거부"**로 넓혔다.

지점을 다른 variant로 옮기지 않은 이유: `InvalidRequest`는 요청이 잘못됐다는 뜻이라 맞지 않고,
일곱째 variant를 만들 이유도 없다 — **워커가 답을 준 이상 결과는 확정이고, 오케스트레이터의
후속 판단을 정하는 것은 그 확정성이지 세션이 열렸는지 여부가 아니다.** 이것은 위 `FailureObservation`
질문 순서와 같은 판단이며, 그 판단이 kind 쪽에도 일관되게 적혀야 한다.

### 정본의 목록이 과포함이었다

인접 결함 1은 관측 상실 경로로 **셋**(`session/new` 타임아웃 포함)을 적어 뒀는데,
코드를 읽으면 `session/new` 타임아웃은 그 자리에서 `return`하므로 **프롬프트가 전달되지
않았다**. 결과가 불확실한 것이 아니라 "실행이 시작되지 않았다"가 확정이다. 그래서 그 자리는
`ResultLost`가 아니라 `WorkerUnavailable`(워커 무응답)로 갈랐다.

이 판단의 근거는 코드 구조에 있는 불변식이다: `fail_all()`이 훑는 `sessions` 맵에는
`session/new`를 통과한 태스크만 들어간다. **세션 맵에 있다 = 프롬프트가 이미 전달됐다.**

### `FailureKind::ResultLost`는 `OutcomeUnknown` 상태가 아니다

이름을 `OutcomeUnknown`으로 짓지 않은 이유가 여기 있다. 정본의 `OutcomeUnknown`은
**비terminal 위상**이고 출구 세 개가 전부 inventory·effect ledger 증명이다. 그 판독기가 없는
지금 위상을 만들면 들어간 Task가 영원히 나오지 못하고, 비terminal Task는 Project archive를
정지시킨다 — 모른다는 사실을 표시하는 대신 **일을 멈추는** 결함이 된다. `ResultLost`는
terminal 분류이며, 해소기가 생기면 그 위상의 **입력**이 될 값이다. 코드 doc과
`execution-consistency.md`에 이 구분을 표로 박아 뒀다.

### breaker는 건드리지 않았다

`ResultLost`도 `Outcome::Failure`로 남긴다. 분류가 가른 것은 **작업의 결말**이지 워커의
건강도가 아니다 — 연결이 끊기거나 응답이 없는 것은 그 워커에 대한 진짜 나쁜 신호이고,
여기서 빼면 고장 난 워커로 계속 디스패치하게 된다. 이것을 고정하는 테스트를 함께 넣었다.

### 가드가 실제로 발화하는지 먼저 확인했다

`metrics.rs`의 `failure_kind_allow_list_covers_every_variant`가 새 variant를 강제한다고
**믿지 않고**, allow-list를 고치기 전에 돌려서 실제로 깨지는 것을 봤다:
`FailureKind::ResultLost is missing from the metric label allow-list`. 가드에 의존하기 전에
가드를 검증한 것이며, 이것이 없으면 "컴파일러가 강제한다"는 문장이 근거 없는 안심이 된다.

### 실패한 첫 시도 — 관측 지점을 잘못 골랐다

breaker 테스트를 처음에 "저장소의 워커 회로 상태가 `Open`인가"로 썼다가 `Closed`로 실패했다.
코드 결함이 아니라 **`Store::update_worker_circuit_state`가 트레이트 기본 구현 `Ok(())`이고
`MemStore`가 이를 재정의하지 않아** 메모리 저장소에서는 쓰기가 조용히 사라지기 때문이었다.
`BreakerRegistry::state_of()`로 관측 지점을 옮겨 해결했다. 테스트가 깨졌을 때 대상 코드부터
의심하면 이런 종류는 오래 걸린다 — **관측 지점이 그 사실을 담을 수 있는지**를 먼저 본다.

### 검증 한계

- **transport의 다섯 생성 지점이 각각 어떤 `observation`을 싣는지는 테스트가 없다.**
  고정한 것은 `observation → FailureKind` 매핑이고, 그 앞단(어느 실패 경로가 어느
  `observation`을 고르는가)은 코드 대조로만 확인했다. 타임아웃과 연결 상실을 실제로
  일으키는 통합 테스트는 만들지 않았다.
- 라이브 fleet에서 연결이 끊긴 뒤 워커에서 작업이 계속 돌아 결과가 실제로 갈리는 상황은
  재현하지 않았다. 이 변경이 고치는 것은 **분류의 정직성**이지 그 상황의 처리 자체가
  아니다 — 처리는 `OutcomeUnknown` 해소기가 생겨야 가능하다.
- 인접 결함 2(prompt 타임아웃이 `cancel` 없이 permit만 놓는 것)는 그대로 두었다.
  해소하려면 `cancel`을 보내야 하는데, "답을 못 들은 실행을 끊는다"는 별도의 동작 변경이라
  자체 설계 질문을 갖는다.
- `ResultLost`는 재시도 정책에 아무 영향도 주지 않는다. **처음에는 근거를 "`#62` 4단계의
  무재시도 결정"이라고만 적었는데, 확인해 보니 `reconcile.rs:312`에 `max_dispatch_retries`
  재시도 루프가 실재한다.** 결론은 같지만 근거가 달랐다: 그 루프는 `Pending` 작업만 돌고
  dead-letter의 `mark_failed`도 `&[TaskPhase::Pending]`으로 좁혀져 있다. `ResultLost`는
  `Dispatched → Failed` 경로에서만 생기므로 그 루프에 닿지 않는다 — 거기서 세는 것은
  *디스패치* 재시도이지 실행 재시도가 아니다. `#62` 4단계는 그 위에 "실행이 실패하면
  재시도하지 않고 새 Task를 만든다"를 얹은 별개의 결정이다. 재시도가 돌아오면 이 variant는
  **재시도하면 안 되는** 종류로 다시 검토해야 한다(중복 실행이 된다).

---

## 2026-08-29 — fix — 답을 못 들었으면 끊는다: prompt 타임아웃이 워커를 계속 돌게 두고 있었다 (`#67`)

바로 앞 커밋(`0bc99e5`)이 "이번 범위 밖"으로 남긴 인접 결함 2를 닫는다. `session/prompt`가
타임아웃하면 `AcpTransport`는 `WorkerEvent::Failed{ observation: ResultLost }`를 쏘고
transport의 용량 permit을 놓은 뒤 떠났다. 워커 쪽 실행에 대해서는 **아무 말도 하지 않았다** —
그 에이전트는 아무도 기다리지 않는 답을 위해 계속 돌았다.

### 설계 질문에 답한다

앞 커밋이 미룬 이유는 이것이 분류 수정이 아니라 동작 변경이기 때문이었다: "답을 못 들은
실행을 끊는 것이 옳은가?" 답은 **끊는다**이다.

- 끊지 않으면: 워커가 버려진 턴에 토큰을 계속 태우고, 그 슬롯은 permit 회계상 **비었다고
  기록되지만 실제로는 비어 있지 않다**. 오케스트레이터의 용량 판단이 거짓이 된다
  (`#67` 3단계가 자기보고 대신 원장을 읽게 만든 바로 그 종류의 거짓이다).
- 끊으면 잃는 것: ack 없는 notification 한 건의 비용뿐이다. 워커가 무시해도 이전과 같다.

비대칭이 명백해서 사용자 결정을 요구할 만한 트레이드오프가 아니다.

### 고칠 자리가 타임아웃 arm 안이어야 하는 이유 — `cancel()`은 이 경로에 닿을 수 없다

처음의 자연스러운 발상은 "dispatcher가 `ResultLost`를 보면 `transport.cancel(task_id)`를
부른다"였다. **그 경로는 구조적으로 막혀 있다.**

`cancel()`은 세션을 이렇게 찾는다:

```rust
sessions.iter().find(|(_, s)| s.task_id == task_id).map(|(sid, _)| sid.clone())
```

그런데 prompt 경로는 타임아웃 arm에 **도달하기 전에** 그 엔트리를 이미 제거한다:

```rust
let already_handled_elsewhere = sessions_map.lock().await.remove(&session_id).is_none();
```

이 제거는 그 자체로 옳다 — 2026-08-11에 넣은, 연결 종료 시 `fail_all()`과 중복 emit하지
않기 위한 장치이고, 맵이 무한정 자라는 것도 막는다. 결과적으로 **나중에 부르는
`cancel(task_id)`는 찾을 것이 없어 조용히 `Ok(())`를 반환한다.** 그때 남기는 debug 로그

```
"cancel: no active worker session found — task already terminal?"
```

는 이 경로에 대해 **거짓**이다. 태스크는 워커 쪽에서 terminal이 아니다 — 라우팅 엔트리만
사라진 것이다. 성공을 반환하는 조용한 no-op이라 호출자가 알아챌 방법도 없다.

이것은 이 스레드가 계속 고쳐 온 것과 같은 계열의 결함이다: **판정처럼 보이는 값이 판정이
아니다.** 앞 커밋에서는 `FailureKind::WorkerError`의 doc이 실제 생성 지점에 대해 거짓이었고,
여기서는 `Ok(())`가 "취소했다"에 대해 거짓이다.

그래서 `connection`과 `session_id`가 아직 스코프에 살아 있는 타임아웃 arm 안에서 보낸다.
`already_handled_elsewhere` 가드가 분기를 공짜로 정확하게 준다 — 이 값이 `false`라는 것은
방금 내가 엔트리를 꺼냈다는 뜻이고, 곧 `fail_all()`이 아직 연결을 정리하지 않았다는 뜻이다.
반대 arm(연결 유실)에서는 보낼 곳이 없으므로 조용해야 맞다.

### permit은 계속 그 자리에서 놓는다

"워커가 멈췄음을 확인할 때까지 슬롯을 유지"가 더 옳아 보인다. 하지만 `session/cancel`은
**ack 없는 notification**이라 확인할 채널이 없다 — 오지 않을 확인을 기다리는 permit은 영영
돌아오지 않는다. 바로 앞 커밋에서 `OutcomeUnknown` 위상을 만들지 않은 것과 같은 판단이고,
같은 원칙("채울 방법이 없는 것은 미리 만들지 않는다")이다.

따라서 정직한 표현은 **초과 점유 창이 닫혔다가 아니라 좁아졌다**이다. 관측 분류도
`ResultLost` 그대로다 — cancel을 보냈다는 것과 결과를 알게 됐다는 것은 다르다. 테스트가 두
사실을 **한 함수 안에서** 단정하는 이유가 이것이다. 나눠 두면 다음 사람이 "cancel을 보내니까
이제 확정 아닌가?"로 읽는다.

### 테스트가 드러낸 정정 — 아무것도 안 보낸 게 아니었다

수정을 잠시 끄고 테스트를 돌려 실패를 확인했다(고치기 전에도 통과하는 테스트는 증거가
아니다). 그 실패 메시지가 예상 못 한 것을 보여줬다:

```
받은 메시지: ["initialize", "session/new", "session/prompt", "$/cancel_request"]
```

`tokio::time::timeout`이 만료되면 요청 future가 drop되고, SDK가 그 시점에
`$/cancel_request`를 보낸다. 즉 예전 코드도 워커에 무언가는 보내고 있었다. 그러나 그것은
SDK 자신이 "hop-scoped"라고 부르는 **JSON-RPC 요청 단위 취소**이고(`conductor.rs:573`
부근), 턴을 멈추라고 ACP가 정의한 것은 `session/cancel`이다
(`impl_jsonrpc_notification!(CancelNotification, "session/cancel")`).

그래서 결함의 정확한 진술은 "아무것도 보내지 않았다"가 아니라 **"보낸 것이 실행을 멈추는
신호가 아니었다"**이다. 인접 결함 목록에 처음 적을 때는 후자를 확인하지 않은 채 전자로
적었다 — 앞 커밋에서 `session/new` 타임아웃을 `ResultLost`로 잘못 묶었던 것과 같은 종류의,
**관찰 없이 적어 둔 목록**이 나중에 실측으로 정정되는 사례다.

### 목 하네스에서 밟기 쉬운 함정

`stall_prompt` 스위치는 응답을 **보내지 않고 루프를 계속 돈다**(`continue`). sleep이나 await로
핸들러 안에서 멈추면 소켓을 드레인하는 주체가 사라져, 뒤이어 오는 `session/cancel`을 아예
읽지 못한다 — 검증하려는 바로 그것이 보이지 않게 된다.

`received` 단정도 스냅샷이 아니라 마감 시한 폴링이다. cancel은 fire-and-forget이라
`Failed` 이벤트와 순서가 정해져 있지 않고, 이벤트 직후의 단정은 그 자체로 flaky하다(§3.3).

`timeout_secs = 1`은 `session/new`와 `session/prompt` 두 타이머에 **같이** 먹힌다. 부하가 큰
러너에서 `session/new`가 먼저 1초를 넘기면 코드는 다른 arm(`NotDelivered`, cancel 없음)을
타고, 그때 제일 먼저 깨지는 단정이 관측값 비교라면 실패 메시지가 "관측값이 틀렸다"를
가리켜 원인을 가린다. 그래서 관측값보다 **먼저** `received`에 `session/prompt`가 있는지를
단정한다 — 굶주림으로 실패해도 메시지가 "프롬프트가 목에 닿지도 않았다"라고 스스로
말한다. 로컬 in-process 루프백에서는 여유가 세 자릿수 배라 실제로 밟힌 적은 없고, 이
단정은 **CI에서만 날 수 있는 실패의 진단을 미리 붙여 둔 것**이다.

### 검증 한계

- **워커가 `session/cancel`을 받고 실제로 실행을 멈추는지는 검증하지 않았다.** 목은 받은
  것을 기록만 한다. 진짜 grok이 그 notification에 어떻게 반응하는지는 이 저장소 밖이다.
- 따라서 **초과 점유 창이 실제로 얼마나 좁아지는지 측정하지 않았다.** 창이 좁아진다는
  진술은 "취소 신호가 갔다"까지만 뒷받침된다.
- `$/cancel_request`를 받은 에이전트가 턴을 멈추는지도 확인하지 않았다. 위 정정은 그 둘이
  **다른 것**이라는 프로토콜 수준의 사실이지, 실제 에이전트 동작의 측정이 아니다.
- `session/new` 타임아웃 경로는 그대로 두었다. 세션 ID를 받지 못했으므로 취소할 대상을
  지목할 수 없다 — 워커에 고아 세션이 남을 수는 있지만 이쪽에서 할 수 있는 것이 없다.
  `NotDelivered` 분류는 여전히 옳다(프롬프트가 가지 않았으므로 실행도 시작되지 않았다).
- 라이브 fleet에서 실제 타임아웃을 일으키지는 않았다. 타임아웃은 목이 응답을 보류하는
  방식으로 재구성했다.

### 게이트 검증 중 발견 — `--no-default-features` 잡은 transport를 acp 없이 돌린 적이 없다

새 테스트는 `#![cfg(feature = "acp")]` 뒤에 있으므로 최소 기능 잡에서는 늘지 않아야 했다.
그런데 통과 수가 **양쪽 다** 1씩 늘었다(1134→1135, 1130→1131). 추적한 결과 최소 기능 잡에서도
`acp_transport_integration.rs`가 10건을 돌고 있었고, `#![cfg(all(feature = "acp", feature =
"mtls"))]`인 `acp_transport_mtls.rs`조차 3건을 돌고 있었다.

원인은 `crates/fleet-worker/Cargo.toml:29`다:

```toml
fleet-transport = { workspace = true, features = ["mtls"] }
```

**`--no-default-features`는 선택된 패키지의 *default* 피처만 끈다** — 의존이 명시적으로
요구한 피처는 끄지 못한다. `fleet-transport`의 `default = []`이지만 `fleet-worker`가 무조건
`mtls`를 요구하고 `mtls = [..., "acp"]`이므로, 워크스페이스 빌드에서는 두 피처가 항상 켜진다.

즉 **`--no-default-features` 잡이 검증한다고 여겨졌던 "acp 없는 transport"는 한 번도 빌드된
적이 없다.** §4.2가 요구하는 조건부 임포트/네임 리졸브 회귀는 이 잡이 잡아 주지 않는다.

같은 계열이 이미 한 번 기록돼 있다(`ssh` 피처, 위 2026-08-23 `#84` 항목의 `docs/log.md:861`) — 그때는 크레이트 단독
clippy에서 드러났다. 일반형은 **워크스페이스 전체에 건 `--no-default-features`는 크레이트
단위의 피처 없는 빌드를 뜻하지 않는다**이다. 잡으려면 크레이트 단독으로 돌려야 한다:

```bash
cargo clippy -p fleet-transport --no-default-features --all-targets -- -D warnings
```

이번 변경의 범위 밖이라 게이트 목록은 고치지 않았다 — 별도 작업으로 분리한다. 이번 변경에
대해서는 이것이 위험을 만들지 않는다(추가한 코드가 전부 `acp` 안에 있다).

## 2026-08-29 — lint — 코드 근거를 자처하던 문서가 코드에 대해 여섯 군데 거짓이었다

`docs/architecture/agents/README.md`가 도메인 진입점에서 "현재 코드에는 Agent 엔티티, 명령 ACK,
다중 runtime catalog, Agent API가 없다"고 단언하고 있었다. `#49` 1단계(2026-08-28)가 `agents`
테이블·`fleet-core/src/agent.rs`·MCP 3종·Dashboard `/api/agents`를 랜딩시켰으므로 네 항목 중 둘이
거짓이다. 그 문서가 "현재 코드 근거"로 지목하는 `implementation-reference.md`도 같은 상태였다.

### 대조 결과

| 문서 | 적혀 있던 것 | 코드 |
|---|---|---|
| `agents/README.md` | Agent 엔티티·Agent API가 **없다** | 둘 다 있다. 없는 것은 그 뒤의 **실행**이다 |
| `implementation-reference.md` Core 행 | Project·Agent 모델은 **목표 계약** | `project.rs`/`agent.rs` 실재. 다만 목표보다 좁다(3-상태·2-상태) |
| 같은 문서 Scheduler 행 | Task 실행 CAS는 **아직 목표 계약** | `tasks.dispatch_control_epoch`·`fleet-store/tests/task_cas.rs`로 구현됨. fencing만 미구현 |
| 같은 문서 join 문단 | join은 Worker-scoped credential로 **전환하지 않는다** | `join_worker`가 `fwo_` 토큰을 발급하고 `enroll_worker`가 원자적으로 저장한다 |
| 같은 문서 Agent 문단 | Agent 엔티티는 **구현되지 않았다** | 엔티티는 구현됨. ACK·runtime catalog·terminal attach·장기 메모리는 미구현 |
| 같은 문서 다이어그램 | `CLI·MCP·Dashboard → fleet-api → Store` | `fleet-mcp`/`fleet-dashboard`는 `fleet-api`에 **의존하지 않고** Store를 직접 잡는다 |

### 다이어그램 오류가 보안 판단으로 번지는 자리

여섯 번째가 단순 작도 오류가 아닌 이유는 표면마다 인가를 각자 구현하기 때문이다. `fleet-mcp`와
`fleet-dashboard`의 `Cargo.toml`에는 `fleet-api`가 없고 두 크레이트 모두 `state.store`를 직접
호출한다. 그래서 `#73`이 `fleet-api`에 default-deny capability 행렬을 깔았을 때 Dashboard의 29곳
산재한 `PermissionKind` 검사는 하나도 덮이지 않았고 `#92`로 넘어갔다. 다이어그램대로라면
"행렬 하나 고치면 전부 덮인다"는 결론이 나온다 — 구조도가 틀리면 그 위의 판단이 함께 틀린다.

### 이름이 겹쳐서 생기는 오독을 미리 끊었다

`tasks.dispatch_control_epoch`는 `#67` 1단계로 실재하지만 그것은 **Task dispatch 세대**이고,
`provisioning.md`의 9-필드 명령 봉투가 요구하는 `control_epoch`는 **Agent 명령**의 것이다. 같은
단어라서 "control_epoch가 있으니 명령 봉투도 일부 있다"로 읽히기 쉬워, 진입점에 넷(`generation`·
`control_epoch`·`worker_incarnation`·`fencing_token`) 전부 없음을 명시했다.

### 검증 기준 절도 함께 되돌렸다

`implementation-reference.md`의 "검증 기준" 1·2번이 `cargo test --workspace`와 "해당 통합 테스트"로
적혀 있었다. 이는 `agent.md` §3.2·§4.3이 요구하는 게이트(피처 세트 두 벌, `DATABASE_URL` 무조건
주입, `--test-threads=1`, `fleet-cli` 선행 빌드)보다 **약한 축약본**이다. 약한 축약본이 문서에
남아 있으면 그것을 보고 명령을 만드는 세션이 생긴다 — `agent.md`가 반복해 기록해 온 재발 법칙
그대로다. 축약본을 지우고 정본을 가리키게 했다(§5의 "정책 전문을 복제하지 않는다"와도 같은 방향).

### 이 항목의 범위

문서만 고쳤고 코드는 한 줄도 바꾸지 않았다. `tool-catalog.md`가 `authority: canonical`인데
소유 로드맵 항목이 없는 문제는 이번에 손대지 않았다 — 그것은 서술의 참·거짓 문제가 아니라
소유권 배정 문제이며 `#86`의 범위다.

### 검토에서 이 커밋 자신이 같은 결함을 두 개 저질렀다

작성한 diff를 다시 읽었더니 **이 커밋이 고치려는 바로 그 결함**이 커밋 안에 두 건 있었다.

1. `agents/README.md`의 본문이 "그 문서만 `implementation: partial`이고 나머지는 `proposed`다"라고
   적었는데, 같은 diff의 스무 줄 위에서 **그 README 자신을** `proposed` → `partial`로 바꿨다.
   frontmatter를 고치고 그것을 가리키던 산문을 다시 읽지 않은 것이다. `grep -l 'implementation: partial'
   docs/architecture/agents/*.md`가 README와 provisioning 둘을 돌려준다. "하위 설계 문서 중에서는 그
   문서만"으로 좁히고 개수(일곱)를 실측해 적었다.
2. 다이어그램 문단에 "`fleet-cli`가 이 셋을 한 프로세스에 함께 띄운다"를 근거 없이 적었다 — 같은
   커밋이 검증 기준 2번으로 "현재 사실에는 코드 경로를 함께 적는다"를 새로 넣으면서. 게다가 이
   문장은 위 대조표 셋째 행(CAS)과 같은 종류로 **반쪽만 참**이었다: `run_serve`는 MCP stdio 서버만 무조건
   그 프로세스의 전경에서 돌리고, `fleet-api`와 `fleet-dashboard`는 각각 `--http-bind`·
   `--dashboard-bind`가 주어졌을 때만 `tokio::spawn`으로 덧붙인다. 플래그 없이 `fleet serve`를
   띄우면 MCP 하나뿐이다. 조건절을 붙이고 `fleet-cli/src/runtime.rs`의 `run_serve`를 근거로 달았다.

문서의 현재 서술이 조용히 거짓이 되는 경로는 "코드가 나중에 바뀌어서"만이 아니다 — **같은
커밋 안에서 내가 방금 바꾼 값을 가리키던 문장을 다시 읽지 않아서**가 더 빠르다. 정정 커밋일수록
그렇다: 고치는 대상에 주의가 쏠려 새로 쓰는 문장은 검토를 덜 받는다.

상대 링크는 두 파일 22개 전부 재검사했다(검증 기준에 새로 넣은 `../../agent.md` 포함). 링크
검사도 게이트와 같아서, 문장을 더 고친 뒤에는 다시 돌려야 그 트리에 대한 판정이 된다.


### 검증 한계

- 대조 대상은 `agents/README.md`와 `implementation-reference.md` 두 문서의 **현재-사실 서술**로
  한정했다. 두 문서가 가리키는 하위 8개 설계 문서의 본문은 전수 대조하지 않았다.
- 다이어그램의 화살표는 크레이트 의존성과 `state.store` 직접 호출로 확인했다. 런타임에 실제로
  그 경로만 쓰이는지는 실행으로 확인하지 않았다.
- 게이트는 세 번 돌렸고 마지막 두 번은 `.md` 산문만 바뀐 트리에 대한 것이다. Rust 게이트가
  이 변경에 대해 말해 주는 것은 사실 없다 — `crates/`의 `include_str!`은 `openapi.yaml`
  하나뿐이고 `.md` 매치는 전부 tempdir 런타임 경로이며 `build.rs`도 없어서, `docs/*.md`는
  컴파일 입력이 아니다. 이 항목의 실질 검증은 게이트가 아니라 위의 여섯 건 대조와 링크
  재검사다. 그럼에도 돌린 이유는 게이트가 트리 전체에 대한 판정이기 때문이지, 이 변경을
  검증하기 때문이 아니다.

---

## 2026-08-29 — `#86` AgentTemplate 1단계: 한 커밋의 경계를 죽은 구조가 정했다

### 무엇이 문제였나

`#86`을 어디까지 한 커밋에 넣을지가 처음부터 순환이었다. 셋 중 어느 하나를 빼도 나머지가 죽은
구조가 된다.

- `agents.agent_template_id`를 빼면 → 게이트 3(retire의 dependent set 해시)의 의존 집합이 항상
  비고, 그 시험은 "빈 집합의 해시가 빈 집합의 해시와 같다"를 확인하는 공허한 테스트가 된다.
- 컬럼은 만들되 표면을 빼면 → 템플릿을 만들 주체가 없으므로 pin할 대상도 없고, 그 컬럼은
  **항상 NULL**이다. `022_projects.sql`이 `tasks.project_id`에서 겪은 결함 그대로다.
- capability 6종만 추가하면 → 검사받는 자리가 없는 죽은 권한이다. `auth.rs`는 이미 그런 권한을
  네 번 명시적으로 거절해 뒀다(`project:policy_manage`, `project:assign`, `agent:attach`,
  `issue:archive_hold_manage`).

그래서 경계는 취향이 아니라 "죽은 구조가 생기지 않는 최소 집합"으로 정해졌다: migration + 코어
모델 + Store 양쪽 + pin 집행 + Dashboard 표면 + capability.

### `027`이 예약해 둔 유예를 갚는다

`027_agents.sql`의 주석은 자기가 무엇을 왜 미뤘는지와 **누가 그것을 갚을지**까지 적어 뒀다:
"`agent_template_id` … 컬럼은 만들지 않는다. 채울 주체(AgentTemplate `#86`, harness 구성 `#51`,
isolation `#52`)가 전부 없어 항상 NULL인 컬럼이 된다." `#86`이 그 주체이므로 `029`가 그 컬럼을
만든다. 유예를 표로 남겨 두면 다음 사람이 "왜 없지"가 아니라 "이제 갚을 차례인가"를 묻게 된다.

### pin 유효성은 FK도 표면도 아닌 트랜잭션 안에 있다

네 자리를 놓고 고른 결과다.

| 자리 | 왜 안 되나 |
|---|---|
| FK `RESTRICT` | revision 행의 **존재**만 본다. `revoked_at`이 찍혔는지, 템플릿이 `Retired`인지는 보지 못한다 |
| 두 표면(Dashboard·MCP) | 같은 술어를 두 번 쓰면 갈라진다. `#48`이 `project_rules`로 추출한 이유와 같다 |
| 트랜잭션 **밖**의 사전 조회 | 검사와 INSERT 사이에 revoke가 끼어든다 |
| `create_agent` 트랜잭션 안 | ← 채택 |

잠금은 `FOR SHARE OF r, t`다. pin끼리는 서로 호환되고(동시에 여러 Agent가 같은 revision을 pin해도
문제없다) retire의 `FOR UPDATE`와만 직렬화된다. 이것이 닫는 창은 구체적이다: 확인 화면이 "의존
Agent 0건"을 보여준 뒤 조작자가 승인 버튼을 누르기 전에 누군가 그 템플릿을 pin한 Agent를 만들면,
조작자는 **자기가 승인하지 않은 회수**를 집행하게 된다. `dependent_set_hash`를 요청 본문에 싣게
한 것도 같은 창을 다른 각도에서 닫는다.

### NULL이 두 번 다르게 굴었다

같은 SQL NULL 의미론이 이 커밋에서 정반대 방향으로 두 번 문제를 만들었다.

1. **`UNIQUE (project_id, name)`은 전역 템플릿의 이름 중복을 못 막는다.** Postgres의 UNIQUE는
   NULL을 서로 다른 값으로 보므로 `(NULL, 'reviewer')` 두 행이 공존한다. 부분 유니크 인덱스
   두 개로 나눴다 — `WHERE project_id IS NOT NULL`과 `WHERE project_id IS NULL`.
2. **`WHERE project_id = $2`는 전역 행을 고르지 못한다.** `NULL = NULL`은 거짓이 아니라 NULL이고
   WHERE에서 NULL은 참이 아니므로 한 건도 안 잡힌다. `IS NOT DISTINCT FROM`으로 바꿨다.

전자는 "NULL이 서로 다르다"가 문제였고 후자는 "NULL이 같지 않다"가 문제였다. 같은 규칙의 두 얼굴이다.

### 고정 해시는 어디서 왔는가

`builtin_default_body_hash_is_pinned`가 처음에 실패했다. 내가 리터럴을 지어냈기 때문이다. 여기서
가장 쉬운 수정은 구현이 출력한 값을 그대로 붙여넣는 것인데, **그러면 그 테스트는 아무것도 증명하지
않는다** — 구현이 무엇을 내든 테스트는 통과한다. 대신 정규 인코딩(길이 접두 필드, `agent_template/v1`
도메인 구분자)을 Python으로 독립 재구현해 `bf84a875…`를 얻었고, 그것이 Rust 구현의 출력과 같아서
그 값을 고정했다. 테스트 주석에 그 출처를 적었다 — 다음 사람이 같은 고민을 할 때 "출력을 붙여넣은
값인가"를 물어야 하기 때문이다.

길이 접두를 쓴 이유도 같은 계열이다. 필드를 그냥 이어 붙이면 `["ab","c"]`와 `["a","bc"]`가 같은
해시를 낸다. `content_hash`는 감사 대조의 근거이므로 연접 충돌이 있으면 그 근거가 아니다.

### 설계 정본을 세 군데 고쳤다

구현이 문서를 따르는 것이 원칙이지만, 문서가 틀린 자리는 문서를 고친다.

1. `agent_template:archive` → `agent_template:lifecycle`. 전이 집합이 publish/deprecate/retire/
   discard인데 "archive"는 그중 하나만 가리킨다.
2. `agent_template:revision:revoke` → `agent_template:revision_revoke`. 다른 이름은 전부 2단이다.
3. viewer의 부여를 "없음"에서 `agent_template:read`로. viewer는 이미 `agent:read`를 갖고 Agent
   상세에 pin 두 필드가 노출되므로, template read가 없으면 viewer는 **풀 수 없는 참조**를 본다.
   본문은 비밀이 아니고, 필드별 게이팅이 지키려는 것은 쓰기다.

operator에게 `update`는 주되 `agent:manage`는 주지 않는 배치가 필드별 게이팅을 **공허하지 않게**
만든다. 둘 다 admin만 가졌다면 `required_permissions_for_change`의 `AgentManage` 가지는 항상
참이어서 검사가 무의미했을 것이다.

### MCP 표면을 만들지 않은 것은 미구현이 아니다

LLM이 직접 부르는 표면에 템플릿 편집 권한을 주면 Agent가 자기 role prompt와 도구 목록을 스스로
고칠 수 있다. 그것이 정확히 `agent-template.md`가 존재하는 이유인 권한 상승 경로다. 문서와 코드
양쪽에 그 부재를 규칙으로 적었다 — 적지 않으면 다음 세션이 "대칭이 안 맞네"라며 채운다.

### 테스트를 두 store에 같은 함수로 건다

`crates/fleet-store/tests/agent_templates.rs`는 시나리오 5개를 `async fn(&dyn Store)`로 쓰고
매크로가 그것을 MemStore와 PgStore 두 테스트로 편다. `#78`에서 MemStore에만 있던 규칙이 실
배포에서만 깨진 적이 있어 게이트 8이 존재하는데, 시나리오를 함수 하나로 두면 **같은 단정**이
양쪽에 걸리는 것이 구조적으로 보장된다(한쪽만 고치는 실수를 컴파일러가 잡아 주지는 않지만,
단정을 한 번만 쓰므로 애초에 갈라질 자리가 없다).

`MemStore`가 `test-support` 피처 뒤에 있어서 통합 테스트에서 보이지 않았다. `fleet-store`를
자기 자신의 dev-dependency로 다시 걸어 그 피처를 켰다 — dev-dependency는 릴리즈 그래프에
들어가지 않으므로 프로덕션 빌드에는 영향이 없다.

### 검증 한계

- **브라우저 실검증을 하지 않았다.** 이 표면에는 화면이 없고 JSON API뿐이라 확인할 UI가 없다.
  관리 화면은 `#92`의 남은 절반이다.
- 게이트 4·5·6은 도달할 수 없었다. 실행 중인 Agent 프로세스(`#89`)와 `projects`의 정책
  컬럼(`#48`)이 없기 때문이며, 없는 것을 흉내 내는 테스트를 쓰면 통과가 거짓이 된다.
- 게이트 7은 절반만이다. 본문 해시는 고정했으나 시드 행을 넣는 주체가 없고, tool binding이
  `ReadOnly` 등급인지 단정하려면 그 등급을 나타내는 타입이 필요한데 `ReadOnly`/`IsolationClass`/
  `ToolGrade`/`tool_catalog` 어느 이름도 `crates/` 전체에서 0건이다. `tool-catalog.md`가
  `authority: canonical`인데 소유 로드맵 항목이 없다는 별개 문제가 그대로 남아 있다.
- `FOR SHARE`/`FOR UPDATE`의 직렬화는 SQL 의미론으로 논증했고, 실제 동시 실행으로 경합을
  재현하지는 않았다. 재현하려면 두 트랜잭션의 인터리빙을 제어해야 하는데 이 하네스에 그 수단이
  없다.

## 2026-08-29 — `#92` AgentTemplate 관리 화면: 브라우저가 잡은 결함은 둘, 내가 결함이라 오인한 것은 셋

### 무엇을 만들었나

`#92`의 "남은 것" 목록 첫 줄이던 AgentTemplate 관리 화면(HTML)을 냈다. Project 화면과 같은 3면
구성이다 — 목록(`/agent-templates`), 생성(`/agent-templates/new`), 상세(`/agent-templates/:id`).
`#86` 1단계가 JSON API만 내고 "브라우저 실검증을 하지 않았다. 이 표면에는 화면이 없고 JSON
API뿐이라 확인할 UI가 없다"를 검증 한계로 적어 뒀는데, 이 커밋이 그 한계를 갚는다.

서버 쪽에서 새로 만든 것은 `GET /api/agent-templates/:id` 하나와, 응답에 실리는 두 파생
필드다. `AgentTemplateSummary`에 `allowed_transitions: Vec<String>`과 `accepts_new_revisions:
bool`을 더했고, 그 근거는 `fleet-core`의 `AgentTemplateStatus::allowed_transitions()`라는 **단일
전이표**다. 화면이 상태 기계를 자기 쪽에 복제하지 않게 하려는 것이다 — 복제하면 규칙이 두 곳에
있게 되고, 둘이 갈라지는 순간 화면은 서버가 거절할 버튼을 보여 준다. 지금은 서버가 "지금 이
템플릿에서 갈 수 있는 곳"을 문자열 목록으로 말해 주고, 화면은 그 목록대로 버튼을 그린다.

### 브라우저 실검증에서 나온 진짜 결함 2건

**(1) `Revisions` 제목이 패널 안 스타일을 쓰고 있었다.** 이 저장소의 대시보드 자산은 제목
스타일을 위치로 나눈다 — 패널 *밖* 제목은 22px(`task-detail.html:50,53,57,62`), 패널 *안* 제목은
`margin-top:0;font-size:15px`다. `Revisions`는 패널 밖에 있는데 15px을 쓰고 있어서, 바로 위
Lifecycle 패널에 달라붙은 캡션처럼 읽혔다. 스크린샷을 보기 전에는 알 수 없는 종류의 결함이다.

**(2) 의존자가 0인데 버튼이 `Retire anyway`였다.** retire 확인 화면은 이 템플릿의 revision을
pin한 Agent 목록을 먼저 조회해서 보여 준다. 목록이 비어 있으면 요약문은 "Retiring is safe."라고
말하는데 버튼만 "anyway"라고 말하고 있었다 — 요약과 버튼이 서로 다른 말을 한 셈이다. 라벨을
의존자 수에서 파생시켰다(`n === 0 ? 'Retire' : 'Retire anyway'`). **있지도 않은 위험을 경고하면,
진짜 경고여야 할 때의 무게가 깎인다.**

### 결함이라 오인한 것 3건 — 셋 다 하네스의 그림자였다

이쪽이 더 기록할 값이 있다. 셋 모두 "화면이 잘못됐다"로 보였고, 셋 모두 아니었다.

- **잘려 보인 패널.** New revision 패널이 아래가 잘린 것처럼 찍혔다. `getBoundingClientRect()`로
  재 보니 패널 top 25 / 높이 502, submit 버튼 top 480, 뷰포트 605 — 전부 제자리였다. 스크린샷
  창(480px)이 뷰포트(605px)보다 짧았을 뿐이다. **스크린샷은 뷰포트가 아니다.**
- **눌리지 않던 submit 버튼.** `Add revision`을 클릭해도 네트워크 요청이 없었다. 폼 validity는
  `true`, 버튼 `disabled`는 `false`, 콘솔은 비어 있었다. `form.requestSubmit()`은 즉시 동작했고,
  클릭 직후 `document.activeElement`가 여전히 textarea였다 — **클릭이 버튼에 닿은 적이 없다**는
  뜻이다. 원인은 뷰포트 에뮬레이션(1200×1400)이 클릭 좌표 매핑을 깨뜨린 것이었고,
  `resize_window({preset:'desktop'})` 후에는 같은 버튼이 `ref` 클릭으로 정상 동작했다.
- **통째로 빈 상세 페이지.** `#detail-body`와 `#not-found`가 둘 다 `display:none`이었다. 원인은
  검증용 프록시가 `/api/events/stream`(SSE)을 `resp.read()`로 통째로 읽으려 한 것이다. SSE는
  끝나지 않는 응답이라 그 스레드와 **브라우저의 커넥션 슬롯**이 영구히 붙잡히고, 뒤따르는
  `GET /api/me`가 pending으로 굶는다. 페이지 부트스트랩이 `await loadPermissions(); await
  refresh();`라 첫 줄이 영원히 안 끝나면 두 컨테이너 중 어느 것도 보이지 않는다. 프록시가 SSE를
  chunked로 흘려보내게 고치자 페이지가 전부 그려졌고 SSE 배지도 `live`가 됐다.

### 비밀번호를 브라우저에 넣지 않고 인증된 화면을 보는 법

`fleet_session`은 HttpOnly라 `document.cookie`로 심을 수 없고, 로그인 폼에 비밀번호를 타이핑하는
것은 금지돼 있다. 그래서 bootstrap과 로그인을 전부 `curl`로 하고(CSRF double-submit 포함), 얻은
세션 쿠키를 **요청 헤더로 주입하는 로컬 프록시**를 세워 브라우저를 그 앞에 붙였다. 비밀번호는
브라우저에 한 번도 들어가지 않았다. 프록시는 검증 하네스이지 제품 코드가 아니라 저장소에 넣지
않았다.

### 검증 한계

- **뷰포트 에뮬레이션을 켠 채로는 클릭 검증이 성립하지 않는다.** 위 두 번째 오인이 그것이다.
  좌표 클릭은 에뮬레이션을 끈 뒤에도 신뢰할 수 없었다(스크린샷 800×480이 뷰포트 1008×605의
  축소본이라, Lifecycle 버튼을 겨눈 클릭이 사이드바 링크에 떨어졌다). 실검증은 전부 `ref`
  클릭으로 했다.
- 빈 페이지와 SSE 배지에서 본 이상 동작은 **프록시의 산물이지 앱의 동작이 아니다.** 프록시
  없이(=실배포 경로로) 같은 화면을 본 적은 없다.
- 라이프사이클은 `draft → published → retired`와 revision 생성·revoke, 그리고 서버측
  `?status=retired` 필터까지 실제 클릭으로 왕복했다. `deprecated` 경유 경로와 `discarded`는
  전이표 단위 테스트로만 덮였고 화면에서 밟지는 않았다.

### 부기: "남은 것"을 옮겨 적는 행위는 재확인이 아니다

로드맵 `#92` 행을 갱신하면서 예전 `**남은 것**: AgentTemplate 관리 화면(HTML), Issue의
MCP 표면`을 `**남은 것**(당시)`로 표시하고 새 목록을 썼다. 그런데 새 목록의 첫 항목을
"Issue의 MCP 표면"으로 그대로 옮겼다 — **같은 행의 상태 칸이 이미 `Issue 표면 HTTP·MCP
완료 2026-08-24`라고 적고 있는데도.** 확인은 한 줄이면 됐다:

```bash
grep -rn 'fleet_list_issues\|fleet_transition_issue' crates/fleet-mcp/src/
```

`schema.rs:64-70`에 네 도구가 전부 선언돼 있고 `server.rs:117`이 `IssueRead` capability까지
매핑하고 있다. 즉 **닫힌 항목을 다시 열어 적을 뻔했다.** 두 항목짜리 목록에서 첫 번째만
갚은 것을 확인하고 두 번째는 검증 없이 복사한 것인데, 목록이 짧을수록 이 생략이 눈에 띄지
않는다. 규칙으로 적어 둔다: **`남은 것`을 새 목록으로 옮길 때는 항목마다 코드에 grep을
때린다.** 옮겨 적는 것은 관성이지 판정이 아니다.

같은 자리에서 두 번째 결정도 명시했다. **AgentTemplate의 MCP 표면 부재는 "남은 것"이
아니다.** 이것은 LLM이 자기 role prompt를 고치는 권한 상승 경로라서 의도적으로 만들지
않은 것이고, 이 저장소는 그 규칙을 이미 갖고 있다. 그런데 안 만든 이유를 남은 목록 옆에
적어 두지 않으면, 다음 세션은 HTTP 표면과 MCP 표면의 비대칭을 보고 "빠뜨렸구나"로 읽는다.
**의도적 부재는 적어 두지 않으면 결함으로 읽힌다** — 채울 방법이 없는 것을 미리 만들지
않는다는 원칙의 이면이다. 만들지 않은 것에는 만들지 않은 이유가 붙어 있어야 한다.

## 2026-08-30 — `#67` 귀속 정정: 제어 스트림의 소유자는 `#89`가 아니었다

### 발견: 문서가 만든 순환

`#92`를 닫은 뒤 다음 항목을 고르려고 막힌 항목들의 공통 선행을 훑었다. `#49` 2단계, `#69`,
`#70` 본체, `#86` 후속, `#87`, `#67`의 남은 게이트 셋이 전부 "Worker 제어 스트림이 없다"에서
멈춰 있었고, 그 스트림의 소유자로 **`#89`**가 적혀 있었다 — `provisioning.md`의 유예 목록 9칸
중 6칸, `control-plane-authority-and-failover.md`의 표 4칸, `agent-template.md`, `mcp-tools.md`,
`agent-management.md`, `project-management.md`, `ui-design.md`, `observability-and-reconciliation.md`,
그리고 `crates/fleet-core/src/agent.rs`의 모듈 doc까지.

그런데 `#89`의 로드맵 행은 **선행 `#88`, `#67`**이다. 즉 `#67`은 스트림을 `#89`로 미루고
`#89`는 `#67`을 기다린다. 둘 중 하나는 틀렸고, 그 상태에서는 **두 항목 모두 영원히 착수
불가로 읽힌다.**

### 판정: 오표기는 `#89` 쪽이다

세 가지가 같은 방향을 가리킨다.

1. **`#67`의 항목명이 "Worker execution lease·Agent command ACK"다.** 명령/ACK 계층이 이름에
   이미 들어 있다.
2. **9-필드 명령 봉투와 ACK를 정의하는 문서가 `#67`의 설계 정본이다.** `provisioning.md`
   §"상태와 명령"이 `agent_id`·`request_id`·`generation`·`task_id`·`actor`·`expires_at`·
   `control_epoch`·`worker_incarnation`·`fencing_token` 아홉 개를 규정하는데, 이 문서는
   `#67` 행의 설계 정본 칸에 `control-plane-authority-and-failover.md`와 나란히 적혀 있다.
3. **`#89`의 완료 게이트가 전부 Issue 전용이다.** dedup 부분 유니크 인덱스, `occurrence_count`,
   Attempt당 상한, `AgentIssueFloodSuspected`, 감사 실패 시 거절 — 하나도 transport가 아니다.
   `#89`는 스트림 **위에** "Agent가 Issue를 연다"를 얹는 소비자다.

따라서 새 로드맵 ID를 세우지 않았다. 처음에는 "소유자가 없는 고아 범위"로 읽고 `#99`를
만들 뻔했는데, **분기 하나를 빠뜨린 오독이었다** — 소유자가 없는 경우와 소유자가 오표기된
경우는 증상이 같다(아무도 그 이름으로 착수하지 않는다). 구분하는 방법은 하나뿐이다:
**범위를 정의하는 문서가 누구의 설계 정본인지 본다.** 여기서는 `provisioning.md`가 답을
줬다. 고아였다면 그 문서를 정본으로 든 항목이 없었을 것이다.

### 함께 고친 사실 오류

`worker-liveness-policy.md`는 "현재 Agent start/stop/capture command는 Worker의 heartbeat
polling으로 전달된다"고 적고 있었다. **코드상 사실이 아니다.** `desired_state`를 저장소 전체에서
추적하면 8곳뿐이고, 타입은 `HeartbeatResponse`의 `&'static str` 하나, 값은 `"running"`/`"drain"`
둘, 생산 조건은 CPU/RAM 90% 초과 또는 이미 `Draining`인 워커, 워커측 소비자는 `info!` 로그
한 줄이다. Agent 명령은 한 건도 실리지 않는다.

**다만 이것을 "drain이 고장났다"로 읽으면 과장이다.** `WorkerSelector::select`가
`status: Some(WorkerStatus::Online)`으로 후보를 거르므로 `Draining` 워커는 선택 단계에서
빠진다 — 집행은 오케스트레이터측에서 이미 성립한다. 워커측 no-op이 만드는 실제 공백은
"워커가 스스로 거절하거나 drain 진행을 보고하지 못한다"이지 "drain이 안 걸린다"가 아니다.
처음 읽었을 때 후자로 적었다가 `selector.rs`를 확인하고 고쳤다.

부수적으로 `Draining`이 단방향 함정이라는 것도 확인했다: 하트비트의 상태 승격이
`worker.status != WorkerStatus::Draining` 가드 뒤에 있어 하트비트로는 절대 풀리지 않고,
운영자의 status-set 경로만 되돌린다. 의도된 설계로 보여 손대지 않았다.

### 이 커밋의 범위와 하지 않은 것

문서·주석의 **귀속만** 고쳤다. 동작 변경은 없다. `#67` 4단계(명령 봉투·ACK)의 구현은 이
커밋에 없다 — 범위가 바뀌었으므로 설계 정본을 먼저 맞추고 로드맵을 동기화한다는 순서를
따랐다.

`docs/log.md`의 과거 항목(`3892`행 등)은 고치지 않았다. 이 파일은 append-only이고, 그때
그렇게 판단했다는 것 자체가 기록이다. `docs/reviews/`의 과거 리뷰도 같은 이유로 두었다.
`issues.md:31`은 원래부터 옳았다 — 거기서 `#89`는 Issue 기능의 소유자이고 선행으로 `#67`을
이미 들고 있었다. **전수 치환이 아니라 칸마다 의미를 읽고 골랐다**: `tasks.agent_id`는
transport가 아니라 스케줄러 사실이므로 `#49` 2단계로, Issue 관련 `#89`는 그대로.

### 검증 한계

귀속의 옳음은 **문서 간 정합**으로 판정했지 실행으로 확인할 수 없다. 로드맵 ID는 코드에
주석으로만 나타나므로 테스트가 이 변경을 검증하지 못한다. 대신 각 자리를 고칠 때마다 그
칸이 말하는 대상이 무엇인지 코드에 확인했다(`desired_state` 전수 추적, `Agent` 구조체에
host 필드 부재, `AgentStatus`가 2-상태, `selector.rs`의 `Online` 필터).

### 부기: 게이트 신선도 검사가 볼륨 스큐 때문에 통과를 위조한다

커밋 직전 `agent.md` §4.3의 신선도 검사(소스 mtime < 게이트 로그 mtime)를 돌렸더니
"게이트가 14 296초 나중"이라는 답이 나왔다. **그런데 그 소스 파일들은 몇 분 전에 내가 쓴
것이다.** `date +%s`와 대조해 보니 레포(`/Volumes/Data01`)의 mtime이 약 3.97시간 과거로
찍히고, 스크래치패드(`/private/tmp`)의 게이트 로그는 정확했다. 즉 두 값의 차이는 신선도가
아니라 **볼륨 간 시계 오프셋**이었다.

이 방향의 스큐는 검사를 무력화하는 데 그치지 않고 **뒤집는다** — 레포 mtime이 항상 4시간
과거로 보이므로, 게이트를 돌린 뒤 소스를 고쳐도 "게이트가 나중"이라는 답이 계속 나온다.
`#49` 때 이 검사가 잡아낸 실제 사고(초록이 고치기 *전* 트리의 판정이었던 것)를, 이 환경에서는
그대로 놓치게 된다. 시각 비교로 순서를 **추론**하는 것 자체가 약점이었다.

대응은 비교를 고치는 것이 아니라 비교를 없애는 것이다: **모든 편집을 확정한 뒤 게이트를
다시 돌린다.** 캐시가 살아 있어 재실행 비용이 작고, 순서가 추론이 아니라 사실이 된다.
`agent.md` §4.3의 해당 bullet에 적어 두었다 — 로그에만 적으면 다음 세션이 명령을 만들 때
보지 않는다는 것을 이 저장소는 이미 하루에 두 번 겪었다.

**남은 정직한 한계**: 이 커밋에서도 게이트 재실행 시작 뒤에 편집이 더 있었다. 전부
Markdown(문서 frontmatter의 `last_verified`와 이 로그 항목, `agent.md`)이고, 게이트 명령 중
Markdown을 읽는 것은 없다 — `include_str!`은 저장소 전체에서 `crates/fleet-api/src/openapi.yaml`
한 건뿐이며 그 파일은 이 커밋에서 바뀌지 않았다. 따라서 컴파일·lint·테스트 결과는 그
편집들과 무관하다. 이것은 "게이트를 다시 돌렸다"가 아니라 **"다시 돌릴 필요가 없음을
근거로 보였다"**이며, 두 진술을 섞지 않기 위해 여기 적는다.

## 2026-08-30 — `#67` 4단계 분할: 봉투는 상대를 만들지만 방향을 만들지 못한다

같은 날 오전에 `#67` 4단계를 "9-필드 명령 봉투 + 워커측 ACK"로 정의하면서 이렇게 적었다:
**"선행은 '명령을 받을 상대'가 아니다 — 첫 명령이 `StartAgent`이므로 봉투가 상대를 만든다.
남은 실제 선행은 워커측 프로세스 기동·격리 경로다."** 구현에 들어가기 전 사실 확인을 하다가
그 문장이 절반만 맞았다는 것을 알았다.

맞는 절반: `StartAgent`가 프로세스를 만들므로, "명령을 실행할 프로세스가 없다"는 미룰 사유가
아니다. 틀린 절반: **명령이 갈 방향은 봉투가 만들지 않는다.** `migrations/027_agents.sql`을
직접 열어 보면 `agents`에는 `worker_id`가 없다. 배정이 저장되지 않으므로 "이 Agent에게
`StartAgent`를 보내라"를 **어느 Worker의 응답에 실을지** 결정할 수단이 없다. 정본
(`provisioning.md` §"상태와 명령")은 이미 "Worker는 자신에게 **배정된** Agent와 현재
incarnation만 처리한다"고 적어 배정을 전제하고 있었는데, 그 전제를 채우는 컬럼이 없었다.

### 배정에는 주인이 없었다

배정이 다른 항목의 것이면 4단계는 그것을 기다리면 된다. 확인해 보니 아니었다.
`#49` 2단계의 정의는 "`#48`/`#67` **뒤** Hibernated 단일 Agent E2E; WarmIdle은 별도 flag"이고,
같은 셀이 봉투·ACK·`generation`을 "전부 `#67` 4단계 선행"으로 귀속시켜 두었다(오늘 오전의
귀속 정정). 즉 `#49`는 `#67` 뒤에 오며 배정을 소유하지 않는다. 배정은 **주인이 없는 채로
4단계 안에 암묵적으로 들어 있었다**.

그래서 4단계를 셋으로 나눴다. **4a** 배정(`agents.worker_id`), **4b** 수렴 프로토콜
(desired state + generation + ACK), **4c** 워커측 1:N 프로세스 매니저. 넷 중 하나라도
빠지면 나머지가 도달 불가가 되는 관계가 아니라, 4a → 4b → 4c 순으로 각자 관측 가능한
결과를 내는 관계다.

### 명령 큐 대신 desired-state 수렴

같이 정한 것이 하나 더 있다. 봉투의 9개 필드를 늘어놓고 보면 대부분이 **명령의 속성이
아니라 `agents` 행의 상태**다 — `agent_id`는 행 자신이고 `generation`/`control_epoch`/
`worker_incarnation`/`actor`는 컬럼이며 `task_id`는 그 Agent가 지금 무엇을 위해 존재하는지다.
그래서 명령 큐 테이블을 만드는 대신 행에 `desired_status`·`command_generation`·
`last_acked_generation`을 두고, heartbeat 응답이 그 Worker에 배정된 Agent들의 desired state를
싣고 Worker가 같은 generation으로 ACK하기로 했다.

이 형태를 고른 이유는 크기가 아니라 **정본의 요구가 공짜로 성립하기 때문**이다. "control
plane은 CAS로 ACK를 반영하므로 지연된 ACK가 새 상태를 덮어쓰지 못한다"는
`WHERE command_generation = $ack_generation` 한 줄이 된다. 큐 모델이었다면 같은 성질을 따로
구현해야 했다. 덤으로 `expires_at`이 대부분 해소된다 — desired state는 매 beat 재전송되므로
"오래된 명령이 뒤늦게 실행되는" 창이 열리지 않고, 신선도 판정은 `command_generation`이 한다.
그리고 이 선택은 새 발명이 아니라 **기존 필드의 연장**이다: `HeartbeatResponse.desired_state`가
이미 오케스트레이터 → 워커의 유일한 명령면으로 존재한다.

### 이 증분에서 만들지 않기로 한 것과 그 사유

| 만들지 않는 것 | 사유 | 언제 |
|---|---|---|
| `fencing_token` (봉투를 9-필드가 아닌 **8-필드**로) | `worker_execution_lease` 테이블이 **존재하지 않는다**. migration 018~029에 없고 `021_control_plane_lease.sql`은 오케스트레이터 리더 선출용의 다른 테이블이다 — 즉 생산자가 없다 | 구현 게이트 ① |
| 워커가 신고하는 `max_agent_processes` | 프로세스 매니저가 없는 동안 워커는 자기 상한을 **집행할 수 없다**. 집행되지 않는 숫자를 신고받는 것은 "항상 NULL인 컬럼"의 뒤집힌 형태다 | 4c |
| ACK가 Agent process의 endpoint·secret을 돌려주는 것 | 소비자가 없고(Task를 Agent로 라우팅하는 것은 `#49` 2단계), secret을 한 번도 나른 적 없는 경로에 secret을 새로 얹게 된다 | `#49` 2단계 |
| `Draining` | Worker의 `Draining`이 operator 개입 없이는 되돌아오지 않는 일방향 문이다(`fleet-api/src/handlers.rs`). 그 모양을 Agent가 물려받을 이유가 없다 | 4c 이후 |

### `on_demand`는 이 단계가 풀지 못한다

`WorkerLivenessMode::OnDemand`의 doc은 스스로 "`on_demand`로 설정된 워커에 실제로 task를
배정하는 로직은 이 증분의 범위 밖(로드맵 `#67` 의존)"이라고 적어 `#67`을 해소처로 지목한다.
그런데 4b가 명령을 heartbeat에 싣는 이상, **idle에 heartbeat을 보내지 않는 워커에는 desired
state가 원리적으로 도달하지 않는다**. 실제로 `WorkerSelector`는 이미 같은 이유로 `on_demand`
워커를 Task 후보에서 빼고 `SelectionError::AllUnprobed`를 낸다. 해소하는 것은 명령 전달이
아니라 **dispatch 직전 ACP probe**(`#70`)이며, `provisioning.md`에 그렇게 적었다.

### 이 커밋의 범위

문서만 바꿨다. `docs/roadmap/roadmap.md`의 `#67` 셀(4단계 → 4a/4b/4c)과 `#49` 셀의 상호
참조, `docs/architecture/agents/provisioning.md`의 §"상태와 명령"(배정 선행·수렴 형태·
`on_demand` 한계)과 유예 목록이다. 코드는 한 줄도 바꾸지 않았으므로 이 커밋의 검증은
문서 정합성뿐이고, 4a의 구현·테스트는 다음 커밋이다.

## 2026-08-30 — `#67` 4a 배정: 절반만 채워진 상태를 타입과 스키마가 함께 막았다

4a는 "명령이 갈 방향"만 만드는 단계다. 명령 자체는 4b, 그 명령을 받아 프로세스를 띄우는
쪽은 4c이므로, 이 단계가 끝나도 Agent 프로세스는 여전히 하나도 뜨지 않는다. 그래서 이 증분의
설계 판단은 대부분 **"지금 채울 수 없는 것을 만들지 않는다"의 적용**이었다.

### `worker_id`와 `assigned_at`은 both-or-neither이며, 그것을 세 겹이 지킨다

한쪽만 채워진 행은 "배정됐는데 언제인지 모른다" 또는 "언제인지는 아는데 어디인지 모른다"이고,
둘 다 읽는 쪽이 해석할 방법이 없다. 그래서 `030_agent_placement.sql`에 `CHECK`를 걸었다.

그런데 `ON DELETE SET NULL`은 `worker_id`만 비운다 — **FK가 스스로 `CHECK`를 깨뜨린다.**
`BEFORE UPDATE OF worker_id` 트리거가 `assigned_at`도 함께 비워 그 자리를 막는다. 이 트리거가
FK가 유발한 UPDATE에서도 발동하는지는 추론하지 않고 확인했다: `createdb -T`로 `fleet_test`를
복제하고 030을 적용한 뒤 롤백되는 트랜잭션 안에서 다섯 가지(한쪽만 → CHECK 위반, 양쪽 NULL/
양쪽 채움 → 허용, `worker_id = NULL` UPDATE → `assigned_at`도 비워짐, `DELETE FROM workers` →
양쪽 비워짐)를 밟고 `dropdb`했다.

**`fleet_test`에 적용해서 확인하지 않은 이유가 중요하다.** `sqlx::migrate!`는 파일 전체를
체크섬하므로, 한 번 적용된 마이그레이션은 그 시점부터 **주석 한 줄도** 고칠 수 없다. 검증하려고
적용하면 검증 결과로 고칠 자유를 잃는다.

세 번째 겹은 Rust 쪽이다. `place_on_create`는 처음에 `Option<WorkerId>`를 돌려주게 썼는데,
`fleet-mcp`에 `chrono` 의존이 없어 호출부에서 `chrono::Utc::now()`를 부를 수 없었다(E0433).
의존을 추가하는 대신 **반환형을 `Option<(WorkerId, DateTime<Utc>)>`로 바꿨다** — 시각이
`fleet-scheduler` 안에서 만들어지고, 호출자가 쌍을 쪼갤 방법이 사라진다. 빠진 의존이 API를
개선한 경우다.

### `AllUnprobed`가 죽은 분기가 아님을 먼저 증명했다

에러 variant도 "채울 방법이 없는 것은 만들지 않는다"의 대상이다. 세 호출 지점을 읽어 확인했다:
`fleet-api`의 `build_worker`가 `liveness_mode`와 무관하게 `WorkerStatus::Online`을 쓰고,
`HealthChecker`(`health.rs:152`)는 on_demand 워커를 **명시적으로 건너뛴다**("Online도 Offline도
아니라 '판단하지 않음'"). 따라서 on_demand 워커는 등록 후 영원히 `Online`으로 남고, 배정
필터의 on_demand 가지에 실제로 도달한다.

메시지 문구는 `SelectionError::AllUnprobed`와 **일부러 같게** 뒀다. 같은 조건에 두 이름을 주면
두 표면의 로그를 함께 보는 운영자가 서로 다른 원인이라고 읽는다.

### 생성은 배정 실패로 막히지 않고, 그 대신 `NULL`이 비종단이다

Agent 정의가 Worker 가용성에 인질로 잡히면 "Worker를 붙이기 전에 Project를 설계한다"는 정상
사용이 막힌다. 그래서 후보가 없어도 생성은 200이다. 대신 회복 경로 둘을 뒀다 —
Dashboard `POST /api/agents/{id}/place`와 MCP `fleet_place_agent`. 여기서 "후보 없음"은
**409**이지 500이 아니다: 서버 결함이 아니라 지금 fleet의 상태이며, 500으로 답하면 운영자가
오케스트레이터를 의심하게 된다.

배정은 별도 UPDATE가 아니라 **생성과 같은 INSERT**에 실린다. 별도였다면 중간 실패가 "아무도
고치지 않을 미배정 행"을 남긴다.

### 감사 이벤트는 생성 이후의 배정 변경만 센다 — 실검증이 문구를 고쳤다

`agent.assign`에 `previous_worker_id`를 함께 남겨 "어디로 갔는가"와 "어디에서 왔는가"를 한
이벤트가 답하게 했다 — 없으면 앞 이벤트를 거슬러 올라가야 출발지를 안다. 그리고 **생성 시
배정은 `agent.assign`을 내지 않는다.** `agent.create`의 detail에 실어 두면 생성이 그 수를
부풀리지 않는다.

설계 문서에는 이것을 "`agent.assign` 건수가 정확히 **이동 횟수**"라고 적어 뒀는데, **실검증이
그 문장을 반증했다.** `NULL`에서 회복하는 명시 배정도 같은 이벤트를 내므로(그때
`previous_worker_id`가 `null`이다) 정확한 이름은 "생성 이후의 배정 변경 횟수"다. 실측(임시 DB
`fleet_verify_4a`): Agent 3개 · 명시 배정 1회에 대해 `agent.create` 3건 · `agent.assign` 1건,
그리고 생성 시 배정된 Agent의 `agent.create` detail에만 `worker_id`가 실려 있었다.
`provisioning.md`의 그 줄을 실측과 함께 고쳤다.

**이 오류는 테스트가 잡을 수 없는 종류다.** 테스트는 "`agent.assign`이 1건 나온다"를 확인하지,
그 1건을 **뭐라고 부를지**는 확인하지 않는다. 코드는 처음부터 옳았고 틀린 것은 설명이었다.

### 만들지 않은 것

| 만들지 않은 것 | 사유 | 언제 |
|---|---|---|
| 하드 상한 | `workers.max_concurrent`는 Task 동시성 상한이지 Agent 프로세스 상한이 아니다. 후자를 집행할 프로세스 매니저가 없다 | 4c |
| 배경 재조정기 | 배정은 관측이 아니라 **결정**이고, 4a에는 옮길 프로세스가 없어 재조정 대상이 없다. 인라인이라 감사 actor가 실재하는 사람이고, 두 오케스트레이터가 같은 Agent 행을 두고 경합할 경로가 없어 `lease_allows_control()` 게이트도 불필요하다 | — |
| `unassign_agent_worker` | 원장이 `worker_id IS NOT NULL AND status <> 'stopped'`만 세므로 회수가 자리를 자동으로 비운다 — 생산자가 없다 | — |
| `HalfOpen` 배제 | dispatch의 `is_open()`과 같은 술어를 쓴다. 여기서만 보수적이면 같은 Worker가 Task는 받고 Agent는 못 받는 설명 불가능한 상태가 된다 | — |

### 검증 한계

1. **회로 상태가 한 쓰기만큼 뒤진다.** 출처가 인메모리 `BreakerRegistry`가 아니라
   `workers.circuit_state` 컬럼이다 — Dashboard의 `DashboardState`에는 `FleetState`가 없어
   강제된 선택이다. `Store::update_worker_circuit_state`가 no-op 기본 구현이라 MemStore 기반
   테스트는 `Worker::circuit_state`를 직접 세팅한다.
2. **원장을 읽고 INSERT하기까지가 원자적이지 않다.** 동시에 두 Agent를 만들면 같은 Worker를
   고를 수 있다. 상한이 없는 지금은 부하 분포가 잠시 기우는 것으로 끝나지만, 상한이 생기는
   순간 초과 배정이 된다 — CAS slot claim을 구현 게이트 ①로 남긴다.
3. Worker 삭제 시 배정이 풀리는 것은 `MemStore`가 흉내 내지 않는다(`ON DELETE SET NULL`은
   애플리케이션 코드가 아니다). 그래서 그 동작은 **실제 Postgres 통합 테스트로만** 증명된다.

### `MemStore`가 FK를 흉내 내야 했던 이유

`MemStore::assign_agent_worker`는 존재하지 않는 Worker를 그대로 받아들이고 있었다. PgStore에서는
FK가 `Conflict`로 잡는 자리다. 흉내 내지 않으면 "없는 Worker 지목 → 400"을 검증하려던 대시보드
테스트가 **성공 경로를 밟고도 초록**이 된다 — 검증하려던 분기를 한 번도 실행하지 않은 채다.
`agent.md` §4.3이 반복 기록해 온 "게이트가 CI보다 약하면"의 저장소 판이다.

흉내는 **판정 순서까지** 맞춰야 했다. 처음 쓴 코드는 Worker 존재 검사를 Agent 조회보다 앞에
뒀는데, PgStore의 UPDATE는 Agent가 없으면 0행을 갱신하고 FK 검사에 **닿지도 않는다** — 둘 다
없을 때의 답은 `Ok(false)`(→ 404)이지 FK 위반(→ 400)이 아니다. 두 표면 모두 Agent를 먼저
조회하므로 API로는 도달하지 않는 차이지만, 그 자리의 주석이 "FK를 미러링한다"고 말하는 이상
말과 코드가 어긋난 채로 둘 수 없었다. 잠금을 중첩하지 않으려고 존재 여부는 미리 계산하고
판정만 `Some(a)` 가지 안으로 옮겼다.

다만 흉내에는 선을 그었다: 트리거·FK 액션까지 재현하기 시작하면 MemStore가 두 번째 DB
구현이 된다(위 한계 3).

### 파생 `Default`가 `limit: 0`을 만들어 세 테스트를 조용히 틀리게 했다

게이트의 test 단계에서 세 타깃이 깨졌는데, 그중 둘의 모양이 원인을 가리켰다:
`fleet-scheduler`의 `stopped_agents_do_not_hold_a_slot`이 `left: 1, right: 2`,
`fleet-store`의 `list_agents_filters_by_worker`가 `left: 1, right: 3` — **다른 크레이트, 다른
Store 구현인데 둘 다 `left`가 1**이었다. 서로 무관한 로직이 같은 답을 틀리게 낼 리는 없고,
공유하는 상한이 있다는 뜻이다.

`AgentFilter`가 파생 `Default`를 쓰고 있었고, 그래서 `..Default::default()`는 `limit: 0`을
만든다. 그리고 두 Store 모두 0을 **조용히 1로 올린다** — `MemStore`는 `filter.limit.max(1)`
(`mem.rs:1752`), `PgStore`는 `filter.limit.clamp(1, 1000)`(`postgres.rs:2939`). 오류도 아니고
빈 목록도 아니라서, `..Default::default()`가 "필터 없음"이 아니라 **"첫 한 행만"**을 뜻하게
된다. 호출자가 알아차릴 방법이 없다.

고칠 자리는 테스트가 아니라 원천이었다. 테스트만 고치면 다음 호출자가 같은 함정을 다시 밟는다.
`AgentFilter`에서 파생 `Default`를 떼고 손으로 쓴 `impl Default`(limit 100)를 넣었다 —
`TaskFilter`·`WorkerFilter`·`AuditFilter`가 같은 이유로 이미 그렇게 돼 있고 값도 100이다.
프로덕션 호출자 중에 `..Default::default()`를 쓰는 곳이 없음을 확인했으므로(대시보드는
`limit: 1000`, MCP는 명시 `limit`/`offset`), 이 수정의 영향은 테스트 범위에 머문다.

같은 함정이 `AgentTemplateFilter`·`IssueFilter`·`ProjectFilter`에 남아 있다.
`crates/fleet-store/tests/issues.rs`의 세 곳(145·166·177)이 이미 `..Default::default()`를
쓰고 있어 **결과가 한 행으로 잘린 채 초록일 수 있다** — 별도 작업으로 남긴다.

셋째 실패는 단순했다: `fleet-mcp`의 `all_tools_includes_list_tasks`가 도구 개수 22를 단정하는데
4a가 `fleet_place_agent`를 더해 23이 됐다. 단정을 23으로 올리고 사유를 주석에 적었다.

### 게이트 두 개를 동시에 돌려서 판정을 잃을 뻔했다

게이트 도중 `gate67a.sh` 프로세스가 **둘**(PID 80912·41892) 떠 있는 것을 발견했다. 두 가지가
동시에 깨진다. (1) 두 실행이 같은 `fleet_test` DB를 쓰는데, `fleet-store` 통합 테스트의 격리는
각 테스트 시작 시 `TRUNCATE ... CASCADE`다 — `-- --test-threads=1`은 **한 바이너리 안에서만**
직렬화하므로 다른 실행의 TRUNCATE를 막지 못한다. (2) 둘 다 같은 로그에 append하므로, 먼저 시작한
쪽이 뒤에 시작한 쪽의 로그에 `test(no-default) exit=`과 `DONE`을 써 넣는다 — **섞인 판정이
완료 표식을 쓰고 있는** 로그가 된다.

`agent.md` §4.3이 "게이트와 CI가 다르면 어느 쪽이 엄격하든 로컬 판정은 CI 판정이 아니다"로
적어 온 것의 또 다른 얼굴이다. 여기서는 게이트가 **자기 자신과도** 달랐다.

둘 다 죽이고 `gate67a.sh`에 `mkdir` 기반 배타 락(`trap`으로 해제)과 단계별 타임스탬프를 넣은
뒤 한 번만 돌렸다. 산문이 아니라 **스크립트에** 넣은 것은, 다음 세션이 읽는 것이 로그가 아니라
실행 목록이기 때문이다(`agent.md` §3.2가 같은 이유로 하루에 두 번 재발한 사례를 적고 있다).

진행 중이던 `test(acp mtls)` 관측은 오염 가능성 때문에 버렸다. 다만 위 세 건의 코드 수정은
유지했다 — 오염이면 `left`가 실행마다 다른 값으로 나오는데(§3.2), 이 셋은 결정적이었다.

### 게이트와 실검증

배타 락 아래에서 최종 트리로 한 번 완주했다. `rustc 1.98.0`, `RUSTFLAGS="-D warnings"`,
`fmt` → `clippy(acp mtls)` → `clippy(no-default)` → `build(acp mtls)` → `test(acp mtls)` →
`build(no-default)` → `test(no-default)` 일곱 단계 전부 `exit=0`.

**조용한 skip이 없음은 개수가 아니라 소요 시간으로 확인했다**(§4.3 (3)). 두 피처 세트 모두
74 suite 전부 ok(1200건 / 1196건)였고, `cross_client`가 `0.00s`가 아니라 각각 6.11s · 5.73s로
15건을 돌았다 — subprocess로 `target/debug/fleet`를 실제로 띄웠다는 뜻이다. `running 0 tests`
8건은 전부 doc-test 7개와 테스트가 없는 `fleet-worker` 바이너리 타깃이다.

브라우저 실검증은 임시 DB(`fleet_verify_4a`)에 서버를 띄워서 했다. 로그인은 폼에 타이핑하지
않고 로컬 API 호출로 세션 쿠키를 받아 브라우저에 주입했다. Project 상세의 Agent 표가 헤더와
두 행 모두 동일한 6열 그리드 트랙을 쓰는 것, 배정된 Agent가 UUID 앞 8자(`e791a1bb`)를 보여주고
`title`에 전체 UUID를 담는 것, 미배정 Agent가 `—`인 것을 확인했다. `POST /api/agents/{id}/place`는
200(미배정 → 배정) · 409(후보 없음) · 404(없는 Agent)를 냈다. 검증 후 서버를 내리고 임시 DB를
`dropdb`했다.

여기서 실검증만 드러낸 사실이 하나 있다: DB에 직접 꽂은 Worker는 하트비트가 없어
`HealthChecker`가 곧 `offline`으로 강등시키므로, 배정 후보로 남기려면 `status`와 함께
`last_seen`도 갱신해야 한다. 처음 두 번의 `place` 시도가 그래서 409였다.

### 곁가지: `#agent-table`은 처음부터 컬럼 정렬이 없었다

`styles.css`는 테이블별 `grid-template-columns`를 ID로만 정의하는데 `#agent-table` 규칙이 없어,
`#49` 1단계 이래 데스크톱에서도 세로로 쌓여 있었다. 같은 누락이 이 저장소에서 세 번째다
(`#activity-table`/`#key-table`/`#user-table` 묶음 주석이 앞선 두 번을 적고 있다). Worker 열이
붙어 5열 → 6열이 되는 이번이 고칠 자리라 함께 넣었다.

## 2026-08-31 — `#67` 4b 수렴 프로토콜: 큐가 아니라 상태를 보내면 늦은 ACK가 공짜로 막힌다

`#67` 4단계의 두 번째 조각으로 Agent 명령 전달·ACK 경로를 넣었다. 4a가 "Agent를 Worker에
얹는다"까지였다면, 4b는 "그 Worker에게 무엇을 하라고 말하고, 말이 도착했는지 안다"이다.

**큐를 만들지 않았다.** `agents` 행에 `desired_status`·`command_generation`·
`last_acked_generation` 세 컬럼(migration `031_agent_desired_state.sql`)을 올리고, 하트비트가
매 박동마다 **명령 목록 전체를 다시 보낸다**. 워커는 같은 세대를 되돌려 ACK하고, 서버는
`WHERE command_generation = $ack AND worker_id = $worker AND last_acked_generation < $ack`
CAS로만 반영한다. 이 모양을 고른 이유는 재전송 로직이 필요 없어서가 아니라 — 그건 부수
효과다 — **"지연된 ACK가 새 상태를 덮어쓰지 못한다"가 별도 코드 없이 성립하기 때문이다.**
큐였다면 중복 소비·순서 역전·유실을 각각 막아야 했고, 세 방어는 서로를 알지 못해 하나만
빠져도 조용히 뚫린다. 수렴 프로토콜에서는 셋이 같은 술어 하나로 접힌다. 세대를 되돌린
ACK는 `last_acked_generation < $ack`가 거짓이라 무시되고, 다른 워커의 ACK는 `worker_id`가
거짓이라 무시되며, 유실된 ACK는 다음 박동이 같은 목록을 다시 실어 스스로 복구한다.

**`Starting`은 컬럼이 아니라 함수다.** `(status, desired_status)`의 순수 함수로
`Agent::is_starting()`과 `AgentSummary.is_starting`에 노출했다. 상태를 하나 더 저장하면
그 값이 두 컬럼과 어긋나는 네 번째 상태가 생기는데, 그것을 만들 방법이 코드에 있다는
것만으로 "절반만 채워진 상태"의 자리가 열린다(4a가 타입과 스키마로 함께 막았던 바로 그
자리다). 마찬가지로 `command_delivered`도 클라이언트에게 두 세대 값을 빼서 비교하라고
맡기지 않고 서버가 파생해 내보낸다 — 비교식이 클라이언트마다 갈라지면 계약이 아니다.

**`command_delivered`는 전달이지 수렴이 아니다.** 이름이 약속하는 것보다 적게 말한다.
기저 상태에서 `0 == 0`이므로 **명령을 한 번도 내린 적 없는 Agent가 `command_delivered:
true`로 보인다.** 이것은 버그가 아니라 이름의 한계이고, 그래서 실검증에 그 단정을
**명시적으로** 넣었다(`baseline command_delivered true (0 == 0, 명령한 적 없음)`).
넣지 않았다면 "명령 후 false"라는 체크와 "기저에서 true"라는 사실이 서로를 조용히
반증하는 자리로 남았을 것이다.

**생성 시 배정은 세대를 올리지 않는다 — 4c가 다시 만날 비대칭.** `create_agent_api`는
`place_on_create` → `agent.with_placement(...)` → `store.create_agent(&agent)` 경로로
가는데, 이 INSERT는 `agent.command_generation`을 그대로 넣는다(`postgres.rs:2888`).
세대를 올리는 것은 `assign_agent_worker`뿐이다(`postgres.rs:3030`). 즉 논리적으로 같은
"배정"에 코드 경로가 둘이고 세대 부수효과는 한쪽에만 있다. 지금은 생성 직후 목표가
`stopped`라 문제가 없지만, 4c가 재배정을 다루면 이 비대칭이 그대로 드러난다.

### 게이트

7단계 전부 `exit=0`(`rustc 1.98.0 (88d9e12ae 2026-08-18)`): fmt → clippy(acp mtls) →
clippy(no-default) → build(acp mtls) → test(acp mtls) → build(no-default) →
test(no-default). 테스트는 양쪽 피처 세트 모두 **74 suite / 0 실패**, 통과 건수는
`acp mtls` 1221, `no-default` 1217 — 4a 기준선(1200 / 1196) 대비 정확히 **+21**로 4b가
넣은 테스트 수와 일치한다.

**조용한 skip은 개수가 아니라 소요 시간으로 배제했다**(§4.3 (3)). `cross_client`가
`15 passed ... finished in 6.19s`(acp mtls) / `4.94s`(no-default)로 실제 subprocess를
띄웠다. `finished in 0.00s`로 찍힌 항목은 전부 열거해 doc-test(0건), `fleet_worker`
바이너리 타깃(0건), env 게이트된 `e2e_with_real_grok`(0건), 그리고 진짜로 빠른 순수 단위
suite(`verify_examples`, `verify_env_example`, `fleet_credentials` lib, `fleet` CLI main)
임을 확인했다 — subprocess에 의존하는 것은 하나도 없다.

### 실검증 (21/21)

임시 DB `fleet_verify_4b`에 `fleet serve`를 띄우고 부트스트랩 → 프로젝트·Agent 생성 →
대시보드 API로 확인했다. 기저 5건(`desired_status=stopped`, 세대 0, ACK 0,
`command_delivered=true`, `is_starting=false`), 미배치 Agent에 `start` → 200, `AgentSummary`
신규 5필드 존재, `desired_status=running`·세대 1·`command_delivered=false`·
`is_starting=true`, 두 번째 `start`도 200이고 **세대가 1 그대로**(멱등), `agent.start`
감사 행 정확히 1건, 없는 Agent → 404, CSRF 헤더 없음 → 403, 회수된 Agent에 `start` →
400. 서버 종료와 `dropdb`까지 정리했다.

**하네스가 다섯 번 거짓 초록에 근접했다** — 전부 검증 대상이 아니라 검증 도구의 결함이다.
(1) 피처 세트를 바꾸면 `fleet-cli` 빌드 캐시가 통째로 무효화돼 7분을 태우고 serve에
도달하지 못했다. `fleet-dashboard`가 `fleet-cli`의 **무조건** 의존이라(`Cargo.toml:49`,
`#[cfg(feature)]` 가드 없음) 이미 있는 `--no-default-features` 바이너리로 충분했다.
(2) 서버 stdin에 `/dev/zero`를 물리면 stdio MCP 리더에 NUL이 무한히 들어간다 — `sleep`이
붙잡은 FIFO로 바꿔 EOF도 입력도 없게 했다. (3) 토큰 추출이 12자를 집었다: `grep -o ... |
head -1`이 상자보다 먼저 나오는 INFO 로그의 `token_prefix=fleet_boot_H`에 맞았다. 최장
일치로 골라 54자를 얻었다. (4) 부트스트랩 폼 필드가 `username`이 아니라 `email`이고,
`validate_password`가 그 이메일을 유사도 컨텍스트로 받는다. (5) 세션 쿠키가 두 번
사라졌다 — 먼저 `Secure` 플래그 때문에 파이썬 `http.cookiejar`가 평문 HTTP로 **저장은
하되 전송하지 않았고**(서버 로그의 `fleet_csrf cookie is missing`이 진짜 이유를 말해줬다;
클라이언트 쪽만 보면 "쿠키 있음"이라 멀쩡해 보인다), 수동 전송으로 바꾼 뒤에는 부트스트랩
성공 경로가 `Redirect`라 `Set-Cookie`가 **302에** 실렸는데 urllib이 그 중간 응답을 내부에서
소비해 최종 응답만 본 내 흡수 코드가 놓쳤다. 리다이렉트 응답까지 흡수하는 핸들러를 심어
고쳤다. **다섯 결함 모두 "실패"가 아니라 "그럴듯한 부분 성공"으로 나타났다** — (5)는
`bootstrap status: 200`을 찍고도 세션이 없었다.

### 검증 한계 (정직하게)

- **4b가 증명한 것은 전달이지 수렴이 아니다.** 워커측 프로세스 매니저가 아직 없으므로
  실제로 ACK를 보내는 것은 테스트 하네스뿐이다. `last_acked_generation == command_generation`은
  "명령이 도착하고 수용됐다"까지만 말하고, "Agent 프로세스가 실제로 그 상태다"는 말하지
  않는다. 그것은 4c의 몫이다.
- 실검증은 단일 오케스트레이터·단일 워커 시나리오다. 늦은 ACK가 CAS에 걸려 무시되는
  경로는 **단위 테스트로만** 덮었고, 실제 두 워커가 경합하는 상황은 재현하지 않았다.
- 하트비트 재전송이 자기 복구한다는 성질도 단위 테스트 수준이다 — 실제로 ACK를
  유실시켜 다음 박동이 복구하는 것을 관측하지는 않았다.

## 2026-08-31 — `#67` 4c-A 워커측 프로세스 매니저: 분할선은 마이그레이션 경계에 있었다

4b까지 오케스트레이터는 "이 Agent가 running이기를 바란다"를 기록하고 Worker에게 전달했다.
그 명령을 받아 **실제로 프로세스를 움직이는** 것이 4c다. 그런데 4c를 통째로 잡으려니 범위가
계속 커져서, 먼저 어디서 잘라야 하는지를 정했다.

### 왜 다시 나눴는가

관측 상태(`Starting`/`Running`/`Failed`)를 오케스트레이터에 실으려면 `027`의
`status IN ('ready','stopped')` CHECK를 넓혀야 한다. 그리고 CHECK를 넓히는 순간 그 값들에는
**생산자와 경로가 같은 커밋 안에 있어야 한다** — 매니저(생산자), 관측을 싣는 ACK, store의
적용 경로, 그리고 표면 3종(API·MCP·대시보드). 하나라도 빠지면 아무도 만들지 않는 상태값이
스키마에 남는다. 그 대가는 이미 치러 봤다: `#70`이 죽은 `FailureKind` 3종을 제거했고,
`#48`·`#49`가 각각 목표 상태 집합을 3-상태·2-상태로 줄인 것도 같은 판단이었다.

그래서 **분할선을 마이그레이션 경계에 놓았다.** 4c-A는 마이그레이션을 만들지 않는다 —
매니저·설정·파생·수렴·정리까지. 4c-B가 CHECK 확장, 세 상태값, 관측을 싣는 ACK, store 경로,
표면 3종, 그리고 `is_starting` → `start_pending` 개명을 한 번에 한다.

### 매니저는 singleton을 대체하지 않는다

처음에는 `GrokRunner`를 Agent별로 쪼개는 그림을 생각했다. 그런데 그 singleton은
`workers.endpoint`로 등록된 **이 Worker 자신의 ACP 종단**이고, 모든 Task dispatch가 그리로
간다. 쪼개는 순간 dispatch는 `tasks.agent_id` 라우팅(`#49` 2단계)이 생기기 전까지 갈 곳을
잃는다. 그래서 매니저는 옆에 선다.

### 오케스트레이터가 모르는 값은 워커가 파생한다

4b가 명령 페이로드를 `(agent_id, desired_status, generation)`으로 못박았으므로 포트·secret·cwd는
페이로드에 없다. 셋 다 워커 로컬 파생으로 정했다. 포트 범위 기본값 `2420-2519`는 singleton
기본 포트 `2419` **바로 다음**이라, 겹치지 않는다는 사실이 값을 읽는 것만으로 확인된다.
secret은 Agent마다 새로 만든다 — `grok.secret` 재사용은 그 값이 이미 Worker 종단을 여는
열쇠이기 때문에 금지다. **secret 하나가 여는 범위는 하나여야 한다.**

### 거절은 한 자리에서

포트 소진과 `max_agent_processes` 초과는 원인이 다르지만 결과가 같다 — "이번 beat에 뜨지
않았다". 4c-B에서 둘 다 같은 관측 상태로 접힐 것이므로, 이름을 둘로 나눠 보고하면 운영자는
서로 다른 두 오류를 보고 같은 처방을 찾게 된다. 한 자리에서 거절하고 원인은 로그 필드로
남긴다.

### 종료 규약을 읽다가 이름이 거짓말하는 것을 발견했다

`grok_process::terminate_child`의 한 줄 요약이 "SIGTERM → 5초 대기 → SIGKILL"이었는데,
본문은 SIGTERM을 **보내지 않는다**. 이 크레이트는 `#![forbid(unsafe_code)]`이고 `libc::kill`은
unsafe, `nix`는 의존성에 없다. `start_kill()`은 Unix에서 곧바로 SIGKILL이다. 즉 실제 규약은
"자식이 스스로 끝나기를 5초 기다린 뒤 죽인다"이고, `grok agent serve`는 끝날 이유를 통보받지
못하므로 그 5초를 **항상 다 쓴다**.

singleton 하나일 때는 종료 경로에 한 번 붙는 고정 비용이라 눈에 띄지 않았다. Agent가 여럿이면
곱해진다 — 상한 4개면 20초, 그동안 `reconcile`은 프로세스 맵의 lock을 쥔 채 반환하지 않고,
heartbeat 간격이 15초이므로 beat을 통째로 건너뛴다. 그래서 정리를 `terminate_all`로 묶어
동시에 돌렸다. **이것은 성능 조정이 아니라 정확성 쪽에 가깝다**: 지연이 주기를 넘기면 Worker가
살아 있는데도 명령을 받지 못하는 구간이 생긴다.

주석은 고쳤다. 보내지 않는 신호를 보낸다고 적힌 한 줄이 남아 있었다면, 다음 사람은 5초가 왜
항상 걸리는지를 엉뚱한 데서 찾았을 것이다.

### `Option<Vec>`이 처음으로 하중을 받았다

4b가 `HeartbeatResponse.agents`를 `Option<Vec>`으로 둔 결정이 여기서 시험 가능해졌다. 매니저는
목록을 **권위 있는 전체 집합**으로 읽고 "목록에 없는 것은 정리한다"를 하므로, `None`을
`unwrap_or_default()`로 접는 순간 store 조회가 실패한 beat 하나가 그 Worker의 Agent를 전부
죽인다. 테스트 두 건이 이 둘을 반드시 구분해서 확인한다 — 한 건만으로는 구분이 무너졌는지
알 수 없다.

### 검증 한계 (정직하게)

- **오케스트레이터는 여전히 전달까지만 안다.** 프로세스가 실제로 떴다는 사실은 워커 로그에만
  남는다. 즉 4c-A에서 **거절은 조용한 실패다** — 보고할 채널이 없다. 4c-B가 그 채널이며, 이
  한계를 미리 적어 두는 이유는 4c-B가 이것을 모르고 물려받지 않게 하기 위해서다.
- 테스트 12건은 전부 **가짜 grok 스크립트**(`sleep 300` / `exit 3`)를 상대로 한 것이다.
  프로세스 수명·수렴·거절은 검증했지만 실제 `grok agent serve`를 여럿 띄워 본 적은 없다.
- `free_port`의 bind 프로브와 자식의 실제 bind 사이에 TOCTOU 창이 남는다. 닫으려면 자식에게
  소켓을 물려줘야 하는데 `grok agent serve`에 그 인터페이스가 없다.
- 포트 소진·상한 거절과 4a 원장의 불일치(원장은 "배정됐다"고 세고 Worker는 "못 띄운다"고
  본다)는 4a가 이미 게이트 ①로 미뤄 둔 CAS slot claim과 **같은 창**이다. 새 창이 아니다.
- `on_demand` Worker에서의 수렴은 시험하지 않았다 — 그 Worker는 heartbeat 루프를 시작하지
  않고, 4a의 배치 선택기가 이미 후보에서 제외한다. 현재 도달 불가능한 조합이다.

## 2026-08-31 — 예산은 잘못 지목한 비용을 따라잡지 못한다

`fleet-worker`의 `disk_cache_get_or_schedule_refresh_populates_background`가
`--no-default-features` 세트에서만 간헐 실패했다. 격리 재실행은 148.87초 실패 / 122.02초 통과 /
130.51초 실패로 갈렸다. 테스트에 붙은 주석은 원인을 "이 프로세스에서
`Disks::new_with_refreshed_list()`의 첫 호출이 macOS에서 수십 초"라고 적고 있었고, 예산은 그
전제 위에서 10 → 30 → 120초로 세 번 올라가 있었다.

그 기전은 틀렸다. 같은 `sysinfo` 0.32.1을 부르는 독립 프로그램은 43ms에 끝나고, 테스트와 똑같은
런타임 모양(`new_current_thread` + `spawn_blocking` + 1200×100ms 폴링)으로 감싸도 103ms다. 즉
주석이 지목한 자리에는 비용이 없었다.

`sample`이 답을 줬다. 3219개 표본 중 3181개가 아래에 있었다.

```
sysinfo::…::get_disk_properties
  → CFURLCopyResourcePropertiesForKeys → FSMountGetVolumeUUID
    → dispatch_once → CFBundleGetMainBundle → _CFBundleCreate
      → _CFIterateDirectory → readdir → __getdirentries64
```

CoreFoundation은 프로세스마다 한 번 "메인 번들"을 찾는데, 번들 안에 있지 않은 실행 파일을
만나면 **그 실행 파일이 놓인 디렉터리를 통째로 훑는다**. 테스트 바이너리의 디렉터리는
`target/debug/deps`이고, 이 체크아웃에서 그 항목 수는 111만 8582개였다.

A/B로 확정했다. 같은 바이너리, 같은 cwd, 실행 파일의 위치만 바꿨다 — `target/debug/deps`에서
97.78초, 빈 디렉터리로 복사해 실행하면 0.10초. **978배**다. `user 9.21 / sys 25.14 /
real 97.80`이라는 비율도 같은 그림을 가리킨다. CPU를 태운 것이 아니라 외장 볼륨의
`__getdirentries64`에 막혀 있었다.

대응은 `cross_client`에 이미 적용해 둔 것과 같다 — 비용을 판정 밖에서 치른다. 테스트가 폴링을
시작하기 전에 `spawn_blocking(collect_disk_free_mb)`를 한 번 await하면 CoreFoundation의
`dispatch_once`가 거기서 끝나고, 예산은 120초에서 30초로 **내려간다**. 내려간 이유가 중요하다.
비용이 줄어서가 아니라 예산이 덮어야 할 대상이 "잘못 지목된 수십 초"에서 "실측된 40ms"로
바뀌었기 때문이다.

**세 번의 예산 상향이 왜 전부 실패했는가**가 이 건에서 남길 것이다. 예산은 잘못 지목한 비용의
분산을 따라잡을 수 없다. "측정 최대치의 배수"라는 산정 방식도 기전을 모르는 한 다음 회차의
최대치를 예측하지 못한다 — 120초는 33.07초의 3.6배로 잡은 값이었는데, 진짜 비용은 디렉터리
항목 수에 비례하는 양이라 그 배수와 아무 관계가 없었다. 기전을 규명하기 전의 상향은 전부 같은
실수의 반복이다.

### 검증 한계 (정직하게)

- 이 테스트는 deps 디렉터리가 큰 트리에서 여전히 100초 가까이 걸린다. 워밍업은 그 시간을
  없애지 않고 단정과 무관하게 만들 뿐이다. 시간을 실제로 줄이는 것은 `target/` 정리다.
- Linux에는 이 경로가 아예 없다. 그래서 CI에서는 원래 느리지 않았고, **CI가 이 수정을 확인해
  주지 않는다.** 근거는 전적으로 위의 로컬 A/B다.
- A/B는 양쪽 각 1회다. 반복 재현이 아니라 기전 실측으로 정당화한다 — `cross_client` 때와 같은
  기준이다.

## 2026-08-31 — `#67` 4c-B 관측 왕복: 파생 술어에 입력을 더하면 아무도 모르게 낡는다

`#67` 4c-B. Worker가 본 Agent 프로세스 상태가 heartbeat을 타고 오케스트레이터에 도달한다.
4c-A가 "조용한 실패 모드"라고 적어 둔 자리가 닫혔다.

만든 것: 마이그레이션 `032_agent_observed_state.sql`(`observed_status`·`observed_at`·
`observed_reason`과 셋의 조합을 강제하는 CHECK), `fleet-core`의 `AgentObservedStatus`·
`AgentObservationReason`·`AgentObservation`, `Store::apply_agent_observations`(PgStore·MemStore),
`reconcile`의 반환값, heartbeat 요청의 `agent_observations`, 표면 셋.

### `027`의 CHECK를 넓히지 않은 것이 이 단계의 핵심 판단이다

4c-A의 범위 표는 4c-B가 `status IN ('ready','stopped')`를 넓혀 관측 상태를 얹을 것으로 적었다.
그렇게 하면 **한 컬럼에 두 명이 쓴다**: 운영자가 회수해 `stopped`를 쓰고, 그 회수를 보기 전에
만들어진 heartbeat이 도착해 `running`으로 덮는다. 회수가 조용히 취소된다. 게다가
`AgentStatus::blocks_project_archive()`가 `status`를 읽으므로 회수된 Agent가 다시 Project
archive를 막기 시작한다.

컬럼을 나누면 그 함수는 **손대지 않고도 계속 옳다** — 돌고 있는 Agent도 `status='ready'`라 막고,
회수된 Agent는 `stopped`라 막지 않는다. 고칠 것이 없다는 사실 자체가 두 축이 실제로 다르다는
방증이었다. `031`의 마이그레이션 주석이 이미 같은 결론을 적어 뒀다("관측은 4c가 별도 필드로
얹어야 한다"). 분할선을 마이그레이션 경계에 두자는 4c-A의 원칙은 옳았고, 틀린 것은 **어느
마이그레이션인가**였다.

정본이 관측 컬럼에 반대해 온 "두 번째 진실 원천" 논거는 여기에 닿지 않는다. 그 논거는 다른
컬럼에서 **파생되는** 값의 저장을 겨눴는데, `observed_status`는 파생되지 않는다 — Worker만
아는 정보다.

### 파생 술어가 조용히 낡는다

이 단계에서 가장 값싸게 놓칠 수 있었던 결함이다. `is_starting()`은
`status == Ready && desired_status == Running`이었다. 관측을 더해도 **이 두 항은 프로세스가
실제로 뜬 뒤에도 그대로다.** 즉 돌고 있는 Agent가 영원히 "시작 중"으로 보고된다.

컴파일러는 이것을 잡지 못한다(타입이 바뀌지 않았다). 4b의 기존 테스트도 전부 초록이다(그
테스트들은 새 컬럼을 모른다). 새 컬럼을 읽는 테스트를 따로 쓰지 않으면 결함이 남는다는 뜻이고,
그래서 `observation_clears_start_pending`을 세 항이 사라지면 깨지도록 썼다.

일반형: **파생값에 입력을 추가하는 변경은 기존 테스트 전부를 통과한 채로 그 파생값을 낡게
만든다.** 새 입력을 읽는 단정이 새로 생겨야만 드러난다.

### 개명은 예정대로, 사유는 예정과 다르게

4c-A는 `is_starting` → `start_pending` 개명을 미리 결정해 뒀고, 사유는 "관측 `Starting`이
저장되면 이름이 충돌한다"였다. 그 충돌은 **일어나지 않았다** — 관측 `Starting`을 만들지 않았기
때문이다(아래). 그런데도 개명은 했다. 진짜 이유는 술어에 세 번째 항이 붙어 뜻이 "시작 중"에서
**"명령은 냈고 아직 아무 답도 없다"**로 바뀐 것이다. 운영상 이 술어는 "Worker가 아직 집어가지
않았다"와 "집어갔는데 못 띄웠다"를 가른다 — 후자는 `observed_status='failed'`이고
`start_pending`은 `false`다.

결정을 미리 적어 둔 것은 값을 했다(다시 판단하지 않았다). 값을 하지 못한 것은 **사유**다.
사유를 미리 적으면 그 사유가 사라졌을 때 결론만 남아 근거 없이 굳는다. 정본에는 원래 절을
지우지 않고 그 아래에 "진짜 이유" 절을 붙여, 예측이 어떻게 빗나갔는지가 남게 했다.

### 생산자를 세어 어휘를 정했다

상태 기계표가 정의한 관측 `Starting`("자식을 띄웠고 아직 health check 전")은 만들지 않았다.
4c-A에 health check가 **없다** — `try_wait()`는 "죽지 않았다"만 말한다. 계산할 방법이 없는
variant는 `#70`이 걷어낸 죽은 값과 같다.

`exited`도 만들지 않았다. `reconcile`은 0단계에서 죽은 자식을 거두고 3단계에서 **같은 beat에**
다시 띄운다. 그러므로 한 beat의 정직한 관측은 `running`(또는 재기동 실패의 이유)이다. 이것을
`reconcile`의 단계 순서를 읽어서야 알았다 — 어휘를 코드가 아니라 상태 기계표에서 베꼈다면
`exited`가 그대로 들어왔을 것이다.

반대로 **어휘에 없던 생산자**가 하나 나왔다. `cmd.spawn()`의 `Err` 경로는 4c-A에서 로그 한
줄이었고 이름이 없었다. `spawn_failed`로 넣었다. 최종 어휘는 `cap_reached`/`no_free_port`/
`spawn_failed` 셋이며 전부 4c-A에 실재하는 경로다.

이유는 상태가 아니라 **필드**다. 그리고 `AgentObservation`은 구조체가 아니라 tagged enum이다 —
구조체였다면 이유 없는 `failed`를 만들 수 있고, 그 결함은 원인(워커의 메시지)에서 아주 먼
곳(**DB CHECK 위반**)에서 드러난다. enum은 그 조합을 아예 만들 수 없게 한다.

`RejectReason::as_str()`(사람이 읽는 로그 문구 "process cap reached")과
`AgentObservationReason::as_str()`(DB CHECK 값 `"cap_reached"`)은 합치지 않고 명시적 `From`으로
이었다. 합치면 로그 문구를 다듬는 순간 스키마가 깨진다.

### 관측 목록도 권위 있는 전체 집합이다 — 방향만 반대다

응답의 명령 목록이 Worker에게 권위 있는 전체 집합인 것과 대칭으로, 요청의 관측 목록은
오케스트레이터에게 그렇다. 목록에 없는 Agent의 관측은 **지운다**. 이 단계가 없으면 회수된
Agent가 `observed_status='running'`인 채로 영원히 남는다 — 지워 줄 사람이 없다.

그래서 요청 필드도 `Option<Vec>`이다. `[]`는 "이 Worker에 도는 Agent가 하나도 없다"는 적극적인
주장이라 남은 관측을 전부 지우고, 필드 부재는 "모른다"라 아무것도 바꾸지 않는다. `Vec` +
`skip_serializing_if = "Vec::is_empty"`로 뒀다면 둘이 구별되지 않아 마지막 Agent를 회수한
순간부터 관측을 지울 방법이 사라진다. 타입이 잡아 주지 않아서 `api_flow.rs`가 **선 위에서**
`[]`를 보내 관측이 지워지는 것을 단정한다.

PgStore의 적용은 "지우기 한 번 + 말한 것마다 쓰기"의 두 단계인데 트랜잭션으로 묶지 않았다.
묶을 필요가 없기 때문이다 — 지우기가 `id <> ALL($2)`로 **말한 것을 제외**하므로 같은 행이
지워졌다가 다시 쓰이는 창이 아예 생기지 않는다. 중간에 읽는 쪽이 보는 것은 "말하지 않은 것은
비었고 말한 것은 아직 이전 값"이며, 이것은 Worker가 실제로 보고한 적 없는 조합이 아니라 그냥
한 beat 이전의 상태다. 처음에는 스냅샷 의미를 트랜잭션으로 지켜야 한다고 봤는데, 지우기의
조건을 다시 읽고 나서 **구성상 이미 지켜지고 있다**는 것을 확인했다.

store 적용의 CAS 조건은 `worker_id` 하나뿐이다. 4b의 `ack_agent_commands`가 가진 세 조건 중
나머지 둘은 generation에 관한 것인데 **관측에는 generation이 없다** — 프로세스는 아무 명령
없이도 죽는다. `updated_at`은 밀지 않는다(ACK와 같은 이유).

### 검증 한계 (정직하게)

- **crash loop은 보이지 않는다.** 0단계 수거와 3단계 재기동이 같은 beat에 일어나므로 매 beat
  죽는 Agent도 `running`으로 관측된다. 드러내려면 상태가 아니라 **사건**(재기동 횟수·전이)이
  필요하고, 그 채널도 그것을 읽을 소비자도 아직 없다. 4c-A의 "채널이 없어 아무것도 모른다"와는
  다른 종류다 — 채널은 있고, 그 채널이 나를 수 있는 것의 모양이 상태라서 사건이 빠진다.
- 관측은 한 beat 늦는다. `reconcile`은 응답을 받은 **뒤에** 돌므로 그 결과는 다음 요청에나
  실린다. ACK와 같은 지연이며, `observed_at`은 store가 적용한 시각이지 Worker가 본 시각이
  아니다.
- 워커측 관측 생성은 여전히 **가짜 grok 스크립트** 상대로만 검증했다.
- `max_agent_processes` 신고는 집행이 생겼는데도 만들지 않았다 — 읽을 소비자(배정 시점의 하드
  상한)가 게이트 ①에 있다. `observed_reason='cap_reached'`가 사후에 같은 사실을 말하며, 정적인
  숫자보다 약하지만 관측이라 거짓일 수 없고 읽는 사람이 있다.

### 실검증에서 나온 것 — 어휘 CHECK는 조합 CHECK에 포섭된다

실서버(`fleet_verify_4cb`, mock transport)에 대고 9단계 27개를 확인했다: 배정 직후 관측 없음,
start 뒤 `start_pending`, `running` 관측이 오면 `start_pending`이 내려가고 `status`는 `ready`
그대로, 필드 부재는 관측을 건드리지 않고 `[]`는 지우며, `failed`는 이유를 달고 오고,
소유하지 않은 Worker의 관측은 **200으로 무시**된다.

한 건이 처음에 실패했는데 스키마가 아니라 단정이 틀렸다. `observed_status`를 어휘 밖의 값으로
바꾸는 UPDATE가 `agents_observed_status_check`를 보고할 것으로 적었지만, 실제로 보고된 이름은
`agents_observation_complete`였다. **조합 CHECK의 세 분기가 status를 `NULL`/`running`/`failed`로
열거하므로 어휘 CHECK는 그것에 완전히 포섭된다** — 어휘 CHECK만 단독으로 위반시키는 UPDATE는
존재하지 않는다(`observed_at`·`observed_reason`을 함께 NULL로 놓아도 보고되는 이름은 여전히
complete였다). 두 제약 모두 실제로 걸려 있고(`pg_constraint` 조회 2건), 어휘 CHECK는 무해하며
컬럼 정의 자리에서 어휘를 스스로 문서화한다. 마이그레이션은 이미 적용됐으므로 손대지 않는다.

단정은 제약의 **이름**이 아니라 **거부 여부**로 고쳤다. 이름을 단정하면 제약을 하나 더 추가하는
것만으로 통과하던 검사가 깨지는데, 그 검사가 지키려던 것은 이름이 아니었다.

## 2026-09-01 — `#67` 게이트 ①-A-1 상한 신고: 기본값은 모르는 값을 아는 값으로 날조한다

구현 게이트 ①("배정 초과·낡은 제어면 없음")을 한 덩어리로 닫으려다 막혔다. 정본
([권한과 장애 전환](architecture/control-plane-authority-and-failover.md):47)이 lease
레코드의 필드로 `task_id`를 적어 두는데, 그 값을 채우려면 dispatch가 Agent를 고를 수
있어야 한다(`#49` 2단계). 지금 만들면 **항상 NULL인 컬럼**이 된다. 그래서 게이트를
①-A(배정 초과)와 ①-B(낡은 제어면)로 갈랐다. ①-A는 lease 테이블을 필요로 하지
않는다 — 세어야 하는 것이 lease 행이 아니라 **배정된 `agents` 행**이기 때문이다.
이 관찰이 분할선을 정했다: 무엇을 세는가가 무엇을 만들어야 하는가를 정한다.

①-A를 다시 둘로 쪼갰다. ①-A-1은 상한이 오케스트레이터까지 **도착**해서 후보를
거르는 데까지, ①-A-2는 그 판정을 `workers` 행 잠금 아래로 옮겨 **불변식을 세우는**
데까지다. 이번은 ①-A-1이다. 순서는 편의가 아니라 데이터 의존이다 — ①-A-1이 없으면
①-A-2가 잠금 아래에서 셀 숫자 자체가 존재하지 않는다.

이 분할과 상한의 의미는 [Agent 프로비저닝](architecture/agents/provisioning.md)의
"배정 슬롯 상한" 절에 정본으로 적었고, 여기 로그는 그 결정에 이르는 과정과 검증
한계만 남긴다. [권한과 장애 전환](architecture/control-plane-authority-and-failover.md)의
"동시 slot claim" 행은 **미착수 그대로 두었다** — 그 행이 약속하는 것은 불변식이고,
불변식은 ①-A-2에서 성립한다. 필터만으로 그 행을 완료로 넘기면 문서가 코드보다 앞선다.

### 기본값을 두지 않은 것이 이 변경의 핵심이다

`workers.max_agent_processes`(migration 033)는 **nullable이고 DEFAULT가 없다**. 바로
옆의 `max_concurrent`는 `NOT NULL DEFAULT 4`인데, 그 모양을 따랐다면 실제 상한이 2인
구버전 Worker의 행에 4가 **적힌다**. 그것은 모르는 값을 아는 값으로 날조하는 것이고,
날조된 4를 믿는 배정은 상한을 지키는 것처럼 보이면서 초과시킨다. `None`은 "상한이
없다"가 아니라 "이 Worker의 상한을 **모른다**"이다.

그리고 모르는 상한은 **필터하지 않는다**. 관대해 보이지만 정당한 이유가 있다 —
오케스트레이터의 상한은 유일한 방어선이 아니라 **두 번째**다. 집행은 워커의
`agent_process.rs`가 하고, 초과 요청은 4c-B의 `observed_reason='cap_reached'`로 되돌아
온다. 그래서 오케스트레이터의 수가 없거나 틀려도 최악이 "거절된 관측"이지 초과 spawn이
아니다. 반대로 "모르면 배정하지 않는다"로 두면 구버전 Worker만 있는 fleet에서 배정이
통째로 멈추는데, 그쪽이 훨씬 나쁜 고장이다. 방어선이 둘일 때 바깥 방어선의 실패
모드를 고르는 문제였고, **거절이 정지보다 낫다**를 골랐다.

### join은 상한을 싣지 않는다

register만 이 필드를 나른다. join 시점에는 `worker.toml`이 아직 없어서 워커가 자기
상한을 모르고, 보낼 수 있는 값은 CLI 기본값 추측뿐이라 그것도 역시 날조다. join이 쓴
설정으로 기동한 워커가 몇 초 뒤 register를 부르고, `upsert_worker`의 `ON CONFLICT`가
이 컬럼을 갱신 목록에 포함하므로 NULL은 덮인다. 이 "덮인다"를 문서로만 주장하지 않고
실검증에서 join → NULL → register(3) → 3까지 왕복시켜 확인했다.

상한이 오는 자리가 heartbeat이 아니라 register인 것도 같은 종류의 판단이다. 이 값은
**관측이 아니라 설정**이고, 프로세스 수명 동안 고정이며, 바뀌는 유일한 계기가 워커
재기동인데 재기동은 register를 정확히 한 번 부른다. `worker_version`·`liveness_mode`와
같은 취급이다.

### `cap_reached`는 죽은 variant가 되지 않는다

4c-B가 만든 관측 사유 `cap_reached`가 이번 필터로 대체되는 것 아니냐는 의심이 자연스
럽지만, 아니다. 첫째로 NULL 상한 Worker는 필터를 통과한다. 둘째가 더 중요한데, 두
수가 **세는 대상이 다르다** — 오케스트레이터는 배정된 `agents` 행을, 워커는 살아 있는
프로세스를 센다. 배정됐지만 아직 뜨지 않은 Agent와, 내려가는 중이라 아직 사라지지
않은 프로세스가 두 수를 갈라 놓는다. 그래서 두 방어선은 겹치지 않는다.

거꾸로 4c-B 때 "읽을 소비자가 없어서 `max_agent_processes` 신고를 만들지 않았다"고
적어 둔 이유는 이번에 **만료됐다**. 미루기의 사유를 표로 남겨 두면 그 사유가 언제
사라졌는지도 같이 드러난다 — 로드맵 `#67` 행의 해당 문장을 이번에 고쳤다.

### 테스트는 정렬로 통과할 수 없게 만들었다

`choose_worker`는 최소 부하 Worker를 고른다. 그래서 "가득 찬 Worker가 후보에서 빠진다"는
단정을 순진하게 쓰면 **정렬만으로도 통과**한다 — 가득 찬 쪽이 대개 부하가 크기 때문이다.
`a_full_worker_is_excluded_even_when_it_is_least_loaded`는 그 우연을 막으려고 반대로
세웠다: 상한 1에 Agent 1개인 `full`(부하 1)과 상한 없이 Agent 5개인 `roomy`(부하 5)를
두고 `roomy`가 뽑히는지 본다. 정렬만 있으면 `full`이 뽑히므로 이 단정은 필터가
걸렸을 때만 통과한다.

나머지 셋은 각각 다른 경계를 지킨다. `unknown_cap_never_filters`는 Agent를 50개
얹어도 NULL 상한이 걸러지지 않는 것을, `all_reported_caps_full_is_all_at_capacity`는
새 오류 `PlacementError::AllAtCapacity`가 언제 나오고 언제 다시 사라지는지를,
`stopped_agents_free_a_capped_slot`은 `count_agents_by_worker`의 `status <> 'stopped'`
조건이 상한과 맞물려 슬롯을 실제로 돌려주는지를 본다.

### 실검증

`fleet_verify_a1`에 서버를 띄우고 **24개 항목을 전부 통과**했다. 순서 자체가 검증의 일부였다 — 상한
필터를 시험하려면 fleet에 **상한을 신고한 Worker만** 있어야 한다. NULL 상한 Worker가
하나라도 섞여 있으면 그쪽으로 배정이 성공해 버려서 필터가 걸렸는지 아닌지 구분되지
않는다. 그래서 join과 미신고 register는 배정 검증을 끝낸 뒤에 만들었다.

상한 2를 신고한 Worker 하나만 있는 fleet에서 Agent 세 개를 만들면 앞의 둘은 배정되고
셋째는 `worker_id: null`로 남는다. 이 결과는 정렬로 만들 수 없다. 이어서
`POST /api/agents/{id}/place`에 자동 배정을 시키면 409와 함께 `agent-process cap`이
본문에 나온다 — 새 오류가 운영자 표면까지 도달하는 것을 여기서 봤다. Agent 하나를
회수하면 같은 호출이 200으로 바뀐다.

미신고 register가 NULL로 저장되고 JSON 응답에서 아예 빠지는 것(없음 != 무제한),
join이 NULL을 남기고 뒤이은 register가 그것을 3으로 덮는 것도 같은 자리에서 확인했다.
다만 **NULL 상한 Worker가 배정을 받는다는 마지막 확인은 판별력이 없다** — 그 시점에
상한 2짜리는 이미 가득 차 부하도 가장 높으므로, 필터가 없어도 least-loaded 정렬만으로
같은 답이 나온다. 그 명제를 실제로 지탱하는 것은 단위 테스트 `unknown_cap_never_filters`
이고, 실검증의 그 단계는 연기 감지(smoke) 이상으로 읽지 않는다.

하네스에 함정이 하나 있어 스크립트에 재빌드를 넣었다. 게이트의 마지막 단계가
`cargo build -p fleet-cli --no-default-features`라서, 게이트가 끝난 직후의
`target/debug/fleet`는 **최소 피처 빌드**다. 그대로 실검증하면 CI 기본 세트와 다른
바이너리를 상대로 판정하게 된다 — 게이트가 남긴 마지막 부작용이 다음 검증의 대상을
바꾸는 형태다. 실검증 스크립트가 시작할 때 `--features "acp mtls"`로 되돌린다.

같은 스크립트에서 락 처리로 버그를 하나 만들었다가 잡았다. `trap cleanup EXIT`가
락 획득 **앞에** 설치돼 있어서, 락을 못 잡고 `exit`하는 경로에서도 cleanup이 돌아
**게이트가 들고 있는 락을 대신 풀었다**. `trap`은 설치 시점부터 모든 종료 경로에서
돌고, 실패해서 나가는 경로도 그중 하나다 — "정리한다"와 "내가 만든 것만 정리한다"는
다른 명제다. `LOCKED=1` 소유 플래그로 갈랐다.

### 부수: 로드맵 표 행이 렌더링에서 쪼개져 있었다

`#67` 행을 고치다 발견했다. 4c-B 때 넣은 `` `running`|`failed` ``와
`` `cap_reached`|`no_free_port`|`spawn_failed` ``의 날 `|`가 표 셀 경계로 읽혀 그 행만
5칸이 아니라 8칸으로 쪼개져 있었다. 마크다운은 인라인 코드 안의 `|`를 보호하지
않는다 — 셀 분리가 코드 스팬 파싱보다 먼저 일어난다. 어휘 나열의 구분자를 `·`로
바꿔 고쳤다. **컴파일러도 테스트도 이것을 잡지 못하고 렌더링에서만 드러난다.**

기계적으로 탐지하려면 행별 셀 수의 균일성을 세는 수밖에 없는데, **단순히 `|`를
세면 틀린다**. `awk -F'|'`로 세었더니 11개 행이 걸렸지만, 그중 `#61`·`#62`·`#85`는
`\|`로 **올바르게 이스케이프**돼 있어 렌더링에서는 멀쩡했다 — 세는 쪽이 이스케이프를
모르면 정상 행을 결함으로 신고한다. 이스케이프를 반영해 다시 세니 헤더가 정한
5열에서 어긋나는 행은 8개다: `#48`·`#49`·`#66`·`#69`·`#72`·`#78`·`#79`·`#84`.
원인은 모두 셀 내용 속의 이스케이프되지 않은 `|`이고, 그것이 코드 스팬 안에 있는지
(`#49`의 `` `GET|POST /api/agents` ``) 산문 안에 있는지(`#66`)만 다르다. 이번 커밋의
범위가 아니라 손대지 않았다.

여기서 얻을 것은 결함 자체보다 **탐지기를 검증 없이 믿으면 안 된다**는 쪽이다. 첫
집계는 3건을 없는 결함으로 만들었고, 그것을 로그에 그대로 적었다면 다음 사람이
멀쩡한 행 셋을 고치러 갔을 것이다.

### 검증 한계

①-A-1은 **읽은 시점의 수**로 판정한다. `count_agents_by_worker`가 돌려준 값을 보고
후보를 거른 뒤 배정하므로, 동시 요청 둘이 같은 빈자리를 함께 통과할 수 있다. 잠금이
가야 할 자리는 `agents`가 아니라 **상한의 주인인 `workers` 행**이고(세는 행들이 아직
존재하지 않으므로 `INSERT ... WHERE (SELECT COUNT(*)) < cap`은 READ COMMITTED에서
전형적인 phantom이다), 그것이 ①-A-2다. 그래서 정본의 동시 slot claim 행은 이번에도
**미착수**로 둔다 — 부분 진전을 완료로 적지 않는다.

실 프로세스 검증은 여전히 가짜 grok 스크립트 상대다. 그리고 워커가 신고하는 수가
설정과 실제로 일치하는지는 이번 범위 밖이다 — 워커는 자기 설정값을 그대로 싣고,
그 설정이 실제 spawn 한계와 맞는지는 4c-A의 집행이 답한다.

## 2026-09-01 — `#67` 게이트 ①-A-2 슬롯 선점: 잠금은 세는 대상이 아니라 상한의 주인에 건다

①-A-1은 `choose_worker`가 가득 찬 Worker를 후보에서 빼게 만들었다. 그것은 **정렬
편향**이지 불변식이 아니다 — 필터는 읽은 시점의 수로 판정하므로 동시 요청 둘이 같은
마지막 슬롯을 보고 함께 통과한다. 이번에 그 판정을 잠금 아래로 옮겼다.

### 잠글 수 없는 것을 잠그려 하지 않는다

먼저 무너진 설계가 `INSERT ... WHERE (SELECT COUNT(*) FROM agents ...) < cap`이다.
READ COMMITTED에서 이것은 전형적인 phantom이다. 서브쿼리는 **아무 행도 잠그지 않고**,
무엇보다 세어야 할 대상이 *아직 존재하지 않는 행*이라 대상 자체를 잠글 방법이 없다.

그래서 잠그는 것을 바꿨다 — 세는 대상이 아니라 **상한의 주인**이다:

```
SELECT max_agent_processes FROM workers WHERE id = $1 FOR UPDATE
```

한 트랜잭션 안에서 그 Worker 행을 잠그고, 잠금 아래에서 다시 세고, 통과하면 배정을
쓰고 커밋한다(`PgStore::create_agent`의 INSERT 경로와 `assign_agent_worker`의 UPDATE
경로 둘 다). 부수 효과가 하나 따라오는데 그게 이 설계의 값이다: 잠금이 전역이 아니라
**Worker별**이므로 서로 다른 Worker로 가는 배정은 그대로 병렬이다.

**READ COMMITTED이 배경이 아니라 전제다.** 이 격리 수준은 문 하나하나가 새 스냅샷을
뜨므로, 대기하던 트랜잭션이 잠금을 얻은 뒤 다시 센 `COUNT(*)`가 방금 커밋된 승자의
행을 본다. REPEATABLE READ였다면 같은 코드가 직렬화 오류를 올렸을 것이고 재시도 루프가
따라붙었을 것이다.

### 실패가 하나가 아니라 둘이라서 `bool`을 버렸다

`assign_agent_worker`의 반환형을 `bool`에서
`SlotClaim::{Claimed, CapReached, NoSuchAgent, NoSuchWorker}`로 넓혔다. 이유는 취향이
아니다 — 실패가 **서로 다른 두 사실**이기 때문이다. 상한에 걸린 것은 지금의 fleet
상태이므로 나중에 다시 하면 되고(409), 존재하지 않는 대상을 지목한 것은 요청의
결함이므로 다시 해도 같다(404/400). `bool`은 그 둘을 한 칸에 뭉개므로 호출부가 반드시
한쪽으로 오분류한다.

실제로 오분류가 이미 있었다. MCP의 배정 경로는 `bool`을 **통째로 버리고** 있어서
존재하지 않는 Agent를 지목해도 조용히 성공으로 응답했다. `SlotClaim`으로 바꾸면서
그 구멍이 같이 닫혔다 — 반환형을 넓힌 것이 버그를 *찾아* 준 것이다.

**`NoSuchAgent`가 `NoSuchWorker`보다 우선한다.** 이것은 취향이 아니라 구현의 사실이다:
Postgres의 UPDATE는 Agent가 없으면 0행을 갱신하고 Worker FK 검사에 닿지도 않으며,
MemStore도 그 순서를 흉내 내고 있었다. 그 순서를 지키려고 Agent 존재 확인을 **일부러
잠그지 않고** 먼저 읽는다 — 여기서 `FOR UPDATE`를 붙이면 `agents → workers` 순서가
생겨 아래의 잠금 순서를 깨뜨린다.

### 조용한 강등은 감사 로그를 거짓말시킨다

`create_agent`가 상한에 걸렸을 때 생성 자체를 실패시키지 않는다 — 4a가 정한 대로
`worker_id = NULL`은 정상 상태다. 그렇다고 저장소가 **조용히** 강등하면 안 된다.
생성 핸들러 둘(Dashboard·MCP)이 응답과 감사 detail을 **같은 로컬 구조체**에서 만들기
때문이다. 되맞추지 않으면 그 구조체는 일어나지 않은 배정을 말하고, 그것이 감사 로그로
들어간다. 응답의 거짓은 다시 조회하면 드러나지만 **감사 로그의 거짓은 남는다**.

그래서 `create_agent`의 반환형이 `Result<Option<WorkerId>, StoreError>`다 — 실제로
기록된 배정을 돌려주고, 호출부가 `Agent::without_placement()`로 되맞춘다. 그 헬퍼는
`worker_id`와 `assigned_at`을 **함께** 지운다. 마이그레이션 `030`의
`agents_placement_complete` CHECK가 "둘 다 있거나 둘 다 없거나"를 요구하므로 한쪽만
지우면 저장이 거부된다.

### 자기를 세면 무해한 no-op이 거절된다

`assign_agent_worker`의 카운트에는 `AND id <> $agent_id`가 붙는다. 이미 그 Worker에
있는 Agent를 같은 Worker로 다시 배정하는 것은 슬롯을 **추가로 쓰지 않는데**, 자기
제외가 없으면 정확히 가득 찬 Worker에서 그 no-op이 `CapReached`로 거절된다.
`create_agent`에는 이 제외가 없다 — 그 시점에는 행이 아직 없기 때문이다. 같은 이유로
카운트 술어(`status <> 'stopped'`)는 필터와 선점에서 **같아야 한다**. 둘이 슬롯의
정의를 달리 보면 필터를 통과한 Worker가 선점에서 거절되어 배정이 이유 없이 실패한다.

### 잠금 순서는 `workers → agents` 하나뿐이다

두 경로가 모두 이 순서를 지킨다. Postgres에서는 순환이 없어 데드락이 없다는 뜻이고,
MemStore에서는 **위반이 곧 데드락**이다 — Mutex 두 개를 반대로 잡으면 즉시 서로를
기다린다. `MemStore::create_agent`가 상한을 `agents` 잠금 **앞에서** 읽는 코드 모양은
그래서 나온 것이지, 스타일이 아니다.

### 한 번도 붉었던 적 없는 동시성 테스트는 아무것도 증명하지 않는다

잠금은 두 자리에 **따로** 걸려 있다 — `create_agent`와 `assign_agent_worker`가 각자
`FOR UPDATE`를 잡는다. 그래서 테스트도 둘이다. `concurrent_creates_cannot_exceed_the_cap`
(생성)과 `concurrent_placements_cannot_exceed_the_cap`(재배정) 각각이 상한 1인 Worker에
8개를 `tokio::sync::Barrier`로 동시에 달려들게 한 뒤 `(Claimed, CapReached) == (1, 7)`과
**저장된 행 수 1**을 함께 단정한다. 뒤의 단정이 따로 있는 이유는 반환값만 보면 Store가
거짓말을 해도 통과하기 때문이다. 한쪽만 붉히고 "선점이 증명됐다"고 적으면 증거보다 강한
주장이 된다 — 실제로 이 항목의 초안이 그렇게 적혀 있었다.

**풀은 크기를 정하는 것으로 부족하고, 미리 채워야 한다.** 이 파일의 공용 풀은
`max_connections(4)`라 8개 태스크가 체크아웃 단계에서 줄을 서고, 그러면 `FOR UPDATE`가
없어도 통과한다. 그런데 `max_connections(8)`으로 올리는 것만으로는 같은 직렬화가 한 겹
아래로 옮겨갈 뿐이다 — sqlx의 `connect()`는 커넥션을 **하나만** 열고 `min_connections`의
기본값은 0이므로, Barrier가 풀린 뒤 나머지 7개를 **접속하는** 비용이 태스크들의 출발선을
흩는다. `min_connections(8)`도 보장이 아니다. 그 채움은 백그라운드 태스크가 하고
`connect()` 반환 시점에 끝났다는 보장이 없다. 유일한 보장은 커넥션 N개를 직접
`acquire()`했다가 버리는 것이고, 그것이 `race_pool()`이 하는 일이다.

**붉은 값이 흔들리지 않는 것은 안심할 근거가 아니라 진단 단서다.** 워밍업 전에는 6회 모두
정확히 `left: (2, 6)`이었고, 초안은 이것을 "phantom 창이 좁다"로 읽었다. 틀렸다 — 하네스가
실효 동시성을 2로 묶고 있었을 뿐이다. 풀을 미리 채우자 같은 잠금 없는 코드가 이렇게
바뀐다:

```
잠금 제거 전(하네스 결함):  left: (2, 6)   right: (1, 7)   ← 6회 모두 동일
잠금 제거 후(풀 워밍업):    left: (8, 0)   right: (1, 7)   ← 두 경로 각 3회 모두 동일
```

8개 **전부**가 상한 1인 Worker에 앉는다. 즉 창은 좁지 않았고, 좁아 보이게 만든 것은
테스트였다. 실효 동시성이 2인 하네스는 기계가 조금만 달라도 `(1, 7)`을 내어 **잠금 없는
코드를 초록으로 통과**시킬 수 있었다.

증명은 한 번에 한 변수만 바꿔서 했다. `assign_agent_worker`의 `FOR UPDATE`만 지운 트리에서
재배정 테스트가 3회 전부 실패(`exit=101`), 되돌린 뒤 `create_agent`의 `FOR UPDATE`만 지운
트리에서 생성 테스트가 3회 전부 실패, 둘 다 되돌린 트리에서 두 테스트가 3회 전부 통과
(0.15 / 0.10 / 0.09s)했다. 복원은 `diff`로 원본과 바이트 동일함을 확인했다.

### 경합으로 닿을 수 없는 분기는 결과를 직접 세워야 증명된다

핸들러 쪽 되맞춤(`if placed != agent.worker_id { agent = agent.without_placement(); }`)에도
같은 잣대를 대려다, 앞 절의 제목(「한 번도 붉었던 적 없는 동시성 테스트는
아무것도 증명하지 않는다」)이 그대로 되돌아왔다. 처음 쓴 것은 대시보드 API를
2-way `tokio::join!`으로 때리는 경합 테스트였고, 초록이었다. 그런데 **되맞춤 두 줄을
지운 트리에서도 12회 전부 초록이었다.** 8-way 배리어로 다시 써서 동시성을 네 배로 올려도
12회 중 0건이었다.

원인은 하네스가 아니라 MemStore의 구조였다. MemStore의 연산 사이에는 `.await` 양보점이
하나도 없어서 `place_on_create`와 `create_agent`가 한 태스크 안에서 이어 붙는다. 스레드를
늘려도 두 요청의 창이 겹치지 않는다 — **동시성을 올려서 넓힐 창이 애초에 없다.** 이것은
앞의 `race_pool()` 사례와 정반대의 교훈이다. 거기서는 하네스가 창을 묶고 있어 푸는 것이
답이었지만, 여기서는 아무리 풀어도 창이 생기지 않는다.

그래서 증명을 둘로 쪼갰다.

**상한 경합은 Postgres에서 증명한다** — 위의 `concurrent_creates_cannot_exceed_the_cap`과
`concurrent_placements_cannot_exceed_the_cap`이 잠금 자리마다 따로 붉어졌다.

**되맞춤 분기는 결과를 직접 세워 결정적으로 증명한다** — `MemStore::dropping_placements()`
라는 주입 스위치를 더해, 후보 Worker를 실어 보내도 배정 없이 저장하게 만들었다. 이것은
기존 `with_failing`으로는 표현할 수 없다. 선점 실패는 **오류가 아니라** `Ok(None)`이기
때문이다 — 생성은 성공하고 배정만 떨어진다. `create_agent`의 반환 타입이
`Result<Option<WorkerId>, _>`인 이유가 바로 그것이고, `failing: HashSet<&'static str>`은
그 모양을 담지 못한다.

**주입 스위치는 진짜 판정 뒤에 둔다.** `if n >= cap` 블록 앞에 두면 상한 경로가 스위치에
가려져, 상한을 통째로 지워도 테스트가 초록으로 통과한다. 판정하려는 대상을 무력화하는
하네스는 DB 스위트를 병렬로 돌리는 것과 같은 계열의 결함이다 — 그런 초록은 아무 말도
하지 않는다.

**공허하게 통과할 여지도 막았다.** 두 테스트 모두 `place_on_create(...)`가 여전히 `Some`을
돌려주는지 먼저 단정한다. 그 줄이 없으면 "후보가 없어서 미배정"과 "후보를 지명했으나
선점이 떨어져 미배정"이 구별되지 않고, 테스트가 통과하면서 아무것도 증명하지 않는다.

측정치는 다음과 같다. 대시보드·MCP 두 표면 모두 GREEN 3/3(`exit=0`), 되맞춤을 **한 표면씩만**
지운 트리에서 각각 RED 3/3(`exit=101`), 복원은 `diff`로 바이트 동일함을 확인했다. 실패
메시지가 주장하려던 거짓을 그대로 보여준다 — 저장된 행에는 배정이 없는데 응답이
`"worker_id":"c16d6a78-…"`와 `"assigned_at"`을 싣고 나간다.

**두 표면의 위험 등급은 같지 않다.** 대시보드의 `create_agent_api`는 되맞춤 **뒤에**
`crate::audit::record`를 부르므로, 되맞춤이 없으면 일어나지 않은 배정이 감사 로그에
**영구히** 남는다. `fleet-mcp`의 `handle_create_agent`에는 감사 기록 호출이 아예 없다
(1135–1214행 확인). 거기서 거짓은 응답에만 실리고, 응답의 거짓은 다시 읽으면 드러난다.
코드는 공유하지만 위험은 공유하지 않으므로, 이 차이를 적어 두지 않으면 MCP 쪽 증명을
과대 해석하게 된다.

경합 테스트 자체는 남겨 뒀다 — 이름을
`concurrent_creates_through_the_api_cannot_exceed_the_cap`으로 바꾸고, 되맞춤 분기에 닿지
못한다는 사실을 doc 주석에 실측치(2-way 12회 0건, 8-way 12회 0건)와 함께 적었다. API
계층에서 상한이 지켜지는지는 여전히 그 테스트가 본다. 다만 그것이 되맞춤을 증명한다고
읽히지 않도록 문서와 주석 양쪽에 한계를 명시했다.

### 실검증은 여기서 장식이 아니다 — 두 조각이 만나는 유일한 자리다

트리 안의 증명 둘은 서로 만나지 않는다. `concurrent_creates_cannot_exceed_the_cap`은
PgStore를 **핸들러 없이** 때리고, `create_agent_follows_the_store_when_the_slot_claim_
drops_the_placement`는 핸들러를 **MemStore로** 때린다. "Postgres가 잠금 아래에서
`Ok(None)`을 준다 → 핸들러가 되맞춘다 → 감사 로그에 null이 남는다"라는 **합성**은 어느
테스트도 통째로 밟지 않는다. 그래서 이번 실검증은 일반 스모크가 아니라 그 합성만
겨냥해 짰다(`verify_a2.py`).

전용 DB(`fleet_verify_a2`)에 서버를 띄우고, 상한 1짜리 Worker를 `register`로 만든 뒤
두 경로를 봤다. **순차 경로**는 결정적이다 — 상한 1에 둘째를 넣으면 응답·저장된 행·감사
로그가 셋 다 미배정이어야 한다(3건 전부 통과). **경합 경로**는 8개를 배리어로 묶어
`POST /api/agents`에 동시에 던지고, 배정 성공이 정확히 하나인지뿐 아니라 **8건 전부
응답·감사 로그·저장된 행이 서로 일치하는지**를 봤다. 13/13 통과했다.

**그런데 첫 실행은 되맞춤이 밟혔는지 말해 주지 못했다.** 미배정 응답은 두 경로가 같은
모양을 낸다 — 후보를 아예 못 찾은 것(`place_on_create`가 `None`)과, 지명했다가 선점에서
떨어진 것(되맞춤)이 밖에서 구분되지 않는다. 가르는 것은 되맞춤 안의
`tracing::info!(target: "fleet::placement", …)` 한 줄인데, 첫 실행은 `RUST_LOG=warn`이라
그 줄이 기록될 수 없었다. **판정에 필요한 신호를 끄고 돌린 것이므로, 그 로그로 "밟혔다"고
쓰면 그것은 관측이 아니라 추론이다.** `RUST_LOG="warn,fleet::placement=info"`로 다시
돌렸다.

두 번째 실행에서 8건이 세 갈래로 갈렸다 — **1건 배정 성공, 2건 되맞춤, 5건은 후보 없음**.
`placement dropped at slot claim ... attempted_worker=Some("057fab89-…")`가 두 번 찍혔다.
지명은 있었고 저장이 그것을 거부했으며, 응답과 감사 로그는 저장된 사실을 따랐다. 주입
없이 얻은 증명이고, 두 조각이 살아 있는 서버에서 이어진 것은 이 지점이다. 검증 뒤
서버를 내리고 `dropdb`까지 정리했다.

### 검증 한계

**`choose_worker`는 승자를 하나만 돌려준다.** 두 요청이 같은 후보를 골랐다가 한쪽이
선점에 실패하면, 다른 Worker가 비어 있어도 그 요청은 미배정으로 끝난다. 불변식은
깨지지 않고 **배정이 놓칠 뿐**이다. 답은 후보를 순위 목록으로 돌려 흘러내리게 하는
것이지만, `POST /api/agents/{id}/place`가 이미 회복 경로라 미룬다. 이 한계는
`fleet-scheduler/src/placement.rs`의 모듈 문서에도 적었다.

**MemStore에는 잠금에 대한 증명이 없다.** 위의 Store 경합 테스트 둘은 Postgres 전용이다 —
MemStore는 Mutex 하나로 직렬화되므로 같은 경합을 재현할 수 없고, 거기서 통과해도 잠금에
대해 아무것도 말하지 않는다. MemStore 쪽에서 검증되는 것은 판정의 **모양**(자기 제외, 반환
variant의 우선순위)과, 그 판정을 **핸들러가 어떻게 기록하는가**(감사 로그 정직성)다. 후자가
MemStore 위에서 성립하는 이유는 그 테스트가 잠금을 보는 게 아니라 반환값과 저장된 행의
일치를 보기 때문이다.

**되맞춤 분기는 주입으로 증명했지만, 실경합에서도 밟히는 것을 확인했다.** 트리 안의
증명은 주입으로 세운 것이다. 그러나 실검증에서 살아 있는 서버에 8-way로 달려들었더니
`fleet::placement` 로그에 `placement dropped at slot claim`이 **2건** 찍혔다 —
`attempted_worker=Some("057fab89-…")`, 즉 지명은 있었고 저장이 그것을 거부했다.
다만 이 숫자는 이 하드웨어의 이 한 번에 대한 것이지 배포 환경의 빈도가 아니다.

**상한이 NULL인 Worker는 여전히 초과할 수 있다.** ①-A-1이 정한 대로 모르는 상한은
필터하지 않고, 선점도 마찬가지로 통과시킨다. 그 경우의 최종 집행자는 4c-A의 워커
프로세스 매니저이고 결과는 `observed_reason = 'cap_reached'`다.

[권한과 장애 전환](architecture/control-plane-authority-and-failover.md)의 동시 slot
claim 행을 **미착수 → 부분**으로 옮겼다. 완료가 아닌 것은 그 행이 세 가지를 묶고 있기
때문이다 — slot claim은 Store의 두 진입점 모두에서 닫혔지만, ACK 유실과 Worker
reincarnation의 프로세스 중복은
`worker_execution_lease`(①-B)를 기다린다.

## 2026-09-01 — Roadmap 표의 파이프 13개: 셀을 세는 스캔이 자기 이스케이프 규약을 못 본다

`6be5f01`이 남긴 약속 — "이 커밋은 `#67` 행만 고치고, 같은 결함을 가진 여덟 행(`#48`,
`#49`, `#66`, `#69`, `#72`, `#78`, `#79`, `#84`)은 별도 docs 커밋으로 남긴다" — 을 이행하면서,
그 약속의 개수부터 다시 셌다. **처음 센 결과는 여덟이 아니라 열하나였다.**

셈에 쓴 것은 `awk '/^\|/{n=gsub(/\|/,"|"); if(n!=6) …}'`였다. `gsub(/\|/,"|")`는 행 안의
**모든 `|` 문자**를 센다 — 이미 `\|`로 올바르게 이스케이프된 것까지 포함해서. 그래서
`` `periodic`\|`on_demand` ``(69행), `` `Pending`\|`Dispatched` ``(70행),
`` 2>&1 \|\| true ``(110행)를 가진 `#61`·`#62`·`#85`가 결함으로 보고됐다. 이 셋은 **처음부터
올바른 행**이다.

이스케이프를 구분해서(`p==0 or line[p-1] != '\\'`) 다시 세자 정확히 여덟 행,
추가 파이프 13개가 나왔다. **`6be5f01`의 약속은 정확했다.** 하마터면 "약속이 세 행을
빠뜨렸다"는 거짓 정정이 커밋 메시지로 이력에 남을 뻔했고, 그것을 막은 것은 새 도구가
아니라 파이프 하나의 앞 문자를 눈으로 본 것이다.

이것은 이 저장소가 §4.3에 쌓아 온 결함군의 또 다른 얼굴이다. 앞의 사례들에서 판정처럼
보이는 값을 무효화한 것은 **플래그 부재**, **파이프 뒤의 `$?`**, **게이트의 신선도**,
**커널의 시계 해상도**, **꺼져 있던 신호**였다. 여기서는 값이 **다른 것을 세고 있었다** —
숫자는 정상이고 비교도 정상인데, 세는 단위가 "셀 경계"가 아니라 "문자"였다.

**결함의 모양은 둘이고 피해가 다르다.** (a) 9개는 인라인 코드 스팬 안의 `|`다
(`tail -f /dev/null | fleet serve`, `GET|POST /api/agents`, `||` 단락 평가,
`unwrap_or_else(|| …)`, `|| true`, `base64 -d` 파이프라인). GFM 표 파서는 **인라인 파싱보다
먼저** 셀을 쪼개므로 코드 스팬은 `|`를 보호하지 못하고, 게이트 셀이 코드 중간에서 잘렸다.
(b) 4개(74·77·80·103행)는 저자가 프로즈와 근거 사이에 **셀 구분자로 쓴** `|`다. 이쪽은
6번째 셀을 만들었고, GFM은 헤더보다 많은 셀을 **조용히 버린다** — 즉 그 네 행의 마지막
셀은 지금까지 **한 글자도 렌더링되지 않았다**. 어긋난 게 아니라 사라진 것이다.

**13개 전부 이스케이프로만 고쳤다.** (b)를 이 파일의 다른 행들처럼(102·105·107·108행의
`검증:`, 75행의 `**검증 한계**:`) 프로즈로 다시 쓰는 선택지가 있었지만 하지 않았다. 이유는
문체가 아니라 증명 가능성이다 — diff가 백슬래시 삽입뿐이면 기계로 증명된다:
`new.replace('\|','|') == old.replace('\|','|')`가 `True`, 파일 길이 차이 정확히 +13.
프로즈를 고치면 그 성질이 사라지고 docs diff를 눈으로 보는 일로 돌아간다. `6be5f01`이
자기 소관 아닌 행을 건드리지 않은 것과 같은 원칙이다. `검증:` 문체 불일치는 **미이행으로
남긴다** — 별도 편집 패스의 몫이다.

142행 이하의 종결 표는 4열(파이프 5개)이라 규격이 다르다. 검증은 두 표를 각각 확인한다:
23·44·54·63·100·126행 헤더의 5열 표는 모든 행이 이스케이프 제외 6개, 142행 표는 5개.

## 2026-09-01 — `#67` 게이트 ②: 차단 사유가 만료돼도 차단이 만료된 것은 아니다

`#67` 4단계(4a·4b·4c-A·4c-B)가 모두 끝났으므로, 게이트 ②(partition 중 Worker
self-fencing E2E)의 기록된 차단 사유 "같은 이유(`#67` 4단계)"와 아키텍처 유예 표의
"워커가 자기 세대를 확인할 입력이 없다"는 문장은 성립하지 않는다. 여기서 곧바로
"게이트 ②가 열렸다"로 넘어갈 뻔했고, 그 추론은 틀렸다.

**같은 문서가 이미 한 번 이 구분을 해 놓았다.** 유예 표의 ①-B 칸은
"**'lease의 주체가 없다'는 만료됐다**"로 시작한 뒤, 남은 차단은 근거가 아니라
**레코드의 필드**(`task_id`)이며 그 필드는 `#49` 2단계에 달려 있다고 적는다. ②는 같은
귀속(`#67` 4단계)과 같은 모양의 사유를 달고 있었는데, ①-B를 고칠 때 함께 재도출되지
않아 낡은 채로 남았다. 즉 이번 것은 새로운 발견이 아니라 **한 번 적용된 재도출이 인접
행에 전파되지 않은 자국**이다.

측정으로 확인한 것은 이렇다. `AgentCommand`가 싣는 필드는 `agent_id`,
`desired_status`, `generation` 셋뿐이다(`crates/fleet-core/src/agent.rs`). 그리고
`generation`의 출처는 두 Store 모두에서 `agents.command_generation`이다
(`postgres.rs`의 `list_agent_commands`가 `SELECT ... command_generation`, `mem.rs`가
`a.command_generation`). migration 031이 그 컬럼에 대해 규정하는 것은
`WHERE command_generation = $ack` CAS로 "늦게 도착한 명령이 새 상태를 덮지 못한다"를
얻는 것, 즉 **Agent 하나에 대한 명령 시퀀스**다. self-fencing이 확인해야 하는 세대는
"이 워커가 아직 제어면이 인정하는 그 워커인가"이고 그것은 `control_epoch`·
`worker_incarnation`·`fencing_token`이며, `provisioning.md`의 9-필드 봉투에는 있지만
실제 `AgentCommand`에는 없다. **이름이 같아서 대체 가능해 보이지만 축이 다르다** —
하나는 Agent 단위 명령 순서, 하나는 워커 단위 제어면 신원이다.

**시간 기반 펜싱을 대체재로 쓰지 않는 이유를 함께 남긴다.** "N회 연속 heartbeat 실패
시 Agent 프로세스를 정리한다"는 새 컬럼도 봉투 필드도 없이 오늘 구현 가능하고, 그래서
매력적으로 보인다. 그러나 그것은 같은 불변식을 싸게 얻는 것이 아니라 **다른 술어**다.
세대 펜싱의 전제는 "나는 내가 낡았음을 증명할 수 있다"이고 타이머의 전제는 "나는
아무에게도 닿지 못한다"인데, 후자는 경합의 존재를 말해 주지 않는다. 그리고 오늘 이
트리에서는 경합이 자동으로 발생하지 않는다 — 오프라인 워커의 Agent를 재배정하는
스케줄러 경로가 없다(`assign_agent_worker`의 호출자는
`crates/fleet-dashboard/src/handlers.rs`와 `crates/fleet-mcp/src/handlers.rs` 둘뿐이고,
`reconcile.rs`에는 Agent 처리가 한 줄도 없으며, `placement.rs`는 `unassign_agent_worker`를
만들지 않은 근거를 명시한다). 따라서 지금 타이머를 넣으면 **아무도 다투지 않는 프로세스만
죽인다** — 펜싱이라는 이름의 가용성 회귀다.

덧붙여 그 타이머는 4c-B가 이미 내린 판단을 뒤집는다. `registration.rs`는 heartbeat이
실패하면 경고만 찍고 프로세스에 손대지 않는데, 근거는 `None`(권위 있는 목록을 받지
못함)과 `Some(vec![])`(비우라는 명령)의 구분이다. 타이머는 정확히 전자를 후자로 읽는다.
**"채울 방법이 없는 것은 미리 만들지 않는다"를 컬럼이 아니라 상태 전이에 적용한 사례**로
남긴다.

그래서 이번 변경은 구현이 아니라 정본 정정이다. 아키텍처 정본의 유예 행은 귀속을
`#67` 4단계에서 `#49` 2단계로 옮기고 사유를 재도출로 바꿨으며, 게이트 6행에는 오늘의
실제 노출이 **운영자 경로뿐**이라는 측정을 적었다(게이트 6행은 그 사실을 담고 있지
않았다). Roadmap `#67` 행의 "다음 게이트" 문장은 2026-08-29자라 4a/4b/4c와
①-A-1/①-A-2 이전이고, ② 절은 그 이후로 거짓이 되어 있었다.

**검증 한계**: 문서 전용 변경이며 소스는 건드리지 않았다. "자동 재배정 경로가 없다"는
`assign_agent_worker` 호출자 grep과 `reconcile.rs`의 Agent 부재로 확인한 정적 사실이고,
파티션을 실제로 일으켜 관측한 것이 아니다. 45초 오프라인 유예(`health.rs`의
`check_interval` 15초 × `missed_heartbeat_threshold` 3)와 워커 기본 heartbeat 15초는
설정값을 읽은 것이며, 펜싱 임계값을 정할 때 다시 쓰이겠지만 이번에는 아무 임계값도
넣지 않았다.

## 2026-09-01 — `#49` 2단계: 넷이 지목하는데 지목당한 쪽이 비어 있었다

`#67` 게이트 ②의 귀속을 `#49` 2단계로 옮긴 직후, 그 2단계를 진행하려고 정본을 열었다가
**2단계의 정의가 어디에도 없다**는 것을 알았다. 로드맵 `#49` 행은 상태 칸에 "2단계 대기"라고
적고 본문은 1단계만 서술한 뒤 마지막에 "**2단계 이후**: #48/#67 뒤 Hibernated 단일 Agent E2E"로
끝난다. 2단계가 무엇인지는 한 줄도 없다.

없는 것을 넷이 지목하고 있었다. `provisioning.md` 유예 표의 `tasks.agent_id` 칸과 "ACK가 Agent
process의 endpoint·secret을 돌려주는 것" 칸, 같은 문서 구현 게이트의 `worker_execution_lease`
행, 그리고 어제 옮긴 `control-plane-authority-and-failover.md`의 게이트 ①-B·②. 전부 선행 칸에
`#49` 2단계를 적어 두었다.

### 정의가 비면 지목이 서로 다른 것을 상상한다

빈자리는 조용히 있지 않았다. `provisioning.md:159`의 2026-08-30 분할 블록이 로드맵의
"2단계 **이후**"를 "2단계"로 옮겨 적어, 2단계를 **Hibernated E2E 자체**로 읽고 있었다. 발원지는
`docs/log.md` 2026-08-30 항목의 "「#49」 2단계의 정의는 «#48/#67 뒤 Hibernated 단일 Agent E2E»이고"
라는 문장이다. 로그는 append-only이므로 고치지 않고, 그 문장을 옮겨 적은 설계 정본만 정정했다.

**분할의 결론은 그대로 선다.** 다만 "다른 근거가 또 있어서"가 아니다 — 두 문장이 서로 다른
대상을 말하기 때문이다. 2단계가 소유하는 것은 Task를 Agent로 보내는 **라우팅**(`tasks.agent_id`)
이고, 4a가 주인을 찾던 것은 Agent를 Worker에 놓는 **배정**(`agents.worker_id`)이다. 인용이
정확했더라도 2단계는 배정을 소유하지 않는다. 그래서 정정은 원문을 지우지 않고 날짜 블록으로
덧붙였다 — 결론이 유지되는 이상, 분할이 왜 일어났는지의 기록을 없앨 이유가 없다.

### 정의: dispatch가 Agent를 고른다

네 지목이 전부 같은 것을 기다리고 있다는 점이 정의를 정한다. `tasks.agent_id`는 "dispatch가
Agent를 고르지 않는다"라서 미뤘고, ACK의 endpoint·secret은 "Task를 Agent로 라우팅하는 소비자가
없다"라서 미뤘고, lease 레코드는 `task_id`를 실어야 하는데 그 값이 없어서 미뤘다. 그러니
2단계는 `tasks.agent_id`와 그 라우팅이다. 현재 `Selector::select`는 `WorkerId`를 돌려주고
(`crates/fleet-scheduler/src/selector.rs`) `Task`에는 `agent_id` 필드가 없다
(`crates/fleet-core/src/task.rs`) — 2단계는 이 두 사실을 바꾸는 일이다.

범위 밖으로 **명시적으로** 뺀 것이 둘이다. Hibernated 단일 Agent E2E는 3단계다(원문이 "이후"라고
적은 대상이고, 선행에 `#86`·`#51`이 더 붙는다). 그리고 항목 제목의 `memory·summary·tool binding` —
이것은 **제목에만 있고** 로드맵 본문에도 `provisioning.md` 유예 표에도 한 번도 나오지 않는다.
즉 오늘 채운 것과 같은 종류의 미정의 경계가 하나 더 있었다. 셋 다 Agent에 *무엇을 실을지*의
문제라 harness 구성(`#51`)에 걸리며, *어디로 보낼지*와는 축이 다르므로 3단계 이후로 뺐다.
빈자리를 채우면서 옆에 있는 빈자리를 그대로 두면 다음 세션이 같은 자리에서 멈춘다.

### `#48`은 선행이 아니라 후행이다

"#48/#67 뒤"라는 구절 때문에 `#48`이 2단계를 막는지 확인했다. 아니다. 로드맵 `#48` 본문은
`#49`를 **한 번도 언급하지 않는다**(4단계 전부와 차단 조건 서술을 훑었다). 방향은 오히려
반대다 — `#48`의 남은 차단 조건 2는 "*자동* provisioning 경로"를 기다리고, 그 경로의 첫 절반이
2단계다. `#48`을 2단계의 선행으로 읽으면 의존이 순환한다. 2026-08-30에 `#67`↔`#89`에서 닫은
것과 같은 모양이며, 그때 그 순환도 정본 두 곳에 실제로 적혀 있었다.

단 2단계가 조건 2를 **닫는지는 별개**로 남긴다: 조건 2는 Agent를 *고르는* 것이 아니라
*만드는* 것을 시험할 수 있고, 조건 3은 lease를 따로 기다린다.

### 검증 한계

문서만 바꿨고 코드는 손대지 않았다. `.md`가 컴파일에 닿는 경로(`include_str!`/`include_bytes!`/
`doc = include`)가 `crates/` 전체에 없음을 다시 확인하고 테스트는 돌리지 않았으며, §4.3 게이트
5줄은 편집을 전부 확정한 뒤 파이프 없이 종료 코드를 직접 읽어 돌렸다. 표 규격 검사기를 처음에
3열 상수로 짜서 2열 표 23행을 가짜 위반으로 뱉었다 — 이 저장소의 표는 2·3·4·5열이 섞여 있으므로
**규격은 표 블록마다 헤더에서 뽑아야** 한다. 고친 검사기로 두 파일 19개 블록 전부 위반 0건이다.
그리고 오늘 확정한 것은 **범위이지 설계가 아니다**: `tasks.agent_id`를 어떻게 채우는지(제출 시점인지
dispatch 시점인지), 고를 Agent가 없을 때 무엇을 하는지는 아직 아무것도 정하지 않았다.

## 2026-09-01 — `#49` 2단계 구현: 지목은 제출이 검증하고 dispatch는 지킬 뿐이다

같은 날 오전에 범위만 확정하고 "설계는 아직 아무것도 정하지 않았다"로 닫았던 그 항목을
구현했다. migration `034`가 `tasks.agent_id`(`agents(id)` FK + `WHERE agent_id IS NOT NULL`
부분 인덱스)를 만들고, 제출 표면 둘(Dashboard `POST /api/tasks`, MCP `fleet_dispatch_task`)이
`fleet_store::apply_agent_pin` 하나를 공유해 지목을 검증하며, selector는 지목이 있으면 후보를
그 Agent의 `worker_id` 하나로 좁힌다.

### 판정을 제출과 dispatch로 가른 선

검증을 제출에 둘 것인가 dispatch에 둘 것인가는 항목마다 답이 다르고, 가르는 기준은 **그 사실이
시간에 따라 변하는가**다.

- **제출에서 거절**: 그런 Agent가 없음, `server_hint`와 동시 지정, 명시한 `project_id`가 Agent의
  Project와 불일치. 셋 다 요청 자체가 틀린 것이고 시간이 지나도 옳아지지 않는다. dispatch까지
  미루면 요청자는 이미 `task_id`를 받고 떠난 뒤에 실패한다. 이 저장소는 `project_id`에 대해
  이미 같은 판단을 했다(`ensure_project_accepts_new_tasks`).
- **dispatch에서 거절**: Agent가 지금 돌고 있는가, 배정된 Worker가 살아 있는가. 제출 시점에
  참이어도 dispatch 시점에 거짓일 수 있으므로 제출에서 검사하면 **거짓 안심**만 준다.

### 상속 검증을 호출부가 아니라 `apply_agent_pin` 안에 둔 이유

`project_id`를 생략하면 Agent에서 물려받는다. 두 호출부는 이미 각자 `project_id`를 검증하지만
그것은 **명시된** 값에 대한 검증이고, 물려받은 값은 그 검사를 건너뛴다. 그대로 두면 보관된
Project의 Agent를 지목하는 것만으로 새 Task를 밀어 넣는 우회로가 된다 — `Draining`/`Archived`
Project에 Task가 들어가지 못하게 한 게이트를 지목 하나로 우회하는 것이다. 검증을 함수 안으로
넣으면 두 표면이 각자 기억해야 하는 규칙이 하나 줄고, 단위 테스트
(`inherited_project_must_still_accept_new_tasks`)가 그 우회로를 직접 겨눈다.

### 핀 둘을 일치 검사로 통과시키지 않은 이유

`agent_id`와 `server_hint`가 함께 오면 일치할 때만 통과시키는 방안이 자연스러워 보인다. 택하지
않았다. Agent가 아직 배정되지 않았으면(`worker_id IS NULL` — `#67` 4a가 회복 가능한 정상
상태로 정의했다) 일치를 판정할 **대상이 없다**. 그러면 같은 요청이 Agent의 배정 상태에 따라
통과했다 거절됐다 한다. 요청자가 예측할 수 없는 규칙보다 항상 거절하는 편이 낫다.

### 좁힌 문장 — 승인 없이 내 판단으로 한 것

`b7a2d06`이 커밋한 정본 문장은 "**2단계 = dispatch가 Agent를 고른다**"였다. 구현한 것은
"**`tasks.agent_id`가 실재하고 dispatch가 그것을 지킨다**"이며, 자동 선택은 3단계로 뺐다.
근거는 소비자다 — 남은 지목 셋(`tasks.agent_id` 유예 행, `#67` 게이트 ①-B·②)이 **셋 다**
dispatch가 고를 것을 요구하지 않고 "컬럼이 존재하고 채워지고 dispatch가 따른다"로 충족된다.
지금 선택기를 만들면 읽는 소비자가 없는 정책을 만드는 것이고, 어떤 Agent가 어떤 Task에 맞는지
판정할 근거(harness 구성 `#51`, AgentTemplate `#86`)도 없다.

**이 좁힘은 누구의 승인도 받지 않았다.** 근거는 `provisioning.md`의 "`#49` 2단계의 설계"
절에 남겼고 로드맵 행에도 정정으로 적었으므로, 되돌리려면 그 두 곳과 selector의 좁히기
블록만 보면 된다. 좁힌 것은 문장이지 선행 관계가 아니다 — `#67` 게이트 ①-B·②는 지목이
실재하는 것만으로 풀린다.

### 되읽을 수 없던 입력 하나

테스트를 쓰다가 알았다. Dashboard는 `agent_id`를 폼으로 받았지만 `TaskSummary`에는 그 필드가
없어서, 제출자가 **자기 지목이 실제로 붙었는지 확인할 방법이 없었다**. 입력으로만 받고
되읽히지 않는 값은 반쪽 구현이다. `project_id`와 같은 자리에 함께 싣도록 고쳤다.

### 테스트를 쓰면서 실측으로 배운 것 셋

1. **`Worker::new`는 id를 새로 뽑는다.** selector 테스트에서 `MockStore` 밖에 워커를 만들고
   그 id를 Agent의 배정으로 쓰면, 지목이 아무 워커도 가리키지 않는 채로 테스트가 통과한다.
   배정은 **store 안의** 워커에서 가져와야 한다.
2. **`handle_dispatch_task`의 실패 반환은 JSON이 아니다.** 성공은 `tool_json`이지만 실패는
   `tool_error(...)`라 평문이고, `parse_tool_json`이 `expected value, line 1 column 1`로
   panic한다. 워커가 없는 fleet에서 제출 시점 동작만 보려면 도구 반환값이 아니라 **저장된 행**을
   읽어야 한다 — 그리고 그게 옳다: 지목의 검증과 상속은 제출에서 끝나고 워커 유무는 그 뒤의
   별개 판정이다.
3. **`--no-fail-fast`는 `--` 앞이다.** cargo 플래그이고 `--test-threads=1`은 libtest 플래그다.
   뒤에 붙이면 `error: Unrecognized option: 'no-fail-fast'`로 테스트 바이너리가 **한 건도
   실행되지 않는다**. §4.3이 경고해 온 "판정처럼 보이는 것이 판정이 아닌" 형태의 하나다.

Dashboard 통합 테스트의 긍정 경로에서 단정하는 값은 `dispatched: false`다. 온라인 워커가 하나
있으므로 지목이 무시됐다면 그 제출은 그냥 성공했을 것이고, 지목된 Agent가 아직 `Ready`라서
selector가 거절하는 것이 곧 "dispatch가 지목을 지켰다"는 증거다 — 성공 단정보다 강하다.

### 검증 한계

Agent가 실제로 `Running`이고 그 Worker로 Task가 흘러가는 **성공** 경로는 단위 테스트
(`agent_pin_routes_to_the_agents_worker`, `MockStore`)로만 확인했다. 실제 워커 프로세스가
Agent를 들고 있는 E2E는 `#67` 4단계의 명령/ACK가 `observed_status`를 채워야 성립하므로
아직 없다. 즉 **지목의 거절 경로는 네 층(단위·Postgres 왕복·MCP·Dashboard HTTP)에서
확인했고, 지목의 성공 경로는 한 층에서만 확인했다.** migration `034`는 이 세션의
`fleet-store` 통합 테스트 실행에서 처음 적용됐다.

## 2026-09-01 — `#67` 게이트 ①-B: 테이블을 만드는 대신 문장 하나에 술어를 얹었다

[권한과 장애 전환](architecture/control-plane-authority-and-failover.md)은 2026-08-20부터
`worker_execution_lease`라는 11필드 레코드를 그려 두고, 구현 게이트 ①-B(낡은 제어면의 명령을
거절한다)를 그 테이블에 걸어 뒀다. 이번에 그 테이블을 **만들지 않기로 확정**했다.

### 왜 만들지 않기로 했나

필드를 하나씩 놓고 "오늘 이 값을 채울 주체가 있는가"를 물었더니 열한 개 중 하나만 남았다.

| 필드 | 처분 |
| --- | --- |
| `agent_id`, `worker_id` | `agents` 행이 이미 그 진실이다. 복제하면 갈릴 때 정본이 정해지지 않는다 |
| `task_id` | `tasks.agent_id`(034)로 역질의된다 |
| `lease_generation`, `fencing_token` | `agents.command_generation`(031)이 이름만 다르고 역할이 같다 |
| `worker_incarnation` | `workers.incarnation_started_at`(028). 028이 이미 "판정의 입력이 다시 피통제자의 자기 신고가 된다"는 이유로 heartbeat 카운터 형태를 거절했다 |
| `state`(`Activating\|Active\|Releasing`) | `Releasing`을 쓸 주체가 없다. 배정 회수 경로를 의도적으로 만들지 않았다 |
| `acquired_at`, `renewed_at`, `expires_at` | 갱신 주체가 없다. heartbeat 응답이 매번 명령 전부를 다시 싣기 때문에 만료가 필요한 창이 열리지 않는다 |
| `control_epoch` | **유일하게 채울 주체가 있고 사후 복원이 불가능하다** → `agents.command_control_epoch`(035) |

즉 이 게이트가 실제로 요구한 것은 테이블이 아니라 컬럼 하나였다. 나머지 열 개를 함께 만들면
"채울 방법이 없는 것은 미리 만들지 않는다"를 열 번 어기는 것이 된다.

### 진짜 처방은 컬럼이 아니라 술어의 자리였다

컬럼만으로는 아무것도 막지 못한다. 막는 것은 **그 값을 어디서 판정하느냐**다.

`lease_allows_control()`처럼 먼저 읽고 분기하면 관측과 쓰기 사이에 창이 남는다. 그 창에서
리스를 방금 잃은 인스턴스는 자기가 여전히 리더라고 믿은 채 명령을 남긴다. 그래서 `#62` 3단계가
Task 상태 CAS에 쓴 것과 **같은 술어**를 Agent 명령 발행 UPDATE의 **같은 문장 안에** 실었다.

```sql
EXISTS (SELECT 1 FROM control_plane_lease WHERE cluster_id = $x AND epoch = $y)
```

`assign_agent_worker`(배정)와 `set_agent_desired_status`(명령 발행) 둘 다다. 이 두 자리가
`agents.command_generation`을 올리는 유일한 자리이므로, 명령의 발행 자체가 리더십에 묶인다.

### `Fenced`를 `NoSuchAgent`와 합치지 않은 이유

반환 타입을 `bool`에서 `SlotClaim`/`CommandIssue`로 바꿨다. 둘을 합치면 리더가 아닌
인스턴스가 요청자에게 **"그런 Agent 없음"이라고 거짓말한다** — 요청자는 존재하는 Agent를
찾으러 가고, 진짜 이유(다른 인스턴스에 다시 걸면 성공한다)는 영영 전달되지 않는다.
표면 매핑도 그 구분을 지킨다: 대시보드는 503 `ApiError::Unavailable`(404·409가 아니다 —
요청은 정상이고 이 인스턴스가 리더가 아닐 뿐이다), MCP는 `JsonRpcError::internal`
(`invalid_params`로 접으면 호출자가 인자를 고치러 간다).

### 판정 순서: fence가 먼저다

`compare_and_set_task_status`가 이미 그렇게 하고 있고(`postgres.rs`), MemStore도 마찬가지다.
존재·상한을 먼저 보면 **fenced 인스턴스가 읽은 상태를 근거로 응답하게 된다** — 그 상태에는
권위가 없다. 그래서 두 메서드 모두 트랜잭션을 열기 **전에** fence를 보고 조기 반환한다.

0행이 돌아온 뒤의 사유 분류에는 `control_fence_holds()` 조회를 새로 썼다. 이것은 쓰기 술어를
**대신하지 않는다** — 관측과 쓰기 사이의 창은 여전히 문장 안의 `EXISTS`가 닫고, 이 조회는
이미 일어난 0행의 사유만 가른다.

### 배운 것

1. **fencing을 "레코드"로 상상한 것이 2주치 차단의 원인이었다.** `provisioning.md`는
   "①-B는 `worker_execution_lease` 레코드가 필요하고 그 레코드는 `task_id`를 실으므로
   `#49` 2단계에 걸린다"고 적어 뒀었다. 선행 관계가 틀린 게 아니라 **전제가 틀렸다** —
   낡은 제어면을 막는 데 필요한 것은 명령마다의 행이 아니라, 명령을 쓰는 그 문장이
   "지금도 내가 리더인가"를 함께 묻는 것이다. 자리를 먼저 만들었다면 아무도 쓰지 않는 행이
   열한 필드로 남았을 것이다.
2. **없는 것을 예고한 문서는 그 자체로 차단을 만든다.** `worker_execution_lease`는 코드에
   한 줄도 없으면서 여덟 개 문서에 걸쳐 "선행"으로 인용되고 있었다. 취소를 확정한 뒤에
   그 여덟 자리를 전부 고친 이유가 그것이다 — 하나라도 남으면 다음 세션이 다시 그것을
   기다린다.
3. **`Fenced`에 생산자를 붙이는 것이 이 작업의 절반이었다.** 타입을 나눠도 어느 테스트도
   그 값을 만들지 않으면 "채울 주체가 없는 variant"가 하나 더 생길 뿐이다. PgStore 5건 ·
   MemStore 5건으로 양쪽 백엔드에서 만들어 냈다 — 술어가 사는 자리가 백엔드마다 구조적으로
   다르기 때문에(SQL 문장 안 / Mutex 밖의 사전 검사) 한쪽만으로는 증명이 되지 않는다.

### 검증 한계

- stale fence는 리스를 1ms TTL로 잡았다 만료시킨 뒤 **다른 instance id로 재획득**해 만든
  것이다. 진짜 네트워크 분단이 아니며, 분단 중 양쪽이 동시에 쓰는 경합은 재현하지 않았다.
- 503과 `JsonRpcError::internal` 매핑에는 HTTP·JSON-RPC 수준의 테스트가 **없다**. 매핑은
  핸들러 코드를 읽어 확인한 것이지 요청을 던져 확인한 것이 아니다.
- Agent **process** 단위의 fencing은 여전히 열려 있다. 이번 변경이 막는 것은 오케스트레이터가
  명령을 **발행하는** 자리이고, 이미 워커 안에서 돌고 있는 프로세스에 세대를 물리는 것은
  비교할 process inventory가 없어 게이트 ③에 남는다.
- migration `035`는 이 세션의 `fleet-store` 통합 테스트 실행에서 처음 적용됐다.

## 2026-09-01 — lint: 취소된 `worker_execution_lease`를 아직 선행으로 지목하던 로드맵 세 행

`#67` 게이트 ①-B 커밋(`38a3633`)은 설계 문서 여덟 곳을 정정했지만, 커밋 본문이 주장한
"여덟 곳 전부"의 근거는 `docs/` 하위를 한 번 훑은 grep이었고 **로드맵 자신의 다른 행은
그 훑기에 포함되지 않았다**. 저장소 전체를 다시 훑으니 `#48`·`#63`·`#70` 세 행이 여전히
그 테이블을 *지금* 막고 있는 것으로 서술하고 있었다. 오늘 기준으로 그 서술을 읽는 사람은
영원히 오지 않을 테이블을 기다리게 된다.

세 행 모두 그 파일의 관용대로 **원문을 지우지 않고 `(**2026-09-01 정정**: …)`을 덧붙였다**.

| 행 | 낡은 서술 | 정정 |
| --- | --- | --- |
| `#48` | 차단 조건 2·3이 "Agent 엔티티와 `worker_execution_leases`가 없어" 열려 있다 | 조건은 열려 있지만 **사유가 바뀌었다** — Agent 엔티티는 `#49` 1단계로 생겼고 slot 경쟁은 ①-A-2가 닫았다. 남은 것은 *자동* provisioning과 배정 회수다 |
| `#63` | "아직 안 한 것: Worker 안의 `worker_execution_lease`/fencing token" | 테이블은 만들지 않는다. 명령 **발행**의 펜싱은 ①-B가 닫았고, 남은 것은 **프로세스** 단위(게이트 ③) |
| `#70` | 본체가 "Reconciler·`worker_execution_lease`·effect ledger"에 막혀 착수 전 | 남은 선행은 Reconciler·effect ledger와 orphan을 지목할 process inventory다 |

`#49` 행의 언급 한 곳은 **고치지 않았다** — 그것은 "넷이 `#49` 2단계를 선행으로 지목했는데
지목당한 쪽이 비어 있었다"는 이야기의 일부이며, 지목 목록을 오늘 기준으로 줄이면 그때 왜
정의가 필요했는지가 사라진다. `provisioning.md:202`도 같은 이유로 그대로 둔다.

함께 `#67` ①-B에서 내용이 바뀐 문서 여섯 곳과 로드맵의 `last_verified`를 `2026-09-01`로
올렸다. 링크는 아홉 파일의 상대 링크 전부를 해석해 확인했다(끊긴 링크 0).

**배운 것.** 커밋 본문에 "N곳 전부"라고 쓰면 그 수는 **주장**이 되고, 근거가 되는 grep의
범위가 주장의 범위보다 좁으면 그 주장은 검증되지 않은 채 이력에 남는다. 이번의 좁힘은
디렉터리가 아니라 **문서 종류**였다 — 설계 문서만 세었고 로드맵을 "내가 방금 고친 행"으로만
읽었다. 다음부터 "전부"를 쓸 때는 그 단어를 쓰기 직전에 저장소 전체를 다시 훑는다.

## 2026-09-01 — lint: 계약 문서가 존재하지 않는 검사를 있다고 말하고 있었다

`#67` 게이트 ②(Agent self-fencing)를 구현하려고 설계를 하나 세웠다. heartbeat 응답의
`AgentCommand` 봉투에 서빙 인스턴스의 `control_epoch`을 실어, 워커가 자기가 마지막으로 본
것보다 낮은 epoch의 목록을 거부하게 하는 안이었다. 근거는 두 가지였다 — 정본의 유예 표가
"남은 차단은 게이트 ①-B와 **같은 필드**"라고 적고 있었고, `HeartbeatResponse.agents`의 doc
주석이 "신선도 판정은 각 원소의 `generation`이 한다"고 적고 있었다.

**코드를 쓰기 전에 셋을 확인했더니 설계가 무너졌다.**

1. `list_agent_commands`의 프로덕션 호출자는 `crates/fleet-api/src/handlers.rs` **한 곳**이다.
   나머지는 트레이트 선언, 두 구현, `fleet-store/tests/agents.rs`의 테스트 네 곳뿐이다.
2. `PgStore` 구현은 스냅샷을 쥐지 않는다 — 트랜잭션 없는 `fetch_all(&self.pool)`이다.
3. `fleet-api` 크레이트에는 Agent 관련 인스턴스-로컬 상태가 없다. 유일한 `RwLock`은
   `cloudflare.rs`의 `JWKS_CACHE`이고, 나머지 grep 히트는 `metrics.rs`의 `cache_read`라는
   **이름의** 토큰 카운터였다.

즉 리스를 잃고도 서빙 중인 인스턴스가 내보내는 목록은 **매 beat 공유 Postgres에서 다시 읽은
값**이라 내용이 신선하고, DB에 닿지 못하면 `Err → None`이라 워커의 `reconcile`이 아무것도
건드리지 않는다. 목록 epoch은 옳은 목록을 거부할 뿐 틀린 목록을 **한 건도** 잡지 못한다.

일반형으로 적어 둔다: **펜싱이 필요한 조건은 "쓰는 쪽이 낡았을 수 있다"가 아니라 "읽은 값이
낡았을 수 있다"이다.** 여기서 서버는 상태를 쥐지 않는 순수 릴레이라 후자가 성립하지 않는다.
①-B가 봉투가 아니라 **쓰기 자리**에 술어를 건 것도 같은 이유였는데, 정본은 그것을 "같은
필드"라고 요약해 두어 정반대의 설계를 가리키고 있었다.

**정정한 것 세 곳.**

| 자리 | 무엇이 틀렸나 |
| --- | --- |
| `crates/fleet-api/src/schema.rs`, `HeartbeatResponse.agents` | "신선도 판정은 각 원소의 `generation`이 한다" — 그렇게 하는 코드가 없다. `AgentProcessManager::reconcile`은 `generation`을 비교하지 않으며, 그 값은 `warn!`/`info!` 필드로만 나온다. `generation`은 신선도 판정이 아니라 **전달 확인의 상관 키**다(워커가 `AgentAck`로 되돌려 주면 `last_acked_generation`이 오른다). 신선도를 보장하는 것은 **목록을 만드는 방식**이다 |
| 정본의 `Agent self-fencing` 유예 행 | "남은 차단은 ①-B와 **같은 필드**" — ①-B는 봉투에 넣지 않고 닫았다 |
| 정본의 `expires_at` 처분 행 | "매 beat 다시 싣기 때문에 만료가 불필요하다"는 근거는 **beat가 도착하는 동안에만** 성립한다. beat가 끊긴 구간이 정확히 게이트 6/②의 창이다. 처분(만들지 않음)은 그대로다 — 갱신 주체가 없다는 사실은 beat와 무관하다 |

035 마이그레이션에도 같은 "매번 다시 싣는다" 근거가 적혀 있지만 **적용된 파일이라 고치지
않는다**. 정정은 정본에만 둔다.

**닫는 자리를 두 번 잘못 잡았고, 그 경과를 기록한다.** 남은 실제 노출은 게이트 6 행이 적은 대로
운영자 재배정뿐이다. 가장 그럴듯한 후보였던 "`assign_agent_worker`에 *이전 배치가 확실히
멈췄을 때만* 술어를 건다"는 **실행 가능한 입력이 없다**: 두 호출자
(`fleet-dashboard/src/handlers.rs`, `fleet-mcp/src/handlers.rs`)가 이미 `status == Stopped`를
400으로 거부하므로, 정지하지 않은 Agent는 새 술어가 막고 정지한 Agent는 기존 가드가 막는다.
교집합은 `worker_id IS NULL` — 재배정이 아니라 최초 배치다.

그래서 "무엇이 프로세스의 부재를 증명하는가"를 열거했다. `last_heartbeat_at` 지연은 *도달 불가*를
증명할 뿐 *배수*를 증명하지 않고 그게 바로 위험한 쪽이며, `incarnation_started_at` 변화가 남긴
고아는 게이트 ③이 다루도록 미뤄 둔 대상이고, Worker 행 삭제는 030의 `ON DELETE SET NULL`로 이미
`worker_id IS NULL`이 된다.

**여기서 한 번 더 틀릴 뻔했다.** 위 열거를 근거로 "증명할 신호가 없으니 게이트 ②는 술어로 닫을 수
없다"까지 적었다가, 커밋 직전에 `agent_observations`의 소비 경로를 따라가 보고 걷어냈다. 증명은
이미 저장되고 있었다 — 4c-B가 만든 `agents.observed_status`/`observed_at`(032)이 워커의 권위 있는
인벤토리가 쓰이는 자리이고, `apply_agent_observations`는 목록에 없는 것의 관측을 **지운다**.

**술어의 대상을 잘못 잡았던 것이다.** `status`(운영자의 회수)가 아니라 `observed_status`(워커의
관측)를 보면 `Stopped` 가드와 컬럼이 달라 교집합 문제가 사라진다. 어휘는 `running`·`failed`·NULL
이고 `failed`의 세 이유는 모두 *프로세스가 생기지 않았다*는 뜻이므로, 술어는
`observed_status IS NULL OR observed_status = 'failed'`가 된다. 032가 "한 컬럼에 writer가 둘이
되면 안 된다"고 축을 나눠 둔 결정이 여기서 그대로 값을 했다.

단절 구간에서는 옳은 쪽으로 실패한다 — 마지막 관측 `running`이 지워지지 않아 이동을 거부하고,
워커의 프로세스 집합은 단절 중에 줄어들 수만 있다(새 프로세스는 heartbeat *응답*에서만 생기는데
그 응답이 오지 않는다). 남는 창은 **프로세스 기동과 그것을 보고하는 다음 beat 사이 한 주기**다.
무한정 열려 있던 창이 좁아지는 것이지 닫히는 것이 아니며, 그대로 적는다.

함께 드러난 비용: `#78`이 정상 종료의 deregister를 의도적으로 제거했으므로(재기동 한 번에
credential 두 테이블이 CASCADE로 사라져 register가 영구 401이 되던 결함) `worker_id`를 NULL로
만드는 경로는 관리자 삭제뿐인데, 030의 `ON DELETE SET NULL`은 `observed_status`를 남긴다. 그래서
단절된 Worker를 지운 뒤에도 그 Agent들은 `running` 관측에 막힌다 — 술어가 선택한 안전 쪽 실패이고,
대가는 회수 불가다. 막을지 강제 경로를 줄지는 설계 결정으로 남긴다.

**검증 한계.** 위 셋은 전부 grep과 소스 읽기이고 런타임 관측이 아니다. 특히 (3)의 "인스턴스-
로컬 상태 없음"은 `RwLock`/`Mutex`/`cache` 이름으로 훑은 결과라, 다른 이름의 상태가 있다면
놓칠 수 있다. "신호 열거가 완전한가"도 스키마를 읽어 세운 것이고 반증되면 결론이 바뀐다.

**배운 것 (1) — 계약 문서가 코드보다 강하게 말하면, 다음 설계는 그 문서를 근거로 세워진다.**
오늘 무너진 설계의 출발점 두 개가 모두 문서였고 둘 다 코드가 하지 않는 일을 한다고 적고 있었다.

**배운 것 (2) — 부정 결론("그런 신호는 없다")은 긍정 결론보다 반증되기 쉽다.** "없다"는 내가
훑은 범위에 대한 말이지 시스템에 대한 말이 아니다. 오늘 그 부정을 뒤집은 것은 새 정보가 아니라
**이미 같은 로드맵 항목 안에 있던 4c-B**였다 — 두 시간 전에 내가 그 봉투의 주석을 고치고 있었다.
게이트를 행 단위로 읽으면 옆 행이 이미 만들어 둔 사실을 없다고 결론짓게 된다. 그래서 이 커밋은
닫는 자리를 정본에 적되 **한 beat의 창이 남는다는 것까지 함께** 적는다 — 검증한 만큼만, 그러나
검증한 것은 빠짐없이.

## 2026-09-01 — `#67` 게이트 ②: 관측 술어와, 그것이 닫아 버린 이동 경로

**한 일.** 다른 Worker가 `running`으로 보고한 Agent의 재배정을 거절한다. 술어는
`assign_agent_worker`의 UPDATE 안에 있고(`worker_id = $2 OR observed_status IS NULL OR
observed_status = 'failed'`), 거절은 신설 `SlotClaim::ObservedRunning`이다. 술어가 볼 사실은
어제(4c-B) 이미 저장되고 있었으므로 새로 만든 관측 경로는 없다.

**설계에서 가장 중요한 것은 술어가 아니라 그 부작용이었다.** 술어를 넣고 나서 이동 경로를
따라가 보니 교집합이 최초 배정뿐이었다 — 살아 있는 Agent는 술어가 막고, 회수된 Agent는
호출부의 `Stopped` 400 가드가 막는다. 즉 게이트 ②는 이동을 **안전하게** 만든 것이 아니라
**없앤** 것이 된다. 가드의 근거였던 "원장이 세지 않으니 아무도 읽지 않는 값"도 코드를 읽어
보니 거짓이었다: `list_agent_commands`는 `last_acked_generation < command_generation`인 동안
`stopped` 행도 싣는다. 그래서 두 표면에서 가드를 걷었다. **거절하던 요청이 이제 성공하므로
계약 변경이고, 사용자가 요청한 것이 아니라 내 범위 결정이다.**

**회수 가능성은 애플리케이션이 아니라 DB에 두었다.** 마이그레이션 036이 트리거(`worker_id`가 바뀌면
세 관측 컬럼을 함께 비운다 — 032의 CHECK가 "셋 다 NULL"만 허용한다), CHECK(배정 없이 관측
없음), 백필을 함께 넣는다. 트리거인 이유는 운영자의 직접 `DELETE FROM workers`가 애플리케이션을
거치지 않기 때문이다 — `PgStore::delete_worker`에만 두면 그 경로가 stale한 `running`을 남겨
Agent를 영구히 묶는다. 이 결정이 없애려는 바로 그 상태다. `MemStore::delete_worker`에는 그
CASCADE가 아예 빠져 있었다(자격증명 둘만 지우고 `agents`는 손대지 않았다).

**읽어서 고친 거짓 주석 하나.** `assign_agent_worker`의 0행 판별 주석이 "아직 커밋하지 않은
트랜잭션 안에서 읽어 같은 스냅샷 위에서 판정한다"고 적고 있었는데, `control_fence_holds`는
`&self.pool`을 쓰고 호출은 `tx.commit()` **뒤**에 있다. `7149c88`에서 고친 문서 오류와 같은
계열이다. `issue_agent_command`가 "0행을 가르는 방식과 그 한계는 `assign_agent_worker`와 같다"로
참조하고 있어서, 참조가 여전히 참인지도 함께 맞췄다(방식은 같고 원인의 **수**는 이제 다르다).

**검증 한계.** ① 남는 창은 프로세스 기동과 다음 beat 사이의 한 주기다. 관측을 신선도로
무효화해서는 닫을 수 없다 — 그러면 분할된 Worker의 `running`이 곧바로 무시 가능해져 술어
자체가 무너진다. 게이트 ③의 몫이다. ② Worker 삭제가 관측을 지우는 것은 위험을 없앤 것이
아니라 판단의 주체를 사람으로 옮긴 것이다. 운영자가 틀렸다면 중복 실행이 그대로 발생한다.
③ 술어의 증명은 관측을 심고 이동을 시도하는 결정적 테스트이지 진짜 네트워크 분단이 아니다.

**배운 것 — 술어를 추가할 때는 그 술어가 남기는 경로를 세어 봐야 한다.** 오늘 술어 자체는
30분이면 끝났고, 나머지는 "그래서 이동은 어떻게 하는가"였다. 안전 조건을 하나 더 얹는 변경은
허용 집합을 좁히므로, 좁히고 남은 집합이 비어 있지 않은지 확인하지 않으면 기능을 없애 놓고
게이트를 닫았다고 적게 된다. `clearing_the_observation_reopens_the_move`를 테스트에 넣은 것이
그 확인이다 — 이 테스트가 없으면 나머지 다섯 개는 전부 "이동이 안 된다"만 증명한다.

## 2026-09-02 — 036의 트리거가 틀린 조건을 보고 있었다: 관측은 배치를 따라가지 않는다

**테스트가 내 마이그레이션을 반증했다.** 어제 쓴 `failed_observation_does_not_block_moving`이
이동 뒤에 `observed_status == None`을 단정하는데 `Some(Failed)`가 나왔다. 036의 트리거가
`IF NEW.worker_id IS NULL`일 때만 관측을 비우고 있었기 때문이다 — 배치가 **다른 Worker로**
옮겨가는 경우(양쪽 다 non-NULL)에는 발화 조건이 거짓이라, 옛 Worker의 `failed`가 Agent를 따라
새 Worker에게 붙는다. 새 Worker는 시도조차 하지 않았는데 "시도했고 실패했다"가 되고, 그 값은
다음 beat까지 지워지지 않는다.

조건을 `IS DISTINCT FROM OLD.worker_id`로 바꿨다. `<>`가 아닌 이유는 NULL 안전성이다 — `<>`는
한쪽이 NULL이면 결과가 NULL이 되어, 지금 고치는 결함의 거울상(배치 해제에서 안 지워짐)이 된다.
`assigned_at`은 030의 조건(NULL이 될 때만)에 그대로 둔다. `assign_agent_worker`가 같은 UPDATE에서
`assigned_at = now()`를 쓰므로, 이쪽까지 "바뀌면"으로 옮기면 방금 찍은 배치 시각을 트리거가 도로
지운다. **같은 트리거 안에서 두 컬럼 군이 서로 다른 조건을 갖는 것이 옳다**는 것이 이 수정의
요지다.

**같은 Worker로의 재배정에서 관측이 살아남아야 하는 것은 성능이 아니라 보안 조건이다.**
`IS DISTINCT FROM`이면 자동으로 그렇게 되지만, 만약 지운다면 게이트 ②가 2단계로 뚫린다 — 술어의
`worker_id = $2` 갈래가 같은 Worker로의 재배정을 허용하므로, 한 번 재배정해 관측을 없앤 뒤
아무 데로나 옮기면 된다. 술어의 갈래 하나와 트리거의 조건 하나가 **곱해져서** 구멍이 되는
모양이라, 각각만 읽으면 둘 다 옳아 보인다. 양쪽 저장소의
`same_worker_reassignment_is_allowed_while_running`에 "관측이 남아 있고, 그 뒤에도 다른 Worker로는
`ObservedRunning`이다"를 넣어 고정했다.

**같은 결함이 MemStore에도 있었고, 거기서는 소리가 나지 않았다.**
`MemStore::assign_agent_worker`도 관측 세 컬럼을 손대지 않았다. 잡히지 않은 이유는 코드가 아니라
**단정의 비대칭**이다 — PgStore 테스트는 이동 뒤 상태를 읽어 `None`을 단정했고, MemStore 테스트는
`SlotClaim::Claimed`만 봤다. 두 백엔드를 병행 유지할 때 사각지대는 구현의 차이가 아니라 테스트가
무엇까지 보느냐의 차이에서 생긴다. MemStore 쪽에도 같은 단정을 넣었다.

**Dashboard에는 어제 바꾼 계약의 테스트가 아예 없었다.** `Stopped` 400 가드를 두 공개 표면에서
걷었는데 테스트는 MCP에만 있었다(그것도 옛 동작을 고정하던 것이라 빨갛게 실패해서 존재를
알았다). 양쪽을 새 계약으로 뒤집어 다시 썼다 — MCP는 `place_agent_accepts_a_stopped_agent`,
Dashboard는 같은 이름의 통합 테스트다. **계약을 두 표면에서 바꿨는데 한쪽만 덮여 있으면, 나중에
한쪽이 되돌아가도 아무도 모른다.**

**검증 한계.** ① 036은 커밋 전이라 편집했고, 이미 적용된 `fleet_test`와 체크섬이 어긋나므로
`dropdb && createdb`로 다시 만들었다. 그 결과 **백필은 빈 테이블에 대해서만 돌았다** — CHECK를
실제로 위반하는 행을 두고 백필을 돌린 적은 한 번도 없다. 어제 적은 한계보다 약해진 것이다.
② 트리거의 새 조건은 결정적 테스트로 증명했을 뿐, 운영자의 직접 `UPDATE agents SET worker_id`
같은 애플리케이션 밖 경로에서는 확인하지 않았다(트리거를 고른 이유가 그 경로인데도 그렇다).

**배운 것 — 조건 하나를 두 목적에 재사용한 것이 원인이었다.** 030의 트리거는 `assigned_at`
하나만 다뤘고 그 조건은 옳았다. 036이 거기에 관측 세 컬럼을 얹으면서 **조건은 그대로 두고
본문만 늘렸다.** 새 컬럼이 옛 컬럼과 같은 생애주기를 갖는지 묻지 않은 것이다. 트리거를 확장할
때 물어야 할 것은 "여기에 넣으면 편한가"가 아니라 "이 컬럼이 저 조건과 같은 순간에 무효가
되는가"다.

## 2026-09-02 — 코드는 고쳤는데 정본 두 곳이 옛 조건을 계속 말하고 있었다

`7cf13c1`을 푸시하고 CI 4개 잡이 전부 초록인 것을 확인한 직후, 다음 작업을 고르려고
로드맵을 읽다가 `#67` 게이트 셀이 **"`worker_id`가 NULL이 되면 트리거가 세 관측 컬럼을 함께
비우고"** 라고 적혀 있는 것을 봤다. 그날 아침에 반증하고 고친 바로 그 조건이다. 같은
문장이 [권한과 장애 전환](architecture/control-plane-authority-and-failover.md)에도
"Worker 삭제가 그 Worker의 Agent 관측도 함께 지운다"는 형태로 남아 있었다 — 이쪽은 거짓은
아니지만 트리거가 **배치 변경 전부**에 발동한다는 사실이 빠져 있어, 읽는 사람이 삭제
경로만 덮는다고 결론 내리게 된다.

**상태 셀만 갱신하고 본문을 갱신하지 않았다.** 커밋 직전에 `#67` 행에 "게이트 ② 트리거
정정 2026-09-02"을 덧붙였고, 그 한 줄이 "로드맵을 갱신했다"는 감각을 만들었다. 그러나
상태 셀은 **무슨 일이 있었는지의 목록**이고 게이트 셀은 **지금 무엇이 참인지의 서술**이다.
전자를 고치는 것은 후자를 고치는 것의 증거가 되지 않는다. agent.md §4.3이 "서술만 갱신하고
실행 목록을 갱신하지 않은 것"을 재발 원인으로 적어 둔 것과 **방향만 반대인 같은 결함**이다:
거기서는 로그가 갱신되고 실행 목록이 남았고, 여기서는 이력이 갱신되고 서술이 남았다.
두 경우 모두 원인은 새로운 무지가 아니라 **한 문서 안에서 성격이 다른 두 칸을 한 번의
갱신으로 덮었다고 착각한 것**이다.

**그래서 코드 게이트는 이 종류를 잡지 못한다.** `cargo test`도 clippy도 문서가 코드와
어긋나는 것을 모른다. 이번에 발견된 것도 게이트가 아니라 **다음 작업을 고르려고 로드맵을
읽은 우연** 때문이다. 커밋에 이미 들어간 뒤였다는 것도 정직하게 적는다 — 발견이 하루
늦었다면 그 문장은 다음 세션의 전제가 됐을 것이다.

**고친 것**: 로드맵 게이트 셀에 정정 경위(왜 `IS DISTINCT FROM`인지, 왜 `assigned_at`은
030의 조건을 유지하는지, 같은 Worker 재배정에서 관측을 남기는 것이 왜 게이트 ②의 전제인지)를
넣고 테스트 개수를 12건에서 13건으로 고쳤다(대시보드 통합 테스트가 빠져 있었다). 권한과 장애
전환 문서에는 트리거 발동 조건의 정정을 한 문단으로 덧붙였다.

## 2026-09-02 — `#69`의 완료 게이트 넷 중 셋은 이름만 있고 대상이 없었다

`#49` 2단계가 어제 닫혀서 `#69`의 선행이 풀렸다고 보고 착수했는데, 설계 정본을 읽는 단계에서
멈췄다. **로드맵이 완료 게이트로 지목한 "secret scan"이 `docs/` 전체에서 그 행 말고는 한 번도
나오지 않는다.** `grep -rn 'secret scan\|시크릿 스캔' docs/`가 로드맵 행 자신만 잡는다.
"checkpoint"는 다섯 문서에 나오지만 전부 스쳐 지나가는 언급이고 소유 절이 없다. 게이트 넷 중
worktree isolation 하나만 정본에 대상이 있었다.

이것은 2026-09-01에 `#49` 2단계가 겪은 것과 **같은 모양**이다 — 그때 프로비저닝 문서가
"지목이 넷인데 대상이 비어 있으면 지목마다 다른 것을 상상하게 되고"라고 적으며 범위 절을
새로 쓴 그 결함이다. 다른 점은 발견 시점뿐이다: 그때는 구현 도중이었고 이번에는 착수 직전이다.

**선행에 대한 서술도 틀려 있었다.** 로드맵 행은 "넷 다 'Agent별 worktree'를 전제하고, 그
바인딩은 `#49` 2단계에 있다"고 적어 뒀는데, 그건 첫 번째에만 참이다. checkpoint push/restore는
push credential을 발급하는 주체에 걸리고, secret scan은 push가 존재하는 것에, 이동 복구 E2E는
그 둘에 걸린다. `#49` 2단계가 닫혔다는 사실만 보고 "`#69`가 열렸다"로 읽었다면 셋을 착수한
뒤에 막힘을 재발견했을 것이다. 어제 로그가 "코드는 고쳤는데 정본이 옛 조건을 말하고 있었다"로
적은 것과 방향이 같다 — 선행이 바뀌었는데 그것을 기술한 칸은 그대로였다.

**push credential이 없다는 것이 결정적이다.** 실행 격리 정본은 Git push credential을 "Security
Manager가 실행 구간에 묶어 발급한 credential으로만" 제공하고 user global credential store·SSH
agent forwarding·repository config의 embedded token을 **모두** 금지한다. 그 Security Manager를
소유하는 로드맵 항목이 없다. 즉 push할 수단 자체가 존재하지 않으므로 게이트 2·3·4는 1단계에서
원리적으로 닫을 수 없다. 로컬 커밋까지는 가능하지만 그것은 checkpoint가 아니다 — Worker 이동은
디스크를 바꾸므로 로컬 커밋은 따라가지 않는다.

**새로 정의한 것**: 실행 격리 정본에 "`#69` 1단계의 범위" 절을 만들어 Project Git repository,
worktree, checkpoint, secret scan 넷의 대상을 정의하고, 게이트별 실제 선행과 1단계가 아닌 것을
표로 적었다. secret scan은 독립 기능이 아니라 push의 전제 조건으로 정의했다 — 최소 대상이
**Fleet 자신이 그 worktree에 넣은 credential**이기 때문이다. 실행 구간에 묶어 발급한 credential이
`.git/config`의 embedded token이나 helper가 쓴 파일로 worktree에 남으면 다음 checkpoint가 그것을
원격으로 실어 나른다. 사용자 secret 탐지는 그 위에 얹는 것이지 그 반대가 아니다.

**결정한 것 — Worker는 repository를 어떻게 아는가**. worktree를 만들려면 Worker가 Agent의
Project와 그 repository를 알아야 하는데 지금은 둘 다 모른다. heartbeat 응답의 `AgentCommand`는
`(agent_id, desired_status, generation)` 셋뿐이고 그 키 집합은 `schema.rs`의 테스트가 잠그고
있다(`assert_eq!(keys, ["agent_id", "desired_status", "generation"])`). 4c-A는 port·secret·cwd를
워커 로컬 파생으로 해결해 이 봉투를 좁게 유지했지만 repository URL은 로컬로 파생할 수 없다.
봉투를 넓히는 쪽으로 가면 `HeartbeatResponse` 주석이 경고한 것이 그대로 실현된다 — repository
URL은 `https://user:token@host/...` 형태를 가질 수 있으므로 응답이 통째로 로깅해도 안전한 값이
아니게 된다. **별도 인증 호출로 간다.** 근거는 취향이 아니라 비대칭이다: 응답의 로깅 안전성은
한 번 깨지면 그 뒤의 모든 코드가 그 전제로 쓰이므로 되돌릴 수 없고, 반대쪽 비용은 엔드포인트
하나다.

**검증 한계**: 이 커밋은 문서만 바꾼다. 정의한 대상 중 코드에 존재하는 것은 아직 없고,
`projects` 테이블에는 repository 컬럼이 없으며 워크스페이스에 Git 라이브러리 의존성도 없다
(`git2`/`gix` 모두 없음). 1단계 구현은 다음 커밋이다.

## 2026-09-02 — 범위를 정의한 커밋이 같은 날 두 번째 정정을 받았다

`584ee78`이 `#69`의 게이트 넷에 대상을 부여하면서 1단계를 "worktree isolation 하나"로 잡았다.
그 커밋을 밀자마자 표면 조사를 한 번 더 돌렸고, 두 가지가 나왔다.

**(1) credential 부재는 push보다 넓다.** 정본의 조항은 "Git push credential은 Security Manager가
발급한 것만"이라고 말하므로, 첫 판은 clone·fetch는 걸리지 않는다고 읽었다. 조항으로는 맞는
독해다. 사실로는 틀렸다 — 조항이 금지해서가 아니라 발급자가 아예 없어서 clone도 막힌다.
credential 마이그레이션은 `005_worker_credentials.sql`과 `018_worker_operational_credentials.sql`
둘뿐이고 모두 Worker 소유의 LLM 자격 증명이며, `fleet-core`에 `Credential` 타입은
`grep 'pub struct.*Credential\|pub enum.*Credential'`로 **0건**이다. 익명 clone만 되는 checkout은
게이트가 말하는 isolation이 아니고, 그 위에 secret scan도 checkpoint도 올릴 수 없다.

**금지 조항을 읽고 범위를 정하면 이렇게 틀린다.** 조항은 "무엇을 쓰면 안 되는가"를 말하고
범위는 "무엇을 쓸 수 있는가"에 달려 있는데, 둘은 같은 문장에서 읽히지 않는다. 조항이 push만
언급했다는 사실은 fetch가 가능하다는 뜻이 아니라 **fetch에 대해 아무 말도 하지 않았다**는
뜻이었다. 확인은 문서가 아니라 마이그레이션 목록과 타입 grep이 해 줬다.

**(2) Agent 디렉터리는 만들어지기만 하고 지워지지 않는다.** `agent_process.rs`에 `remove_dir`가
한 번도 나오지 않는다. 정본의 "실행 snapshot과 cleanup"이 cleanup 규약을 정해 두었지만 그것을
수행하는 코드가 없다. `584ee78`의 범위 절은 이 사실을 적지 않았다 — 게이트 이름을 정의하는 데
집중하느라 이름이 붙지 않은 결함을 지나쳤다.

이 둘이 합쳐져 1단계가 바뀐다. Git이 빠지고 **Agent 디렉터리의 경계와 생명주기**가 들어온다.
경계 쪽에서 값어치를 하는 검사는 `..` 거절이 아니라 symlink 저항이다 — `agent_id`는 UUID라
경로 조작이 원리적으로 섞이지 않지만, workspace root나 그 하위가 심볼릭 링크면 canonical 경로는
root 밖으로 나간다. 이 검사가 `Task.cwd`의 containment와 다른 이유는 **경로를 정한 쪽과 확인하는
쪽이 같기** 때문이다. 거기서 막힌 것은 오케스트레이터가 남의 파일시스템을 canonicalize할 수
없다는 사실이었지 검사 자체가 아니었다.

**삭제 판정이 이 정정의 핵심이고, 그것 때문에 구현을 시작할 수 없었다.** `reconcile`은 명령
목록을 권위 있는 전체 집합으로 읽고 목록에 없는 프로세스를 종료하며, 그 주석은 사라진 것을
"재배정됐거나 이미 회수가 확인된 경우이며 어느 쪽이든 들고 있을 이유가 없다"고 적었다.
프로세스에 대해서는 맞다. 디렉터리에 그대로 적용하면 틀린다 — 종료는 다시 띄우면 되지만 삭제는
되돌릴 수 없고, checkpoint push가 없는 지금은 그 작업물의 복구 경로가 아예 없다.

부재가 모호한 이유는 `list_agent_commands`의 술어에 있다:
`WHERE worker_id = $1 AND (status <> 'stopped' OR last_acked_generation < command_generation)`.
`worker_id`가 다른 Worker로 바뀌었을 때, NULL이 됐을 때, 그리고 회수가 **확인까지 끝났을 때**
모두 행이 조용히 빠진다. 셋은 서로 다른 사실인데 도착하는 값은 하나다. `Option`의 `None`은
조회의 전체 실패만 덮으므로 이 셋을 가르지 못한다.

그래서 판정을 뒤집는다 — 부재가 아니라 **명시적 `desired_status = Stopped`**를 삭제 근거로 삼는다.
같은 술어의 `last_acked_generation < command_generation` 덕분에 회수 명령은 확인이 올 때까지 매
beat 실려 오므로, 그 창 안에서 Worker는 회수를 모호하지 않게 본다. 놓치면 디렉터리가 남고(누수,
다음 회수 신호나 운영 정리로 회수 가능), 부재로 지우면 작업물이 사라진다(복구 없음).
`DELETE FROM agents`가 코드베이스에 없어 Agent 행은 하드 삭제되지 않으므로, 회수 신호를 낼
주체는 언제나 존재한다.

**검증 한계**: 여기까지는 문서와 조사뿐이고 코드는 아직 한 줄도 바뀌지 않았다. 위 술어에 대한
독해는 `postgres.rs`의 쿼리와 그 주석에 근거하며, `mem.rs`가 같은 술어를 흉내 내는지는 이
시점에 확인하지 않았다 — 구현할 때 확인한다.

## 2026-09-02 — 부재는 종료의 근거는 되지만 삭제의 근거는 아니다

`#69` 1단계를 구현했다. 범위는 같은 날의 두 번째 정정이 정한 것 — Git이 아니라 **Agent
디렉터리의 경계와 생명주기**다.

**경계.** `AgentProcessManager`가 `workspace_root.join(agent_id)`로 만들던 경로에
`ensure_workspace`/`contained`를 넣었다. `agent_id`는 UUID라 `..`도 구분자도 섞일 수 없으므로
여기서 값어치를 하는 검사는 경로 조작 방어가 아니다. 실제로 막는 것은 **`workspace_root/<agent_id>`
자리에 이미 심볼릭 링크가 있는 경우**다 — `create_dir_all`은 대상이 디렉터리로 존재하면 조용히
성공하고, 그 뒤의 모든 쓰기가 링크가 가리키는 곳으로 나간다.

**한쪽만 정규화하면 정상 구성이 위반이 된다.** root 자신이 링크인 것(workspace를 다른 볼륨에
두는 배치)은 막을 이유가 없다. 양변을 `canonicalize`하면 자식은 root가 가리키는 실제 경로 아래에
놓여 `starts_with`가 성립하고, 자식 쪽 링크만 밖으로 나간다. 정정 커밋 `d181ffe`의 정본 문장은
"root 자체나 그 하위가 심볼릭 링크면 밖으로 나간다"고 적었는데 앞 절반이 틀렸고, 구현하면서
드러나 같이 고쳤다 — **문서가 코드보다 앞서면 이런 종류의 오류는 구현 전에는 드러나지 않는다.**

거절은 새 variant 없이 `AgentObservationReason::SpawnFailed`로 실린다. 그 독스트링이 이미
"바이너리 부재, workspace 생성 실패 등"을 포함하고 있었다.

**생명주기 — 이 변경의 실제 내용은 삭제 판정이다.** `agent_process.rs`에는 `remove_dir`가 한 번도
없었고, 정본의 cleanup 규약을 수행하는 코드가 없었다. 그런데 삭제를 그냥 쓸 수 없었다.

`reconcile`은 명령 목록을 권위 있는 전체 집합으로 읽고 목록에 없는 프로세스를 종료한다. 그
주석은 사라진 것을 "재배정됐거나 이미 회수가 확인된 경우이며 어느 쪽이든 들고 있을 이유가
없다"고 정당화한다. **프로세스에 대해서는 맞고 디렉터리에 그대로 옮기면 틀린다.** 종료는 잘못해도
다시 띄우면 되지만 삭제는 되돌릴 수 없고, checkpoint push가 없는 지금 그 작업물에는 복구 경로가
아예 없다.

부재가 모호한 근거는 `list_agent_commands`의 술어다 —
`WHERE worker_id = $1 AND (status <> 'stopped' OR last_acked_generation < command_generation)`.
다른 Worker로 이동, 미배치(`worker_id = NULL`), 회수 확인 완료가 모두 부재로 뭉쳐진다. 셋은
서로 다른 사실인데 Worker에 도착하는 값은 하나이고, `Option`의 `None`은 조회의 **전체** 실패만
덮으므로 이 셋을 가르지 못한다. `mem.rs`가 같은 술어를 그대로 흉내 내는 것을 확인했다 —
`d181ffe`가 검증 한계로 남겨 둔 항목이며, 여기서 해소됐다. 판정은 백엔드에 무관하다.

그래서 부재가 아니라 **명시적 `desired_status = stopped`**를 근거로 삼는다. 같은 술어의 두 번째
disjunct가 회수 명령을 확인 전까지 실어 주므로 그 창 안에서는 모호하지 않다. 창은 최악의 경우
한 beat이다 — `registration.rs`는 응답을 파싱하는 시점에 ack를 버퍼에 넣고 그것이 다음 요청에
실리는데, 오케스트레이터의 heartbeat 핸들러가 `ack_agent_commands`(handlers.rs:362)를
`list_agent_commands`(405)보다 **먼저** 부르므로 그 응답에는 이미 이 명령이 없다. 회수가 `status`
컬럼까지 내리지 않은 경우에는 첫 disjunct가 살아 있어 더 오래 오지만, 짧은 쪽에 맞춰 설계했다.

그래서 삭제 실패는 전파하지 않고 경고만 남긴다. **틀리는 방향을 고른 것이다** — 놓치면 디렉터리가
남고(누수, `DELETE FROM agents`가 코드베이스에 없어 회수 신호를 낼 주체는 언제나 존재한다),
부재로 지우면 살아 있는 Agent의 작업물이 사라진다(복구 없음). 삭제 직전에 `contained`를 한 번 더
통과시키는 것도 같은 이유다 — 생성과 삭제 사이에 링크가 끼어들 수 있고, 재귀 삭제는 되돌릴 수
없으므로 같은 검사의 값이 두 자리에서 다르다.

**순서에 대한 관측.** 2.5단계는 `terminate_all(doomed).await`의 **뒤**에 있고, 2단계의
`keep` 집합은 `desired = running`만 담으므로 `stopped` 명령을 받은 Agent의 프로세스는 디렉터리가
지워지기 전에 이미 종료된다. 이것이 우연이 아님을 테스트가 역으로 보여 줬다 —
`removing_an_already_removed_directory_is_not_an_error`는 프로세스를 띄운 채 `remove_workspace`를
직접 부르는데, 그때 가짜 grok의 셸이 `shell-init: error retrieving current directory: getcwd`를
뱉는다. 즉 **cwd를 들고 있는 프로세스가 살아 있는 동안 지우면 그 프로세스가 깨진다**는 것이고,
운영 경로에서 그 일이 일어나지 않는 이유는 2단계와 2.5단계의 순서다. 순서를 바꾸면 이 소음이
운영 로그로 올라온다.

**검증 한계.** 신규 테스트 6건은 모두 단위 시험이고 가짜 grok 스크립트를 쓴다 — 실제 Agent가
worktree에 파일을 쓰는 동안의 삭제는 시험하지 않았다. 한 beat 창에 대한 서술은 두 핸들러의 호출
순서를 읽어 얻은 것이지 실제 beat 두 번을 태워 확인한 것이 아니다. symlink 시험 2건은
`#[cfg(unix)]`이라 Windows에서는 존재하지 않으며, 이 프로젝트는 Windows를 지원 대상으로 삼지
않으므로 그 자리는 비어 있는 채로 둔다.

**빌드 시간에 대한 기록.** 이 변경을 시험하는 동안 `cargo test -p fleet-worker`가 11분 52초에
크레이트 2개를 컴파일하는 속도로 진행했다. agent.md §3.3 기록 2가 규명한 `target/debug/deps`의
111만 항목 문제와 같은 원인이며, 거기서 A/B로 확정한 978배 페널티가 컴파일 자체에도 걸린다는
관측이다. 그 기록은 테스트 **런타임**에 대한 것이었고 이것은 **빌드**다. 대응(target 정리)은
같지만 여기서는 실행하지 않았다.

## 2026-09-02 — 명령을 기록하는 것과 전달을 기록하는 것은 다르다

`#67` 구현 게이트 ④("명령·ACK·actor·generation의 감사 추적")를 닫았다. 시작은 표를 하나 그린
것이었다 — `command_generation`을 올리는 경로와 그것을 감사에 남기는 경로를 나란히 놓자 셋 중
하나만 남기고 있었다는 것이 드러났다.

| 감사 이벤트 | 세대를 올리는가 | 그때까지의 `detail.generation` |
|---|---|---|
| `agent.create` | 아니오 (기본값 0) | 없음 |
| `agent.assign` | 언제나 +1 | **없음** |
| `agent.start` | `desired_status`가 바뀔 때 +1 | 있음 |
| `agent.stop` | `desired_status <> 'stopped'`일 때만 +1 | **없음** |

`AGENT_START`의 docstring이 이미 이유를 적어 두고 있었다 — "그것이 있어야 나중에 Worker의
ACK(`last_acked_generation`)와 이 이벤트를 맞대어 '그 명령이 실제로 전달됐는가'를 감사 로그만으로
답할 수 있다." 그 이유는 `assign`과 `stop`에도 **그대로** 성립하는데 값만 없었다. 근거가 특정
경로에 붙어 적히면, 같은 근거가 걸리는 다른 경로는 아무도 다시 읽지 않는다.

**세대 없는 감사 줄이 말하지 못하는 것은 "언제"가 아니라 "닿았는가"다.** `agent.assign`이 시각과
출발지·도착지를 남겨도, 그 재배정이 새 Worker에 도달했는지는 알 수 없다. 배정과 전달이 한 줄
안에서 구분되지 않기 때문이다. 이것은 정보가 부족한 것이 아니라 **발행을 전달로 읽게 만드는**
종류의 부재다.

### `stop`만 조건부인 이유

`assign`은 무조건 싣는다. `assign_agent_worker`가 값이 같아도 세대를 올리기 때문이다 — 새 Worker는
이전 Worker가 받은 명령을 본 적이 없으므로 그것이 옳다.

`stop`은 다르다. 한 번도 start되지 않은 Agent를 회수하면 `desired_status`가 이미 `stopped`라
저장소가 세대를 올리지 않고, Worker로 나가는 명령도 없다. 그런데 저장소는 "명령을 발행했는가"를
돌려주지 않는다 — `update_agent_status`의 반환은 `bool`이고 그 뜻은 "행이 있었는가"다. 그래서
호출부가 쓰기 **전후**의 `command_generation`을 직접 비교하는 것 말고는 알 방법이 없다.

**여기서 `null`과 `0`의 차이가 안전 문제가 된다.** `last_acked_generation`의 기본값도 0이므로,
명령이 없었던 자리에 0을 실으면 `last_acked_generation >= 0`이 언제나 참이다 — 대조가 늘
"전달됨"으로 나온다. 즉 0은 값이 약한 것이 아니라 **거짓을 참으로 만든다**. `null`만이 "맞대어 볼
명령이 없다"를 말하고, 회수 이벤트 자체는 언제나 기록되므로 `null`이 이벤트의 부재와 혼동되지도
않는다.

`agent.create`에 세대를 싣지 않은 것은 같은 판단의 반대편이다. 생성은 명령이 아니라 정의이고
세대는 0에 머문다 — 여기에 0을 실으면 위와 똑같은 거짓 대조가 하나 더 생긴다. 채울 방법이 없는
칸은 만들지 않는다.

### 테스트가 감사 줄에서 세대를 **읽어서** ACK에 넣는다

세 테스트(`placing_an_agent_audits_the_generation_and_the_ack_closes_the_trail`,
`stopping_a_started_agent_audits_the_generation_it_issued`,
`stopping_a_never_started_agent_records_no_generation`)는 detail의 세대를 상수로 다시 적지 않고
**감사 줄에서 읽어** `ack_agent_commands`에 넣은 뒤 `command_delivered()`가 뒤집히는지를 본다.
세대를 싣는 이유가 그 대조이므로, 값이 있다는 것만 보면 값이 **쓸모 있다**는 것은 보지 않은
것이다. 배정 테스트는 `actor_user_id`도 함께 단정한다 — 세대가 있어도 actor가 비면 "누가"가 빠져
추적이 완결되지 않고, 게이트 ④의 이름에 actor가 들어 있는 이유가 그것이다.

`GET /api/agents/{id}` 라우트는 없다(목록만 있다). 전달 표시는 저장소를 직접 읽어 확인했다.

### 검증 한계

MCP 표면은 손대지 않았다. `fleet_stop_agent`를 비롯한 tool은 감사 이벤트를 **하나도** 쓰지 않는데,
세대를 실을 자리가 없어서가 아니라 `ToolContext`가 `state`/`dispatcher`/`capabilities`만 들고 있어
**actor를 채울 수 없어서**다. actor 없이 줄만 만들면 `actor_label`이 비거나 상수가 되어, 추적이
완결된 것처럼 보이는 줄이 쌓인다 — 없는 것보다 나쁘다. 그 인증 설계(짧은 session assertion 또는
local peer identity)는 [인가와 감사](security/authorization-and-audit.md)가 정본으로 들고 있고
"MCP tool별 감사도 착수 전"으로 이미 기록해 둔 항목이라 이번 범위 밖에 둔다.

또 하나: 이 게이트가 증명하는 것은 **감사 로그로 대조가 성립한다**는 것까지다. 실제 Worker가
ACK를 보내는 경로(heartbeat)는 별개로 이미 있지만, 이 테스트들은 `ack_agent_commands`를 직접
호출하므로 heartbeat 왕복 자체를 다시 시험하지는 않는다.

## 2026-09-02 — 값을 저장하는 것과 그 값으로 조회할 수 있는 것은 다르다

`#95` 1단계로 `audit_log`에 `project_id` 상관관계 컬럼을 넣었다. 대기 사유였던 "Project 엔티티가
아직 없다"는 `#48` 1·2·3단계(2026-08-24, `022_projects.sql`)로 이미 해소돼 있었고, 그 뒤로도
로드맵 행이 옛 사유를 그대로 들고 있었다.

### `detail` JSONB에 이미 넣고 있었는데 왜 컬럼인가

Project 범위 감사 지점 11곳 중 5곳이 `detail.project_id`에 값을 싣고 있었다. 나머지 6곳은 아니었다.
이것이 **관행이지 계약이 아니었다**는 증거다 — 컴파일러가 강제하지 않으니 저자가 기억한 곳에만 값이
있었다. 그리고 값이 있는 5곳조차 **조회할 수 없었다**. `detail`은 저장될 뿐 색인되지 않고,
`AuditFilter`에는 그 축의 술어를 걸 자리가 없다.

가장 날카로운 자리는 `agent_template.*` 세 지점이다. 이들은 `authorize_template_scope(&principal,
template.project_id)`를 호출한다 — **인가 판단에 이미 `project_id`를 쓰면서 그 판단의 감사 기록에서는
그 값을 버리고 있었다.**

### 컬럼·술어·파라미터를 한 변경으로 넣은 이유

같은 저장소에 선례가 있다. `AuditFilter::actor_user_id`는 오래전부터 존재하지만 `ListAuditQuery`가
그것을 노출한 적이 없어 `list_auth_audit_api`가 항상 `None`을 하드코딩한다 — 즉 **행위자 축으로는
아무도 조회할 수 없다.** 필드만 만들고 표면을 열지 않으면 죽은 축이 하나 더 생긴다. 그래서
`project_id`는 컬럼 + `AuditFilter` 술어 + `?project_id=` 쿼리 파라미터를 한 커밋에 넣었다.

### FK를 걸지 않았다

감사는 **시도**의 사실을 기록하며, 존재한 적 없는 Project를 지목한 거절도 그 사실에 포함된다. FK를
걸면 감사 쓰기가 실패하는 시점이 하필 기록할 가치가 가장 큰 순간과 겹친다. `011`이 `actor_user_id`에
`ON DELETE SET NULL`을 고른 것과 같은 계열이되 근거가 다르다 — 거기서는 대상이 사라져도 기록이
남아야 해서, 여기서는 **대상이 애초에 없었어도** 기록이 남아야 해서다.
`audit_records_a_project_that_does_not_exist`가 이 결정을 테스트로 고정한다.

### 형식이 깨진 `project_id`는 400이다

조용히 `None`으로 떨어뜨리면 "그 Project에 아무 일도 없었다"가 아니라 **필터가 통째로 무시된 전체
목록**이 돌아간다. 감사 표면에서 그 실패 양식은 과소 보고보다 위험하다. 형식은 맞지만 존재하지 않는
Project는 빈 목록이며 400이 아니다 — 그것은 "질문이 잘못됐다"가 아니라 "답이 없다"이기 때문이다.

### backfill을 넣은 이유와 두 절이 필요한 이유

컬럼만 추가하면 `?project_id=X`가 도입 이전 이벤트를 한 건도 돌려주지 않는다. 감사 표면에서 조용한
누락은 "그 Project에서 아무 일도 없었다"로 읽히므로 부분 구현보다 나쁘다. 절이 둘인 것은 값이 두
곳에 흩어져 있어서다 — `agent.*`/`issue.*`는 `detail`에, `project.*`는 대상 자체가 Project라
`target_id`에 있다. 첫 절의 `WHERE project_id IS NULL` 가드가 순서 의존성과 멱등성을 동시에 준다.

버려진 DB(`fleet_bf_probe`)에 `001`~`036`을 적용하고 legacy 모양의 행 7건을 넣은 뒤 `037`을 돌려
확인했다: `detail`에 값이 있던 2건과 `target_type='project'`인 2건이 채워지고, 나머지 3건은 NULL로
남았다. `detail`에 `"project_id":"not-a-uuid"`를 넣으면 `::uuid` 캐스트가 시끄럽게 실패한다는 것도
확인했다 — 감사 데이터를 조용히 버리지 않는 쪽이 원하는 동작이다. 확인 후 `dropdb`로 정리했다.

### `/api/audit`는 그동안 테스트가 0건이었다

이 엔드포인트를 건드리는 테스트가 저장소 전체에 없었다. 참조는 라우트 등록 한 줄과 핸들러뿐이다.
그래서 docstring이 "인증/권한 감사 로그를 반환한다"고 **거짓을 말하고 있는 것도 드러난 적이
없었다** — `audit_log`는 처음부터 `agent.*`·`issue.*`·`project.*`를 담고 있었고, 함수명
`list_auth_audit_api`가 그 오해를 굳혀 왔다. 이번에 붙인 테스트가 그 문장을 반증했고, 주석 두 곳과
계약 문서를 사실에 맞췄다. 컴파일러가 잡을 수 없는 종류의 거짓은 **표면에 테스트가 붙는 순간에만**
드러난다.

### 2단계로 미룬 것

| 필드 | 판정 | 근거 |
|---|---|---|
| `request_id` | 보류 | 한 요청이 감사 이벤트를 2건 이상 내는 경로가 **코드베이스에 없다**. 묶을 것이 없는데 묶는 키를 만들면 항상 1:1인 칸이 생긴다 |
| `policy_revision` | 보류 | policy revision 개념 자체의 생산자가 없다 |

측정 근거: dashboard `login`의 `audit::record` 4회는 서로 배타적인 분기이며 각각 `return Err(...)`로
끝난다. fleet-api의 fail-closed mint 경로는 감사 실패 시 회수(revoke)하는데, 이는 **보상이지 두 번째
감사 이벤트가 아니다**. `fleet-mcp`·`fleet-worker`·`fleet-scheduler`는 감사 기록이 0건이다.

### 검증 한계

1단계는 **범위 강제를 만들지 않는다.** `GET /api/audit`는 여전히 `PermissionKind::AuditRead` 하나로만
잠겨 있어, 그 권한을 가진 사람은 아무 Project의 감사나 읽을 수 있다. 이번에 넣은 것은 그 축으로
**거를 수 있게** 하는 것까지다. 인가와 감사 정본의 구현 게이트 6은 여전히 미충족이며, 막고 있는 것은
`#58`과 같은 것 — 승인된 Project 멤버십 모델이 없다는 사실이다.

### 게이트

7단계 전부 `exit=0`. `rustc 1.98.0`(CI와 일치), fmt, clippy 두 피처 세트, 그리고 각 세트마다
`cargo build -p fleet-cli` 후 `DATABASE_URL` 주입 `cargo test --workspace --no-fail-fast --
--test-threads=1` — acp+mtls 74 스위트 1321건, no-default 74 스위트 1317건, 실패 0.

새 테스트 4건이 양쪽에서 **실행됐음을 이름으로** 확인했다. 통과 개수로는 확인할 수 없다 —
`require_db!`가 `DATABASE_URL` 부재 시 조용히 반환하므로 건너뛴 테스트도 `ok`로 찍힌다.
`audit_integration`이 9건 0.27s/0.21s(0.00s 아님)로 실제 DB를 왕복했고, `037`이
`_sqlx_migrations`에 `success=t`로 남았으며 `audit_log.project_id`와 `idx_audit_log_project`를
`psql`로 직접 봤다. §3.2가 "개수가 아니라 소요 시간이 조용한 skip을 드러낸다"고 적은 지표를
이번에는 통합 스위트가 실제로 DB에 닿았는지를 읽는 데 썼다.

`fleet_test`는 매 테스트가 TRUNCATE하므로 **backfill이 실데이터에 대해 도는 것은 이 게이트가
시험하지 않았다.** 그것은 버려진 DB에서만 확인됐다.

## 2026-09-03 — 거절을 기록할 수 있는 자리는 거절을 판단한 자리뿐이었다

`crates/fleet-dashboard`의 `/api` 권한 거절이 감사에 남지 않고 있었다. `fleet-api`는 `#76`에서
`http.capability_denied`로 이미 거절을 기록하고 있었으므로, 두 표면 중 한쪽만 보이는 상태였다.
거절이 보이지 않으면 권한 열거(enumeration) 시도가 감사 표면에서 **성공한 요청만 남은 그림**과
구분되지 않는다.

### 기록할 수 있는 자리는 `require_permission` 하나뿐이었다

이 함수는 `Result<(), StatusCode>`를 돌려주고 실패는 `StatusCode::FORBIDDEN` 하나다. 호출부는
`error.rs`의 `impl From<StatusCode> for ApiError`에 기대어 `?` 하나로 넘기는데, **그 변환 시점에는
어떤 `PermissionKind`가 없었는지가 이미 사라져 있다.** 따라서 하류의 어떤 오류 변환 계층도 이
사실을 복원할 수 없고, 미들웨어에서 403 응답을 보고 기록하는 방식도 같은 이유로 권한 이름을
알 수 없다. 판단한 자리가 기록해야 한다.

### 병렬 헬퍼 대신 시그니처를 바꿨다

`require_permission_audited`를 따로 두면 "감사되는 거절"이 계약이 아니라 **관례**가 된다. 그것은
`#95` 1단계가 `project_id`에서 진단한 바로 그 실패 모양이다 — Project 범위 감사 지점 11곳 중
5곳만 값을 싣고 있었고, 가장 날카로운 자리는 인가를 판단하면서 그 판단의 기록에서 같은 값을
버리는 코드였다. 시그니처를 `async fn require_permission(&DashboardState, &AuthPrincipal,
PermissionKind)`로 바꾸면 **감사 없이 거절하는 코드가 컴파일되지 않는다.**

비용은 호출부 53곳이다(`handlers.rs` 48, `provisioning.rs` 4, `sse.rs` 1). 착수 전에 먼저 셌고,
그 측정이 결정을 쉽게 만들었다 — async 전파는 한 단계 깊이(`authorize_template_scope` 하나)뿐이고,
`State` 추출자가 없던 핸들러는 `list_tools_api` 하나였으며, `#[cfg(test)]` 안의 호출부는 0곳이라
테스트용 `DashboardState` 픽스처를 새로 만들 필요가 없었다. `#73` 행이 적어 둔 "29곳"은 그 시점의
실측이고 그 사이에 `#86`~`#93` 관리 화면이 들어왔다.

### IP는 핸들러가 아니라 principal에 실었다

`AuthPrincipal`에는 IP가 없었다. 호출부 53곳에 `ConnectInfo`·`HeaderMap` 추출자를 붙이는 대안은
값을 싣는 일을 다시 "저자가 기억했는가"로 만든다 — 위에서 시그니처로 없앤 그 문제를 다른 축에서
되살리는 셈이다.

대신 `require_session`을 봤더니 **이미 `extract_client_ip`를 부르고 있었다.** 다만 그 호출이
`if let Some(ref session_ip) = session.ip_address` 안에 있어서, 세션에 IP가 기록돼 있지 않으면
계산 자체가 수행되지 않았다. 이 계산을 principal 구성 **위로** 끌어올려 `AuthPrincipal.client_ip`에
싣고, 기존 세션 IP 대조는 그 값을 다시 쓰게 했다. 핸들러 시그니처 변경 0건, `AuthPrincipal` 생성
지점은 프로덕션 1곳·테스트 헬퍼 1곳뿐이다.

### 하네스가 프로덕션과 달랐다 — 그대로 뒀다면 새 단정이 아무것도 검증하지 않았다

`tests/dashboard_api.rs`는 서버를 `axum::serve(listener, app)`로 띄우고 있었다(7곳). 프로덕션
(`app.rs:438`)과 `tests/rate_limit.rs`는 `into_make_service_with_connect_info::<SocketAddr>()`를
쓴다. 그 차이 때문에 요청 extension에 `ConnectInfo`가 없었고, `require_session`이 IP를 확정할 수
없어 `client_ip`가 **항상 `None`**이 된다. 새로 넣은 `ip_address` 단정은 이 하네스에서 자동으로
통과했을 것이다 — 검증한 것처럼 보이면서 아무것도 검증하지 않는 상태다. 7곳을 프로덕션 모양으로
맞추고, 테스트에 "이 단정이 깨지면 하네스가 프로덕션에서 벗어난 것"이라는 주석을 남겼다.

### 방향은 log-only다

거절 기록은 실패해도 이미 결정된 403을 되돌리지 않는다. `#89`가 "감사 실패 시 Issue 생성 거절"을
요구하는 것과 충돌하지 않는다 — **판단의 방향이 다르다.** 권한을 *내주는* 쪽(`worker.llm_credential.export`
같은 발급·export)은 기록 실패가 곧 무증적 권한 부여이므로 fail-closed여야 하지만, *주지 않는* 쪽은
그런 위험이 없다. `fleet-api`의 `record_capability_denial`이 같은 근거로 log-only다.

### `project_id`는 항상 `None`이고, 그것은 단정이다

거절은 대상 엔티티를 적재하기 *전에* 일어난다. 53곳 중 52곳이 권한 검사 시점에 어떤 엔티티도
손에 쥐고 있지 않다. 1단계에서 `project_id`를 채운 자리들과 상황이 반대다 — 거기서는 값을 **이미
쥐고 있으면서 버리고** 있었다. 그래서 `require_permission`에 `project_id` 인자를 더하지 않았다.
테스트가 `event.project_id.is_none()`을 단정해 이 성질을 고정한다.

### 알려진 노출: 거절 1건 = 감사 행 1건

억제를 넣지 않았다. `/api`에는 로그인과 달리 rate limit이 없으므로 인증된 저권한 사용자가
`audit_log` 쓰기 볼륨을 정할 수 있다. 그럼에도 전건 기록을 고른 이유는 두 가지다. (1) 이 기록의
목적이 권한 열거 탐지인데 같은 (사용자, 권한) 쌍의 반복을 접으면 열거와 오조작을 가르는 신호인
*빈도*가 사라진다. (2) `fleet-api`의 `record_capability_denial`도 전건 기록이라, 여기만 접으면 두
표면의 카운트를 같은 기준으로 비교할 수 없다. 억제를 넣는다면 자리는 `require_permission` 안이고
필요한 상태는 `check_rate_limit`이 이미 쓰는 것과 같은 종류다 — 실제 남용이 관측되기 전에는
만들지 않는다.

### 게이트

7단계 전부 `exit=0`. `rustc 1.98.0`(CI와 일치), `RUSTFLAGS="-D warnings"`, fmt, clippy 두 피처
세트, 그리고 각 세트마다 `cargo build -p fleet-cli` 후 `DATABASE_URL` 주입
`cargo test --workspace --no-fail-fast -- --test-threads=1` — acp+mtls 74 스위트 1323건,
no-default 74 스위트 1319건, 실패 0.

새 테스트 2건이 양쪽에서 **실행됐음을 이름으로** 확인했다(`permission_denial_is_audited`,
`permitted_request_is_not_audited_as_denial`). 통합 스위트가 실제로 DB에 닿았는지도 소요 시간으로
읽었다 — `agents.rs` 49건 0.64s, `auth_integration.rs` 35건 0.73s, `audit_integration.rs` 9건
0.28s로 `0.00s`가 아니다. `0.00s`로 끝난 다섯 스위트는 `verify_env_example`·`lifecycle`·
`verify_examples`, 즉 DB를 쓰지 않는 파일 검사다.

첫 시도는 fmt에서 `exit=1`로 멈췄다. `&state,`를 끼워 넣은 치환이 12곳을 rustfmt 폭 밖으로
밀어냈기 때문이다. fmt를 게이트의 **첫 단계**로 둔 덕에 20분짜리 clippy 벽을 치르기 전에
드러났다 — CI도 같은 순서라 이 배치가 CI의 실패 지점을 그대로 재현한다.

**이 게이트가 증명하지 않는 것**: 새 테스트 2건은 `MemStore` 위에서 돌므로 Postgres 제약 아래의
`audit_log` 쓰기 경로는 시험하지 않았다. `ip_address`의 `127.0.0.1`은 루프백 리스너에서 온
값이라 `extract_client_ip`의 프록시 헤더 해석 경로는 이번에 한 번도 실행되지 않았다. 거절 폭주 시
쓰기 증폭도 추론했을 뿐 측정하지 않았다.

### 남은 것

MCP tool별 감사는 착수하지 못했다. `ToolContext`에 호출 principal이 없어서 "누가 거절당했는가"를
쓸 수 없고, 이것은 `#93`이 걸려 있는 문제와 같다(stdio 런처의 `FLEET_MCP_CAPABILITIES`가 호출
주체 없이 열려 있다). Dashboard mutation 감사, `request_id`·`policy_revision`, `GET /api/audit`의
범위 강제도 그대로 남는다 — 마지막 것은 승인된 Project 멤버십 모델이 없다는 `#58`의 사유와 같다.

## 관례는 골고루 낡지 않는다 — 새로 생긴 자리부터 낡는다 (#95 3단계)

착수 시점의 Dashboard는 non-GET route 31개 중 **20개만** 감사하고 있었다. 흥미로운 것은 개수가
아니라 **어느 11개가 빠졌는가**다: Issue 편집·댓글·연결·해제, Task 제출, SSH 키 생성·삭제, 호스트
프로비저닝, 비밀번호 재설정 요청, 인증 메일 재발송 — 전부 나중에 들어온 표면이다. 같은 저장소가
같은 진단을 세 번째 내놓았다. 1단계는 `project_id`를 싣는 감사 지점 11곳 중 5곳만 싣고 있었고,
2단계는 권한 거절을 아무 데서도 기록하지 않고 있었다. 세 번 모두 원인은 "코드가 없다"가 아니라
**감사가 계약이 아니라 관례였다**는 것이다. 그리고 관례는 균등하게 낡지 않는다. 처음 작성될 때
저자가 보고 있던 자리는 남고, 나중에 붙은 자리가 빠진다.

### 강제 지점을 어디에 둘 수 있는가가 단계마다 다르다

2단계에는 값싼 답이 있었다. `require_permission`의 시그니처를 바꾸면 "감사하지 않고 거절하는
코드"가 **컴파일되지 않는다**. 호출부 53곳을 고치는 대가를 치를 값어치가 있었던 이유가 그것이다.

mutation 감사에는 그런 시그니처가 없다. 기록은 store 호출이 성공한 *뒤* 성공 분기 안에서
일어나고, 그 분기를 통과했다는 사실을 붙잡는 타입이 없다. 타입으로 못 잠그면 다음 후보는
테스트다 — 그래서 `crates/fleet-dashboard/tests/audit_contract.rs`는 핸들러가 아니라 `app.rs`의
**라우터 원문**을 읽어 route 집합과 표를 맞댄다. 표에 없는 mutation route가 생겨도, 표에 있는데
라우터에서 사라져도 깨진다.

계약을 두 겹으로 나눈 것은 타협이 아니라 판정 능력의 경계다. 계약 테스트는 "분류됐는가"까지만
보고, "행이 실제로 남는가"는 `dashboard_api.rs`의 런타임 테스트 5건이 HTTP로 요청을 보내고
`list_audit_events`로 확인한다. 소스 스캔으로 후자까지 하려면 함수 본문을 해석해야 하는데, 이
저장소에는 그 해석이 곧바로 틀리는 자리가 **이미 두 곳** 있다: `provision_host_api`는 헬퍼
`run_provisioning` 안에서 기록하고, `delete_project_api`는 action을 `match`로 계산해 한 요청에
행을 둘 남길 수 있다. 예외를 두 개 안고 시작하는 테스트는 시간이 지날수록 틀린 말을 한다.

### 부정 테스트가 계약의 절반이다

런타임 테스트 5건 중 3건은 **행이 남지 않아야 한다**고 단정한다. 멱등 재연결은 2회 요청에 1행,
걸려 있지 않은 링크의 해제는 0행, 미등록 이메일의 비밀번호 재설정 요청도 0행이다.

긍정 테스트만 있으면 **무조건 기록하는 구현도 초록**이다. 그리고 그런 구현에서는 감사 행 수가
"몇 번 변경됐는가"가 아니라 "몇 번 요청됐는가"를 센다 — 감사 표를 읽는 사람이 정확히 반대로
읽게 되는 종류의 거짓말이다. `forgot_password`가 `if let Ok(Some(user))` 안에서만 기록하는 것은
FK 안전도 함께 준다: `actor_user_id`가 항상 실재하는 사용자이고, 공격자가 넣은 이메일 문자열은
감사 표에 아예 들어가지 않는다.

`detail`에 무엇을 넣지 않았는지도 단정으로 만들었다. `issue.update`는 바뀐 **필드 이름만** 싣고
값은 싣지 않으며, `issue.comment`는 `comment_id`만 싣는다. 테스트는 요청에 sentinel 문자열을 넣고
감사 이벤트의 `Debug` 출력 **어디에도** 그 문자열이 없음을 확인한다 — 키 이름을 세는 방식보다
강하다. 키를 하나 더 붙이는 실수까지 잡기 때문이다.

### 스캐너의 정의가 route 하나를 통째로 빠뜨리고 있었다

여기까지 쓰고 나서 발견한 것이 이번의 가장 값진 결과다. 스캐너의 정의는 "non-GET이면
mutation"인데, **상태를 바꾸면서 GET인 route가 실재한다**. 메일로 보낸 링크를 클릭해 도달하는
`GET /verify-email`은 토큰을 소비하고 `users.email_verified`를 세운다. 링크는 GET일 수밖에 없다 —
메일 클라이언트는 POST를 만들지 못한다.

그대로 뒀다면 이렇게 된다: `AUTH_EMAIL_VERIFIED`를 남기는 `audit::record` 호출을 지워도 두 계약
테스트가 **모두 초록**이다. 스캐너는 GET을 걸러내고, action 검증은 표를 순회하는데 그 route가
표에 없기 때문이다. 3단계가 닫으려던 결함이 정확히 한 곳에서, 그것도 "닫았다"는 문서와 함께
살아남을 뻔했다.

정의를 "본문이 상태를 바꾸는 route"로 넓히는 것은 답이 아니다. 소스 스캔은 그것을 판정할 수
없고, 판정하려 들면 위에서 말한 본문 해석의 함정으로 곧장 들어간다. 그래서 값싼 정의를 그대로
두고 **예외를 눈에 보이는 표로** 옮겼다 — `STATE_CHANGING_GET_ROUTES`가 항목 하나를 담고, 그
경로가 사라지거나 메서드가 바뀌면 깨진다. 이 표가 자동으로 채워지지 않는다는 것은 한계로 적었다.
지금 항목이 하나뿐인 근거는 감사 호출 38곳의 감싸는 함수를 전부 뽑아 GET 핸들러가
`verify_email_page` 하나임을 확인한 **실측**이지, 테스트의 보장이 아니다.

### 초록인 파서는 파서가 맞는다는 뜻이 아니다

개발 도중 스캐너가 `axum::routing::delete(...)` 표기를 놓쳐 31개 중 **29개만** 인식했다. 그때
테스트는 초록이었다. 표에 29개가 있었고 스캔도 29개를 찾았으니 일치했기 때문이다.

이것은 `agent.md` §4.3이 기록해 온 조용한 skip과 같은 계열이다 — 거기서는 바이너리가 없어서 14건이
`0.00s`에 "통과"했고, 여기서는 파서가 못 읽어서 2건이 계약 밖으로 사라졌다. 둘 다 **없는 것을 센
결과가 통과처럼 보인다**. 대응도 같다: 0을 성공으로 해석하지 않는다. 모든 `.route()`는 메서드
핸들러를 최소 하나 가지므로, 인식한 메서드가 0개면 "route가 없다"가 아니라 "파서가 깨졌다"이고,
그때 시끄럽게 실패하도록 했다.

### 게이트

게이트 7단계 전부 `exit=0`이다. `rustc 1.98.0`(CI와 일치), `RUSTFLAGS="-D warnings"`,
`cargo fmt --all -- --check`, clippy 두 피처 세트(`--all-targets -- -D warnings`), 그리고 각
세트마다 `cargo build -p fleet-cli` 후 `DATABASE_URL`을 주입한
`cargo test --workspace --no-fail-fast -- --test-threads=1` — acp+mtls 75 스위트 1331건,
no-default 75 스위트 1327건, 실패 0.

새 테스트 8건(계약 3건, 런타임 5건)이 양쪽 세트에서 실제로 실행됐음을 이름으로 확인했다
(`audit_contract` 3건 0.02s, `dashboard_api` 127건 0.49s). 통합 스위트가 DB에 닿았는지는
소요 시간으로 읽었다 — `agents.rs` 49건 0.61s, `auth_integration.rs` 35건 0.79s,
`audit_integration.rs` 9건 0.29s로 `0.00s`가 아니다.

**게이트를 두 번 돌렸고, 그 이유가 이 항목에서 가장 실용적인 부분이다.** 1차가 clippy를 돌고
있을 때 `STATE_CHANGING_GET_ROUTES`를 추가했다. 그 결과 1차 로그는 fmt·clippy가 편집 *전*
트리를, 뒤이은 테스트 단계가 편집 *후* 트리를 가리키는 **혼합 판정**이 됐다. 두 실행의 통과
개수가 똑같은 것(1331/1327)이 바로 그 증거다 — 1차의 테스트 단계는 이미 8번째 테스트를
컴파일하고 있었다.

`agent.md` §4.3은 "게이트를 돌린 사실과 그 게이트가 지금 트리를 가리키는 것은 다르다"를
이미 적어 두었지만, 거기 적힌 형태는 **게이트가 끝난 뒤에** 편집한 경우다. 이번은 다르다:
게이트는 아직 돌고 있었고, 로그 파일 하나가 서로 다른 두 트리에 대한 판정을 **한 줄씩
나눠서** 담았다. 단계별 `exit=0`이 일곱 줄 나란히 있으면 그것을 하나의 판정으로 읽게 되고,
그 읽기가 틀린다. 대응은 단순하다 — 게이트가 도는 중에는 편집하지 않고, 편집했으면 전부
다시 돌린다. 캐시가 살아 있어 2차의 clippy 두 세트는 몇 분에 끝났다.

**이 게이트가 증명하지 않는 것**: 런타임 테스트 5건은 `MemStore` 위에서 돌므로 Postgres 제약
아래의 `audit_log` 쓰기 경로를 시험하지 않는다. `ip_address`의 `127.0.0.1`은 루프백 리스너에서 온
값이라 `extract_client_ip`의 프록시 헤더 해석 경로는 이번에도 실행되지 않았다. `host.provision`과
SSH 키 두 handler는 런타임 테스트가 없고(실제 SSH 연결과 키 픽스처가 필요하다) 감사 호출이 있다는
사실만 코드 읽기로 확인했다. 계약 테스트는 route 분류만 보므로, 분류된 route의 어떤 실행 경로가
기록을 건너뛰더라도 이 게이트는 침묵한다.

### 남은 것

MCP tool별 감사는 여전히 `ToolContext`에 호출 principal이 없어 막혀 있다 — `#93`이 걸려 있는 것과
같은 문제다. `request_id`와 `policy_revision`은 생산자가 없다는 사유가 그대로다. `GET /api/audit`의
범위 강제도 남는다: 승인된 Project 멤버십 모델이 선행이고, 그것은 `#58`이 막혀 있는 이유와 같다.
