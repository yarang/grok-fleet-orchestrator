---
type: runbook
authority: canonical
implementation: partial
verification: code-checked
source: "docs/deployment/litellm-gateway.md"
last_verified: "2026-08-23"
last_verified_commit: "working-tree"
owners: ["deployment", "operations"]
---

# liteLLM Gateway 배포 Runbook

이 문서는 liteLLM gateway의 준비, 기동, 검증과 중단 절차를 다룬다. gateway 채택과
요청 경계는 [LLM Gateway 아키텍처](../architecture/llm-gateway.md), TLS·public endpoint는
[Reverse proxy 경계](reverse-proxy.md)가 소유한다.

liteLLM은 아키텍처 정본이 정의한 **gateway dialect 계약**의 기준 구현이다. OpenRouter 같은
OpenAI-compatible 집계 서비스는 이 gateway의 **upstream provider**로 두거나, gateway를 거치지 않는
per-model credential(경로 B)로 직접 지정한다 — 어느 쪽을 고르는지는 아키텍처 정본이 소유한다.

![liteLLM deployment boundary](../assets/diagrams/deployment/litellm-gateway-deployment.mermaid)

## 저장소에서 검증된 자산

| 자산 | 용도 | 현재 제약 |
|---|---|---|
| `docker-compose.yml`의 `litellm` | 로컬 개발 gateway | `4000` 포트는 기본적으로 `127.0.0.1`에만 publish된다(로드맵 `#54`) — 외부에 실제로 노출하려면 이 파일을 그대로 쓰지 말고 reverse proxy(TLS·ACL·rate limit)를 앞에 둔 별도 배포로 전환한다 |
| `examples/litellm-config.yaml` | 모델 매핑·master key·callback 예시 | `master_key`는 파일에 하드코딩하지 않고 `os.environ/LITELLM_MASTER_KEY`로 컨테이너 env var를 참조한다(로드맵 `#54`) — 그래도 예시 model list·DB URL은 production에 그대로 사용 금지 |
| `examples/groq-compat/` | Groq가 거부하는 비표준 message field 제거 | config 위치 기준 module path와 함께 배포 필요 |
| Worker `[llm_proxy]` | agent subprocess의 gateway URL·key 주입 | `worker.toml`의 secret 권한·전달 관리 필요 |

## 시작 전

1. 배포 방식을 고정한다. 저장소가 직접 증명하는 경로는 Docker Compose 로컬 구성이다.
2. 사용할 liteLLM image·package version을 고정하고 변경 전 호환성을 검증한다. `docker-compose.yml`은
   floating `main-latest` 대신 실제 발급된 stable release(`v1.98.0`)에 고정돼 있다(로드맵 `#54`) —
   버전을 올릴 때는 changelog를 확인하고 태그를 명시적으로 바꾼다.
3. `master_key`와 provider key를 제한된 secret 파일 또는 secret manager로 전달한다.
   `docker-compose.yml`은 `LITELLM_MASTER_KEY`를 파일에 하드코딩하지 않고 `${LITELLM_MASTER_KEY:-...}`로
   외부 env var를 우선 참조하도록 이미 되어 있다(로드맵 `#54`) — 로컬 dev용 기본값은 그대로 두되,
   실제 배포 전에는 반드시 셸/secret manager에서 `LITELLM_MASTER_KEY`를 설정해 이 값을 덮어쓴다.
4. `examples/litellm-config.yaml`을 복사해 실제 model list와 secret 참조로 바꾸고 예시 key·DB URL을
   제거한다.
5. Groq model을 사용하면 `examples/groq-compat/` module이 config와 함께 import 가능한지
   확인한다.
6. 외부에 노출하면 liteLLM은 private bind로 제한하고 reverse proxy에서 TLS·ACL·rate limit을
   적용한다. `docker-compose.yml`의 `litellm` 포트 publish는 기본이 `127.0.0.1:4000:4000`이라
   호스트 밖에서는 애초에 닿지 않는다(로드맵 `#54`) — 이 파일을 그대로 배포에 쓰지 말라는 뜻이지,
   별도 배포 구성에서 private bind를 직접 구성해야 하는 책임이 없어지는 것은 아니다.

## 기동과 검증

로컬 개발 구성은 `docker compose up -d litellm`로 기동한다. Production에서 systemd·venv 등
다른 방식을 사용하면 unit, package lock, config 경로·소유권과 rollback artifact를 별도로
관리한다.

`docker-compose.yml`의 `litellm` service는 `/health/readiness`(DB 연결까지 확인, 인증 불필요)를
healthcheck로 등록해 뒀고, `orchestrator`는 `depends_on: litellm: condition: service_healthy`로
이 healthcheck 통과를 기다린 뒤에야 기동한다(로드맵 `#54`) — 이전에는 컨테이너가 뜨기만 하면
바로 시작해, liteLLM이 아직 요청을 받을 준비가 되기 전에 오케스트레이터의 첫 dispatch가
connection-refused로 실패할 수 있었다.

1. gateway health 또는 model 목록 요청을 gateway credential로 확인한다.
2. 등록된 공급자별로 작은 completion을 실행하고 provider 오류를 분리해 기록한다.
3. Groq 경로에서 tool call 후 두 번째 요청이 성공하는지 검증해 sanitizer callback 적용을
   확인한다.
4. 테스트 Worker에 `[llm_proxy]` URL·key를 설정하고 agent subprocess가 gateway를 경유하는지
   확인한다.
5. 인증 없는 요청, 잘못된 key, 알 수 없는 model과 공급자 장애가 fail-closed로 보이는지
   확인한다.

## 변경·중단·복구

- model·callback·provider key 변경 전에 config revision, liteLLM version과 rollback 파일을 기록한다.
- canary Worker로 공급자별 검증을 끝낸 뒤 나머지 Worker에 적용한다.
- gateway 장애 시 Worker 하드웨어를 격리하지 말고 gateway·provider·credential 상태를 먼저
  분리한다.
- rollback은 이전 config·image/package version·callback을 함께 복원한 뒤 공급자별 completion과
  Worker 경유 요청을 재검증한다.

## 관련 정본과 근거

- [LLM Gateway 아키텍처](../architecture/llm-gateway.md)
- [Reverse proxy 경계](reverse-proxy.md)
- [구성과 비밀 관리](configuration.md)
- [Credential registry](../credentials/registry.md)
- [Groq compatibility hook](../../examples/groq-compat/README.md)
