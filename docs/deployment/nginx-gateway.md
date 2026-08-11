# Caddy에서 Nginx로의 역방향 프록시 이전 제안 (Nginx Transition Proposal)

> 작성일: 2026-08-06. 담당: Antigravity.
>
> 소규모 단일 서버에서 대규모 다중화(Scale-Out)로 확장하는 로드맵에 맞추어, 기존 Caddy 게이트웨이를 **Nginx**로 전환할 것을 적극 제안합니다. 이 문서에서는 Nginx 전환의 아키텍처적 당위성, 단일/다중 서버 환경에서의 Nginx 상세 설정 파일(`nginx.conf`) 예시, 그리고 운영 이전 절차를 정리했습니다.

---

## 1. Nginx 전환의 당위성 및 비교

| 비교 항목 | Caddy | Nginx (제안) | 아키텍처적 기대 효과 |
|---|---|---|---|
| **동시성 & 성능** | Go 기반 고성능 | C 기반 극대화된 Event-driven | 수천 개의 워커가 실시간 Websocket/mTLS 통증 시 리소스 풋프린트 급감 |
| **로드 밸런싱** | 기본적 Upstream 지원 | 최소 연결(`least_conn`), IP 해시, Keepalive 튜닝 등 고도화된 업스트림 기능 제공 | 오케스트레이터 다중화 시 워커 부하를 지능적으로 분산 가능 |
| **보안 하드닝** | 자동화에 집중됨 | `limit_req_zone`, 버퍼 크기 제어, SSL/TLS 세부 파라미터 튜닝의 유연성 제공 | 게이트웨이 레벨에서 DoS/Abuse 공격을 1차적으로 조기 필터링 |
| **인증서 관리** | 내장 자동 발급 (ACME) | Certbot 외부 연동 필요 | 운영 체계 표준(Certbot + systemd cron)을 따르므로 인프라 관리 일관성 향상 |
| **운영 인프라 표준** | 스타트업/소규모 중심 | 엔터프라이즈 및 프로덕션 표준 대중성 | 엔지니어링 리소스 확보 및 레퍼런스 최적화 용이 |

---

## 2. 단일 서버 배포용 Nginx 설정 (`/etc/nginx/sites-available/fleet`)

단일 서버에서 오케스트레이터 API(`fleet serve` 포트 `8082`)와 대시보드 페이지를 서빙하기 위한 하드닝된 설정입니다.
Nginx가 프록시하는 헤더들을 올바르게 주입하여, 우리가 구축한 `extract_client_ip`가 외부 클라이언트의 실제 IP를 온전히 인식할 수 있도록 구성했습니다.

```nginx
# 1차 보안을 위한 Rate Limit Zone 설정 (IP당 초당 10회, 버스트 20회 허용)
limit_req_zone $binary_remote_addr zone=fleet_limit:10m rate=10r/s;

server {
    listen 80;
    listen [::]:80;
    server_name fleet.agentthread.dev;

    # Certbot ACME 챌린지 경로
    location /.well-known/acme-challenge/ {
        root /var/www/html;
    }

    # 모든 HTTP 요청을 HTTPS로 리다이렉트
    location / {
        return 301 https://$host$request_uri;
    }
}

server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name fleet.agentthread.dev;

    # SSL 인증서 (Certbot 발급 경로 예시)
    ssl_certificate /etc/letsencrypt/live/fleet.agentthread.dev/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/fleet.agentthread.dev/privkey.pem;

    # SSL 세션 및 보안 하드닝
    ssl_session_timeout 1d;
    ssl_session_cache shared:SSL:10m;
    ssl_session_tickets off;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:DHE-RSA-AES256-GCM-SHA384;
    ssl_prefer_server_ciphers off;

    # HSTS 헤더 강제 (1년)
    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains" always;
    add_header X-Frame-Options "DENY" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-XSS-Protection "1; mode=block" always;

    # 업로드 크기 제한 (패키지 및 에이전트 아티팩트 배포 고려)
    client_max_body_size 50M;

    # 대시보드 및 API 통합 프록시 경로
    location / {
        # Rate Limit 적용 (버스트 허용, 지연 없이 즉시 처리)
        limit_req zone=fleet_limit burst=20 nodelay;

        proxy_pass http://127.0.0.1:8082;
        
        # HTTP/1.1 프로토콜 및 커넥션 풀링 유지
        proxy_http_version 1.1;
        proxy_set_header Connection "";

        # Real IP 역추출용 중요 프록시 헤더 주입
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # 타임아웃 조율 (대용량 워커 통신 대비)
        proxy_connect_timeout 60s;
        proxy_read_timeout 120s;
        proxy_send_timeout 120s;
    }
}
```

