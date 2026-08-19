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
  - Gemini/Antigravity 에이전트 지침 진입점 [`GEMINI.md`](file:///Users/yarang/working/tools/grok-fleet-orchestrator/GEMINI.md) 신설 및 `docs/index.md` 색인 등록.

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

## 2026-08-19 — Task dispatch credential precondition 구현 (#71)

- 유형: `implementation`
- `crates/fleet-core/src/task.rs`의 `FailureKind`에 `CredentialMissing` variant를 추가했다(`#[serde(rename_all = "snake_case")]` → `"credential_missing"`). 이 enum을 `match`하는 exhaustive 지점이 코드베이스 어디에도 없어(`cargo check --all-features`/`--no-default-features` 둘 다로 확인) 다른 크레이트 수정은 필요 없었다.
- `crates/fleet-scheduler/src/selector.rs`의 `WorkerSelector::select`에 모델 라벨 필터 직후 credential 매칭 필터를 추가했다: `task.model`이 `Some(model)`이면 `Store::get_worker_credential(worker.name, model)`이 `Some`을 반환하는 worker만 후보로 남기고, 전부 제외되면 신설한 `SelectionError::NoWorkerForCredential(model)`을 반환한다. `#71` 등록 당시 설계 초안은 `Task.resolved_model` 기준을 제안했지만, 구현 중 `dispatcher.rs`를 다시 대조해 두 가지 사실을 확인하고 `Task.model` 기준으로 조정했다: (1) 워커에 실제로 전달되는 `DispatchRequest.model`은 `task.model.clone()`이지 `resolved_model`이 아니다 — "실행에 실제로 쓰이는" 필드는 `model`이다. (2) `HeuristicTaskRouter::resolve_routing`은 `Dispatcher::submit`의 0단계에서 항상 호출되며, 사용자가 `model`을 지정하지 않은 task에도 프로파일 휴리스틱으로 `resolved_model`을 채운다 — 이를 기준으로 삼으면 `#71`의 문제 배경(사용자가 명시적으로 model을 지정한 경우)을 크게 벗어나 사실상 모든 fleet가 전 모델에 credential을 프로비저닝해야 하게 되고, 기존 dispatch 테스트 스위트 대부분(worker에 credential을 등록하지 않은 채 `submit()` 성공을 기대하는 테스트들)이 깨진다. `SelectionError` 매칭이 필요한 두 지점(`Dispatcher::dispatch_existing`의 즉시 실패 경로, `Reconciler::reconcile_once`의 재시도 소진 dead-letter 직전)에서 `crate::selector::SelectionError`를 import해 `NoWorkerForCredential`이면 `FailureKind::CredentialMissing`, 그 외에는 기존과 동일하게 `FailureKind::WorkerUnavailable`로 매핑했다. `Reconciler`의 dead-letter 분기는 재시도 소진 판정 자체가 실제 dispatch 시도 없이 `retry_count` 비교만으로 이뤄지므로, 원인 분류를 위해 `self.state.selector.select(&task)`를 부작용 없는 순수 조회로 한 번 더 호출한다.
- `crates/fleet-scheduler/src/selector.rs`의 테스트용 `MockStore`에 `credentials: HashSet<(worker_name, model_id)>` fixture와 `with_credential` 빌더를 추가하고(기존에는 `get_worker_credential`이 `unimplemented!()`였다), 이미 `task.model`을 지정하던 기존 테스트 `select_model_routes_to_matching_worker`/`select_model_and_required_labels_compose`에 해당 worker의 credential fixture를 채워 회귀를 피했다.
- `DATABASE_URL`을 주입하지 않아 `PgStore`의 `get_worker_credential` 실구현은 이번 작업으로 변경하지 않았다(기존 구현을 그대로 재사용) — `MemStore`(테스트·기본 배포)만 실제로 검증했다.

## 2026-08-19 — Task dispatch credential precondition 검증 (#71)

- 유형: `verification`
- 신규 테스트: `crates/fleet-scheduler/src/selector.rs`의 `select_credential_required_and_present_routes_normally`(a: credential 보유 worker로 정상 dispatch), `select_credential_missing_on_all_candidates_errors`(credential 없는 worker만 있을 때 `NoWorkerForCredential`), `select_credential_partial_provisioning_routes_to_credentialed_worker`(c: 일부만 provisioned된 fleet에서 credential 보유 worker로만 라우팅), `select_no_model_skips_credential_check`(d: model 미지정 task는 credential 무관 정상 dispatch); `crates/fleet-scheduler/src/dispatcher.rs`의 `submit_marks_credential_missing_when_worker_lacks_credential`(재시도 비활성 시 즉시 `FailureKind::CredentialMissing`으로 Failed), `submit_selects_worker_when_credential_present`; `crates/fleet-scheduler/src/reconcile.rs`의 `stale_pending_task_dead_letters_as_credential_missing_when_no_worker_has_credential`(b: 재시도 소진 뒤 dead-letter가 `FailureKind::CredentialMissing`으로 분류됨); `crates/fleet-core/src/task.rs`의 `failure_kind_credential_missing_snake_case`.
- `cargo check --workspace --all-features`, `cargo check --workspace --no-default-features`, `cargo clippy --all-targets --all-features`(이번에 건드린 파일 기준 경고 0 — 도입 당시 `selector.rs` 모듈 문서 주석의 `clippy::doc_lazy_continuation` 경고 1건을 리스트 번호 재정렬로 해소했다), `cargo test --workspace`(전부 통과, vendor doctest 포함), `git diff --check`를 통과했다.

## 2026-08-19 — 로드맵 단계 0 잔여 게이트 마무리 (#57·#58·#59)

- 유형: `implementation` + `verification`
- `#57`: `crates/fleet-api/tests/bootstrap_tokens.rs`에 발급→`token_id` 노출→그 식별자로 회수까지의 API/CLI 라운드트립 E2E를 추가했다. `crates/fleet-store/tests/bootstrap_token_migration.rs`는 016까지만 적용한 옛 스키마(`token TEXT PRIMARY KEY`)에 plaintext 토큰 행을 직접 심고 017/018을 적용한 뒤, 저장 값이 digest로 바뀌었음에도 원래 원문으로 `consume_bootstrap_token` 인증이 그대로 성공함을 검증한다(`legacy_plaintext_token_still_authenticates_after_digest_migration`).
- `#58`: `crates/fleet-api/src/cloudflare.rs`의 `cloudflare_access_middleware`가 CF Access JWT를 서명까지 검증해 `VerifiedUser`를 요청에 넣어도, `auth_middleware`가 CF-only 배포 분기에서 그 값을 전혀 읽지 않고 `AuthorizationContext`도 만들지 않아 `authorize_http_endpoint`(capability 검사) 자체가 통째로 스킵되던 것을 연결했다 — 이제 검증된 이메일이 `principal_id`로 들어가고 capability 검사를 반드시 거친다. 매핑되지 않은 CF principal은 하위 호환을 위해 잠정적으로 전체 capability를 받는다(least privilege 아님, 이메일별 capability manifest는 후속 작업). Project scope/confused-deputy 검증은 `#48`(Project 기능) 부재로 의도적으로 미착수 상태로 남겼다.
- `#59`: `crates/fleet-api/tests/bootstrap_token_dump.rs`가 신선한 PostgreSQL에 전체 마이그레이션을 적용하고, bootstrap token 발급과 join으로 얻은 worker operational token(`fwo_...`) 모두 `pg_dump` 결과(schema+data)에 원문이 등장하지 않음을 검증한다. digest·행 존재를 함께 단언해 빈 덤프로 인한 거짓 통과를 막는다.
- 이전 시도가 하네스 프로세스 정지(600초 무진행)로 중단됐으나, 이미 만들어진 코드(3개 커밋)와 미완성 상태로 남아 있던 `bootstrap_token_dump.rs`(완성된 상태였음)를 이어받아 검증만 완료하고 마지막 조각을 커밋했다.
- `cargo check --workspace`(`--all-features`/`--no-default-features`), `cargo clippy --all-targets --all-features`(이번에 건드린 파일 기준 경고 0), `cargo test --workspace`(전부 통과), `DATABASE_URL=postgres://$(whoami)@localhost/fleet_test`로 `bootstrap_token_dump`/`bootstrap_token_migration`/`enroll_worker`/`worker_credential_rotation`/`integration`/`auth_integration`/`audit_integration`과 fleet-api 전체 스위트 재실행(전부 통과), `git diff --check`를 통과했다.
- 로드맵 "현재 구현 순서" 표(단계 0: `#58`, `#59`)가 이걸로 완료·부분 구현으로 닫혔다 — 단계 1(`#60`, 이미 완료)로의 순차 전환 조건을 충족한다.

## 2026-08-20 — Worker LLM credential 인가 우회 차단과 감사 기록 (#66)

- 유형: `implementation`
- 발견: `crates/fleet-api/src/app.rs`의 `required_capability` 행렬이 LLM credential 하위 자원을 하나도 매핑하지 않아 `authorize_http_endpoint`가 `Ok(())`로 통과시키고 있었다. 결과적으로 **인증만 성공하면**(capability가 빈 bearer principal, CF Access 세션, 심지어 `#60`에서 만든 워커 자신의 `fwo_` operational 토큰으로도) `GET /v1/workers/{name}/credentials/{model}/export`로 **아무 워커의 LLM 프로바이더 API 키든 평문으로** 가져갈 수 있었고, 핸들러는 `tracing::debug!`만 남겨 누가 무엇을 가져갔는지 사후에 확인할 방법이 없었다. `GET .../credentials`(목록)와 `PUT .../credentials`(저장·덮어쓰기)도 같은 이유로 무방비였다. `DELETE .../credentials/{model}`은 매핑이 없던 게 아니라 더 넓은 `(&Method::DELETE, path) if path.starts_with("/workers/")` 항목에 흡수돼 `worker:delete`만 요구했는데, 워커 operational 토큰이 바로 그 capability를 갖고 있어(`app.rs`의 `WorkerRegister`+`WorkerDelete` 부여) 워커가 다른 워커의 credential을 지울 수 있었다. `#58`(CF Access 경로), `#60`에 이어 "route를 추가하면서 capability 행렬에 등록하지 않는" 동일 결함의 세 번째 사례다.
- `crates/fleet-core/src/auth.rs`의 `PermissionKind`에 `worker:llm_credential:read`/`:export`/`:manage`를 신설했다. 기존 `worker:credential:manage`는 `#60`의 operational identity(워커가 자신을 인증하는 `fwo_` 토큰) 전용으로 그대로 두고, 이름과 doc comment로 두 비밀의 차이를 명시했다 — 하나의 capability로 뭉치면 워커 등록 권한이 곧 API 키 열람 권한이 된다. `read`/`export`를 나눈 것은 목록 조회가 `api_key`를 반환하지 않기 때문이고, `export`를 `manage`에서 분리한 것은 프로비저너 토큰(장기 보관·프로비저닝 호스트 상주)이 credential을 덮어쓰거나 지울 수 없게 하기 위해서다.
- `required_capability`에 `llm_credential_route()` 분류기를 추가해 GET 목록=`read`, GET export=`export`, PUT/DELETE=`manage`로 매핑하고, 이 항목들을 기존 `worker:delete` 항목보다 **앞에** 배치했다(뒤에 두면 DELETE가 다시 흡수된다). 분류기는 `strip_prefix("credentials")` 기반이라 단수형 operational credential 경로(`/credential`, `/credential/rotate`)와 문자열 수준에서 겹치지 않으며, 알 수 없는 `/credentials…` 변형은 capability를 요구하는 쪽(fail-closed)으로 분류한다.
- `crates/fleet-api/src/handlers.rs`의 export/put/delete 핸들러가 `AuthorizationContext`를 받아 `Store::record_audit_event`로 `worker.llm_credential.export`/`.put`/`.delete`를 기록한다(`crates/fleet-core/src/audit.rs`에 action 상수 신설). 기록 실패 처리는 의도적으로 비대칭이다: export는 응답 **전에** 기록하고 실패하면 500으로 거부하며 평문을 반환하지 않는다(키가 유출됐을 때 회수 범위를 정하는 유일한 근거이므로 근거를 남기지 못하면 열람을 허용하지 않는다), put/delete는 변경이 이미 커밋된 뒤이므로 기록 실패를 `tracing::error!`로만 경보하고 성공을 반환한다(500을 내면 "변경되지 않았다"는 거짓 신호가 된다). 감사 `detail`에는 worker/model 식별자와 인증 방식만 넣고 비밀 값은 넣지 않는다.
- `MemStore`/`PgStore` 모두 `record_audit_event`를 이미 구현하고 있어 store 변경은 없었다. `crates/fleet-store/src/rbac.rs`의 `permission_description`은 exhaustive match라 신규 capability 설명 세 줄을 추가했다(`seed_rbac_if_empty`가 `PermissionKind::all()`에서 DB로 자동 동기화하므로 마이그레이션은 불필요).
- `crates/fleet-api/src/openapi.yaml`의 네 엔드포인트 설명을 갱신했다 — export 항목에 남아 있던 "there is no additional elevated-permission check"는 이제 사실이 아니다.
- **운영 영향(하위 호환 깨짐, 의도적)**: `fleet-provisioner`의 `PushCredentials` 스텝과 `fleet-cli`의 credential 명령은 계속 이 엔드포인트를 사용하므로, 배포 manifest의 bearer 토큰에 새 capability(프로비저너는 `worker:llm_credential:read`+`:export`, credential 등록용 토큰은 `:manage`)를 추가하지 않으면 403으로 실패한다. 엔드포인트를 삭제하지 않고 게이트만 추가한 이유는 프로비저닝 경로가 이 API에 정상적으로 의존하고 있기 때문이다.

## 2026-08-20 — Worker LLM credential 인가·감사 검증 (#66)

- 유형: `verification`
- 신규 통합 테스트 `crates/fleet-api/tests/worker_llm_credential_authz.rs`: capability 없는 인증 principal의 export가 403이고 응답 본문에 키가 실리지 않으며 감사 이벤트도 남지 않음, `worker:llm_credential:export` 보유 principal은 200과 평문 키를 받고 감사 이벤트가 정확히 1건(행위자 라벨·target 확인, detail에 비밀 값 없음) 기록됨, `manage`만 가진 principal은 export 403, 워커 자신의 operational 토큰(`fwo_`)으로는 **자기 자신의** LLM credential export도 403, 목록 조회는 `read` 없이는 403, DELETE는 `worker:delete`나 워커 토큰으로는 403이고 `manage`로만 200이며 감사 이벤트가 남음, PUT은 `read`/`export`/무권한 모두 403이고 `manage`로만 200.
- 신규 단위 테스트: `crates/fleet-api/src/app.rs`의 `capability_matrix_covers_llm_credential_routes`(네 route의 요구 capability 고정, mount 지점 유무 무관), `llm_credential_routes_do_not_collide_with_operational_credential`(단수형 `/credential`은 LLM 경로로 분류되지 않고, 복수형 DELETE가 `worker:delete`로 흡수되지 않음), `llm_credential_export_is_not_covered_by_manage`; `crates/fleet-core/src/auth.rs`의 `llm_credential_permissions_are_distinct_from_operational_credential`(capability 이름 중복 없음, Operator/Viewer 기본 역할에 export·manage 미부여, Admin은 보유).
- `cargo check --workspace --all-features`, `cargo check --workspace --no-default-features`, `cargo clippy --all-targets --all-features`(이번에 건드린 파일 기준 경고 0), `cargo test --workspace`(전부 통과), `git diff --check`를 통과했다. `DATABASE_URL`은 주입하지 않았다 — 이번 변경은 `PgStore` 쿼리를 건드리지 않고 `MemStore` 기준으로만 검증했다.
