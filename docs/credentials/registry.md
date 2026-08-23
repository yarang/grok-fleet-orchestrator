# 시크릿·크리덴셜 레지스트리

> 정책은 [`README.md`](./README.md) 참고. 이 문서는 **메타데이터만** 기록한다 — 값은 절대 넣지 않는다.
>
> 상단 표는 "현재 유효한 항목의 스냅샷"(항목 폐기/교체 시 갱신), 하단 [변경 이력](#변경-이력)은
> append-only 기록(새 줄만 추가, 과거 줄은 오탈자 외 수정 금지)이다.

## 현재 항목

| 이름 | 목적 | 저장 위치 | 형식 | 소비자 | 회전 주기 | 상태 |
|---|---|---|---|---|---|---|
| `FLEET_MASTER_KEY` | `fleet-credentials`(워커 LLM API 키 암호화)의 AES-256-GCM 마스터 키 | `arm1:/etc/fleet/master.key` (`r--------`, fleet:fleet) | 32B, hex 또는 base64url | `fleet-api`, `fleet-cli`(provision/rotate) | 수동(미정) — 회전 시 `worker_credentials` 테이블 전체 재암호화 필요 | 사용 중 |
| `DATABASE_URL` 등 오케스트레이터 환경변수 | Postgres 접속·기타 orchestrator 설정 | `arm1:/etc/fleet/fleet.env` (`rw-r-----`, root:fleet) | dotenv | `fleet.service`(EnvironmentFile) | 수동(미정) | 사용 중 |
| 워커별 LLM API 키 (`grok-build` 모델 등: GLM, Gemini 등) | grok agent가 워커에서 실행 시 사용하는 모델 프로바이더 키 | Postgres `worker_credentials` 테이블 (AES-256-GCM 암호화, `fleet-credentials` 크레이트) | `WorkerCredentials` 구조체 → `~/.grok/config.toml` `[model.X]` 섹션 렌더링 | 각 워커 호스트의 grok 클라이언트 | `WorkerCredentials.rotated_at`로 감사 추적 (자동 회전 없음) | 사용 중 — 이미 전용 시스템으로 관리됨, 이 파일 기반 규칙 대상 아님 |
| `WIKI_MCP_API_KEY` | `wiki.agentthread.dev/mcp` Streamable HTTP 엔드포인트 Bearer 인증 | `ec1:/etc/fleet/secrets/wiki-mcp.env` (`rw-------`, root:root) | `secrets.token_urlsafe(32)`, base64url 문자열 | `wiki-mcp.service`(EnvironmentFile) | 수동(미정) | 사용 중 — 2026-08-10 생성 |
| Cloudflare API 토큰 (`agentthread.dev` 존, DNS 편집 권한) | DNS 레코드 자동화(A 레코드 생성 등) | `arm1:/etc/fleet/secrets/cloudflare.env` (원격), 로컬 Mac `~/.config/cloudflare/hosts/agentthread.dev.env` (로컬) | Cloudflare API Token (`cfut_...`) | 수동 실행(현재 자동화된 서비스 소비자 없음 — ad-hoc 작업용) | 수동(미정) | 사용 중 — 2026-08-11 원격 저장 완료, 2026-08-15 로컬 저장 완료 |
| ec1 워커 부트스트랩 시크릿 | 워커 등록/조인 인증 | `ec1:/etc/fleet/worker.toml` (worker의 일회성 입력) | Fleet 저장소는 SHA-256 digest만 저장하며 발급 응답에서만 원문을 1회 표시. Worker identity 전환은 [`worker-enrollment.md`](../contracts/worker-enrollment.md) 참고 | `fleet-worker.service` | 수동(미정) | 사용 중 — 기존 DB token은 migration에서 digest로 치환. 기존 worker 설정의 원문은 join 뒤 제거해야 함 |
| SSH 호스트 접근 키 (`oci-yarangdev-arm1/ec1/ec2` — arm2는 2026-08-20 terminate로 폐기) | 운영자(사람/Claude 세션)의 프로덕션 호스트 SSH 접근 | 로컬 Mac `~/.ssh/`(config·개인키), 각 호스트 `~/.ssh/authorized_keys` | OpenSSH 개인/공개키 | 사람 운영자 SSH 클라이언트 | 수동(미정) | 사용 중 — 이 registry의 관리 범위 밖(로컬 머신 전용, 서버 배치 대상 아님) |
| `LITELLM_MASTER_KEY` | liteLLM 게이트웨이(`/api-gateway/`) 전체 인증용 단일 Bearer 마스터 키 | `arm1:/etc/fleet/secrets/litellm-gateway.env` (`rw-------`, root:root) | `sk-litellm-...` (임의 문자열) | `litellm-gateway.service`(EnvironmentFile), 게이트웨이를 경유하는 워커의 `~/.grok/config.toml` `api_key` | 수동(미정) | 사용 중 — 2026-08-11 생성 |
| `GEMINI_API_KEY` / `ZAI_API_KEY` / `GROQ_API_KEY` | liteLLM `config.yaml`의 `model_list`가 참조하는 업스트림 프로바이더 키 | `arm1:/etc/fleet/secrets/litellm-gateway.env` (`rw-------`, root:root) | 각 프로바이더 발급 형식 | `litellm-gateway.service`(EnvironmentFile) | 수동(미정) | 사용 중 — 상세는 [liteLLM 배포 Runbook](../deployment/litellm-gateway.md) 참고, 값 위치만 여기 등재 |
| `FLEET_API_TOKENS` | HTTP API(`/v1/...`) scoped bearer credential manifest | `arm1:/etc/fleet/fleet.env` (`DATABASE_URL`과 동일 파일, `rw-r-----`, root:fleet) | JSON 배열: `principal_id`, `token`, `capabilities` | `fleet.service`(`--api-tokens`/`FLEET_API_TOKENS`) | 수동(미정) | 평면 쉼표 token 목록은 거부. capability 최소화, principal별 분리 필요 |
| `FLEET_CF_AUDIENCE` | Cloudflare Access Application AUD — 설정 시 `CF-Access-Jwt-Assertion` 헤더 검증 활성화 | `arm1:/etc/fleet/fleet.env` (위 `FLEET_API_TOKENS`와 동일 파일) | Cloudflare Access AUD 문자열 | `fleet.service`(`--cf-audience`/`FLEET_CF_AUDIENCE`) | 수동(미정) | 사용 중 여부 미확인 — Cloudflare API 토큰(위 행)과는 별개 값(Access AUD ≠ API 토큰) |
| `FLEET_CF_PRINCIPAL_CAPABILITIES` | CF Access principal(JWT `email` 클레임)별 capability 매핑 (로드맵 `#74`) | `arm1:/etc/fleet/fleet.env` (위 `FLEET_API_TOKENS`와 동일 파일) | JSON 배열: `email`, `capabilities` | `fleet.service`(`--cf-principal-capabilities`/`FLEET_CF_PRINCIPAL_CAPABILITIES`) | 수동(미정) | `FLEET_CF_AUDIENCE`가 설정됐는데 이 값이 비어 있으면 기동 자체가 거부된다 — 매핑 없이는 CF Access를 통과한 누구도 아무 capability를 갖지 못한다(fail-closed, 과거의 전체 capability 부여 결함을 대체) |
| `FLEET_MCP_CAPABILITIES` | MCP stdio launcher의 명시적 도구 capability allow-list | MCP를 실행하는 service/launcher의 environment | 쉼표 구분 `task:read,task:create` 등 | `fleet serve`의 MCP stdio | launcher 배포 시 갱신 | 필수 — 미설정·빈 값·알 수 없는 값이면 MCP가 fail-closed. OS process identity/signed assertion을 대체하지 않음 |
| `FLEET_GMAIL_USER` / `FLEET_GMAIL_APP_PASS` | 대시보드 알림 메일 발송용 Gmail SMTP 인증 (`smtp.gmail.com:587`) | 배포 환경변수 (파일 위치 미확인) | `gmail_user`: 이메일 주소, `gmail_app_pass`: Google App Password 16자리 | `fleet-dashboard`(`crates/fleet-dashboard/src/email.rs`) | 수동(미정) | 사용 중 여부 미확인 — 코드에 구현 완료, 실배포 값 저장 위치는 이 세션에서 확인 못함 |
| SSH 키 금고 (`ssh_keys` 테이블) | 대시보드 프로비저닝(`fleet_provisioner`)이 사용하는 원격 호스트 SSH 개인키 저장소 — 개인 `~/.ssh/` 키·`worker_credentials`(LLM 키)와는 별도의 **세 번째** 자격증명 저장소 | Postgres `ssh_keys` 테이블 (AES-256-GCM 암호화, `encrypted_blob` = nonce+ciphertext+tag, `fleet-credentials`의 `MasterKey`로 암호화) | `id, name(UNIQUE), encrypted_blob, fingerprint, key_type(ed25519\|rsa\|ecdsa)` (`crates/fleet-store/migrations/010_ssh_keys.sql`) | `fleet-provisioner`, 대시보드 프로비저닝 API (`ssh_key_name`으로 참조, `PermissionKind::HostProvision` 권한 필요) | 회전 없음 — 교체는 삭제 후 재등록(update 엔드포인트 없음) | 사용 중 — 감사 로그 미연동, MCP 미노출, CLI 직접 조작은 금고를 우회할 수 있음. 현재 SSH 절차는 [Worker 프로비저닝](../deployment/worker-provisioning.md) 참고 |
| `fcoinfup-arm1` Cold Standby — `FLEET_MASTER_KEY` | 위와 동일 목적. **arm1의 값과는 완전히 별개로 새로 생성** — 복제/공유 없음 | `oci-fcoinfup-arm1:/etc/fleet/master.key` (`r--------`, fleet:fleet) | 32B hex | `fleet.service`(이 호스트) | 수동(미정) | 사용 중 — 2026-08-21 Cold Standby 신규 설치, 같은 날 Docker 빌드 설치를 삭제하고 GitHub clone + 네이티브 `cargo build`로 재설치하며 값 재생성(이전 값 폐기) |
| `fcoinfup-arm1` Cold Standby — `DATABASE_URL`/`FLEET_API_TOKENS`/`FLEET_MCP_CAPABILITIES` | Postgres 접속(로컬 `fleet` role, 네이티브 설치, arm1과 별개 인스턴스)·admin bearer 토큰(단일 `root` principal, 전체 capability)·MCP capability allow-list(`task:read,task:list,worker:list,dashboard:view,metrics:view`로 최소 설정) | `oci-fcoinfup-arm1:/etc/fleet/fleet.env` (`rw-------`, fleet:fleet) | dotenv | `fleet.service`(EnvironmentFile) | 수동(미정) | 사용 중 — 2026-08-21 생성, 같은 날 재설치로 값 재생성(이전 값 폐기). **DNS·외부 공개 없음** — `FLEET_HTTP_BIND`/`FLEET_DASHBOARD_BIND`를 `127.0.0.1`로 고정해 SSH 터널 없이는 접근 불가. 승격 전까지 admin 토큰은 root 하나뿐이라 최소 권한 분리가 안 돼 있음 — 실제 승격 시 principal별 분리 필요 |

## 알려진 미정 항목 (조치 필요)

- ~~`arm2:/etc/fleet/fleet.env.bak-debug` — 소유자/생성 경위 불명~~ — 2026-08-20 arm2
  terminate로 호스트 자체가 사라져 해소(파일도 함께 소멸). 상세는 아래 변경 이력 참고.
- ~~`FLEET_API_TOKENS`~~ — 2026-08-20 이전(migration) 작업 중 세션 로그에 평문 노출됨
  (마스킹 정규식 누락). 2026-08-21 primary 재배포 과정에서 완전히 새 값(JSON 형식)으로
  교체하며 해소. 상세는 아래 변경 이력 참고.
- `FLEET_GMAIL_APP_PASS` — 2026-08-20 같은 사고로 세션 로그에 평문 노출됨. **회전 필요**
  — 아직 미완료.

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

### 2026-08-20 — arm2 예고 없는 terminate → orchestrator arm1 이전, 시크릿 일괄 재배치

- 사용자가 OCI 콘솔에서 `oci-yarangdev-arm2` 인스턴스를 **terminate**(복구 불가)하면서
  `fleet.agentthread.dev` 실서비스가 중단됨. DNS는 여전히 옛 IP를 가리키고 있어 즉시
  장애로 이어졌다.
- 복구 절차로 orchestrator 전체(`fleet.service`, `litellm-gateway.service`, Postgres
  `fleet`/`litellm` DB, nginx+TLS)를 `oci-yarangdev-arm1`으로 이전:
  - `FLEET_MASTER_KEY`(`master.key`), `fleet.env`, `secrets/cloudflare.env`,
    `secrets/litellm-gateway.env` — arm2에서 확보해둔 사본을 그대로 arm1의 동일 경로·
    동일 권한(`fleet:fleet`/`root:fleet`/`root:root`)으로 재배치. 값 자체는 변경 없음.
  - `DATABASE_URL`의 Postgres 비밀번호(`fleet`, `litellm` role)만 **새로 발급**
    (arm1 신규 설치이므로 재사용하지 않음) — 레지스트리 표의 저장 위치만 arm1로 갱신,
    비밀번호 값은 세션에 노출하지 않음.
  - Cloudflare API 토큰(로컬 Mac 사본, `~/.config/cloudflare/hosts/agentthread.dev.env`)
    으로 `fleet.agentthread.dev` A 레코드를 arm1 IP로 직접 갱신(Cloudflare API 호출,
    Claude 세션이 수행). 값은 노출하지 않음, 토큰 자체는 회전하지 않았음.
- **위생 사고**: 이전 작업 중 `fleet.env`를 마스킹해 출력하려다 정규식이
  `DATABASE_URL`(URL 내장 비밀번호), `FLEET_API_TOKENS`(`TOKEN=` 패턴 불일치),
  `FLEET_GMAIL_APP_PASS`(`PASS`≠`PASSWORD`)를 걸러내지 못해 **세 값이 세션 로그에
  평문 노출**됨. `DATABASE_URL`의 비밀번호는 위 이전 과정에서 신규 발급으로 이미
  무효화됐으나, `FLEET_API_TOKENS`와 `FLEET_GMAIL_APP_PASS`는 **아직 회전하지 않은
  채 그대로 사용 중** — 위 "알려진 미정 항목" 참고, 조속히 회전 필요.
- **미이관 항목**: `puwu`(삭제 예정 프로젝트, arm2 terminate와 함께 소멸 확정),
  `wiki.agentthread.dev`(ec1의 `wiki-mcp`를 arm2 경유로 리버스 프록시하던 구조 —
  arm2 소멸로 함께 다운, 이 세션에서 미복구), worker `ec1`/`ec2`의 ACP 리버스 SSH
  터널(구 도착지 arm2 → arm1로 재설정 필요, 이 세션에서 미착수). 모두 후속 조치
  필요.

### 2026-08-21 — `fcoinfup-arm1`에 Cold Standby orchestrator 신규 설치

- 사용자가 `oci-yarang-arm1`(primary) 장애/미확인 상태와 무관하게 이중화를 위해
  두 번째 프로덕션급 인스턴스 설치를 요청. `control-plane-authority-and-failover.md`의
  Single Active Primary + Cold Standby 모델을 따라, DNS 전환이나 외부 공개 없이
  **Cold Standby**로 구축했다(primary 생사를 세션이 임의 판단해 트래픽을 돌리지 않음).
- 절차: 로컬 저장소 소스를 rsync로 전송 → 호스트에서 Docker 멀티스테이지 빌드로
  `fleet` 바이너리 추출(호스트에 Rust 툴체인 설치 없음) → PostgreSQL 16 네이티브 설치,
  `fleet` role/DB를 호스트에서 직접 생성(비밀번호는 원격 호스트 안에서만
  `openssl rand`로 생성해 세션에 노출된 적 없음) → `/etc/fleet/master.key`,
  `/etc/fleet/fleet.env`(`FLEET_API_TOKENS` 포함, admin 토큰도 원격에서만 생성) →
  systemd `fleet.service`.
- **배선 이슈 발견·수정**: `fleet serve`가 MCP stdio 컴포넌트를 프로세스 마지막
  blocking call로 실행하는 구조라, systemd 기본 `StandardInput=null`(즉시 EOF)에서는
  HTTP/dashboard가 뜨자마자 전체 프로세스가 종료됨. `ExecStart`를
  `tail -f /dev/null | fleet serve`로 감싸 절대 EOF 없는 stdin을 공급해 해결(표준
  systemd stdio 데몬화 우회). 이건 이 프로젝트의 systemd 배포 문서에 없던 함정이라
  `docs/deployment/install.md` 또는 `operations.md`에 후속으로 반영 필요.
- 검증: `fleet migrate`(마이그레이션 20개 적용) → `fleet doctor`(5개 항목 전부 OK) →
  `curl 127.0.0.1:8081/v1/health` 200 → admin 토큰 없이 `/v1/workers` 401, 토큰과 함께
  200 → `ss -tlnp`로 `8081`/`8082` 둘 다 `127.0.0.1` 바인딩만 확인(외부 노출 없음) →
  `fleet.service`/`postgresql.service` 둘 다 `enabled`(재부팅 생존).
- **미해결**: `apt` 업그레이드가 커널 재부팅을 권고했으나, 이 호스트가 MariaDB/Redis/
  AgentForge 등 다른 서비스를 공유하는 호스트라 세션이 임의로 재부팅을 하지 않았다 —
  운영자 확인 필요. admin 토큰이 `root` principal 하나에 전체 capability를 몰아준
  상태라 승격 전 principal별 최소 권한 분리가 필요하다. liteLLM 게이트웨이·nginx·TLS·
  Cloudflare DNS는 이번 설치 범위 밖(승격 시점에 별도 결정).

### 2026-08-21 — Docker 설치 삭제 → GitHub clone + 네이티브 build로 재설치, AgentForge 제거

- 사용자 요청으로 위 Docker 기반 설치를 전부 삭제(systemd 유닛, 바이너리, `/etc/fleet`,
  Postgres role/db, Docker 이미지·빌드소스)한 뒤, GitHub(`origin`, public repo)에서
  `git clone` + `rustup`(stable, 1.98.0) 네이티브 `cargo build --release --features
  "acp mtls"`로 재설치했다. **`origin`이 그동안 `8a2bc29`(오래전 커밋)에 멈춰 있어
  재설치 전 로컬 main(당시 `d9ffa33`, 오늘 세션의 #57~#72 등 19개 커밋)을 먼저
  `git push origin main`으로 fast-forward 반영**(gitea는 9월까지 다운이라 origin만
  갱신). Postgres 비밀번호·`master.key`·admin bearer 토큰은 재설치 시점에 호스트에서
  전부 새로 생성했다(기존 Docker 설치분 값은 이미 삭제되어 폐기).
- Docker 빌드 잔여물(이미지 3.5GB + 빌드 캐시 3.9GB, 전부 이 세션이 만든 것이고
  컨테이너·다른 이미지 참조 0건 확인 후) `docker system prune -af --volumes`로 정리.
  Docker 엔진 자체(`docker.service`/`containerd.service`)는 이 호스트의 다른 서비스가
  이미 쓰던 사전 설치 인프라라 제거하지 않았다.
- 사용자 요청으로 이 호스트의 **AgentForge 플랫폼도 완전히 제거**했다:
  `agentforge-daemon.service`(2026-08-01부터 구동, `oci-yarang-arm1`의 죽은 NATS
  broker에 의존해 실제로는 연결 끊김 에러만 계속 내던 상태), 전용
  `tunnel-nats.service`(AutoSSH, 다른 소비자 없음 확인), `/opt/agentforge`(NATS
  자격증명 파일 포함), `/home/ubuntu/agentforge-daemon`(venv·템플릿·`work/` 디렉터리,
  삭제 전 확인한 `work/`는 224KB `hello-forge` 테스트 프로젝트 하나뿐이었음)를 전부
  삭제했다. **이 호스트(`fcoinfup-arm1`)로 범위를 한정** — `oci-ajou-arm1`/`arm2`에도
  같은 AgentForge daemon이 있으나 이번 요청 범위 밖이라 손대지 않았다.
- 재설치 후 검증은 이전 항목과 동일 절차(`fleet doctor` 5/5, health 200, 인증 401/200,
  loopback 바인딩, 재부팅 생존)를 다시 통과했다.

### 2026-08-21 — fcoinfup-arm1 admin bearer token 코드 버그로 인한 유출·즉시 회전

- MCP client가 `fcoinfup-arm1` Cold Standby의 `fleet serve`(SSH stdio 경유,
  HTTP/dashboard bind 미설정)에 연결하도록 구성하는 작업 중, JSON-RPC
  `initialize` 요청을 SSH로 수동 전달해 launcher를 검증하는 과정에서 `fleet`
  CLI 자체의 부팅 로그 한 줄이 `Command::Serve { api_tokens: Some(...), .. }`를
  `{:?}`(Debug)로 그대로 찍어 **`root` principal의 admin bearer token 원문이
  세션 트랜스크립트에 그대로 노출**됨. 이번 사고는 앞서 2026-08-20 항목(마스킹
  정규식 누락, 사람의 수작업 실수)과 달리 **소프트웨어 자체의 로깅 코드 버그**가
  원인 — 사람이 값을 다루다 실수한 것이 아니라, `fleet` 바이너리가 시크릿을
  구조체째 로그로 인쇄하도록 짜여 있었다.
- **즉시 대응**(발견 직후 순서대로 수행):
  1. `openssl rand` 기반 신규 토큰을 원격 호스트 SSH 세션 내부에서만 생성(로컬
     세션에 값 노출 없음).
  2. `oci-fcoinfup-arm1:/etc/fleet/fleet.env`의 `FLEET_API_TOKENS`를 SSH 경유
     `python3` 정규식 치환으로 갱신.
  3. `admin_api_tokens` 테이블의 `root` principal 행을 직접
     `UPDATE ... SET token_digest = ..., rotated_at = NOW(), rotation_generation
     = rotation_generation + 1`로 갱신 — env 값만 바꿔서는 무효화되지 않는다
     (#72의 `sync_env_admin_tokens_to_store`는 기존 DB principal을 덮어쓰지
     않고 신규 principal만 삽입하는 idempotent 동기화이므로, DB에 이미 있는
     `root` digest는 env 변경만으로는 절대 갱신되지 않음 — 반드시 DB를 직접
     같이 갱신해야 함).
  4. `fleet.service` 재기동.
  5. 유출됐던 구 토큰으로 재요청 → `401` 확인, 무효화 완료 검증.
- **근본 원인 수정**: `crates/fleet-cli/src/main.rs`의 부팅 로그를
  `command = ?cli.command`(전체 Debug)에서 `command = command_name(&cli.command)`
  (서브커맨드 이름 문자열만 반환하는 신규 헬퍼)로 교체 — 커밋 `d3cc2f6`. 부팅
  로그가 실제로 필요로 하는 정보(bind 주소 등)는 각 핸들러가 이미 개별적으로
  안전한 필드만 골라 로그에 남기고 있어 정보 손실 없음. 기본 feature와
  `--features "acp mtls"`(프로덕션 Dockerfile과 동일 조합) 양쪽에서
  `cargo check`/`cargo test -p fleet-cli` 통과 확인.
  GitHub `origin`에 push 완료(gitea는 9월까지 다운 처리 — [[gitea-remote-down]]).
- **미해결 잔여 항목**: (a) `oci-fcoinfup-arm1`에서 현재 가동 중인 `fleet`
  바이너리는 아직 수정 전 버전 — 토큰 자체는 이미 회전해 즉각적 위험은 닫혔지만,
  로깅 버그는 재기동 시마다 여전히 재발할 수 있다. 수정된 바이너리 재빌드·배포
  필요. (b) 최초 서비스 기동(`systemctl enable --now fleet.service`) 시점의
  구 토큰이 원격 호스트 자체의 `journalctl` 로그에도 남아 있을 가능성 — 호스트
  로컬 로그이며 이 세션 트랜스크립트에는 없고 해당 토큰은 이미 무효화됐으나,
  정리 여부는 아직 미결정.
  → **2026-08-21 후속 항목에서 (a) 해소, (b)는 실제로 더 심각했음이 드러남**(아래 참고).

### 2026-08-21 — 수정 바이너리 배포 중 `journalctl`에서 토큰 2건 추가 발견, 검증 중 3번째 자체 유출·즉시 재회전

- 위 항목의 (a) 조치: `oci-fcoinfup-arm1:~/fleet-src`를 `git fetch`+`git merge --ff-only`로
  `d3cc2f6`/`2134584`까지 갱신 → `cargo build --release --features "acp mtls"`(2분 24초) →
  `fleet.service` 중지 → 기존 바이너리를 `/usr/local/bin/fleet.bak-<timestamp>`로 백업 →
  신규 바이너리 교체 → 재기동. 재기동 직후 부팅 로그가 `command="serve"`만 찍는 것을
  확인(더 이상 Debug 전체 덤프 없음).
- (b) 조치 중 `journalctl -u fleet.service`를 전체 스캔해보니, 예상보다 **훨씬 많은 토큰이
  이미 로컬 로그에 평문으로 남아 있었다** — 이 문서의 앞선 항목이 다룬 토큰
  (17:24 기동분, 이미 회전·401 확인됨) 외에도:
  - 최초 설치 시점(당일 14:02~14:03경, 여러 차례 재기동) 발급된 토큰 1건 —
    이 문서의 이전 changelog 항목("fcoinfup-arm1에 Cold Standby orchestrator 신규 설치")이
    다룬 최초 설치분으로 추정. 이 세션 트랜스크립트에는 등장한 적 없음(호스트 로컬
    로그에만 존재).
  - 위 항목의 즉시 대응 절차로 회전해 넣은 새 토큰 1건(17:37 기동분) — **회전 자체가
    아직 버그 있는 구버전 바이너리로 재기동하며 수행됐기 때문에, 회전으로 새로 발급한
    토큰조차 같은 방식으로 로그에 재노출**됐다. 즉 이전 항목의 "즉시 대응"은 노출
    경로(로깅 버그)를 막지 못한 채 값만 바꾼 것이어서 회전 직후 사실상 다시 노출된
    상태였다.
- 두 토큰 모두 위 (a)의 신규 바이너리 배포 전에 발견했으므로, 신규 바이너리 배포와
  함께 **다시 회전**(동일 절차: 원격 호스트 내부에서만 `openssl rand`로 신규 토큰
  생성 → `fleet.env` 치환 → `admin_api_tokens.token_digest` 직접 갱신 →
  `fleet.service` 재기동, 이번엔 수정된 바이너리이므로 회전 자체는 로그에 남지 않음).
- **검증 과정에서 3번째 자체 유출 발생**: 회전 결과를 확인하려고 `curl`을 `bash -x`
  트레이스와 함께 실행했는데, 트레이스 출력이 명령줄에 포함된 `Authorization: Bearer
  <신규 토큰>` 헤더를 그대로 이 세션 트랜스크립트에 인쇄했다 — 코드 버그가 아니라
  **검증 스크립트 작성 실수**(원격 호스트가 아니라 이 세션 자체가 원인). 즉시 같은
  절차로 재회전, 이번엔 `set -x` 없이(그리고 토큰 값을 절대 echo하지 않는 스크립트로)
  검증만 재수행.
- **최종 검증**: 지금까지 이 사고 전체에서 노출된 적 있는 토큰 4건(최초 설치분,
  1차 회전분, 2차 회전분, `set -x`로 유출된 3차 회전분) 전부 `/v1/workers`에
  `401` 확인. 현재 활성 토큰은 `200` 확인, 값은 세션에 출력하지 않음. 최신 부팅 로그도
  `command="serve"`만 기록됨을 재확인.
- **여전히 미결정**: 위 4건의 죽은 토큰 원문이 `journalctl`(호스트 로컬, 순환 보존)에
  그대로 남아 있다 — 전부 무효화되어 즉각적 위험은 없지만, 완전한 위생을 원하면
  `journalctl --vacuum-time`/로그 재기록이 필요하다. 같은 날 다른 정상 운영 로그까지
  함께 사라지는 트레이드오프가 있어 사용자 확인 없이 진행하지 않았다.

### 2026-08-21 — primary(`oci-yarangdev-arm1`) 보안 패치 배포 중 장애 및 레거시 토큰 발견·회전

- ajou-ec1을 Fleet Worker로 정식 enrollment하기 위한 사전 조건으로, primary에도
  `d3cc2f6`(부팅 로그 시크릿 유출 수정) 이후 최신 커밋까지 배포하려다 3중 장애가
  연쇄로 발생했다.
- **1) 마이그레이션 체크섬 불일치**: 새 바이너리가 `migration 3 was previously applied
  but has been modified`로 즉시 종료. `git log --follow -p`로 대조한 결과 원인은
  #60 커밋(`71396aa`)에서 이미 배포된 `003_bootstrap_tokens.sql`의 **주석 한 줄**을
  수정한 것 — 실제 DDL은 완전히 동일. `fcoinfup-arm1`(현재 HEAD로 신규 설치돼
  올바른 checksum을 가짐)의 `_sqlx_migrations.checksum`과 로컬 `shasum -a 384`
  결과가 정확히 일치함을 검증한 뒤, primary DB의 해당 행만 그 값으로 `UPDATE`했다
  (스키마 변경 없이 checksum 메타데이터만 현재 파일과 동기화 — 데이터 위험 없음).
  이 프로젝트의 "이미 배포된 migration 파일은 절대 수정하지 않는다" 원칙이 실제로
  왜 필요한지 보여준 사례다.
- **2) 롤백 불가 상태 진입**: 체크섬 문제 확인 전 구버전으로 일단 롤백을
  시도했는데, 그 직전 새 바이너리가 이미 migration 14~20을 전부 적용해버린 뒤라
  구버전이 `migration 14 was previously applied but is missing in the resolved
  migrations`로 아예 기동 불가 상태가 됐다. 앞으로(신버전, 토큰 형식 문제) 뒤로(구버전,
  스키마 불일치) 모두 막힌 상태에서, 신버전으로 전진하며 문제를 해결하는 쪽을 택했다.
