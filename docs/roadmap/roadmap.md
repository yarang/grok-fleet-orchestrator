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
14. ✅ **다크 모드, 컬럼 정렬, 고급 필터링 부재** — 해결됨 (2026-08-14).
    셋 다 실제로 부재함을 확인했습니다. 사용자에게 범위(다크모드만 /
    정렬+필터만 / 전부)를 물었고 **"전부 구현"**이 채택됐습니다.

    **다크 모드**: `styles.css`(대시보드)와 `login.css`(로그인/부트스트랩,
    별도 스타일시트)에 각각 다크 팔레트를 추가했습니다. 흥미롭게도
    `--surface-tile-1`/`--surface-tile-2`/`--primary-on-dark` 등 다크 모드용
    토큰이 Apple 디자인 시스템 문서 정렬 작업 때 이미 정의는 돼 있었지만
    실제로 쓰인 적은 없었습니다(다크 모드 자체가 없었으므로) — 이번에
    최초로 실사용했습니다. 시스템 설정(`prefers-color-scheme`)이 기본이고,
    `app.js`가 사이드바 하단에 동적으로 만드는 토글 버튼으로 명시적 선택도
    가능(`localStorage` 영속화, `<html data-theme="...">`). 대시보드 페이지
    14곳이 사이드바 마크업을 각자 복제하고 있어(공용 템플릿 없음) 정적
    버튼 대신 JS로 동적 생성해 페이지 하나하나를 고칠 필요가 없게 했습니다.

    구현 도중 다크 모드가 새로 드러낸 기존 버그 2건도 함께 고쳤습니다:
    `#logout-btn`과 로그인 페이지의 `.auth-logo`가 `background: var(--ink)`를
    쓰고 있었는데, `--ink`가 다크 모드에서 거의 흰색으로 뒤집히면서 흰
    배경+흰 글자로 안 보이게 되는 문제였습니다(라이트 모드에서만 우연히
    괜찮았던 것) — 각각 `--primary`(디자인 시스템 자체가 "모든 인터랙티브
    요소에 단일 액센트 컬러" 원칙을 표방)와 고정 다크 색상(브랜드 로고는
    테마와 무관하게 유지)으로 교체했습니다. `.btn-danger`의 하드코딩된
    연한 핑크 톤도 `--danger`/`--danger-border`/`--danger-bg-hover` 토큰으로
    바꿔 다크 모드에서도 대비를 유지하게 했습니다.

    **컬럼 정렬**: `tasks.js`/`hosts.js`에 헤더 클릭 정렬을 추가했습니다
    (클릭 시 오름차순, 재클릭 시 내림차순, `▲`/`▼` 표시). 타입별 비교
    (문자열/숫자/날짜)를 구분하는 공용 패턴을 두 파일에 동일하게 적용.

    **고급 필터링**: `tasks.js`에 프롬프트 텍스트 검색 + worker 드롭다운 +
    model 드롭다운을 기존 상태 필터(pill)와 AND로 결합해 추가했습니다.
    worker/model 옵션은 별도 백엔드 엔드포인트 없이 현재 로드된 태스크
    목록에서 클라이언트가 유일값을 추출해 채웁니다. 필터 결과가 0건일 때
    "태스크 자체가 없음"과 "필터에 안 걸림"을 구분해서 안내합니다.

    **범위 제한(의도적)**: `hosts.js`는 정렬까지만(고급 필터는 제외 —
    호스트 수는 물리 인프라 규모로 자연히 상한이 있어 #23과 같은 사유로
    우선순위가 낮다고 판단). 로그인/부트스트랩 페이지는 `app.js`를
    로드하지 않아(인증 전) 명시적 토글 없이 시스템 설정만 따릅니다.

    ✅ **검증 완료**: `cargo build --workspace --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets
    --all-features`(경고 0건, 벤더 코드 제외), `cargo test -p
    fleet-dashboard`(전체 그린) 통과. JS 파일은 `bun build --target=browser`로
    번들링해 구문 오류가 없음을 확인(이 저장소에 JS 테스트 도구는 없음).
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
22. 🔵 **Dashboard API에 `/v1` 버전 부재** — **재평가 후 최하위 강등** (2026-08-14).
    premise 자체(`/api/*` 약 30개 라우트에 버전 프리픽스 없음)는 정확하지만,
    가치가 낮다고 판단했습니다. 워커용 `/v1` API(`fleet-api`)는 독립적으로
    버전이 다를 수 있는 외부 `fleet-worker` 바이너리가 붙는 실질적 호환성
    문제가 있어 버전 프리픽스가 의미 있습니다. 반면 대시보드 API는 쿠키
    세션 + RBAC로 인증되고 소비자가 **이 저장소에 같이 들어있는 first-party
    JS뿐**입니다(`docs/architecture/api-reference.md`에 이미 명시) —
    프론트엔드와 API가 항상 같은 바이너리로 같이 배포되므로 "옛 클라이언트가
    새 서버에 붙는" 시나리오 자체가 존재하지 않습니다.

    사용자에게 3가지 처리 방식(AskUserQuestion)을 제시했고
    **"우선순위 강등 + 사유 문서화"**가 채택됐습니다 — `/api/*` → `/api/v1/*`로
    30개 라우트와 그걸 호출하는 모든 JS `fetch()` 호출, 관련 테스트
    (`dashboard_api.rs` 등)까지 넓게 손대는 변경인데 실제로 막아주는 문제가
    뚜렷하지 않다고 판단했습니다. 코드 변경 없음. 향후 대시보드 API를
    외부(브라우저 확장, 서드파티 통합 등)에 공식 노출할 계획이 생기면 그때
    재검토 권고.
