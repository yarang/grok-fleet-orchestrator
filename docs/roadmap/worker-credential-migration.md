---
type: implementation-roadmap
authority: canonical
implementation: partial
verification: integration-tested
source: "docs/roadmap/worker-credential-migration.md"
last_verified: "2026-08-19"
owners: ["fleet-api", "fleet-worker", "security"]
---

# Worker Credential 점진 전환 계획

## 원칙

새 credential 경로를 먼저 완성하고 검증한 뒤에만 이전 경로를 삭제한다. bootstrap token은 가입
승인에만 쓰며, 정상 운영은 Worker별 operational credential 하나만 사용한다. 이전 token을 자동
fallback으로 허용하지 않는다.

```mermaid
flowchart LR
    A["계약·삭제 기준"] --> B["Atomic enrollment"]
    B --> C["Worker self identity"]
    C --> D["Rotate / revoke"]
    D --> E["Safe bootstrap delivery"]
    E --> F["Legacy config detection"]
    F --> G["Re-enroll"]
    G --> H["Legacy path deletion"]
    H --> I["mTLS transition"]
    I --> J["E2E release gate"]
```

## 10개 구현 단계

| 단계 | 결과 | 상태 | 완료 증거 |
|---|---|---|---|
| 1 | 계약·legacy 삭제 기준 정립 | 완료 | Worker enrollment 계약·이 문서 |
| 2 | bootstrap 소비, Worker 생성, credential digest 저장 atomic transaction | 완료 | `PgStore::enroll_worker`(`crates/fleet-store/src/postgres.rs`)가 `pool.begin()` 트랜잭션 하나로 세 단계를 실행하고 실패 시 `tx` drop으로 자동 롤백. `crates/fleet-store/tests/enroll_worker.rs::enroll_worker_commits_all_three_on_success`/`enroll_worker_rolls_back_on_credential_digest_conflict`/`enroll_worker_rolls_back_on_name_conflict` (실제 PostgreSQL 대상, `DATABASE_URL` 필요) 통과 |
| 3 | Memory/test Store의 rollback 동등성 | 완료 | `MemStore::enroll_worker`(`crates/fleet-store/src/mem.rs`)가 `bootstrap_tokens`/`workers`/`worker_operational_credentials` 세 `Mutex`를 한 스코프에서 보유해 all-or-nothing 보장. `crates/fleet-store/src/mem.rs`의 `enroll_worker_tests` 모듈(`enroll_worker_commits_all_three_on_success`/`enroll_worker_rolls_back_on_credential_digest_conflict`/`enroll_worker_rolls_back_on_name_conflict`) 통과 |
| 4 | join handler를 atomic enrollment 호출로 전환 | 완료 | `crates/fleet-api/src/handlers.rs::join_worker`가 `consume_bootstrap_token`/`upsert_worker`/`upsert_worker_operational_credential` 개별 호출 대신 `state.store.enroll_worker(...)` 단일 호출로 재작성됨. `crates/fleet-api/tests/bootstrap_tokens.rs` 통과 |
| 5 | register/heartbeat/deregister `worker:self` binding | 완료 | `AuthorizationContext.worker_id: Option<WorkerId>`(`crates/fleet-api/src/app.rs`), `enforce_worker_self_binding`(`crates/fleet-api/src/handlers.rs`)가 `register_worker`/`heartbeat`/`deregister_worker` 세 핸들러에 모두 적용되어 불일치 시 `ApiError::Forbidden`(403) 반환. `crates/fleet-api/tests/worker_self_binding.rs::heartbeat_for_self_succeeds_but_for_other_worker_is_forbidden`/`register_for_self_succeeds_but_impersonating_other_worker_is_forbidden`/`deregister_self_succeeds_but_deregistering_other_worker_is_forbidden` 통과 |
| 6 | operational credential rotate/revoke/expiry | 완료 | `Store::rotate_worker_operational_credential`/`revoke_worker_operational_credential`(`crates/fleet-store/src/lib.rs`, PgStore/MemStore 구현), `POST /v1/workers/:id/credential/rotate`·`DELETE /v1/workers/:id/credential`(`crates/fleet-api/src/handlers.rs`, `PermissionKind::WorkerCredentialManage` capability로 admin 전용 fail-closed 등록), `fleet workers credential rotate/revoke`(`crates/fleet-cli`). `crates/fleet-api/tests/worker_credential_rotation.rs`(rotate 후 이전 token 401, revoke 후 401, 만료 credential 401, worker 자기 자신의 rotate 시도 403, 존재하지 않는 worker 404) 및 `crates/fleet-store/tests/worker_credential_rotation.rs`(실제 PostgreSQL 대상, `DATABASE_URL` 필요) 통과 |
| 7 | stdin/token-file bootstrap 전달 | 완료 | `fleet_worker::join::resolve_bootstrap_token`이 `--token`(deprecated, argv/env 노출 경고 로그만 남기고 원문은 남기지 않음)과 `--token-file <path>`(`-`면 stdin)를 지원. `crates/fleet-worker/src/join.rs`의 `resolve_token_reads_from_stdin_when_path_is_dash`/`resolve_token_reads_from_file`/`resolve_token_rejects_both_sources`/`resolve_token_argv_path_does_not_leak_token_into_logs`(캡처한 로그에 원문 부재를 assert) 통과 |
| 8 | `bootstrap_token` 및 평면 bearer legacy 삭제 | 완료 | `fleet-worker`가 `worker.toml`의 `bootstrap_token`을 register/heartbeat/deregister 장기 bearer로 재사용하던 legacy 필드를 `operational_token`으로 교체하고, 남은 `[worker] bootstrap_token` 키는 `WorkerConfig::from_str`에서 명시적으로 거부(fail-closed)한다(`crates/fleet-worker/src/config.rs::reject_legacy_bootstrap_token_field`). 이 과정에서 `operational_token`이 register/heartbeat/deregister 요청에 실제로 bearer로 전송되지 않던 배선 누락도 함께 고쳤다(`crates/fleet-worker/src/registration.rs`). `crates/fleet-worker/src/config.rs::legacy_bootstrap_token_field_is_explicitly_rejected`(old config fail-closed test)/`operational_token_field_parses_correctly`/`config_without_any_token_field_still_parses`, `crates/fleet-worker/src/registration.rs::operational_token_is_sent_as_register_bearer`/`no_operational_token_sends_no_authorization_header` 통과. fleet-api의 평면 bearer 목록(`FLEET_API_TOKENS` 쉼표 구분 문자열)은 이번 세션 이전에 이미 principal·capability를 가진 JSON manifest(`ApiTokenCredential`)로 전환되어 있었다(별도 미착수 우선순위였던 #57~59 관련 작업, 이번 #60 6~8단계와 별개 커밋 경계) |
| 9 | Worker mTLS enrollment/rotation | 대기 | 실제 사설 CA/PKI 인프라가 없어 이번 세션 범위 밖. 아래 "9단계 설계 메모" 참고 — 코드 스캐폴드는 만들지 않았다 |
| 10 | staging migration rehearsal·audit·release gate | 대기 | 별도 staging 환경이 없어 이번 세션 범위 밖. 9단계가 실착수된 뒤에만 의미 있는 rehearsal 대상이 생긴다 |

## 현재 경계

- join 성공 시 `fwo_` operational token 원문은 worker.toml에 한 번만 기록하며, 저장소에는 digest만 남긴다.
- bootstrap consume·Worker insert·credential insert는 PgStore에서 단일 DB 트랜잭션으로, MemStore에서 단일 lock 범위로 묶여 있다 (2~4단계 완료). 중간 실패 시 bootstrap token 소진·Worker 생성·credential 저장 중 무엇도 반영되지 않는다.
- 발급된 operational credential은 register/heartbeat/deregister 세 경로에서 `ctx.worker_id`와 요청 대상 `worker_id`를 비교해 검증한다 (5단계 완료). admin bearer·Cloudflare Access·no-auth 경로(`ctx.worker_id == None`)는 이 제약을 받지 않는다.
- credential rotate/revoke, 만료된 credential 거부, stdin/token-file bootstrap 전달, legacy `bootstrap_token` worker.toml 필드 거부는 모두 구현·테스트 완료됐다 (6~8단계).
- **알려진 갭**: SSH 자동 프로비저닝(`fleet provision`, `crates/fleet-provisioner/src/templates.rs::render_worker_config`)은 여전히 `ProvisionOptions.bootstrap_token`을 `worker.toml`의 `[worker] bootstrap_token`으로 그대로 기록한다. 8단계의 fail-closed 검사 덕분에 이렇게 생성된 worker.toml은 `fleet-worker` 기동 시점에 즉시, 명확한 에러로 거부되므로 인증 없이 조용히 운영되는 위험은 없다 — 다만 `fleet provision` 흐름 자체가 여전히 legacy 필드를 생성한다는 점은 남은 작업이다. 프로비저닝된 워커는 별도로 `fleet-worker join`을 실행하거나 `fleet workers credential rotate <worker_id>` 출력값을 `operational_token`에 수동 반영해야 daemon이 기동한다. `fleet provision`을 `/v1/workers/join`과 자동 연동하는 것은 이 문서의 8단계 범위를 넘어서는 별도 증분으로 남겨둔다.
- mTLS 전환(9단계)과 staging rehearsal(10단계)은 착수 전이다.

## 9단계 설계 메모 (코드 스캐폴드 없음)

실제 PKI/사설 CA 인프라가 갖춰지기 전까지는 착수하지 않지만, 방향을 기록해 둔다:

- Worker가 `fleet-worker join` 시점에 CSR(Certificate Signing Request)을 생성해 orchestrator에 제출하고, orchestrator(또는 그 뒤의 Security Manager)가 사설 CA로 서명한 짧은 TTL의 클라이언트 인증서를 발급하는 흐름을 목표로 한다. `crates/fleet-worker/src/config.rs::MtlsSection`이 이미 서버 측(worker가 mTLS로 자신을 노출) 설정을 담고 있으므로, orchestrator→worker 방향의 클라이언트 인증서 발급/회전은 대칭적인 별도 흐름으로 추가한다.
- 회전은 `operational_token` rotate와 마찬가지로 in-place 교체를 기본으로 하되, 인증서는 만료 전 사전 회전(예: 유효기간의 2/3 경과 시점)이 필요하다 — 토큰과 달리 재발급 지연이 곧 서비스 중단으로 이어지기 때문이다.
- self-binding(`worker:self`) 모델을 그대로 유지한다 — 인증서 subject/SAN에 `worker_id`를 담아 `enforce_worker_self_binding`이 동일하게 검사할 수 있게 한다.
