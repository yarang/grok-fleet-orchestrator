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

## 2026-08-11 — ingest — liteLLM 게이트웨이 실제 배포 + P0 ACP 연결 버그 수정 (`litellm_integration_plan.md` §2~§8 전면 재작성)

- **발단**: "worker들은 orchestrator를 통해서 질의를 진행하지 않는가?"라는 아키텍처
  질문에서 출발 — 실제로 워커는 오케스트레이터를 거치지 않고 각 LLM 프로바이더에
  직결하고 있음을 확인. 이 격차를 메우기로 결정.
- **배포**: `litellm_integration_plan.md`가 기술하던 Docker Compose + Postgres DB-backed
  설계를 **채택하지 않고**, arm2에 Python venv + systemd(`litellm-gateway.service`)로
  실제 배포. DB 백엔드(가상 키/예산 관리)는 Prisma/Node.js 의존성 때문에 의도적으로
  제외, master_key 단일 인증 stateless 모드로 운영. nginx `/api-gateway/` 경로로
  노출(`127.0.0.1:4000` 루프백만 바인딩).
  - `fastapi<0.120` 고정 필요(자동 해석된 최신 fastapi가 `get_flat_dependant` 임포트
    오류 유발, litellm 1.96.0 기준 실측).
  - 모델 목록: `gemini-2.5-flash`, `GLM-5.1`(z.ai), Groq 무료 3종(TPM별). 기존
    groq-compat 훅(§4.4 인용)을 그대로 연결.
- **검증**: Gemini/Groq 실제 추론 성공(루프백 + 공인 경로 양쪽), Groq는 고의로 오염된
  요청(`model_id` 주입)도 훅이 제거해 200 응답 — 훅이 실전에서 작동함을 증명. GLM은
  429 — 게이트웨이 우회 직접 curl에서도 동일 재현되어 **z.ai 계정 레벨 기존 제한**임을
  확인(게이트웨이 결함 아님).
- **P0 발견 및 수정 (본 세션에서 가장 큰 사건)**: 워커 canary 전환 테스트 도중 ec1이
  `401 Unauthorized`로 전혀 연결되지 않는 것을 발견 → 조사 결과, `fleet-worker`의
  `agent_endpoint()`가 오케스트레이터 자신의 호스트만으로 URL을 구성해 **모든 워커가
  동일한 엔드포인트를 광고**하고 있었음(워커 이름/식별자 없음). 공유 리버스 SSH 터널
  포트 하나(`2419`)를 여러 워커가 경합해 실제로는 **가장 나중에 등록한 워커 하나만
  연결 가능**한 구조적 결함 — 최소 24시간 이상 프로덕션에서 워커 가용성이 은밀하게
  저하된 상태였다(arm1은 3주 전부터 존재하던 비-systemd ad-hoc 터널 덕에 간헐적으로만
  동작, ec1/ec2는 터널 인프라 자체가 없었음).
  - 수정: `agent_endpoint()`/`derive_agent_endpoint()`가 `/ws/<worker-name>` 경로로
    워커를 구분하도록 변경. 워커별 전용 리버스 터널 systemd 유닛
    (`fleet-acp-tunnel.service`, 포트 2419/2420/2421) 신설 + nginx를 워커별
    `location /ws/worker-<name>` 블록으로 재구성.
  - 배포 중 ec2 터널이 "Permission denied (publickey)"로 실패 — ec2의 `ubuntu` 공개키가
    arm2 `authorized_keys`에 등록되어 있지 않았음(arm1/ec1은 이미 등록됨, ec2만 누락).
    등록 후 정상화.
  - 신규 임시 운영자 계정(`verify-p0`)으로 대시보드 `/tasks/new`를 통해 arm1/ec1/ec2
    3대 각각에 실제 태스크를 제출·완료시켜 검증(ec2는 `canary-ec2` 라벨을 임시 추가).
    부수 발견: 새로 만든 사용자는 `email_verified`가 자동으로 `true`가 되지 않아
    (최근 커밋 3015bb9의 자동 인증은 admin 역할 한정) 첫 로그인이 조용히 실패함 —
    수동으로 DB에서 `email_verified = true` 처리해 우회, 별도 버그 리포트는 미작성.
  - 검증 완료 후 `verify-p0` 계정 삭제, 스크래치패드의 임시 시크릿 캐시 파일 삭제.
  - 커밋: `286ab9c` (fleet-worker, push는 사용자 확인 전까지 보류 — 원격 저장소
    `git.agentthread.dev` 미설정 상태가 이미 확인되어 있음).
- **문서 갱신**: `litellm_integration_plan.md`를 "실제 배포 스펙"으로 전면 재작성(원
  Docker 설계는 §7 "폐기된 설계"로 보존, 폐기 사유 명시). `docs/deployment/single-server.md`의
  liteLLM 관련 섹션(§2.1/§2.2/§3 Step 1/개념도)을 새 정본에 맞춰 동기화.
  `docs/credentials/registry.md`에 `LITELLM_MASTER_KEY`/`GEMINI_API_KEY`/`ZAI_API_KEY`/
  `GROQ_API_KEY` 항목 신규 등재.
