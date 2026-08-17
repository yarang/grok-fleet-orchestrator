# examples/groq-compat/ — Groq 호환 정규화 (OpenAI strict-schema sanitizer)

Groq 의 OpenAI 호환 엔드포인트를 `grok` CLI(grok-build) 같은 클라이언트가
**에이전틱 작업(툴 호출 포함)** 에 쓸 수 있게 만드는 요청 본문 정규화 계층.

## 문제

Groq 은 `chat/completions` 요청 본문을 **엄격하게 검증**한다. 스펙에 없는
프로퍼티가 메시지에 하나라도 붙어 있으면 400 으로 거부한다:

```
'messages.6' : for 'role:assistant' the following must be satisfied
[('messages.6' : property 'model_id' is unsupported)]
```

`grok` CLI 는 assistant 메시지에 `model_id` / `model_fingerprint` 를 붙여
대화 이력에 저장하고 그대로 페이로드에 실어 보낸다. xAI / OpenAI / OpenRouter
는 모르는 필드를 무시하므로 문제가 없지만 **Groq 은 거부한다.**

결과적으로 **툴 호출이 한 번이라도 일어난 턴은 두 번째 요청부터 전부 실패**한다.
첫 요청(툴 호출 전)은 성공하므로 "간단한 질문은 되는데 코드를 읽으려 하면
죽는" 형태로 나타난다. 즉 TPM 한도와 무관하게 **에이전틱 작업이 원천 불가**했다.

### 실측 (2026-08-11)

| 검증 | 결과 |
|---|---|
| Groq 3개 무료 모델(8b / gpt-oss-20b / 70b), 4개 독립 세션 | 100% 재현 — 턴 내부·턴 간 모두 400 |
| 동일 에이전트·동일 플로우를 OpenRouter 로 실행 | 정상 동작 → **Groq 고유 문제** 확정 |
| liteLLM 게이트웨이 경유 (`litellm.completion`) | **동일하게 400** — liteLLM 은 걸러주지 않음 |
| liteLLM `drop_params: true` 적용 | **여전히 400** — 이 옵션은 top-level 파라미터만 처리, 메시지 프로퍼티는 대상 아님 |
| 본 디렉토리의 정규화 적용 후 | ✅ read → edit → confirm 멀티스텝 루프 정상 완주 |

## 구성

| 파일 | 역할 |
|---|---|
| `sanitizer.py` | 순수 로직. 의존성 없음. 두 경로가 공유하는 단일 화이트리스트. |
| `litellm_hook.py` | **정본 경로** — liteLLM 게이트웨이 `async_pre_call_hook` 어댑터. |
| `shim.py` | **로컬 경로** — docker 없이 쓰는 standalone 리버스 프록시(표준 라이브러리만 사용). |
| `test_sanitizer.py` | `sanitizer.py` 단위 테스트. `python3 test_sanitizer.py` 로 실행. |

### 화이트리스트 방식을 쓰는 이유

허용 목록은 정확히 "OpenAI Chat Completions 스펙이 정의한 프로퍼티"다.
Groq 은 그 밖의 필드를 **어차피 전부 거부**하므로, 화이트리스트가 Groq 이
받아줬을 정보를 잃게 만드는 일은 원리적으로 없다. 반대로 블랙리스트 방식은
클라이언트가 새 비표준 필드를 추가할 때마다 다시 깨진다.

---

## 경로 1 — liteLLM 게이트웨이 (정본, Fleet 프로덕션)

[`LLM Gateway 아키텍처`](../../docs/architecture/llm-gateway.md)
가 정한 대로 **모든 추론 주체는 liteLLM 을 바라본다.** 이 훅은 그 게이트웨이
안에서 동작하므로 워커·오케스트레이터 코드는 전혀 바뀌지 않는다.

`examples/litellm-config.yaml` (이미 반영됨):

```yaml
litellm_settings:
  callbacks: ["groq_compat.litellm_hook.proxy_handler_instance"]
```

`docker-compose.yml` (이미 반영됨) — liteLLM 은 콜백 문자열을 **config.yaml 이
있는 디렉토리 기준**으로 해석하므로 이 디렉토리를 config 옆에 마운트해야 한다:

```yaml
volumes:
  - ./examples/litellm-config.yaml:/app/config.yaml:ro
  - ./examples/groq-compat:/app/groq_compat:ro
```

Groq API 키는 환경변수로 주입한다:

```bash
export GROQ_API_KEY=gsk_...
docker compose up -d litellm
```

검증:

```bash
curl http://localhost:4000/v1/chat/completions \
  -H "Authorization: Bearer sk-litellm-master-key" \
  -H 'Content-Type: application/json' \
  -d '{"model":"groq-free-70b","messages":[
        {"role":"user","content":"hi"},
        {"role":"assistant","content":"ok","model_id":"x"},
        {"role":"user","content":"again"}]}'
```

훅이 없으면 위 요청은 400 (`property 'model_id' is unsupported`), 있으면 200.
liteLLM 로그에 다음이 남는다:

```
groq-compat: stripped non-standard message properties ['model_id'] (call_type=completion)
```

---

## 경로 2 — standalone shim (로컬 개발)

노트북에서 `grok` CLI 하나만 Groq 에 붙이려고 Postgres + liteLLM 컨테이너를
띄우는 것은 과하다. `shim.py` 는 같은 정규화를 표준 라이브러리만으로 수행한다.

```bash
# 1) 프록시 기동
python3 examples/groq-compat/shim.py
# [shim] listening on http://127.0.0.1:8899/v1 -> https://api.groq.com/openai/v1

# 2) ~/.grok/config.toml
#    [model.groq-free-70b]
#    base_url    = "http://127.0.0.1:8899/v1"
#    api_key     = "gsk_..."
#    model       = "llama-3.3-70b-versatile"
#    api_backend = "chat_completions"

# 3) 사용
grok -m groq-free-70b -p "..."
```

환경변수: `PORT`(기본 8899), `HOST`(기본 127.0.0.1), `UPSTREAM_BASE_URL`.
API 키는 저장하지 않고 클라이언트의 `Authorization` 헤더를 그대로 전달한다.

---

## 남는 제약 — TPM 은 여전히 실질적 상한

정규화는 **프로토콜 문제만** 해결한다. Groq 무료 티어의 분당 토큰 한도(TPM)는
그대로이며, 이것이 실사용의 진짜 상한이다.

- 멀티스텝 턴은 모델 호출 N 회 × 프롬프트 크기만큼 **같은 1분 안에** 누적된다.
- 프롬프트를 최소화(약 3,200토큰)해도 8K TPM 모델은 분당 약 2회 호출이 한계다.
- 즉 **동작은 하지만 느리다** — 툴을 여러 번 쓰는 작업은 분 단위로 끊긴다.

과거 프롬프트 최소화 실측과 무료 티어 한도 조사는 Git 이력에서 복원한다. 외부
공급자의 현재 한도는 배포 전 공식 문서로 다시 확인한다.
