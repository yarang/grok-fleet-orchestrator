# liteLLM 게이트웨이 도입 및 연동 설계 계획서

> 최종 개정: 2026-08-11 (§2~§6 전면 재작성 — MVP를 실제로 arm2에 배포·검증한 결과를
> 반영. 원래의 Docker Compose + Postgres DB-backed 설계는 §7 "폐기된 설계"로 이동).

본 문서는 Grok Fleet Orchestrator에 **liteLLM 프록시 게이트웨이**를 연동하기 위한
아키텍처와 배포 스펙을 정의합니다.

> ⚠️ **정본(canonical source) 표시**: 아래 §3~§5의 `config.yaml` / systemd unit /
> nginx 라우팅 사양이 liteLLM 인프라 정의의 정본이다. [`docs/deployment/single-server.md`](../deployment/single-server.md)
> 등 다른 문서에 등장하는 동일 스펙은 **이 문서를 인용한 사본**이며, 값이 어긋나면
> 이 문서가 우선한다.

---

## 0. 요약 (TL;DR)

- **상태**: arm2에 배포 완료, 프로덕션 운용 중 (2026-08-11부터).
- **배포 방식**: `docker-compose` 아님 — Python venv + systemd (`litellm-gateway.service`).
  §7에 이유를 기록한 원래 Docker 설계에서 **의도적으로 이탈**했다.
- **DB 백엔드 없음**: `master_key` 단일 인증의 stateless 모드. 가상 키별 예산 관리는
  Prisma(Node.js) 의존성이 필요해 이번 스코프 밖으로 미뤘다 (§4.3).
- **노출 경로**: `https://fleet.agentthread.dev/api-gateway/` (nginx reverse proxy,
  내부는 `127.0.0.1:4000`만 바인딩 — 외부에 4000 포트 직접 노출 없음).
- **등록된 모델**: Gemini 2.5 Flash, GLM-5.1(z.ai), Groq 무료 티어 3종. 실측 검증
  완료 (§6).
- **groq-compat 훅**: grok CLI가 assistant 메시지에 붙이는 비표준 필드(`model_id`,
  `model_fingerprint`)를 Groq가 400으로 거부하는 문제를 pre-call 훅으로 해결.
  구현 정본은 [`examples/groq-compat/`](../../examples/groq-compat/) (§4.4).
- **워커 연동**: 현재 `ec1`만 게이트웨이를 경유하도록 전환됨 (canary). `arm1`/`ec2`는
  아직 각 프로바이더에 직결 — 마이그레이션 미완료 (§8).

---

## 1. 도입 목표 (Goal)

