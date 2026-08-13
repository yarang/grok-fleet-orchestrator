# Roadmap & Gap Analysis

> 최초 작성 2026-07-28 · **최종 실측 검증 2026-08-01**. 우선순위: **P0** critical, **P1** high, **P2** medium, **P3** low.
>
> 항목 번호는 팀 내 참조 키이므로 **완료되어도 번호를 재사용하거나 삭제하지 않는다** — ✅로 표시하고 원인/커밋을 남긴다.
> 이 문서의 갱신 오너는 planner이며, 단계 완료 시점마다 코드 실측 대조 후 일괄 갱신한다.
> 다른 담당자는 커밋 메시지에 항목 번호(`#7`, `#20`)만 남기면 된다.
>
> 🔐 **보안 백로그는 [`docs/security-findings.md`](../security/findings.md)에 별도 관리**한다.
> 미해결 발견 6건(S1~S6, HIGH 3건 포함)이 등록되어 있으며, 각 항목은 재확인 명령·
> 악용 시나리오·수정 방향·회귀 테스트 방침을 포함한다. 아래 #33 참조.

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

5. ✅ **지연 시간/처리량 메트릭** — 해결됨. `fleet_task_duration_seconds`(`b501ca5`) +
   `fleet_http_request_duration_seconds`(`8755c0d`) 히스토그램 구현
   (`_bucket{le=}`/`_sum`/`_count` 정상 출력).

   **`dispatch_latency`는 스키마 변경이 선행되어야 해 항목 #31로 분리했다** — 이 항목에서
   누락시키지 않기 위해 별도 번호를 부여한다.

   참고: prometheus 크레이트 의존성이 없고 `metrics.rs`가 텍스트를 직접 생성하는 방식이라
   히스토그램 버킷을 수작업 구현했다.

6. ✅ **DB 백업 스크립트** — 해결됨 (`853a1d0`). `scripts/db-backup.sh`(pg_dump 자동화),
   `scripts/db-restore.sh`(복구), systemd 타이머 포함.

## P2 — Scale Prep

7. ✅ `CorsLayer::permissive()` 제거 — 해결됨 (`1d4422e`). allow-list 기반 `cors_layer()`로 교체,
   빈 목록이면 `CorsLayer::new()`(교차 출처 차단)로 안전측 기본값.
8. ✅ fleet-api 보안 헤더 — 해결됨 (`1d4422e`). CSP/HSTS/X-Frame-Options 등 6개 적용
   (dashboard에는 이미 적용돼 있었고 fleet-api만 누락된 상태였다).
9. ✅ 구조화된 감사 로그 테이블 — 해결됨 (`b501ca5`). `migrations/011_audit_log.sql` + `/api/audit`
   조회 API. 사용자 생성/토글/삭제·로그인 등에서 기록. `actor_user_id`는 `ON DELETE SET NULL`,
   `actor_label` 별도 보존 — 계정 삭제로 감사 흔적을 지울 수 없게 설계.

   경로 정리: 예전에는 `/api/audit`가 `events` 테이블(작업·워커 생명주기)을 `/api/events`와
   중복 제공해 이름이 혼동됐다. 중복 핸들러를 제거하고 `/api/audit`는 인증/권한 감사 전용,
   작업·워커 이벤트는 `/api/events` 전용으로 분리했다.
10. ✅ 세션 토큰 로테이션 — 해결됨 (`0177e56`). 30분 단위 지수 로테이션 및 30초의 동시 병렬 요청
    유예기간(grace period)을 두어 세션 쿠키를 자율적으로 로테이션 수행.
11. ✅ 페이지네이션 불일치 및 필터 전달 수정 — 해결됨 (`7e17558`). 쿼리스트링 라벨 필터의 접두사
    `label_` 탈거 누락 버그 해결 및 limit/offset 페이지네이션 매개변수를 Postgres Store까지 전달 연동.
12. ✅ API 오류 응답 포맷 통일 — 해결됨 (`8755c0d`). dashboard도 `ApiError`로 일원화해
    `{error:{code,message}}` 형식을 공유한다. 500번대는 내부 상세를 응답에 노출하지 않고
    서버 로그에만 남긴다.
13. ✅ OpenTelemetry / 분산 추적 부재 — 해결됨. OTLP gRPC(Tonic) 연동 및 SdkTracerProvider 조건부 초기화 구현 완료, `submit`/`cancel`/`register`/`heartbeat` 등 핵심 오케스트레이션 라이프사이클에 `#[instrument]` 스팬 완벽 장착.
14. ⏳ 다크 모드, 컬럼 정렬, 고급 필터링 부재.
15. ✅ 시작 시 설정 검증 및 데드코드 정리 — 해결됨 (`0177e56`). `DATABASE_URL`, `FLEET_BASE_URL`
    등의 형식 검증 및 `OrchestratorConfig` 등 레거시 데드코드 구조체 일괄 소거 완료.
16. ✅ 커넥션 풀 튜닝 — 해결됨 (`bac4dc7`). `PoolConfig`로 `acquire_timeout`(30s),
    `max_lifetime`(30m), `idle_timeout`(10m) 설정. 장수명 서버 프로세스(`fleet serve`)에서
    sqlx 기본값을 그대로 쓰던 문제 해소.
17. ✅ `fleet_list_tasks` MCP 도구 — 해결됨 (`3fb296a`). `schema.rs:34 TOOL_LIST_TASKS`,
    핸들러·테스트 포함.
18. ✅ 예약된 DB 정리 작업 — 해결됨 (`bac4dc7`). `fleet-scheduler/src/cleanup.rs`의
    `SessionCleanup`이 `runtime.rs`에서 `tokio::spawn`으로 기동되어 주기적으로 만료 세션과
    오래된 로그인 시도를 정리한다(`--no-cleanup`으로 비활성화, 주기·보존기간 설정 가능).
    이전에는 `delete_expired_sessions`에 프로덕션 호출자가 아예 없었다.
19. ✅ 마이그레이션 롤백 — 해결됨 (`853a1d0`). `scripts/db-migrate-safe.sh`로 세이프 롤백 제공.

## P3 — Nice-to-have

20. ✅ Bearer 토큰 상수시간 비교 — 해결됨 (`1d4422e`). 평문 `==` 비교를 `ct_eq` 다이제스트 비교로
    교체 (`subtle`은 이미 워크스페이스에 있었다).
