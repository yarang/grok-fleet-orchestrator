# liteLLM 게이트웨이 도입 및 연동 설계 계획서 (liteLLM Integration Plan)

> 작성일: 2026-08-06. 담당: Antigravity.
> 
> 이 문서의 목적은 Grok Fleet Orchestrator에 **liteLLM 프록시 게이트웨이**를 표준 연동 체계로 안착시켜, 워커 노드와 오케스트레이터 내부에서 여러 외부 LLM 공급자(OpenAI, Anthropic, Gemini, Zhipu 등)를 단일 OpenAI API 규격으로 관리하고 비용 및 예산(Spend Tracking) 한도를 제어하도록 구성하는 기술 설계 계획을 제시하는 것입니다.

---

## 1. 도입 목표 (Goal)

1.  **멀티 LLM API 단일화**: 워커와 오케스트레이터의 LLM 통신 경로를 liteLLM으로 중앙화하고, 하위 LLM API의 키 및 포맷 전환을 프록시가 대행하도록 아키텍처 정비.
2.  **비용/예산 통제(Spend Control)**: 에이전트/워커별로 토큰 사용량과 과금 한도(Budget)를 설정하여 무분별한 API 오용 방지.
3.  **장애 자동 스위칭(Fallback)**: 메인 공급자(예: Anthropic) 장애 시 서브 공급자(예: OpenAI)로 끊김 없이 연동되는 로드밸런싱 확보.

---

## 2. 사용자 피드백/리뷰 필요 사항 (User Review Required)

> [!IMPORTANT]
> **인프라 종속성 추가 (PostgreSQL & Redis)**
> *   liteLLM의 비용 추적, 사용자 관리, 그리고 Rate limit 정책을 프로덕션 수준으로 구현하려면 데이터베이스(Postgres)와 캐시(Redis)가 필요합니다.
> *   기존 오케스트레이터 DB(`fleet-db`) 내에 `litellm` 전용 독립 데이터베이스/스키마를 생성하여 사용함으로써 추가 DB 서버 증설 오버헤드를 막을 것을 제안합니다.
> *   캐시용으로 Redis 컨테이너를 Docker Compose에 추가하거나, 소규모 배포 시에는 로컬 인메모리 캐시로 대체할 수 있게 기본값을 설정하겠습니다.

---

## 3. 상세 연동 아키텍처 및 흐름

```
[워커 / 대시보드 / 에이전트] 
       │ (OpenAI SDK 스펙 통신)
       ▼ (포트 4000)
┌────────────────────────────────────────────────────────┐
│             liteLLM Proxy Gateway                      │
│ - API Key 인증 & Rate Limiting                         │
│ - Model Mapping (e.g., 'gpt-4o' -> 'claude-3-5-sonnet')│
│ - Redis 캐싱 & DB 사용량 로깅                           │
└────────────────────────────────────────────────────────┘
       │ (각 벤더사 고유 포맷 번역)
       ├───► OpenAI API (GPT)
       ├───► Anthropic API (Claude)
       └───► Google Vertex AI (Gemini)
```

---

## 4. 제안된 변경 사항 (Proposed Changes)

### 4.1 [MODIFY] `docker-compose.yml` (Root)
*   **작업 내용**: `litellm` 게이트웨이 서비스 정의 추가.
*   **세부 구성**:
    *   BerriAI 공식 이미지 `ghcr.io/berriai/litellm:main-latest` 사용.
    *   포트 `4000` 노출.
    *   오케스트레이터 DB 서버(`db.internal`)에 종속성을 연결하여 데이터 저장.

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
      # 외부 LLM API 키 (필요한 것만 활성화)
      - OPENAI_API_KEY=${OPENAI_API_KEY}
      - ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}
      - GEMINI_API_KEY=${GEMINI_API_KEY}
    command: [ "--config", "/app/config.yaml", "--port", "4000", "--detailed_debug" ]
    depends_on:
      db:
        condition: service_healthy
```

---

### 4.2 [NEW] `examples/litellm-config.yaml`
*   **작업 내용**: liteLLM의 모델 라우팅, Fallback 및 로드밸런싱 설정을 선언하는 템플릿 예시 추가.

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

  # 3. 범용 추론 가상 모델 정의 (Fallback & Load Balancing 연동)
  - model_name: fleet-default-model
    litellm_params:
      model: anthropic/claude-3-5-sonnet-20241022
      api_key: "os.environ/ANTHROPIC_API_KEY"
    fallback_models: ["gpt-4o"] # Claude 장애 시 GPT로 스위칭

general_settings:
  master_key: sk-fleet-master-key-1234 # 관리 대시보드 진입 및 키 생성용 마스터키
  database_url: "os.environ/DATABASE_URL"

litellm_settings:
  drop_params: true # 지원되지 않는 파라미터가 와도 에러 대신 드롭 처리하여 호환성 극대화
  set_verbose: true
```

---

### 4.3 [MODIFY] `docs/deployment.md`
*   **작업 내용**: 배포 문서 내에 **"5. 외부 LLM Gateway (liteLLM) 연동 가이드"** 색션을 추가하여, 다중화된 오케스트레이터 배포 환경에서 백엔드 LLM 인프라를 확장하는 표준 명령어 및 가이드를 명시합니다.

---

### 4.4 [MODIFY] `crates/fleet-core/src/config.rs` & `fleet.env`
*   **작업 내용**: 오케스트레이터가 외부 LLM 프록시를 바라보도록 환경변수 명세를 추가하고, 시작 시 유효성 검증을 정비합니다.
    *   `FLEET_LLM_GATEWAY_URL` : liteLLM 프록시 게이트웨이 엔드포인트 (기본: `http://localhost:4000`)
    *   `FLEET_LLM_API_KEY` : liteLLM 통신용 API 키

---

## 5. 검증 계획 (Verification Plan)

### 자동화 테스트 (Automated Tests)
*   **테스트 대상**: 환경변수 밸리데이터 및 설정 파일 로딩 테스트.
*   **실행 명령어**:
    ```bash
    cargo test -p fleet-core --lib config::tests
    ```

### 수동 검증 (Manual Verification)
1.  `docker-compose up -d` 기동 후 `http://localhost:4000/health` 및 `http://localhost:4000/v1/models` 가 OpenAI 규격으로 사용 가능한 모델 목록을 올바르게 응답하는지 확인.
2.  `curl` 명령을 사용하여 임의의 가짜 키로 호출 시 차단되고, `sk-fleet-master-key-1234`로 호출 시 Claude/GPT 모델 추론 스트림이 정상 수신되는지 검증.
    ```bash
    curl http://localhost:4000/v1/chat/completions \
      -H "Content-Type: application/json" \
      -H "Authorization: Bearer sk-fleet-master-key-1234" \
      -d '{
        "model": "fleet-default-model",
        "messages": [{"role": "user", "content": "Hello, is liteLLM working?"}]
      }'
    ```
