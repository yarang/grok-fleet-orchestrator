# 멀티 LLM API 공급자 지원을 위한 프록시(liteLLM) 도입 보고서

> 최종 개정: 2026-08-13 (배포 방식 정정 — §3.2/§4 참고).

본 문서는 Grok Fleet Orchestrator의 LLM 호출 경로를 통일하고, 비용 통제 및 가용성 향상을 위해 **LLM Gateway**로서 **liteLLM**을 채택하고 정립하는 기술적 분석 보고서입니다.

> ⚠️ **정정 (2026-08-13)**: 이 문서는 liteLLM **채택 여부**(정본)에 대해서는 여전히
> 유효하지만, §3.2/§4의 **배포 방식(Postgres DB-backed + 컨테이너)** 서술은
> 이후 [`litellm_integration_plan.md`](./litellm_integration_plan.md)(정본)에서
> **DB 없는 Python venv + systemd 방식으로 뒤집혔습니다** (§1의 "정본/사본 구분"
> 규칙에 따라 인프라 스펙은 그 문서가 우선합니다). 아래 §3.2/§4는 갱신 당시 최초
> 결정을 보존한 것이며, 실제 배포 방식은 `litellm_integration_plan.md` §2~§4를
> 참고하세요.

---

## 1. 아키텍처 내 LLM Proxy의 필요성

워커 노드와 오케스트레이터의 LLM 통신 경로에 **LLM Proxy(liteLLM)**가 통합되면 다음과 같은 구조로 일원화됩니다.

![Multi-Provider LLM Proxy Diagram](../assets/diagrams/llm-wiki/multi-provider-llm-proxy.mermaid)

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

> 과거 단일 서버 배포 초안에는 반대로 "무겁고 복잡한 liteLLM 대신 One API"로 기술되어 있었다. 해당 폐기 문서는 삭제했다. **이 문서(`multi_provider_llm_proxy_analysis.md`)가 게이트웨이 선택에 대한 단일 진실 공급원(canonical source)이며, 다른 문서와 상충할 경우 이 문서가 우선한다.**

---

## 3. liteLLM 도입 확정 분석

### 3.1 주요 이점 (Pros)
*   **글로벌 메이저 API 완벽 지원**: OpenAI, Anthropic, Google Gemini 등 업계 표준 모델들을 단일 규격으로 상호 전환해 줍니다.
*   **비용 및 예산 제어 (Spend Control)**: 사용자/워커별로 토큰 사용량과 과금 한도(Budget)를 설정할 수 있어 외부 유출 등으로 인한 API 남용을 방지합니다.
*   **로드 밸런싱 & 폴백 (Failover)**: 특정 API 공급자의 일시적 장애 발생 시 자동으로 대체 모델로 우회(Fallback)하도록 구성할 수 있습니다.
*   **관리 대시보드 내장**: 실시간 토큰 사용량과 비용 청구 현황을 직관적인 비주얼 대시보드로 실시간 모니터링할 수 있습니다.

### 3.2 인프라 최적화 방안 (Cons 극복) — ⚠️ 아래는 초기 결정, 이후 뒤집힘

*   **기본 단점**: 프로덕션 운영 시 사용자 관리 및 로깅을 위해 추가 데이터베이스(Postgres)가 필요합니다.
*   **초기 해결책 (폐기됨)**: 신규 DB 서버를 구축하지 않고, 기존 프로젝트 DB인 `fleet-db` 내에 별도 스키마 또는 `litellm` 이름의 데이터베이스를 논리적으로 분리하여 적재함으로써 추가적인 인프라 부하 및 오버헤드를 극소화하는 방안을 검토했습니다.
*   **✅ 실제 채택된 해결책**: `litellm_integration_plan.md`에서 재검토한 결과, 가상 키/예산/사용량 통계 기능(DB 필요)은 MVP 스코프 밖으로 제외하기로 결정하여 **DB 자체를 두지 않는 stateless `master_key` 단일 인증** 방식을 채택했습니다. `litellm` 논리 DB는 미리 만들어 두었으나 Phase 2(예산 관리 도입) 전까지는 미사용 상태입니다.

---

## 4. 결론

Grok Fleet Orchestrator의 멀티 에이전트 환경 및 비용 추적 요구사항을 충족하기 위한 게이트웨이로 **liteLLM Proxy**를 공식 표준으로 채택합니다.

> ⚠️ 원래 결론이었던 "PostgreSQL 서버를 활용하는 경량화된 컨테이너 배포 구조"는
> 실제로는 채택되지 않았습니다. 실제 배포 구조는 **DB 없이 Python venv + systemd**로
> 구성되어 있습니다 — 상세는 [`litellm_integration_plan.md`](./litellm_integration_plan.md)
> (정본) 참고.
