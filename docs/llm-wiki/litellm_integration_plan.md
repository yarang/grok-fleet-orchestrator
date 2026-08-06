# liteLLM 게이트웨이 도입 및 연동 설계 계획서

> 최종 개정: 2026-08-06.

본 문서는 Grok Fleet Orchestrator에 **liteLLM 프록시 게이트웨이**를 연동하기 위한 인프라 패키징 및 설정 파일 설계 사양을 정의합니다.

> ⚠️ **정본(canonical source) 표시**: 아래 §3의 `docker-compose.yml` / `litellm-config.yaml` 사양이 liteLLM 인프라 정의의 정본이다. [`docs/single_server_deployment_plan.md`](../single_server_deployment_plan.md) 등 다른 문서에 등장하는 동일 스펙은 **이 문서를 인용한 사본**이며, 값이 어긋나면 이 문서가 우선한다. 포트·이미지 태그·환경변수를 변경할 때는 이 문서를 먼저 갱신한 뒤 인용처를 동기화한다.

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
