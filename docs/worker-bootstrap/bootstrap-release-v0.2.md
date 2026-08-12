# 오케스트레이터 및 워커 설치/연동 가이드 — 구현 현황 요약 (Release v0.2)

> **문서 성격**: 이 문서는 `fleet`/`fleet-worker` 설치·조인·프로비저닝의 **실제 코드 구현 현황을 코드 대비 검증한 요약/색인 문서**입니다. 각 주제의 상세 설계는 아래 §0 표에 링크된 정본 문서를 참조하세요. 이 문서 단독으로 전체 스펙을 재서술하지 않습니다 — v0.1(2026-08-06)까지는 세부 설계가 문서마다 중복 서술되어 있었고, 실제 코드가 갱신되며 문서 간 불일치가 발생했습니다. v0.2는 그 중복을 없애고 "지금 코드가 실제로 무엇을 하는지"를 한곳에서 확인할 수 있게 하는 데 목적이 있습니다.
>
> 최종 코드 대조 검증일: 2026-08-12.

---

## 0. 문서 지도 (Documentation Map)

| 주제 | 정본 문서 | 구현 상태 (2026-08-12 기준) |
|---|---|---|
| 토큰 인증 (부트스트랩 토큰 + Cloudflare Access) | [`join-authentication.md`](./join-authentication.md) | 🟢 구현됨 |
| 토큰 전달 방식 3종 비교 | [`token-delivery.md`](./token-delivery.md) | 🟡 대체로 구현됨 — SSH 자동 주입의 세부 메커니즘 일부가 실제 코드와 다름 (§3.1 참고) |
| SSH 자동 프로비저닝 상세 명세 | [`ssh-provisioning.md`](./ssh-provisioning.md) | 🟡 대체로 구현됨 — 토큰 주입 메커니즘 서술이 실제 코드와 다름 (§3.1 참고, 문서 상단에 정정 배너 추가됨) |
| `fleet serve` 아키텍처 + 대시보드 설계 | [`serve-and-bootstrap-design.md`](./serve-and-bootstrap-design.md) | 🟢 대부분 구현됨 — 수치 일부 정정 필요 (§1 참고) |
| `.ssh/config` 자동 임포트 + 라벨 매핑 | 본 문서 §3.2 | 🔴 미구현 — 신규 설계 제안 |

---

## 1. `fleet serve` — 코드 대비 검증된 현황