---

## 3. 다중 서버(Scale-Out) 대응 로드 밸런서 설정

오케스트레이터 노드가 복수 개(`10.0.0.10:8082`, `10.0.0.11:8082`)로 다중화되었을 때의 Nginx 로드 밸런싱 구성 예시입니다.

```nginx
# 오케스트레이터 서버 풀 정의
upstream fleet_backend {
    # 최소 연결 수 알고리즘 채택 (부하 분산 최적화)
    least_conn;

    server 10.0.0.10:8082 max_fails=3 fail_timeout=10s;
    server 10.0.0.11:8082 max_fails=3 fail_timeout=10s;

    # Nginx와 오케스트레이터 간 연결 재사용 (성능 향상)
    keepalive 32;
}

server {
    listen 443 ssl http2;
    server_name fleet.agentthread.dev;

    # ... [SSL 설정 생략] ...

    location / {
        proxy_pass http://fleet_backend;
        
        proxy_http_version 1.1;
        proxy_set_header Connection "";

        # Real IP 전달
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

---

## 4. 이전 및 구축 단계 운영 절차 (Transition Playbook)

### Step 1. Nginx 및 Certbot 설치
```bash
sudo apt update
sudo apt install nginx certbot python3-certbot-nginx -y
```

### Step 2. 임시 Nginx 설정을 통한 Let's Encrypt 인증서 최초 발급
1. `/etc/nginx/sites-available/fleet` 파일을 생성하고 80번 포트(HTTP) 블록만 작성합니다.
2. 설정을 활성화하고 nginx를 기동합니다.
   ```bash
   sudo ln -s /etc/nginx/sites-available/fleet /etc/nginx/sites-enabled/
   sudo nginx -t && sudo systemctl restart nginx
   ```
3. Certbot을 실행해 인증서를 발급받습니다.
   ```bash
   sudo certbot certonly --webroot -w /var/www/html -d fleet.agentthread.dev
   ```

### Step 3. Nginx 설정 파일에 HTTPS 블록 추가 및 Caddy 서비스 종료
1. Caddy 서비스를 중지하여 포트를 비웁니다.
   ```bash
   sudo systemctl stop caddy
   sudo systemctl disable caddy
   ```
2. Nginx 설정 파일에 HTTPS(443) 프록시 블록 전체를 복사/적용합니다.
3. Nginx 설정을 확인하고 재실행합니다.
   ```bash
   sudo nginx -t && sudo systemctl restart nginx
   ```

### Step 4. `FLEET_TRUSTED_PROXIES` 환경변수 갱신
* Nginx를 오케스트레이터와 동일한 로컬 서버(`127.0.0.1`)에서 기동할 경우, 오케스트레이터의 systemd 서비스 설정 혹은 `.env` 파일에 신뢰 IP 대역을 다음과 같이 추가하여 Nginx의 포워딩 헤더를 신뢰하도록 지정합니다.
  ```env
  FLEET_TRUSTED_PROXIES="127.0.0.1,::1"
  ```
* 설정을 반영하기 위해 오케스트레이터 서비스를 재시작합니다.
  ```bash
  sudo systemctl restart fleet
  ```
