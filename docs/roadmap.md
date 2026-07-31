# Roadmap & Gap Analysis

> 최초 작성 2026-07-28 · **최종 실측 검증 2026-08-01**. 우선순위: **P0** critical, **P1** high, **P2** medium, **P3** low.
>
> 항목 번호는 팀 내 참조 키이므로 **완료되어도 번호를 재사용하거나 삭제하지 않는다** — ✅로 표시하고 원인/커밋을 남긴다.
> 이 문서의 갱신 오너는 planner이며, 단계 완료 시점마다 코드 실측 대조 후 일괄 갱신한다.
> 다른 담당자는 커밋 메시지에 항목 번호(`#7`, `#20`)만 남기면 된다.

## P0 — Production Blockers

1. ✅ **Dockerfile** — 해결됨 (`acca872`). 멀티스테이지 빌드 + `.dockerignore`.
2. ✅ **docker-compose** — 해결됨 (`acca872`). orchestrator + Postgres + worker 로컬 환경, `docker/worker.toml` 포함.

> P0 배포 차단 요인은 현재 **전부 해소**됐다.

## P1 — Security & Data Risks

3. ✅ **패스워드 재설정 엔드포인트 레이트 리미팅** — 해결됨 (`b501ca5`).

   **주의 — 이 항목은 한때 "구현됨"으로 오판됐다.** `/forgot-password`, `/reset-password`,
   `/resend-verification`에 `check_rate_limit()` **호출부는 존재했지만 영구 무동작**이었다.
   리미터는 `login_attempts`의 `success = FALSE` 행을 세는데, 세 핸들러에서 실패를 기록하는
   코드가 **이미 차단된 `if !allowed` 블록 내부에만** 있었다. 정상 요청은 아무 행도 남기지
   않아 카운터가 0에서 오르지 않았고, 차단 분기는 도달 불가능했다. (대조군: `/login`은 실제
   실패 경로에서 기록해 정상 작동했다 — 동일 헬퍼를 복사하면서 기록 호출만 누락한 것.)
   추가로 `/reset-password`는 식별자로 raw token을 써서, 매 시도 새 토큰을 쓰는 공격자에게는
   식별자별 카운터가 구조적으로 항상 0이었다 → 토큰 열거 무제한.

   수정: 정상 경로 기록용 `record_rate_limited_request` 신설, reset은 무효/소비됨/만료 토큰
   경로에서 각각 기록, 식별자를 raw token → `rl_identifier`로 교체.

   **교훈**: 호출부 존재만으로 완료 판정하지 말 것. 항목 4의 회귀 테스트가 이 상태를 고정한다.

4. ✅ **PgStore auth 통합 테스트 부재** — 해결됨 (`afd8d35`). `crates/fleet-store/tests/auth_integration.rs`에
   User/Session/LoginAttempt/PasswordReset/EmailVerification 통합 테스트 35개 추가, 실제 Postgres
   대비 검증. `count_recent_failed_attempts`의 `IS NOT DISTINCT FROM` IP 스코핑 버그 전용 회귀
   테스트 포함.

   이 작업 중 `migrations/004_rbac.sql`의 부분 인덱스 술어(`WHERE expires_at > NOW()` — `NOW()`는
   STABLE이라 Postgres가 "must be marked IMMUTABLE"로 거부)가 **마이그레이션 004~010 전체를
   실패시켜 빈 DB에 auth 테이블이 하나도 생성되지 않던 문제**도 발견/수정했다 (`b501ca5`).

   근본 원인은 테스트 하네스였다: `require_db!`가 연결/마이그레이션 실패를 `None`(skip)으로
   뭉개, 35개 테스트가 단 한 줄의 assert도 실행하지 않고 `... ok`로 통과 표시됐다. CI에도
   Postgres 서비스가 있었지만 같은 이유로 놓쳤다. 하네스를 **"`DATABASE_URL` 미설정 시에만 skip,
   설정됐는데 연결/마이그레이션 실패 시 `panic!`"** 으로 전환해 재발 시 CI가 반드시 하드 실패한다.

5. 🟡 **지연 시간/처리량 메트릭** — 부분 완료. `fleet_task_duration_seconds` 히스토그램 구현됨
   (`_bucket{le=}`/`_sum`/`_count` 정상 출력). **잔여**: `dispatch_latency`, `http_request_duration`.
   → 담당: coder

   참고: prometheus 크레이트 의존성이 없고 `metrics.rs`가 텍스트를 직접 생성하는 방식이라
   히스토그램 버킷을 수작업 구현해야 한다 — 겉보기보다 비용이 크다.