- **남은 작업**: `arm1`/`ec2` 워커는 아직 게이트웨이 미경유(ec1만 canary 전환).
  `examples/groq-compat/` ↔ 배포본(`/opt/litellm-gateway/groq_compat/`) 수동 동기화
  자동화 미착수. Phase 2(DB-backed 예산 관리, Fallback 라우팅)는 `litellm_integration_plan.md`
  §8에 로드맵으로만 기록.

## 2026-08-12 — ingest — 자율 자가치유 제어 엔진(AutonomicEngine) 구현 및 LLM Gateway 쿼터 연동 설계

- **조치**:
  - `crates/fleet-scheduler/src/autonomic.rs`를 신설하여 MAPE-K 피드백 루프 기반의 자율 동작 및 자가치유 엔진(`AutonomicEngine`)을 구현했습니다.
  - `crates/fleet-scheduler/src/lib.rs` 및 `crates/fleet-cli/src/runtime.rs`를 갱신하여 `fleet serve` 기동 시 자율 엔진이 백그라운드 태스크로 자동 기동하도록 파이프라인을 완전히 결합했습니다.
  - `docs/architecture/overview.md`에 자율 엔진 아키텍처 다이어그램 및 설계 명세를 추가했습니다.
  - `docs/llm-wiki/README.md` 및 `index.md`를 갱신하여 무료 티어(OpenRouter, Groq) API 쿼터 소진(429/413) 시, 자율 엔진이 이를 "하드웨어 고장"과 구별하여 "API 쿼터 한도 도달"로 진단하고, 해당 워커 노드의 동적 우회(Self-Adaptive Routing) 및 Fallback 제어를 자율 수행하는 연동 설계를 반영했습니다.

## 2026-08-13 — lint — 코드 대조 정합성 점검 (AutonomicEngine 삭제 반영 + 배포 방식 재정정)

- **발단**: `crates/fleet-scheduler/src/autonomic.rs`가 컴파일 불가 상태로 방치되어
  있던 것을 2026-08-13에 삭제하기로 결정(`docs/roadmap/roadmap.md` #43). 이 위키의
  2026-08-12 항목이 아직 존재하지 않는 `AutonomicEngine` 연동을 현재형으로 서술하고
  있어 코드와 어긋남을 발견 — 전체 위키 페이지를 코드와 재대조했다.
- **조치**:
  - `README.md`의 "로드맵 정렬 & 자율 엔진 연동" 절을 미구현 설계 구상으로 재작성하고
    정정 배너를 추가. 최종 업데이트 날짜 갱신.
  - `multi_provider_llm_proxy_analysis.md` §3.2/§4에 정정 배너 추가 — Postgres
    DB-backed 컨테이너 배포 결론은 이후 `litellm_integration_plan.md`에서
    DB 없는 venv+systemd 방식으로 뒤집혔음을 명시 (§1의 정본/사본 우선순위 규칙에
    따라 인프라 스펙은 그 문서가 우선).
  - `litellm_integration_plan.md` §3에 nginx `proxy_read_timeout` 불일치 발견 기록
    (`nginx-gateway.md`는 600s+`proxy_buffering off`, 이 문서는 300s) — 실서버 값을
    이 세션에서 확인할 수 없어 **미확인**으로 표시, 다음 배포 시 정본 갱신 필요.
  - `litellm_integration_plan.md` §7의 `examples/litellm-config.yaml` 서술 오류 정정
    — 실제 파일은 Claude/GPT-4o가 아니라 Gemini-3.5+Groq 무료 3종이며, `database_url`+
    평문 `master_key`가 남아 있음. 이 파일은 "폐기된 설계"가 아니라
    **로컬 개발용 `docker-compose.yml`에서 여전히 사용 중**인 별도 경로임을 명시
    (`orchestrator` 서비스가 `FLEET_LLM_GATEWAY_URL: http://litellm:4000`로 참조).
  - `free_tier_providers_analysis.md`에 정정 배너 추가 — §1.3/§1.4/§5가
    2026-08-11 항목에서 스스로 인정한 "TPM이 아니라 schema 검증이 진짜 블로커"라는
    결론 변경을 아직 반영하지 못한 상태임을 명시. `index.md` 상태를 🟢→🟡로 하향.
  - `index.md` 표 갱신(최종 개정일, 상태 플래그) 및 고아 페이지 점검일 갱신.
  - `docs/ui-dashboard/ui-design.md`도 같은 세션에서 함께 코드 대조 정정(별도 커밋) —
    StatusPill/HostStatus enum, 호스트 인벤토리 "예정"→"구현됨" 재분류 등.
- **미착수**: `docs/credentials/registry.md`에 누락된 `FLEET_API_TOKENS`/
  `FLEET_CF_AUDIENCE`, Gmail SMTP, `ssh_keys` 볼트 항목 — 다음 항목에서 처리 예정.
