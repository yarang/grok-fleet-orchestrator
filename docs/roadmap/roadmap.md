---
type: implementation-roadmap
authority: canonical
implementation: partial
verification: code-checked
source: "docs/roadmap/roadmap.md"
last_verified: "2026-08-18"
last_verified_commit: "working-tree"
owners: ["planning"]
---

# 구현 로드맵

이 문서는 승인된 설계를 어떤 순서로 구현할지, 각 작업의 상태와 완료 게이트가 무엇인지 관리한다.
시스템 동작과 계약의 정본은 각 Architecture·Security·Contracts 문서이며, 이 문서는 설계를
재서술하지 않는다. 상태와 ID 규칙은 [Roadmap 도메인 안내](README.md)를 따른다.

## 현재 구현 순서

각 단계는 이전 단계의 정상·실패 경로 검증을 통과한 뒤에만 활성화한다. 신규 Agent/Project 기능은
direct-worker Task 경로를 대체하지 않고 feature flag 뒤에서 점진적으로 도입한다.

| 단계 | 배포 가능한 increment | 포함 Roadmap ID | 우선순위 | 활성화 게이트 |
|---:|---|---|---|---|
| 0 | production trust foundation | #58, #59 | P1 | no-auth fail-closed, endpoint/tool authorization matrix, bootstrap 원문 DB 부재 |
| 1 | Worker identity와 credential authority | #60, #66, #55 | P1 | mTLS/self binding, Security Manager delivery grant, raw export 비활성화 |
| 2 | 실행 일관성과 제어 권한 | #62, #63, #61 | P1 | TaskAttempt CAS, control epoch, inventory-first recovery, bounded control channel |
| 3 | durable workspace와 공유 Host 안전성 | #69, #64 | P1 | Git checkpoint 복구, container isolation, privileged helper 거절 경로 |
| 4 | 최소 Project·Agent lifecycle | #48, #67, #49 | P2 | Project scope, one Agent/one Attempt, Hibernated E2E; WarmIdle 비활성 |
| 5 | 운영 완결성 | #68, #70, #65, #50 | P1~P2 | archive hold, reconciliation/alert, reproducible skill, attach security gate |
| 6 | 확장 기능 | #51, #52, #53, #54, #56 | P2 | 앞 단계 불변식 유지와 vendor/gateway 회귀 검증 |

**첫 착수 단위**는 #58의 `AuthorizationContext`/fail-closed middleware와 #59의 bootstrap digest
migration이다. 이 단계가 완료되기 전에는 Project/Agent control API, Security Manager delivery API,
MCP mutation 확장을 외부에 노출하지 않는다.

P1은 프로덕션 보안과 실행 신뢰성 게이트다. P2 기능 확장은 관련 P1 불변식을 우회하지 않는
범위에서만 병행한다. 긴급 credential 폐기·재발급 같은 사고 대응은 이 순서보다 우선한다.

## 활성 항목

### 기능 확장 (#48-#52)

| ID | 항목 | 상태 | 설계 정본 | 다음 완료 게이트 |
|---|---|---|---|---|
| #48 | Project 기능 | 설계 완료·구현 대기 | [모델](../architecture/project-feature-design.md), [계약](../contracts/project-management.md) | 스키마·소유권·dispatch 자격·RBAC 통합 테스트 |
| #49 | Agent provisioning·memory·summary·tool binding | 설계 완료·구현 대기 | [Agent provisioning](../architecture/agents/provisioning.md), [외부 계약](../contracts/agent-management.md) | #48/#67 뒤 Hibernated 단일 Agent E2E; WarmIdle은 별도 flag |
| #50 | Agent terminal monitoring·CLI attach | 설계 완료·구현 대기 | [Terminal access](../architecture/agents/terminal-access.md) | #49 Phase 4 뒤 cleanup·attach 권한·gateway E2E 검증 |
| #51 | Agent harness·Skill·project constitution | 설계 완료·구현 대기 | [Harness composition](../architecture/agents/harness-composition.md) | #49 tool binding과 합성 순서·권한·revision 검증 |
| #52 | Multi-vendor Agent runtime | 설계 완료·구현 대기 | [Runtime adapters](../architecture/agents/runtime-adapters.md) | #49 Phase 0에서 transport와 native Skill 동작 검증 |

### LLM gateway (#53-#56)

| ID | 항목 | 우선순위·상태 | 정본 | 완료 게이트 |
|---|---|---|---|---|
| #53 | Worker LLM proxy 설정 원자성 | 완료 | [LLM Gateway](../architecture/llm-gateway.md) | `crates/fleet-worker/src/config.rs`의 `WorkerConfig::validate()`가 `[llm_proxy]`의 `gateway_url`/`api_key` 중 한쪽만 채워진 조합과 스킴 없는 `gateway_url`을 `WorkerError::Config`로 fail-closed 거부. `from_str` 통합 테스트 `llm_proxy_gateway_url_only_is_rejected`, `llm_proxy_api_key_only_is_rejected`, `llm_proxy_gateway_url_without_scheme_is_rejected`, `llm_proxy_valid_combination_parses_ok`, `llm_proxy_section_absent_parses_ok`로 검증 |
| #54 | liteLLM 배포 hardening | P1 · 구현 대기 | [배포 Runbook](../deployment/litellm-gateway.md) | secret 외부 주입, 고정 image version, healthcheck, 비공개 기본 bind 검증 |
| #55 | Worker-scoped gateway credential | P1 · 설계 필요 | [LLM Gateway](../architecture/llm-gateway.md), [보안 모델](../security/control-plane-security-model.md) | principal·scope·발급·전달·회수·회전·감사 계약과 격리 테스트 |
| #56 | Provider·gateway·quota 실패 상태 분리 | P2 · 설계 필요 | [LLM Gateway](../architecture/llm-gateway.md), [Routing](../architecture/tasks/routing-policy.md) | 상태·telemetry·fallback 계약과 오분류 방지 테스트 |

### 보안과 실행 신뢰성 (#57-#76)