23. ✅ **프론트엔드 페이지네이션 UI 부재** — 해결됨 (2026-08-14). 조사 결과
    백엔드는 `list_tasks`/`list_workers`/`list_audit_events`가 이미 `limit`/
    `offset`을 완전히 지원했지만(#11), 대시보드 태스크 목록(`tasks.js`)은
    `?limit=200` 고정값으로 한 번만 조회하고 그 이상 쌓인 태스크는 그냥
    보이지 않았습니다 — offset을 보내는 UI 자체가 없었습니다.

    `tasks.html`/`tasks.js`에 "Load more" 버튼을 추가했습니다. 총 개수를
    알려주는 엔드포인트가 없으므로, 매 조회마다 `limit`보다 1개 더
    요청해서(`limit+1`) "더 있음"을 판단하고 실제로는 `limit`개만
    렌더링하는 방식을 씁니다(페이지 크기 100씩 증가, `created_at DESC`
    최신순이라 페이지 경계가 안정적). 클릭할 때마다 페이지 크기를 늘려
    처음부터(`offset=0`) 다시 조회하는 방식을 택했습니다 — 기존 SSE
    기반 실시간 갱신(`fetchTasks()`가 매 이벤트/5초마다 전체를 다시
    가져와 리렌더)과 자연스럽게 맞물립니다.

    **범위 제한(의도적)**: `hosts.js`(`/api/hosts`)와
    `admin-activity.js`(`/api/events`)도 프론트에서 페이지네이션 UI가
    없기는 마찬가지지만, 조사해보니 `Store::list_hosts()`는 애초에
    `limit`/`offset` 파라미터 자체가 없어(무조건 전체 반환) 백엔드부터
    고쳐야 하는 별도 작업이고, `/api/events`는 `offset` 기반이 아니라
    `after_seq` 커서 기반(실시간 tailing에 최적화된 모델이라 "이전
    페이지로" 탐색 자체가 다른 설계 문제)이라 이번 항목과 성격이 다릅니다.
    호스트 수는 물리 인프라 규모로 자연히 상한이 있어 무기한 증가할
    작업(태스크) 목록만큼 급하지 않다고 판단해 이번 범위에서 제외했습니다
    — 필요 시 별도 항목으로 분리 권장.

    신규 테스트: `fleet-store`에 `task_list_respects_limit_and_offset`
    (limit이 정확히 결과 개수를 제한하는지, offset으로 페이지가 겹치지
    않는지, "limit+1 조회로 더 있음을 판단"하는 프론트 로직이 실제
    백엔드 동작과 맞는지 검증 — 실제 Postgres로 검증).

    ✅ **검증 완료**: `cargo build --workspace --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets
    --all-features`(경고 0건, 벤더 코드 제외), `cargo test --workspace
    --features "acp mtls" -- --test-threads=1`(`DATABASE_URL`을 실제
    Postgres `fleet_test`로 지정, 전체 그린) 통과. 프론트엔드(JS/HTML)
    변경은 이 저장소에 JS 테스트 도구가 없어 수동 코드 리뷰로 검증.
24. ✅ **모바일 반응형 감사 미수행** — 해결됨 (2026-08-14). premise를 재확인한
    결과 "미수행"은 부정확했습니다 — 사이드바 슬라이드오버(880px)와 테이블
    한 줄→세로 스택 전환(834px) 미디어쿼리가 이미 존재했습니다. 하지만
    실제로 감사(전체 8개 `.table` 인스턴스를 대조)해보니, 그 미디어쿼리가
    테이블 ID를 하나하나 나열하는 방식이었는데(`#worker-list`, `#task-list`,
    `#task-table`, `#host-table`) 이후 추가된 관리자 페이지 3곳
    (`#activity-table`, `#key-table`, `#user-table`)이 그 목록에 반영되지
    않고 있었습니다.

    더 나아가 이 3개 테이블은 **데스크톱용 `grid-template-columns` 규칙
    자체가 아예 없어서, 모바일 전용 버그가 아니라 애초에 모든 화면
    크기에서 컬럼 정렬 없이 세로로 쌓여 보이고 있었습니다** — `#task-table`이
    한때 겪었던 것과 같은 유형의 결함(코드 주석에 이미 기록된 선례,
    `styles.css` 참고)이 관리자 페이지 3곳에서 반복된 것입니다.

    수정: 세 테이블에 데스크톱 컬럼 규칙을 신설했고, 모바일 미디어쿼리는
    테이블 ID를 계속 나열하는 대신 공용 `.table .row` 선택자로 일반화해
    앞으로 테이블이 추가돼도 이 부류의 누락이 재발하지 않도록 했습니다.
    나머지 레이아웃(로그인 폼, `.detail-grid`, JSON 출력 패널의
    `overflow-x`, 모든 페이지의 viewport 메타 태그)은 감사 결과 이미 적절히
    반응형으로 구성돼 있어 손대지 않았습니다.

    ✅ **검증 완료**: `cargo build --workspace --features "acp mtls"`,
    `cargo clippy -p fleet-dashboard --all-targets --all-features`(경고
    0건), `cargo test -p fleet-dashboard`(자산 임베드 테스트 포함, 전체
    그린) 통과. 순수 CSS 변경이라 이 저장소에 JS/CSS 테스트 도구는 없어
    실제 8개 테이블 마크업과 대조하는 수동 코드 리뷰로 검증.
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
26. 🔵 **시크릿 매니저 통합 부재 (Vault/AWS SM)** — **스킵, 최하위 강등**
    (2026-08-14). premise 자체(외부 시크릿 매니저 연동이 없음)는 정확합니다
    — `fleet-credentials`는 AES-256-GCM + 로컬 마스터키(환경변수/파일)로
    워커 API 키를 암호화해 Postgres에 저장할 뿐, Vault/AWS Secrets Manager
    같은 외부 KMS와는 연동하지 않습니다.

    사용자에게 처리 방식(AskUserQuestion)을 물었고 **"실제 사용 중인
    벤더가 없음 — 스킵"**이 확인됐습니다 — 현재 배포 환경에 HashiCorp
    Vault나 AWS Secrets Manager가 운영되고 있지 않아, 연결·인증 흐름을
    실제로 검증할 수도 없는 상태에서 통합 코드를 작성하는 것은 가치가
    낮다고 판단했습니다. 코드 변경 없음.

    향후 실제로 Vault 또는 AWS SM을 운영하게 되면, 개별 워커 API 키
    자체보다는 `fleet-credentials::MasterKey`의 로딩 경로(현재 환경변수/
    파일 2가지)에 해당 KMS를 세 번째 옵션으로 추가하는 방향을 권고합니다
    — 루트 비밀(마스터키) 하나만 외부 KMS가 관리하고, 그 아래 개별 자격
    증명은 기존 AES-GCM-in-Postgres 설계를 그대로 유지하는 편이 변경
    범위가 작고 기존 아키텍처와도 잘 맞습니다. 재검토 시 어떤 벤더를 실제로
    쓸지부터 다시 확인 필요.
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

42. ✅ **워커 노드 연동 분산 OTLP Tracing Context Propagation 구축** (P2, 모니터링) —
    해결됨 (2026-08-14). 원 서술("오케스트레이터와 `fleet-worker` 간 WebSocket
    통신 시 `traceparent` 전파")이 실제 아키텍처와 어긋났습니다 — `fleet-worker`는
    ACP WebSocket 경로에 전혀 관여하지 않습니다. 오케스트레이터의 `AcpTransport`는
    `grok agent serve`(이 저장소 밖 외부 바이너리)의 WS 엔드포인트에 직접 붙고,
    `fleet-worker`는 그 서브프로세스를 관리할 뿐 — 오케스트레이터와 실제로 주고받는
    통신은 `POST /v1/workers/register`·`heartbeat` **HTTP** 호출뿐입니다.

    이 발견을 사용자에게 제시하고(AskUserQuestion) 3가지 범위(HTTP 경로만 / HTTP +
    ACP `_meta` 주입까지 / 로드맵만 정정하고 스킵) 중 **"HTTP 경로(register/
    heartbeat)만 구현"**이 채택됐습니다 — ACP `_meta` 주입은 grok이 외부 블랙박스라
    실제로 이어붙이는지 이 저장소에서 검증할 수 없어 제외.

    구현: `fleet-worker`에 `fleet-cli::logging.rs`와 동일한 패턴(같은 `OTEL_
    EXPORTER_OTLP_ENDPOINT` env var, service.name만 `"grok-fleet-worker"`)으로
    OpenTelemetry 연동(`init_tracing()`)을 신규 추가했습니다. 양쪽 프로세스가
    W3C `TraceContextPropagator`를 전역 등록하고, `RegistrationClient::
    register_once`/`heartbeat_once`(신규 `#[tracing::instrument]`)가 자신의
    스팬 컨텍스트를 `traceparent`/`tracestate` 헤더로 실어 보내면, `fleet-api::
    handlers::register_worker`/`heartbeat`가 그 헤더를 파싱해 자신의 스팬을
    거기 이어붙입니다(`continue_trace_from_headers`). 이 연결은 register/heartbeat
    두 라우트에만 적용되며, 헤더가 없거나 OTel이 비활성이면 양쪽 다 조용히
    no-op(로컬 루트 스팬)입니다.

    신규 테스트 5개(`fleet-worker`/`fleet-api` 각 크레이트, `opentelemetry_sdk::
    testing::trace::InMemorySpanExporter`로 실제 OTel 파이프라인을 구동해
    trace-id가 발신 헤더에서 수신 스팬까지 정확히 전파되는지 검증 — 라이브
    OTLP 컬렉터 불필요). 상세 배경은
    [`docs/architecture/overview.md`](../architecture/overview.md) "분산 추적:
    register/heartbeat 경로의 traceparent 전파" 절 참고.

    ✅ **검증 완료**: `cargo build --workspace --features "acp mtls"`,
    `cargo check --no-default-features`, `cargo clippy --all-targets
    --all-features`(경고 0건, 벤더 코드 제외), `cargo test --workspace
    --features "acp mtls" -- --test-threads=1`(`DATABASE_URL`을 실제 Postgres
    `fleet_test`로 지정, 전체 그린) 통과.

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

    **추가 정리 (2026-08-14)**: 당시 검색이 문자 그대로 `struct MemStore`를
    찾는 방식이었던 탓에, `fleet-scheduler/tests/dispatch_e2e.rs`가 이름만
    `InMemoryStore`로 다르게 지은 11번째 중복(Store trait 전체를 재구현한
    ~210줄)을 놓쳤었습니다. `#38` 작업 중 발견해 기록해 두었다가 이번에
    canonical `fleet_store::mem::MemStore`로 교체하고, 그 구현에만 쓰이던
    이제-불필요한 import(`HashMap`, `async_trait`, `tokio::sync::Mutex`,
    `EventEntry`/`TaskOutput`/`BootstrapToken`/`WorkerHeartbeat`/`StoreError`
    등)를 함께 제거했습니다(순변화 -210줄). 기존 17개 e2e 테스트 전부
    동일하게 통과 — 동작 변화 없음, 순수 중복 제거.

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

## 신규 항목 (2026-08-14 추가)

48. ⏳ **프로젝트(Project) 기능 도입** (P2, 신규 기능) — **설계 완료, 구현 대기**.
    사용자 요청으로 신규 등록. `fleet-core::ProjectId`(타입)와 `tasks.project_id`
    컬럼(`013_task_threads.sql`)이 "나중에 project 기능이 도입되면 재귀 backfill
    없이 바로 채워질 수 있도록" 이미 예약돼 있었으나, 실제 `Project` 엔티티나
    host/agent(워커) 소속 개념은 지금까지 전혀 구현되지 않은 상태였음을 확인.

    **범위**: (1) 프로젝트가 여러 host와 agent(워커)를 담을 수 있어야 함, (2)
    하나의 프로젝트에 여러 agent가 배치돼 태스크가 그 agent들 사이로 분산
    디스패치될 수 있어야 함, (3) 위 둘을 지원하는 배치/해제 절차와 디스패치
    프로토콜.

    **1차 핵심 설계 결정** (AskUserQuestion, 2026-08-14): 프로젝트↔host/agent
    소속은 다대다(M:N — 조인 테이블 `project_workers`/`project_hosts`),
    `project_id` 지정 태스크의 디스패치는 소프트 힌트(배치된 agent가 없으면
    전체 풀로 폴백).

    ⚠️ **2차 개정 (2026-08-14, 같은 날)**: `#49` 설계 논의 중 사용자가
    "프로젝트의 하드 격리가 기본이어야 리소스 경쟁/충돌을 예방한다"고
    재검토를 요청 — "host 소유권만 하드로 하고 워커 M:N 공유는 그대로 두는"
    절충안을 처음 제안했으나, 사용자가 **"host 소유권이 비배타적이어도
    워커를 실행하며 리소스 경쟁이 생기지 않는가?"**라고 되물었고 정확한
    지적이었습니다 — 워커가 M:N으로 공유되면 그 워커가 도는 host의 물리
    자원과 워커 자체의 세션 슬롯을 다른 프로젝트 태스크와 항상 경합하게
    됩니다. **워커/호스트 모두 배타적(1:N) 소유로 전면 개정**했습니다 —
    `project_workers`/`project_hosts` M:N 조인 테이블을 `workers.project_id`/
    `hosts.project_id` 직접 FK로 교체. 디스패치도 소프트 폴백 대신 **하드
    필터**로 바뀌었는데, 새 에러/재시도 메커니즘을 만들지 않고 **기존 `#38`의
    `WorkerUnavailable` 재시도/Dead-Letter 경로를 그대로 재사용**합니다(배정된
    워커가 없으면 `Pending` 유지 → `Reconciler` 재시도 → 소진 시 dead-letter).
    구현 전이라 재작업 비용 없이 바로잡을 수 있었습니다.

    ⚠️ **4차 개정 (재검토, 2026-08-14)**: 구현 착수 전 사용자 요청으로
    `#48`/`#49` 설계 문서 전체를 처음부터 다시 정독해 실제 버그/모순 8건,
    설치·운영 편의성 이슈 4건을 찾고 바로잡았습니다. 이 항목에 해당하는
    수정: (1) `Project.default_agent_template_id`가 아직 정의되지 않은
    `#49`의 `AgentTemplateId`를 참조해 `#48` Phase 1이 컴파일 자체가 안 되는
    전방 참조 버그를 발견 — `tasks.project_id`의 FK-후행 예약 패턴과 동일하게
    원시 `Uuid`로 두고 FK/강타입은 `#49` Phase 1에서 추가하도록 수정. (2)
    `AgentProvisioningMode` enum의 정의 소유권이 불명확했던 것을 `#48`
    (`fleet-core::project`)로 명확화. (3) `workdir_template` 필드가 프로즈에서만
    언급되고 실제 스키마/구조체에 없던 것을 `projects` 테이블에 추가. (4)
    워커 재등록 시 `project_id`가 host의 현재 값으로 매번 재동기화돼야 한다는
    불변식을 명시. (5) `unassign_worker`/`unassign_host`를
    `unassign_worker_from_project`/`unassign_host_from_project`로 개명해
    `assign_*` 계열과 명명을 통일. 또한 정책 결정 1건 확인: 재배치된 워커/호스트의
    진행 중 태스크는 그대로 완료까지 진행(재배치는 향후 디스패치 자격에만
    영향). 문서에 §UI/UX(열린 질문 목록) 절도 신설했습니다. 자세한 내용은
    [`docs/architecture/project-feature-design.md`](../architecture/project-feature-design.md)의
    개정 이력 참고.

    전체 설계(ER 다이어그램, `WorkerSelector` 파이프라인의 5.5단계 하드 필터,
    `Store` 트레이트 확장, RBAC 권한 4종, REST/MCP API 표면, 4단계 구현 계획,
    열린 질문)는
    [`docs/architecture/project-feature-design.md`](../architecture/project-feature-design.md)에
    정리했습니다 — 이 항목의 구현 진행 상황을 갱신할 때마다 그 문서도 함께
    최신화합니다.

    ⚠️ **4차 개정 (프로토콜/절차 재검토, 2026-08-14)**: `#38`의 실제 dead-letter
    코드(`reconcile.rs`)를 확인한 결과, 재시도 소진 시 `TaskFailure.error`가
    `"dispatch retries exhausted (N attempts)"`로 원래 `SelectionError` 메시지를
    덮어써 `NoWorkerForProject`로 죽은 태스크와 다른 이유로 죽은 태스크가
    `Failed` 상태에서 구분되지 않는다는 걸 발견 — 마지막 에러 텍스트를 보존하는
    개선안을 [`project-feature-design.md`](../architecture/project-feature-design.md) §5에
    기록(`#38` 구현 범위, 이 항목이 실질적으로 의존).

    **다음 단계**: 설계 문서 §8의 Phase 1(스키마 + Store + RBAC)부터 순차 구현.

49. ⏳ **에이전트(Agent) 동적 프로비저닝 · 메모리 · 스레드 요약 · 도구 바인딩**
    (P2, 신규 기능) — **설계 완료(3차 개정 포함), 구현 대기**. `#48` 등록 직후
    사용자가 요구사항을 두 차례 확장했고, `#48`의 하드 격리 개정에 맞춰 한 번
    더 재정렬했습니다.

    **1차 확장**: (1) host당 여러 에이전트 동시 운영, (2) custom 프롬프트로
    에이전트 구분, (3) 프로젝트가 필요할 때 에이전트를 직접 만들어서 호출,
    (4) host 여유 있을 때만 생성, (5) 에이전트가 여러 세션에 걸친 맥락 유지,
    (6) 그 맥락을 프로젝트별 메모리로 관리, (7) 프로젝트에 속하지 않는 태스크
    스레드는 스레드별 요약으로 관리, (8) 필요 시 결과물을 디렉토리로 관리.

    **2차 확장**(같은 날, 1차의 (2)를 구체화): (9) custom 프롬프트는
    오케스트레이터가 중앙 관리하고 CLI와 연결, (10) custom 프롬프트를 tool/skill과
    연결해 에이전트를 만들고 태스크 할당, (11) tool/MCP를 중앙에서 관리해 필요한
    tool 제공, (12) 에이전트에 필요한 tool을 미리 연결하는 template 설정,
    (13) 필수(required) tool과 옵션(optional) tool을 필요 시 제공.

    기술적 표면이 `#48`(스키마/RBAC/소프트 디스패치 필터)과 크게 달라(프로세스
    수명주기 관리, 오케스트레이터→워커 제어 채널, 메모리 저장소, 도구 바인딩)
    AskUserQuestion 확인 후 별도 항목(`#49`)으로 분리, 2차 확장도 신규 항목을
    또 쪼개지 않고 `#49`(Agent 생성 방식 자체를 다루는 항목) 안에 통합했습니다
    ("Agent가 어떻게 만들어지는가"의 본질적인 일부라고 판단).

    **1차 핵심 설계 결정** (AskUserQuestion, 2026-08-14): Agent를 Worker와
    분리된 신규 엔티티로 도입. 프로비저닝은 진짜 동적이되, `fleet-worker`가
    인바운드 연결을 받지 않는다는 기존 원칙(`#42`)을 지키려 기존 heartbeat
    폴링에 커맨드를 얹는 방식 채택. 메모리는 구조화된 텍스트/JSON 누적 +
    프롬프트 주입.

    **2차 판단** (사용자가 직접 "어떻게 판단하는가" 요청 — 추가 질문 없이 코드
    조사 후 판단 제시): CLI 연결은 동의(`fleet-cli`의 기존 `Workers`/`Tasks`
    명령 그룹 패턴에 `Agent` 그룹 추가). 도구(MCP) 바인딩 메커니즘은 조사
    결과를 정직하게 반영 — vendor ACP SDK의 `SessionBuilder::with_mcp_server()`가
    이미 존재하지만 (a) `fleet-transport`가 그 기능에 필요한
    `unstable_mcp_over_acp` 피처를 켜지 않은 상태이고, (b) SDK 자체가 "unstable"로
    표시했으며, (c) 외부 MCP 서버에 단순 연결하는 게 아니라 Rust로 구현한
    `McpServerConnect` 인프로세스 프록시가 필요해, 처음 판단만큼 간단한
    해결책이 아니라는 걸 재조사로 확인하고 스스로 정정했습니다. grok 자체가
    로컬 MCP 설정 파일을 읽는지(더 단순한 대안 경로)도 미확인 — 그래서
    데이터 모델(중앙 카탈로그 + 템플릿 + 필수/옵션)은 확정하되, "실제 연결
    메커니즘"은 구현 착수 시 최우선 검증 스파이크(신설 Phase 0)로 미루기로
    판단. 필수/옵션 도구 활성화는 명시적 선택(태스크가 `requested_optional_tools`로
    직접 요청) 방식을 권고.

    **3차 재정렬** (`#48`의 하드 격리 전면 개정에 따라, 2026-08-14): host가
    이미 배타적으로 한 프로젝트에만 속하게 됐으므로, 그 위의 Agent도 자동으로
    모호함 없이 하나의 프로젝트에만 속합니다 — 2차 판단에서 열어뒀던 "에이전트가
    여러 프로젝트에 공유되는 경우" 열린 질문이 해소됐습니다. 또한 사용자가
    요청한 "에이전트를 사용자가 직접 설정 vs 오케스트레이터가 만들어서 사용"
    옵션을 `Project.agent_provisioning_mode`(`Manual`/`Automatic`, 기본 `Manual`)로
    모델링하고, `Automatic` 모드는 기존 `Reconciler`와 동일한 백그라운드 루프
    패턴(`AgentAutoProvisioner`)으로 "대기 태스크 + host 여유"를 주기적으로
    확인해 자동 프로비저닝하도록 설계했습니다 — 자동 생성이 영원히 host 여유를
    점유하지 않도록 `Project.agent_idle_timeout_secs` 기반 자동 종료 정책도 함께.

    전체 설계(확장된 ER 다이어그램 — `agent_templates`/`mcp_servers`/
    `agent_template_tools`/`agent_tools` 포함, heartbeat 커맨드 큐 프로토콜
    시퀀스 다이어그램, custom_prompt/메모리 주입 플로우 다이어그램,
    `016_agents.sql` 전체 스키마, RBAC 5종, 6단계 구현 계획(Phase 0 검증
    스파이크 신설), 6개 열린 질문)는
    [`docs/architecture/agent-provisioning-design.md`](../architecture/agent-provisioning-design.md)에
    정리했습니다.

    ⚠️ **알려진 리스크**: (1) Phase 4(동적 프로비저닝 — `fleet-worker`를 단일
    프로세스 관리에서 다중 프로세스 관리로 재작성)가 여전히 가장 위험도 높은
    부분 — 실기기 대상 수동 검증 병행 필수. (2) 도구 바인딩 메커니즘(경로 A:
    ACP unstable 피처, 경로 B: grok 자체 설정 파일)이 둘 다 미검증 — Phase 0을
    건너뛰고 바로 구현하지 말 것.

    ⚠️ **4차 재검토 (2026-08-14)**: `#48`과 함께 구현 착수 전 전체 재검토를
    거쳤습니다. 이 항목에 해당하는 수정: (1) `agents` 테이블에 `provisioned_by`
    컬럼이 없어 §4.1의 "Manual로 만든 에이전트는 유휴 타임아웃 대상 아님"
    규칙을 구현할 방법이 없던 버그를 발견 — `provisioned_by TEXT NOT NULL
    DEFAULT 'manual'` 컬럼 추가. (2) `mcp_servers` 삭제가 `ON DELETE CASCADE`로
    조용히 전파돼 운영 중인 에이전트가 도구를 잃을 수 있던 리스크를
    `ON DELETE RESTRICT` + API 409 응답으로 차단(정책 결정). (3) `agent_memory`
    보존/정리 정책이 아예 없던 누락을 열린 질문으로 명문화(`SessionCleanup`과
    동일 패턴 예정). (4) §4.1의 유휴 판단 기준을 **전면 재설계** — 사용자가
    "동작 중인지 판단하는 근거가 정확히 뭐냐, stdio만 보면 동작 중에도
    타임아웃될 수 있다"고 지적해, 프로세스 저수준 신호 대신 fleet가 이미
    신뢰하는 소스(`Worker.active_tasks`, `Dispatched` 태스크 존재,
    `agent_commands` pending 여부)만으로 판단하도록 재작성하고, 타이머 기준
    시각을 `GREATEST(created_at, 마지막 완료 시각)`으로, 커맨드 발행 직전
    재확인을 통한 레이스 방지도 추가. 적용 대상은 여전히 `Automatic`으로 생성된
    에이전트로 한정(Manual 생성분은 정책적으로 제외 유지). 설치·운영
    편의성 이슈 4건(다중 프로세스 로그 수집, 동적 포트 범위, 업그레이드 경로,
    프로비저닝 실패 알림)을 신설 §13에 정리하고, §UI/UX(열린 질문 목록) 절도
    신설했습니다. 자세한 내용은
    [`docs/architecture/agent-provisioning-design.md`](../architecture/agent-provisioning-design.md)의
    개정 이력 참고.

    ⚠️ **5차 개정 (프로토콜/절차 재검토, 2026-08-14)**: 구현 착수 전, 실제 코드
    (하트비트 프로토콜 `crates/fleet-api/src/schema.rs`, `GrokRunner`
    `crates/fleet-worker/src/grok_process.rs`, 서킷브레이커
    `crates/fleet-scheduler/src/breaker.rs`, `Reconciler`)를 근거로 §4의 절차와
    에러 처리를 재검증했습니다. 실제 발견/수정 사항: (1) "워커는 인바운드
    연결을 받지 않는다(#42)"는 원칙이 mTLS 배포에선 부정확함을 확인 —
    범위를 "제어 플레인만 아웃바운드"로 정정. (2) `HeartbeatResponse`가
    현재 확장 불가능한 고정 구조라 `pending_commands` 필드 추가 자체가
    Phase 4 스키마 변경 범위임을 명시. (3) **`AgentAutoProvisioner`가
    막 생성돼 아직 ack되지 않은 `Pending` 에이전트를 "없음"으로 오판해
    같은 대기 태스크에 중복 에이전트를 생성할 수 있는 레이스**를 발견 —
    eligibility 체크에 `Pending` 포함하도록 수정. (4) `agent_idle_timeout_secs`를
    프로젝트에서 매번 라이브 조회하면 프로젝트 삭제 시 그 소속이던 자동생성
    에이전트가 영원히 유휴 스윕에서 빠지는 좀비가 됨을 발견 — `agents.idle_timeout_secs`
    스냅샷 컬럼으로 전환. (5) `hosts.max_agents` 체크가 TOCTOU 레이스임을
    발견 — `SELECT ... FOR UPDATE` 트랜잭션으로 수정. (6) `/v1/workers/register`가
    이름 유일성을 검사하지 않는(upsert) 것을 확인 — `worker.name`에 `agent_id`
    전체 UUID를 포함해 충돌을 구조적으로 차단하도록 명시. (7) `agent_commands`
    ACK 프로토콜(신규 `POST /v1/workers/agent-commands/:id/ack` 엔드포인트,
    `Pending→Starting→Running` 전이 시점, 실패 경로)을 구체화하고 멱등성을
    "agent_id당 프로세스 1개" 효과 단위로 확정. (8) 기존 `GrokRunner`의
    "비정상 종료 시 자동 재시작" 루프가 `stop` 커맨드의 kill을 그대로
    되살릴 수 있음을 발견 — 의도된 종료 신호를 먼저 보내도록 명시. (9)
    호스트 삭제 시 `agents.host_id`의 `ON DELETE CASCADE`로 실행 중 agent가
    조용히 사라지고 프로세스가 고아로 남는 문제를 발견 — **정책 결정: 터미널
    상태가 아닌 agent가 있으면 호스트 삭제를 애플리케이션 레벨 409로
    차단**(RESTRICT, `mcp_servers` 정책과 동일 기조). 자세한 내용은
    [`docs/architecture/agent-provisioning-design.md`](../architecture/agent-provisioning-design.md)의
    개정 이력과 `agent-dynamic-provisioning-sequence.mermaid` 참고.

    **다음 단계**: 설계 문서 §11의 **Phase 0(검증 스파이크)부터** 순차 구현
    — `#48` Phase 1과 독립적으로 병행 가능.

50. ⏳ **에이전트 터미널 모니터링·CLI 직접 접속 (tmux 기반)** (P2, 신규 기능,
    `#49`에 전적으로 의존) — **설계 완료, 구현 대기**. 사용자 요청으로 신규
    등록: "worker의 동작을 tmux로 터미널 동작을 모니터링하고 cli로 직접
    연결하는 것을 지원하고 싶다."

    **핵심 설계 결정** (AskUserQuestion, 2026-08-14): 연결 방식은
    **하이브리드**(기본 읽기 전용 모니터링 + 필요 시 SSH+tmux 인터랙티브
    attach로 에스컬레이션), 적용 범위는 **`#49` 이후부터**(에이전트별 다중
    프로세스가 생긴 뒤). `#49` Phase 4가 `GrokRunner`를 재작성하며 이미
    지적했던 "다중 프로세스 로그 수집 부재"(§13) 문제를, grok을 tmux
    세션 안에서 실행하는 것만으로 사실상 함께 해소하도록 설계했습니다.

    읽기 전용 스냅샷은 `#49`의 `agent_commands`/heartbeat 폴링 큐를 그대로
    재사용(`command_type=capture_terminal` 신설, ack 응답에 `result` 필드
    추가)해 새 인바운드 채널을 만들지 않았습니다. 인터랙티브 attach는
    새 WebSocket 릴레이(오케스트레이터가 기존 SSH 키 볼트로 호스트에 붙어
    PTY를 열고, `fleet-cli`와는 raw 바이트로 중계)가 필요해 신규 RBAC
    권한 `AgentAttach`(Admin 기본 전용, `AgentManage`/`AgentDelete`보다
    상위 등급)를 신설했습니다 — 사실상 호스트 셸 접근과 동급의 민감한
    작업이므로 별도 감사 로그(`agent_attached`/`agent_detached`)도
    필수로 뒀습니다.

    전체 설계(아키텍처 다이어그램, 읽기 전용/인터랙티브 두 시퀀스
    다이어그램, RBAC, API/CLI 표면, 호스트 프로비저닝 변경, 동시 attach
    정책, 열린 질문)는
    [`docs/architecture/agent-terminal-access-design.md`](../architecture/agent-terminal-access-design.md)에
    정리했습니다.

    ⚠️ **2차 개정 (자체 재감사, 2026-08-14)**: 사용자가 "tmux 이슈가
    완전히 해결됐나, 숨긴 게 있나"고 직접 반문 — 재감사 결과 본문에
    확정처럼 서술했지만 실은 미검증인 가정이 다수 발견됐습니다. 가장
    심각한 두 가지: (1) **tmux 서버가 `fleet-worker`/systemd 재시작에서
    실제로 살아남는지 미검증** — `KillMode=control-group`이면 재시작 시
    tmux까지 함께 죽어 이 설계의 핵심 가치 제안(워커 재시작 생존성) 자체가
    무효화될 수 있음. (2) **`russh`가 PTY+exec 조합을 지원하는지 미검증**
    — 인터랙티브 attach 전체가 여기 달려 있음. 이 둘은 구현 착수 전
    Phase 0 성격의 실기기 검증이 사실상 필수입니다. 그 외에도 동시 세션
    생성 레이스, `capture_terminal` 큐잉 모델, 결과 텍스트 보존 정책,
    tmux 소켓 권한 등 총 14개 미해결 항목을 설계 문서 §9에 심각도순으로
    전면 재정리했습니다.

    **다음 단계**: `#49` Phase 4가 완료된 뒤, 위 §9 최우선 2개 항목의
    실기기 검증부터 — 그 결과가 부정적이면 §2 핵심 설계 결정을 다시
    논의. 그 전까지는 착수 대상 아님.

---

## 현재 진행 상황 (2026-08-11 기준)

> **P0·P1은 전부 해소됐다.** 남은 항목은 모두 P2 이하다.

### 남은 작업 배정
| 담당 | 항목 |
|---|---|
| 미배정 | #48, #49, #50 |

> **2026-08-14 기준**: `#14`(다크모드/정렬/필터) 완료, `#26`(시크릿 매니저)은
> 재평가 후 🔵 최하위 강등, 그 외 전 항목 ✅ 해결/🔵 재평가-강등 상태였다가,
> 같은 날 사용자 요청으로 `#48`(프로젝트 기능 도입) → `#49`(에이전트 동적
> 프로비저닝/메모리/스레드 요약, `#48` 요구사항의 확장) → `#50`(에이전트
> 터미널 모니터링·CLI 직접 접속, `#49`에 전적으로 의존) 순서로 신규
> 등록됐다 — 셋 다 설계는 완료([`project-feature-design.md`](../architecture/project-feature-design.md),
> [`agent-provisioning-design.md`](../architecture/agent-provisioning-design.md),
> [`agent-terminal-access-design.md`](../architecture/agent-terminal-access-design.md)),
> 구현은 아직 시작 전이라 미배정 상태.

> ⚠️ **정정 (2026-08-13)**: 이 표가 #32를 여전히 "security 담당·미해결"로 열거하고
> 있었으나, 해당 항목 본문은 이미 "✅ 해결됨(`db614ec`)"으로 끝나 있었다 — 헤더
> 아이콘과 이 표 갱신이 누락된 채 방치된 사례여서 제거했다. `#27`도 원래 목록에
> 있었으나 본문이 "🔵 사실상 충족 — 종료 또는 최하위 강등 권고"로 이미 사실상
> 실질 작업이 아니라고 결론 낸 항목이라 함께 제거했다.
>
> ⚠️ **정정 (2026-08-14)**: `#22`도 재평가 후 "🔵 재평가 후 최하위 강등"으로
> 결론 낸 항목이라(대시보드 API는 first-party 전용이라 버전 프리픽스의 실질
> 가치가 낮음) `#27`과 같은 사유로 제거했다.

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
