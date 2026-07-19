# examples/

운영 환경에서 Grok Fleet Orchestrator 를 띄우기 위한 샘플 설정 파일 모음.
각 파일은 대응하는 Rust 스키마와 1:1 매핑되므로, 필드 추가/변경 시 해당
소스 파일을 함께 확인할 것.

## 파일 목록

| 파일                       | 용도                                                       | 대상 머신       |
|----------------------------|------------------------------------------------------------|-----------------|
| `worker.toml`              | fleet-worker 가 읽는 메인 설정 (orchestrator URL, 라벨 등) | 워커 머신       |
| `workers.yaml`             | SSH 자동 프로비저닝용 인벤토리 (`fleet provision`)         | orchestrator    |
| `fleet.service`            | orchestrator 용 systemd 유닛                               | orchestrator    |
| `fleet-worker.service`     | 워커 데몬용 systemd 유닛                                   | 워커 머신       |
| `fleet.env`                | orchestrator 환경변수 (systemd `EnvironmentFile`)          | orchestrator    |
| `mcp-clients.json`         | grok build / Claude Code / Cursor / Gemini CLI 연결 예시   | MCP 클라이언트  |

## 전형적인 배포 플로우

### 1. orchestrator 머신 준비

```bash
# install.sh 로 바이너리 설치 (또는 cargo install / cargo binstall)
curl -fsSL https://github.com/yarang/grok-fleet-orchestrator/releases/latest/download/install.sh \
  | bash

# Postgres 준비 (별도 머신 권장)
createdb fleet

# 설정 파일 배치
sudo useradd -r -s /usr/sbin/nologin fleet
sudo mkdir -p /etc/fleet /var/lib/fleet
sudo cp examples/fleet.env   /etc/fleet/fleet.env
sudo cp examples/fleet.service /etc/systemd/system/fleet.service
# /etc/fleet/fleet.env 를 실제 DATABASE_URL/FLEET_API_TOKENS 로 편집!

# mTLS 인증서 발급 (옵션; --features mtls 빌드 필요)
fleet mtls init-ca   --out /etc/fleet
fleet mtls issue-server --out /etc/fleet --cn fleet.example.com --days 365

# DB 마이그레이션 + 서비스 시작
sudo --preserve-env=DATABASE_URL fleet migrate
sudo systemctl daemon-reload
sudo systemctl enable --now fleet
fleet doctor   # 인프라 진단
```

### 2. 워커 머신 자동 프로비저닝

```bash
# orchestrator 머신에서 실행
fleet token new --prefix fleet-worker --bytes 32 > /tmp/bootstrap_token

# 인벤토리 편집 — host/name/grok_secret 채우기
cp examples/workers.yaml /tmp/workers.yaml
$EDITOR /tmp/workers.yaml

fleet provision \
  --inventory /tmp/workers.yaml \
  --bootstrap-token "$(cat /tmp/bootstrap_token)"

# workers.yaml 의 각 호스트에 대해:
#   1. SSH 접속
#   2. grok + fleet-worker 바이너리 배포
#   3. /etc/fleet-worker/worker.toml 생성 (values 자동 치환)
#   4. systemd 유닛 활성화 (fleet-worker.service)
#   5. orchestrator 등록
```

### 3. MCP 클라이언트 연결

`mcp-clients.json` 에서 사용 중인 클라이언트에 해당하는 블록을 복사해
클라이언트의 MCP 설정 파일에 붙여넣는다. 로컬 orchestrator 면 stdio,
원격이면 SSH 또는 HTTPS 중 택일.

## 커스터마이징 체크리스트

샘플 파일들을 그대로 쓰기 전에 반드시 변경해야 하는 값들:

- **`DATABASE_URL`** — `fleet.env`, 기본값은 동작하지 않는 placeholder.
- **`FLEET_API_TOKENS`** — `openssl rand -hex 32` 로 새 토큰 생성.
- **`worker.orchestrator_url`** — `worker.toml` 과 `workers.yaml`.
- **`[grok] secret` / `grok_secret`** — 각 워커마다 서로 다른 32-byte hex.
- **systemd `User=` / `Group=`** — 환경에 맞춘 시스템 계정 사용.
- **`ProtectSystem=strict` + `ReadWritePaths=`** — 상태 디렉토리 일치 확인.

## 검증

`fleet doctor` 는 다음을 한 번에 점검한다:

- Postgres 연결 + 마이그레이션 적용 여부
- HTTP API `/health` 응답
- 대시보드 `/` 응답
- 등록된 워커 수 + online 비율
- mTLS 인증서 유효기간 (활성화된 경우)

```bash
fleet doctor
# 출력 예:
# ✓ database            connected (postgres 16.2)
# ✓ migrations          14 applied, 0 pending
# ✓ http api            listening on 127.0.0.1:8081
# ✓ dashboard           listening on 127.0.0.1:8082
# ✓ workers             3 registered, 3 online
# ✓ mtls                CA valid until 2027-07-19
```
