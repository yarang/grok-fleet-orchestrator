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
| #53 | Worker LLM proxy 설정 원자성 | P1 · 구현 대기 | [LLM Gateway](../architecture/llm-gateway.md) | URL-only, key-only, invalid URL, 정상 조합과 subprocess 환경변수 회귀 테스트 |
| #54 | liteLLM 배포 hardening | P1 · 구현 대기 | [배포 Runbook](../deployment/litellm-gateway.md) | secret 외부 주입, 고정 image version, healthcheck, 비공개 기본 bind 검증 |
| #55 | Worker-scoped gateway credential | P1 · 설계 필요 | [LLM Gateway](../architecture/llm-gateway.md), [보안 모델](../security/control-plane-security-model.md) | principal·scope·발급·전달·회수·회전·감사 계약과 격리 테스트 |
| #56 | Provider·gateway·quota 실패 상태 분리 | P2 · 설계 필요 | [LLM Gateway](../architecture/llm-gateway.md), [Routing](../architecture/tasks/routing-policy.md) | 상태·telemetry·fallback 계약과 오분류 방지 테스트 |

### 보안과 실행 신뢰성 (#57-#70)

| ID | 항목 | 우선순위·상태 | 정본 | 완료 게이트 |
|---|---|---|---|---|
| #57 | Bootstrap token 공개 식별자 전환 | P1 · 구현 중 | [보안 발견 S9](../security/reports/security-findings.md) | CLI list가 `token_id`를 표시하도록 동기화됨. API/CLI E2E와 기존 운영 token migration 검증이 남음 |
| #58 | HTTP·MCP principal/capability authorization | P1 · 구현 중 | [Authorization](../security/authorization-and-audit.md) | HTTP scoped token manifest(principal·capability)와 관리 route capability, MCP launcher capability allow-list, no-provider fail-closed를 적용. Cloudflare claim principal, Worker self identity, Project scope/confused-deputy·audit 테스트가 남음 |
| #59 | Bootstrap token digest 저장 | P1 · 구현 중 | [보안 모델](../security/control-plane-security-model.md) | core/store/API/MCP를 digest 저장·1회 원문 반환·digest 기반 회수로 전환. 실제 PostgreSQL migration/DB dump 회귀 테스트가 남음 |
| #60 | Worker operational identity | P1 · 부분 구현 | [보안 모델](../security/control-plane-security-model.md), [Enrollment](../contracts/worker-enrollment.md) | join은 digest-only operational token을 1회 발급하며, bootstrap 소비·Worker 생성·credential 저장이 atomic enrollment transaction으로 묶였다(PgStore `.begin()`, MemStore 단일 lock). register/heartbeat/deregister는 `worker:self` binding을 검사해 다른 worker_id를 조작하면 403을 반환한다(테스트로 검증됨). credential rotate/revoke API(`worker:credential:manage` capability), stdin/token-file bootstrap 전달, worker.toml legacy `bootstrap_token` fail-closed 거부까지 구현·테스트 완료(6~8단계). mTLS(9단계), staging rehearsal(10단계)은 PKI/staging 인프라 부재로 착수 전. 상세는 [Worker credential 전환](worker-credential-migration.md) |
| #61 | Worker liveness mode | P1 · 설계 확정·구현 대기 | [Worker liveness](../architecture/worker-liveness-policy.md) | periodic/on-demand 전이와 장기 idle 오판 방지 테스트 |
| #62 | TaskAttempt·멱등성 | P1 · 설계 확정·구현 대기 | [실행 일관성](../architecture/tasks/execution-consistency.md) | 늦은 완료·취소 경쟁·중복 제출·이전 epoch·비가역 부작용 테스트 |
| #63 | Cold Standby fencing | P1 · 설계 확정·구현 대기 | [권한과 장애 전환](../architecture/control-plane-authority-and-failover.md) | 이중 lease 거부, lease 상실 fail-closed, 이전 epoch 거부, 수동 승격 E2E |
| #64 | Agent 실행 격리 | P1 · 설계 확정·구현 대기 | [실행 격리](../architecture/agents/execution-isolation.md) | 다른 Agent의 process·workspace·credential 접근 차단과 wipe 실패 안전성 테스트 |
| #65 | 재현 가능한 Skill loading | P2 · 설계 확정·구현 대기 | [Harness composition](../architecture/agents/harness-composition.md) | revision 불일치 fail-closed, manifest 기록, 재실행 입력 동일성 테스트 |
| #66 | Fleet Security Manager·credential delivery | P1 · 설계 확정·구현 대기 | [보안 모델](../security/control-plane-security-model.md), [Credential 지침](../credentials/README.md) | encrypted backend abstraction, Attempt/Worker-bound one-time grant, revoke/rotation, raw export 제거·break-glass 감사 |
| #67 | Worker execution lease·Agent command ACK | P1 · 설계 확정·구현 대기 | [권한과 장애 전환](../architecture/control-plane-authority-and-failover.md), [Agent provisioning](../architecture/agents/provisioning.md) | CAS slot claim, fencing token, Worker incarnation, ACK 유실 `OutcomeUnknown`, self-fencing E2E |
| #68 | Project archive·retention·hold | P2 · 설계 확정·구현 대기 | [Lifecycle](../architecture/project-task-agent-lifecycle.md) | idempotent drain, effect/cleanup hold, retention/reopen, archive-blocked E2E |
| #69 | Project Git workspace·checkpoint recovery | P1 · 설계 확정·구현 대기 | [Entity placement](../architecture/entity-placement-and-context.md), [실행 격리](../architecture/agents/execution-isolation.md) | Agent worktree isolation, checkpoint push/restore, secret scan, Worker 이동 복구 E2E |
| #70 | Observability·reconciliation·recovery | P1 · 설계 확정·구현 대기 | [관측성·재조정·장애 복구](../architecture/observability-and-reconciliation.md) | inventory-first recovery, orphan/grant/effect quarantine, secret-free metric/audit, alert/runbook E2E |
| #71 | Task dispatch credential precondition | P1 · 설계 확정·구현 대기 | 이 행 (설계 요약: `Task.resolved_model`이 `Some`인 경우, worker 후보 선택 단계에서 `Store::get_worker_credential(worker.name, model)`로 해당 worker가 그 model의 활성 credential을 가졌는지 확인한다. 없는 worker는 후보에서 제외 — label 필터와 동일한 자리에 합류시킨다. 후보가 전부 제외되면 기존 `DispatchError::NoWorker` 경로(재시도 → Reconciler 소진 시 dead-letter)를 그대로 재사용하되, dead-letter 시 `FailureKind`에 신설한 `CredentialMissing`을 붙여 원인을 구분한다. `model`이 없는 task는 이 검사를 건너뛴다 — 어떤 credential이 필요한지 알 수 없기 때문이다.) | credential 없는 worker로 model 지정 task가 라우팅되지 않음, 재시도 소진 뒤 `FailureKind::CredentialMissing`으로 dead-letter, 정상/부분 provisioned fleet에서의 정상 dispatch 회귀 테스트 |

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
