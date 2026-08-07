# LLM Wiki — 변경 로그 (Log)

> **Append-only.** 새 항목은 파일 맨 아래에 추가한다(시간순). 과거 항목은 수정하지 않는다(오탈자 수정 제외).
> 각 항목은 `ingest`(신규 결정/소스 반영) / `query`(질문에 대한 답을 새 페이지로 파일링) / `lint`(모순·오래된 정보·고아 페이지 점검 및 수정) 중 하나로 분류한다.
> 형식: `## YYYY-MM-DD — <type> — <한 줄 제목>`

---

## 2026-08-06 — ingest — liteLLM 게이트웨이 채택 결정 및 스펙 문서 최초 작성

- `multi_provider_llm_proxy_analysis.md`, `litellm_integration_plan.md`, `README.md`를 최초 작성.
- 소스: 로드맵 #34(liteLLM 중앙 게이트웨이 통합 및 연동) 요구사항.
- 결과: 멀티 LLM 공급자 통합, Spend Control, Fallback 요구사항을 근거로 liteLLM을 공식 게이트웨이로 확정.
- 커밋: `docs: build central design wiki index and import design artifacts`,
  `docs: integrate liteLLM routing plans and build LLM Wiki index`,
  `docs: refine LLM wiki docs to remove redundant Redis dependency and minor APIs`.

## 2026-08-06 — lint — `single_server_deployment_plan.md`와의 모순 발견 및 해소

- **발견**: `docs/single_server_deployment_plan.md`가 "무겁고 복잡한 liteLLM 대신 One API 채택"이라고 본 위키의 결론과 정반대로 기술하고 있었음. `docs/roadmap.md` #34 및 본 위키와 직접 충돌.
- **원인**: `single_server_deployment_plan.md`가 liteLLM 확정 결정(ingest 커밋들) **이전**에 작성된 채 갱신되지 않은 stale 문서였음.
- **조치**: `single_server_deployment_plan.md`의 아키텍처 다이어그램·컴포넌트 설명·Docker Compose 예시·Caddy 라우팅·Scale-Out 문단을 liteLLM 기준(포트 4000, `ghcr.io/berriai/litellm`, `litellm` 논리 DB, `FLEET_LLM_GATEWAY_URL`)으로 전면 수정.
- `docs/roadmap.md` #34에 정합성 수정 이력 메모 추가.

## 2026-08-06 — lint — 정본/사본 관계 명문화 (wiki 구조화 1차)

- **발견**: `README.md`가 `multi_provider_llm_proxy_analysis.md`에 "liteLLM vs One API 비교표가 있다"고 소개했지만, 실제 문서에는 비교표가 없었음(설명-실체 불일치 — 고아 참조에 준하는 결함).
- **조치**:
  - `multi_provider_llm_proxy_analysis.md`에 liteLLM vs One API vs 자체 구현(Rust-native) 비교표(§2) 신설, 문서 자신을 "게이트웨이 선택의 정본(canonical source)"으로 명시.
  - `litellm_integration_plan.md` 상단에 "인프라 스펙의 정본" 배너 추가.
  - `single_server_deployment_plan.md`의 Docker Compose 예시를 "정본의 사본"으로 표시하고, 수정 순서(정본을 먼저 고칠 것)를 명문화.
  - `README.md`에 정본 표시 및 인용처 역참조 추가.
- **계기**: Karpathy의 "LLM Wiki" 패턴(<https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f>) 검토 — 부기(bookkeeping)를 사람이 아니라 LLM이 상시 수행해야 한다는 문제의식 확인.

## 2026-08-07 — lint — Karpathy LLM Wiki 패턴 정합화 (`index.md`/`log.md` 도입)

- **발견**: gist 원문과 대조한 결과, 패턴의 핵심 부기 파일 2종 — `index.md`(콘텐츠 지향 목록)와 `log.md`(append-only 시간순 기록) — 가 누락되어 있었음. 8/6의 lint 조치 두 건이 여러 문서에 흩어져 기록되고, 구조화된 위치가 없었음.
- **조치**:
  - 본 `log.md` 신설 — 과거 이력(8/6 ingest 1건, lint 2건)을 소급 기록.
  - `index.md` 신설 — 정본/사본/메타데이터를 갖춘 페이지 카탈로그 + 역참조 표.
  - `README.md`를 페이지 나열 위주에서 **스키마(운영 규칙: 정본/사본 구분, ingest/query/lint 워크플로우, 필수 부기 파일)** 문서로 재편, 상세 목록은 `index.md`로 이관해 중복 방지.

## 2026-08-07 — ingest — list_workers 라벨 필터 및 페이지네이션 결함 해결 및 CI 검증 완료

- **조치**:
  - `crates/fleet-api/src/handlers.rs`와 `crates/fleet-dashboard/src/handlers.rs`의 쿼리스트링 라벨 필터 접두사 `label_` 누락 및 파싱 불일치 버그를 전면 수정.
  - `limit/offset` 페이지네이션 매개변수가 Postgres Store 계층까지 연동되지 않던 쿼리 누락 결함을 정비하고 E2E 통합 테스트 코드들을 전량 보강.
  - 전담 서브에이전트 `grok_actions_tracker` 를 자동으로 탑재하여 최신 커밋(`7e17558`)의 GitHub Actions 원격 빌드가 100% 그린(success)으로 패스함을 추적 입증하고 `ci_monitor_report.md` 를 작성함.