21. ✅ **OpenAPI/Swagger 스펙 부재** — 해결됨 (2026-08-13). `fleet-api`의
    `/v1` HTTP API(워커 등록/조인/하트비트/목록/삭제, 자격 증명 CRUD,
    부트스트랩 토큰 CRUD, 호스트 등록, `/metrics`) 전체를 다루는 OpenAPI
    3.0.3 스펙을 `crates/fleet-api/src/openapi.yaml`에 손으로 작성했습니다
    (`schema.rs`/`handlers.rs`/`app.rs`를 직접 대조해 작성 — 자동 생성 아님).
    `GET /openapi.yaml`로 서빙되며 `/metrics`와 동일하게 인증 미들웨어
    바깥에 있어 토큰 없이 Swagger UI 등에 URL을 바로 넘길 수 있습니다.

    작성 과정에서 `docs/architecture/api-reference.md`에 아예 빠져 있던
    자격 증명 엔드포인트 4종(`PUT/GET /v1/workers/:name/credentials`,
    `GET .../export`, `DELETE .../:model_id`)도 발견해 스펙에 포함했습니다 —
    즉 이 OpenAPI 스펙이 기존 산문 문서보다 커버리지가 더 넓습니다.

    **범위 제한**: 대시보드 API(`/api/*`, `fleet-dashboard` 크레이트, ~30개
    라우트)는 별도 훨씬 큰 표면이라 이번 스펙 범위 밖입니다 — 필요 시
    후속 항목으로 분리 권장.

    신규 테스트 2개: YAML 구문 유효성 + 알려진 경로 전부 존재 확인(파싱
    기반, 오타 회귀 방지), `/openapi.yaml`이 토큰 설정 상태에서도 인증
    없이 200을 반환하는지 확인.

    ✅ **검증 완료**: `cargo build --release --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets
    --all-features`(경고 0건), `cargo test --workspace --features "acp mtls"`
    (전체 그린) 통과.
22. ⏳ Dashboard API에 `/v1` 버전 부재.
23. ⏳ 프론트엔드 페이지네이션 UI 부재.
24. ⏳ 모바일 반응형 감사 미수행.
25. ✅ **서킷 브레이커 상태가 인메모리 전용 (다중 인스턴스 불가)** — 해결됨
    (2026-08-13). 서술 자체가 부정확했습니다 — `worker.circuit_state` DB 컬럼
    영속화(`PgStore::update_worker_circuit_state`, `dispatcher.rs` 4곳에서 호출)와
    Postgres LISTEN/NOTIFY 기반 인스턴스 간 실시간 동기화 코디네이터
    (`fleet-scheduler/src/sync.rs`의 `MultiAdminSync`)는 **이미 오래전에 구현·
    테스트까지 완료**돼 있었습니다(단위 테스트 4개 + 스케일아웃 통합 테스트
    `test_circuit_breaker_sync_between_scaleout_nodes`). 문제는 다른 곳에
    있었습니다 — **`MultiAdminSync`가 실제 `fleet serve` 기동 경로
    (`crates/fleet-cli/src/runtime.rs`)에는 한 번도 연결(`spawn`)되지 않아서**,
    구현·테스트는 그린인데 실배포에서는 죽은 코드였습니다(Autonomic Engine과
    같은 유형의 "구현은 됐지만 배선이 안 된" 결함).

    수정: `run_serve`가 `HealthChecker`/`Reconciler`/`SessionCleanup`을 기동하는
    자리에 `MultiAdminSync::new(state.clone(), store.pool().clone()).run()`을
    함께 `tokio::spawn`하도록 연결했습니다. 형제 background 루프들과 동일한
    옵트아웃 컨벤션을 따라 신규 CLI 플래그 `--no-circuit-sync`(기본값: 비활성 —
    즉 동기화는 기본 켜짐)를 추가했습니다. 단일 인스턴스 배포에서는 자신이
    발행한 이벤트를 다시 받아 멱등하게 재적용할 뿐이라 켜둬도 무해합니다.
    상세 배경은 [`docs/architecture/overview.md`](../architecture/overview.md)
    "PostgreSQL LISTEN/NOTIFY로 다중 admin 동기화" 절의 정정 참고.

    ✅ **검증 완료**: `cargo check --no-default-features`,
    `cargo clippy --all-targets --all-features`(경고 0건),
    `cargo test --workspace --features "acp mtls"`(전체 그린) 통과.
26. ⏳ 시크릿 매니저 통합 부재 (Vault/AWS SM).
27. 🔵 예시 설정 파일 — **사실상 충족**. `orchestrator.example.toml`은 없으나 `examples/fleet.env`
    (DATABASE_URL, FLEET_HTTP_BIND, FLEET_API_TOKENS 등) + `examples/worker.toml`이 같은 역할을 한다.
    종료 또는 최하위 강등 권고.
