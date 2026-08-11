# 리눅스 패키징 시스템 관리 기능 상세 설계서

이 설계서는 서버 관리용 워커(Server Management Worker)에 탑재할 **리눅스 패키징 시스템 관리(APT, YUM/DNF)의 상세 기능 및 구현 사양**을 정의합니다.

---

## 1. 패키지 관리 핵심 기능 아키텍처

워커 에이전트는 다양한 리눅스 배포판(Ubuntu/Debian, CentOS/RHEL)을 지원해야 하므로, OS 버전을 자동 감지하여 적절한 패키지 래퍼(Wrapper) 명령을 수행합니다.

```
                  ┌────────────────────────────────────────┐
                  │        Orchestrator Control UI         │
                  └───────────────────┬────────────────────┘
                                      │ (Command / JSON)
                                      ▼
┌────────────────────────────────────────────────────────────────────────┐
│  fleet-worker Agent (Host OS)                                          │
│                                                                        │
│  - OS Detection (Debian / RedHat)                                      │
│  - PackageKit D-Bus / Sudoer Exec                                      │
│                                                                        │
│       ┌───────────────────────┼───────────────────────┐                │
│       ▼ (Debian-based)        ▼ (RedHat-based)        ▼ (Common)       │
│  ┌──────────────────┐    ┌──────────────────┐    ┌──────────────────┐  │
│  │ APT Wrapper      │    │ DNF/YUM Wrapper  │    │ Version Hold     │  │
│  │ (apt-get, dpkg)  │    │ (dnf, rpm)       │    │ (Apt hold, etc)  │  │
│  └──────────────────┘    └──────────────────┘    └──────────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 2. 세부 관리 기능 명세 (Feature Specifications)

### 2.1 보안 패치 및 업데이트 탐지 (Upgrade & Patch Audit)
* **기능**: 현재 OS에서 보안 및 일반 업데이트가 필요한 패키지의 목록과 전체 개수를 수집합니다.
* **배포판별 명령어**:
  * **Ubuntu/Debian**:
    ```bash
    # 업데이트 인덱스 갱신 후, 업그레이드 시뮬레이션(Dry-run) 실행 및 파싱
    sudo apt-get update -y && apt-get upgrade -s | grep -E "^Inst"
    ```
  * **RHEL/CentOS**:
    ```bash
    # 업데이트 가능 패키지 리스트업
    sudo dnf check-update --quiet
    ```
* **결과 포맷**: 워커는 이를 구조화된 JSON 데이터로 변환하여 오케스트레이터 대시보드에 보고합니다.

### 2.2 비차단(Non-blocking) 원격 패키지 설치 및 제거
패키지 설치 및 삭제 작업은 네트워크 다운로드 등으로 수 분 이상 지속될 수 있어, **비동기 작업 실행(Worker Task)** 형태로 설계합니다.
* **기능**: `git`, `docker-ce`, `build-essential` 등 특정 패키지를 무인 설치(Unattended Install)하고 결과를 반환합니다.
* **배포판별 명령어**:
  * **Ubuntu/Debian**: `DEBIAN_FRONTEND=noninteractive apt-get install -y <package>` (중간 대화형 프롬프트 강제 방지)
  * **RHEL/CentOS**: `dnf install -y <package>`
* **로그 추적**: 패키지 설치 진행 중 생성되는 `stdout` 출력을 임시 파일(`/var/log/fleet-pkg-install.log`)에 기록하고, 오케스트레이터의 로그 스트리밍 모듈을 통해 실시간 시각화합니다.

### 2.3 패키지 버전 잠금 (Apt Hold / Unhold)
* **기능**: CUDA 드라이버나 Docker 엔진 등, 전체 패키지 업그레이드 시 버전이 임의로 바뀌면 시스템 장애를 유발할 수 있는 크리티컬 패키지를 지정 버전으로 강제 고정합니다.
* **배포판별 명령어**:
  * **Ubuntu/Debian**: `sudo apt-mark hold <package>` (해제 시 `unhold`)
  * **RHEL/CentOS**: `sudo dnf versionlock add <package>` (해제 시 `delete`)

### 2.4 커스텀 저장소(Repository) 추가/제거
* **기능**: 오케스트레이터가 관리하는 사설 APT/YUM 저장소나 GPU 드라이버 배포용 외부 PPA 저장소를 원격으로 주입합니다.
* **동작**: `/etc/apt/sources.list.d/` 또는 `/etc/yum.repos.d/` 디렉토리에 신규 `.list` 또는 `.repo` 설정 파일을 원자적으로 쓰고 공개키(GPG Key)를 `apt-key`나 `rpm --import`로 시스템에 추가합니다.

---

## 3. 권한 위임 및 보안 아키텍처

패키지 관리 명령은 리눅스 `root` 권한이 요구되므로, 보안 위험을 최소화하기 위한 두 가지 접근 방식을 제시합니다.

### 3.1 [권장] PackageKit D-Bus 연동 (Daemon API)
* **원리**: 리눅스 표준 데몬인 `PackageKit`은 패키지 설치/업데이트를 담당하는 D-Bus 인터페이스를 노출합니다.
* **구현**: `fleet-worker`는 root 권한을 가지지 않고 일반 시스템 계정으로 실행하되, `/etc/dbus-1/system.d/`에 정책 파일을 설정하여 오직 `PackageKit` D-Bus API로만 패키지 제어 요청을 보냅니다.
* **효과**: 임의의 쉘 명령 주입(Shell Injection) 취약점을 완벽하게 예방할 수 있어 상업 서비스에서 가장 안전한 설계입니다.

### 3.2 Sudoers 화이트리스트 (명령어 제한)
D-Bus 사용이 불가능한 미니멀 OS 환경을 위한 폴백 방안입니다.
* **구현**: `fleet-worker` 실행 계정에 대해 모든 sudo 권한을 주는 대신, `/etc/sudoers.d/fleet` 파일을 생성하여 패키지 관리에 필요한 명령어만 패스워드 없이 실행할 수 있도록 화이트리스트를 작성합니다.
* **예시**:
  ```sudoers
  # /etc/sudoers.d/fleet
  fleet ALL=(ALL) NOPASSWD: /usr/bin/apt-get update, /usr/bin/apt-get install -y *, /usr/bin/apt-mark *
  ```
