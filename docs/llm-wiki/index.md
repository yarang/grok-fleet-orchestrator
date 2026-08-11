# LLM Wiki — 인덱스 (Index)

> `docs/llm-wiki/`에 존재하는 모든 위키 페이지의 콘텐츠 지향 목록이다. 새 페이지를 추가하거나 기존 페이지의 상태(정본/사본)가 바뀌면 이 표를 함께 갱신하고 [`log.md`](./log.md)에 항목을 남긴다. 운영 규칙(스키마)은 [`README.md`](./README.md) 참고.

## 페이지 목록

| 페이지 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`multi_provider_llm_proxy_analysis.md`](./multi_provider_llm_proxy_analysis.md) | liteLLM vs One API vs 자체 구현(Rust-native) 비교 및 게이트웨이 채택 근거 | 🟢 정본 — 게이트웨이 **선택** | 2026-08-07 |
| [`litellm_integration_plan.md`](./litellm_integration_plan.md) | liteLLM Docker Compose / `litellm-config.yaml` 인프라 연동 상세 스펙 (§3.3 Groq strict-schema 훅 포함) | 🟢 정본 — 인프라 **스펙** | 2026-08-11 |
| [`free_tier_providers_analysis.md`](./free_tier_providers_analysis.md) | Groq/OpenRouter 무료 티어 검증 + 워커별 모델 분리 아키텍처 연동 설계 + 쿼터 관리/UI 통합 체크리스트 | 🟢 정본 — 무료 공급자 **채택 여부** | 2026-08-11 |
| [`ci_monitor_report.md`](../../.gemini/antigravity-cli/brain/d50a2258-c305-47ae-b38f-e60e1f48b093/ci_monitor_report.md) | list_workers 라벨/페이지네이션 결함 해결 커밋 Actions 빌드 모니터링 결과 보고서 | 🔵 사본 — CI **모니터** | 2026-08-07 |
| [`README.md`](./README.md) | 위키의 목적, 정본/사본 규칙, ingest/query/lint 워크플로우 (스키마) | ⚪ 스키마 문서 | 2026-08-07 |
| [`log.md`](./log.md) | ingest/query/lint 작업의 append-only 시간순 기록 | ⚪ 부기 문서 | 2026-08-07 |

## 이 위키를 인용하는 외부 문서 (역참조)

| 문서 | 인용 대상 | 위치 |
|---|---|---|
| [`docs/deployment/single-server.md`](../deployment/single-server.md) | `litellm_integration_plan.md`의 Docker Compose 스펙 (사본) | §2.2, §3 Step 1 |
| [`docs/roadmap.md`](../roadmap/roadmap.md) #34 | `multi_provider_llm_proxy_analysis.md`의 채택 결론 | 항목 34 |
| [`examples/groq-compat/README.md`](../../examples/groq-compat/README.md) | `litellm_integration_plan.md` §3.3의 훅 배경·실측·검증 절차 (구현 정본) | 전체 |

## 고아 페이지 / 미해결 교차참조

_(lint 시 발견되는 항목을 여기 기록한다. 현재 없음 — 마지막 점검: 2026-08-07, [`log.md`](./log.md) 참고.)_