6. ⏳ **DB 백업 스크립트 부재** — `pg_dump` 자동화, 시점 복구 설정 없음. 현재 `docs/deployment.md`에
   산문 안내만 존재. → 담당: devops

## P2 — Scale Prep

7. ✅ `CorsLayer::permissive()` 제거 — 해결됨 (`1d4422e`). allow-list 기반 `cors_layer()`로 교체,
   빈 목록이면 `CorsLayer::new()`(교차 출처 차단)로 안전측 기본값.
8. ✅ fleet-api 보안 헤더 — 해결됨 (`1d4422e`). CSP/HSTS/X-Frame-Options 등 6개 적용
   (dashboard에는 이미 적용돼 있었고 fleet-api만 누락된 상태였다).
9. ✅ 구조화된 감사 로그 테이블 — 해결됨 (`b501ca5`). `migrations/011_audit_log.sql` + `/api/audit/auth`
   조회 API. 사용자 생성/토글/삭제·로그인 등에서 기록. `actor_user_id`는 `ON DELETE SET NULL`,
   `actor_label` 별도 보존 — 계정 삭제로 감사 흔적을 지울 수 없게 설계.

   주의: 기존 `/api/audit`는 `events` 테이블(작업·워커 생명주기)을 반환하는 **별개** 엔드포인트다.
10. ⏳ 세션 토큰 로테이션 부재 — 8시간 고정 토큰 (`auth.rs:37 SESSION_DURATION_SECS`). 로그인 시
    1회 발급 후 갱신·연장 경로 없음.
11. ⏳ 페이지네이션 불일치 — `WorkerFilter`에 offset 없음. 추가로 fleet-api `list_workers`가
    `WorkerFilter::default()`를 사용해 **쿼리스트링 필터가 스토어까지 전달되지 않는다** (문서 최초
    기술보다 심각).
12. ⏳ API 오류 응답 포맷 불일치 — fleet-api는 `{error:{code,message}}`, dashboard는 ad-hoc.
    → 담당: coder
13. ⏳ OpenTelemetry / 분산 추적 부재 — `#[instrument]` 스팬 없음.
14. ⏳ 다크 모드, 컬럼 정렬, 고급 필터링 부재.
15. 🟡 시작 시 설정 검증 — 부분 완료. `DATABASE_URL` 존재 여부는 `fleet-cli/src/runtime.rs`에서
    fail-fast 검증하고, fleet-worker는 자체 `validate()`를 갖추고 있다. **잔여**: 비어있음/형식
    검증 없음. 또한 `fleet-core/src/config.rs`의 `OrchestratorConfig`는 검증도 없고 **어디서도
    역직렬화되지 않는 죽은 구조체**다 — 사용하거나 제거할 것.
16. ⏳ 커넥션 풀 튜닝 부재 — `max_connections`만 설정. `acquire_timeout`, `max_lifetime`,
    `idle_timeout` 미설정. → 담당: dbtest
17. ✅ `fleet_list_tasks` MCP 도구 — 해결됨 (`3fb296a`). `schema.rs:34 TOOL_LIST_TASKS`,
    핸들러·테스트 포함.
18. ⏳ 예약된 DB 정리 작업 부재 — 만료된 세션/토큰 미청소. `delete_expired_sessions`는 구현되어
    있으나 **프로덕션 호출자가 없다**(테스트만 호출). `delete_old_login_attempts`는 로그인 성공 시
    기회적으로만 실행된다. → 담당: dbtest
19. ⏳ 마이그레이션 롤백 스크립트 부재 — sqlx 단순(비가역) 모드, down 스크립트 없음.
    → 담당: devops

## P3 — Nice-to-have

20. ✅ Bearer 토큰 상수시간 비교 — 해결됨 (`1d4422e`). 평문 `==` 비교를 `ct_eq` 다이제스트 비교로
    교체 (`subtle`은 이미 워크스페이스에 있었다).
21. ⏳ OpenAPI/Swagger 스펙 부재.
22. ⏳ Dashboard API에 `/v1` 버전 부재.
23. ⏳ 프론트엔드 페이지네이션 UI 부재.
24. ⏳ 모바일 반응형 감사 미수행.
25. ⏳ 서킷 브레이커 상태가 인메모리 전용 (다중 인스턴스 불가).
26. ⏳ 시크릿 매니저 통합 부재 (Vault/AWS SM).
27. 🔵 예시 설정 파일 — **사실상 충족**. `orchestrator.example.toml`은 없으나 `examples/fleet.env`
    (DATABASE_URL, FLEET_HTTP_BIND, FLEET_API_TOKENS 등) + `examples/worker.toml`이 같은 역할을 한다.
    종료 또는 최하위 강등 권고.
