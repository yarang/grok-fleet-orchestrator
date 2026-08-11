# 멀티 LLM API 공급자 지원을 위한 프록시(liteLLM) 도입 보고서

> 최종 개정: 2026-08-06.

본 문서는 Grok Fleet Orchestrator의 LLM 호출 경로를 통일하고, 비용 통제 및 가용성 향상을 위해 **LLM Gateway**로서 **liteLLM**을 채택하고 정립하는 기술적 분석 보고서입니다.

---

## 1. 아키텍처 내 LLM Proxy의 필요성

워커 노드와 오케스트레이터의 LLM 통신 경로에 **LLM Proxy(liteLLM)**가 통합되면 다음과 같은 구조로 일원화됩니다.

```
[Worker 01] ──┐       ┌───────────────────────────────────────┐
[Worker 02] ──┼──────►│             liteLLM Proxy             │       ┌──► OpenAI (GPT)
[Worker 03] ──┘       │  - 표준화된 OpenAI API 규격 수신       ├───────┼──► Anthropic (Claude)
                      │  - API Key 및 테넌트별 예산 통제        │       └──► Google Gemini
                      └───────────────────────────────────────┘
```

모든 내부/외부 추론 주체는 표준화된 **OpenAI API 스펙(SDK)**만을 바라보고 코딩을 수행하며, 실제 타겟 모델로의 변환과 키 매핑은 프록시가 투명하게 대행합니다.

---

## 2. 대안 비교: liteLLM vs One API vs 자체 구현(Rust-native)

| 기준 | **liteLLM** (Python) | One API (Go) | 자체 구현 (fleet-credentials 직결) |
|---|---|---|---|
| 배포 무게 (RAM/CPU) | 중간~무거움 (Python 런타임) | 매우 가벼움 (단일 Go 바이너리) | 없음 (별도 프로세스 불필요) |
| 지원 프로바이더 수 | 100+ (OpenAI/Anthropic/Gemini/Bedrock 등) | 주요 공급자 위주, 확장 속도 느림 | 워커 config.toml에 등록된 것만 |
| 에이전트/워커별 Spend Control (예산·한도) | 내장 (`general_settings`, 태그별 budget) | 제한적 (사용자 단위 위주) | 없음 (자체 구현 필요) |
| Fallback / 로드밸런싱 | 내장 (`fallback_models`) | 있음 (단순 라운드로빈 수준) | 없음 |
| 관리 대시보드 | 내장 | 있음 (경량) | 없음 |
| 추가 인프라 요구 | Postgres (Redis는 선택, 본 프로젝트는 제외) | Postgres/MySQL | 없음 (현재 방식) |
| 운영 복잡도 | 중간 | 낮음 | 최저 (이미 구현됨) |

**선택 기준**: 단순 라우팅과 최소 리소스만 필요하다면 One API가 유리하다. 그러나 이 프로젝트는 로드맵 #34가 요구하는 **에이전트/워커별 세부 토큰 예산 통제(Spend Control)**와 **다중 공급자 장애 시 Fallback**이 핵심 요구사항이며, 향후 지원 프로바이더가 늘어날 가능성(100+ 플러그인 생태계)까지 감안하면 초기 리소스 비용보다 기능 완결성이 우선한다. 따라서 **liteLLM을 공식 표준으로 채택**한다 — One API는 "더 가볍다"는 이유만으로는 이 요구사항을 충족하지 못하므로 채택하지 않는다.

> 과거 [`docs/deployment/single-server.md`](../deployment/single-server.md) 초안에는 반대로 "무겁고 복잡한 liteLLM 대신 One API"로 기술되어 있었다. 이는 본 비교 분석 이전에 작성된 문서로, 2026-08-06 정합성 수정을 통해 본 결론을 따르도록 갱신했다. **이 문서(`multi_provider_llm_proxy_analysis.md`)가 게이트웨이 선택에 대한 단일 진실 공급원(canonical source)이며, 다른 문서와 상충할 경우 이 문서가 우선한다.**

---

## 3. liteLLM 도입 확정 분석

### 3.1 주요 이점 (Pros)
*   **글로벌 메이저 API 완벽 지원**: OpenAI, Anthropic, Google Gemini 등 업계 표준 모델들을 단일 규격으로 상호 전환해 줍니다.
*   **비용 및 예산 제어 (Spend Control)**: 사용자/워커별로 토큰 사용량과 과금 한도(Budget)를 설정할 수 있어 외부 유출 등으로 인한 API 남용을 방지합니다.
*   **로드 밸런싱 & 폴백 (Failover)**: 특정 API 공급자의 일시적 장애 발생 시 자동으로 대체 모델로 우회(Fallback)하도록 구성할 수 있습니다.
*   **관리 대시보드 내장**: 실시간 토큰 사용량과 비용 청구 현황을 직관적인 비주얼 대시보드로 실시간 모니터링할 수 있습니다.

### 3.2 인프라 최적화 방안 (Cons 극복)
*   **기본 단점**: 프로덕션 운영 시 사용자 관리 및 로깅을 위해 추가 데이터베이스(Postgres)가 필요합니다.
*   **해결책**: 신규 DB 서버를 구축하지 않고, 기존 프로젝트 DB인 `fleet-db` 내에 별도 스키마 또는 `litellm` 이름의 데이터베이스를 논리적으로 분리하여 적재함으로써 **추가적인 인프라 부하 및 오버헤드를 극소화**합니다.

---

## 4. 결론
Grok Fleet Orchestrator의 멀티 에이전트 환경 및 비용 추적 요구사항을 충족하기 위한 게이트웨이로 **liteLLM Proxy**를 공식 표준으로 채택하며, 인프라의 복잡도를 낮추기 위해 기존 PostgreSQL 서버만을 활용하는 경량화된 컨테이너 배포 구조로 통합을 진행합니다.
