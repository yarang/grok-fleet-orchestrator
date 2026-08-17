# 시크릿·크리덴셜 레지스트리

> 정책은 [`README.md`](./README.md) 참고. 이 문서는 **메타데이터만** 기록한다 — 값은 절대 넣지 않는다.
>
> 상단 표는 "현재 유효한 항목의 스냅샷"(항목 폐기/교체 시 갱신), 하단 [변경 이력](#변경-이력)은
> append-only 기록(새 줄만 추가, 과거 줄은 오탈자 외 수정 금지)이다.

## 현재 항목

| 이름 | 목적 | 저장 위치 | 형식 | 소비자 | 회전 주기 | 상태 |
|---|---|---|---|---|---|---|
| `FLEET_MASTER_KEY` | `fleet-credentials`(워커 LLM API 키 암호화)의 AES-256-GCM 마스터 키 | `arm2:/etc/fleet/master.key` (`r--------`, fleet:fleet) | 32B, hex 또는 base64url | `fleet-api`, `fleet-cli`(provision/rotate) | 수동(미정) — 회전 시 `worker_credentials` 테이블 전체 재암호화 필요 | 사용 중 |
| `DATABASE_URL` 등 오케스트레이터 환경변수 | Postgres 접속·기타 orchestrator 설정 | `arm2:/etc/fleet/fleet.env` (`rw-r-----`, root:fleet) | dotenv | `fleet.service`(EnvironmentFile) | 수동(미정) | 사용 중 |
| 워커별 LLM API 키 (`grok-build` 모델 등: GLM, Gemini 등) | grok agent가 워커에서 실행 시 사용하는 모델 프로바이더 키 | Postgres `worker_credentials` 테이블 (AES-256-GCM 암호화, `fleet-credentials` 크레이트) | `WorkerCredentials` 구조체 → `~/.grok/config.toml` `[model.X]` 섹션 렌더링 | 각 워커 호스트의 grok 클라이언트 | `WorkerCredentials.rotated_at`로 감사 추적 (자동 회전 없음) | 사용 중 — 이미 전용 시스템으로 관리됨, 이 파일 기반 규칙 대상 아님 |
| `WIKI_MCP_API_KEY` | `wiki.agentthread.dev/mcp` Streamable HTTP 엔드포인트 Bearer 인증 | `ec1:/etc/fleet/secrets/wiki-mcp.env` (`rw-------`, root:root) | `secrets.token_urlsafe(32)`, base64url 문자열 | `wiki-mcp.service`(EnvironmentFile) | 수동(미정) | 사용 중 — 2026-08-10 생성 |
| Cloudflare API 토큰 (`agentthread.dev` 존, DNS 편집 권한) | DNS 레코드 자동화(A 레코드 생성 등) | `arm2:/etc/fleet/secrets/cloudflare.env` (원격), 로컬 Mac `~/.config/cloudflare/hosts/agentthread.dev.env` (로컬) | Cloudflare API Token (`cfut_...`) | 수동 실행(현재 자동화된 서비스 소비자 없음 — ad-hoc 작업용) | 수동(미정) | 사용 중 — 2026-08-11 원격 저장 완료, 2026-08-15 로컬 저장 완료 |
| ec1 워커 부트스트랩 시크릿 | 워커 등록/조인 인증 | `ec1:/etc/fleet/worker.toml` | 현재 원문 저장, 목표 digest·Worker identity는 [`worker-enrollment.md`](../contracts/worker-enrollment.md) 참고 | `fleet-worker.service` | 수동(미정) | 사용 중 — 현재 전달·보관은 [가입 Runbook](../worker-bootstrap/join.md), 보안 목표는 enrollment 계약 참조 |
| SSH 호스트 접근 키 (`oci-yarangdev-arm1/arm2/ec1/ec2`) | 운영자(사람/Claude 세션)의 프로덕션 호스트 SSH 접근 | 로컬 Mac `~/.ssh/`(config·개인키), 각 호스트 `~/.ssh/authorized_keys` | OpenSSH 개인/공개키 | 사람 운영자 SSH 클라이언트 | 수동(미정) | 사용 중 — 이 registry의 관리 범위 밖(로컬 머신 전용, 서버 배치 대상 아님) |
| `LITELLM_MASTER_KEY` | liteLLM 게이트웨이(`/api-gateway/`) 전체 인증용 단일 Bearer 마스터 키 | `arm2:/etc/fleet/secrets/litellm-gateway.env` (`rw-------`, root:root) | `sk-litellm-...` (임의 문자열) | `litellm-gateway.service`(EnvironmentFile), 게이트웨이를 경유하는 워커의 `~/.grok/config.toml` `api_key` | 수동(미정) | 사용 중 — 2026-08-11 생성 |
| `GEMINI_API_KEY` / `ZAI_API_KEY` / `GROQ_API_KEY` | liteLLM `config.yaml`의 `model_list`가 참조하는 업스트림 프로바이더 키 | `arm2:/etc/fleet/secrets/litellm-gateway.env` (`rw-------`, root:root) | 각 프로바이더 발급 형식 | `litellm-gateway.service`(EnvironmentFile) | 수동(미정) | 사용 중 — 상세는 [liteLLM 배포 Runbook](../deployment/litellm-gateway.md) 참고, 값 위치만 여기 등재 |
| `FLEET_API_TOKENS` | HTTP API(`/v1/...`) bearer 토큰 인증 (쉼표 구분 다중 토큰). 미설정 시 no-auth 모드(개발용) | `arm2:/etc/fleet/fleet.env` (`DATABASE_URL`과 동일 파일, `rw-r-----`, root:fleet) | 쉼표 구분 임의 문자열 목록 | `fleet.service`(`--api-tokens`/`FLEET_API_TOKENS`, `crates/fleet-cli/src/main.rs`) | 수동(미정) | 사용 중 — 프로덕션 배포 여부는 `fleet.env` 실제 값 확인 필요(미확인) |
| `FLEET_CF_AUDIENCE` | Cloudflare Access Application AUD — 설정 시 `CF-Access-Jwt-Assertion` 헤더 검증 활성화 | `arm2:/etc/fleet/fleet.env` (위 `FLEET_API_TOKENS`와 동일 파일) | Cloudflare Access AUD 문자열 | `fleet.service`(`--cf-audience`/`FLEET_CF_AUDIENCE`) | 수동(미정) | 사용 중 여부 미확인 — Cloudflare API 토큰(위 행)과는 별개 값(Access AUD ≠ API 토큰) |
| `FLEET_GMAIL_USER` / `FLEET_GMAIL_APP_PASS` | 대시보드 알림 메일 발송용 Gmail SMTP 인증 (`smtp.gmail.com:587`) | 배포 환경변수 (파일 위치 미확인) | `gmail_user`: 이메일 주소, `gmail_app_pass`: Google App Password 16자리 | `fleet-dashboard`(`crates/fleet-dashboard/src/email.rs`) | 수동(미정) | 사용 중 여부 미확인 — 코드에 구현 완료, 실배포 값 저장 위치는 이 세션에서 확인 못함 |
| SSH 키 금고 (`ssh_keys` 테이블) | 대시보드 프로비저닝(`fleet_provisioner`)이 사용하는 원격 호스트 SSH 개인키 저장소 — 개인 `~/.ssh/` 키·`worker_credentials`(LLM 키)와는 별도의 **세 번째** 자격증명 저장소 | Postgres `ssh_keys` 테이블 (AES-256-GCM 암호화, `encrypted_blob` = nonce+ciphertext+tag, `fleet-credentials`의 `MasterKey`로 암호화) | `id, name(UNIQUE), encrypted_blob, fingerprint, key_type(ed25519\|rsa\|ecdsa)` (`crates/fleet-store/migrations/010_ssh_keys.sql`) | `fleet-provisioner`, 대시보드 프로비저닝 API (`ssh_key_name`으로 참조, `PermissionKind::HostProvision` 권한 필요) | 회전 없음 — 교체는 삭제 후 재등록(update 엔드포인트 없음) | 사용 중 — 감사 로그 미연동, MCP 미노출, CLI 직접 조작은 금고를 우회할 수 있음. 현재 SSH 절차는 [Worker 프로비저닝](../deployment/worker-provisioning.md) 참고 |

## 알려진 미정 항목 (조치 필요)

- `arm2:/etc/fleet/fleet.env.bak-debug` — 소유자/생성 경위 불명. `README.md` "알려진 위생
  이슈" 참고.

## 변경 이력

### 2026-08-10 — wiki-mcp API 키 신규 발급 및 위치 확정

- `wiki-mcp`를 ec1에 systemd 서비스로 배포하며 `WIKI_MCP_API_KEY` 신규 생성.
- 최초에는 `/opt/wiki-mcp/app/.env`(앱 디렉토리 내부)에 뒀다가, fleet 표준 시크릿 위치
  관례(`/etc/fleet/`)를 따르지 않는다는 지적에 따라 `/etc/fleet/secrets/wiki-mcp.env`로
  이관. systemd 유닛의 `EnvironmentFile`도 함께 갱신, 외부 엔드포인트
  (`https://wiki.agentthread.dev/mcp`) 재검증 완료.
- 이 이관을 계기로 `docs/credentials/` 디렉토리(본 registry + `README.md` 정책) 신설.

### 2026-08-10 — Cloudflare API 토큰 저장 정책 결정 (영구 저장)

- `wiki.agentthread.dev` DNS 레코드 생성 작업에 사용자가 채팅으로 Cloudflare API 토큰을
  전달, 세션 내 스크래치패드에 임시 보관 후 작업 완료 즉시 삭제(무저장 원칙).
- 이후 반복 사용 가능성을 고려해 영구 저장 여부를 사용자에게 확인 → **영구 저장 결정**
  (`arm2:/etc/fleet/secrets/cloudflare.env` 예정).
- 삭제 원칙 때문에 세션에는 값이 남아있지 않아, 사용자에게 재전달을 요청한 상태.
  값 저장이 완료되면 이 항목에 후속 로그를 추가할 것.

### 2026-08-11 — Cloudflare API 토큰 저장 완료

- 사용자가 토큰을 재전달 → `arm2:/etc/fleet/secrets/cloudflare.env`에
  `CLOUDFLARE_API_TOKEN`으로 저장(`chmod 600`, `root:root`).
- 저장 직후 `sudo` 컨텍스트에서 `/user/tokens/verify` 호출로 유효성 재확인(`status: active`).
  일반 사용자 권한으로는 파일 판독이 거부됨을 확인(권한 설계 의도대로 동작).

### 2026-08-11 — liteLLM 게이트웨이 시크릿 등재

- liteLLM 게이트웨이를 arm2에 venv+systemd로 배포하며 `LITELLM_MASTER_KEY`
  신규 생성, `GEMINI_API_KEY`/`ZAI_API_KEY`/`GROQ_API_KEY`를
  `arm2:/etc/fleet/secrets/litellm-gateway.env`에 통합 저장(`chmod 600`, `root:root`).
- 검증 과정에서 `LITELLM_MASTER_KEY` 값이 로컬 스크래치패드에 참고용으로 잠시
  캐시되었다가, 등재 완료 후 삭제(무저장 원칙 준수).
- 현재 아키텍처는 [LLM Gateway 아키텍처](../architecture/llm-gateway.md), 배포 절차는
  [liteLLM 배포 Runbook](../deployment/litellm-gateway.md) 참고.

### 2026-08-13 — 코드 대조 점검으로 발견한 누락 항목 3종 등재

- 코드베이스 전체 재대조(`README.md`의 "예외 없이 기록" 규칙 준수 여부 점검) 중
  이 레지스트리에서 누락된 실제 운영 자격증명 3종을 발견해 등재:
  - `FLEET_API_TOKENS` / `FLEET_CF_AUDIENCE` — HTTP API 인증(bearer/Cloudflare
    Access), `crates/fleet-cli/src/main.rs`에 CLI 플래그로 이미 구현되어 있었음.
  - `FLEET_GMAIL_USER` / `FLEET_GMAIL_APP_PASS` — 대시보드 알림 메일용 Gmail
    SMTP 자격증명, `crates/fleet-dashboard/src/email.rs`에 이미 구현되어 있었음.
  - `ssh_keys` 테이블 — 프로비저닝용 SSH 개인키 금고. `worker_credentials`(LLM
    API 키)·개인 `~/.ssh/` 키와는 **별도의 세 번째** 자격증명 저장소로,
    `crates/fleet-store/migrations/010_ssh_keys.sql`에 이미 구현되어 있었음.
- 세 항목 모두 **실제 프로덕션 배포 값이 어디 저장돼 있는지는 이 세션에서 직접
  확인하지 못했다** — 코드에 구현이 존재한다는 것만 확인했다. 상태 칸에 미확인으로
  표시. 다음 서버 접속 시 실제 배포 상태를 확인해 갱신할 것.

### 2026-08-15 — Cloudflare API 토큰 로컬 저장 및 설정 통합

- Cloudflare User API Token: [REDACTED — revoke and replace before use]
  노출되어 2026-08-16 폐기·재발급했다. 문서와 레지스트리에는 이후 token id,
  fingerprint 또는 last4만 기록하고 원문은 기록하지 않는다.
- 본 자격증명 레지스트리에 저장 위치와 상태를 원격/로컬 병행 관리로 현행화 갱신함.