- **3) 레거시 평문 CSV 토큰 발견**: 새 바이너리는 `FLEET_API_TOKENS must be a JSON
  array` 오류로 재차 종료됐다 — primary의 `FLEET_API_TOKENS`가 이 registry가 이미
  2026-08-20에 지적했던 "평면 쉼표 token 목록" 레거시 형식 그대로였다(JSON 전환이
  한 번도 안 됨). 이 과정에서 **구버전 바이너리가 그 값 2건(대시보드 토큰과 동일한
  값 1건 + 별도 값 1건)을 부팅 로그에 평문으로 그대로 찍었다** — fcoinfup-arm1과
  같은 클래스의 사고가 primary에서도 처음 확인됨.
- **즉시 조치**: `FLEET_API_TOKENS`를 `root` principal 전체 capability의 신규 JSON
  형식 토큰으로, `FLEET_DASHBOARD_TOKEN`도 별도 신규 값으로 전부 교체(둘 다 원격
  호스트 SSH 세션 내부에서만 생성, 로컬 세션에 노출 없음). `FLEET_MCP_CAPABILITIES`도
  이 세션 전까지 미설정 상태였음을 발견해 함께 채워 넣었다(`task:create,task:read,
  task:list,task:output,task:cancel,worker:list,worker:register,dashboard:view,
  metrics:view`). 재기동 후 노출됐던 구 토큰 2건 `401`, 신규 토큰 `200`, 최신 부팅
  로그 `command="serve"`(시크릿 없음) 확인.
