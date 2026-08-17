---
type: architecture-decision
authority: canonical
implementation: partial
verification: code-checked
source: "docs/architecture/llm-gateway.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["architecture", "deployment"]
---

# LLM Gateway 아키텍처

Fleet는 Worker의 LLM 호출을 선택적으로 중앙 liteLLM gateway에 라우팅한다. 이 문서는
게이트웨이 선택과 요청 경계만 소유한다. 설치·구성·복구는
[liteLLM 배포 Runbook](../deployment/litellm-gateway.md)이 소유한다. 과거 분석·배포 원문은 Git
이력에서 복원한다.

![LLM gateway routing](../assets/diagrams/architecture/llm-gateway-routing.mermaid)

## 현재 결정

- 멀티 공급자 변환과 모델 매핑은 자체 Rust proxy 대신 liteLLM이 담당한다.
- Worker의 `[llm_proxy]` 설정은 `gateway_url`과 `api_key`를 선택적으로 받는다.
- `fleet-worker`는 grok·agy 하위 프로세스에 provider-compatible base URL과 gateway credential
  환경변수를 주입한다.
- Worker 요청은 Orchestrator API를 경유하지 않고 liteLLM으로 직접 전송된다.
- `fleet serve`의 `FLEET_LLM_GATEWAY_URL`은 선택 설정으로 URL 형식만 검증한다. 요청 proxy나
  gateway health 보장을 의미하지 않는다.

## liteLLM 선택 근거

Fleet에는 OpenAI-compatible 요청 변환, 다중 provider 모델 매핑과 Groq 같은 provider별
호환 어댑터가 필요하다. liteLLM은 이 범위를 외부 gateway로 제공하므로 Fleet에
독자 proxy를 구현하는 대안보다 선호한다. 단, 현재 선택은 liteLLM의 모든 기능을
채택했다는 뜻이 아니며 budget·usage 집계·fallback은 별도 완료 계약이 필요하다.

## 현재 구현과 미구현 경계

| 능력 | 상태 | 근거 또는 완료 조건 |
|---|---|---|
| Worker의 gateway 환경변수 주입 | 구현 | `fleet-worker/src/config.rs`, `grok_process.rs` |
| liteLLM 로컬 구성과 Groq request sanitizer | 구현 자산 있음 | `docker-compose.yml`, `examples/litellm-config.yaml`, `examples/groq-compat/` |
| 중앙 provider credential 관리 | 부분 구현 | gateway secret 전달·rotation·revoke Runbook 검증 필요 |
| Worker·Agent별 budget·usage 집계 | 미구현 | DB-backed 계정·예산 정책과 Fleet telemetry 계약 필요 |
| 공급자 장애·quota 기반 fallback | 미구현 | quota와 Worker health를 분리하는 상태·routing 계약 필요 |

## 불변식

- gateway credential과 provider key를 문서, Worker label, URL query에 기록하지 않는다.
- gateway 장애를 Worker 하드웨어 고장으로 단정하지 않는다.
- budget·fallback을 설정하지 않은 단일 `master_key` 구성을 비용 통제가 구현된 것으로
  표시하지 않는다.

## 관련 정본과 근거

- [liteLLM 배포 Runbook](../deployment/litellm-gateway.md)
- [Credential registry](../credentials/registry.md)
- [Groq compatibility hook](../../examples/groq-compat/README.md)
