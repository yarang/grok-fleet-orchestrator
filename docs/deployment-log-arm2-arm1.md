# Deployment Log — arm2 (orchestrator) + arm1 (worker)

기준 일자: 2026-07-20
릴리스: v0.1.0 + 커밋 `f5205ab` (mcpServers 호환성 패치, 미태깅)
**ACP 인증 해결됨 (2026-07-20):** API 키 기반, OAuth 불필요

## 인프라 토폴로지

```
┌────────────────────────────────────────────────────────────────┐
│ Mac (로컬)                                                       │
│ - ~/.config/cloudflare/.env  (CLOUDFLARE_API_TOKEN, Mac-only)   │
│ - 크로스컴파일 환경 (cargo-zigbuild, aarch64-unknown-linux-gnu) │
└────────────────────────────────────────────────────────────────┘
                            │
                            │ DNS 관리 (Cloudflare API)
                            ▼
        agentthread.dev (zone 2ea31993...)
        └─ A: fleet.agentthread.dev → 168.107.38.139 (DNS-only)

┌────────────────────────────────────────────────────────────────┐
│ oci-yarangdev-arm2  (168.107.38.139)  — orchestrator            │
│ - Postgres 16 (fleet DB)                                        │
│ - fleet.service (v0.1.0 + f5205ab 패치, /usr/local/bin/fleet)   │
│ - Caddy (TLS 종단, fleet.agentthread.dev)                       │
│   /ws, /ws/* → 127.0.0.1:2419  (역방향 SSH 터널 endpoint)       │
│   /*         → 127.0.0.1:8081  (HTTP API)                       │
│ - fleet-tunnel.service 없음 (터널은 arm1에서 arm2로 향함)       │
│                                                                 │
│ SSH trust: arm2의 ubuntu 키가 arm1의 authorized_keys에 등록     │
└────────────────────────────────────────────────────────────────┘
                            ▲
                            │ Reverse SSH tunnel (autossh -R 2419)
                            │
┌────────────────────────────────────────────────────────────────┐
│ oci-yarangdev-arm1  — worker                                    │
│ - fleet-worker.service (/usr/local/bin/fleet-worker)            │
│ - fleet-tunnel.service (autossh -R 2419:127.0.0.1:2419 arm2)    │
│ - grok agent serve --bind 0.0.0.0:2419 --secret <hex>           │
│ - /etc/fleet/worker.toml:                                       │
│     orchestrator_url = "https://fleet.agentthread.dev"          │
│     bootstrap_token  = "<WORKER_TOKEN>"                         │
│     labels = {}  (v0.1.0 직렬화 버그 회피)                      │
│ - grok 바이너리: /usr/local/bin/grok → ~/.grok/bin/grok (symlink)│
│ - OCI Security List: arm2 → arm1:2419 차단 → 터널로 우회        │
└────────────────────────────────────────────────────────────────┘
```

## 현재 상태 (동작)

| 컴포넌트 | 상태 | 비고 |
|---|---|---|
| Cloudflare DNS `fleet.agentthread.dev` | ✅ | DNS-only 회색구름, TTL 300, record id `84f33043...` |
| Caddy TLS (arm2) | ✅ | Let's Encrypt 자동 갱신, ws+HTTP 모두 라우팅 |
| Postgres → fleet DB | ✅ | 마이그레이션 적용 |
| fleet.service (arm2) | ✅ | HTTP API 127.0.0.1:8081, dashboard 127.0.0.1:8082 |
| worker-arm1 등록 | ✅ | id `62a7e01d-...` (재등록). heartbeat 15s |
| 역방향 SSH 터널 | ✅ | autossh systemd, 자동 재접속 |
| ACP `initialize` | ✅ | protocolVersion=1 교환 성공 |
| ACP `session/new` 파라미터 | ✅ | `f5205ab` 패치로 mcpServers/cwd 해결 |
| ACP authenticate | ✅ | **해결됨** — config API 키로 자동 통과 (아래 섹션 참조) |
| ACP `session/prompt` (LLM 호출) | ✅ | GLM-5.1로 응답 스트리밍 확인 (`pong` 등) |

## ✅ 해결됨: ACP 인증 — API 키 기반 (OAuth 불필요)

### 처음 가설 (틀림)

grok 0.2.103 ACP 서버가 `initialize` 응답에 `authMethods`를 포함하고,
`defaultAuthMethodId: null`이면 클라이언트가 반드시 `authenticate` RPC를
OAuth로 호출해야 한다고 추정함.

→ **틀렸음. 실제로는 워커의 grok이 config의 API 키로 사전 인증되면
ACP 서버가 `defaultAuthMethodId`를 자동 설정하여 클라이언트 코드 변경이 불필요.**

### 실제 메커니즘 (검증됨)

