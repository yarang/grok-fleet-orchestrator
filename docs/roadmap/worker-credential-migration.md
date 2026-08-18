---
type: implementation-roadmap
authority: canonical
implementation: partial
verification: test-checked
source: "docs/roadmap/worker-credential-migration.md"
last_verified: "2026-08-18"
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
| 6 | operational credential rotate/revoke/expiry | 대기 | 이전 token deny·Worker reconcile test |
| 7 | stdin/token-file bootstrap 전달 | 대기 | argv/log redaction test |
| 8 | `bootstrap_token` 및 평면 bearer legacy 삭제 | 대기 | old config fail-closed test |
| 9 | Worker mTLS enrollment/rotation | 대기 | certificate self-binding E2E |
| 10 | staging migration rehearsal·audit·release gate | 대기 | 운영 증거·runbook |

## 현재 경계

- join 성공 시 `fwo_` operational token 원문은 worker.toml에 한 번만 기록하며, 저장소에는 digest만 남긴다.
- bootstrap consume·Worker insert·credential insert는 PgStore에서 단일 DB 트랜잭션으로, MemStore에서 단일 lock 범위로 묶여 있다 (2~4단계 완료). 중간 실패 시 bootstrap token 소진·Worker 생성·credential 저장 중 무엇도 반영되지 않는다.
- 발급된 operational credential은 register/heartbeat/deregister 세 경로에서 `ctx.worker_id`와 요청 대상 `worker_id`를 비교해 검증한다 (5단계 완료). admin bearer·Cloudflare Access·no-auth 경로(`ctx.worker_id == None`)는 이 제약을 받지 않는다.
- credential rotate/revoke, 만료된 credential에 대한 강제 재-enroll, stdin/token-file bootstrap 전달, legacy `bootstrap_token`·평면 bearer 삭제, mTLS 전환, staging rehearsal은 모두 착수 전이다 (6~10단계).
