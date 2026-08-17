---
type: architecture-decision-record
authority: historical
implementation: not-applicable
verification: design-reviewed
source: "docs/reviews/multi-agent-realignment-report.md"
last_verified: "2026-08-15"
---

# 다중 에이전트 설계 재정렬 기록 (Historical)

> **지위: Historical decision record.** 확정된 lifecycle·격리·Skill 정책은
> [아키텍처 정본 지도](canonical-map.md)의 해당 정본에 반영됐다. 이 문서는 대안과
> 결정 과정을 보존한다.

> 작성일: 2026-08-15  
> **토론 참가자**: `core_architect` (코어), `infra_architect` (인프라), `operations_architect` (운영/보안)  
> **분석 대상**: `docs/` 내 전체 설계 도메인 문서군

---

## 1. 개요

본 보고서는 `system-entities-critique.md`에서 제기된 구조적 모순, 동시성 병목, 테넌트 격리 결함, 토큰 인플레이션 문제를 해결하기 위해 **3개 영역 전담 설계 에이전트들이 상호 작용하며 진행한 기술 논의 및 합의 사항**을 기록합니다. 또한, 시스템 구축 시 사용자의 개입과 최종 판단이 필요한 아키텍처 트레이드오프(Trade-off) 요소를 체크리스트로 제시합니다.

---

## 2. 에이전트간 상호 토론 및 합의 (Simulated Consensus Dialogue)

### 💬 주제 A: 프로젝트 재배정 시 워크스페이스 소거(Purge) 및 프로세스 권한
*   **`core_architect`**: "보안을 위해 프로젝트가 변경될 때 이전 프로젝트의 작업 디렉토리를 완전히 소거(`rm -rf`)해야 합니다. 또한 A 프로젝트의 잔여 태스크가 실행 중일 때 B 프로젝트 태스크가 겹치지 않도록 워커를 `Draining` 상태로 제어해야 합니다."
*   **`infra_architect`**: "동의합니다. 하지만 구현 상의 난관이 있습니다. 현재 배포 규칙상 워커 서비스는 보안 위협 최소화를 위해 unprivileged user인 `User=fleet`으로 실행하도록 되어 있습니다. 반면, 하위 에이전트 프로세스나 스케줄러가 생성한 로컬 디렉토리는 `/root/.grok/` 등 쓰기 권한 오버라이드로 인해 일반 `fleet` 권한으로는 삭제에 실패할 수 있습니다."
*   **`operations_architect`**: "워커 전체를 root로 실행하는 것은 LLM 프롬프트 인젝션 취약점 노출 시 시스템 장악 위험이 너무 큽니다. 따라서 ** unprivileged `User=fleet` 방식을 유지**하되, `/etc/sudoers.d/fleet-wipe` 설정을 통해 **워크스페이스 특정 하위 디렉토리에 대해서만 패스워드 없이 실행 가능한 전용 삭제 스크립트(`fleet-worker-wipe`)를 sudo 화이트리스트로 등재**해 해결할 것을 제안합니다."
*   **합의 사항**: `User=fleet` 보안 설정을 유지하고, 전용 삭제 도구 실행 시에만 `/etc/sudoers.d/` 제어권을 활용하는 원칙을 수립함.

### 💬 주제 B: API 게이트웨이 타임아웃 vs 프롬프트 캐시 미스 지연
*   **`core_architect`**: "프롬프트 최적화를 위해 스킬 전문을 미리 프롬프트에 구겨 넣지 않고 모델이 필요할 때만 도구(Tool Call)로 호출하는 '동적 스킬 로딩'을 도입할 것입니다. 이는 KV-Cache 효율을 극대화합니다."
*   **`operations_architect`**: "좋은 방향이지만, 최초의 도구 스키마 파싱 및 캐시 미스(Cold Start) 상황에서는 LLM의 첫 토큰 생성 지연(Time to First Token)이 평소보다 훨씬 길어집니다. 현재 Nginx와 게이트웨이 타임아웃이 300초와 600초로 문서마다 어긋나 있습니다."
*   **`infra_architect`**: "대기 지연으로 인한 역방향 프록시 단절을 방지하기 위해 **모든 API 게이트웨이 역방향 프록시의 `proxy_read_timeout` 규격을 600초로 상향 통일**하고, `proxy_buffering off` 설정을 강제해 최초 청크가 끊기지 않도록 배포 사양을 갱신하겠습니다."
*   **합의 사항**: 타임아웃 규격을 600초로 단일화하고, 스토어와 게이트웨이 전반에 스트리밍 응답 버퍼링 비활성화를 강제함.

