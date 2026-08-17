---
type: runbook
authority: canonical
implementation: partial
verification: code-checked
source: "docs/deployment/worker-provisioning.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["fleet-provisioner", "deployment"]
---

# Worker SSH 프로비저닝 Runbook

이 문서는 현재 `fleet provision`이 SSH로 `fleet-worker` 바이너리, `worker.toml`과 systemd unit을
배포하는 절차를 설명한다. 이 경로는 `/v1/workers/join`을 호출하지 않는다. daemon을 시작하면
Worker가 설정에 따라 `/v1/workers/register`와 heartbeat를 호출한다.

## 현재 동작

```mermaid
sequenceDiagram
    actor Operator
    participant CLI as fleet provision
    participant SSH as Worker SSH
    participant FS as Worker filesystem
    participant Daemon as fleet-worker
    participant API as fleet-api

    Operator->>CLI: provision with host, binary and secrets
    CLI->>SSH: verify host key and connect
    CLI->>FS: upload fleet-worker
    CLI->>FS: write /tmp/fleet-worker.toml
    CLI->>FS: move to /etc/fleet/worker.toml and chmod 0600
    CLI->>FS: install fleet-worker.service
    Operator->>Daemon: start service after review
    Daemon->>API: register and heartbeat
```

`fleet provision`은 bootstrap token을 자동 발급하지 않고, SFTP token-file을 만들거나 `shred`하지
않는다. 전달된 `--bootstrap-token`이 있으면 완성된 `worker.toml`에 그대로 기록한다.

## 사전 조건

- 대상 host, SSH user, private key와 검증된 known-hosts 정보를 준비한다.
- 로컬 `fleet-worker` binary 경로와 Worker의 grok secret을 준비한다.
- Orchestrator URL과 host 등록용 API bearer가 필요한지 확인한다.
- Worker의 지속 신원과 bootstrap token이 분리되지 않은 현재 제약을
  [Worker enrollment](../contracts/worker-enrollment.md)에서 확인한다.

운영 환경은 `strict` host-key 정책을 사용하고 `fleet scan-host-keys` 출력의 fingerprint를 별도
신뢰 채널에서 확인한다. `tofu`는 최초 연결 공격을 방지하지 못하고 `accept-all`은 운영에 사용하지
않는다.

## 단일 Host 실행

다음 예시는 playbook이 요구하는 비밀이 아닌 입력을 나타낸다. 실행 전 `FLEET_GROK_SECRET`,
`FLEET_ORCHESTRATOR_URL`과 필요한 `FLEET_API_TOKEN`을 권한이 제한된 실행 환경으로 주입한다.

```bash
fleet provision \
  --host worker.example.com \
  --name worker-01 \
  --user ubuntu \
  --ssh-key /secure/path/worker_ed25519 \
  --host-key-policy strict \
  --known-hosts /etc/fleet/known_hosts \
  --fleet-worker-bin ./target/release/fleet-worker
```

`--bootstrap-token`을 추가하면 현재 설정 파일에 원문으로 남는다. 해당 동작을 승인하지 않았다면
사용하지 않고, production Worker 인증 전환이 구현될 때까지 서비스를 시작하지 않는다.

## 검증

1. `/usr/local/bin/fleet-worker`가 기대 version과 checksum을 가지는지 확인한다.
2. `/etc/fleet/worker.toml`과 systemd unit의 소유권·mode를 확인한다.
3. 설정의 Orchestrator URL, Worker name, grok secret과 선택적 mTLS 경로를 검토한다.
4. 서비스를 명시적으로 시작하고 register·heartbeat 결과를 확인한다.
5. host inventory 등록은 best-effort이므로 CLI 성공만으로 API 등록 성공을 단정하지 않는다.

## 실패와 복구

- host-key 불일치는 접속을 중단하고 fingerprint를 재검증한다.
- playbook 실패 뒤 `/tmp/fleet-worker.toml`, `/tmp/fleet-worker.service`와 부분 설치 파일을 확인한다.
- 설정 이동과 일부 systemd 명령의 오류는 현재 구현에서 별도로 전파되지 않을 수 있으므로 원격
  파일·service 상태를 직접 확인한다.
- 이전 정상 binary와 설정을 보존했다면 서비스를 중단한 뒤 명시적으로 복원한다. 자동 rollback은
  구현되지 않았다.

## 관련 정본

- [Worker 수동 가입](../worker-bootstrap/join.md)
- [Worker enrollment](../contracts/worker-enrollment.md)
- [구성과 비밀 관리](configuration.md)
- [설치 Runbook](install.md)