- **결과**: primary는 이제 최신 커밋(`8c42d51` 기준 소스 빌드)·최신 스키마(migration
  20)·JSON 형식 admin 토큰·MCP capability 설정을 모두 갖춘 상태다. 이 기회에
  `worker-ec1`/`ec2`/`arm1`의 offline 상태를 함께 확인했으나, 이는 이 작업 이전부터
  이어진 상태로 이번 장애와는 무관하다.
- **후속 필요**: 2026-08-20에 기록된 "FLEET_API_TOKENS, FLEET_GMAIL_APP_PASS 미회전"
  항목 중 `FLEET_API_TOKENS`는 이번 조치로 사실상 해소됐다(완전히 새 값으로 교체).
  `FLEET_GMAIL_APP_PASS`는 여전히 미회전 상태로 남아 있다.

### 2026-08-21 — `worker-ajou-ec1` enrollment, 코드 버그 3건 발견·수정, primary MCP 런처 추가

- 사용자 요청("orchestrator를 거치고 MCP로만 제어")에 따라 `oci-ajou-ec1`을 정식 Fleet
  Worker로 enrollment했다. 과정에서 fleet-worker join/register/heartbeat 경로의 실제
  버그 3건을 이번 세션에서 처음 발견해 수정하고 각각 회귀 테스트를 추가했다
  (전부 GitHub `origin`에 push, primary에 재빌드·배포 완료):
  1. `POST /v1/workers/join`이 admin bearer 보호 모드에서 401로 막힘 — join CLI는
     Authorization header를 안 보내고 bootstrap token은 body로 검증하는데, 미들웨어가
     이를 몰라 핸들러 도달 전에 거부하고 있었다(`worker-enrollment.md`가 이미
     "proposed-contract"로 문서화해둔 gap). 커밋 `5cd7c97`.
  2. join 응답의 `worker.toml` 렌더링이 `orchestrator_url`을 항상 플레이스홀더로
     남기고, `bind_addr`을 orchestrator가 워커에 도달하는 **공개** 주소(Cloudflare
     Tunnel 도메인)에서 잘못 파생시켜, 워커가 자기 자신을 그 도메인에 bind하려다
     실패하는 구성을 내려주고 있었다. `AppState::public_base_url`(`FLEET_BASE_URL`
     env — registry에 이미 값은 있었지만 코드가 한 번도 실제로 읽지 않던 상태)을
     추가해 실제 URL을 채우고, bind_addr는 기존 worker-ec1/ec2와 동일한 고정값
     `0.0.0.0:2419`로 되돌렸다. 커밋 `3293b05`.
  3. heartbeat 요청의 `disk_free_mb`/`mem_available_mb`/`load_avg`가 `None`일 때
     `skip_serializing_if`가 없어 명시적 JSON `null`로 나가는데, 서버 스키마는
     non-Option `#[serde(default)]`라 `null`을 역직렬화하지 못해(422) 기동 직후
     heartbeat가 항상 실패했다(디스크 캐시가 첫 갱신을 마치기 전 구간). 커밋 `3cb4396`.
