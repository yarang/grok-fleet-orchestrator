# Deployment Log — arm2 (orchestrator) + arm1 (worker)

기준 일자: 2026-07-20
릴리스: v0.1.0 + 커밋 `f5205ab` (mcpServers 호환성 패치, 미태깅)

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
| worker-arm1 등록 | ✅ | id `4129f848...` (최초), 재등록 후 `4dc8fbf6...`. heartbeat 15s |
| 역방향 SSH 터널 | ✅ | autossh systemd, 자동 재접속 |
| ACP `initialize` | ✅ | protocolVersion=1 교환 성공 |
| ACP `session/new` 파라미터 | ✅ | `f5205ab` 패치로 mcpServers/cwd 해결 |
| **ACP `authenticate`** | ❌ | **블로커 — 아래 참조** |

## 블로커: ACP `authenticate` 인증 플로우 미구현

### 원인

grok 0.2.103 ACP 서버는 `initialize` 응답에 `authMethods`를 포함하며,
`session/new` 전에 클라이언트가 반드시 `authenticate` RPC를 호출해야 함.

```json
// initialize 응답 (arm1 직접 probe)
{
  "protocolVersion": 1,
  "authMethods": [
    {"id": "grok.com", "name": "Grok", "description": "Sign in with Grok"}
  ],
  "_meta": {
    "defaultAuthMethodId": null,    // ← null = 자동 인증 불가
    "agentVersion": "0.2.103",
    "currentWorkingDirectory": "/"
  }
}
```

`session/new`는 `Authentication required: no auth method id provided` 로 거절.

### `authenticate` 메서드 시그니처 (probe로 추출)

```
method: "authenticate"
params: { methodId: "grok.com", ... }
```

응답 케이스:
- `methodId` 없음 → `-32602 Invalid params: missing field methodId`
- `methodId: "grok.com"` + 추가 필드 없음 → 무한 대기 (OAuth 콜백 대기)
- `methodId: "grok.com"` + 임의 필드 → `-32000 Authentication cancelled`

### 동작 시나리오

워커의 grok이 로그인된 경우 (`~/.grok/active_sessions.json`에 세션 키 있음):
- 아마도 `authenticate(methodId="grok.com")`가 캐시된 세션으로 자동 완료될 것으로 추정
- **테스트 필요** — 아직 grok login을 완료하지 못함

워커의 grok이 로그인되지 않은 경우 (현재 상태):
- `authenticate` 호출 시 OAuth 대기 → 클라이언트 타임아웃
- `--secret` 플래그는 WebSocket handshake 보호용일 뿐 ACP 레벨 인증은 아님

### 필요 코드 변경 (fleet-transport)

1. `AcpClient::open_session` 흐름 수정:
   ```rust
   // 1. initialize (현재와 동일)
   let init_resp = send_request(build_initialize(...)).await?;

   // 2. NEW: authMethods 파싱
   let auth_methods: Vec<AuthMethod> = parse(init_resp.result.authMethods);
   let default_method = init_resp.result._meta.defaultAuthMethodId;

   // 3. NEW: defaultAuthMethodId가 있으면 authenticate 호출
   if let Some(method_id) = default_method.or_else(|| auth_methods.first().map(|m| m.id.clone())) {
       send_request(build_authenticate(id, &method_id)).await?;
   }

   // 4. session/new (현재와 동일)
   let resp = send_request(build_session_new(...)).await?;
   ```

2. `messages.rs`에 `AuthMethod`, `AuthenticateParams`, `InitializeResult._meta` 추가.

3. **워커 배포 단계에 `grok login --device-auth` 추가** — provisioner playbook 또는 런북.

### 디바이스 인증 시도 이력

- 시도 1: `7BPF-8ASR` — 15초 타임아웃, 사용자 인증 지연
- 시도 2: `76TN-K2XT` — 5분 타임아웃, 사용자 미인증 (세션 비활성으로 추정)

다음 세션에서 재시도 시:
```bash
ssh oci-yarangdev-arm1 'timeout 600 grok login --device-auth'
# 안내된 URL과 코드를 Mac 브라우저에서 열고 인증
# 완료 후 ~/.grok/active_sessions.json 확인 ({} → 세션 객체로 변경)
ssh oci-yarangdev-arm1 'sudo systemctl restart fleet-worker'
ssh oci-yarangdev-arm2 'sudo journalctl -u fleet -f | grep acp_transport'
```

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
- `f5205ab` fix(acp): mcpServers 호환성 + systemd stdin workaround (현재)
- (다음) ACP authenticate 플로우 구현 → v0.1.1 태깅

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