28. ✅ **추가 MCP 도구 (토큰 관리, 호스트 인벤토리, 브레이커 리셋)** — 해결됨
    (2026-08-13). `crates/fleet-mcp/src/schema.rs`/`handlers.rs`에 신규 도구
    4종을 추가해 8개 → 12개로 늘렸습니다: `fleet_list_hosts`(호스트 인벤토리
    조회, 상태 필터 지원), `fleet_reset_worker_breaker`(worker_id 또는
    worker_name으로 CircuitBreaker 강제 리셋 — store 영속화 +
    `WorkerCircuitChanged` 이벤트 발행으로 #25에서 새로 연결한
    `MultiAdminSync`를 통해 다른 인스턴스에도 전파됨), `fleet_list_bootstrap_tokens`,
    `fleet_revoke_bootstrap_token`. 네 도구 모두 기존 Store trait 메서드만
    재사용했고(`list_hosts`, `update_worker_circuit_state`,
    `list_bootstrap_tokens`, `revoke_bootstrap_token`) 새 백엔드 로직은
    없습니다 — 이미 대시보드 HTTP API에만 있던 기능을 MCP 클라이언트에도
    노출한 것입니다. 토큰 **발급**(create)은 의도적으로 범위에서 제외했습니다
    (fleet-cli/HTTP API 전용 유지).

    이 참에 `fleet-mcp`가 그동안 `handle_*` 핸들러를 단위 수준에서 전혀
    테스트하지 않고 있었음(`cross_client.rs`의 `DATABASE_URL` 게이트 서브프로세스
    테스트로만 커버)을 발견해, `fleet_store::mem::MemStore`(#45)로 실제
    `ToolContext`를 구성하는 신규 도구 4종의 유닛 테스트 10개를 추가했습니다.

    ✅ **검증 완료**: `cargo build --release --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets
    --all-features`(경고 0건), `cargo test --workspace --features "acp mtls"`
    (전체 그린) 통과.
29. ✅ Dashboard API의 하드코딩된 도구 목록 — 해결됨 (`8755c0d`). `list_tools_api`가
    `fleet_mcp::schema::all_tools()`를 그대로 노출해 단일 진실 원천을 따른다.
    (별도 보고 없이 #12 작업에 포함되어 반영됐다 — 실측 대조 중 확인.)
30. ✅ CI 커버리지 리포팅 — 해결됨 (`afd8d35`). `.github/workflows/ci.yml`에 `coverage` job 추가
    (`cargo-llvm-cov`, 워크스페이스 전체, Postgres 서비스, lcov 아티팩트 + 잡 로그 요약).
    외부 서비스(Codecov 등) 계정/토큰 연동은 후속 과제.

## 신규 항목 (2026-08-01 추가)

31. ✅ **`dispatch_latency` 메트릭** (P2) — 해결됨 (`ed82b27`). `tasks` 테이블에 `dispatched_at` 컬럼을 추가하는 마이그레이션(`012_task_dispatch_latency.sql`)을 진행하고, 스케줄러 디스패치 시점에 갱신하도록 처리. 대기 시간차를 계산하여 Prometheus Histogram `fleet_task_dispatch_latency_seconds` 메트릭으로 노출 완료.

32. ✅ **`/admin/*` HTML 페이지에 RBAC 검사 부재** (P2, 보안) — 해결됨 (`db614ec`).

    `admin_activity_page`(당시 `admin_audit_page`), `admin_users_page`, `admin_tools_page` 세 핸들러 모두 인자 없이
    `serve_page(...)`만 호출하며 `require_permission` 검사가 없다. **`/admin/activity`(당시 `/admin/audit`) 한 곳의
    문제가 아니라 관리자 페이지 전반의 동일 패턴이다** — 한 곳만 고치면 나머지 두 곳이 그대로
    남는다.

    **정확한 노출 범위 (과대평가 주의)**: 세 라우트 모두 `protected` 라우터에 속해
    `require_session`은 적용된다. 즉 **미인증 접근은 불가**하다. 또한 실제 데이터를 제공하는
    API는 권한을 강제한다 — `/api/audit`는 `require_permission(AuditRead)`,
    `/api/users`는 `UserRead`를 검사한다. 따라서 권한 없는 인증 사용자가 얻는 것은
    **API 호출이 403으로 막히는 빈 HTML 셸**이며 데이터 유출은 아니다.

    → 심각도는 *데이터 노출*이 아니라 **심층 방어 결여 + 관리 UI 구조 노출**이다. 다만 페이지
    계층에 검사가 없다는 사실 자체가, 향후 어떤 페이지가 데이터를 인라인으로 렌더링하기
    시작하면 즉시 실제 취약점으로 바뀌는 구조다. 세 페이지를 **일괄** 수정할 것.

    ✅ 해결됨 (`db614ec`). 지적된 3개 페이지에 더해 `admin_ssh_keys_page`·`provision_page`까지
    5개를 `serve_page_if_permitted`로 일괄 처리했다. 작업 중 **실제 데이터 유출 1건**을 함께
    발견·수정했다 — `/api/events/stream`(SSE)에 권한 검사가 전무해 `task:output` 권한이 없는
    `Viewer`가 REST(`get_task_detail_api`)에서 막힌 작업 stdout/stderr를 받아갈 수 있었다.
    이후 동일 데이터의 폴링 경로(`GET /api/events`)에도 같은 누락이 있음이 드러나
    `event_view.rs` 공통 필터로 통합했다 (`80acf3c`).

33. ✅ **미해결 보안 발견 6건 (S1~S6) 해결 완료** (P1, 보안) — → 담당: security
    
    상세: [`docs/security-findings.md`](../security/findings.md). 2026-08-06에 6대 보안 결함 전체를 해결 완료했습니다.

    * **S1 (락아웃 증폭)**: 차단 분기 내의 `record_login_failure` 호출을 제거해 계정 영구 잠금 취약점을 해소했습니다.
    * **S2 (JWT 서명 미검증)**: `jsonwebtoken`을 도입해 JWKS(certs) 기반 RS256 서명, `iss`, `aud`, `exp`를 정상 검증하도록 고도화하고 로컬 테스트 모킹용 헬퍼를 추가했습니다.
    * **S3 (프록시 IP 루프백)**: `FLEET_TRUSTED_PROXIES` allow-list를 활용해 프록시 뒤의 Real Client IP를 정확히 역추출하는 `extract_client_ip`를 미들웨어 및 5개 핸들러에 적용했습니다.
    * **S4 (IP 실패 엔드포인트 잠식)**: `count_recent_ip_failures` SQL에 필터를 넣어 비밀번호 재설정(`forgot_password`) 및 이메일 재발송(`resend_verification`) 로그의 가짜 실패 이력을 IP 예산 카운트에서 제외시켰습니다.
    * **S5 (clear_login_attempts NULL 지뢰)**: `$2::text IS NULL OR ...`로 안전화하고, 로그인 성공 시 특정 IP만이 아닌 계정 전체의 IP 실패 기록을 일괄 초기화하도록 변경했습니다.
    * **S6 (60초 메일 제한)**: 메일 발송 엔드포인트에 1시간당 최대 3회 발송만 허용하는 별도의 `check_rate_limit_custom` 로직을 추가하여 이메일 폭탄 취약점을 해결했습니다.

34. ✅ **liteLLM 중앙 게이트웨이 통합 및 연동** (P2, LLM 인프라) — 해결됨 (`cbbea58`).
    
    상세: [`docs/llm-wiki/README.md`](../llm-wiki/README.md). 오케스트레이터의 멀티 LLM 공급자 연동과 Spend Control(비용 추적/한도)을 제어하기 위해 liteLLM 프록시 서비스를 Docker Compose에 수용하고, 시작 시 환경변수(`FLEET_LLM_GATEWAY_URL`) Fail-Fast 설정을 추가합니다.

    **문서 정합성 수정 (2026-08-06~07)**: [`docs/single_server_deployment_plan.md`](../deployment/single-server.md)가 이 결정 이전에 작성되어 "One API(경량 대안)를 liteLLM 대신 채택"이라고 반대로 기술하고 있던 충돌을 발견했다. liteLLM 기준(포트 4000, `ghcr.io/berriai/litellm`, `litellm` 전용 논리 DB, `FLEET_LLM_GATEWAY_URL`)으로 갱신해 모순을 해소하는 한편, 재발 방지를 위해 `docs/llm-wiki/`를 게이트웨이 결정·스펙의 **정본(canonical source)**으로 격상했다 — `multi_provider_llm_proxy_analysis.md`에 liteLLM vs One API 비교표를 추가해 선택 근거를 명문화하고, `single_server_deployment_plan.md`의 Docker Compose 예시는 이제 그 정본을 인용한 사본임을 명시했다. 이어서 [Karpathy의 "LLM Wiki" 패턴](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f)에 맞춰 `docs/llm-wiki/index.md`(페이지 카탈로그)와 `docs/llm-wiki/log.md`(ingest/query/lint append-only 이력)를 신설하고, `README.md`를 운영 규칙(스키마) 문서로 재편했다 — 상세 이력은 `log.md` 참고.
    
    **구현 완료 (2026-08-07)**: `crates/fleet-core/src/config.rs`에 `FLEET_LLM_GATEWAY_URL`의 유효한 HTTP/HTTPS URL 스킴 검증 필터를 추가하고 단위 테스트 2개를 작성하여 무결성을 확보했습니다. 또한 `docker-compose.yml`에 포트 4000번 기반의 `litellm` 컨테이너 서비스를 추가하고, 오케스트레이터 서비스가 기동 시 이를 자동 바인딩하도록 연동시켰으며, `examples/litellm-config.yaml` 템플릿 설정을 구축했습니다.

## 신규 항목 (2026-08-11 추가)

35. ✅ **docs 디렉토리 내 다이어그램 Mermaid 변환 및 문서 정합성 개선** (P2, 문서화) — 해결됨 (2026-08-11). docs 내 ASCII 다이어그램 16개를 Mermaid로 마이그레이션하고, `/api-gateway/` Nginx 설정 누락 해결 및 `single-server.md` 내 폐기된 Docker 스펙 정리 완료.
36. ✅ **mTLS 인증서 자동 회전(Auto-Rotation) 정책 도입** (P1/P2, 보안/운영) —
    해결됨 (2026-08-13). 실측해보니 오케스트레이터(클라이언트) 쪽은 이미
    매 핸드셰이크마다 파일을 다시 읽고 있어(`ClientTlsConfig`) 문제가
    없었고, 진짜 격차는 fleet-worker의 `MtlsProxy`(서버 역할)가 기동 시
    한 번만 인증서를 읽고 이후 변경을 절대 반영하지 않는다는 점이었습니다
    — 이 절반만 실제로 고쳤습니다.

    `crates/fleet-transport/src/tls.rs`에 `RotatingCertResolver`를
    추가했습니다 — rustls `ResolvesServerCert`를 구현해 매 핸드셰이크마다
    캐시된 `CertifiedKey`(Arc)만 반환하고(디스크 I/O 없음), 별도 백그라운드
    루프가 `reload()`를 호출할 때만 캐시를 교체합니다. `ServerTlsConfig::
    build_rotating_server_config()`가 이 리졸버로 구성된 `ServerConfig`를
    만듭니다. `fleet-worker`의 `worker.toml` `[mtls]` 섹션에 신규 필드
    `cert_reload_interval_secs`(초, 미지정/0이면 기존처럼 정적 로드 — 하위
    호환 기본값)를 추가하고, `runner.rs`가 설정된 경우 백그라운드 재적재
    루프를 spawn하도록 연결했습니다. 재적재 실패 시 **기존 캐시를 그대로
    유지**하고 에러만 로깅합니다 — 잘못된(또는 원자적 교체 도중 반만 쓰인)
    파일 때문에 서비스가 끊기지 않도록("서비스 중단 없이 교체"가 이 항목의
    핵심 요구사항).

    **의도적 범위 제한**: 클라이언트 CA(`client_ca_path`)는 회전 대상이
    아닙니다 — CA 교체는 사설 PKI 전체를 다시 구성하는 것과 같은 무게의
    작업이라 프로세스 재시작으로 처리합니다. CRL/OCSP도 여전히 미지원입니다
    (기존에 알려진 별도 제한, 이번 항목의 범위 밖). `fleet provision`
    인벤토리/CLI를 통한 자동 설정 배선은 하지 않았습니다 — `worker.toml`을
    직접 편집해 설정해야 합니다(#37과 달리 "배선 자체가 요청 사항"은
    아니었으므로 범위를 좁혔습니다).

    **실측 검증**: `crates/fleet-transport/tests/mtls_proxy.rs`에 엔드투엔드
    테스트 2개를 추가했습니다 — (1) 같은 CA로 서명된 서로 다른 서버 인증서
    두 개를 준비해, 실제 러닝 프록시에 연결→`reload()`→재연결하며 클라이언트가
    받는 인증서의 raw DER 바이트가 회전 전/후 실제로 바뀌는지 확인(단순히
    "에러 없이 리턴"보다 훨씬 강한 증거), (2) 손상된 인증서 파일로
    `reload()`가 실패해도 에러만 반환하고 패닉하지 않는지 확인.

    ✅ **검증 완료**: `cargo build --release --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets
    --all-features`(경고 0건), `cargo test --workspace --features "acp mtls"`
    (전체 그린, 신규 mTLS 테스트 2개 포함) 통과.
37. ✅ **인벤토리 기반 mTLS 프로비저닝 자동화 지원** (P2, 인프라) — 해결됨
    (2026-08-13). `InventoryDefaults`/`InventoryWorker`(`crates/fleet-provisioner/src/inventory.rs`)에
    mTLS 필드를 추가했습니다. 여러 워커가 공유할 만한 값(`mtls_enabled`,
    `mtls_listen_addr`, `mtls_client_ca`, `mtls_advertised_port`)은
    `defaults:`에, 워커마다 고유해야 하는 서버 인증서/키
    (`mtls_server_cert`/`mtls_server_key` — `fleet mtls issue-server`로 사전
    발급된 원격 경로만 참조, 파일 자체는 업로드하지 않음)는 개별 워커에만
    두도록 설계했습니다. `mtls_advertised_host`를 생략하면 워커 `name`으로
    자동 폴백합니다. `effective_mtls_*` 헬퍼로 개별값→defaults 우선순위를
    해석하고, `fleet-cli::runtime::build_inventory_step_context`가 이를
    `StepContext`에 주입하도록 연결했습니다.

    필드 누락 검증은 새로 만들지 않고 기존 템플릿 렌더링 단계
    (`templates.rs`의 `StepError::Template("mtls_enabled=true requires ...")`)를
    그대로 재사용합니다 — 단일 호스트 모드(`--host`)와 완전히 동일한 에러
    경로이므로 중복 검증 로직이 없습니다. `examples/workers.yaml`과
    `docs/architecture/overview.md`의 "mTLS 제한" 절을 갱신했습니다.

    신규 테스트 5개(공유 defaults + per-worker cert 파싱, advertised_host
    name 폴백/명시적 오버라이드, per-worker mtls_enabled가 defaults를
    오버라이드하는 케이스 등).

    ✅ **검증 완료**: `cargo build --release --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets
    --all-features`(경고 0건), `cargo test --workspace --features "acp mtls"`
    (전체 그린) 통과.
38. ✅ **스케줄러 작업 실패 시 자동 재시도 및 Dead Letter Queue (DLQ) 설계** (P2, 안정성) —
    해결됨 (2026-08-13). 사용자에게 3가지 구현 범위(AskUserQuestion)를 제시했고
    **"전체 구현"**이 채택됐습니다: `tasks.retry_count` 컬럼 추가
    (`014_task_retry.sql`), `ReconcileConfig::max_dispatch_retries`(기본 20) +
    `Dispatcher::with_max_dispatch_retries`, CLI
    `--reconcile-max-dispatch-retries`/`FLEET_RECONCILE_MAX_DISPATCH_RETRIES`
    (기본 20 — 재시도 기본 ON).

    `FailureKind::WorkerUnavailable`/`CircuitOpen`만 재시도 대상으로 삼습니다
    (`WorkerError`/`AuthFailed`는 제외 — 실제 dispatch/transport 에러는 재시도해도
    같은 결과가 반복될 가능성이 높아 기존과 동일하게 즉시 `Failed`). 재시도가
    활성화된 경우(`max_dispatch_retries > 0`) **`submit()`의 API 계약이
    바뀝니다**: 위 두 실패 유형에서 더 이상 즉시 `Err`를 반환하지 않고
    `retry_count`를 1 올린 뒤 작업을 `Pending`으로 남겨둔 채 `Ok(task_id)`를
    반환합니다 — 실제 재시도는 `Reconciler`의 stale-`Pending` 스윕이
    백그라운드에서 이어받습니다. `retry_count`가 상한에 도달하면 새 DLQ
    테이블 없이 기존 `Failed` 상태를 dead-letter로 재사용해 전이시킵니다
    (`ReconcileSummary::dead_lettered`로 집계). `max_dispatch_retries == 0`이면
    이 기능 도입 이전과 완전히 동일하게 동작합니다(하위 호환).

    `crates/fleet-scheduler/src/dispatcher.rs`(`Dispatcher::submit`),
    `crates/fleet-scheduler/src/reconcile.rs`(`Reconciler::reconcile_once`의
    dead-letter 분기), `crates/fleet-cli/src/{main.rs,runtime.rs}`(CLI 플래그
    배선, `Dispatcher`/`Reconciler`가 동일한 상한값을 공유하도록 연결)을
    수정했습니다. `docs/architecture/overview.md` §7에 "자동 재시도 및 Dead
    Letter" 절을 추가하고, 마이그레이션 개수 서술(13→14)을 갱신했습니다.

    구현 도중 발견한 부수 버그도 같이 고쳤습니다: `fleet_dispatch_task`(MCP,
    `fleet-mcp/src/handlers.rs`)와 `POST /api/tasks`(대시보드,
    `fleet-dashboard/src/handlers.rs`)가 `submit()`이 `Ok`이면 무조건
    `status: "dispatched"`/`dispatched: true`를 하드코딩해 응답하고 있었는데,
    이 변경 이후에는 재시도가 예약된 `Pending` 작업도 `Ok`를 반환하므로 그
    응답이 거짓이 됩니다 — 실제 태스크 상태를 조회해 정직하게 보고하도록
    수정하고, `docs/architecture/api-reference.md`의 `fleet_dispatch_task`
    출력 설명도 함께 정정했습니다.

    신규 테스트 6개: `Dispatcher::submit()` 재시도 비활성/활성 각 1건,
    `Reconciler`의 재시도 소진 dead-letter 1건, 상한 미만 시 정상 재시도 1건,
    `fleet-store`의 `increment_task_retry_count` 영속성/NotFound 각 1건
    (실제 Postgres로 검증).

    ✅ **검증 완료**: `cargo build --release --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets
    --all-features`(경고 0건, 벤더 코드 제외), `cargo test --workspace
    --features "acp mtls" -- --test-threads=1`(`DATABASE_URL`을 실제 Postgres
    `fleet_test`로 지정, 전체 그린) 통과.
39. ✅ **Known Hosts TOFU 모드에서의 대규모 인프라 배포 절차 상 보안 공백 보완** (P2, 보안) —
    해결됨 (2026-08-13). 신규 `fleet scan-host-keys` 명령을 추가했습니다 —
    `ssh-keyscan`과 동일한 목적으로, 실제 인증 없이 서버가 제시하는 SSH 호스트
    공개키만 수집합니다(`fleet-provisioner::ssh::scan_host_key` — `check_server_key`
    콜백에서 키를 캡처한 뒤 `Ok(false)`로 즉시 handshake를 종료시켜, 개인키·
    사용자 계정 없이도 키를 얻습니다). `--host <addr>` 단일 호스트 또는
    `--inventory <file>` 일괄 스캔을 지원하고, 기본 동작은 지문(SHA-256) 출력만
    — `--write`를 명시해야 `known_hosts`에 반영됩니다(대역 밖 검증을 건너뛰기
    어렵게 하려는 의도적 설계). 파일 append 로직(`append_known_hosts_line`)은
    `ssh` 카고 피처와 무관하게 항상 컴파일되는 순수 파일 I/O이며, 기존
    `russh_keys::learn_known_hosts_path`와 동일한 줄 형식을 생성해 100% 호환됩니다.

    **실측 검증**: `fleet scan-host-keys --host github.com`을 실행해 GitHub가
    공식 문서에 게시한 ed25519 지문(`SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU`)과
    **정확히 일치**함을 확인했습니다 — 실제 네트워크 경로(handshake→키 캡처→
    지문 계산)가 전부 올바르게 동작함을 실제 공인 SSH 서버로 검증한 것입니다.
    `--write` 모드로 known_hosts 파일에 기록된 내용도 GitHub 공식 문서의
    `known_hosts` 항목과 바이트 단위로 일치함을 확인했습니다.

    `docs/architecture/overview.md`("SSH 호스트 키 검증" 절)와
    `docs/worker-bootstrap/bootstrap-release-v0.2.md`를 갱신해 기존 외부
    `ssh-keyscan` 의존 예시를 신규 내장 명령으로 교체했습니다.

    ✅ **검증 완료**: `cargo build --release --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets
    --all-features`(경고 0건), `cargo test --workspace --features "acp mtls"`
    (전체 그린) 통과 + 위 실제 네트워크 스모크 테스트.
40. 🔵 **`xai-circuit-breaker` 기반 고성능 회로 차단기 도입** (P2, 성능/안정성) —
    **2026-08-13 재평가: 항목이 요구하는 3가지 중 2가지는 이미 구현돼 있었습니다.**
    (1) 슬라이딩 윈도우 실패율 측정 — `BreakerInner.samples: VecDeque<(bool,
    Instant)>` + `window_duration_secs`/`min_samples`/`error_rate_threshold`로
    이미 존재(`crates/fleet-scheduler/src/breaker.rs`). (2) Lost Probe 캔슬
    안전장치 — 정확히 `probe_claimed_at_millis`라는 이름은 아니지만
    `half_open_probes: VecDeque<Instant>` + `open_duration_secs` 경과 시
    `check()`가 스스로 회수하는 동일한 메커니즘을 항목 #44에서 구현·테스트
    완료(2026-08-13).

    남은 것은 (3) `AtomicU8`/`AtomicBool` 기반 lock-free `is_open()` 핫패스
    최적화뿐입니다. 실측 없이 우선순위를 낮춰 남겨둡니다 — 현재도 워커별
    `Mutex<BreakerInner>`로 세분화(fine-grained)돼 있어 단일 전역 락 병목이
    아니며, 한 워커당 동시 dispatch 수는 `max_concurrent`로 이미 상한이 걸려
    있어 lock 경합이 실제 병목이라는 프로파일링 근거가 없습니다. 근거 없이
    lock-free 재작성부터 하면 검증 難도만 올라갑니다 — 실측(프로파일링)으로
    경합이 확인되면 그때 착수 권고.
41. ✅ **WebSocket Demuxer 패턴을 적용한 동시 다중 세션 고도화** (P2, 네트워크) — 해결됨
    (2026-08-13). `xai-computer-hub-sdk` 분석에 근거해 단일 WebSocket 연결 상에서 ACP
    프롬프트 세션의 순서 보장 및 Head-of-Line Blocking 방지를 위한 RPC Frame
    Demultiplexer 구현.

    **2026-08-12 정정**: 이 항목이 서술하는 "단일 WebSocket 연결 위에서 여러 ACP 세션을
    다중화" 구조는 이미 구현되어 있었습니다 — `crates/fleet-transport/src/acp_transport.rs`가
    2026-08-11 SDK 마이그레이션으로 태스크당 세션(`sessions: Arc<Mutex<HashMap<SessionId,
    InFlightSession>>>`)을 `SessionId` 기준으로 라우팅하며, `acp_concurrent.rs`의 3개
    테스트로 검증됨(`docs/architecture/overview.md §동시 실행` 참조). 이 항목이 실제로
    추가로 필요했던 것은 **Head-of-Line Blocking 방지**(단일 WS 커넥션 위에서 순차 프레임
    처리이므로, 한 세션의 handler 처리가 느려지면 같은 연결의 다른 세션 notification
    배달까지 지연될 위험)였습니다.

    **2026-08-13 구현**: vendor SDK(`agent-client-protocol-rust-sdk`)의
    `incoming_protocol_actor`가 연결 하나당 단일 순차 루프로 `on_receive_notification`
    핸들러를 인라인 `.await`한다는 것을 소스 대조로 확인했습니다(vendor 코드라 패치
    대상이 아님) — 즉 우리 쪽 핸들러가 조금이라도 느려지면 같은 연결의 다른 세션까지
    지연됩니다. 세션마다 전용 알림 큐(`InFlightSession.notify_tx:
    mpsc::UnboundedSender<SessionMsg>`)를 두어, `on_receive_notification`은
    non-blocking send만 하고 즉시 반환하도록 하고, 실제 처리(버퍼 append + seq 증가 +
    `WorkerEvent::Output` 브로드캐스트)는 세션마다 spawn되는 전용 워커 태스크로
    위임했습니다 — 단일 컨슈머(FIFO)라 세션 내부 순서는 보존되고, 세션마다 독립된
    태스크라 세션 간 지연 전파가 구조적으로 차단됩니다. `dispatch()`가 최종
    `TaskResult.output`을 읽기 전에는 `SessionMsg::Flush` 배리어로 "워커가 큐잉된
    모든 청크를 실제로 처리 완료했는지"를 확인합니다(채널에 들어감 ≠ 처리 완료).

    신규 테스트 2개(`acp_concurrent.rs`):
    `dispatch_accumulates_multiple_chunks_in_order`(단일 세션 8청크, Flush 배리어
    검증), `concurrent_sessions_streaming_multiple_chunks_do_not_cross_contaminate`
    (동시 4세션 x 각 5청크, 순서 보존 + 세션 간 무혼선 검증). 상세 배경은
    [`docs/architecture/overview.md`](../architecture/overview.md) "Head-of-Line
    Blocking 방지" 절 참고.

    ✅ **검증 완료**: `cargo build --release --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets
    --all-features`(경고 0건, 벤더 코드 제외), `cargo test --workspace
    --features "acp mtls" -- --test-threads=1`(`DATABASE_URL`을 실제 Postgres
    `fleet_test`로 지정, 전체 그린) 통과.

42. ⏳ **워커 노드 연동 분산 OTLP Tracing Context Propagation 구축** (P2, 모니터링) — `xai-tracing` 기법을 차용해 오케스트레이터와 `fleet-worker` 간 WebSocket 통신 시 `traceparent` 스팬 캐리어를 전파하여 E2E 분산 추적 시각화 완성.

## 신규 항목 (2026-08-12 추가 — `docs/` 전체 코드 대조 검증 중 발견)

43. ⚫ **Autonomic Self-Healing Engine (MAPE-K) — 삭제됨** (2026-08-13) —
    `crates/fleet-scheduler/src/autonomic.rs`(172줄)가 컴파일되지 않는 미완성 상태로
    방치되어 있었습니다(`Worker.metrics` — 애초에 존재하지 않는 필드, `FleetEvent::
    WorkerLeft`/`BreakerRegistry::get` 시그니처 불일치 등). 재연결을 검토한 결과 단순
    타입 수정이 아니라 **하드웨어 메트릭을 어디에 저장할지부터 다시 설계해야 하는
    별도 기능 개발**(예: `hosts` 테이블 join, 또는 `Worker`에 필드 추가)이 필요했고,
    온도/스톨 감지 로직 자체도 코드 주석이 스스로 "시뮬레이션"이라 밝힌 자리표시자여서
    당장 재연결해도 실질적 가치가 없었습니다. 파일을 삭제하고 `lib.rs`/`runtime.rs`의
    주석 처리된 배선 코드도 함께 정리했습니다. **설계 의도는
    [`docs/architecture/overview.md`](../architecture/overview.md)의 "Autonomic
    Self-Healing Engine" 절에 보존**했고, 원본 구현은 이 삭제 이전 git 이력에서
    복원 가능합니다. 재구현하려면: (1) 워커별 하드웨어 메트릭 저장 위치 결정,
    (2) `FleetEvent::WorkerLeft{worker_id, reason, at}` 시그니처로 재작성,
    (3) `BreakerRegistry::get(worker_id, initial_state)` 시그니처로 재작성,
    (4) 온도/스톨 감지에 쓸 실제 신호원 확보(현재는 없음) — 이 4가지가 선결 과제입니다.

44. ✅ **CircuitBreaker HalfOpen이 1회 프로브 제한을 실제로 강제하지 않음** — 해결됨
    (2026-08-13). `crates/fleet-scheduler/src/breaker.rs`의 `check()`가 `HalfOpen`
    상태에서 무조건 허용하도록 "단순화"되어 있었고, `half_open_max_probes` 설정
    필드(기본값 1)가 어디에서도 읽히지 않아, 복구 중인 워커에 동시 요청이 몰리면
    전부 통과되어 다시 과부하시킬 수 있는 상태였습니다.

    수정: `BreakerInner`에 `half_open_probes: VecDeque<Instant>`(발급된 프로브
    슬롯의 발급 시각)를 추가해 `check()`가 `half_open_max_probes`를 실제로 강제하도록
    했습니다. `record()`가 호출되면 슬롯을 해제합니다. 호출자가 `record()`를 영영
    호출하지 않는 "lost probe" 상황(크래시 등)을 대비해, `open_duration_secs` 이상
    미해결인 슬롯은 `check()`가 스스로 회수하도록 안전장치를 넣었습니다(로드맵
    #40이 언급한 `probe_claimed_at_millis` lost-probe 캔슬 아이디어와 같은 방향).
    새 테스트 4개 추가(동시 프로브 상한 강제, `half_open_max_probes>1` 케이스,
    프로브 실패 시 재개방, lost-probe 회수). 참고: 쿨다운 기본값도 문서가 오래
    서술해온 30초가 아니라 실제로는 **10초**입니다
    (`CircuitBreakerConfig::default().open_duration_secs`).

    ✅ **검증 완료 (2026-08-13)**: `cargo build --release --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets --all-features`
    (자체 크레이트 경고 0건 — vendor SDK 예제 경고 1건은 무관) 전부 통과.
    `cargo test -p fleet-scheduler breaker::` **8/8 통과** (신규 4개 포함).

45. ✅ **`MemStore`가 `fleet-store` 밖에 6개 이상 독립 중복 정의로 흩어짐** (P3, 기술 부채) —
    해결됨 (2026-08-13). 실측해보니 실제로는 **10개 파일**에 각자 `struct MemStore`가
    흩어져 있었습니다: `fleet-api/src/test_support.rs`(+ `mod test_support;`가
    `#[cfg(test)]`로 가려져 있어 `tests/*.rs` 통합 테스트는 애초에 재사용 자체가
    불가능했음), `fleet-api/tests/{metrics_endpoint,cloudflare_access,api_flow,
    transport_integration}.rs`(4개), `fleet-dashboard/src/app.rs`,
    `fleet-dashboard/tests/dashboard_api.rs`, `fleet-scheduler/src/{cleanup,health,
    reconcile}.rs`(3개).

    수정: `fleet-store`에 `mem::MemStore`(신규, `test-support` 카고 피처 뒤에
    게이트 — 프로덕션 빌드에 포함되지 않음)를 신설해 **Store trait의 모든 메서드를
    실제로 동작하는 인메모리 구현**으로 통합했습니다. 단순 통합에 그치지 않고
    `PgStore`의 실제 SQL 동작(정렬 순서 `ORDER BY ... DESC`, `NotFound` 반환 조건,
    `Dispatched` 전이 시 `dispatched_at` 자동 갱신 등)과 일치하도록 기존 10개
    구현의 사소한 divergence도 함께 바로잡았습니다(예: 기존 `fleet-api`
    `test_support.rs`는 `update_task_status`가 존재하지 않는 태스크에도 조용히
    no-op이었으나 실제 `PgStore`는 `NotFound`를 반환함 — 통합 과정에서 발견해
    실제 동작에 맞춤). 회복성(에러 주입) 테스트가 필요한 소수 케이스는
    `MemStore::with_failing(&["list_tasks", ...])` 제네릭 실패 주입 메커니즘으로
    흡수했고, `fleet-scheduler/src/cleanup.rs`의 5개 테스트는 기존의 "카운터만
    돌려주는" 스크립트형 mock 대신 **실제 세션/로그인시도 레코드를 삽입해 진짜
    삭제 로직을 태우는 방식**으로 다시 작성해 테스트 사실성도 함께 개선했습니다.

    각 소비 크레이트(`fleet-api`/`fleet-dashboard`/`fleet-scheduler`)의
    `[dev-dependencies]`에 `fleet-store = { features = ["test-support"] }`를
    추가하고, 10개 파일의 로컬 `MemStore` 정의를 전부 제거했습니다. 순변화:
    19개 파일, +1,150/−1,951줄 (신규 통합 구현 949줄을 포함하고도 순감소 801줄).

    ✅ **검증 완료**: `cargo test --workspace --features "acp mtls"` 전체
    그린(실패 0건), `cargo build --release --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets
    --all-features`(자체 크레이트 경고 0건) 모두 통과.

46. ✅ **`docs/` 전체 코드 대조 검증 및 문서 구조 정리** — 해결됨
    (2026-08-12, 이 세션). `docs/architecture/*.md`(overview.md 9개 절 + 신규 2개 절,
    api-reference.md, mcp-specification.md), `docs/worker-bootstrap/*.md`(5개 문서),
    `docs/deployment/*.md`(5개 문서)를 전면 코드 대조해 다수의 사실 오류를 정정했습니다:
    MCP 도구 이름 전체가 허구였음, 디스패처가 "1초 폴링"이 아니라 이벤트 기반, ACP
    전송 계층 3개 절이 2026-08-11에 이미 대체된 구세대 구현을 서술 중이었음,
    `install.sh` 기본 설치 경로 오기재, nginx 단일 서버 예시에 오케스트레이터 API
    라우팅(`/v1/`)이 통째로 빠져 배포 시 워커 셀프 서비스 등록이 불가능했을 것,
    `server-topology.md`가 2026-08-11에 폐기된 liteLLM Docker 설계를 최신으로
    서술 중이었음 등. 다이어그램 리소스를 `docs/assets/diagrams/<domain>/*.mermaid`로
    단일화(30여 개 파일 통합, 확장자 통일)하고, `agent.md`/`CLAUDE.md`의 정본/사본
    관계를 명확히 했으며, `engineering-patterns/reuse-patterns.md`의 고아 판정을
    코드 근거로 철회했습니다. `docs/server-management/*`(명시적 미구현 제안),
    `docs/security/findings.md`(기존 내용과 이번 세션 재확인 결과 일치)는 별도
    수정 불필요로 판단했습니다.

    **후속 (2026-08-13, 동일 세션 연속 진행)**: `docs/ui-dashboard/ui-design.md`,
    `docs/llm-wiki/*.md`, `docs/credentials/registry.md`까지 절 단위로 마저 정독·
    정정 완료. 주요 발견: ui-design.md는 "8개 페이지"라 서술했지만 실제 HTML
    라우트는 18개(7개 누락), 호스트 인벤토리(§3.2.5/§3.2.6/§10.3)를 "예정"으로
    서술했지만 `007_hosts.sql`/heartbeat 확장/`host_events` 훅 모두 이미 구현·
    배포 완료 상태였음, `WorkerStatus`/host `status` enum이 실제 코드와 다른
    값(`pending`/`ready`/`unknown` 등 존재하지 않는 상태)을 나열하고 있었음.
    llm-wiki에서는 `README.md`가 이미 삭제된 `AutonomicEngine` 연동을 현재형으로
    서술 중이었던 것을 발견해 미구현 설계 구상으로 재분류(항목 #43 삭제와의
    직접적 정합성 문제), `multi_provider_llm_proxy_analysis.md`의 Postgres
    DB-backed 배포 결론이 이후 문서에서 뒤집혔음을 명시, `litellm_integration_plan.md`
    §7의 `examples/litellm-config.yaml` 서술 오류 정정 및 nginx timeout 값
    불일치(300s vs 600s, 미확인) 기록, `free_tier_providers_analysis.md`의 부분
    노후(§1.3/§1.4/§5)를 `index.md`에 상태 하향으로 반영.
    `docs/credentials/registry.md`에는 코드에는 이미 구현돼 있었지만 레지스트리에
    누락돼 있던 자격증명 3종(`FLEET_API_TOKENS`/`FLEET_CF_AUDIENCE`,
    `FLEET_GMAIL_USER`/`FLEET_GMAIL_APP_PASS`, `ssh_keys` 프로비저닝 키 금고)을
    신규 등재했습니다(실배포 값 저장 위치는 미확인).

47. ✅ **HealthChecker↔Task 연동 부재** (P2, 정확성) — 해결됨 (2026-08-13). 워커가
    `Offline`으로 표시돼도(45초/3회 하트비트 누락) 그 워커에 배정된 `Dispatched`
    작업은 아무도 실패 처리하지 않아, 워커가 heartbeat만 끊기고 ACP WebSocket
    연결은 살아있는 애매한 상태에서 작업이 무기한 `Dispatched`로 남을 수 있었습니다
    (`docs/architecture/overview.md`의 "태스크 모니터링 및 종료 감지 파이프라인"
    절에서 처음 발견·문서화).

    수정: `Reconciler`(`crates/fleet-scheduler/src/reconcile.rs`)에 세 번째 스윕
    `reap_stale_dispatched`를 추가했습니다 — 담당 워커가 여전히 등록돼 있지만
    `Offline`이고 마지막 하트비트로부터 `offline_worker_grace`(기본 **5분**, 신규
    CLI 플래그 `--reconcile-offline-worker-grace-secs` /
    `FLEET_RECONCILE_OFFLINE_WORKER_GRACE_SECS`) 이상 지났다면 `Failed(WorkerUnavailable)`로
    전이합니다. 기존 "워커 row 자체가 사라진" 경로(30초 유예)보다 훨씬 긴 유예를
    두는 이유는 `Offline`이 되돌릴 수 있는 상태이기 때문입니다. 테스트 3개 추가
    (유예 초과 시 실패 처리, 유예 이내 방치, `Degraded`는 건드리지 않음 확인).

    ✅ **검증 완료 (2026-08-13)**: `cargo build --release --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets --all-features`
    (자체 크레이트 경고 0건) 전부 통과. `cargo test -p fleet-scheduler reconcile::`
    **10/10 통과** (신규 3개 포함).
    ⚠️ **알려진 한계**: `update_task_status`가 낙관적 잠금을 하지 않아, `Failed`로
    마킹한 직후 워커가 실제로는 재연결해 뒤늦게 `WorkerEvent::Completed`가
    도착하면 상태가 덮어써질 수 있는 이론적 경쟁 상태가 남아 있습니다(5분 유예가
    이 창을 좁힐 뿐 완전히 없애지는 못함).

---

## 현재 진행 상황 (2026-08-11 기준)

> **P0·P1은 전부 해소됐다.** 남은 항목은 모두 P2 이하다.

### 남은 작업 배정
| 담당 | 항목 |
|---|---|
| 미배정 | #14, #22~#24, #26, #42 |

> ⚠️ **정정 (2026-08-13)**: 이 표가 #32를 여전히 "security 담당·미해결"로 열거하고
> 있었으나, 해당 항목 본문은 이미 "✅ 해결됨(`db614ec`)"으로 끝나 있었다 — 헤더
> 아이콘과 이 표 갱신이 누락된 채 방치된 사례여서 제거했다. `#27`도 원래 목록에
> 있었으나 본문이 "🔵 사실상 충족 — 종료 또는 최하위 강등 권고"로 이미 사실상
> 실질 작업이 아니라고 결론 낸 항목이라 함께 제거했다.

### 호환성 주의 — `/api/audit` 의미 변경 (`8755c0d`)
`/api/audit`가 반환하는 데이터가 **바뀌었다**. 기존에는 작업·워커 생명주기 이벤트(`events`
테이블)를 `/api/events`와 중복 제공했으나, 이제는 **인증/권한 감사 로그(`audit_log` 테이블)**
전용이다. 작업·워커 이벤트가 필요한 소비자는 `/api/events`로 옮겨야 한다.
API 버전(`/v1`, #22)이 없는 상태의 파괴적 변경이므로 외부 소비자가 있다면 사전 공지가 필요하다.

### 완료된 기능
- ✅ RBAC 권한 강제 (10개 API 핸들러)
- ✅ 쿠키 세션 인증 (Phase 9.1)
- ✅ 이메일 기반 로그인 + 인증 플로우 / Gmail SMTP 통합
- ✅ 비밀번호 재설정 플로우 (T4) + 재발송 UI (T5)
- ✅ 태스크 API 페이지네이션 offset (T6) / 워커 상세 쿼리 최적화 (T7)
- ✅ 태스크/워커 상세 페이지, 사용자 관리 CRUD UI, SSE 실시간 업데이트
- ✅ mTLS, 서킷 브레이커, ACP 자동 재연결, 헬스체커 + 하트비트
- ✅ 프로비저너 (Ansible 플레이북)
- ✅ Prometheus 메트릭 (7개 패밀리 + task_duration / http_request_duration 히스토그램)
- ✅ 컨테이너화 (Dockerfile / docker-compose)
- ✅ 구조화된 감사 로그 (`audit_log`) + `/api/audit`↔`/api/events` 역할 분리
- ✅ CI 커버리지 리포팅 (cargo-llvm-cov)
- ✅ 커넥션 풀 튜닝 + 만료 세션/로그인시도 정리 백그라운드 잡
- ✅ DB 백업·복구·세이프 롤백 스크립트 (systemd 타이머)
- ✅ API 오류 응답 포맷 통일 (`{error:{code,message}}`)

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
