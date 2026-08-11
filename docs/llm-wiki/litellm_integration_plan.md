# liteLLM 게이트웨이 도입 및 연동 설계 계획서

> 최종 개정: 2026-08-11 (§3.3 공급자 strict-schema 대응 훅 추가).

본 문서는 Grok Fleet Orchestrator에 **liteLLM 프록시 게이트웨이**를 연동하기 위한 인프라 패키징 및 설정 파일 설계 사양을 정의합니다.

> ⚠️ **정본(canonical source) 표시**: 아래 §3의 `docker-compose.yml` / `litellm-config.yaml` 사양이 liteLLM 인프라 정의의 정본이다. [`docs/deployment/single-server.md`](../deployment/single-server.md) 등 다른 문서에 등장하는 동일 스펙은 **이 문서를 인용한 사본**이며, 값이 어긋나면 이 문서가 우선한다. 포트·이미지 태그·환경변수를 변경할 때는 이 문서를 먼저 갱신한 뒤 인용처를 동기화한다.

---

## 1. 도입 목표 (Goal)

1.  **멀티 LLM API 단일화**: 워커와 오케스트레이터의 LLM 통신 경로를 liteLLM으로 중앙화하고, 하위 LLM API의 키 및 포맷 전환을 프록시가 대행하도록 아키텍처 정비.
2.  **비용/예산 통제(Spend Control)**: 에이전트/워커별로 토큰 사용량과 과금 한도(Budget)를 설정하여 무분별한 API 오용 방지.
3.  **장애 자동 스위칭(Fallback)**: 메인 공급자(예: Anthropic) 장애 시 서브 공급자(예: OpenAI)로 끊김 없이 연동되는 로드밸런싱 확보.

---

## 2. 인프라 최적화 의사 결정 (Database & Cache Settings)

*   **PostgreSQL 단일화 (Redis 제거)**:
    *   liteLLM의 토큰 비용 추적, API 키 발급 및 한도 설정을 위해 PostgreSQL 연동을 활용합니다.
    *   추가적인 캐시 서버(Redis) 증설 오버헤드를 막기 위해, 소규모/중규모 배포 환경에서는 Redis 없이 기존 PostgreSQL 서버 내 독립 DB(`litellm`)만을 바인딩하여 심플하게 운영합니다.

---

## 3. 상세 구성 명세 (Configuration Spec)

### 3.1 `docker-compose.yml` 추가 사양
오케스트레이터의 `docker-compose.yml`에 게이트웨이 서비스를 병합합니다.

```yaml
  litellm:
    image: ghcr.io/berriai/litellm:main-latest
    container_name: fleet-litellm-gateway
    ports:
      - "4000:4000"
    volumes:
      - ./examples/litellm-config.yaml:/app/config.yaml
    environment:
      - DATABASE_URL=postgresql://fleet:secret@db.internal:5432/litellm
      # 외부 LLM API 키 (필요한 것만 주입)
      - OPENAI_API_KEY=${OPENAI_API_KEY}
      - ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}
      - GEMINI_API_KEY=${GEMINI_API_KEY}
    command: [ "--config", "/app/config.yaml", "--port", "4000", "--detailed_debug" ]
    depends_on:
      db:
        condition: service_healthy
```

### 3.2 `examples/litellm-config.yaml` 템플릿 명세
프로젝트 최상위의 예시 템플릿 파일로 신설할 설정 구조입니다.

```yaml
model_list:
  # 1. Claude 모델 매핑
  - model_name: claude-3-5-sonnet
    litellm_params:
      model: anthropic/claude-3-5-sonnet-20241022
      api_key: "os.environ/ANTHROPIC_API_KEY"

  # 2. OpenAI GPT 모델 매핑
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: "os.environ/OPENAI_API_KEY"

  # 3. 가상 대표 모델 정의 (Fallback 연동)
  - model_name: fleet-default-model
    litellm_params:
      model: anthropic/claude-3-5-sonnet-20241022
      api_key: "os.environ/ANTHROPIC_API_KEY"
    fallback_models: ["gpt-4o"] # Claude 장애 시 GPT로 자동 대체

general_settings:
  master_key: sk-fleet-master-key-1234 # 관리 API 및 대시보드 진입용 마스터키
  database_url: "os.environ/DATABASE_URL"

litellm_settings:
  drop_params: true # 지원하지 않는 비표준 인자가 들어와도 안전하게 드롭 후 통과 처리
  set_verbose: true
```

### 3.3 공급자 strict-schema 대응 훅 (`groq-compat`)

> 추가: 2026-08-11. 실제 파일은 [`examples/groq-compat/`](../../examples/groq-compat/),
> 배경·실측·검증 절차는 해당 디렉토리의 `README.md`가 정본이다.

Groq 처럼 `chat/completions` **요청 본문을 엄격 검증**하는 공급자는, 스펙에 없는
프로퍼티가 메시지에 하나라도 붙어 있으면 400 으로 거부한다. `grok` CLI(grok-build)는
assistant 메시지에 `model_id` / `model_fingerprint` 를 붙여 보내므로, **툴 호출이
한 번이라도 발생한 턴은 두 번째 요청부터 전부 실패**한다(= 에이전틱 작업 불가).

**§3.2 의 `drop_params: true` 로는 해결되지 않는다** — 이 옵션은 top-level 파라미터만
드롭하고 메시지 프로퍼티는 건드리지 않는다(2026-08-11 실측). liteLLM 자체도 이 필드를
걸러주지 않고 그대로 업스트림에 전달한다. 따라서 pre-call 훅이 별도로 필요하다.

```yaml
litellm_settings:
  callbacks: ["groq_compat.litellm_hook.proxy_handler_instance"]
```

liteLLM 은 이 문자열을 **`config.yaml` 이 있는 디렉토리 기준**으로 해석해
`<config_dir>/groq_compat/litellm_hook.py` 를 로드한다
(`litellm/proxy/types_utils/utils.py::get_instance_fn`). 그러므로 §3.1 의 볼륨
마운트에 다음이 **반드시** 포함되어야 한다 — 빠지면 liteLLM 기동 시 콜백 임포트가
실패한다:

```yaml
    volumes:
      - ./examples/litellm-config.yaml:/app/config.yaml:ro
      - ./examples/groq-compat:/app/groq_compat:ro   # 콜백 모듈 (필수)
    environment:
      - GROQ_API_KEY=${GROQ_API_KEY:-}
```

훅은 OpenAI Chat Completions 스펙이 정의한 프로퍼티만 남기는 **화이트리스트** 방식이다.
공급자가 스펙 밖 필드를 어차피 거부하므로 정보 손실이 발생하지 않으며, 클라이언트가
새 비표준 필드를 추가해도 다시 깨지지 않는다.

---

## 4. 검증 계획 (Verification Plan)

### 4.1 자동화 테스트
*   **테스트 대상**: 오케스트레이터 기동 시 `FLEET_LLM_GATEWAY_URL` 환경변수 Fail-Fast 밸리데이터 동작 검증.
*   **명령어**: `cargo test -p fleet-core`

### 4.2 수동 API 연동 테스트
Nginx를 거치거나 내부 망에서 직접 통신하여 OpenAI 호환 규격으로 추론을 던져봅니다.
```bash
curl http://localhost:4000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer sk-fleet-master-key-1234" \
  -d '{
    "model": "fleet-default-model",
    "messages": [{"role": "user", "content": "Hello, is liteLLM working?"}]
  }'
```
