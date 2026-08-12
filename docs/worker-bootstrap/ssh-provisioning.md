# SSH 자동화 주입(Secure SSH Provisioning) 구현 상세 명세서

> ⚠️ **정정 (2026-08-12)**: 아래 §1·§3의 "SFTP로 `/run/fleet-bootstrap.token` 파일을 써넣고 `shred -u -n 3`로 파쇄" 흐름은 **실제 코드와 다릅니다.** 실제로는 `crates/fleet-provisioner/src/steps/install_fleet_worker.rs`가 `bootstrap_token`이 이미 내장된 `worker.toml`을 단일 파일 쓰기로 `/etc/fleet/worker.toml`에 직접 씁니다 — 별도 토큰 파일도 파쇄 단계도 없습니다. 실제 CLI 인자와 흐름은 [`bootstrap-release-v0.2.md §3.1`](./bootstrap-release-v0.2.md)을 참조하세요. 이 문서는 최초 설계 의도를 남기기 위해 원문을 보존하며, 전면 재작성은 별도 작업으로 트래킹합니다.

이 명세서는 채택된 **1번 방법(SSH 자동화 주입 방식)**에 대한 구체적인 구현 플레이북(Playbook), 관련 리눅스 쉘 명령어 스펙, 그리고 `fleet-provisioner` 크레이트 내에서의 내부 라이브러리 연동 상세 흐름을 정의합니다.

---

## 1. SSH 프로비저닝 워크플로우 다이어그램 (Mermaid)

![SSH Provisioning Workflow Diagram](../assets/diagrams/worker-bootstrap/ssh-provisioning-sequence.mermaid)

---

## 2. 세부 구현 단계 및 터미널 명령어 스펙

`fleet-provisioner`가 SSH 실행 세션에서 수행하는 구체적인 커맨드 시퀀스입니다.

### 2.1 사전 검사 (Pre-flight Checks)
대상 서버가 가입 가능한 상태인지 진단합니다.
```bash
# 1. systemd 존재 여부 및 grok 바이너리 설치 상태 진단
systemctl --version >/dev/null 2>&1 && which grok >/dev/null 2>&1
```

### 2.2 임시 토큰 주입 (Token Injection via SFTP)
오케스트레이터의 SFTP 클라이언트는 RAM 디스크 경로(`/run/` 또는 `/dev/shm/`) 아래에 파일을 생성하여 디스크 I/O 흔적을 피하고 메모리 상에서만 토큰이 존재하도록 처리합니다.
* **대상 경로**: `/run/fleet-bootstrap.token`
* **소유권 및 권한 설정**:
  ```bash
  # 소유자(root) 이외의 사용자 접근 차단
  chmod 0600 /run/fleet-bootstrap.token
  ```

### 2.3 가입 실행 (Exec Join)
임시 토큰 파일을 인자로 제공하여 워커 데몬 가입 프로세스를 실행합니다.
```bash
# /etc/fleet/worker.toml에 최종 프로덕션용 API Key 및 설정 파일 자동 생성
sudo fleet-worker join \
  --orchestrator-url https://fleet.agentthread.dev \
  --token-file /run/fleet-bootstrap.token \
  --name worker-arm64-01 \
  --labels arch=arm64,gpu=false \
  --config-out /etc/fleet/worker.toml
```

### 2.4 토큰 파쇄 및 정리 (Token Shredding)
가입이 성공하여 `worker.toml`이 올바르게 생성된 경우, 임시 토큰 파일을 단순 삭제하지 않고 리눅스의 **`shred`** 명령어를 사용해 데이터 비트를 덮어씌워 완전히 파쇄한 후 삭제합니다.
```bash
# 난수로 3회 덮어쓰고, 용량을 0으로 만든 후 파일 제거
sudo shred -u -n 3 /run/fleet-bootstrap.token || sudo rm -f /run/fleet-bootstrap.token
```

### 2.5 데몬 활성화 및 시작 (Daemon Activation)
```bash
# systemd 설정 반영 및 서비스 등록 후 구동
sudo systemctl daemon-reload
sudo systemctl enable fleet-worker.service
sudo systemctl restart fleet-worker.service
```

---

## 3. Rust 라이브러리 수준의 예시 구현체 설계

`crates/fleet-provisioner/src/playbook.rs` 내부에서 이 프로세스를 처리하기 위한 의사코드(Pseudocode) 스펙입니다.

```rust
use russh::client::Msg;
use russh_sftp::client::Sftp;
use std::path::Path;

pub async fn run_bootstrap_step(
    ssh_session: &mut russh::client::Handle<Client>,
    sftp: &Sftp,
    token: &str,
    worker_name: &str,
    orchestrator_url: &str,
) -> Result<(), ProvisionError> {
    // 1. 임시 토큰 파일 생성 및 쓰기 (SFTP)
    let token_path = "/run/fleet-bootstrap.token";
    let mut file = sftp.create(token_path).await?;
    file.write_all(token.as_bytes()).await?;
    
    // 2. 권한 변경 (chmod 600)
    sftp.set_metadata(token_path, russh_sftp::protocol::Permissions::from_bits_truncate(0o600)).await?;

    // 3. 가입 명령 실행 (SSH Exec)
    let join_cmd = format!(
        "sudo fleet-worker join --orchestrator-url {} --token-file {} --name {} --config-out /etc/fleet/worker.toml",
        orchestrator_url, token_path, worker_name
    );
    let response = run_ssh_command(ssh_session, &join_cmd).await?;
    if response.exit_code != 0 {
        return Err(ProvisionError::JoinFailed(response.stderr));
    }

    // 4. 안전한 토큰 파쇄
    let shred_cmd = format!("sudo shred -u -n 3 {}", token_path);
    let _ = run_ssh_command(ssh_session, &shred_cmd).await;

    // 5. systemd 재시작
    let systemd_cmd = "sudo systemctl daemon-reload && sudo systemctl restart fleet-worker.service";
    run_ssh_command(ssh_session, systemd_cmd).await?;

    Ok(())
}
```
