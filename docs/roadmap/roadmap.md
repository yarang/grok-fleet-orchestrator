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
29. ✅ Dashboard API의 하드코딩된 도구 목록 — 해결됨 (`8755c0d`). `list_tools_api`가
    `fleet_mcp::schema::all_tools()`를 그대로 노출해 단일 진실 원천을 따른다.
    (별도 보고 없이 #12 작업에 포함되어 반영됐다 — 실측 대조 중 확인.)
30. ✅ CI 커버리지 리포팅 — 해결됨 (`afd8d35`). `.github/workflows/ci.yml`에 `coverage` job 추가
    (`cargo-llvm-cov`, 워크스페이스 전체, Postgres 서비스, lcov 아티팩트 + 잡 로그 요약).
    외부 서비스(Codecov 등) 계정/토큰 연동은 후속 과제.

## 신규 항목 (2026-08-01 추가)

31. ✅ **`dispatch_latency` 메트릭** (P2) — 해결됨 (`ed82b27`). `tasks` 테이블에 `dispatched_at` 컬럼을 추가하는 마이그레이션(`012_task_dispatch_latency.sql`)을 진행하고, 스케줄러 디스패치 시점에 갱신하도록 처리. 대기 시간차를 계산하여 Prometheus Histogram `fleet_task_dispatch_latency_seconds` 메트릭으로 노출 완료.

32. ⏳ **`/admin/*` HTML 페이지에 RBAC 검사 부재** (P2, 보안) — → 담당: security

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
36. ⏳ **mTLS 인증서 자동 회전(Auto-Rotation) 정책 도입** (P1/P2, 보안/운영) — 사설 CA 기반 mTLS 인증서 만료 시 서비스 중단 없이 교체하기 위해 TLS 컨텍스트 동적 리로드(File Watcher) 또는 중앙 인증서 자동 배포 설계.
37. ⏳ **인벤토리 기반 mTLS 프로비저닝 자동화 지원** (P2, 인프라) — `--inventory` 모드에서 `InventoryWorker` 스키마 확장을 적용하여 mTLS 설정 및 인증서 자동 주입 파이프라인 구현.
38. ⏳ **스케줄러 작업 실패 시 자동 재시도 및 Dead Letter Queue (DLQ) 설계** (P2, 안정성) — 네트워크 일시 순단 시 태스크가 즉시 Failed로 유실되지 않도록 자동 재스케줄러 큐 및 Stale 상태 격리를 위한 DLQ 메커니즘 도입.
39. ⏳ **Known Hosts TOFU 모드에서의 대규모 인프라 배포 절차 상 보안 공백 보완** (P2, 보안) — 대규모 배포 시 첫 SSH 연결의 MITM 방어를 위해 `fleet provision` 도구 실행 시 SSH 호스트 키 사전 수집/검증 기능 구현.
40. ⏳ **`xai-circuit-breaker` 기반 고성능 회로 차단기 도입** (P2, 성능/안정성) — `grok-build` 분석에 따라 슬라이딩 윈도우 실패율 측정, `AtomicU8/AtomicBool`을 이용한 lock-free `is_open()` 핫패스 최적화 및 `probe_claimed_at_millis`를 이용한 Lost Probe 캔슬 안전장치 설계 도입.
41. ⏳ **WebSocket Demuxer 패턴을 적용한 동시 다중 세션 고도화** (P2, 네트워크) — `xai-computer-hub-sdk` 분석에 근거해 단일 WebSocket 연결 상에서 ACP 프롬프트 세션의 순서 보장 및 Head-of-Line Blocking 방지를 위한 RPC Frame Demultiplexer 구현.
42. ⏳ **워커 노드 연동 분산 OTLP Tracing Context Propagation 구축** (P2, 모니터링) — `xai-tracing` 기법을 차용해 오케스트레이터와 `fleet-worker` 간 WebSocket 통신 시 `traceparent` 스팬 캐리어를 전파하여 E2E 분산 추적 시각화 완성.

---

## 현재 진행 상황 (2026-08-11 기준)

> **P0·P1은 전부 해소됐다.** 남은 항목은 모두 P2 이하다.

### 남은 작업 배정
| 담당 | 항목 |
|---|---|
| security | #32 (`/admin/*` RBAC — 3개 페이지 일괄) |
| 미배정 | #10, #11, #13, #14, #15 잔여, #21~#28, #31, #36~#39, #40~#42 |

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