| ID | 항목 | 우선순위·상태 | 정본 | 완료 게이트 |
|---|---|---|---|---|
| #57 | Bootstrap token 공개 식별자 전환 | P1 · 완료 | [보안 발견 S9](../security/reports/security-findings.md) | API·CLI(`token_id` 발급/list/revoke) 라운드트립과 원문 미노출을 `crates/fleet-api/tests/bootstrap_tokens.rs`로, 017 마이그레이션 이전 발급된 plaintext 토큰이 마이그레이션 뒤에도 원문으로 인증됨을 `crates/fleet-store/tests/bootstrap_token_migration.rs::legacy_plaintext_token_still_authenticates_after_digest_migration`으로 검증 |
| #58 | HTTP·MCP principal/capability authorization | P1 · 부분 구현 | [보안 모델](../security/control-plane-security-model.md) | HTTP scoped bearer·MCP launcher capability allow-list에 이어, CF Access 전용 배포에서도 `auth_middleware`가 검증된 `VerifiedUser.email`을 `AuthorizationContext.principal_id`로 연결하고 `authorize_http_endpoint` capability 검사를 반드시 거치도록 고쳤다(이전엔 CF-only 경로가 capability 검사 자체를 건너뛰었음). 매핑 없는 CF principal은 호환성을 위해 잠정적으로 전체 capability를 받는다 — least privilege 아님, 후속 작업 필요(문서화됨). Project scope/confused-deputy 검증은 `#48`(Project 기능) 구현 후에만 대상이 생기므로 의도적으로 열어둠 |
| #59 | Bootstrap token digest 저장 | P1 · 완료 | [보안 모델](../security/control-plane-security-model.md) | digest 저장·1회 원문 반환·digest 기반 회수에 이어, 실제 PostgreSQL에 001~018 마이그레이션을 전부 적용하고 발급된 bootstrap token·join으로 얻은 worker operational token 모두 `pg_dump` 결과에 원문이 나타나지 않음을 `crates/fleet-api/tests/bootstrap_token_dump.rs::issued_bootstrap_token_never_appears_in_a_database_dump`로 검증 |
| #60 | Worker operational identity | P1 · 부분 구현 | [보안 모델](../security/control-plane-security-model.md), [Enrollment](../contracts/worker-enrollment.md) | join은 digest-only operational token을 1회 발급하며, bootstrap 소비·Worker 생성·credential 저장이 atomic enrollment transaction으로 묶였다(PgStore `.begin()`, MemStore 단일 lock). register/heartbeat/deregister는 `worker:self` binding을 검사해 다른 worker_id를 조작하면 403을 반환한다(테스트로 검증됨). credential rotate/revoke API(`worker:credential:manage` capability), stdin/token-file bootstrap 전달, worker.toml legacy `bootstrap_token` fail-closed 거부까지 구현·테스트 완료(6~8단계). mTLS(9단계), staging rehearsal(10단계)은 PKI/staging 인프라 부재로 착수 전. 상세는 [Worker credential 전환](worker-credential-migration.md) |
| #61 | Worker liveness mode | P1 · 부분 구현 | [Worker liveness](../architecture/worker-liveness-policy.md) | 설계 문서의 5단계 중 1~2단계 완료: `WorkerLivenessMode`(`periodic`\|`on_demand`, 기본값 `periodic`) 필드가 스키마·register API·worker.toml 전 구간에 배선됐고, HealthChecker는 `on_demand` Worker를 heartbeat-timeout 기반 Offline 판정에서 제외한다. **3~5단계(on_demand 실제 dispatch용 bounded ACP probe, `last_activity_at` 기록, 1000-idle-worker E2E)는 의도적으로 미착수** — 설계 문서 자신이 명시하듯 heartbeat 없이 Agent start/stop/capture 명령을 전달할 별도 control-stream/bounded poll 인프라(`#67` 영역, 미구현) 없이는 on_demand를 실제로 안전하게 켤 수 없다 |
| #62 | TaskAttempt·멱등성 | P1 · 설계 확정·구현 대기 | [실행 일관성](../architecture/tasks/execution-consistency.md) | 늦은 완료·취소 경쟁·중복 제출·이전 epoch·비가역 부작용 테스트 |
| #63 | Cold Standby fencing | P1 · 설계 확정·구현 대기 | [권한과 장애 전환](../architecture/control-plane-authority-and-failover.md) | 이중 lease 거부, lease 상실 fail-closed, 이전 epoch 거부, 수동 승격 E2E |
| #64 | Agent 실행 격리 | P1 · 설계 확정·구현 대기 | [실행 격리](../architecture/agents/execution-isolation.md) | 다른 Agent의 process·workspace·credential 접근 차단과 wipe 실패 안전성 테스트 |
| #65 | 재현 가능한 Skill loading | P2 · 설계 확정·구현 대기 | [Harness composition](../architecture/agents/harness-composition.md) | revision 불일치 fail-closed, manifest 기록, 재실행 입력 동일성 테스트 |
| #66 | Worker LLM credential 보관·전달 강화 | P1 · 부분 구현 | [보안 모델](../security/control-plane-security-model.md), [Credential 레지스트리](../credentials/registry.md) | 이번에 닫힌 범위(인가·감사): `crates/fleet-api/src/app.rs`의 `required_capability` 행렬에 LLM credential 하위 자원(`/workers/{name}/credentials…`) 네 route가 전부 빠져 있어, 인증만 통과하면(capability가 빈 principal이나 워커 자신의 `fwo_` operational 토큰으로도) 어떤 워커의 LLM 프로바이더 API 키든 `GET .../export`로 평문 열람이 가능했고 감사 기록도 남지 않았다(`DELETE .../credentials/{model}`은 `worker:delete` 행렬 항목에 잘못 흡수돼 워커 토큰으로도 삭제 가능). `PermissionKind`에 `worker:llm_credential:read`/`:export`/`:manage` 세 capability를 신설하고(operational identity용 기존 `worker:credential:manage`와 이름·의미 모두 분리), GET 목록=read, GET export=export, PUT/DELETE=manage로 fail-closed 매핑했다. export는 저장·삭제 권한으로도 열리지 않는다 — 프로비저너 토큰(read+export)이 credential을 덮어쓰거나 지울 수 없게 하기 위해서다. `export`/`put`/`delete`는 `AuditEvent`(`worker.llm_credential.*`)로 기록하며, export는 감사 기록에 실패하면 평문을 반환하지 않는다(누가 키를 가져갔는지 남기지 못하면 열람 자체를 거부한다). **운영 영향**: 기존 배포의 프로비저너/CLI bearer 토큰 manifest에 새 capability를 추가하지 않으면 `fleet provision`의 `PushCredentials` 스텝이 403으로 실패한다. 남은 범위: TaskAttempt·Worker에 바인딩된 1회용 전달 grant(`#62` 의존, 미착수), 암호화 backend 추상화 교체, rotation 전체 워크플로, Security Manager 분리 | `crates/fleet-api/tests/worker_llm_credential_authz.rs`(capability 없는 principal·워커 자신의 operational 토큰이 export/list/delete에서 403이고 응답에 키가 실리지 않음, export 성공 시 감사 이벤트 1건 기록·비밀 값 미포함, `manage`만으로는 export 불가, `worker:delete`로는 credential 삭제 불가), `crates/fleet-api/src/app.rs`의 `capability_matrix_covers_llm_credential_routes`/`llm_credential_routes_do_not_collide_with_operational_credential`(단수형 `/credential`과 복수형 `/credentials` 분리), `crates/fleet-core/src/auth.rs`의 `llm_credential_permissions_are_distinct_from_operational_credential` |
| #67 | Worker execution lease·Agent command ACK | P1 · 설계 확정·구현 대기 | [권한과 장애 전환](../architecture/control-plane-authority-and-failover.md), [Agent provisioning](../architecture/agents/provisioning.md) | CAS slot claim, fencing token, Worker incarnation, ACK 유실 `OutcomeUnknown`, self-fencing E2E |
| #68 | Project archive·retention·hold | P2 · 설계 확정·구현 대기 | [Lifecycle](../architecture/project-task-agent-lifecycle.md) | idempotent drain, effect/cleanup hold, retention/reopen, archive-blocked E2E |
| #69 | Project Git workspace·checkpoint recovery | P1 · 설계 확정·구현 대기 | [Entity placement](../architecture/entity-placement-and-context.md), [실행 격리](../architecture/agents/execution-isolation.md) | Agent worktree isolation, checkpoint push/restore, secret scan, Worker 이동 복구 E2E |
| #70 | Observability·reconciliation·recovery | P1 · 설계 확정·구현 대기 | [관측성·재조정·장애 복구](../architecture/observability-and-reconciliation.md) | inventory-first recovery, orphan/grant/effect quarantine, secret-free metric/audit, alert/runbook E2E. **`#90` 취소로 흡수(2026-08-23)**: 실패 원인별 관측 수단이 없다 — `/metrics`는 `phase=failed` 집계만 있고 kind 분해가 없어 운영자가 "credential 미프로비저닝으로 몇 건 실패했나"를 집계할 수 없다. `task_failed`에 `kind` 라벨을 추가한다. 함께 정리할 것: `FailureKind`의 `Timeout`·`AuthFailed`·`Cancelled` 세 variant가 저장소 어디서도 생성되지 않는 죽은 코드다 — 구현하거나 제거한다 |
| #71 | Task dispatch credential precondition | P1 · 완료 | 이 행 (구현 요약: `crates/fleet-scheduler/src/selector.rs`의 worker 후보 선택 단계에 credential 매칭 필터를 추가했다. 로드맵 설계 초안은 `Task.resolved_model` 기준을 제안했으나 구현 중 코드 대조 결과 두 가지 이유로 `Task.model` 기준으로 조정했다: (1) `dispatcher.rs`의 `DispatchRequest.model`(워커에 실제로 전달되는 필드)이 `task.model`이지 `resolved_model`이 아니고, (2) `HeuristicTaskRouter::resolve_routing`이 `model` 미지정 task에도 항상 `resolved_model`을 채우므로 이를 기준으로 삼으면 일반 task까지 credential 보유 워커로 강제 제한해 기존 동작과 테스트 스위트 대부분을 깨뜨린다. `task.model`이 `Some`이면 `Store::get_worker_credential(worker.name, model)`이 `Some`을 반환하는 worker만 후보로 남기고(label 필터와 동일 자리), 후보가 전부 제외되면 `SelectionError::NoWorkerForCredential`을 반환한다. `Dispatcher::dispatch_existing`은 재시도 비활성(즉시 실패) 경로에서 이 에러를 `FailureKind::CredentialMissing`으로 매핑하고, `Reconciler`는 재시도 소진 dead-letter 직전 selector를 한 번 더(부작용 없이) 호출해 같은 방식으로 분류한다. `model`이 없는 task는 이 검사를 건너뛴다.) | `crates/fleet-scheduler/src/selector.rs`의 `select_credential_required_and_present_routes_normally`/`select_credential_missing_on_all_candidates_errors`/`select_credential_partial_provisioning_routes_to_credentialed_worker`/`select_no_model_skips_credential_check`, `crates/fleet-scheduler/src/dispatcher.rs`의 `submit_marks_credential_missing_when_worker_lacks_credential`/`submit_selects_worker_when_credential_present`, `crates/fleet-scheduler/src/reconcile.rs`의 `stale_pending_task_dead_letters_as_credential_missing_when_no_worker_has_credential`, `crates/fleet-core/src/task.rs`의 `failure_kind_credential_missing_snake_case` |
| #72 | Admin API bearer token DB 기반 rotate/revoke | P1 · 완료 | [보안 모델](../security/control-plane-security-model.md) | `#60`과 동일 패턴(digest 저장, 1회 원문 응답, rotate 즉시 이전 값 무효화)을 admin bearer 토큰에 적용했다. `admin_api_tokens` 테이블(`principal_id` PK, `token_digest` UNIQUE, `capabilities`, `rotation_generation`), `POST /v1/admin/tokens`(생성)·`POST /v1/admin/tokens/:principal_id/rotate`·`DELETE /v1/admin/tokens/:principal_id`·`GET /v1/admin/tokens`(메타데이터만) API, 신규 `admin_token:manage`/`admin_token:list` capability(bootstrap token 전용 `token:*`과 분리)로 fail-closed 매핑했다. `sync_env_admin_tokens_to_store`가 서버 기동 시 `FLEET_API_TOKENS`의 각 항목을 멱등하게 DB로 1회 upsert해, `auth_middleware`가 env 목록과 DB 활성 digest를 모두 검사하는 무중단 전환을 구현했다. `fleet admin-tokens create/rotate/revoke/list` CLI 추가. `FLEET_GMAIL_APP_PASS`류 제3자 발급 시크릿은 범위 밖(문서에 명시) | `crates/fleet-api/tests/admin_token_rotation.rs`(생성→인증 성공, rotate 후 이전 토큰 401·새 토큰 200, revoke 후 401, capability 분리로 403, list에 원문·digest 미노출, env→DB 자동 전환 후에도 env 토큰 인증 유지), `crates/fleet-store/tests/admin_token_rotation.rs`(PgStore 대상 create/rotate/revoke/list, 중복 principal_id Conflict, 존재하지 않는 principal NotFound) |
| #73 | HTTP capability 행렬 기본 deny 전환 | **완료** | [Authorization](../security/authorization-and-audit.md) | `crates/fleet-api/src/app.rs`의 `authorize_http_endpoint`가 `required_capability`의 `None`을 `403`으로 처리하도록 전환했다. `/health`와 `POST /workers/join`(body의 bootstrap token이 자체 인증 수단)만 명시적으로 허용한다. 과거 누락이었던 `GET /v1/workers/{id}`(신설 `worker:list` 매핑)와 `POST /hosts/register`(신설 `host:provision` 매핑)를 행렬에 등록했다 — 전자는 워커의 ACP `server-key`를 담은 `endpoint` 필드를 무권한 노출했고, 후자는 기존 Host의 `ssh_host`/`ssh_user`/`status`/`worker_id`를 무권한으로 덮어썼다. **범위(Dashboard `/api`) 미착수**: `crates/fleet-dashboard`에는 중앙 capability 행렬이 아예 없고 핸들러에 `PermissionKind` 검사가 29곳 산재한다 — `#86`~`#93` 관리 화면이 이 표면에 놓이므로 `#92`로 넘긴다. 검증: `crates/fleet-api/src/app.rs`의 `capability_matrix_covers_router_routes`/`authorize_http_endpoint_denies_by_default_for_any_unmatched_route`/`get_worker_by_id_and_host_register_now_require_capability`/`is_worker_by_id_route_matches_single_segment_only`, `crates/fleet-api/tests/capability_matrix_default_deny.rs`(8건, 실제 HTTP 스택 — 이전에 뚫려 있던 두 route가 이제 403/200을 정확히 내는지, 워커 operational credential로 다른 워커·Host를 건드릴 수 없는지, `allow_no_auth` 모드에서도 join이 여전히 동작하는지) |
| #74 | Cloudflare principal capability 매핑 fail-closed | P0 · 설계 확정·구현 대기 | [Authorization](../security/authorization-and-audit.md) | `cf_access_capabilities`의 매핑 부재 시 `PermissionKind::all()` 부여를 제거한다. `fleet-cli`에 매핑 설정 경로를 추가하고(현재 `with_cf_principal_capabilities` 호출부가 테스트에만 존재), `FLEET_CF_AUDIENCE`가 설정됐는데 매핑이 없으면 non-loopback 무인증 bind와 동일하게 기동을 거부한다. 매핑 없는 CF principal이 `worker:llm_credential:export`·`admin_token:manage`·`token:issue`를 얻지 못하는 테스트 |
| #75 | Worker endpoint secret 분리 | P1 · 설계 확정·구현 대기 | [Enrollment](../contracts/worker-enrollment.md) | `agent_endpoint`의 `?server-key=` 값이 `workers.endpoint`를 거쳐 `GET /v1/workers/{id}`·Dashboard `/api/workers`·`/api/events`·MCP `fleet_list_workers`로 전파되는 경로를 차단한다. 1차로 응답·이벤트에서 마스킹, 최종적으로 ACP 인증을 URL query 밖(헤더 또는 mTLS)으로 이전하고 `[grok].secret`을 join 응답 본문과 분리 전달. 어떤 조회 응답·이벤트·metric에도 `server-key` 원문이 나타나지 않는 redaction 테스트 |
| #76 | 감사 범위 확장과 상관관계 필드 | P1 · 설계 확정·구현 대기 | [Authorization](../security/authorization-and-audit.md) | `AuditEvent`에 `request_id`·`project_id`·`attempt_id`·`policy_revision`을 추가하고, bootstrap/admin token 발급·회수, Worker 삭제·등록, Host 등록, capability 거절을 append-only audit으로 기록한다. LLM credential export의 "감사 실패 시 거절" 패턴을 다른 mutation에도 적용. secret이 audit record에 실리지 않는 테스트 |