### 💬 주제 C: 워커 가입인증 토큰(Join Token) 라이프사이클 데드락
*   **`infra_architect`**: "현재 `auth_middleware`가 모든 `/v1/` API에 대해 어드민 토큰을 요구하므로, 새 워커가 가입하는 `/v1/workers/join` 요청마저 사전에 튕겨나가 부트스트랩이 데드락 상태가 됩니다."
*   **`core_architect`**: "조인 엔드포인트 `/v1/workers/join`은 예외적으로 어드민 API 키 검증 대상에서 Whitelist 처리하되, 요청 바디에 포함된 1회성 `bootstrap_token` 자체를 보안 키로 사용해 검증하도록 로직을 설계하겠습니다. 이때 1회성 토큰의 소모와 이름 충돌 검사는 데이터베이스 트랜잭션 내에서 원자적으로 처리되어 실패 시 토큰이 억울하게 소모(burn)되는 일을 막아야 합니다."
*   **합의 사항**: 조인 경로의 미들웨어 예외 처리 적용 및 토큰 검증-등록 트랜잭션 원자성 보장.

### 💬 주제 D: pg_notify 유실 및 동기화 불일치
*   **`operations_architect`**: "자가 치유로 인해 서킷 브레이커가 작동해 워커가 차단되는 긴급한 상태 변화가 `pg_notify` 유실로 다른 어드민 서빙 인스턴스에 전파되지 않으면, 죽은 노드로의 태스크 디스패치 루프가 반복됩니다."
*   **`core_architect`**: "동의합니다. 데이터베이스 테이블에 순차적 일련번호(`seq`)를 부여한 `events` 테이블을 정본으로 삼고, 각 어드민 서버가 백그라운드 루프에서 자신이 읽은 마지막 시퀀스 번호(`last_seen_seq`) 이후의 이벤트만 조회해 반영하는 커서 조회를 구현해 유실 시에도 최종 일관성을 보장하겠습니다."
*   **합의 사항**: 이벤트 저널링 테이블 도입 및 폴링 커서 보완 메커니즘을 핵심 아키텍처 스펙으로 선언함.

### 💬 주제 E: Gitea 기반 분산 데이터 평면 및 저장소 통합 명세
*   **`core_architect`**: "작업 스냅샷을 위해 압축 Tarball 방식 대신 Git 델타 전송을 사용하겠습니다. Git 커밋 이력 및 Diff 메타데이터는 복구 시 정보의 질이 훨씬 높고 비용 효율적입니다."
*   **`infra_architect`**: "현재 프로젝트 환경에서 감지된 Gitea 서버 정보(`HTTP: https://git.agentthread.dev`, `SSH: git-ssh.agentthread.dev`)를 데이터 평면(Data Plane)으로 영입하여 사용하겠습니다. 워커들은 Gitea로 직접 Pull/Push 하도록 설정해 오케스트레이터의 파일 전송 부하를 0으로 우회시킵니다."
*   **`operations_architect`**: "Gitea 디스크 비대화를 막기 위해 태스크 수명주기에 밀착된 임시 브랜치(`tmp/task-{id}`) 모델을 적용하고, 완료 즉시 Gitea API를 활용하여 브랜치를 물리적으로 삭제하고 백그라운드 Git GC로 소거하겠습니다. 워커 노드는 얕은 복제(`--depth 1`)로 대역폭을 보존합니다."
*   **합의 사항**: Gitea를 데이터 평면 정본으로 확정하고 임시 브랜치 수명주기 통제 메커니즘을 채택함.

---

## 3. 사용자 선택 및 아키텍처 의사결정 체크리스트 (Decision Checklist)

설계를 전면 개편하기 전에 최종 아키텍처의 트레이드오프 방향성에 대해 개발자(사용자)의 선택이 필요한 핵심 질문들입니다.

### [x] 의사결정 1: 워커 실행 권한 및 작업공간 소거(Wipe) 방식

*   **확정 결정 (2026-08-16)**: 워커와 Agent는 비권한 `fleet` 계정으로 실행한다. root 권한은 root 소유·불변 전용 소거 도구 `fleet-worker-wipe`에만, `/etc/sudoers.d/fleet-wipe`의 정확한 명령 allow-list로 위임한다.
*   sudoers는 일반 shell, `rm`, 와일드카드 인자, 환경변수 보존을 허용하지 않는다. wipe 도구는 opaque workspace id만 받으며, 허용된 Fleet workspace root 아래만 descriptor 기반으로 정리해 path traversal·symlink race를 거절한다.
*   모든 wipe 요청은 task/attempt/workspace id, 요청 actor, 전후 결과를 감사 로그에 남기며, 실패는 Draining 완료로 간주하지 않는다.
*   운영 파일 권한·sudoers 예시는 [구성과 비밀 관리](../deployment/configuration.md)를 따른다.

