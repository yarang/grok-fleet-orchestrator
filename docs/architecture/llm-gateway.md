---
type: architecture-decision
authority: canonical
implementation: partial
verification: code-checked
source: "docs/architecture/llm-gateway.md"
last_verified: "2026-08-25"
last_verified_commit: "working-tree"
owners: ["architecture", "deployment"]
---

# LLM Gateway 아키텍처

Fleet는 Worker의 LLM 호출을 선택적으로 중앙 gateway에 라우팅한다. 이 문서는 게이트웨이
**선택 기준**과 요청 경계만 소유한다. liteLLM의 설치·구성·복구는
[liteLLM 배포 Runbook](../deployment/litellm-gateway.md)이 소유한다. 과거 분석·배포 원문은 Git
이력에서 복원한다.

![LLM gateway routing](../assets/diagrams/architecture/llm-gateway-routing.mermaid)

## 두 개의 독립 경로

Fleet에서 Worker의 LLM endpoint를 정하는 경로는 **두 개**이며 서로 독립이다. 어느 공급자를
"추가"하는 비용은 어느 경로를 쓰느냐에 따라 완전히 다르므로 먼저 구분한다.

| | 경로 A — gateway | 경로 B — per-model credential |
|---|---|---|
| 설정 위치 | Worker `worker.toml`의 `[llm_proxy]` | `worker_credentials` 행 (worker × model) |
| 적용 범위 | 그 Worker의 **모든** 모델 | 하나의 `(worker, model_id)` 쌍 |
| 전달 방식 | subprocess 환경변수 주입 (`grok_process.rs`) | grok `config.toml`의 `[model.<id>]` 렌더링 |
| 담고 있는 값 | `gateway_url`, `api_key` | `base_url`, `api_key`(암호화), `model`, `api_backend`, `context_window` |
| 공급자 추가 비용 | gateway가 dialect를 구현해야 함 | **설정만** — Rust 변경 없음 |

경로 B의 `base_url`은 자유 문자열이고 `api_backend`는 요청 dialect를 고른다. 따라서
**OpenAI-compatible endpoint는 코드 변경 없이 이미 사용 가능하다.** 이것이 공급자 추가의
기본 경로이며, 경로 A는 Worker 전체를 한 gateway 뒤로 넣을 때만 쓴다.

## 현재 결정

- 멀티 공급자 변환과 모델 매핑은 자체 Rust proxy 대신 외부 gateway가 담당한다. liteLLM이
  기준 구현이며, 아래 dialect 계약을 만족하는 다른 gateway로 대체할 수 있다.
- Worker의 `[llm_proxy]` 설정은 `gateway_url`과 `api_key`를 선택적으로 받는다.
- `fleet-worker`는 grok·agy 하위 프로세스에 provider-compatible base URL과 gateway credential
  환경변수를 주입한다.
- Worker 요청은 Orchestrator API를 경유하지 않고 gateway로 직접 전송된다.
- `fleet serve`의 `FLEET_LLM_GATEWAY_URL`은 선택 설정으로 URL 형식만 검증한다. 요청 proxy나
  gateway health 보장을 의미하지 않는다.
- **OpenRouter는 경로 B의 유효한 endpoint다**(설정만으로 사용). 경로 A의 gateway로 쓰는 것은
  아래 dialect 계약을 부분적으로만 만족하므로 조건부로 허용한다.

## 게이트웨이 dialect 계약

`apply_llm_proxy_envs`(`fleet-worker/src/grok_process.rs`)는 **하나의** `gateway_url`을 네 공급자
계열의 환경변수로 팬아웃한다.

