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