### 무인 부트스트랩과 배포 자동화 (#77-#85)

2026-08-22 [무인 부트스트랩 검토](../reviews/bootstrap-automation-review-2026-08-22.md)에서 도출한
항목이다. 순서는 근거를 가진다 — `#77`은 이후 모든 검증의 전제, `#78`은 이미 운영 중인 fleet을
망가뜨리는 유일한 항목, `#79`는 없으면 이후 스텝의 성공 여부를 관측할 수 없다. `#82`는 `#79`와
`#80`에 의존한다.

| ID | 항목 | 우선순위·상태 | 정본 | 완료 게이트 |
|---|---|---|---|---|
| #77 | 배포 예시를 현재 파서에 정합 | 완료 | 이 행 | `examples/fleet.env`의 `FLEET_API_TOKENS`를 `{principal_id, token, capabilities}` JSON 배열로 교체하고 capability 최소권한 안내를 붙였다(`worker:llm_credential:export`는 기본 예시에서 제외). `examples/fleet.service` 주석의 `token1,token2`도 같은 형식으로 교체. `examples/fleet-worker.service` 헤더의 "`fleet provision`이 자동 배포한다" 서술을 정정했다 — 프로비저너가 실제로 쓰는 것은 `templates.rs`의 `FLEET_WORKER_UNIT`(User=root, 하드닝 없음)이며 두 형상 일치는 별도 항목이다. 검증: `crates/fleet-api/tests/verify_env_example.rs`의 `examples_fleet_env_api_tokens_parse_as_manifest`(파서와 동일 조건으로 manifest 파싱·필드 비어있음 거부), `examples_fleet_env_does_not_grant_credential_export_by_default` |
| #78 | Graceful shutdown hard-delete 중단 | 완료 | 이 행 | `crates/fleet-worker/src/runner.rs`의 shutdown 경로에서 `client.deregister()` 호출을 제거했다. 그 요청은 `DELETE /v1/workers/{id}` → `DELETE FROM workers`로 이어져 `worker_operational_credentials`(018 CASCADE)와 `worker_credentials`(005 CASCADE, 암호화된 LLM 키)를 함께 파괴했고, 인증이 구성된 모든 배포에서 `systemctl restart` 한 번이 워커를 영구 401로 만들었다. "이 워커는 이제 없다"는 신호는 `fleet-scheduler`의 HealthChecker가 heartbeat timeout으로 Offline 전이와 `WorkerLeft` 이벤트를 내며 비파괴적으로 담당한다. 영구 제거는 관리자의 `DELETE /v1/workers/{id}`만 수행하고, `WorkerClient::deregister` 메서드는 그 경로용으로 유지했다. `MemStore::delete_worker`를 PgStore와 동작 일치시켰다 — 두 credential 테이블을 함께 제거하고 존재하지 않는 id에 `NotFound`를 반환한다(이 divergence 때문에 결함이 모든 인메모리 테스트를 통과했다). `config.rs`의 legacy 거부 에러가 안내하던 `credential rotate` 복구 경로가 404임을 명시하도록 정정. 검증: `crates/fleet-store/src/mem.rs`의 `delete_worker_cascade_tests`(2종), `crates/fleet-store/tests/worker_delete_cascade.rs`(2종, 실제 PostgreSQL에서 두 CASCADE와 무관 워커 자산 보존 확인) | **후속**: `WorkerConfig`의 `rotation_generation` 제약(PG는 `>= 1` CHECK, MemStore는 미강제)이라는 별도 divergence를 발견했다 — 추적 필요 |
| #79 | 원격 실행 실패 관측 가능화 | **완료** | 이 행 | `crates/fleet-provisioner/src/ssh.rs`의 `RemoteExecutor`에 `exec_checked` 기본 메서드를 추가했다 — `exec_streaming`을 위임해 exit code를 관측하고 비0이면 `StepError::RemoteExit`로 승격한다(`exec()` 자체는 `test -f ... && echo yes` 같은 조회 명령을 위해 exit code 무시를 유지). `install_fleet_worker.rs`·`push_credentials.rs`·`install_cloudflared.rs`의 `let _ =`/`|| true`로 버려지던 mutation(`sudo mkdir`, `sudo mv`, `daemon-reload` 등)을 `exec_checked`로 교체했다. 구현 중 `install_cloudflared.rs`에서 실제 잠복 버그를 발견했다 — `/etc/cloudflared` 디렉토리를 만드는 스텝이 없어 새 호스트에서 `config.yml` `mv`가 조용히 실패하고 있었다(스텝은 "Applied"로 보고됨); `sudo mkdir -p /etc/cloudflared`를 추가했다. `start_services.rs`의 죽은 `wait_timeout_secs`(참조 0건)를 실제 폴링으로 구현했다: 로컬 `systemctl is-active`를 즉시 1회가 아니라 타임아웃까지 재시도하고, `orchestrator_api_token`이 있으면 `GET /v1/workers`로 워커가 `online`으로 보고될 때까지 폴링한다(토큰 없으면 로컬 확인만 했다고 경고 로그 후 진행 — 하위 호환). 401/403은 재시도해도 해소되지 않으므로 `NotFound` 타임아웃과 구분해 즉시 실패시킨다 — capability 부족을 "워커가 등록 안 됨"으로 오인시키지 않기 위해서다(이 구분 자체가 `#79`의 요지를 재확인한 발견). `PlaybookError::StepFailed`에 `completed_steps: Vec<StepReport>`를 추가하고 `fleet-cli`의 실패 처리 두 곳(`recover_completed_steps`)이 이를 써서 빈 `steps: vec![]` 대신 실제 실행 이력을 리포트에 채우게 했다. 검증: `crates/fleet-provisioner/src/playbook.rs`의 `completed_steps_includes_earlier_skipped_and_applied_steps`, `start_services.rs`의 `apply_fails_when_daemon_reload_fails`/`apply_continues_when_only_cloudflared_enable_fails`/`classify_status_*`(3종), `crates/fleet-cli/src/runtime.rs`의 `recover_completed_steps_tests`(2종) |
| #80 | 최초 admin 토큰 발급 경로 | P1 · 설계 확정·구현 대기 | [보안 모델](../security/control-plane-security-model.md) | `fleet serve` 최초 기동 시 `list_admin_tokens()`가 비어 있으면 full capability 토큰 1개를 발급하고 평문을 `0600` 파일(예 `/etc/fleet/bootstrap-admin-token`)로 1회 출력한다. 저널 싱크는 평문이 영구 잔류하므로 쓰지 않는다. 현재는 `admin-tokens create`/`token issue` 둘 다 기존 bearer를 요구해 최초 토큰을 만들 수 없다. **dashboard OTP 재사용은 금지** — `rbac.rs`의 `issue_admin_bootstrap_if_needed`가 `purpose`를 구분하지 않아(컬럼은 `004_rbac.sql`, 타입은 `auth.rs`에 이미 존재) worker_join 토큰이 하나라도 살아 있으면 admin OTP가 발급되지 않으며, `#81` 적용 후에는 사실상 영구 미발급이 된다. 게이트: 빈 store에서 1회 발급·재기동 시 미발급, 발급 경로가 다른 토큰과 동일한 `AuditEvent`를 남김 |
| #81 | 이기종 fleet 지원 (arch·OS 감지 배선) | P1 · 설계 확정·구현 대기 | 이 행 | `crates/fleet-cli/src/runtime.rs`의 `run_playbook`이 하드코딩한 `os:"ubuntu", arch:"x86_64"`를 제거하고 `check_prereqs`의 실제 출력을 후속 스텝에 전달한다. `install_fleet_worker`는 타깃별 바이너리를, `install_deps`는 패키지 매니저를 선택한다(릴리스 워크플로는 이미 4개 타깃 빌드). 커밋된 인벤토리 25대 중 7대가 arm64다(`oci-*-arm1` 계열). 게이트: arm64 호스트 dry-run이 올바른 타깃을 선택하는 테스트 |
| #82 | provision→join 배선 | P1 · 설계 확정·구현 대기 | [Worker provisioning](../deployment/worker-provisioning.md), [Enrollment](../contracts/worker-enrollment.md) | `RemoteExecutor::exec_with_stdin`을 신설해 `fleet-worker join --token-file -`에 토큰을 채널로 직접 쓴다(argv·쉘 히스토리·디스크 미경유; `write_file`은 base64를 명령행에 보간하므로 사용 금지). `StartServices` 앞에 `JoinWorker` 스텝을 추가하고, `templates.rs`/`steps.rs`/`inventory.rs`/`main.rs`에서 `bootstrap_token` 방출과 플래그를 제거하며, `worker.toml` 렌더러를 orchestrator 쪽으로 단일화한다(현재 두 렌더러의 `bind_addr` 기본값이 서로 다름). `is_applied`는 파일 존재가 아니라 신원 검사(`existing_worker_id` → `GET /v1/workers/{id}` 200이면 skip)이며 `existing_worker_id`를 가진 `worker.toml`은 덮어쓰지 않는다. 실패 보상은 전진(재시도·re-key)만 허용하고 워커 삭제로 후퇴하지 않는다. 전제: `FLEET_BASE_URL` 설정, `#79` 선행. 게이트: 무인 provision 후 워커가 스스로 register·heartbeat에 성공, 재실행이 신원을 보존, join 응답 유실 후 재시도가 409를 "이미 가입됨"으로 해석 |
| #83 | 인벤토리 모드 완결성 | P1 · 구현 대기 | 이 행 | `runtime.rs`의 `fleet_worker_bin: None` 하드코딩을 제거하고 `InventoryDefaults`/`InventoryWorker`에 필드를 추가한다. `grok_secret`은 `#82` 이후 orchestrator가 생성하므로 인벤토리에서 제거한다. `api_token` 필수 검증 추가. 선언만 있고 참조 0건인 `retry_failed`는 구현하거나 제거한다. 게이트: 커밋된 25노드 인벤토리로 dry-run이 완주 |
| #84 | 원격 파일 전송 방식 교체 | P1 · 구현 대기 | 이 행 | `ssh.rs`의 `upload_file`이 릴리스 바이너리 전체를 base64로 명령행에 보간해 `ARG_MAX`를 넘기고, 셸 리다이렉트로 생성해 SSH 사용자 umask(보통 0644)를 남긴다. 청크 전송 또는 SFTP + `sudo install -m 0600`으로 교체한다. 게이트: 대용량 바이너리 업로드 성공, 평문 시크릿이 world-readable로 잔류하지 않음 |
| #85 | mTLS 자산 배포와 SAN 일관성 | P1 · 설계 확정·구현 대기 | [Topology](../deployment/topology.md) | Transport 정본을 mTLS 직접 다이얼로 확정한 데 따른 배포 파이프라인 결손을 닫는다. `issue_mtls_assets` 스텝을 신설해 orchestrator측에서 `fleet mtls issue-server`를 워커명 SAN으로 호출하고 `server.pem`/`server.key`/`ca.pem`을 `0600 root`로 업로드하며, `advertised_host`를 SAN과 같은 값으로 강제한다(현재 `inventory.rs` 주석이 "경로만 채운다"고 명시). 표준 playbook에서 `InstallCloudflared`를 제거한다 — 잘못된 아키텍처 바이너리를 받고 에러를 `|| true`로 삼킨 뒤 성공을 보고하며, ingress 포트도 워커의 어떤 리스너와도 맞지 않는다. 런타임(`MtlsProxy`, 인증서 회전, 발급 CLI)은 이미 구현되어 있다. 게이트: 인증서 없이 mTLS 활성화 시 명확한 실패, SAN 불일치 감지, 무인 provision 후 control plane이 실제로 ACP 다이얼에 성공 |