- 부수적으로: `fleet-worker`의 graceful shutdown이 deregister를 호출한다는 걸
  실제로 겪었다(재배포 중 `systemctl stop`이 곧바로 worker row를 지움) — 버그는
  아니고 설계대로지만, 재배포 시 재-join이 필요하다는 걸 기록해둔다.
- **primary 자체 리소스 부족 회피**: `ajou-ec1`(RAM 956Mi)에서 직접
  `cargo build`했다가 OOM 직전까지 몰아넣어(load average 13+) 한동안 SSH 자체가
  안 붙는 사고가 있었다 — 사용자가 콘솔로 `pkill -9`해서 복구. 이후 `fleet-worker`는
  `oci-yarangdev-arm1`(RAM 11GB)에서 x86_64 크로스 컴파일(`rustup target add
  x86_64-unknown-linux-gnu` + `gcc-x86-64-linux-gnu`)로 만들어 옮기는 방식으로
  전환했다 — 소형 인스턴스에서 워크스페이스급 빌드를 직접 돌리지 않는다는 걸 이후
  운영 원칙으로 남긴다.
- **MCP 접속 대상 정정**: 직전 세션에서 만든 로컬 `.mcp.json`이 `fcoinfup-arm1`
  (Cold Standby, 별개 DB — worker가 전혀 없음)을 가리키고 있었다. `worker-ajou-ec1`은
  primary(`oci-yarangdev-arm1`)에 등록되므로 `.mcp.json`을 primary로 재설정했다.
  이 과정에서 SSH 런처가 `/etc/fleet/fleet.env`를 순수 `bash source`로 읽다가
  `FLEET_API_TOKENS`(JSON, 쉼표·중괄호 포함)와 `FLEET_GMAIL_APP_PASS`(공백 포함 Google
  App Password)를 셸 토큰으로 잘못 쪼개는 걸 발견 — systemd `EnvironmentFile=`과
  달리 bash `source`는 값을 셸 문법으로 재해석한다. `/usr/local/bin/fleet-mcp-launch.sh`
  (라인 단위 `IFS='=' read` + `export`, 셸 재해석 없음)를 primary에 추가해 해결.
- **검증**: `worker-ajou-ec1`이 `online`, heartbeat가 15초 간격으로 계속 성공(재확인
  시점 `age < 6s`). MCP `initialize`/`tools/list`가 primary에 붙어 `fleet_dispatch_task`
  등 9개 도구를 정상 반환하는 것까지 확인했다 — 아직 실제 task는 dispatch하지 않음
  (Claude Code 세션 재시작 후 `.mcp.json` 로드가 필요, 다음 단계).