1. **grok CLI의 진짜 인증 수단 = `~/.grok/config.toml`의 API 키**
   - Mac의 `active_sessions.json`은 OAuth 세션이 아니라 실행 중인 인스턴스
     목록(pid/cwd)일 뿐. 인증 토큰이 아님.
   - Mac config:
     ```toml
     [model.gllm-5]
     base_url = "https://api.z.ai/api/coding/paas/v4"
     api_key = "99c75377ac...dsWDmPNyYrecaQ4D"
     model = "GLM-5.1"
     api_backend = "chat_completions"
     ```

2. **ACP 서버(grok agent serve)는 config의 API 키를 자동으로 사용**
   - API 키 있으면 `defaultAuthMethodId`가 자동으로 설정됨
   - 클라이언트가 `authenticate` RPC를 명시적으로 호출할 필요 없음
   - `session/new` 바로 통과

3. **fleet-transport 코드 수정 불필요** — `initialize` → `session/new` 기존 흐름 그대로 작동

### 워커 측 구성 (검증 완료)

`/root/.grok/config.toml` (fleet-worker.service가 `User=root`로 실행되므로 root 홈):
```toml
[cli]
auto_update = true

[model.grok-build]
base_url = "https://api.z.ai/api/coding/paas/v4"
api_key = "<API 키>"
model = "GLM-5.1"
api_backend = "chat_completions"
context_window = 200000
```

### 함정: `User=root`와 홈 디렉토리

fleet-worker.service의 `User=root`로 인해 `/root/.grok/config.toml`을 봄.
최초 검증 시 `/home/ubuntu/.grok/config.toml`에만 config를 넣어서 실패했었음.

**Provisioner는 worker 실행 계정의 홈 디렉토리를 인식해서 올바른 위치에 배포해야 함.**
(또는 fleet-worker를 `User=ubuntu`로 변경하는 것도 검토 필요)

### 검증 결과 (LLM 호출 성공)

```
worker 로그:
  default model resolved model_id=GLM-5.1 source=default
  event="client_new" base_url=https://api.z.ai/api/coding/paas/v4
            model=GLM-5.1 has_api_key=true has_authorization_header=true
  sampling_request{model="GLM-5.1" auth_type="bearer" auth_prefix="99c75377acfb"}
  event="sse_chunk" data={...content:"pong"}   ← 정확한 응답
  ttft_ms=1

orchestrator 로그:
  ACP session established worker_id=62a7e01d-... session=019f7bdf-6b02-...
  ACP session established worker_id=4dc8fbf6-... session=019f7bdf-86b0-...
```

### 보안 노트

- 현재 Mac의 API 키(`99c75377ac...`)가 arm1의 `/root/.grok/config.toml`에 복사됨.
- 동일 사용자 소유의 서버이므로 즉각적 위험은 낮으나, **키 회전 권장**.
- 회전 시: xAI 콘솔에서 신규 키 발급 → Mac config 업데이트 → arm1 config 업데이트 → fleet-worker 재시작.
- 회전을 자동화하려면 orchestrator의 credentials 관리 기능 필요 (아래 "다음 단계" 참조).

### 다음 단계: orchestrator credentials 중앙 관리

사용자 제안: "다른 컴퓨터에서 접근하여 작업하기 위해서는 서버에서 키를 관리해야하지 않을까?"
→ **맞음. 이제 코드로 구현 필요.**

필요 기능:
1. Postgres에 `worker_credentials` 테이블 (AES-GCM 암호화)
2. 마스터 키 로딩: `FLEET_MASTER_KEY` 환경변수 또는 `/etc/fleet/master.key` 파일
3. CLI: `fleet credentials set <worker-name> --api-key <key> [--base-url <url>] [--model <id>]`
4. API: `POST /v1/workers/:id/credentials` (관리자 전용)
5. provisioner playbook: 워커 생성 시 자동으로 config.toml 작성
6. 회전: `fleet credentials rotate <worker-name>` → 새 키 배포 + worker 재시작

상세 구현은 v0.1.1 패치 스프린트에서 진행.

## 배포된 바이너리 메타데이터

| 호스트 | 경로 | 크기 | 빌드 |
|---|---|---|---|
| arm2 | `/usr/local/bin/fleet` | 9.4MB | Mac 크로스컴파일 (cargo-zigbuild) |
| arm2 | `/usr/local/bin/fleet.bak-20260720-*` | 11MB | v0.1.0 원본 (롤백용) |
| arm1 | `/usr/local/bin/fleet-worker` | 4.7MB | arm2에서 scp |
| arm1 | `/usr/local/bin/grok` | - | symlink → ~/.grok/bin/grok (0.2.103) |

## Mac 크로스컴파일 환경

```bash
brew install zig                  # 0.16.0
cargo install cargo-zigbuild      # 0.23.0
rustup target add aarch64-unknown-linux-gnu  # 프로젝트 stable 툴체인에 추가

# 빌드 (grok-fleet-orchestrator 루트에서)
cargo zigbuild --release -p fleet-cli --target aarch64-unknown-linux-gnu

# 산출물
target/aarch64-unknown-linux-gnu/release/fleet  (ELF 64-bit ARM aarch64)
```

## 커밋 / 배포 이력