### UI 관리 표면과 Issue 추적 (#86-#93)

2026-08-22 [UI 관리 대상·Issue 명세 설계](../reviews/ui-management-and-issue-spec-2026-08-22.md)에서
도출했다. host·worker·task UI는 이미 구현됐고 project·agent UI는 `#48`·`#49`가 소유하므로 여기서
중복 등록하지 않는다. 신규는 agent_template 정본화와 Issue 추적 둘이다.

**2026-08-23 프레임 정정**: Issue는 orchestrator의 인프라 장애 추적이 아니라 **프로젝트가 해결해야
할 일감을 관리하는 이슈 트래커**다. 이에 따라 dead-letter 자동 Issue 생성 항목을 취소하고 관측
요구를 `#70`으로 옮겼으며, Agent가 백로그에서 일을 집어가는 경로(`#93`)를 추가했다.

**하드 순서 제약**: `#86` catalog 부분(global 템플릿)은 `#48` 전에도 성립하나 project-scoped
템플릿은 `#48` 뒤다. `#88`은 Issue가 정의상 Project scope 자원이라 `#48` 없이 착수할 수 없다.
두 모듈 모두 `#73` 전에 `/v1` route를 추가하면 안 된다 — 행렬 미등록 route가 통과하는 창이 열린다.

