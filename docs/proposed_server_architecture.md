# 분산 AI 에이전트 인프라 서버 구조 제안서

이 제안서는 **Grok Fleet Orchestrator** 시스템을 안정적이고 지속적으로 운영하기 위한 최종 서버 인프라 및 네트워크 아키텍처 제안입니다. 

---

## 1. 아키텍처 개요 및 설계 목표

* **단일 Egress IP 통제**: 모든 분산 워커의 모델 API 호출을 단일 게이트웨이(`One API`)로 통합하여 IP 차단을 방지합니다.
* **하이브리드 설치 모델**: 메인 컨트롤러는 오버헤드가 적은 **네티브(systemd)** 서비스로 운영하고, 관리가 편리해야 하는 DB와 API 프록시는 **컨테이너(Docker)**로 격리하여 운영 오버헤드를 줄입니다.
* **보안 통신망 구성**: 사설망 내의 워커들은 외부 인터넷 아웃바운드 포트가 차단되며, 오케스트레이터와의 통신은 **mTLS** 및 **역방향 SSH 터널**로 암호화 및 보호됩니다.

---

## 2. 서버 아키텍처 및 트래픽 흐름 (Mermaid)

```mermaid
graph TD
    subgraph "External Access"
        Client[AI Client: Cursor / Claude Code] -->|HTTPS / WSS| CF_Tunnel[Cloudflare Tunnel]
    end

    subgraph "Main Controller Server (Static Public IP)"
        CF_Tunnel -->|Local Port Pass| ReverseProxy[Caddy / Nginx]
        
        subgraph "Native Layer (Host OS)"
            ReverseProxy -->|Port 8081/8082| FleetServe[fleet serve \n Native systemd]
            autosshd[SSH Daemon / autossh endpoint]
        end

        subgraph "Docker Container Layer"
            FleetServe -->|Port 5432| PgDB[(PostgreSQL 16)]
            FleetServe -->|OpenAI API Call| OneAPI[One API Proxy]
            OneAPI -->|Port 6379| Redis[(Redis Caching)]
        end
    end

    subgraph "Worker Machine 01 (Private Net)"
        subgraph "Services"
            WorkerDaemon01[fleet-worker \n Native systemd]
            MtlsProxy01[MtlsProxy]
            GrokAgent01[grok agent serve]
        end
        WorkerDaemon01 -->|Manage| GrokAgent01
        MtlsProxy01 -->|Decrypt & Forward| GrokAgent01
    end

    %% Connections
    autosshd <-->|Reverse SSH Tunnel| WorkerDaemon01
    FleetServe <-->|mTLS / Secure ACP| MtlsProxy01
    GrokAgent01 -->|API completion request| OneAPI
    OneAPI -->|Consolidated Egress IP| ExtAPI[External Model Providers: \n z.ai / OpenAI / Anthropic]
```

---

## 3. 서버별 세부 컴포넌트 구성

### 3.1 메인 오케스트레이터 서버 (Main Controller Host)
이 서버는 클라이언트 요청 수신, 스케줄링, 토큰 트래킹 및 API 프록싱을 담당하는 중앙 서버입니다. 고정 퍼블릭 IP를 지닙니다.

1. **`fleet serve` (Native systemd Service)**:
   * **역할**: MCP stdio/HTTP 서버 구동, 스케줄러 루프 및 웹 대시보드 호스팅.
   * **특징**: 단일 Rust 바이너리로 로컬 리눅스에 직접 설치되며, `fleet.service` 시스템 유닛으로 무중단 자동 재시작됩니다.
2. **`PostgreSQL 16` (Docker Container)**:
   * **역할**: 작업 큐, 워커 인벤토리 및 감사 로그 영속화.
   * **특징**: 데이터를 호스트 볼륨에 마운트하여 컨테이너로 실행하므로 DB 관리(백업, 마이그레이션)가 간편합니다.
3. **`One API` & `Redis` (Docker Containers)**:
   * **역할**: 외부 API 게이트웨이 및 캐싱 레이어.
   * **특징**: 워커 노드들의 LLM 완성 API 요청을 단일 대상을 통해 라우팅하며, 로컬 Redis를 함께 띄워 중복 요청 캐싱 및 속도 제한(Rate Limiting) 속도를 보장합니다.
4. **`Caddy` (Reverse Proxy & TLS)**:
   * **역할**: 외부 유입 경로에 대한 SSL 종단 및 HTTP/WebSocket 트래픽 포워딩.
