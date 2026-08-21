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
| `FLEET_MCP_CAPABILITIES` | MCP stdio launcher의 명시적 도구 capability allow-list | MCP를 실행하는 service/launcher의 environment | 쉼표 구분 `task:read,task:create` 등 | `fleet serve`의 MCP stdio | launcher 배포 시 갱신 | 필수 — 미설정·빈 값·알 수 없는 값이면 MCP가 fail-closed. OS process identity/signed assertion을 대체하지 않음 |
| `FLEET_GMAIL_USER` / `FLEET_GMAIL_APP_PASS` | 대시보드 알림 메일 발송용 Gmail SMTP 인증 (`smtp.gmail.com:587`) | 배포 환경변수 (파일 위치 미확인) | `gmail_user`: 이메일 주소, `gmail_app_pass`: Google App Password 16자리 | `fleet-dashboard`(`crates/fleet-dashboard/src/email.rs`) | 수동(미정) | 사용 중 여부 미확인 — 코드에 구현 완료, 실배포 값 저장 위치는 이 세션에서 확인 못함 |
| SSH 키 금고 (`ssh_keys` 테이블) | 대시보드 프로비저닝(`fleet_provisioner`)이 사용하는 원격 호스트 SSH 개인키 저장소 — 개인 `~/.ssh/` 키·`worker_credentials`(LLM 키)와는 별도의 **세 번째** 자격증명 저장소 | Postgres `ssh_keys` 테이블 (AES-256-GCM 암호화, `encrypted_blob` = nonce+ciphertext+tag, `fleet-credentials`의 `MasterKey`로 암호화) | `id, name(UNIQUE), encrypted_blob, fingerprint, key_type(ed25519\|rsa\|ecdsa)` (`crates/fleet-store/migrations/010_ssh_keys.sql`) | `fleet-provisioner`, 대시보드 프로비저닝 API (`ssh_key_name`으로 참조, `PermissionKind::HostProvision` 권한 필요) | 회전 없음 — 교체는 삭제 후 재등록(update 엔드포인트 없음) | 사용 중 — 감사 로그 미연동, MCP 미노출, CLI 직접 조작은 금고를 우회할 수 있음. 현재 SSH 절차는 [Worker 프로비저닝](../deployment/worker-provisioning.md) 참고 |
| `fcoinfup-arm1` Cold Standby — `FLEET_MASTER_KEY` | 위와 동일 목적. **arm1의 값과는 완전히 별개로 새로 생성** — 복제/공유 없음 | `oci-fcoinfup-arm1:/etc/fleet/master.key` (`r--------`, fleet:fleet) | 32B hex | `fleet.service`(이 호스트) | 수동(미정) | 사용 중 — 2026-08-21 Cold Standby 신규 설치, 같은 날 Docker 빌드 설치를 삭제하고 GitHub clone + 네이티브 `cargo build`로 재설치하며 값 재생성(이전 값 폐기) |
| `fcoinfup-arm1` Cold Standby — `DATABASE_URL`/`FLEET_API_TOKENS`/`FLEET_MCP_CAPABILITIES` | Postgres 접속(로컬 `fleet` role, 네이티브 설치, arm1과 별개 인스턴스)·admin bearer 토큰(단일 `root` principal, 전체 capability)·MCP capability allow-list(`task:read,task:list,worker:list,dashboard:view,metrics:view`로 최소 설정) | `oci-fcoinfup-arm1:/etc/fleet/fleet.env` (`rw-------`, fleet:fleet) | dotenv | `fleet.service`(EnvironmentFile) | 수동(미정) | 사용 중 — 2026-08-21 생성, 같은 날 재설치로 값 재생성(이전 값 폐기). **DNS·외부 공개 없음** — `FLEET_HTTP_BIND`/`FLEET_DASHBOARD_BIND`를 `127.0.0.1`로 고정해 SSH 터널 없이는 접근 불가. 승격 전까지 admin 토큰은 root 하나뿐이라 최소 권한 분리가 안 돼 있음 — 실제 승격 시 principal별 분리 필요 |

## 알려진 미정 항목 (조치 필요)

- ~~`arm2:/etc/fleet/fleet.env.bak-debug` — 소유자/생성 경위 불명~~ — 2026-08-20 arm2
  terminate로 호스트 자체가 사라져 해소(파일도 함께 소멸). 상세는 아래 변경 이력 참고.
- `FLEET_API_TOKENS`, `FLEET_GMAIL_APP_PASS` — 2026-08-20 이전(migration) 작업 중 세션
  로그에 평문 노출됨(마스킹 정규식 누락). **회전 필요** — 아직 미완료.

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
