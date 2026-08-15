---
type: wiki
status: canonical
source: "docs/architecture/multi-agent-realignment-report.md"
last_verified: "2026-08-15"
---

# 다중 에이전트 설계 재정렬 및 교차 도메인 조정 보고서 (Multi-Agent Design Realignment Report)

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
*   **`infra_architect`**: "동의합니다. 하지만 구현 상의 난관이 있습니다. [`deployment.md`](../deployment/deployment.md) 규격 상 워커 서비스는 보안 위협 최소화를 위해 unprivileged user인 `User=fleet`으로 실행하도록 되어 있습니다. 반면, 하위 에이전트 프로세스나 스케줄러가 생성한 로컬 디렉토리는 `/root/.grok/` 등 쓰기 권한 오버라이드로 인해 일반 `fleet` 권한으로는 삭제에 실패할 수 있습니다."
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

---

## 3. 사용자 선택 및 아키텍처 의사결정 체크리스트 (Decision Checklist)

설계를 전면 개편하기 전에 최종 아키텍처의 트레이드오프 방향성에 대해 개발자(사용자)의 선택이 필요한 핵심 질문들입니다.

### [ ] 의사결정 1: 워커 실행 권한 및 작업공간 소거(Wipe) 방식
*   *대안 A ( unprivileged + Sudoers 화이트리스트)*: 워커 프로세스는 `fleet` 계정으로 구동하여 인젝션 공격 시 루트 권한 유출을 차단하되, 드레인 시 소거 도구만 `sudo` 권한으로 실행해 잔여 디렉토리를 안전하게 청소합니다. (보안성 높음, OS 설정 다소 복잡)
*   *대안 B (Root 프로세스 구동)*: 워커 프로세스 자체를 `root`로 구동하여 삭제 및 프로세스 관리를 단순화합니다. (설정 단순, 프롬프트 인젝션 취약점 노출 시 보안 위험 극대화)
*   *추천*: **대안 A**

### [ ] 의사결정 2: 에이전트 실행 환경의 격리 수준 (Isolation Level)
*   *대안 A (호스트 프로세스 격리)*: 호스트 OS 위에 하나의 워커 프로세스가 여러 에이전트 가상 세션을 `tmux`와 환경변수 분리만으로 통제합니다. (자원 오버헤드 거의 없음, 파일 및 로컬 네트워크 격리 약함)
*   *대안 B (도커/컨테이너 기반 격리)*: 각 에이전트 세션을 독립된 Docker/Podman 컨테이너로 프로비저닝하여 디렉토리와 격리된 샌드박스를 제공합니다. (보안성 및 자원 격리 완벽, 호스트에 도커 데몬 필수 및 웜업 지연 시간 발생)
*   *추천*: 시스템 사양에 따라 선택 (안전한 다중 테넌트 보장에는 **대안 B** 권장)

### [ ] 의사결정 3: 프롬프트 캐싱 및 스킬 로딩 방식
*   *대안 A (도구 호출 기반 동적 페치)*: 스킬들의 이름과 요약만 템플릿에 명세하고, 에이전트가 동작 중 구체적인 스킬 내용이 필요할 때 `fetch_skill_content` MCP 도구를 호출해 동적으로 본문을 로드합니다. (토큰 절약 극대화, 추론 중간에 Tool-Call 1턴 추가 발생)
*   *대안 B (인라인 프레임 구성)*: 캐시가 동작하기 쉬운 프롬프트 상단부에 정적으로 모든 필수 스킬 문서를 주입합니다. (API 비용 다소 증가, Tool-Call 턴이 없어 최종 지연 단축)
*   *추천*: 프롬프트 캐싱 비용 절감을 위해 **대안 A** 권장

### [ ] 의사결정 4: 다중 관리자 동기화 신뢰성 메커니즘
*   *대안 A (DB 시퀀스 저널링 & 커서)*: 추가 백엔드 인프라 없이 PostgreSQL `events` 테이블과 monotonic sequence를 활용해 유실을 방지합니다. (추가 컴포넌트 없음, 폴링 딜레이 존재)
*   *대안 B (외부 메시지 브로커 / Redis 도입)*: 분산 동기화와 메시지 펍섭을 위해 Redis/KeyDB를 스택에 추가합니다. (지연 시간 0에 수렴, 인프라 배포 복잡도 증가)
*   *추천*: 단일 서버 단독 배정 패턴 호환성을 유지하기 위해 **대안 A** 권장

---

## 4. 향후 도메인별 문서 패치 계획 (Execution Plan)

의사결정이 합의되는 대로 아래의 순서로 `docs` 내의 설계 문서들을 업데이트합니다.

1.  **Phase 1. 코어 리모델링**:
    *   [`system-entities-mapping.md`](./system-entities-mapping.md)를 2D 위상-실행 매트릭스로 재편.
    *   [`project-feature-design.md`](./project-feature-design.md)에 Draining 시퀀스 다이어그램 추가.
2.  **Phase 2. 인프라 배포 사양 정비**:
    *   [`nginx-gateway.md`](../deployment/nginx-gateway.md)에 `/ws` 리버스 프록시 규격 주입.
    *   [`deployment.md`](../deployment/deployment.md)에 격리 디렉토리 구조 및 `wipe` 전용 sudoers 사양 기록.
3.  **Phase 3. 부트스트랩 및 가입 흐름 교정**:
    *   [`join-authentication.md`](../worker-bootstrap/join-authentication.md)의 일회성 토큰 persistence 오기 교정.
4.  **Phase 4. 자가 치유 및 게이트웨이 설정 동기화**:
    *   [`hardware-healing.md`](../server-management/hardware-healing.md)에 `pg_notify` 유실 대비 Cursor 스펙 명시.
