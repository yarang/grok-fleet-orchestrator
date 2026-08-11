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

## 2026-08-07 — lint — 커스텀 스킬 외부 전역 플러그인 패키징 및 워크스페이스 정리

- **조치**:
  - 기존에 프로젝트 워크스페이스 내부(`.agents/skills/`)에 위치해 있던 `github-actions-monitor` 와 `multi-agent-spec-designer` 스킬 소스 일체를 플랫폼 전역 플러그인 경로(`~/.gemini/config/plugins/grok-fleet-custom-plugins`)로 완전히 추출 및 독립 패키징화 완료.
  - 이를 통해 모든 프로젝트 워크스페이스에서 두 스킬이 범용(universal) 로드되도록 아키텍처 격리.
  - 프로젝트 내부의 중복 로컬 스킬 폴더들은 깔끔하게 삭제하여 Git 리포지토리의 경량성 및 무결성 확보.

## 2026-08-08 — ingest — 디스패치 지연 시간(dispatch_latency) 메트릭 수집 및 DB 스키마 추가

- **조치**:
  - `tasks` 테이블에 `dispatched_at` TIMESTAMP 컬럼을 추가하는 신규 DB 마이그레이션(`012_task_dispatch_latency.sql`)을 작성하여 데이터 스키마 연동.
  - `Task` 모델 및 `PgStore` / `InMemoryStore` 의 상태 갱신 코드(`update_task_status`)에 `Dispatched` 상태 전이 시 `dispatched_at` 을 갱신하는 로직 추가.
  - `fleet_task_dispatch_latency_seconds` 히스토그램 메트릭을 `crates/fleet-api/src/metrics.rs` 에 구현하고 `/metrics` 엔드포인트 통합 테스트 보강 완료.

## 2026-08-11 — query — Groq/OpenRouter 무료 티어 도입 검증 및 설계

- **소스**: 사용자가 Groq/OpenRouter 무료 티어 요약을 제공, 검증·문서화·Fleet 통합 설계·UI
  연동 고려·사용자 결정 체크리스트 작성을 요청.
- **조치**: 공식 문서(Groq `console.groq.com/docs/rate-limits`, OpenRouter
  `openrouter.ai/docs/api-reference/limits`) 및 웹 검색으로 대조 검증 —
  OpenRouter는 원본과 완전 일치, Groq는 모델 라인업이 일부 최신화 필요(원본의 "Qwen 3 32B"가
  현재 `qwen/qwen3.6-27b`로 교체됨 등)했음을 확인. `free_tier_providers_analysis.md` 신설 —
  기존 워커별 모델 분리 + 라벨 라우팅 아키텍처(코드 변경 불필요) 재사용 설계, CircuitBreaker가
  "워커 건강"과 "계정 쿼터 소진"을 구분하지 못하는 아키텍처 공백 식별(방안 A 최소 대응 / 방안
  B 능동적 쿼터 추적 옵션 제시), 모델 선택 UI·토큰 통계 UI(기존 대기 작업)와의 연결점 정리.
- **문서 위치 결정**: 새 디렉토리를 만들지 않고 `docs/llm-wiki/`에 편입 — 이미 이 위키가
  "멀티 LLM 공급자 게이트웨이" 도메인을 다루고 있어 분절(fragmentation)을 피함.

## 2026-08-11 — ingest — Groq/OpenRouter 실키 발급 및 실사용 검증, Groq 결론 변경

- 사용자가 Groq/OpenRouter API 키를 실제로 발급해 전달, 로컬 `~/.grok/config.toml`에
  `[model.groq-free]`/`[model.openrouter-free]`로 등록 후 `grok` CLI로 end-to-end 검증.
- **Groq**: raw API 호출(curl)은 200 OK로 키 자체는 유효했으나, `grok` CLI 실사용(프로젝트
  컨텍스트 없는 `/tmp`에서도 동일)은 매번 413 Payload Too Large. grok-build 자체 시스템
  프롬프트+도구 스키마만으로 약 19,000토큰을 소비하는데 Groq 무료 티어의 최대 TPM(12,000)도
  이에 못 미쳐 어떤 무료 모델을 선택해도 단일 턴이 성립하지 않음을 확인. §1.2에서 "선택
  가능한 옵션"으로 소개했던 카드 등록형 Developer 티어(무과금, 10배 한도)가 grok-build용으로는
  **사실상 필수 조건**으로 결론이 바뀜 — `free_tier_providers_analysis.md` §1.3, §5 갱신.
- **OpenRouter**: `openai/gpt-oss-20b:free`로 실제 응답 정상 수신, 즉시 사용 가능 확인.
  TPM 상한이 없어(RPM/RPD만 제한) grok-build의 큰 시스템 프롬프트도 문제없음 — 단 하루
  50회 요청 한도는 여전히 유효.
- 두 키 모두 아직 Fleet 워커(`docs/credentials/`)에는 등록하지 않음 — 로컬 개인 사용 확인
  단계이며, 워커 연동 여부는 체크리스트 항목으로 남김.

## 2026-08-11 — query — Groq 프롬프트 크기 축소 방법 조사 및 실측

