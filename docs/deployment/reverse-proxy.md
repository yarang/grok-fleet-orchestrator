---
type: operations-reference
authority: canonical
implementation: partial
verification: code-checked
source: "docs/deployment/reverse-proxy.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["deployment", "security"]
---

# Reverse proxy 경계

이 문서는 Nginx를 사용할 때의 현재 Fleet reverse-proxy 책임을 정의한다. Caddy 이전 비교나
마이그레이션 절차는 현재 운영 규칙이 아니며 이 문서에 두지 않는다.

## 책임

- TLS 종료와 upstream 연결
- 신뢰된 proxy에서만 Real-IP 전달
- public endpoint·rate limit·WebSocket/SSE 경계
- 설정 검증, reload, rollback

Nginx 설정을 적용하기 전 `FLEET_TRUSTED_PROXIES`가 실제 proxy 대역과 정확히 일치하는지
확인한다. 불일치하면 client IP, rate limit, audit 경계가 잘못된다.

`/v1/health`와 `/metrics`는 애플리케이션 인증 미들웨어 밖에 있다. 외부 노출 여부는 gateway
ACL과 network policy로 명시한다. TLS 인증서, upstream 주소, `/v1` API, Dashboard, SSE/WebSocket
경로를 변경한 뒤에는 configuration test, reload, health, 인증 요청, stream 연결을 각각 검증한다.

실제 배포 설정의 일부만 저장소 예시로 검증됐다. 모든 프로덕션 환경에 Nginx가 이미 구성됐다고
가정하지 않는다.