5. **`Cloudflared` (Daemon)**:
   * **역할**: 인바운드 80/443 포트를 방화벽에서 열지 않고도 Cloudflare Edge 망을 통해 클라이언트가 안전하게 오케스트레이터 웹 대시보드 및 MCP 포트에 접근하게 합니다.

### 3.2 분산 워커 서버 (Worker Hosts)
원격지 IDC 또는 온프레미스 인프라망에 위치한 GPU/CPU 빌드 워커 머신들입니다.

1. **`fleet-worker` (Native systemd Service)**:
   * **역할**: 워커 서버 등록/하트비트 루프 실행 및 내부 `grok` 프로세스 라이프사이클 관리.
2. **`grok agent serve` (Subprocess)**:
   * **역할**: 실제 코드 편집, 파일 시스템 작업, 빌드 명령 실행.
3. **`MtlsProxy` (Embedded Proxy)**:
   * **역할**: 메인 오케스트레이터 서버로부터 오는 외부 유입 연결에 대해 사설 CA 기반 상호 TLS(mTLS) 인증을 수행하고 TLS를 종단하여 내부 로컬 루프백(`127.0.0.1`) 상의 `grok`으로 전달합니다.

---

## 4. 네트워크 트래픽 시나리오

### 4.1 클라이언트 요청 및 작업 디스패치 (Inbound Flow)
1. 사용자가 AI 클라이언트에서 작업을 요청하면 Cloudflare Tunnel을 통해 메인 오케스트레이터 서버의 `Caddy`로 유입됩니다.
2. `Caddy`가 `fleet serve` 네이티브 포트(`8081` 또는 `8082`)로 중계합니다.
3. 오케스트레이터는 DB 작업 큐에 태스크를 쓰고, 대상 워커의 `MtlsProxy` 엔드포인트(`wss://worker-ip:2420`)로 mTLS를 맺어 작업을 디스패치합니다.

### 4.2 LLM API 호출 및 단일 IP 통제 (Outbound Flow)
1. 워커 노드의 `grok agent`가 빌드 및 기획 도중 LLM 완성(Completion) 호출이 필요할 시, 자체적으로 외부 인터넷으로 직접 나가지 않고 **메인 서버의 `One API` 프록시 포트**로 API 요청을 전달합니다.
2. `One API`는 사전에 설정된 여러 공급업체(z.ai, OpenAI, Anthropic 등)의 API 키를 자동으로 주입하고 적절한 타겟 모델로 스위칭합니다.
3. 요청은 **메인 컨트롤러 서버가 가지는 단 하나의 고정된 퍼블릭 IP**를 출발지로 삼아 외부 API 제공처로 송신됩니다.

---

## 5. 인프라 구축 핵심 설정 파일 가이드

### 5.1 메인 서버 `fleet.service` (Systemd 예시)
```ini
[Unit]
Description=Grok Fleet Orchestrator Serve
After=network.target postgresql.service

[Service]
Type=simple
User=fleet
WorkingDirectory=/etc/fleet
EnvironmentFile=/etc/fleet/fleet.env
ExecStart=/usr/local/bin/fleet serve \
  --http-bind 127.0.0.1:8081 \
  --dashboard-bind 127.0.0.1:8082 \
  --transport acp
Restart=always
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

### 5.2 메인 서버 Docker Compose (`docker-compose.yml` 예시)
```yaml
version: '3.8'

services:
  postgres:
    image: postgres:16-alpine
    container_name: fleet-postgres
    restart: always
    environment:
      POSTGRES_USER: fleet
      POSTGRES_PASSWORD: CHANGE_ME_DB_PASSWORD
      POSTGRES_DB: fleet
    ports:
      - "127.0.0.1:5432:5432"
    volumes:
      - pgdata:/var/lib/postgresql/data

  one-api:
    image: justsong/one-api:latest
    container_name: fleet-one-api
    restart: always
    environment:
      - SQL_DSN=postgres://fleet:CHANGE_ME_DB_PASSWORD@postgres:5432/fleet?sslmode=disable
      - REDIS_CONN_STRING=redis://redis:6379
      - SESSION_SECRET=CHANGE_ME_SESSION_KEY
    ports:
      - "127.0.0.1:3000:3000"
    depends_on:
      - postgres
      - redis

  redis:
    image: redis:alpine
    container_name: fleet-redis
    restart: always

volumes:
  pgdata:
```