| 주입 변수 | 형태 | 하위 CLI가 기대하는 dialect |
|---|---|---|
| `OPENAI_BASE_URL` / `OPENAI_API_BASE` | `{gateway_url}/v1` | OpenAI Chat Completions |
| `ANTHROPIC_BASE_URL` | `{gateway_url}` (그대로) | Anthropic Messages |
| `GEMINI_BASE_URL` / `GEMINI_API_BASE` / `ANTIGRAVITY_BASE_URL` | `{gateway_url}` (그대로) | Gemini `generateContent` |
| `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `GEMINI_API_KEY` / `ANTIGRAVITY_API_KEY` / `FLEET_LLM_API_KEY` | 단일 `api_key` 원문 | — (아래 참고) |

**키도 같은 방식으로 팬아웃된다.** `[llm_proxy]`의 단일 `api_key`가 다섯 변수로 그대로
복제되며, base_url과 달리 **어느 dialect가 실제로 존재하는지와 무관하게** 주입된다. 위
불변식과 합치면 귀결이 하나 나온다 — 부분 구현 gateway를 경로 A로 넣으면, 그 gateway가
서비스하지 않는 dialect의 클라이언트도 **키를 손에 쥔 채** 기동한다. 그 클라이언트가 base_url을
무시하거나 공급자 기본 endpoint로 폴백하도록 구성돼 있으면 gateway 키가 제3자 endpoint로 나갈
수 있다. 이것이 "부분 구현 gateway는 경로 B로 돌린다"는 불변식의 두 번째 이유다.

이 팬아웃은 **gateway가 이 dialect를 전부 구현한다**고 가정한다. 이 가정은 지금까지 코드에만
있었고 문서에 명시된 적이 없다. 이것을 불변식으로 승격한다:

> 경로 A의 gateway로 채택하려면 그 gateway는 팬아웃되는 **모든** dialect를 같은 origin에서
> 서비스해야 한다. 일부만 구현하는 gateway를 넣으면 미구현 dialect를 쓰는 Agent만 **요청
> 시점에** 실패한다 — 설정 시점에는 아무 신호도 없다.

liteLLM은 이 계약을 만족하도록 구성할 수 있다. OpenRouter는 아래처럼 **부분적으로만** 만족한다.

## OpenRouter 적용 범위

2026-08-25 기준, 인증 없는 POST 프로브로 라우트 존재 여부를 확인했다(`401` = 라우트 존재·인증
거부, `404` = 라우트 없음).

| 경로 | 결과 | 판정 |
|---|---|---|
| `https://openrouter.ai/api/v1/chat/completions` | `401` | 존재 — OpenAI Chat Completions |
| `https://openrouter.ai/api/v1/responses` | `401` | 존재 — OpenAI Responses |
| `https://openrouter.ai/api/v1/messages` | `401`, Anthropic 형식 error 봉투 | 존재 — Anthropic Messages |
| `https://openrouter.ai/api/v1/models/{model}:generateContent` | `404` | **없음** |
| `https://openrouter.ai/api/v1beta/models/{model}:generateContent` | `404` | **없음** |
| `https://openrouter.ai/api/v1/v1/messages` | `404` | **없음** — base에 `/v1`을 넣고 다시 `/v1`을 이어붙이면 깨진다 |
| `https://openrouter.ai/api/messages` | `200` `text/html` (`x-matched-path: /[maker-id]/[slug]`) | **API 라우트 아님** — Next.js 마케팅 페이지다. 상태코드만 보면 존재로 오독한다 |

따라서:

- **경로 B(권장)**: `base_url = "https://openrouter.ai/api/v1"`, `api_backend`는
  `chat_completions` 또는 `responses`, `model`은 OpenRouter의 namespace 표기(`vendor/model`).
  **Rust 변경이 필요 없다.**
- **경로 A(권장하지 않음)**: `GEMINI_*`·`ANTIGRAVITY_*`가 존재하지 않는 route를 가리키므로
  dialect 계약을 만족하지 않는다. **Gemini·Antigravity 계열 Agent를 쓰는 Worker에서는 OpenRouter를
  경로 A gateway로 쓰지 않는다.** 혼합이 필요하면 liteLLM을 앞에 두고 OpenRouter를 liteLLM의
  upstream provider로 둔다.
- **경로 A의 OpenAI+Anthropic 조합은 확인 필요**: Gemini를 쓰지 않는 Worker라 해도 하나의
  `gateway_url`이 두 dialect를 동시에 만족하는지는 **하위 CLI의 base URL 이어붙이기 규칙에
  달려 있고, 이 저장소는 그것을 증명하지 않는다.** 프로브가 좁힌 결과는 다음과 같다.

  | 하위 CLI의 join 규칙 | 필요한 `gateway_url` | `OPENAI_BASE_URL`(`{gw}/v1`) 결과 | 판정 |
  |---|---|---|---|
  | `{base}/v1/messages` | `https://openrouter.ai/api` | `…/api/v1` → `401`(존재) | 두 dialect 동시 만족 |
  | `{base}/messages` | `https://openrouter.ai/api/v1` | `…/api/v1/v1` → `404` | **동시 만족 불가** |

  OpenRouter는 `/api/v1/messages` 한 형태만 서비스하므로 두 base 모양을 함께 허용하지 않는다.
  따라서 경로 A 채택 전에 대상 CLI로 1회 실요청 검증이 **필수**다.

## 현재 구현과 미구현 경계

