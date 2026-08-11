# 보안 발견 해결 보고서 (Security Findings Resolution Report)

> 최종 업데이트: 2026-08-06. 담당: Antigravity.
>
> 2026-08-01 기점으로 리포트되었던 6대 보안 결함(S1~S6)이 모두 성공적으로 해결되었습니다.
> 관련 소스 코드 수정 완료 및 단위/통합 테스트가 100% 통과(그린)되었습니다.

---

## 🛠️ 해결된 보안 결함 상세 내역

### S1. `/login` 및 `bootstrap` 락아웃 증폭 차단 (High)
* **수정**: `/login` 및 `bootstrap` 핸들러(`crates/fleet-dashboard/src/handlers.rs`)의 Rate Limit 차단 분기(`if !allowed`)에서 `record_login_failure` 중복 호출을 삭제하고, 감사 로그(`audit::record`)만 생성하도록 분리하였습니다.
* **결과**: 공격자가 의도적으로 오답 로그인을 퍼부어 임의 사용자의 계정을 영구 락아웃시키는 가용성 DoS 공격이 방어되었습니다.

### S2. Cloudflare Access JWT 서명 & 체인 검증 안전화 (High)
* **수정**: `parse_jwt_unsafe`로 페이로드만 분석하던 `crates/fleet-api/src/cloudflare.rs`에 `jsonwebtoken` 라이브러리를 도입하였습니다. 
  * Cloudflare JWKS(`https://<team>.cloudflareaccess.com/cdn-cgi/access/certs`)에서 공개키(JWK)를 동적으로 조회합니다.
  * 신뢰할 수 없는 임의 도메인으로의 certs 요청을 방어하기 위해 `iss`가 `cloudflareaccess.com` 서브도메인인지 화이트리스트 검사합니다.
  * 메모리 내 `OnceLock<RwLock<JwksCache>>`를 사용해 키셋을 캐싱하고 1시간 주기로 자동 갱신합니다.
  * RS256 서명, `iss`, `aud`, `exp`를 완벽히 검증하도록 E2E 검증을 적용했습니다.
* **결과**: 외부 오리진 직접 노출이나 설정 실수 시에도 가짜 서명 토큰을 통한 인증 우회가 원천 차단되었습니다.

### S3. 신뢰 프록시 뒤에서의 Real Client IP 역추출 (High)
* **수정**: `crates/fleet-dashboard/src/auth.rs` 내에 `extract_client_ip` 헬퍼 함수를 추가하고, 환경변수 `FLEET_TRUSTED_PROXIES`에 등록된 IP 대역에서 전송된 프록시 헤더(`CF-Connecting-IP`, `X-Forwarded-For`)만 파싱하여 실제 클라이언트 IP를 추출하도록 하였습니다.
* **결과**: Caddy/Cloudflared 등 프록시 환경에서 클라이언트 IP가 항상 루프백(`127.0.0.1`)으로 통일되어 전역 DoS 상태가 유발되는 취약점이 해소되었습니다.

### S4. IP 실패 카운터의 로그인-재설정 스코프 격리 (Medium)
* **수정**: `crates/fleet-store/src/postgres.rs` 의 `count_recent_ip_failures` SQL 쿼리에 필터를 추가하여, 비밀번호 재설정(`forgot_password`) 및 이메일 재발송(`resend_verification`) 목적의 정상 트래픽에 의해 누적된 기록은 IP 실패 카운트에서 제외하였습니다.
* **결과**: 정상적인 재설정 행위가 로그인 실패 임계값에 포함되어 로그인 기능이 영문 없이 차단되는 간섭 현상을 방지했습니다.

### S5. `clear_login_attempts` SQL NULL 논리 오류 제거 (Medium)
* **수정**: `crates/fleet-store/src/postgres.rs` 의 `clear_login_attempts` 삭제 쿼리의 IP 대조 부분을 `$2::text IS NULL OR ip_address IS NOT DISTINCT FROM $2` 형태로 안전화했습니다. 아울러 로그인 성공 시 특정 IP뿐만 아니라 해당 계정에 쌓인 타 IP의 모든 실패 이력도 완전히 초기화하도록 `clear_login_attempts(identifier, None)`으로 변경했습니다.
* **결과**: DB 내에 찌꺼기 실패 기록이 영구히 잔존하여 예산을 잠식하는 누수 결함을 해결했습니다.

### S6. 이메일 발송 전용 rate limit 적용 (Low)
* **수정**: 이메일 스팸 폭탄을 방어하기 위해 별도의 임계치(`MAX_EMAIL_SEND_ATTEMPTS = 3`, `MAX_IP_EMAIL_SEND_ATTEMPTS = 10`, `EMAIL_SEND_WINDOW_SECS = 3600`)를 설정하고, `check_rate_limit_custom`을 도입하여 `/forgot-password` 및 `/resend-verification` 핸들러가 1시간당 최대 3회 발송만 허용하도록 리팩토링했습니다.
* **결과**: 대량 메일 발송 엔드포인트에 대한 Abuse 공격 장벽이 크게 높아졌습니다.