1. **멀티 LLM API 단일화**: 워커의 LLM 통신 경로를 liteLLM으로 중앙화하고, 하위 LLM
   API의 키 및 포맷 전환을 프록시가 대행하도록 아키텍처 정비. 이 목표의 배경은
   ["worker들은 orchestrator를 통해서 질의를 진행하지 않는가?"라는 아키텍처 질문](#배경-왜-이-작업을-시작했는가)
   — 실제로 워커는 오케스트레이터를 거치지 않고 각 LLM 프로바이더에 직접 접속하고
   있었다. 그 격차를 메우는 것이 이 게이트웨이의 1차 목적이다.
2. **비용/예산 통제(Spend Control)**: 워커/에이전트별 토큰 사용량과 과금 한도를
   설정해 무분별한 API 오용을 방지한다. — **MVP 스코프 밖** (DB 백엔드 필요, §4.3).
3. **장애 자동 스위칭(Fallback)**: 메인 공급자 장애 시 서브 공급자로 전환. —
   **MVP 스코프 밖** (아직 미설정, §8 로드맵).
4. **무료 티어(Groq) 활용 가능화**: strict-schema 검증 때문에 grok CLI에서 바로
   쓸 수 없던 Groq 무료 모델을, 게이트웨이 레벨의 sanitizer 훅으로 실사용 가능하게
   만든다 (§4.4). 배경은 [`free_tier_providers_analysis.md`](./free_tier_providers_analysis.md).

### 배경: 왜 이 작업을 시작했는가

Fleet 대시보드에 태스크 제출 UI(`/tasks/new`)를 만들면서 "워커가 실제로 어떤 경로로
LLM을 호출하는가"를 재확인한 결과, 워커는 오케스트레이터를 전혀 거치지 않고
`~/.grok/config.toml`에 박힌 API 키로 각 프로바이더(Gemini/GLM/Anthropic 등)에
직접 접속하고 있었다. 이 상태에서는:

- 오케스트레이터가 워커별 LLM 사용량/비용을 볼 수 없다.
- 프로바이더 키가 워커 호스트마다 흩어져 회전·감사가 어렵다 (`fleet-credentials`가
  일부 완화하지만, 실제 요청 경로 자체를 통제하지는 못한다).
- 무료 티어(Groq)처럼 요청 스키마가 까다로운 프로바이더를 워커 각자가 알아서
  처리해야 한다.

liteLLM 게이트웨이를 중앙에 두고 워커의 `base_url`만 게이트웨이로 돌리면 이 세
문제를 한 번에 해소할 수 있다는 것이 이 작업의 출발점이다.

---

## 2. 아키텍처 개요 (실제 배포)

![liteLLM Integration Architecture Diagram](../assets/diagrams/llm-wiki/litellm-integration-architecture.mmd)

핵심 설계 결정 세 가지:

1. **Docker 없음** — Python venv + systemd. §7 참고.
2. **DB 없음** — master_key 단일 인증, stateless. §4.3 참고.
3. **경로 기반 노출** — 별도 서브도메인이나 포트 노출 없이 기존
   `fleet.agentthread.dev` nginx 서버 블록에 `/api-gateway/` 위치만 추가.
   liteLLM 자체는 `127.0.0.1:4000`에만 바인딩되어 외부에서 직접 도달 불가능.

---

## 3. 배포 방식 (venv + systemd)

`/opt/litellm-gateway/`에 배포:

```bash
python3 -m venv /opt/litellm-gateway/.venv
/opt/litellm-gateway/.venv/bin/pip install 'litellm[proxy]' 'fastapi<0.120'
```

`fastapi<0.120` 고정이 필요한 이유: `pip install litellm[proxy]`가 자동으로 끌어오는
최신 fastapi가 `ImportError: cannot import name 'get_flat_dependant' from
fastapi.dependencies.utils`를 유발 (2026-08-11 실측, litellm 1.96.0 기준). 이후
fastapi가 이 심볼을 다시 노출하거나 litellm이 해당 임포트를 제거하면 이 고정을
풀어도 된다 — 업그레이드 전 반드시 `litellm-gateway.service` 기동 확인.

### systemd unit (`/etc/systemd/system/litellm-gateway.service`)

```ini
[Unit]
Description=liteLLM Gateway — centralized LLM provider routing for Fleet workers
After=network.target

[Service]
Type=simple
User=ubuntu
Group=ubuntu
WorkingDirectory=/opt/litellm-gateway
EnvironmentFile=/etc/fleet/secrets/litellm-gateway.env
ExecStart=/opt/litellm-gateway/.venv/bin/litellm --config /opt/litellm-gateway/config.yaml --port 4000 --host 127.0.0.1
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

### nginx 라우팅 (`/etc/nginx/sites-available/fleet`, `fleet.agentthread.dev` 서버 블록)

```nginx
location /api-gateway/ {
    proxy_pass http://127.0.0.1:4000/;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_read_timeout 300s;
}
```

`proxy_read_timeout 300s`는 grok-build의 긴 에이전틱 턴(툴 호출 다수)이 liteLLM
기본 타임아웃보다 오래 걸릴 수 있어 넉넉히 잡았다.

---

## 4. `config.yaml` 상세 (정본)

`/opt/litellm-gateway/config.yaml`:

```yaml
model_list:
  # 기존 워커들이 이미 쓰던 것과 동일한 model 문자열을 그대로 model_name으로 등록 —
  # 워커 config.toml의 model= 필드를 바꾸지 않고 base_url만 게이트웨이로 돌리면 되게 함.
  - model_name: gemini-2.5-flash
    litellm_params:
      model: gemini/gemini-2.5-flash
      api_key: "os.environ/GEMINI_API_KEY"

  - model_name: GLM-5.1
    litellm_params:
      model: openai/GLM-5.1
      api_base: https://api.z.ai/api/coding/paas/v4
      api_key: "os.environ/ZAI_API_KEY"

  # Groq 무료 티어 (신용카드 불필요). TPM 한도가 낮으므로 저우선순위 태스크
  # 전용으로 라우팅할 것 — 한도는 docs/llm-wiki/free_tier_providers_analysis.md 참고.
  # groq-compat 훅(아래 callbacks)이 있어야 grok CLI의 비표준 메시지 필드를
  # 제거해 에이전틱(툴 호출) 턴이 400으로 실패하지 않는다.
  - model_name: groq-free-8b        # TPM 6,000  / TPD 500,000
    litellm_params:
      model: groq/llama-3.1-8b-instant
      api_key: "os.environ/GROQ_API_KEY"
  - model_name: groq-free-oss20b    # TPM 8,000  / TPD 200,000
    litellm_params:
      model: groq/openai/gpt-oss-20b
      api_key: "os.environ/GROQ_API_KEY"
  - model_name: groq-free-70b       # TPM 12,000 / TPD 100,000
    litellm_params:
      model: groq/llama-3.3-70b-versatile
      api_key: "os.environ/GROQ_API_KEY"

general_settings:
  master_key: "os.environ/LITELLM_MASTER_KEY"
  # database_url은 의도적으로 미설정 — DB 백엔드(가상 키/예산 관리)는 Prisma(Node.js
  # 필요)까지 끌고 와야 해서 지금 스코프(워커 직결 문제 해소 + groq-compat) 밖이다.
  # master_key 단일 인증의 stateless 모드로 운영한다. 토큰 사용량 통계/가상 키별
  # 예산 관리가 필요해지면 그때 Node.js + `prisma generate`를 추가하고 이 값을 되살릴 것.

litellm_settings:
  # Groq는 chat/completions 본문을 엄격 검증하므로, grok CLI가 assistant
  # 메시지에 붙이는 비표준 프로퍼티(model_id / model_fingerprint)를 제거해야
  # 한다. drop_params는 top-level 파라미터만 처리하므로 이 훅이 별도로 필요하다.
  # 경로는 이 config.yaml이 있는 디렉토리 기준으로 해석된다.
  callbacks: ["groq_compat.litellm_hook.proxy_handler_instance"]
```

### 4.1 모델 문자열 = 워커 설정 그대로

`model_name`을 워커의 기존 `~/.grok/config.toml` `[model.X]` 이름과 동일하게
유지했다. 워커 마이그레이션은 `base_url`/`api_key`만 바꾸면 되고 `model=` 필드는
그대로 둔다 — 회귀 위험을 최소화하는 선택.

### 4.2 인증

`LITELLM_MASTER_KEY`는 게이트웨이 전체의 단일 Bearer 토큰이다 (가상 키 없음).
워커의 `~/.grok/config.toml`은 이 값을 `api_key`로 그대로 사용한다. 값은
`/etc/fleet/secrets/litellm-gateway.env`에 있으며 [`docs/credentials/registry.md`](../credentials/registry.md)에
등재되어 있다.

### 4.3 DB 백엔드를 넣지 않은 이유

liteLLM의 가상 키/예산/사용량 통계 기능은 `general_settings.database_url`을
설정해야 활성화되는데, 이는 `prisma` Python 패키지 + `prisma generate`(Node.js
런타임 필요)까지 끌고 온다. 이번 작업의 1차 목표는 "워커가 오케스트레이터를 거치지
않고 프로바이더에 직결하는 아키텍처 격차"를 메우는 것이었고, 여기에 Node.js 툴체인을
새로 들이는 비용은 맞지 않다고 판단해 **의도적으로 제외**했다. Postgres에는
`litellm` 논리 DB(`CREATE DATABASE litellm OWNER fleet;`)를 미리 만들어 뒀지만
현재 미사용 — Phase 2에서 예산 관리가 필요해지면 그때 켠다 (§8).

### 4.4 groq-compat 훅

> 실제 파일은 [`examples/groq-compat/`](../../examples/groq-compat/)가 정본,
> 배경·실측·검증 절차는 그 디렉토리의 `README.md` 참고. 프로덕션 배포본은
> `/opt/litellm-gateway/groq_compat/`에 동일 내용을 복사해 둔 것이다 —
> `examples/groq-compat/`를 고치면 **수동으로 arm2에 재배포**해야 한다(자동 동기화
> 없음. 이는 자동화 대상 TODO, §8 참고).

Groq처럼 `chat/completions` **요청 본문을 엄격 검증**하는 공급자는, 스펙에 없는
프로퍼티가 메시지에 하나라도 붙어 있으면 400으로 거부한다. `grok` CLI(grok-build)는
assistant 메시지에 `model_id` / `model_fingerprint`를 붙여 보내므로, **툴 호출이
한 번이라도 발생한 턴은 두 번째 요청부터 전부 실패**한다(= 에이전틱 작업 불가).

**`drop_params: true`로는 해결되지 않는다** — 이 옵션은 top-level 파라미터만
드롭하고 메시지 프로퍼티는 건드리지 않는다 (2026-08-11 실측). liteLLM 자체도 이
필드를 걸러주지 않고 그대로 업스트림에 전달한다. 따라서 pre-call 훅이 별도로
필요하다. liteLLM은 `callbacks`에 지정한 문자열을 **`config.yaml`이 있는 디렉토리
기준**으로 해석해 `<config_dir>/groq_compat/litellm_hook.py`를 로드한다
(`litellm/proxy/types_utils/utils.py::get_instance_fn`).

훅은 OpenAI Chat Completions 스펙이 정의한 프로퍼티만 남기는 **화이트리스트**
방식이다. 공급자가 스펙 밖 필드를 어차피 거부하므로 정보 손실이 발생하지 않으며,
클라이언트가 새 비표준 필드를 추가해도 다시 깨지지 않는다.

---

## 5. 시크릿 관리

`/etc/fleet/secrets/litellm-gateway.env` (root:root, `600`):

| 변수 | 용도 |
|---|---|
| `GEMINI_API_KEY` | Gemini 2.5 Flash 라우팅 |
| `ZAI_API_KEY` | GLM-5.1 (z.ai) 라우팅 |
| `GROQ_API_KEY` | Groq 무료 티어 3종 라우팅 |
| `LITELLM_MASTER_KEY` | 게이트웨이 Bearer 인증 마스터 키 |

`docs/credentials/registry.md`에 항목이 등재되어 있다 — 값 자체는 registry에
넣지 않고 저장 위치·형식·소비자만 기록하는 정책을 따른다.

---

## 6. 검증 결과 (2026-08-11)

arm2 루프백 직접 curl과 `https://fleet.agentthread.dev/api-gateway/` 공인 경로
curl(ec1에서 실행) 양쪽으로 검증:

| 모델 | 결과 | 비고 |
|---|---|---|
| `gemini-2.5-flash` | ✅ 성공 | 루프백/공인 경로 모두 정상 응답 |
| `groq-free-*` | ✅ 성공 | 고의로 `model_id`를 주입한 오염된 요청도 groq-compat 훅이 제거해 200 응답 — 훅이 실제로 작동함을 증명 |
| `GLM-5.1` | ⚠️ 429 (z.ai 계정 레벨 Fair Usage 제한) | 게이트웨이를 우회한 직접 curl에서도 동일 오류 재현 — **게이트웨이 결함 아님**, 기존 z.ai 계정 자체의 사전 제한 |

### 워커 실연동 검증 (canary)

`ec1:~/.grok/config.toml`의 `[model.gemini]`/`[model.grok-build]` `base_url`을
`https://fleet.agentthread.dev/api-gateway`로, `api_key`를 `LITELLM_MASTER_KEY`로
전환 (원본은 타임스탬프 백업 유지). 이후 대시보드 `/tasks/new`에서
`model=canary-ec1` 라벨로 실제 태스크를 제출해 게이트웨이 경유 응답이 정상
완료됨을 확인.

---

## 7. 폐기된 설계 (참고용, 채택하지 않음)

원래 계획은 Docker Compose 기반이었다 (2026-08-07 최초 작성):

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
      - OPENAI_API_KEY=${OPENAI_API_KEY}
      - ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}
      - GEMINI_API_KEY=${GEMINI_API_KEY}
    command: [ "--config", "/app/config.yaml", "--port", "4000", "--detailed_debug" ]
    depends_on:
      db:
        condition: service_healthy
