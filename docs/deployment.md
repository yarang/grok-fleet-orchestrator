# 배포 가이드

이 문서는 Grok Fleet Orchestrator를 로컬 개발에서 프로덕션까지 배포하는 방법을
다룁니다.

## 0. 설치

orchestrator 바이너리(`fleet`)와 워커 데몬(`fleet-worker`)을 내려받는 세 가지 방법.
운영 환경에서는 **(A)** 를 기본으로, cargo 가 이미 있다면 **(B)** 가 더 빠르다.
소스 수정 중이거나 특정 feature 조합이 필요하면 **(C)**.

### (A) install.sh — curl | bash 원라인 설치

```bash
curl -fsSL https://github.com/yarang/grok-fleet-orchestrator/releases/latest/download/install.sh \
  | bash
```

이 명령은:

1. OS · 아키텍처 자동 감지 (Linux/macOS × x86_64/aarch64).
2. GitHub Release 에서 미리 빌드된 tarball 다운로드 (`fleet-v<ver>-<target>.tar.gz`).
3. `sha256sum` 으로 무결성 검증 (`fleet-v<ver>-checksums.txt`).
4. `fleet` + `fleet-worker` 바이너리를 `~/.local/bin` (또는 `--bin-dir`)에 설치.
5. `~/.zshrc` / `~/.bashrc` 에 `PATH` 추가 (이미 있으면 skip).

주요 플래그:

| 플래그               | 용도                                                |
|----------------------|-----------------------------------------------------|
| `--version <tag>`    | 특정 릴리스 설치 (예: `v0.1.0`). 미지정 시 latest. |
| `--bin-dir <path>`   | 설치 경로 오버라이드 (기본 `~/.local/bin`).         |
| `--user`             | 시스템 전역(`/usr/local/bin`) 대신 사용자 디렉토리. |
| `--no-modify-path`   | 셸 rc 파일 수정하지 않음.                           |
| `--build`            | tarball 대신 `cargo build --release` 폴백.          |
| `--dry-run`          | 다운로드/설치 단계 출력만 하고 실제 실행은 skip.    |
| `--uninstall`        | 설치 제거 (또는 `uninstall.sh` 실행).               |
| `--help`             | 전체 도움말.                                        |

환경변수: `FLEET_VERSION=<tag>`, `FLEET_BIN_DIR=<path>`.

설치 확인:

```bash
fleet --version
fleet-worker --version
fleet doctor   # 아직 DB 없으면 실패 — 다음 단계 참조
```

제거:

```bash
curl -fsSL https://github.com/yarang/grok-fleet-orchestrator/releases/latest/download/uninstall.sh \
  | bash
# 또는
install.sh --uninstall
# --purge 를 붙이면 /etc/fleet, ~/.config/fleet 도 함께 제거
```

### (B) cargo-binstall — cargo 패키지 매니저 통합