- **질문**: "Groq을 사용할 grok의 자체 프롬프트를 줄이는 방법은 없는가?"
- **조사**: `grok --help`/`grok inspect`/`~/.grok/docs/user-guide/`를 훑어 토큰 절감 수단을
  찾음 — `grok inspect`에서 `~/.claude/skills/`(Claude Code 호환 스캔) 경유로 74개 스킬(대부분
  MoAI 스위트)이 자동 로드되고 있음을 발견, 이게 시스템 프롬프트 비대화의 주요 원인 중
  하나로 확인. `GROK_CLAUDE_SKILLS_ENABLED=false` 환경변수와 읽기 전용 `--agent explore`를
  실제 Groq 429 에러의 "Requested: N" 수치로 직접 측정하며 조합 실험.
- **결과**: 기본값 ~19,000~22,000토큰 → `--agent explore` + 스킬 비활성화 조합으로 **7,784**
  까지 축소, 8K/12K TPM 무료 모델(`openai/gpt-oss-20b`, `llama-3.3-70b-versatile`)에서 실제
  응답 수신까지 확인. 단, **편집 가능한 에이전트는 트리밍을 최대로 적용해도(10,208~14,309)
  모든 무료 TPM 한도를 초과** — 이 경로는 읽기 전용 작업에만 유효함을 확인.
  `free_tier_providers_analysis.md` §1.4에 실측 표와 함께 반영.
- `~/.grok/config.toml`에 TPM이 다른 Groq 모델 2종(`groq-free-oss20b` 8K, `groq-free-70b`
  12K)을 추가 등록해 실험/향후 사용에 대비.

## 2026-08-11 — ingest — Groq strict-schema 호환 훅 도입 (진짜 블로커는 TPM이 아니었음)

- **발단**: "편집 가능한 에이전트는 무료 TPM에 못 들어간다"는 직전 결론(§1.4)을 더 밀어붙여,
  커스텀 최소 에이전트 정의(`tools:` 화이트리스트 + 최소 `GROK_HOME`)로 프롬프트를
  **19,000 → 3,216토큰**까지 축소. 가장 작은 무료 모델(`llama-3.1-8b-instant`, 6K TPM)에서도
  실제 파일 편집이 성공 — **TPM 문제는 해결됨**(Developer 티어 카드 등록 불필요).
- **발견(결론 변경)**: 그 과정에서 TPM과 무관한 진짜 블로커가 드러남. Groq 은 요청 본문을
  엄격 검증하는데 `grok` CLI 가 assistant 메시지에 `model_id`/`model_fingerprint` 를 붙여
  보내므로 **툴 호출이 한 번이라도 일어난 턴은 두 번째 요청부터 400 으로 전부 실패**한다
  (`property 'model_id' is unsupported`). 무료 모델 3종·4개 독립 세션에서 100% 재현,
  턴 내부·턴 간(`--resume`) 모두 발생. 동일 에이전트를 OpenRouter 로 돌리면 정상 동작해
  **Groq 고유 문제**로 확정. 즉 직전의 "읽기 전용은 가능" 결론도 성립하지 않는다 —
  코드를 읽으려면 툴을 호출해야 하므로 읽기 전용 Q&A 역시 실패한다.
- **liteLLM 검증**: 게이트웨이를 경유해도 동일하게 400 — liteLLM 은 이 필드를 걸러주지
  않는다. 스펙 문서가 이미 켜두라고 한 `drop_params: true` 로도 해결되지 않음(이 옵션은
  top-level 파라미터 전용). 따라서 pre-call 훅이 별도로 필요함을 실측으로 확정.
- **조치**: `examples/groq-compat/` 신설 — OpenAI Chat Completions 스펙 화이트리스트 기반
  정규화 로직(`sanitizer.py`, 단위 테스트 10건)을 두고, 이를 두 경로가 공유한다:
  - `litellm_hook.py` — **정본 경로**. liteLLM `async_pre_call_hook` 어댑터.
    `litellm-config.yaml` 의 `callbacks:` 및 `docker-compose.yml` 볼륨 마운트 반영.
  - `shim.py` — docker 없는 로컬 개발용 standalone 프록시(표준 라이브러리만 사용).
  - 게이트웨이 선택 정본(`multi_provider_llm_proxy_analysis.md`)의 liteLLM 채택 결정은
    **변경하지 않는다** — 본 훅은 게이트웨이를 대체하지 않는 정규화 계층이며,
    `litellm_integration_plan.md` §3.3(인프라 스펙 정본)에 편입했다.
- **검증**: ① 훅을 liteLLM 실제 진입점으로 호출 후 `litellm.completion` → 200 성공,
  ② shim 경유 `grok` CLI 로 read → edit → confirm 멀티스텝 루프 정상 완주(파일 실제 변경),
  ③ `test_sanitizer.py` 10건 통과.
- **남는 제약**: 프로토콜 문제만 해결됐고 TPM 은 그대로다. 멀티스텝 턴은 모델 호출 N회가
  같은 1분에 누적되므로 8K TPM 모델 기준 분당 약 2회가 한계 — 동작하지만 느리다.
  `free_tier_providers_analysis.md` §1.3/§1.4/§5 는 위 결론 변경을 아직 반영하지 않았으므로
  후속 갱신이 필요하다.