- `32bb1d2` v0.1.0 install infrastructure
- `f5205ab` fix(acp): mcpServers 호환성 + systemd stdin workaround
- `18be151` docs: ACP authenticate 블로커 해결 — API 키 기반 인증 (OAuth 불필요)
- `f1e7e73` feat(credentials): Phase 8.6 — 워커 API 키 중앙 관리 (AES-256-GCM)
- `2462f52` feat(provisioner): Phase 8.7 — PushCredentials 스텝 (credentials 자동 배포)
- (다음) PR 오픈 → v0.1.1 태깅

## Phase 8.6 + 8.7 검증 결과 (2026-07-20)

### Phase 8.6 — credentials 중앙 관리

orchestrator (arm2) 가 모든 워커의 API 키를 Postgres 에 AES-256-GCM 암호화하여
저장. 복호화는 orchestrator 의 관리 API 로만 가능.

**마스터 키 로딩**:
```
INFO fleet::runtime: master key loaded — worker credentials API enabled
                    (FLEET_MASTER_KEY env or /etc/fleet/master.key file)
```

- `/etc/fleet/master.key`: `fleet:fleet` 소유, 0400 권한.
- `fleet.service` 가 `User=fleet` 으로 실행되므로 root 소유면 읽기 실패.

**저장 예시** (DB row, api_key 는 암호화됨):
```
worker_name | model_id   | base_url                             | model_name | blob_len | rotated_at
worker-arm1 | grok-build | https://api.z.ai/api/coding/paas/v4  | GLM-5.1    |     103  | 2026-07-19 23:28:27
```

- `encrypted_blob`: AES-256-GCM 으로 암호화된 api_key + nonce + tag.
- `list` API 는 api_key 를 절대 반환하지 않음. `export` API 만 복호화.

### Phase 8.7 — PushCredentials 자동 배포

프로비저너에 `push_credentials` 스텝 추가. credentials 회전 시 수동
scp/awk 병합이 필요 없이 `fleet provision --tags credentials` 한 방으로 동기화.

**3가지 시나리오로 end-to-end 검증 완료** (arm2 → arm1 jump host):

1. **신규 설치** (빈 config.toml):
   ```
   $ fleet provision --host arm1 --tags credentials ...
   → pushed 1 credential section(s) → /root/.grok/config.toml
   ```
   결과: 빈 파일에 `[model.grok-build]` 섹션 자동 작성.

2. **회전** (키 + 모델 교체):
   ```
   $ fleet credentials set --model-name GLM-5.2 --api-key ROTATED_KEY_9999 ...
   → rotated: worker=worker-arm1 model=grok-build rotated_at=...
   $ fleet provision --tags credentials ...
   → pushed 1 credential section(s) → /root/.grok/config.toml
   ```
   결과: `[model.grok-build]` 섹션의 `api_key`, `model` 이 새 값으로 교체.

3. **섹션 보존** (회전 후 기존 섹션 유지):
   - 회전 전 config.toml 에 `[cli]`, `[ui]` 섹션 존재.
   - provision 실행 후: `[cli]`/`[ui]` 보존 + `[model.*]` 만 정확히 교체.
   ```toml
   [cli]
   auto_update = true

   [ui]
   max_thoughts_width = 120
   fork_secondary_model = "grok-build"
   yolo = false

   [model.grok-build]
   base_url = "https://api.z.ai/api/coding/paas/v4"
   api_key = "99c75377acfb4889854f95d9cbb7c24c.dsWDmPNyYrecaQ4D"
   model = "GLM-5.1"
   api_backend = "chat_completions"
   context_window = 200000
   ```

**회전 자동화 플로우** (단일 명령):
```bash
# 1. orchestrator 에서 키 갱신
fleet credentials set --worker worker-arm1 --model-id grok-build \
  --base-url https://api.z.ai/api/coding/paas/v4 \
  --api-key NEW_KEY --model-name GLM-5.2

# 2. 워커에 자동 배포
fleet provision --host arm1.fcoinfup.com --ssh-key ~/.ssh/id_ed25519 \
  --name worker-arm1 --orchestrator-url http://127.0.0.1:8081 \
  --api-token $FLEET_API_TOKEN --tags credentials
```

**설계 결정**:
- `is_applied() == false` 강제 → 재실행만으로 동기화 보장.
- TOML 파서 의존성 없이 line-based 병합 → `[model.*]` 섹션만 교체,
  나머지 섹션은 라인 단위로 보존.
- dry_run 모드는 `api_token` 검증 생략 → 전체 playbook 시뮬레이션 허용.
- PushCredentials 는 항상 sudo 사용 (`/root/.grok/config.toml` 이 root 소유 0600).

## 롤백 절차

arm2 `fleet` 바이너리를 v0.1.0으로 되돌려야 하는 경우:
```bash
ssh oci-yarangdev-arm2 '
  sudo systemctl stop fleet
  sudo cp /usr/local/bin/fleet.bak-20260720-HHMMSS /usr/local/bin/fleet
  sudo systemctl start fleet
'
```

메시지 레벨 호환성만 바뀐 것이므로 DB 스키마 영향 없음.