| 능력 | 상태 | 근거 또는 완료 조건 |
|---|---|---|
| Worker의 gateway 환경변수 주입 | 구현 | `fleet-worker/src/config.rs`, `grok_process.rs` |
| OpenAI-compatible endpoint를 per-model credential로 지정 | 구현 | `worker_credentials.base_url`·`api_backend`, `fleet-credentials/src/lib.rs` |
| OpenRouter를 경로 B로 사용 | 구현(설정만) | 위 프로브 표의 `chat_completions`·`responses` 행. 실제 요청 성공은 미검증 — 아래 검증 한계 참조 |
| liteLLM 로컬 구성과 Groq request sanitizer | 구현 자산 있음 | `docker-compose.yml`, `examples/litellm-config.yaml`, `examples/groq-compat/` |
| 중앙 provider credential 관리 | 부분 구현 | gateway secret 전달·rotation·revoke Runbook 검증 필요 |
| gateway dialect 커버리지 검증 | 미구현 | 경로 A 채택 전 대상 CLI로 1회 실요청 검증 **필수**(위 OpenRouter 절). 자동 preflight는 완료 조건 미정 |
| Worker·Agent별 budget·usage 집계 | 미구현 | DB-backed 계정·예산 정책과 Fleet telemetry 계약 필요 |
| 공급자 장애·quota 기반 fallback | 미구현 | quota와 Worker health를 분리하는 상태·routing 계약 필요 |

## 유예한 결정

채울 방법이 없는 필드를 미리 만들지 않는다. 아래는 **의도적으로 만들지 않은** 것과 그 이유다.

| 유예 항목 | 이유 | 만들어야 할 조건 |
|---|---|---|
| `WorkerCredentials`의 임의 HTTP 헤더 필드 (`HTTP-Referer`, `X-Title`) | 렌더링 대상인 grok `[model.<id>]` 섹션에 헤더 슬롯이 없다. 지금 칼럼을 넣으면 **아무도 읽지 않는 값**이 된다 | 하위 Agent CLI가 per-model 커스텀 헤더를 받는 설정 키를 노출할 것 |
| provider routing 선호(`provider.order`, `allow_fallbacks`) 전달 | 위와 동일 — extra body를 실어보낼 경로가 없다 | 위와 동일, 또는 liteLLM을 경유해 gateway config에서 지정 |
| OpenRouter usage·cost 응답 수집 | 소비자가 없다. budget·usage 집계 자체가 미구현이다 | budget 계약이 먼저 정해질 것 |
| `api_backend` 값 검증 | 문서상 `chat_completions`\|`responses`지만 실제로는 자유 문자열이고, 기존 테스트(`fleet-api/tests/worker_llm_credential_authz.rs`)가 `"openai"`를 통과시킨다. 조이는 것은 HTTP API 동작 변경이므로 별도 커밋 | 별도 로드맵 항목으로 분리 |

## 불변식

- gateway credential과 provider key를 문서, Worker label, URL query에 기록하지 않는다.
- gateway 장애를 Worker 하드웨어 고장으로 단정하지 않는다.
- budget·fallback을 설정하지 않은 단일 `master_key` 구성을 비용 통제가 구현된 것으로
  표시하지 않는다.
- **경로 A gateway는 팬아웃되는 모든 dialect를 서비스해야 한다.** 부분 구현 gateway는 경로 B로
  쓰거나, 전부 구현하는 gateway의 upstream으로 둔다.
- 공급자를 "지원한다"고 적을 때 어느 경로인지(A인지 B인지) 함께 적는다. 두 경로의 비용과
  실패 양상이 다르다.

## 검증 한계

- 위 표는 **라우트 존재 여부**만 증명한다. 유효한 API key로 실제 completion을 받는 것,
  그리고 grok·agy가 이 endpoint로 정상 동작하는 것은 이 저장소에서 검증하지 않았다.
- 하위 Agent CLI(grok, agy)가 `ANTHROPIC_BASE_URL`·`GEMINI_BASE_URL`을 실제로 어떤 경로 규칙으로
  이어붙이는지는 외부 구현이며 이 저장소가 증명하지 않는다. 위 join 규칙 표는 **두 갈래를 좁혔을
  뿐 어느 쪽인지는 정하지 못했다.**
- 프론트매터의 `verification: code-checked`는 **Fleet 쪽 진술**(`apply_llm_proxy_envs`의 팬아웃
  형태, `WorkerCredentials`의 필드와 렌더링, `worker_credentials` 스키마)에만 해당한다.
  OpenRouter 라우트 표는 코드가 아니라 **2026-08-25에 실행한 외부 네트워크 프로브**에서 왔고,
  정책의 `verification` 어휘에는 외부 관측을 가리키는 값이 없다. 이 문서의 외부 진술은
  그날의 관측이며 공급자가 라우트를 바꾸면 무효가 된다.

## 관련 정본과 근거

- [liteLLM 배포 Runbook](../deployment/litellm-gateway.md)
- [Credential registry](../credentials/registry.md)
- [Groq compatibility hook](../../examples/groq-compat/README.md)
