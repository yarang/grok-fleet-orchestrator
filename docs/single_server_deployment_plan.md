# 소규모 운영을 위한 단일 서버(Single Server) 구축 계획서

이 계획서는 **Grok Fleet Orchestrator** 시스템을 최소한의 리소스로 안정적으로 운영하기 위한 **단일 서버(Single Server) 구조 설계 및 구축 가이드**입니다. 향후 다중 서버(Scale-Out)로 손쉽게 마이그레이션할 수 있는 아키텍처적 대비책을 포함하고 있습니다.

---

## 1. 단일 서버 구성 개념도 (Single Server Layout)

서버 1대(물리 서버 또는 VM 1대) 내부에서 모든 컴포넌트가 로컬 네트워크(`localhost / 127.0.0.1`)를 통해 유기적으로 통신하며, 외부 노출이 필요한 영역만 안전하게 필터링합니다.

```
┌────────────────────────────────────────────────────────────────────────┐
│  Single Linux Server (Host OS)                                         │
│                                                                        │
│  [외부 클라이언트] ──► [포트 443 / HTTPS] ──► [ Caddy 웹 서버 (Native) ]      │
│                                                   │                    │
│      ┌────────────────────────────────────────────┼──────────────┐     │
│      ▼ (Dashboard & MCP)                          ▼ (API Proxy)  │     │
│  ┌───────────────────────────┐            ┌───────────────────────────┐│
│  │ fleet serve (Native)      │            │ One API (Docker)          ││
│  │ - Port 8081 (API)         │            │ - Port 3000 (Internal)    ││
│  │ - Port 8082 (Dashboard)   │            └─────────────┬─────────────┘│
│  └─────────────┬─────────────┘                          │              │
│                │                                        ▼              │
│                └──────────► [ PostgreSQL 16 (Docker) ] ─┘              │
│                             - Port 5432 (Internal)                     │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 2. 컴포넌트별 배치 및 설치 방식

### 2.1 호스트 네이티브(Host Native) 영역
서버의 자원을 가장 효율적으로 사용해야 하고, 시스템 데몬 관리가 수월해야 하는 서비스입니다.
* **`fleet serve` (오케스트레이터)**:
  * **설치**: Rust 컴파일 산출물(바이너리)을 `/usr/local/bin/fleet`에 위치시킵니다.
  * **관리**: `systemd` 서비스(`fleet.service`)를 통해 구동 및 모니터링합니다.
* **`Caddy` (리버스 프록시)**:
  * **설치**: APT/YUM 패키지 매니저를 통해 네이티브 설치합니다.
  * **역할**: 외부 도메인 바인딩, Let's Encrypt SSL 인증서 자동 발급, 외부 요청 라우팅을 담당합니다.

### 2.2 Docker 컨테이너 영역 (Docker Compose)
관리가 복잡하고 다른 서버로의 이전이 잦을 수 있는 상태 저장(Stateful) 서비스 및 서드파티 프록시입니다.
* **`PostgreSQL 16`**: 데이터 영속성 관리가 핵심이므로 Docker 볼륨 마운트 방식으로 실행합니다.
* **`One API`**: 소규모 환경에서 무겁고 복잡한 `liteLLM` 대신 **압도적인 가성비(Go 기반 초경량)**를 지닌 One API를 선택하여 Docker 컨테이너로 기동합니다.

---

## 3. 원클릭 인프라 구축 가이드 (Setup Script)

### Step 1: Docker 및 Docker Compose 구성
서버의 특정 경로(예: `/etc/fleet`)에 아래의 `docker-compose.yml` 파일을 작성하고 컨테이너를 구동합니다.

```yaml
# /etc/fleet/docker-compose.yml
version: '3.8'

services:
  postgres:
    image: postgres:16-alpine
    container_name: fleet-postgres
    restart: always
    environment:
      POSTGRES_USER: fleet
      POSTGRES_PASSWORD: DB_SECURE_PASSWORD
      POSTGRES_DB: fleet
    ports:
      - "127.0.0.1:5432:5432"  # 외부 노출 차단, 오직 로컬 통신만 허용
    volumes:
      - pgdata:/var/lib/postgresql/data

  one-api:
    image: justsong/one-api:latest
    container_name: fleet-one-api
    restart: always
    environment:
      - SQL_DSN=postgres://fleet:DB_SECURE_PASSWORD@postgres:5432/fleet?sslmode=disable
    ports:
      - "127.0.0.1:3000:3000"  # 외부 노출 차단
    depends_on:
      - postgres

volumes:
  pgdata:
```

### Step 2: Caddy 리버스 프록시 설정
외부 도메인(`fleet.yourdomain.com`)을 통해 대시보드 및 API 프록시에 안전하게 암호화(HTTPS) 접속을 지원하도록 설정합니다.

```caddy
# /etc/caddy/Caddyfile
fleet.yourdomain.com {
    # 1. 웹 대시보드 라우팅
    reverse_proxy /dashboard* 127.0.0.1:8082
    reverse_proxy /api/events/stream* 127.0.0.1:8082
    
    # 2. 오케스트레이터 HTTP API 라우팅
    reverse_proxy /v1/* 127.0.0.1:8081
    
    # 3. 모델 API 게이트웨이 (One API) 라우팅 (워커가 접근하는 경로)
    reverse_proxy /api-gateway/* 127.0.0.1:3000
}
```

---

## 4. 다중 서버(Scale-Out) 마이그레이션 경로

이 단일 서버 구조는 서비스 규모가 확장되어 다중 서버(Scale-Out)로 전환해야 할 때 다운타임을 최소화하며 자연스럽게 확장할 수 있도록 설계되었습니다.

```
[단일 서버 (Single VM)]
  ├── DB (Docker Volume) ─────────► [이전] ──► 완전 관리형 DB (Amazon RDS / Cloud SQL)
  ├── API Proxy (Docker) ─────────► [이전] ──► 별도의 API Proxy 전용 VM
  └── fleet serve (Native) ───────► [복제] ──► 로드밸런서(ALB) 뒤에 다중 Active 서버 배치
```

1. **데이터베이스(DB) 분리**:
   * 로컬 Docker 볼륨 데이터를 `pg_dump` 하여 외부 RDS 등으로 이전하고, `fleet.env`의 `DATABASE_URL` 주소만 변경하면 가동 중단 없이 DB 분리가 완료됩니다.
2. **오케스트레이터 이중화**:
   * `fleet serve`는 완전히 무상태(Stateless) 프로세스이므로, 새로운 서버들에 복사하여 동일한 외부 DB 주소를 바라보게 띄운 후 L4 로드 밸런서만 앞에 붙이면 즉시 Active-Active 이중화가 작동합니다.
3. **API 게이트웨이 분리**:
   * `One API` 또는 `liteLLM` 컨테이너를 별도의 독립된 게이트웨이 서버로 분격하여 트래픽 오버헤드를 오케스트레이터로부터 완벽히 분리합니다.