| ID | 항목 | 우선순위·상태 | 정본 | 완료 게이트 |
|---|---|---|---|---|
| #86 | AgentTemplate 정본과 revision immutability | P2 · 설계 확정·구현 대기 | [AgentTemplate](../architecture/agents/agent-template.md) | `agent_templates`(정체성)와 `agent_template_revisions`(불변 본문) 2계층. `Draft/Published/Deprecated/Retired/Discarded` 전이와 `Retired → Published` 간선 부재. published revision의 content 변경 거절, 같은 content 재발행이 새 revision id에 같은 `content_hash`. retire가 dependent set 해시 없이 실패하고 집합 변화 시 409. 템플릿이 Project grant를 넘는 tool을 부여하지 못하며 저장 후 grant가 좁아진 경우에도 admission에서 차단. `builtin/default@1` 시드가 두 store에서 같은 `content_hash`이고 tool binding이 `ReadOnly` 등급 한정. MemStore/PgStore 공유 행동 테스트. **권한(2026-08-22 결정)**: 편집은 필드별로 게이팅한다 — `role_prompt`·메타데이터는 `agent_template:update`, `tools`/`skills`/`isolation_class`는 거기에 Agent tool-binding 권한(`AgentManage` 상당)을 추가 요구. `agent_template:update`만 가진 principal의 tool 필드 변경이 403인 시험과, operator 역할(`read`+`update` 부여)이 실질적으로 prompt만 편집 가능함을 역할 번들 기준으로 검증. `#48`의 구현 차단 조건은 이 항목에 적용하지 않는다 — 그 차단은 자동 provisioning을 통한 `AgentCreate` 우회를 겨냥하며 템플릿 편집은 Agent를 만들지 않는다 |
| #87 | Attempt snapshot의 template revision 고정 | P2 · 설계 확정·구현 대기 | [AgentTemplate](../architecture/agents/agent-template.md), [실행 일관성](../architecture/tasks/execution-consistency.md) | 선행 `#62`, `#86`. Attempt는 revision을 참조하지 않고 본문·hash를 materialize한다. 실행 중 revision retire가 Attempt harness manifest hash를 바꾸지 않는 E2E. WarmIdle 호환성 키에 `agent_template_revision_id`가 포함되어 불일치 시 `WarmIdle → Hibernated → Starting`만 발생. `Retired` pin의 `Hibernated → Starting`이 신설 `FailureKind::TemplateUnavailable`로 admission 즉시 거절되고 fallback도 retry 예산 소모도 없음. `Deprecated`는 기동을 막지 않고 경고 metric만 |
| #88 | Issue 엔티티와 상태 머신 | P2 · 설계 확정·구현 대기 | [Issue 추적](../architecture/issues.md) | 선행 `#48`, `#73`, `#76`. `Open/Triaged/ReadyForAgent/Resolved/Closed(+reason)`이며 `InProgress` 부재(비터미널 연관 Task에서 유도). `tasks`에 `issue_id` 컬럼을 두지 않고 join 테이블로 연관. **교착 없음 3종**: 열린 Issue가 있어도 Task가 dead-letter까지 도달, 비터미널 Task가 있어도 Issue close 성공, Attempt 전이 코드 경로가 issue 테이블을 참조하지 않음을 강제하는 구조 시험. MemStore/PgStore 공유 행동 테스트 |
| #89 | Agent 보고 경로와 폭주 방지 | P2 · 설계 확정·구현 대기 | [Issue 추적](../architecture/issues.md), [Agent provisioning](../architecture/agents/provisioning.md) | 선행 `#88`, `#67`. Agent는 principal이 없으므로 Worker control stream 보고 → control plane 대리 생성. `project_id`를 요청 본문이 아니라 저장된 Attempt 행에서 유도(본문 위조가 무시되는 시험). 부분 유니크 인덱스로 동일 blocker N회가 Issue 1건 + `occurrence_count=N`. Attempt당 상한 초과가 Attempt를 실패시키지 않음. Project 버킷 소진 시 `AgentIssueFloodSuspected` alert이고 dedup 적중은 토큰 미소모. metric label에 UUID·prompt 미노출. 감사 실패 시 Issue 생성 거절 |
| #90 | ~~dead-letter → Issue 집계 자동 생성~~ | **취소 (2026-08-23)** | [관측성·재조정](../architecture/observability-and-reconciliation.md) | Issue를 이슈 트래커로 재정의하면서 취소했다 — 인프라 장애는 alert이지 프로젝트 일감이 아니다. 조사 중 확인된 사실: `crates/fleet-scheduler/src/reconcile.rs`의 dead-letter 경로는 `CredentialMissing`과 `WorkerUnavailable` 두 kind만 붙이고, `FailureKind`의 `Timeout`·`AuthFailed`·`Cancelled` 세 variant는 **저장소 어디서도 생성되지 않는다**(죽은 코드). `/metrics`에도 kind별 분해가 없어 운영자가 실패 원인을 집계할 수단이 없다. 이 관측 요구는 `#70`이 흡수한다. ID는 참조 안정성을 위해 재사용하지 않는다 |
| #91 | Issue → archive hold 승격과 drain 순서 | P2 · 설계 확정·구현 대기 | [Issue 추적](../architecture/issues.md), [Lifecycle](../architecture/project-task-agent-lifecycle.md) | 선행 `#68`, `#88`. 열린 Issue만으로는 archive가 막히지 않음. `kind='issue'` hold가 `ArchiveBlocked`를 만들고 기존 hold 해제 경로로만 풀림. 승격에 `issue:update`가 아니라 Project hold capability 필요. `Draining` 중 Issue 쓰기는 허용하고 Issue→Task 생성만 차단. archive 후 read-only 봉인, Project reopen이 Issue를 자동 reopen하지 않음 |
| #92 | AgentTemplate·Issue 관리 표면 노출 | P2 · 설계 확정·구현 대기 | [Authorization](../security/authorization-and-audit.md) | 선행 `#73`(Dashboard 표면 포함), `#86`, `#88`. `agent_template:{read,create,update,archive,revision:revoke,manage_global}`와 `issue:{read,create,comment,update,assign,close,reopen,link,archive_hold_manage}` 신설 — UI 문서의 단일 `AgentTemplateManage`는 채택하지 않고 `#66`·`#72`의 분리 선례를 따른다. 모든 신규 route의 행렬 등록을 강제하는 커버리지 테스트. Project scope 밖 조회는 404, scope 안 권한 부족은 403. `ApiError`에 422/428/429가 없어 낙관적 동시성(`If-Match` → 409/428)과 rate limit(429)을 표현할 수 없으므로 확장이 선행된다. `BuiltinRole::Operator` 고정 목록에 `agent_template:read`/`agent_template:update`를 추가한다(admin은 `PermissionKind::all()`로 자동 보유하며 `builtin_roles_cover_all_permissions`가 강제) |
| #93 | Agent backlog claim (Issue 자동 착수) | P2 · 설계 확정·구현 대기 | [Issue 추적](../architecture/issues.md) | 선행 `#88`, `#89`, `#48`. Agent가 `ReadyForAgent` Issue를 집어 Task를 만든다. **승인은 상태가 소유한다** — `Triaged → ReadyForAgent` 전이는 사람만 하며 신설 `issue:approve_agent_work` capability를 요구한다. claim은 `(issue_id, status, claim_generation)` CAS이고 만료되는 lease를 얻으며, 만료 시 `ReadyForAgent`로 복귀한다(`#62`의 lease 관례 재사용, 새 기구 금지). claim은 Issue 상태를 바꾸지 않는다 — 상태를 하나 더 만들면 `InProgress`를 금지한 것과 같은 문제가 생긴다. Project는 동시 claim 수와 시간당 claim 수를 정책으로 갖는다 — 예산이 없으면 무한 생성-소비 루프가 성립한다. 예산 소진은 실패가 아니라 대기이며 지속되면 alert. 게이트: `ReadyForAgent`가 아닌 Issue는 claim되지 않음, 사람 승인 전이 없이 Agent가 자기가 연 Issue를 착수할 수 없음, claim CAS 경쟁에서 정확히 하나만 성공, lease 만료 복귀, `origin_issue_id` 계보 깊이 상한이 순환을 끊음, `Draining` 중 claim 거절 |

