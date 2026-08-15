# LLM Wiki — 인덱스 (Index)

> `docs/llm-wiki/`에 존재하는 모든 위키 페이지의 콘텐츠 지향 목록이다. 새 페이지를 추가하거나 기존 페이지의 상태(정본/사본)가 바뀌면 이 표를 함께 갱신하고 [`log.md`](./log.md)에 항목을 남긴다. 운영 규칙(스키마)은 [`README.md`](./README.md) 참고.

## 페이지 목록

| 페이지 | 한 줄 요약 | 상태 | 최종 개정 |
|---|---|---|---|
| [`multi_provider_llm_proxy_analysis.md`](./multi_provider_llm_proxy_analysis.md) | liteLLM vs One API vs 자체 구현(Rust-native) 비교 및 게이트웨이 채택 근거 (배포 방식 서술 §3.2/§4는 이후 뒤집힘 — 정정 배너 참고) | 🟢 정본 — 게이트웨이 **선택** | 2026-08-13 |
| [`litellm_integration_plan.md`](./litellm_integration_plan.md) | liteLLM 게이트웨이 실제 배포 스펙 (venv+systemd, arm2 프로덕션 운용 중) — DB-less, groq-compat 훅, 워커 canary 전환 현황. 원래의 Docker Compose 설계는 §7에서 "로컬 개발용으로 병행 사용 중"으로 재분류 | 🟢 정본 — 인프라 **스펙** (⚠️ nginx timeout 값이 `nginx-gateway.md`와 불일치, 미확인) | 2026-08-13 |
| [`free_tier_providers_analysis.md`](./free_tier_providers_analysis.md) | Groq/OpenRouter 무료 티어 검증 + 워커별 모델 분리 아키텍처 연동 설계 + 쿼터 관리/UI 통합 체크리스트 | 🟡 정본 (⚠️ §1.3/§1.4/§5 부분 노후 — `log.md` 2026-08-11 항목 참고) — 무료 공급자 **채택 여부** | 2026-08-11 |
| [`ci_monitor_report.md`](./ci_monitor_report.md) | list_workers 라벨/페이지네이션 결함 해결 커밋 Actions 빌드 모니터링 결과 보고서 | 🔵 사본 — CI **모니터** | 2026-08-07 |
| [`README.md`](./README.md) | 위키의 목적, 정본/사본 규칙 명시 (스키마). 자율 엔진(AutonomicEngine) 연동 절은 엔진 자체가 삭제되어 "미구현 설계 구상"으로 재분류됨 | ⚪ 스키마 문서 | 2026-08-13 |
| [`log.md`](./log.md) | ingest/query/lint 작업의 append-only 시간순 기록 | ⚪ 부기 문서 | 2026-08-07 |

## 이 위키를 인용하는 외부 문서 (역참조)

| 문서 | 인용 대상 | 위치 |
|---|---|---|
| [`docs/deployment/single-server.md`](../deployment/single-server.md) | `litellm_integration_plan.md`의 venv+systemd 배포 스펙 인용 (§2.1) + 폐기된 Docker Compose 설계 명시(§2.2, §3 Step 1) | §2.1, §2.2, §3 Step 1 |
| [`docs/roadmap.md`](../roadmap/roadmap.md) #34 | `multi_provider_llm_proxy_analysis.md`의 채택 결론 | 항목 34 |
| [`examples/groq-compat/README.md`](../../examples/groq-compat/README.md) | `litellm_integration_plan.md` §3.3의 훅 배경·실측·검증 절차 (구현 정본) | 전체 |
| [`docs/architecture/overview.md`](../architecture/overview.md) | `README.md`가 자율 동작 제어 엔진 절을 인용 (엔진은 2026-08-13 삭제되어 양쪽 모두 "미구현 설계 구상"으로 정정됨) | ## Autonomic Self-Healing Engine (Autonomy) — 🔴 미구현·비연결 상태 |

## 고아 페이지 / 미해결 교차참조

_(lint 시 발견되는 항목을 여기 기록한다. 마지막 점검: 2026-08-13, [`log.md`](./log.md) 참고. 이번 점검에서 새 고아 페이지는 없었으나, `free_tier_providers_analysis.md`의 부분 노후(§1.3/§1.4/§5)와 `litellm_integration_plan.md`↔`nginx-gateway.md`의 nginx timeout 불일치를 발견 — 위 표와 각 문서의 정정 배너 참고.)_
