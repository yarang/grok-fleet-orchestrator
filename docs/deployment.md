# 배포 가이드

이 문서는 Grok Fleet Orchestrator를 로컬 개발에서 프로덕션까지 배포하는 방법을
다룁니다.

## 사전 요구사항

- Rust 1.75+ (rustup 권장)
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
