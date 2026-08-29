---
type: security-report
authority: historical
implementation: partial
verification: code-checked
source: "docs/security/reports/security-findings.md"
last_verified: "2026-08-29"
last_verified_commit: "working-tree"
owners: ["security"]
---

# 보안 발견 및 해결 이력

> **Historical:** 현재 신원·권한·secret 계약은
> [control-plane-security-model.md](../control-plane-security-model.md)가 정본이다. 이 문서는
> 발견·해결·미해결 항목의 이력을 보존하며, 현재 정책을 재정의하지 않는다.

> 기록 기준일: 2026-08-29. 초기 해결 기록: 2026-08-06.
>
> 기존 S1~S6과 S13은 해결됐다. 2026-08-16 전체 설계 감사에서 control-plane identity와
> secret lifecycle의 신규 공백을 발견했으며 정본 설계는
> [`control-plane-security-model.md`](../control-plane-security-model.md)다.

## 신규 미해결 항목

| ID | 우선순위 | 항목 | 상태 |
|---|---|---|---|
| S7 | P1 | HTTP 공용 bearer의 과도한 권한과 endpoint별 authorization 부재 | 설계 확정·구현 대기 |
| S8 | P1 | MCP ToolContext의 principal/RBAC 부재 | 설계 확정·구현 대기 |
| S9 | P1 | Bootstrap token 원문 저장·목록·URL 전달 | 부분 해결 — API/MCP/URL 원문 노출 제거, DB digest 저장은 대기 |
| S10 | P1 | Join 이후 Worker별 operational identity 전환 부재 | 설계 확정·구현 대기 |
| S11 | P1 | URL query 및 CLI argument의 secret 전달 | 설계 확정·구현 대기 |
| S12 | P2 | master key와 provider credential rotation/runbook 부재 | 설계 필요 |

S9의 첫 구현은 `token_id = SHA-256(raw token)` 공개 식별자로 목록과 회수를
전환했다. 원문은 발급 응답에서만 1회 반환하며, API/MCP 목록과 URL은 원문을
받거나 반환하지 않는다. 기존 DB schema는 아직 원문을 보관하므로 HMAC/Argon2id
digest 저장과 immutable DB token id 마이그레이션이 남아 있다.

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

### S13. Dashboard 정적 자산의 저장형 XSS — 4계층 (High)

로드맵 [`#98`](../../roadmap/roadmap.md). 배경·판정 근거·미룬 항목은 그 행이 소유하며 여기서
재서술하지 않는다.

* **결함**: 한 종류의 실수가 아니라 서로 다른 네 계층이 겹쳐 있었다.
  1. 약 20곳에서 `escapeHtml` 호출이 누락됐다.
  2. 11개 페이지 스크립트가 `d.textContent = s; return d.innerHTML` 형태의 지역
     `escapeHtml`로 전역(`app.js:292`)을 **가렸다**. 텍스트 노드 직렬화는 `&`·`<`·`>`만
     바꾸고 `"`·`'`는 그대로 두므로 속성 문맥에서 무력하다. 호출부는 이스케이프하는
     것처럼 보이므로 호출부만 읽는 리뷰로는 보이지 않는다.
  3. 3곳이 `onclick="fn('${…}')"`를 문자열로 만들었다. **HTML 이스케이프로 원리적으로
     막을 수 없다** — HTML 파서가 속성값의 문자 참조를 JS 파싱보다 먼저 디코드하므로
     `&#39;`가 `'`로 되돌아간 뒤 JS에 넘어간다. 값이 `a');alert(1);//`이면 통과한다.
  4. HTML 조각을 반환하는 헬퍼(`grokBadge`)는 호출부에서 감쌀 수 없어 이스케이프가
     헬퍼 안에 있어야 한다.
* **공격 방향**: `worker_version`·`os_type`·`arch`·`grok_version`·워커 라벨은 워커의 자기
  신고 값이므로 침해된 워커가 **관리자 브라우저**에서 스크립트를 실행한다(워커 → 관리자
  권한 상승). SSH 키 이름은 업로드 시 `trim().is_empty()`만 검증하므로
  (`crates/fleet-dashboard/src/provisioning.rs::create_ssh_key_api`) `host:provision` 보유자
  → 관리자 상승 경로가 된다.
* **수정**: 지역 정의 11개를 삭제해 전역 하나로 수렴시키고, 인라인 `onclick` 3곳을 저장소가
  이미 쓰던 관례(`data-*` + `addEventListener`)로 이전했으며, 미이스케이프 보간 13곳과
  `provision.js` 5곳에 헬퍼를 적용하고 조각 반환 헬퍼는 내부에서 이스케이프하도록 고쳤다.
* **감사 방법이 결론을 바꿨다**: 보간 중심(`${…}`) 스캔은 48건을 전부 안전으로 판정했으나,
  `provision.js`가 전부 `'+ var +'` 문자열 결합이라 그물에 걸리지 않아 통째로 누락됐다.
  **싱크 중심**(`innerHTML =`·`insertAdjacentHTML`·`outerHTML`·`.href =`) 스캔으로 다시
  돌려서야 드러났다. 데이터가 어떤 문법으로 조립되든 결국 통과해야 하는 문이 싱크이므로,
  이스케이프 감사의 정본 스캔은 싱크 쪽이어야 한다. 최종 81건 판정, 잔여 0.
* **회귀 방어**: `crates/fleet-dashboard/tests/asset_xss_invariants.rs`가 계층 2와 3의
  구조적 불변식만 잠근다. 계층 1은 값마다 출처 판단이 필요해 리뷰의 몫으로 남겼다 —
  변수 이름 allow-list로 단정하면 시간이 지날수록 틀려지는 테스트가 된다.
* **실검증**: 임시 DB에 페이로드가 실린 SSH 키를 넣고 대시보드를 띄워 확인했다.
  `/admin/ssh-keys`에서 `img[src="x"]`·`[onerror]`·목록 내 `[onclick]` 전부 0건, `alert`
  0회. 첫 행 삭제 뒤 재렌더링된 목록의 Delete가 다시 동작해 **위임 리스너 재바인딩**도
  실측했다. `/hosts/provision`의 `<option>`은 `value`가 페이로드 원본으로 **무손실
  복원**되는데도 주입이 0건인데, 이것이 계층 2의 정확한 반증이다 — `"`를 `&quot;`로 바꿔
  속성 조기 종료를 막았고 파서가 디코드해 값만 돌아왔다. 삭제한 약한 헬퍼들은 `"`를
  그대로 뒀으므로 같은 자리에서 속성이 닫혔을 것이다.
* **결과**: 워커와 `host:provision` 보유자가 관리자 세션에서 스크립트를 실행할 수 있던
  경로를 제거했고, 가장 잡기 어려운 계층(전역을 가리는 지역 정의)은 테스트가 잠갔다.
