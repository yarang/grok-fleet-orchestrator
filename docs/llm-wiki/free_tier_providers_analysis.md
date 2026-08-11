# 무료 티어 LLM 공급자(Groq / OpenRouter) 도입 분석 및 설계

> 최종 개정: 2026-08-11.
>
> 이 문서는 사용자가 제공한 Groq/OpenRouter 무료 티어 정보를 공식 문서 대조로 검증하고,
> Fleet의 기존 워커별 모델 분리 + 라벨 라우팅 아키텍처([`docs/architecture/overview.md`](../architecture/overview.md))에
> 어떻게 편입할지 설계한 문서다. [`multi_provider_llm_proxy_analysis.md`](./multi_provider_llm_proxy_analysis.md)(게이트웨이 선택의 정본)를
> 보완하는 문서이며, "무료 공급자를 쓸 것인가/어떻게 쓸 것인가"에 대한 정본이다.

---

## 1. 검증 결과

원본 제공 정보를 [Groq 공식 rate-limits 문서](https://console.groq.com/docs/rate-limits)와
[OpenRouter 공식 limits 문서](https://openrouter.ai/docs/api-reference/limits)로 대조 검증했다.

### 1.1 OpenRouter — 정확함, 정본으로 채택

원본 제공 수치(무과금 계정 RPM 20 / RPD 50, $10+ 과금 이력 시 RPD 1,000, RPM 20 유지)가 공식
문서와 **완전히 일치**한다. 추가로 공식 문서에서 확인한, 원본에 없던 중요 사항 2가지:

- **계정 단위로 통제된다.** "여러 계정이나 API 키를 만들어도 rate limit 회피 불가 — 전역으로
  통제된다"고 명시. 워커를 여러 대 늘려도 OpenRouter 무료 한도는 계정 전체에서 공유된다 —
  워커 대수를 늘려서 회피할 수 없다.
- 한도 초과 시 `429 Too Many Requests`, 권장 대응은 "크레딧 $10 이상 구매" 또는 "유료 모델로
  전환(플랫폼 한도 없음)".

### 1.2 Groq — 대체로 정확하나 모델 목록이 최신화 필요

원본의 카드 신청 불필요(신용카드 없이 즉시 사용) 주장은 [웹 검색으로 재확인](https://www.grizzlypeaksoftware.com/articles/p/groq-api-free-tier-limits-in-2026-what-you-actually-get-uwysd6mb) —
정확하다. 다만 **모델 목록/한도는 원본이 다소 stale** — 공식 문서 기준 현재 값:

| 모델 | RPM | RPD | TPM | TPD |
|---|---|---|---|---|
| `llama-3.1-8b-instant` | 30 | **14,400** | 6,000 | 500,000 |
| `llama-3.3-70b-versatile` | 30 | 1,000 | 12,000 | 100,000 |
| `openai/gpt-oss-120b` | 30 | 1,000 | 8,000 | 200,000 |
| `openai/gpt-oss-20b` | 30 | 1,000 | 8,000 | 200,000 |
| `openai/gpt-oss-safeguard-20b` | 30 | 1,000 | 8,000 | 200,000 |
| `qwen/qwen3.6-27b` | 30 | 1,000 | 8,000 | 200,000 |
| `groq/compound` | 30 | 250 | 70,000 | — |
| `groq/compound-mini` | 30 | 250 | 70,000 | — |
| `whisper-large-v3` (음성) | 20 | 2,000 | — | — |

> 원본이 언급한 "Qwen 3 32B"는 현재 공식 목록에 없다(모델이 `qwen/qwen3.6-27b`로 교체된 것으로
> 보임 — 벤더 측 라인업 변경). `groq/compound`, `groq/compound-mini`, `whisper-*`, 코드/가드용
> `llama-prompt-guard-2-*` 등은 원본에 없던 항목.

### 1.3 실사용 검증 (2026-08-11) — Groq는 카드 미등록 시 사실상 사용 불가, OpenRouter는 정상 동작

사용자가 실제 API 키를 발급해 `~/.grok/config.toml`에 등록하고 `grok` CLI로 end-to-end 검증했다.

- **Groq — 실패 (413 Payload Too Large)**: `grok -m groq-free`로 아주 짧은 프롬프트("say hello in
  exactly 3 words")를 보내도, 프로젝트 컨텍스트가 전혀 없는 `/tmp`에서조차 **매번 413**로
  거부됐다. 원인: grok-build 자체의 시스템 프롬프트 + 도구(bash/edit/read 등) 스키마만으로
  이미 **약 19,000토큰**을 소비하는데, `llama-3.1-8b-instant`의 TPM 한도는 6,000, 가장 큰
  무료 모델도 12,000에 불과해 **어떤 무료 모델을 선택해도 단일 턴조차 성립하지 않는다.**
  (raw `curl` 호출은 200 OK — 키 자체는 정상, TPM만 문제.) §1.2에서 언급한 "Developer 티어"
  (카드 등록, 과금 없음, 10배 한도 → 대략 60,000~120,000 TPM 추정)가 grok-build용으로는
  **사실상 필수 조건**으로 결론이 바뀐다.
- **OpenRouter — 성공**: `grok -m openrouter-free`(`openai/gpt-oss-20b:free`, context 131,072)로
  실제 응답을 정상 수신했다. OpenRouter 무료 티어는 Groq와 달리 **TPM(토큰/분) 한도가 없고**
  RPM/RPD(요청 횟수)만 제한하므로, grok-build의 큰 시스템 프롬프트도 "요청 1건"으로만
  카운트되어 문제가 없다. 단, **하루 50회**라는 요청 횟수 한도는 여전히 유효 — 코딩 에이전트가
  한 세션에서 쉽게 소진할 수 있는 양이므로 실사용 시 유의.

**추가로 발견한 중요 옵션**: Groq은 신용카드를 등록해도 **과금되지 않는** "Developer 티어"를
제공한다 — 카드 등록만으로 **한도가 10배 증가**하고 유료 사용 시 토큰당 25% 할인까지 받는다.

> ⚠️ **실측 결과 (2026-08-11): 카드 미등록 무료 티어는 `grok` CLI(grok-build)에 사실상 사용
> 불가.** 실제 발급받은 키로 `grok -m groq-free` 호출 시(프로젝트 컨텍스트 없는 `/tmp`에서도
> 동일) 매 요청마다 **413 Payload Too Large** — grok-build 자체의 시스템 프롬프트 + 도구
> 스키마만으로 이미 **약 19,000토큰**을 소비하는데, `llama-3.1-8b-instant`의 TPM 한도는
> 6,000, 가장 큰 무료 모델(`llama-3.3-70b-versatile` 등)도 12,000에 불과해 **어떤 모델을
> 골라도 단일 턴조차 성립하지 않는다.** 위 "Developer 티어"(카드 등록, 과금 없음, 10배 한도 →
> 대략 60,000~120,000 TPM 추정)가 사실상 필수 조건이다 — 카드 등록 없이는 grok-build용으로는
> 채택 불가로 결론 변경. (raw API 호출 자체는 curl로 200 OK 확인 — 키는 정상, TPM만 문제.)

---

## 2. Fleet 통합 설계

### 2.1 기존 아키텍처 재사용 — 새 시스템 불필요

오늘 세션 초반에 확정한 **워커별 모델 분리 + 라벨 라우팅** 설계를 그대로 재사용한다. 새 인프라를
만들 필요가 없다:

- Groq/OpenRouter 둘 다 **OpenAI 호환 Chat Completions API**를 제공하므로, 기존
  [`fleet-credentials`](../../crates/fleet-credentials/src/lib.rs)의 `WorkerCredentials` →
  `~/.grok/config.toml` `[model.X]` 섹션 렌더링을 그대로 쓴다.

```toml
[model.groq-free]
base_url = "https://api.groq.com/openai/v1"
api_key = "gsk_..."
model = "llama-3.1-8b-instant"
api_backend = "chat_completions"
context_window = 131072

[model.openrouter-free]
base_url = "https://openrouter.ai/api/v1"
api_key = "sk-or-v1-..."
model = "meta-llama/llama-3.1-8b-instruct:free"   # ':free' 접미사 필수
api_backend = "chat_completions"
context_window = 128000
```

- 워커 라벨에 `tier=free`, `provider=groq`(또는 `openrouter`)를 부여해, 기존 `selector.rs`의
  모델/라벨 기반 라우팅으로 특정 태스크를 이 워커로만 보낼 수 있다 — 코드 변경 없음.
- Postgres `worker_credentials` 테이블에 암호화 저장되므로 [`docs/credentials/registry.md`](../credentials/registry.md)에
  항목만 추가하면 기존 크리덴셜 관리 지침을 그대로 따른다.

### 2.2 새로 고려해야 할 것 — 쿼터는 "워커 건강"과 다른 축이다

기존 [`CircuitBreaker`](../../crates/fleet-scheduler/src/breaker.rs)/재조정 루프는 **워커 단위
dispatch 성공·실패**만 본다. 그런데 무료 티어 한도 초과는 워커가 고장난 게 아니라 **계정
쿼터가 바닥난 것**이다 — 특히 OpenRouter 무과금 계정은 **하루 50회**로, 코딩 에이전트가 한 세션
안에 다 써버릴 수 있는 양이다. 이 차이를 무시하면:

- 쿼터 소진 후 들어오는 요청마다 429 → `FailureKind::WorkerError`로 잡혀 CircuitBreaker가 열림
  → 자정(UTC) 쿼터 리셋 이후에도 half-open 재시도까지 불필요하게 지연될 수 있음.
  Groq/OpenRouter 모두 UTC 자정 리셋으로 보이나, 정확한 리셋 시각은 각 계정 대시보드에서
  확인 필요(공식 문서에 리셋 시각 명시 없음 — §6 체크리스트).
- 실패 원인이 "쿼터 소진"인지 "실제 API 장애"인지 대시보드에서 구분이 안 됨(현재
  `TaskFailure.error`는 자유 텍스트라 429 메시지가 그대로 남긴 하지만, 별도 분류는 없음).

**두 가지 대응 방안**(§6에서 사용자 결정 필요):

| 방안 | 구현 비용 | 설명 |
|---|---|---|
| **A. 최소 대응(권장 시작점)** | 없음 — 라벨/문서만 | 쿼터 소진은 CircuitBreaker가 자연히 흡수하게 둔다. 무료 티어는 "가끔 안 될 수 있는 자원"으로 취급하고 저우선순위 태스크에만 라우팅. 429 발생 빈도를 관찰 후 필요 시 B로 승격. |
| **B. 능동적 쿼터 추적** | 중간 — 신규 DB 테이블 + 스케줄러 훅 | `model_quota_usage(model_id, date, request_count)` 같은 테이블을 두고, dispatch 전에 오늘 카운트를 확인해 한도 근접 시 그 모델을 selector 후보에서 제외. 429를 사전에 대부분 회피 가능하지만 새 코드 경로 필요. |

### 2.3 UI 통합

이미 대기 중이던 두 항목과 직접 연결된다(이전에 큐에 넣어두고 착수 못 한 작업들):

- **모델 선택 UI**(대시보드에서 태스크 제출 시 모델 선택): 무료 티어 모델에는
  "무료 · 일일 50회"처럼 한도를 배지로 표시해, 운영자가 실수로 중요 태스크를 극히 제한된
  자원에 보내지 않도록 한다.
- **토큰 사용량 통계**: 방안 B(능동적 쿼터 추적)를 만들 경우, 같은 대시보드 위젯에서
  "오늘 남은 무료 쿼터"를 함께 보여줄 수 있다 — 별도 UI를 새로 만들 필요 없이 기존 계획에
  필드만 추가.

두 UI 작업 모두 아직 미착수 상태이므로, 이 문서의 결정(§6)이 먼저 나야 그 UI 작업의 스펙이
확정된다 — 순서상 이 문서가 선행 조건이다.

---

## 3. 문서 위치 결정

**`docs/llm-wiki/`(이 디렉토리) 내부에 두기로 결정** — 별도 디렉토리를 만들지 않는다.

- `docs/llm-wiki/`가 이미 "멀티 LLM 공급자 게이트웨이 선택/스펙"을 다루는 전용 위키이고,
  이 문서는 정확히 같은 주제(어떤 LLM 공급자를 어떤 조건으로 쓸지)의 연장선이다.
- 새 디렉토리를 만들면 `docs/index.md`의 도메인 클러스터링(architecture/deployment/
  worker-bootstrap/...)에 억지로 9번째 카테고리를 추가해야 하는데, 이 주제는 이미 8번째
  카테고리("LLM 게이트웨이")에 정확히 속한다 — 분절(fragmentation)을 피하는 것이 이번
  `docs/` 재구성 작업 전체의 원칙이었다([`docs/log.md`](../log.md) 참고).

---

## 4. 코드 변경 필요 여부

**§2.1(연동)만 진행할 경우 코드 변경 없음** — `config.toml` 작성 + 워커 라벨 부여만으로 끝난다.
§2.2의 방안 B(능동적 쿼터 추적)를 선택하면 다음이 필요하다(아직 미착수, 사용자 결정 대기):

1. `fleet-store` 마이그레이션: `model_quota_usage` 테이블.
2. `fleet-scheduler/src/selector.rs`: 후보 필터링에 "오늘 남은 쿼터 > 0" 조건 추가.
3. `fleet-api`: dispatch 성공 시 카운터 증가(어느 시점에 카운트할지 — 요청 시점 vs 성공
   응답 시점 — 는 설계 세부 사항으로 별도 논의 필요).
4. 대시보드: 쿼터 표시 위젯.

---

## 5. 체크리스트 — 사용자 결정 필요

- [x] **API 키 발급**: Groq(`gsk_...`), OpenRouter(`sk-or-v1-...`) 둘 다 2026-08-11 전달받아
      로컬 `~/.grok/config.toml`에 `[model.groq-free]`/`[model.openrouter-free]`로 등록,
      `grok` CLI로 end-to-end 검증 완료(§1.3). **Fleet 워커(`docs/credentials/`)에는 아직
      등록 안 함** — 로컬 개인 사용 확인 단계.
- [ ] **공급자 활성화 (실사용 가능 여부 기준으로 재정리됨)**:
      - **OpenRouter**: 즉시 사용 가능 확인됨 — Fleet 워커에도 연동할지 결정 필요
      - **Groq**: 카드 미등록 상태로는 grok-build에 사실상 사용 불가(§1.3) — ① 카드 등록해
        Developer 티어로 갈지, ② grok-build가 아닌 다른 경량 용도(예: 직접 API 호출, 단순
        completion)로만 남겨둘지, ③ 아예 보류할지
- [ ] **Groq Developer 티어(무료, 카드 등록만, 과금 없음)**: §1.3 실측으로 카드 등록이
      사실상 필수 조건이 됨 — 등록할지 여부
- [ ] **OpenRouter 하루 50회 한도 운용 방식**: 개인 로컬 사용에 한정할지, Fleet 워커에도
      연동해 여러 태스크가 같은 쿼터를 공유하게 할지(§1.1 — 계정 단위 전역 공유라 워커를
      늘려도 한도는 안 늘어남)
- [ ] **OpenRouter Fleet 워커 연동**: 로컬 개인 사용을 넘어 arm1/ec1/ec2 워커의
      `config.toml`에도 추가할지, 추가한다면 라벨(`tier=free`)로 어떤 태스크만 보낼지
- [ ] **쿼터 관리 수준**: §2.2 방안 A(최소 대응, 지금 시작) vs 방안 B(능동적 추적, 코드
      필요) — A로 시작 후 필요 시 B로 승격을 권장. OpenRouter 하루 50회는 특히 방안 B
      (사전 차단)의 효용이 커 보임 — 소진 후 429 반복보다 사전에 다른 모델로 우회가 나음
- [ ] **호스트 배치**: 기존 워커(arm1/ec1/ec2)에 추가 항목으로만 넣을지, 전용 "무료 티어
      실험용" 워커를 새로 둘지
- [ ] **라우팅 정책**: 무료 티어 모델을 자동으로 폴백 후보에 넣을지(기본 유료 모델 실패 시
      무료로 자동 전환), 명시적으로 요청한 태스크에만 쓸지

## 6. 다음 단계

위 체크리스트 답변을 주시면 §2.1(연동)부터 바로 진행 가능합니다 — API 키만 있으면 코드
변경 없이 당일 적용됩니다. 방안 B(쿼터 추적)를 원하시면 별도 설계 스펙을 먼저 작성하겠습니다.

## 참고 자료

- [Groq Rate Limits](https://console.groq.com/docs/rate-limits) — 공식 문서, 모델별 RPM/RPD/TPM/TPD
- [Groq API Free Tier Limits in 2026](https://www.grizzlypeaksoftware.com/articles/p/groq-api-free-tier-limits-in-2026-what-you-actually-get-uwysd6mb) — 카드 불요 확인
- [OpenRouter API Rate Limits](https://openrouter.ai/docs/api-reference/limits) — 공식 문서