[cargo-binstall](https://github.com/cargo-bins/cargo-binstall) 가 설치되어 있으면
GitHub Release tarball 을 직접 내려받아 cargo 의 바이너리 디렉토리에 배치.

```bash
cargo binstall --git https://github.com/yarang/grok-fleet-orchestrator \
    fleet-cli fleet-worker
```

`fleet-cli` 패키지는 `fleet` 바이너리를, `fleet-worker` 패키지는
`fleet-worker` 바이너리를 제공한다. 두 패키지 모두 동일한 tarball 에서
해당 바이너리만 추출한다 (cargo-binstall 메타데이터는 각 Cargo.toml 의
`[package.metadata.binstall]` 참조).

### (C) 소스에서 빌드

```bash
git clone https://github.com/yarang/grok-fleet-orchestrator
cd grok-fleet-orchestrator

# release 빌드 (acp + mtls feature 포함; release.yml 과 동일)
cargo build --release --features "acp mtls"

# 결과:
#   target/release/fleet         (~10MB)
#   target/release/fleet-worker  (~5MB)

sudo cp target/release/fleet target/release/fleet-worker /usr/local/bin/
```

`acp` 가 기본 feature 이지만 `mtls` 는 명시해야 한다.
`--no-default-features` 로 최소 빌드도 가능 (이 경우 `--transport mock` 만 사용 가능).

### 샘플 설정 파일

`examples/` 디렉토리에 운영용 샘플이 준비되어 있다:

| 파일                       | 용도                                            |
|----------------------------|-------------------------------------------------|
| `examples/worker.toml`     | fleet-worker 가 읽는 메인 설정                  |
| `examples/workers.yaml`    | SSH 자동 프로비저닝용 인벤토리                  |
| `examples/fleet.service`   | orchestrator 용 systemd 유닛                    |
| `examples/fleet-worker.service` | 워커 데몬용 systemd 유닛                  |
| `examples/fleet.env`       | orchestrator 환경변수                           |
| `examples/mcp-clients.json`| MCP 클라이언트(grok build, Claude, Cursor 등) 연결 예시 |
| `examples/README.md`       | 전체 배포 플로우 + 커스터마이징 체크리스트      |

## 사전 요구사항

- Rust 1.75+ (rustup 권장) — **소스 빌드 시에만 필요**, install.sh / cargo-binstall 은 Rust 없이도 동작
- PostgreSQL 14+ (15 또는 16 권장)
- (워커 머신) Linux x86_64 또는 arm64, systemd

## 1. 로컬 개발 환경

### 1.1 Postgres 시작

**macOS (Homebrew)**:
```bash
brew install postgresql@16
brew services start postgresql@16
createdb fleet_dev
```

**Docker**:
```bash
docker run -d --name fleet-pg \
  -e POSTGRES_DB=fleet_dev \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -p 5432:5432 \
  postgres:16
```

### 1.2 빌드 + 마이그레이션

```bash
git clone https://github.com/yarang/grok-fleet-orchestrator
cd grok-fleet-orchestrator

export DATABASE_URL=postgres://$(whoami)@localhost/fleet_dev

cargo build
./target/debug/fleet migrate
```

### 1.3 서버 시작

```bash
./target/debug/fleet serve \
  --http-bind 127.0.0.1:8081 \
  --dashboard-bind 127.0.0.1:8082
```

확인:
```bash
curl http://127.0.0.1:8081/v1/health
# {"status":"ok","version":"0.1.0","heartbeat_interval_secs":15}

curl http://127.0.0.1:8081/metrics
# Prometheus 텍스트 포맷 출력

open http://127.0.0.1:8082/   # 대시보드
```

### 1.4 MCP 클라이언트 연결

#### grok build

`~/.config/grok/mcp.json` (또는 프로젝트의 `.grok/mcp.json`):
```json
{
  "mcpServers": {
    "fleet": {
      "command": "/abs/path/to/fleet",
      "args": ["serve"]
    }
  }
}
```

> **참고**: stdio MCP 서버는 `DATABASE_URL` 환경변수를 상속받아야 합니다.
> 셸 프로파일(`~/.zshrc`, `~/.bashrc`)에 `export DATABASE_URL=...`을 추가하거나
> `env` 래퍼를 사용하세요:
> ```json
> { "command": "env", "args": ["DATABASE_URL=postgres://...", "/path/to/fleet", "serve"] }
> ```

#### Claude Code

`~/Library/Application Support/Claude/claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "fleet": {
      "command": "/abs/path/to/fleet",
      "args": ["serve"]
    }
  }
}
```

## 2. 단일 서버 프로덕션 배포

오케스트레이터가 한 머신에서 모든 것을 담당하는 가장 단순한 프로덕션 구성.

### 2.1 바이너리 빌드 (release)

```bash
cargo build --release
# 결과: target/release/fleet (~15MB, LTO + strip)
```

### 2.2 systemd 서비스

`/etc/systemd/system/fleet.service`:
```ini
[Unit]
Description=Grok Fleet Orchestrator
After=network.target postgresql.service
Requires=postgresql.service

[Service]
Type=simple
User=fleet
Group=fleet
Environment=DATABASE_URL=postgres://fleet@localhost/fleet_prod
Environment=RUST_LOG=info,fleet=info
Environment=FLEET_API_TOKENS=<token1>,<token2>
ExecStart=/opt/fleet/bin/fleet serve \
  --http-bind 127.0.0.1:8081 \
  --dashboard-bind 127.0.0.1:8082 \
  --db-max-conn 20 \
  --health-interval-secs 15
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### 2.3 Nginx 리버스 프록시 (옵션)

```nginx
server {
    listen 443 ssl http2;
    server_name fleet.example.com;

    ssl_certificate     /etc/letsencrypt/live/fleet.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/fleet.example.com/privkey.pem;

    # 대시보드
    location / {
        proxy_pass http://127.0.0.1:8082;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        # SSE 지원
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 86400s;
    }

    # API
    location /v1/ {
        proxy_pass http://127.0.0.1:8081;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }

    location = /metrics {
        proxy_pass http://127.0.0.1:8081/metrics;
    }
}
```

> 중요: SSE (`/api/events/stream`)는 버퍼링을 꺼야 합니다 (`proxy_buffering off`).

## 3. 분산 배포 (Cloudflare Zero Trust)

이 구성에서는 오케스트레이터와 워커 모두 인바운드 포트를 인터넷에 노출하지 않습니다.
모든 트래픽이 Cloudflare Tunnel을 통해 흐릅니다.

### 3.1 아키텍처

```text
┌────────────────────┐
│ Admin browser      │
│ + MCP client       │
└─────────┬──────────┘
          │ HTTPS (Cloudflare Access JWT)
          ▼
┌────────────────────┐         ┌────────────────────┐
│ Cloudflare Edge    │◄───────►│ Cloudflare Access  │
│ (Anycast IP)       │         │ (OIDC/SAML IdP)    │
└─────────┬──────────┘         └────────────────────┘
          │ Tunnel (mTLS)
          ▼
┌────────────────────────────────────────────────────┐
│ Orchestrator 머신                                   │
│ cloudflared tunnel run fleet-orchestrator          │
│                                                    │
│ ingress:                                           │
│   - fleet.example.com/*      → 127.0.0.1:8081     │
│   - dash.fleet.example.com/*  → 127.0.0.1:8082     │
└────────────────────────────────────────────────────┘
          ▲
          │ HTTP heartbeat (워커 → 오케스트레이터)
          │
   ┌──────┴──────┬─────────────┐
   ▼             ▼             ▼
 Worker A      Worker B      Worker C
 (터널 클라이언트로 오케스트레이터에 연결)
```

### 3.2 Cloudflare 설정

1. **Tunnel 생성**:
```bash
cloudflared tunnel create fleet-orchestrator
cloudflared tunnel route dns fleet-orchestrator fleet.example.com
cloudflared tunnel route dns fleet-orchestrator dash.fleet.example.com
```

2. **config.yml** (`~/.cloudflared/config.yml`):
```yaml
tunnel: <tunnel-uuid>
credentials-file: /root/.cloudflared/<tunnel-uuid>.json

ingress:
  - hostname: dash.fleet.example.com
    service: http://127.0.0.1:8082
  - hostname: fleet.example.com
    service: http://127.0.0.1:8081
  - service: http_status:404
```

3. **Access Application 생성** (Cloudflare 대시보드):
   - Application domain: `fleet.example.com`, `dash.fleet.example.com`
   - Identity provider: Google / GitHub / Okta 등
   - AUD 값 기록 (예: `abc123def456...`)

4. **오케스트레이터 시작**:
```bash
fleet serve \
  --http-bind 127.0.0.1:8081 \
  --dashboard-bind 127.0.0.1:8082 \
  --cf-audience abc123def456...
```

이제 모든 요청은 유효한 CF-Access-Jwt-Assertion 헤더가 있어야 통과합니다.
헬스체크 (`/v1/health`, `/health`, `/metrics`)는 예외적으로 인증 없이 허용됩니다.

### 3.3 워커 프로비저닝

워커 머신에서:
```bash
# 1. fleet 바이너리 전송 (또는 cargo install)
scp fleet user@worker-host:/usr/local/bin/

# 2. 프로비저닝 실행 (orchestrator 머신에서)
fleet provision \
  --host worker-host \
  --user ubuntu \
  --ssh-key ~/.ssh/id_ed25519 \
  --name worker-a \
  --labels arch=x86_64,gpu=false,region=us-east \
  --cf-token <cloudflare-api-token> \
  --orchestrator-url https://fleet.example.com
```

또는 인벤토리 파일로 일괄 처리:
```bash
fleet provision --inventory workers.yaml --parallel 4
```

`workers.yaml` 형식:
```yaml
defaults:
  user: ubuntu
  ssh_key: ~/.ssh/id_fleet
  cf_token: cloudflare-token-here
  orchestrator_url: https://fleet.example.com

workers:
  - name: build-1
    host: 10.0.1.10
    labels: { arch: x86_64, gpu: false }
  - name: gpu-1
    host: 10.0.2.20
    labels: { arch: arm64, gpu: true }
```

자세한 프로비저닝 스텝은 소스 코드의
`crates/fleet-provisioner/src/steps/` 디렉토리를 참조하세요.

### 3.3 worker.toml 형식 (자동 생성)

`InstallFleetWorker` 스텝이 `/etc/fleet/worker.toml`을 자동 생성합니다.
수동으로 작성하는 경우 다음 형식을 따르세요:

```toml
[worker]
name = "build-farm-1"
orchestrator_url = "https://fleet.example.com"
heartbeat_interval_secs = 15
bootstrap_token = "fleet-xxx"        # 선택 — orchestrator가 bearer auth 요구 시
labels = { arch = "arm64" }

[grok]
bin = "/usr/local/bin/grok"
bind_addr = "127.0.0.1:2419"
secret = "<random-server-key>"       # 필수 — grok agent serve가 검증
max_concurrent_tasks = 4
restart_delay_secs = 5
# cwd = "/var/lib/fleet-worker"      # 선택
```

검증 모드 (`--check`)로 설정 파일 문법을 확인:

```bash
fleet-worker --config /etc/fleet/worker.toml --check
# → "config OK: name=build-farm-1 orchestrator=https://..."
```

`grok_secret`은 프로비저닝 시 caller가 생성해 전달해야 합니다. 인벤토리 모드에서는
각 워커 항목에 `grok_secret:` 필드로 지정:

```yaml
workers:
  - name: build-1
    host: 10.0.1.10
    grok_secret: "random-32-byte-hex-string"
```

또는 CLI 플래그 / 환경변수:

```bash
fleet provision --host 10.0.1.10 \
  --grok-secret "$(openssl rand -hex 32)" \
  --bootstrap-token "$FLEET_BOOTSTRAP_TOKEN" ...
```

### 3.4 셀프 서비스 워커 가입 (`fleet-worker join`)

Phase 8.3부터는 orchestrator 측에서 발급한 **bootstrap 토큰** 한 번으로
워커 머신이 직접 가입할 수 있습니다. SSH 프로비저닝 없이도
`fleet provision` 없이도 한 번의 명령으로 워커 등록 + `worker.toml` 생성 +
(옵션) 데몬 시작까지 완료됩니다.

#### 워크플로

```text
┌──────────────────────┐                   ┌──────────────────────┐
│ Orchestrator admin   │                   │ Worker 머신 (신규)   │
└──────────┬───────────┘                   └──────────┬───────────┘
           │  fleet token issue                       │
           │  --max-uses 1                            │
           │  --expires-in-secs 3600                  │
           │                                          │
           │       fleet-xxxxxxxxxxxxxxxx              │
           │ ────────────────────────────────────────► │ (채널: 슬랙/이메일/...)
           │                                          │
           │                                          │ fleet-worker join \
           │                                          │   --orchestrator-url https://fleet.example.com \
           │                                          │   --token fleet-xxxx... \
           │                                          │   --name gpu-a100-1 \
           │                                          │   --labels gpu=true,arch=arm64 \
           │                                          │   --config-out /etc/fleet/worker.toml \
           │                                          │   --start
           │                                          │
           │           POST /v1/workers/join           │
           │ ◄──────────────────────────────────────── │
           │ ────────────────────────────────────────► │
           │   200 OK + worker_config_toml 본문        │
           │                                          │   (atomic write + rename)
           │                                          │   exec fleet-worker --config ...
           │                                          │
           │  /v1/workers (heartbeat 흐름 시작)       │
           │ ◄──────────────────────────────────────── │
```

#### 1) 토큰 발급 (오케스트레이터 머신)

```bash
# 단일-사용, 1시간 짜리 토큰
fleet token issue \
  --api-url https://fleet.example.com \
  --api-token "$FLEET_ADMIN_TOKEN" \
  --max-uses 1 \
  --expires-in-secs 3600 \
  --notes "gpu-a100-1 온보딩 (2026-07-19)"

# fleet-9f3a7c2b... (15분 안에 1회 사용 가능)
```

여러 워커를 한 번에 온보딩할 때는 multi-use 토큰이 편리합니다:

```bash
fleet token issue --max-uses 10 --expires-in-secs 86400 --notes "build-farm batch"
```

토큰 목록 / 폐기:

```bash
fleet token list --api-url https://fleet.example.com --api-token "$FLEET_ADMIN_TOKEN"
fleet token revoke fleet-9f3a7c2b... --api-url https://fleet.example.com
```

#### 2) 워커 머신에서 가입

```bash
# 바이너리가 이미 설치되어 있다고 가정
fleet-worker join \
  --orchestrator-url https://fleet.example.com \
  --token fleet-9f3a7c2b... \
  --name gpu-a100-1 \
  --labels gpu=true,arch=arm64 \
  --max-concurrent-tasks 2 \
  --config-out /etc/fleet/worker.toml \
  --start
```

이 명령은:

1. 이름을 DNS-safe하게 검증 (`^[a-z0-9][a-z0-9-]{1,62}[a-z0-9]$`).
2. `grok_secret`이 주어지지 않으면 `/dev/urandom`에서 32바이트를 뽑아
   자동 생성합니다 (Windows는 UUID v4 폴백).
3. `POST /v1/workers/join` 으로 토큰을 검증 + 워커 upsert를 한 번에.
   - 토큰이 만료/소진/존재하지 않으면 401로 즉시 실패.
   - 이미 같은 이름의 워커가 살아 있으면 409.
   - **성공 시**: 응답 바디에 `worker_config_toml`이 포함되며,
     orchestrator가 생성한 grok 세션 엔드포인트(`ws://<orchestrator-host>/ws?server-key=<secret>`)와
     heartbeat 주기 등을 그대로 받아옵니다.
4. 응답받은 TOML을 `<config-out>.tmp`에 쓴 뒤 `rename` 으로原子교체 —
   잘못된 반쪽짜리 파일이 남지 않습니다.
5. `--start`가 주어지면 Unix에서는 `execvp` 로 현 프로세스를
   `fleet-worker --config <path>` 로 대체 (PID 보존, systemd 관점에서 자연스러운
   exec), Windows에서는 자식을 spawn 하고 종료될 때까지 대기합니다.

#### 3) 자동 생성되는 `worker.toml` 구조

```toml
# Auto-generated by fleet-worker join (2026-07-19T09:30:00Z)
# Do not edit by hand — re-run `fleet-worker join` to update.

[worker]
name                = "gpu-a100-1"
orchestrator_url    = "https://fleet.example.com"
heartbeat_interval_secs = 15
max_concurrent_tasks    = 2
bootstrap_token         = "fleet-9f3a7c2b..."
labels = { arch = "arm64", gpu = "true" }

[grok]
bin          = "/usr/local/bin/grok"            # PATH에서 발견된 기본값
bind_addr    = "127.0.0.1:2419"
secret       = "<32-byte-random-hex>"            # join 시 자동 생성
```

> 보안상 grok_secret은 한 번만 전송되며 orchestrator 저장소에 보관되지 않습니다.
> 워커 머신의 `/etc/fleet/worker.toml` (mode 0600, 소유자 = fleet-worker 실행 계정)
> 만이 진실의 원천입니다.

#### 토큰 안전성 가이드

- **단명(short-lived) + 단일 사용**이 기본값 (`max_uses=1`, `expires_in_secs=3600`).
  한 번 사용된 토큰은 자동으로 비활성화됩니다.
- 토큰은 **atomic `UPDATE ... WHERE use_count < max_uses AND ...`** SQL로
  소비되므로 두 클라이언트가 동시에 같은 단일-사용 토큰을 제출해도
  DB가 정확히 한 쪽만 성공시킵니다.
- 유출이 의심되면 즉시 `fleet token revoke`로 폐기하세요. 폐기된 토큰은
  사용량에 관계없이 모든 후속 join 시도가 401로 거부됩니다.
- `--api-tokens` (admin 전용) 과 bootstrap 토큰 (워커 가입 전용)은 별도의
  자격 증명입니다 — bootstrap 토큰을 가진 워커는 다른 API 엔드포인트를
  호출할 수 없습니다.

## 4. 모니터링

### 4.1 Prometheus 스크랩

`prometheus.yml`:
```yaml
scrape_configs:
  - job_name: fleet
    scrape_interval: 15s
    static_configs:
      - targets: ['localhost:8081']
    metrics_path: /metrics
```

### 4.2 Grafana 대시보드

추천 패널:
- `fleet_workers_total{status="online"}` — 온라인 워커 수 (게이지)
- `sum(fleet_workers_capacity_total) - sum(fleet_workers_active_tasks_total)` — 잔여 용량
- `rate(fleet_tasks_total{phase="completed"}[5m])` — 완료 처리량
- `fleet_tasks_total{phase="failed"}` — 실패한 작업 수

### 4.3 로깅

`RUST_LOG` 환경변수로 제어:
- `error` — 에러만
- `warn` — 경고 + 에러
- `info` — 일반 정보 (권장)
- `debug` — 디버그 정보
- `trace` — 상세 추적 (성능 영향)

예: `RUST_LOG=info,fleet_scheduler=debug,fleet_store=warn`

JSON 로그가 필요한 경우 `tracing-subscriber`의 `fmt::layer().json()`을
`fleet-cli/src/logging.rs`에서 활성화하세요.

## 5. 백업 및 복구

### 5.1 Postgres 백업

```bash
# 전체 덤프
pg_dump --format=custom fleet_prod > fleet_$(date +%Y%m%d).dump

# 특정 테이블만
pg_dump -t fleet_workers -t fleet_tasks fleet_prod > fleet_state.sql
```

### 5.2 복구

```bash
createdb fleet_restored
pg_restore -d fleet_restored fleet_20260719.dump
```

`fleet_events`는 append-only이므로 증분 백업이 간단합니다:
```sql
COPY (SELECT * FROM fleet_events WHERE seq > $last_backup_seq)
TO '/backup/events_increment.csv' WITH CSV;
```

## 6. 업그레이드

### 6.1 무중단 배포

오케스트레이터는 상태 비저장(stateless)이므로 롤링 업그레이드가 가능합니다:

1. 새 바이너리를 `/opt/fleet/bin/fleet.new`에 배치
2. `mv /opt/fleet/bin/fleet.new /opt/fleet/bin/fleet`
3. `systemctl restart fleet`

다중 admin 인스턴스를 실행 중인 경우 한 대씩 순차적으로 재시작합니다.
LISTEN/NOTIFY가 끊기는 동안의 이벤트는 트랜잭션 커밋 시점에 DB에 반영되므로
재연결 시 누락 없이 동기화됩니다.

### 6.2 마이그레이션

마이그레이션은 idempotent SQL이며 `fleet serve` 시작 시 자동 적용됩니다.
명시적으로만 실행하려면:
```bash
fleet migrate
```

다운그레이드는 지원되지 않습니다. 백업에서 복구하세요.

## 7. 문제 해결

### `doctor` 명령으로 진단

```bash
fleet doctor \
  --api-url https://fleet.example.com \
  --dashboard-url https://dash.fleet.example.com
```

출력 예:
```
==============================================================================
CHECK                            STATUS   DETAIL
==============================================================================
DATABASE_URL                     OK       environment variable is set
postgres_connect                 OK       connected to Postgres
migrations                       OK       applied successfully
workers                          OK       4 total (online=3, offline=1)
dispatch_readiness               OK       —
tasks                            OK       backend reachable (sampled 1 task(s))
api_health                       OK       https://fleet.example.com/v1/health returned 200 OK
dashboard_health                 OK       https://dash.fleet.example.com/health returned 200 OK
==============================================================================
summary: 8 OK, 0 WARN, 0 FAIL (total 8)
==============================================================================
```

### 일반적인 문제

| 증상                                | 원인 / 해결                                    |
|-------------------------------------|------------------------------------------------|
| `submit_task`가 pending에만 남음     | 워커가 온라인이 아님. `workers list` 확인       |
| MCP 클라이언트에서 "tool not found"  | `fleet` 바이너리 경로 오류. mcp.json 확인        |
| `/metrics` 401                      | bearer auth 켜져 있는데 토큰 누락                |
| SSE가 끊김                           | 중간 프록시 버퍼링. `proxy_buffering off` 설정  |
| 워커가 등록 안 됨                    | `--api-tokens` 와 워커의 `Authorization` 헤더 불일치 |
| Cloudflare Access 403               | AUD 값 불일치. `--cf-audience` 재확인           |

더 자세한 문제는 GitHub Issues에 신고해 주세요.