### [x] 의사결정 2: 에이전트 실행 환경의 격리 수준 (Isolation Level)

*   **확정 결정 (2026-08-16)**: 신뢰된 단일 프로젝트 작업은 `host_trusted` 실행을 허용하고, 다중 프로젝트 또는 신뢰할 수 없는 외부 입력을 다루는 작업은 `container_required` 격리를 의무화한다.
*   `host_trusted`는 단일 프로젝트·신뢰된 입력·최소 권한 실행 조건을 모두 만족할 때만 허용한다. 전용 Fleet 사용자와 전용 tmux socket만 사용한다.
*   `container_required`는 rootless runtime, 범위가 제한된 workspace mount, egress allow-list, 자원 제한을 기본값으로 한다.
*   격리 결정과 정책 버전은 Agent와 TaskAttempt에 고정하여 재시도·감사 시 같은 조건을 재현한다. 상세 계약은 [Agent 실행 격리](../architecture/agents/execution-isolation.md)를 따른다.

### [x] 의사결정 3: 프롬프트 캐싱 및 스킬 로딩 방식

*   **확정 결정 (2026-08-16)**: 필수 Skill은 revision을 고정해 inline으로 주입하고, 선택 Skill은 카탈로그만 먼저 제공한 뒤 명시적 요청으로 동적 조회한다.
*   필수 Skill은 실행 시작 시 본문과 revision/hash를 고정한 static prefix로 주입해 재현성과 prefix cache 효율을 확보한다.
*   선택 Skill은 이름·설명·revision·권한만 카탈로그에 노출한다. 본문은 `fetch_skill_content` 같은 읽기 전용 도구로 필요할 때만 조회하며, 결과도 attempt에 id·revision·content hash로 기록한다.
*   실행 중 최신 revision으로 자동 전환하지 않으며, 선택 Skill 조회 실패는 경고로 남긴다. 필수 역량이 필요한 작업을 조용히 계속 실행하지 않는다.

### [x] 의사결정 4: Orchestrator 운영 모델과 다중 관리자 동기화
*   *대안 A (DB 시퀀스 저널링 & 커서)*: 추가 백엔드 인프라 없이 PostgreSQL `events` 테이블과 monotonic sequence를 활용해 유실을 방지합니다. (추가 컴포넌트 없음, 폴링 딜레이 존재)
*   *대안 B (외부 메시지 브로커 / Redis 도입)*: 분산 동기화와 메시지 펍섭을 위해 Redis/KeyDB를 스택에 추가합니다. (지연 시간 0에 수렴, 인프라 배포 복잡도 증가)
*   **확정 결정 (2026-08-16)**: Fleet는 하나의 논리적 제어 기관만 허용한다.
    Primary Orchestrator 하나만 실행하고, 두 번째 인스턴스는 평상시 중지된
    **Cold Standby**로 유지한다. Active-Active dispatch는 지원 운영 모델이 아니다.
*   **안전 조건**: Standby 승격 전 기존 Primary의 종료 또는 네트워크 fencing을
    확인해야 한다. DB lease와 단조 증가 epoch를 사용해 두 인스턴스가 동시에
    dispatch 권한을 갖는 split-brain을 차단한다.

---

## 4. 향후 도메인별 문서 패치 계획 (Execution Plan)

의사결정이 합의되는 대로 아래의 순서로 `docs` 내의 설계 문서들을 업데이트합니다.

1.  **Phase 1. 코어 리모델링**:
    *   [`system-entities-mapping.md`](./system-entities-mapping.md)를 2D 위상-실행 매트릭스로 재편.
    *   [`project-feature-design.md`](./project-feature-design.md)에 Draining 시퀀스 다이어그램 추가.
2.  **Phase 2. 인프라 배포 사양 정비**:
    *   [`reverse-proxy.md`](../deployment/reverse-proxy.md)에 `/ws` 리버스 프록시 규격 주입.
    *   [`configuration.md`](../deployment/configuration.md)에 격리 디렉토리 구조 및 `wipe` 전용 sudoers 사양 기록.
3.  **Phase 3. 부트스트랩 및 가입 흐름 교정**:
    *   [`join-authentication.md`](../worker-bootstrap/join-authentication.md)의 일회성 토큰 persistence 오기 교정.
4.  **Phase 4. 자가 치유 및 게이트웨이 설정 동기화**:
    *   [`hardware-healing.md`](../operations/proposals/hardware-healing.md)에 `pg_notify` 유실 대비 Cursor 스펙 명시.
