---
type: wiki
status: canonical
source: "docs/llm-wiki/README.md"
last_verified: "2026-08-15"
---

# LLM 게이트웨이 및 프록시 위키 (LLM Gateway Wiki)

> 최종 업데이트: 2026-08-13.

이 디렉토리는 Grok Fleet Orchestrator에서 멀티 LLM 공급업체(OpenAI, Anthropic, Gemini 등)를 단일 API 규격으로 수용하고 통제하기 위한 **LLM Gateway (LLM Proxy)** 관련 설계 결정을 짓고 유지관리하는 위키다. 매번 원본 논의를 재검색하는 대신, 한 번 정리된 결론을 지속적으로 갱신하며 쌓아 올린다 — [Karpathy의 "LLM Wiki" 패턴](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f)을 따른다.

* 📇 페이지 목록 + 메타데이터: [`index.md`](./index.md)
* 🕓 변경 이력 (ingest/query/lint 기록): [`log.md`](./log.md)

---

## 운영 규칙 (스키마)

### 1. 정본(canonical) / 사본(derived) 구분
각 위키 페이지는 자신이 어떤 주제의 **정본**인지 명시한다 — 게이트웨이 선택은 [`multi_provider_llm_proxy_analysis.md`](./multi_provider_llm_proxy_analysis.md), 인프라 스펙(포트·이미지·환경변수)은 [`litellm_integration_plan.md`](./litellm_integration_plan.md)가 정본이다. 다른 도메인 문서가 같은 내용을 인용할 때는 정본 링크만 둔다. 값이 어긋나면 정본이 우선한다.

### 2. 작업 흐름
* **Ingest**: 새로운 결정·요구사항이 생기면 관련 위키 페이지에 통합한다. 한 소스가 여러 페이지에 영향을 줄 수 있으므로 교차참조를 빠짐없이 갱신한다. 신규/변경 페이지는 `index.md`에 반영하고, `log.md`에 항목을 추가한다.
* **Query**: 질문에 대한 답을 위키에서 찾아 종합한다. 새로운 결론이 나오면 기존 페이지에 통합하거나 새 페이지로 파일링해 지식을 누적한다.
* **Lint**: 정기적으로 모순, 오래된 정보, 고아 페이지, 누락된 교차참조를 점검한다. 발견/조치 내역은 반드시 `log.md`에 기록한다.

### 3. 필수 부기(bookkeeping) 파일
* [`index.md`](./index.md) — 각 페이지의 링크·한줄요약·상태(정본/사본/스키마)·최종개정일을 담은 콘텐츠 지향 카탈로그.
* [`log.md`](./log.md) — append-only. 새 항목은 파일 끝에 추가하며 과거 항목은 수정하지 않는다(오탈자 수정 제외).

---

## 🚀 로드맵 정렬 & 자율 엔진 연동 (설계 구상 — 미구현)

> ⚠️ **정정 (2026-08-13)**: 이 절이 서술하는 `AutonomicEngine` 연동은 **구현된 적이
> 없으며, 그 근거였던 `crates/fleet-scheduler/src/autonomic.rs`는 컴파일 불가 상태로
> 방치되어 있다가 2026-08-13 삭제되었습니다** ([`docs/roadmap/roadmap.md`](../roadmap/roadmap.md)
> #43, [`docs/architecture/overview.md`](../architecture/overview.md)의 "Autonomic
> Self-Healing Engine" 절 참고). 아래 내용은 **미래 설계 구상(design draft)**으로만
> 읽어야 하며, 현재 liteLLM 프록시와 스케줄러 사이에 이런 자동 연동은 존재하지 않습니다.
> 쿼터 소진 시 CircuitBreaker가 물리 워커를 격리하는 문제는 여전히 미해결 상태입니다.

본 위키의 설계는 공식 개발 로드맵 **[34번 마일스톤: liteLLM 중앙 게이트웨이 통합 및 연동]**과 1:1로 매핑되는 것을 목표로 했습니다. 향후 오케스트레이터에 자율 동작 엔진이 재구현된다면, 다음과 같은 연동 설계가 후보입니다:

1. **쿼터 인식 기반 자율 라우팅 (Quota-aware Autonomous Routing)** — 구상 단계:
   - OpenRouter(일 50회)나 Groq(낮은 TPM 한도)와 같은 무료 티어 API 사용 시, 쿼터 소진으로 인해 `429 Too Many Requests`나 `413 Payload Too Large` 에러가 발생할 수 있습니다.
   - (구상) 자율 엔진이 이를 모니터링하여 "워커 하드웨어 장애"와 "API 쿼터 소진"을 분석으로 구별해내고, 쿼터가 고갈된 모델 라벨을 가진 워커 노드들을 스케줄러 선택 후보에서 일시 배제하거나 유료/Fallback 공급자로 트래픽을 자동 우회(Self-Adaptive Routing)시키는 것을 목표로 합니다.
   - 이를 통해, API 쿼터 소진이 불필요하게 `CircuitBreaker`를 동작시켜 정상 작동 가능한 물리 워커 노드를 통째로 격리하는 문제를 사전에 자율 방어하는 것이 최종 목표이나, 아직 착수되지 않았습니다.
2. **중앙 집중식 Fallback 및 장애 제어** — 구상 단계:
   - 특정 LLM 공급자 백엔드가 장애를 일으킬 때, (구상) 자율 엔진이 이를 인지하고 liteLLM 프록시 게이트웨이와 상호 작용하여 사전에 수립된 모델 폴백 경로로 에이전트 요청을 다이내믹하게 스위칭하는 것을 목표로 합니다.