## 보존 항목 레지스트리 (#1-#47)

완료·강등·폐기된 ID는 참조 안정성을 위해 재사용하지 않는다. 상세 변경 경위는 Git 이력과
관련 코드·문서에서 확인한다.

| ID | 항목 | 종결 상태 | 대표 근거 |
|---|---|---|---|
| #1 | Dockerfile | 완료 | `acca872` |
| #2 | docker-compose | 완료 | `acca872` |
| #3 | password reset rate limit | 완료 | `b501ca5` |
| #4 | PgStore auth integration test | 완료 | `afd8d35` |
| #5 | latency·throughput metrics | 완료 | `b501ca5`, `8755c0d` |
| #6 | DB backup script | 완료 | `853a1d0` |
| #7 | restrictive CORS | 완료 | `1d4422e` |
| #8 | fleet-api security headers | 완료 | `1d4422e` |
| #9 | structured audit log | 완료 | `b501ca5` |
| #10 | session token rotation | 완료 | `0177e56` |
| #11 | pagination·label filter propagation | 완료 | `7e17558` |
| #12 | API error envelope | 완료 | `8755c0d` |
| #13 | OpenTelemetry tracing | 완료 | 코드·테스트 |
| #14 | dark mode·sort·filter | 완료 | 코드·테스트 |
| #15 | startup configuration validation | 완료 | `0177e56` |
| #16 | connection pool tuning | 완료 | `bac4dc7` |
| #17 | `fleet_list_tasks` MCP tool | 완료 | `3fb296a` |
| #18 | scheduled DB cleanup | 완료 | `bac4dc7` |
| #19 | migration rollback tooling | 완료 | `853a1d0` |
| #20 | constant-time bearer comparison | 완료 | `1d4422e` |
| #21 | fleet-api OpenAPI | 완료 | 코드·테스트 |
| #22 | Dashboard API version prefix | 강등 | first-party bundled client 전용 |
| #23 | task list pagination UI | 완료 | 코드·테스트 |
| #24 | mobile responsive audit | 완료 | UI 검증 기록 |
| #25 | shared circuit-breaker state | 완료 | 코드·통합 테스트 |
| #26 | external secret manager | 강등 | 현재 배포 범위 밖 |
| #27 | example configuration | 사실상 충족 | `examples/`, deployment docs |
| #28 | token·host·breaker MCP tools | 완료 | 코드·테스트 |
| #29 | dynamic Dashboard tool list | 완료 | `8755c0d` |
| #30 | CI coverage report | 완료 | `afd8d35` |
| #31 | dispatch latency metric | 완료 | `ed82b27` |
| #32 | `/admin/*` HTML RBAC | 완료 | `db614ec` |
| #33 | security findings S1-S6 | 완료 | [보안 발견 이력](../security/reports/security-findings.md) |
| #34 | liteLLM 기본 연동 | 완료·후속 #53-#56 | [LLM Gateway](../architecture/llm-gateway.md) |
| #35 | Mermaid migration·docs consistency | 완료 | Git 이력 |
| #36 | mTLS certificate auto-rotation policy | 완료 | deployment·security docs |
| #37 | inventory-based mTLS provisioning | 완료 | code·runbook |
| #38 | scheduler retry·DLQ | 완료 | code·tests |
| #39 | known-hosts TOFU deployment gap | 완료 | `fleet scan-host-keys` |
| #40 | `xai-circuit-breaker` adoption | 재평가·종결 | 현 구현 유지 |
| #41 | WebSocket demuxer concurrency | 완료 | code·tests |
| #42 | distributed OTLP propagation | 완료 | code·tests |
| #43 | autonomic self-healing engine | 폐기 | Git 이력; ID 재사용 금지 |
| #44 | HalfOpen single-probe enforcement | 완료 | code·tests |
| #45 | duplicated `MemStore` consolidation | 완료 | code·tests |
| #46 | docs fact-check·structure cleanup | 완료 | docs·Git 이력 |
| #47 | HealthChecker↔Task integration | 완료 | code·tests |

## 갱신 시점

- 요구 승인 시 ID, 독자 가치, 우선순위와 책임 문서를 등록한다.
- 설계 승인 시 정본 링크, 구현 단계, 선행 조건과 검증 가능한 완료 게이트를 확정한다.
- 구현 착수 시 `구현 중`, 코드·테스트·운영 문서가 게이트를 충족하면 `완료`로 바꾼다.
- 계약이나 범위 변경은 설계 정본을 먼저 수정한 뒤 이 문서의 순서·게이트만 동기화한다.
- 폐기하거나 강등해도 ID는 삭제·재사용하지 않고 한 줄 종결 기록을 남긴다.

특정 시점의 테스트 개수, 상세 회고와 설계 개정 횟수는 누적하지 않는다. 현재 사실은 CI와 코드,
변경 경위는 Git 이력과 필요한 경우 `docs/reviews/`가 담당한다.
