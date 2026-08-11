# 워커 부트스트랩 가이드 및 fleet-worker 기능 명세서

이 문서는 **Grok Fleet Orchestrator** 시스템의 워커 노드 초기 설치 및 인증 자동화(Bootstrap) 가이드와 `fleet-worker` 데몬이 수행하는 주요 기능 리스트를 상세히 기술합니다.

---

## 1. SSH 자동화 부트스트랩 (SSH Automatic Bootstrapping)

오케스트레이터는 신규 워커 노드를 안전하게 등록하기 위해 관리자의 개입을 최소화한 **원클릭 SSH 부트스트랩 메커니즘**을 사용합니다.

### 1.1 부트스트랩 상세 프로토콜 흐름
1. **임시 일회용 토큰 생성**: 
   * 오케스트레이터 컨트롤러 내부에서 10분 후 만료되며 단 1회만 사용 가능한 OTP형 가입 토큰(`fleet_tok_...`)을 동적으로 발행합니다.
2. **보안 전송 (SFTP/RAM Disk)**:
   * 오케스트레이터의 SFTP 클라이언트는 워커 머신의 물리 디스크에 가입 흔적을 남기지 않기 위해 RAM 디스크 임시 경로인 `/run/fleet-bootstrap.token`으로 파일을 기입합니다.
   * 기입 즉시 `chmod 0600` 명령을 수행하여 root 이외의 사용자가 토큰을 유출할 수 없도록 격리합니다.
3. **가입 실행 및 자격증명(Credentials) 발급**:
   * SSH `exec` 세션으로 `fleet-worker join --token-file /run/fleet-bootstrap.token` 명령을 실행합니다.
   * 검증이 통과되면 오케스트레이터는 해당 워커 전용 고유 API Key 및 `/etc/fleet/worker.toml` 구성 내용을 반환하여 안전하게 영구 저장합니다.
4. **암호학적 토큰 파쇄 (Shredding)**:
   * 가입 성공 즉시 `shred -u -n 3 /run/fleet-bootstrap.token` 명령을 기동하여 메모리 및 저장소 상에서 초기 가입 토큰 데이터를 영구적으로 소멸시킵니다.
5. **데몬 구동**:
   * systemd 데몬을 재로드(`systemctl daemon-reload`)하고 `fleet-worker.service` 유닛을 시작하여 정식 온라인 노드로 전환합니다.

---

## 2. fleet-worker 기능 명세서 (Feature List)

`fleet-worker`는 워커 노드 머신에서 백그라운드 서비스(systemd)로 상시 가동되는 핵심 데몬 에이전트이며, 다음과 같은 세부 기능을 내장하고 있습니다.

### 2.1 자격 증명 및 가입 기능 (Bootstrap & Credentials)
* **`fleet-worker join` CLI**:
  * 부트스트랩 토큰 검증, 워커 이름 검사, CSPRNG 기반 `grok_secret` 난수 자동 생성 및 정식 설정 파일(`worker.toml`)의 원자적 생성(atomic write)을 처리합니다.
* **API Key & UUID 보존**:
  * 재시작 시 동일 노드로 식별되기 위해 발급받은 UUID(`existing_worker_id`)와 API 통신을 위한 고유 자격 증명을 로컬 설정 파일에 암호화하여 유지합니다.

### 2.2 프로세스 감시 및 복구 (Supervision & Auto-Restart)
* **`grok agent serve` 프로세스 제어**:
  * 하위 자식 프로세스로 AI 코딩 엔진인 `grok` 프로세스를 구동하고 표준 입출력을 감시합니다.
* **프로세스 헬스체크 및 크래시 복구**:
  * `grok` 프로세스의 비정상 종료(Crash)를 감지하고 설정된 지연 시간(`restart_delay_secs`)을 거쳐 최대 10회까지 자동으로 재시작하여 인프라 가동 중단을 방지합니다.
* **Graceful Shutdown**:
  * 데몬 중지 시그널(SIGINT, SIGTERM) 수신 시, 하위 `grok` 프로세스에 먼저 정상 종료 시그널을 전달하고 타임아웃(5초) 초과 시에만 강제 종료(SIGKILL)를 발송하여 진행 중인 코딩 컴파일 컨텍스트를 보호합니다.

### 2.3 보안 및 터널링 프록시 (mTLS & Tunnel Proxy)
* **`MtlsProxy` (내장 TLS 종단 프록시)**:
  * 외부 바이너리인 `grok agent`가 mTLS를 직접 지원하지 않는 한계를 해결하기 위해, `fleet-worker` 내부에 내장형 프록시를 띄워 외부 연결은 **사설 CA mTLS(Mutual TLS)**로 인증/종단하고, 실제 트래픽만 로컬 루프백(`127.0.0.1`) 상의 grok으로 평문 포워딩합니다.
* **역방향 SSH 터널링 (autossh 연동)**:
  * 인바운드 방화벽이 차단된 폐쇄망 워커 노드를 위해 오케스트레이터 방향으로 안전한 역방향 터널을 자동 개설하여 아웃바운드 포트만으로 내부 통신을 터널링합니다.

### 2.4 관측 가능성 및 메트릭 수집 (Observability)
* **실시간 리소스 수집**:
  * 리눅스 `sysinfo` 라이브러리를 통해 호스트 머신의 실시간 CPU 로드, RAM 사용량, 디스크 여유 공간을 수집합니다.
* **주기적 하트비트**:
  * 15초 주기로 수집한 시스템 메트릭과 `grok` 엔진의 헬스 상태를 패키징하여 오케스트레이터의 `/v1/workers/heartbeat` API로 전송합니다.

### 2.5 [서버 관리 확장] 자가 치유 및 OS 통합 (Self-Healing - 로드맵)
* **룰 기반 디스크 클리닝**:
  * 디스크 용량이 90% 이상 도달 시 자동으로 임시 컴파일 부산물 및 Docker 캐시를 제거하는 자가 복구 기능.
* **systemd D-Bus 서비스 바인딩**:
  * D-Bus API를 통해 워커 호스트 리눅스의 주요 서비스 상태를 진단하고 오케스트레이터의 원격 재시작 제어를 반영합니다.