```text
                         ┌──────────────────────────────┐
                         │         fleet serve          │
                         └──────────────┬───────────────┘
                                         │ (Spawns)
         ┌──────────────────────────────┼──────────────────────────────┐
         ▼                              ▼                              ▼
 ┌──────────────────┐           ┌──────────────────┐           ┌──────────────────┐
 │   HTTP API Server │           │  MCP stdio Server │           │ Background Loops  │
 │   (Axum Router)   │           │   (JSON-RPC)      │           │                    │
 └────────┬──────────┘           └────────┬──────────┘           └────────┬──────────┘
          │                              │                              │
          │ - /v1/workers/register       │ - MCP 도구 8개 (아래 목록)     │ - Dispatcher (이벤트 기반)
          │ - /v1/workers/heartbeat      │                              │ - Reconciler (30s 안전망)
          │ - /v1/bootstrap-tokens       │                              │ - Health Checker (15s)
          │ - fleet-dashboard 크레이트 별도 마운트 (`/`, `/hosts`, `/admin/*` 등)                       │
          └──────────────────────────────┴──────────────────────────────┘
```

이전 버전(v0.1) 대비 정정된 사실관계:

| 항목 | 이전 문서 서술 | 실제 코드 (파일 근거) |
|---|---|---|
| MCP 도구 개수 | 7개 | **8개** — `fleet_dispatch_task`, `fleet_get_task_status`, `fleet_list_workers`, `fleet_list_tasks`, `fleet_cancel_task`, `fleet_wait_for_task`, `fleet_stream_task_output`, `fleet_collect_results` (`crates/fleet-mcp/src/schema.rs`) |
| 태스크 디스패처 방식 | `fleet_tasks` 테이블을 1초 주기 폴링 | **이벤트 기반** (`mpsc` 채널 소비, `crates/fleet-scheduler/src/dispatcher.rs`). 테이블명도 `tasks`(`fleet_tasks` 아님). 별도로 정체된 태스크를 쓸어가는 **reconciler**가 **30초** 주기로 동작(`crates/fleet-scheduler/src/reconcile.rs`, 안전망 용도) |
| 헬스체커 주기/오프라인 판정 | 15초 간격, 45초(3회 누락) | **일치** — `crates/fleet-scheduler/src/health.rs`의 `HealthConfig::default` |
| 회로차단기 | Circuit Breaker 언급 | **일치, 실제 3상태(Closed/Open/HalfOpen) 구현** — `crates/fleet-scheduler/src/breaker.rs`, `workers.circuit_state` 컬럼에 영속화 |
| ACP over WebSocket | 태스크 위임 전송 방식 | **일치** — `crates/fleet-transport/src/acp_transport.rs`, 공식 `agent-client-protocol` SDK 기반 |
| `/dashboard` 경로 | HTTP API 라우터의 정적 자산 경로 | 실제로는 **별도 크레이트 `fleet-dashboard`**가 자체 라우터(`/`, `/tasks`, `/hosts`, `/admin/*` 등)로 서빙하며 `fleet serve`가 함께 기동 |

상세 모듈 설계(대시보드 RBAC, SSE 등)는 [`serve-and-bootstrap-design.md`](./serve-and-bootstrap-design.md)를 정본으로 참조하세요.

---

## 2. 워커 설치 및 수동 조인(Join) — 검증됨, 실제 코드와 일치

전체 시퀀스 다이어그램은 [`serve-and-bootstrap-design.md §3`](./serve-and-bootstrap-design.md)을 참조하세요. 여기서는 운영자가 그대로 복사해 실행할 수 있는 명령어만 정리합니다.

### 1) 설치

```bash
# install.sh 원라인 설치 (아키텍처 자동 판별: uname -s/-m → x86_64-unknown-linux-gnu 등)
curl -fsSL https://github.com/yarang/grok-fleet-orchestrator/releases/latest/download/install.sh | bash

# 소스 빌드 (install.sh --build 모드가 내부적으로 실행하는 것과 동일)
cargo build --release --features "acp mtls"
```

### 2) 토큰 발급 및 조인

```bash
# 오케스트레이터에서
fleet token issue --max-uses 1

# 워커 머신에서
sudo fleet-worker join \
  --orchestrator-url http://<오케스트레이터_IP>:<포트> \
  --token <발급받은_토큰> \
  --name <워커_이름> \
  --config-out /etc/fleet/worker.toml   # 기본값도 /etc/fleet/worker.toml
```

### 3) 서비스 등록

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now fleet-worker.service
```

systemd 유닛 템플릿은 `crates/fleet-provisioner/src/templates.rs`(`FLEET_WORKER_UNIT`)에 실제로 정의되어 있으며 예시 유닛 파일이 `examples/fleet-worker.service`, `examples/fleet.service`로도 제공됩니다.

---

## 3. SSH 프로비저닝 자동화 — 구현됨 vs 신규 제안 (구분 필수)

### 3.1 실제 구현됨: `fleet provision --host / --inventory`

`fleet provision`은 이미 존재하는 명령이지만, v0.1 문서들이 서술한 것과 **CLI 인자·내부 메커니즘이 다릅니다.**

```bash
# 단일 호스트
fleet provision --host <worker-ip> --user ubuntu --ssh-key ~/.ssh/id_ed25519 \
  --labels arch=arm64,gpu=false --orchestrator-url https://fleet.agentthread.dev

# 인벤토리 파일 기반 (--host와 상호 배타)
fleet provision --inventory workers.yaml --orchestrator-url https://fleet.agentthread.dev
```

주요 플래그: `--host` / `--inventory <파일>`(상호 배타), `--user`, `--ssh-port`, `--ssh-key`, `--labels`, `--cf-token`, `--orchestrator-url`, `--fleet-worker-bin`, `--grok-secret`, `--bootstrap-token`, `--api-token` (`crates/fleet-cli/src/main.rs`).

**토큰 주입 메커니즘도 기존 문서 서술과 다릅니다.** `ssh-provisioning.md`/`token-delivery.md`가 서술하는 `/run/fleet-bootstrap.token`에 SFTP로 써넣고 `shred -u -n 3`로 파쇄하는 2단계 흐름은 **코드에 존재하지 않습니다.** 실제로는 `crates/fleet-provisioner/src/steps/install_fleet_worker.rs`가 `bootstrap_token`이 이미 내장된 완성된 `worker.toml`을 단일 파일 쓰기로 `/etc/fleet/worker.toml`에 직접 씁니다 — 별도 토큰 파일도, 별도 파쇄 단계도 없습니다. `russh`/`russh-keys` 기반 SSH 클라이언트(`crates/fleet-provisioner/src/ssh.rs`)는 실재하며, 호스트 키 검증 정책도 `accept-all`/`tofu`/`strict` 3단계로 실제 구현되어 있습니다.

> ⚠️ `ssh-provisioning.md`와 `token-delivery.md`는 이 사실을 반영해 별도로 패치했습니다(상단 정정 배너 추가). 두 문서를 전면 재작성하는 작업은 별도 이슈로 트래킹하는 것을 권장합니다 — 본 문서에서 다루는 범위를 넘어섭니다.

### 3.2 신규 설계 제안 (미구현): `.ssh/config` 자동 임포트

아래는 **아직 코드로 구현되지 않은 제안**입니다. `--import-ssh-config`, `.ssh/config` 파서, `labels.yaml` 병합 로직 모두 저장소에 존재하지 않습니다(전수 검색 결과 0건). 구현 시 다음을 반영해 설계하세요.

#### 3.2.1 스키마 — 신규 테이블 대신 기존 `hosts` 테이블 확장을 권장

`inventory_hosts`라는 별도 테이블을 새로 만들면 이미 존재하는 `hosts` 테이블(`crates/fleet-store/migrations/007_hosts.sql`)과 목적이 상당 부분 겹칩니다. `hosts`는 이미 `hostname`(UNIQUE), `ssh_host`/`ssh_port`/`ssh_user`, `status`(`provisioned|online|offline|failed`), `worker_id` FK, `provisioned_at`/`created_at`/`updated_at`(트리거로 자동 갱신)을 갖고 있습니다. 다만 스케줄러 라벨은 `hosts`가 아니라 `workers.labels`(JSONB, GIN 인덱스)에만 존재해 — `.ssh/config` 임포트 단계(아직 워커로 등록되기 전)에서는 라벨을 붙일 곳이 없다는 문서의 지적(v0.1 §4)은 **유효한 문제**입니다. 새 테이블 대신 컬럼 확장으로 해결하는 것을 제안합니다:

```sql
-- 014_host_inventory.sql (제안 — 파일명/번호는 구현 시 확정)
ALTER TABLE hosts ADD COLUMN host_alias TEXT UNIQUE;                    -- .ssh/config의 Host 별칭
ALTER TABLE hosts ADD COLUMN identity_file TEXT;                        -- IdentityFile 로컬 "경로"만 저장 (원문 키 저장 금지 — §3.2.2)
ALTER TABLE hosts ADD COLUMN labels JSONB NOT NULL DEFAULT '{}'::jsonb; -- workers.labels와 동일 패턴
CREATE INDEX idx_hosts_labels_gin ON hosts USING GIN (labels jsonb_path_ops); -- 002_indexes.sql의 workers 인덱스와 동일 패턴
```

기존 컨벤션(`UUID` PK, `TEXT`, `TIMESTAMPTZ`, `'{}'::jsonb` 명시 캐스팅, `updated_at` 트리거)을 그대로 따릅니다.

#### 3.2.2 보안 — IdentityFile 처리 기본값을 명시

- **기본값(Default)은 반드시 "수동 허용(Opt-in) 모드"로 합니다.** `.ssh/config` 임포트 시점에는 `identity_file` **경로**만 저장하고, 개인키 원문은 절대 읽지 않습니다. 실제 프로비저닝을 실행하는 순간에만 관리자가 명시적으로 키 사용을 승인합니다.
- "자동 수집 모드"(개인키 원문을 서버가 읽어 DB에 영구 저장)는 **기본값으로 채택하지 않습니다.** 오케스트레이터 DB가 전체 인프라의 SSH 접근권을 쥔 단일 장애점이 되기 때문입니다. 이 모드를 지원하려면 별도의 보안 검토(키 관리/로테이션 설계, 감사 로그, DB 유출 시 영향 범위 평가)를 먼저 완료해야 하며, 그 전까지는 로드맵에서 제외하거나 "실험적(experimental)" 표시를 명확히 답니다.
- `known_hosts` TOFU 검증은 최초 연결 시 스푸핑을 탐지할 수 없는 근본 한계가 있습니다. 최초 등록 시 호스트 지문(fingerprint)을 관리자가 대시보드에서 육안 확인하는 단계를 넣는 것을 권장합니다.
- 프로비저닝 흐름도 [`join-authentication.md`](./join-authentication.md)가 정의한 **Cloudflare Access 1단계 방어**를 통과해야 합니다 — SSH를 통해 워커에 직접 접근하더라도, 워커가 오케스트레이터의 `/v1/workers/join`을 호출하는 구간은 동일한 네트워크 경계 보안을 적용합니다.

#### 3.2.3 실패 경로

`hosts.status`에 이미 `failed` 값이 정의되어 있으므로, 프로비저닝 실패 시 `host_events`(`007_hosts.sql`)에 `provision_fail` 이벤트를 기록하고 `status`를 `failed`로 전환하는 경로를 시퀀스에 포함해야 합니다. 재시도 정책(동일 호스트 재프로비저닝 허용 여부), SSH 세션/명령 타임아웃 값도 구현 전에 확정이 필요합니다.

### 3.3 라벨 매핑 (`labels.yaml`) — 최소 스키마 예시

```yaml
# labels.yaml
hosts:
  my-worker-alias:
    arch: arm64
    gpu: "false"
  gpu-box-1:
    arch: x86_64
    gpu: "true"
```

- `.ssh/config`의 `Host` 별칭과 `labels.yaml`의 키가 매칭되지 않는 호스트는 **임포트를 실패시키지 말고 라벨 없이(빈 `{}`) 등록하되, 경고 로그를 남기는 것**을 기본 동작으로 제안합니다.
- 대시보드 인라인 라벨 편집 UI는 [`docs/ui-dashboard/ui-design.md`](../ui-dashboard/ui-design.md)의 "SSH Config 자동 임포트 UI 흐름" 절을 정본으로 참조하세요.

---

## 변경 이력

- **2026-08-12**: v0.2 최초 작성분을 코드 대비 검증 후 요약/색인 문서로 축소. MCP 도구 개수·디스패처 동작 방식 정정, SSH 프로비저닝을 "구현됨(§3.1)"과 "신규 제안(§3.2)"으로 명확히 분리, `inventory_hosts` 신규 테이블 제안을 기존 `hosts` 테이블 확장안으로 변경, IdentityFile 처리 기본값(수동 허용)을 명시, 실패 경로 보완. `ssh-provisioning.md`/`token-delivery.md`에 정정 배너 추가(연동 패치).