28. ⏳ 추가 MCP 도구 (토큰 관리, 호스트 인벤토리, 브레이커 리셋).
29. ⏳ Dashboard API의 하드코딩된 도구 목록 — `handlers.rs:848`이 도구 목록을 하드코딩해
    `fleet-mcp/src/schema.rs`(단일 진실 원천)와 어긋날 수 있다.
30. ✅ CI 커버리지 리포팅 — 해결됨 (`afd8d35`). `.github/workflows/ci.yml`에 `coverage` job 추가
    (`cargo-llvm-cov`, 워크스페이스 전체, Postgres 서비스, lcov 아티팩트 + 잡 로그 요약).
    외부 서비스(Codecov 등) 계정/토큰 연동은 후속 과제.

---

## 현재 진행 상황 (2026-08-01 기준)

### 남은 작업 배정
| 담당 | 항목 |
|---|---|
| coder | #5 잔여(dispatch_latency, http_request_duration), #12 |
| devops | #6, #19 |
| dbtest | #16, #18 |
| 미배정 | #10, #11, #13, #14, #15 잔여, #21~#29 |

### 완료된 기능
- ✅ RBAC 권한 강제 (10개 API 핸들러)
- ✅ 쿠키 세션 인증 (Phase 9.1)
- ✅ 이메일 기반 로그인 + 인증 플로우 / Gmail SMTP 통합
- ✅ 비밀번호 재설정 플로우 (T4) + 재발송 UI (T5)
- ✅ 태스크 API 페이지네이션 offset (T6) / 워커 상세 쿼리 최적화 (T7)
- ✅ 태스크/워커 상세 페이지, 사용자 관리 CRUD UI, SSE 실시간 업데이트
- ✅ mTLS, 서킷 브레이커, ACP 자동 재연결, 헬스체커 + 하트비트
- ✅ 프로비저너 (Ansible 플레이북)
- ✅ Prometheus 메트릭 (7개 패밀리 + task_duration 히스토그램)
- ✅ 컨테이너화 (Dockerfile / docker-compose)
- ✅ 구조화된 감사 로그 (`audit_log`)
- ✅ CI 커버리지 리포팅 (cargo-llvm-cov)

### 테스트 현황
~497개 (`#[test]`/`#[tokio::test]` 함수 기준, 2026-07-31 스냅샷 — 병렬 작업 중이라 수치는 계속
변한다. `grep -rE '^\s*#\[(tokio::)?test\]' crates/`로 재확인 가능). 11개 크레이트 전체에 단위/
통합/E2E 테스트 보유. fleet-store 상세: 55개 (auth_integration.rs 35 + integration.rs 18 +
src/rbac.rs 단위 2).

과거 "~375+" 수치는 신뢰할 수 없었다 — `migrations/004_rbac.sql` 버그로 DB 통합 테스트가
마이그레이션 실패 시 조용히 skip되면서도 `... ok`로 표시되던 상태에서 집계됐다 (#4 참고).
하네스를 하드 실패로 전환한 뒤 재계수한 값이다.

---

## 검증 원칙 (2026-08-01 추가)

이번 라운드에서 **동일 계열 결함이 3건** 나왔다 — *존재하지만 동작하지 않는데 신호는 초록색*:

| 결함 | 표면 신호 | 실제 |
|---|---|---|
| #3 레이트 리미터 | 호출부 grep으로 확인됨 | 카운터가 영구 0, 차단 분기 도달 불가 |
| 004 마이그레이션 | 인덱스 정의 존재 | 모든 Postgres에서 실패, 004~010 미적용 |
| auth 통합 테스트 | CI green, `... ok` 35개 | assert 0회 실행 |

세 건 모두 grep·CI 통과로는 잡히지 않고 **데이터 흐름 추적**으로만 드러났다. 또한 004 수정은
#3 취약점을 **활성화**시키는 관계였다 — 수정 전에는 테이블 부재로 쿼리가 Err → `unwrap_or(false)`
→ 우연히 fail-closed였으나, 수정 후 카운터가 0으로 정상 조회되며 fail-open이 된다. 둘 중 하나만
머지됐다면 취약점이 열렸을 것이다.

**따라서 완료 판정 기준은 "코드/파일이 존재하는가"가 아니라 "실패하도록 만들었을 때 실제로
실패하는가"로 둔다.** 리미터는 차단을 증명하는 테스트로, 마이그레이션은 CI 하드 실패로,
테스트는 실행됐음을 보장하는 하네스로 각각 증명한다.