```

이 설계는 다음 이유로 **실제 배포 시점에 채택하지 않았다**:

1. **Docker 도입 비용 대비 이득 없음** — arm2에는 이미 Postgres·nginx·systemd
   기반 운영 관례가 잡혀 있고(기존 `fleet.service` 등 전부 호스트 네이티브),
   liteLLM 하나만을 위해 Docker 런타임을 새로 들이는 것은 이번 MVP 스코프에서
   정당화되지 않는다고 판단했다.
2. **`database_url` 전제가 실제로는 불필요** — 위 설계는 Postgres DB-backed
   가상 키/예산 관리를 처음부터 켜는 것을 전제했지만, §4.3에서 설명한 대로 이
   기능은 Prisma/Node.js 의존성 때문에 MVP에서 제외했다. DB 없이 운영하기로
   결정하면서 Docker의 `depends_on: db healthy` 같은 이점도 자연히 사라졌다.
3. **`examples/litellm-config.yaml`의 모델 목록이 실제 채택 프로바이더와 다름** —
   원 설계는 Claude/GPT-4o를 예시로 들었으나, 실제로 Fleet이 쓰는 프로바이더는
   Gemini/GLM(z.ai)/Groq다. `examples/litellm-config.yaml` 파일 자체는 여전히
   프로젝트에 남아 있지만 **더 이상 배포 정본이 아니며 예시 템플릿일 뿐**이다 —
   실제 정본은 본 문서 §4의 `config.yaml`.

이 결정을 뒤집을 만한 조건(예: 여러 서버에 동일 게이트웨이를 반복 배포해야 하는
경우, 또는 DB-backed 예산 관리를 본격 도입하며 Prisma까지 들이는 경우)이 생기면
Docker Compose 방식을 재검토할 수 있다 — 그 전까지는 venv + systemd가 정본이다.

---

## 8. 남은 작업 (로드맵)

- [ ] `arm1`/`ec2` 워커도 게이트웨이 경유로 전환 (현재 `ec1`만 canary 전환 완료).
- [ ] `examples/groq-compat/` ↔ `/opt/litellm-gateway/groq_compat/` 수동 동기화를
      배포 스크립트로 자동화 (현재 사람이 직접 scp).
- [ ] Phase 2: 사용량 통계/워커별 예산 한도가 필요해지면 `database_url` 활성화 +
      Node.js/Prisma 추가 (§4.3의 보류 조건 충족 시).
- [ ] Phase 2: Fallback 라우팅(메인 공급자 장애 시 서브 공급자 전환) 설정 —
      원 설계 §1 목표 3에는 있었으나 MVP에서 미구현.
- [ ] `litellm` Postgres DB는 이미 생성돼 있으나 미사용 — Phase 2 전까지는 존재
      사실만 기록해 두고 별도 조치 불필요.

---

## 9. 검증 방법 재현 (Runbook)

```bash
# 루프백 직접 (arm2에서)
curl -s http://127.0.0.1:4000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $LITELLM_MASTER_KEY" \
  -d '{"model":"gemini-2.5-flash","messages":[{"role":"user","content":"ping"}]}'

# 공인 경로 (아무 워커에서나)
curl -s https://fleet.agentthread.dev/api-gateway/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $LITELLM_MASTER_KEY" \
  -d '{"model":"groq-free-70b","messages":[{"role":"user","content":"ping"}]}'
```

`journalctl -u litellm-gateway.service -f`로 실시간 로그를 관찰하며 콜백
(`groq_compat`) 로드 여부와 업스트림 오류를 확인한다.
