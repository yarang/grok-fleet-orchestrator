# LLM 게이트웨이 및 프록시 위키 (LLM Gateway Wiki)

> 최종 업데이트: 2026-08-06.

이 페이지는 Grok Fleet Orchestrator에서 멀티 LLM 공급업체(OpenAI, Anthropic, Gemini 등)를 단일 API 규격으로 수용하고 통제하기 위한 **LLM Gateway (LLM Proxy)** 구조 및 상세 설계 가이드입니다.

---

## 🏛️ LLM 아키텍처 및 검토 문서

### 1. [멀티 LLM API 공급자 대안 분석 보고서](./multi_provider_llm_proxy_analysis.md)
*   **내용**: 기존 단일 LLM 대상에서 벗어나 멀티 벤더를 수용할 때의 아키텍처적 장단점 분석.
*   **비교 분석**: Python 기반 **`liteLLM`**과 Go 기반 **`One API`**, 그리고 내장 Rust 프록시 구현의 CPU/메모리 부하 및 기능 범위 비교.
*   **기술 검토 결론**:
    *   초경량 배포 및 기본 라우팅에는 Go 기반 `One API`가 유리하지만,
    *   에이전트/워커별 세부 토큰 제한, 비용 청구(Spend Control), 100+ 공급자 플러그인 유연성을 감안해 프로덕션에서는 **`liteLLM`**을 공식 표준 게이트웨이로 확정함.

### 2. [liteLLM 게이트웨이 연동 설계 계획서](./litellm_integration_plan.md)
*   **내용**: 시스템에 liteLLM을 연동하기 위한 인프라 패키징 및 설정 규칙 정의.
*   **구현 범위**:
    *   `docker-compose.yml` 에 liteLLM 이미지 및 Postgres 데이터베이스 스키마 격리 연동.
    *   `examples/litellm-config.yaml` 을 통한 Claude <-> GPT-4o 간의 로드밸런싱 및 Failover Fallback 규칙 설정.
    *   오케스트레이터 기동 시 `FLEET_LLM_GATEWAY_URL`을 체크하는 Fail-Fast 밸리데이터 구현.

---

## 🚀 향후 로드맵 전개 방향 (Roadmap Alignment)
*   본 위키의 설계는 공식 개발 로드맵 **[34번 마일스톤: liteLLM 중앙 게이트웨이 통합 및 연동]**과 1:1로 매핑됩니다.
*   본 프록시 구조가 안착되면, 3단계의 [하드웨어 자가 치유] 및 에이전트 자동 추론 작업 시 벤더 사정에 얽매이지 않고 회복 탄력적인 LLM 공급 체인을 확보하게 됩니다.
