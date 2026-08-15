---
type: wiki
status: canonical
source: "docs/architecture/system-entities-mapping.md"
last_verified: "2026-08-15"
---

# 시스템 엔티티 관계 및 매핑 규칙 (System Entities Mapping & Rules)

> 작성일: 2026-08-15
>
> 이 문서는 Grok Fleet Orchestrator 시스템 내의 주요 추상화 요소인 **프로젝트(Project)**, **호스트(Host)**, **에이전트(Agent)**, **워커(Worker)**, **태스크(Task)**, **스킬(Skill)**, **도구(Tool/MCP)** 간의 물리적·개념적 관계와 매핑 불변식을 정의합니다.

---

## 1. 3축 아키텍처 모델 (The Tri-Axis Architecture)

시스템의 모든 구성 요소는 서로 독립된 세 개의 핵심 축(Axis)을 기준으로 분류되고 상호 작용합니다.

```
                  [ axis 2: WHAT - 행동 구성 ]
                  Persona (custom_prompt)
                           │
                           ▼
                         Skill (절차 지침)
                           │
                           ▼
                        Tool/MCP (실행 도구)
                           │
  [ axis 1: WHERE - 물리 배치 ] ─────────────────── [ axis 3: WHEN - 스코프 체인 ]
  Project ➔ Host ➔ Agent ➔ Worker               Project ➔ Template ➔ Agent ➔ Task
```

### 1.1 축 1 — WHERE (물리적 배치 및 격리)
*   **구조**: `Project` $\rightarrow$ `Host` $\rightarrow$ `Agent` $\rightarrow$ `Worker`
*   인프라 및 자원의 물리적 배치를 결정합니다. `Host`는 자원 용량(`max_agents`), 네트워크 위치, 프로세스 관리만을 담당하는 순수 인프라 계층으로 작동하며, 행동 세부 사양(축 2)에는 관여하지 않습니다.

### 1.2 축 2 — WHAT (행동 구성 및 명세)
*   **구조**: `custom_prompt` (페르소나) $\rightarrow$ `Skill` (절차 지침) $\rightarrow$ `Tool/MCP` (실행 표면)
*   에이전트가 "누구이고", "어떻게 행동하며", "무엇을 실행할 수 있는지"를 결정합니다. 상위 페르소나에서 하위 도구로 갈수록 명세가 구체화됩니다.

### 1.3 축 3 — WHEN (스코프 및 결정 범위)
*   **구조**: `Project` (Baseline) $\rightarrow$ `AgentTemplate` (Preset) $\rightarrow$ `Agent` (Instance) $\rightarrow$ `Task` (Single-run)
*   설정값과 보안 규칙이 언제, 어느 범위까지 상속되고 무시(Override)될 수 있는지를 결정합니다.

---

## 2. 핵심 엔티티 배타적 격리 규칙 (Exclusivity Invariants)

리소스 충돌과 경쟁 상태를 원천 차단하기 위해, 물리 인프라와 논리 프로젝트 스코프는 **1:N 배타적 소유 관계**를 강제합니다.

### 2.1 스키마 단에서의 하드 격리
*   조인 테이블(M:N)을 배제하고 `hosts.project_id`, `workers.project_id` 직접 외래키(FK)를 배치하여 프로젝트 간 컴퓨팅 자원을 물리적으로 격리합니다.
*   `project_id`가 `NULL`인 자원은 일반/공유 풀(General Pool)에 속함을 의미합니다.

### 2.2 호스트-워커 격리 불변식 (Invariant Rules)
1.  **호스트 배정**: 호스트를 특정 프로젝트에 할당(`assign_host_to_project`)하면, 해당 호스트에 묶인 `hosts.worker_id`와 해당 호스트 위의 모든 `agents.host_id` 레코드의 `project_id`가 단일 트랜잭션 내에서 일괄 갱신됩니다.
2.  **워커 배정 가드**: 특정 워커를 프로젝트에 할당할 때, 해당 워커가 호스트에 묶여 있다면 호스트가 소속된 프로젝트와 일치해야만 합니다. 불일치 시 `409 Conflict`를 반환합니다. 오직 독립형(Standalone) 워커만 개별 프로젝트 할당이 허용됩니다.
3.  **하트비트 재동기화**: 워커가 네트워크 장애 등으로 재연결(`upsert_worker`)될 때, 워커의 `project_id`는 바인딩된 호스트의 `project_id`로 자동 강제 동기화됩니다.

### 2.3 실행 중 태스크 보존 정책 (In-Flight Task Policy)
*   호스트나 워커가 다른 프로젝트로 재배정되더라도, 이미 디스패치되어 실행 중인(`Dispatched`) 태스크는 중단 없이 정상 완료될 때까지 보존됩니다. 재배정은 해당 시점 이후에 접수되는 신규 태스크 스케줄링부터 즉시 적용됩니다.

---

## 3. 프롬프트 합성 파이프라인 (Prompt Assembly Flow)

태스크가 스케줄러에 의해 디스패치(`Dispatcher`)되어 실행 장치로 전송될 때, 프롬프트 문자열은 권위와 스코프의 높낮이에 따라 아래의 strict한 순서로 자동 결합됩니다.

```
┌────────────────────────────────────────────────────────┐
│ 1. Project.constitution_prompt (프로젝트 헌법)            │
├────────────────────────────────────────────────────────┤
│ 2. Agent.custom_prompt (에이전트 페르소나)                │
├────────────────────────────────────────────────────────┤
│ 3. Active Skills (필수 스킬 + 태스크 요청 선택 스킬)     │
├────────────────────────────────────────────────────────┤
│ 4. Agent Memory (구조화된 에이전트 메모리 컨텍스트)        │
├────────────────────────────────────────────────────────┤
│ 5. Thread Context (동일 스레드 내 이전 Q&A 이력)         │
├────────────────────────────────────────────────────────┤
│ 6. Task Prompt (사용자가 제출한 원시 프롬프트)             │
└────────────────────────────────────────────────────────┘
```

1.  **프로젝트 헌법 (`constitution_prompt`)**: 프로젝트 내 모든 에이전트가 지켜야 할 철칙(CLAUDE.md 등)으로, 항상 맨 앞에 주입됩니다.
2.  **에이전트 페르소나 (`custom_prompt`)**: 에이전트 고유의 역할과 정체성을 확립합니다.
3.  **활성 스킬 (`Active Skills`)**: 템플릿에 지정된 필수(Required) 스킬 전체와 태스크가 실행 시점에 요청한 옵션(Optional) 스킬들이 마크다운 텍스트 지침서 형태로 주입됩니다:
    $$\text{Active Skills} = \text{Skills}_{\text{required}} \cup \{ s \in \text{Skills}_{\text{optional}} \mid s.\text{name} \in \text{task.requested\_optional\_skills} \}$$
4.  **에이전트 메모리 (`Agent Memory`)**: 해당 `agent_id` 하위에 누적된 최신 $N$개의 컨텍스트 메모리 스냅샷이 결합됩니다.
5.  **스레드 컨텍스트 (`Thread Context`)**: 동일 `thread_id` 내 상위 부모 태스크들로부터 수집된 질문-응답(Q&A) 요약본이 계층 구조로 포맷팅되어 결합됩니다.
6.  **태스크 프롬프트 (`Task Prompt`)**: 사용자가 실행 요청 시 작성한 최종 프롬프트(`task.prompt`)가 꼬리에 붙어 실행기로 전달됩니다.

---

## 4. 제약 조건 가드 및 스케줄링 필터 (Constraint Guards)

### 4.1 컴퓨팅 용량 제약 (Capacity Constraints)
*   **호스트 레벨**: 하드웨어 가용 지표(`load_avg`, `mem_available_mb`)로 판단하며, 동적 에이전트가 실행될 때마다 `hosts.max_agents` 상한선을 트랜잭션 락(`FOR UPDATE`)을 통해 보장합니다.
*   **워커 레벨**: 동시에 실행 가능한 최대 세션 한도(`Worker.max_concurrent`, 기본값 4)를 기준으로 제어하며, `active_tasks < max_concurrent` 상태를 만족하는 경우에만 디스패치 대상 후보로 분류됩니다.

### 4.2 호스트 ↔ 도구 매칭 규칙 (Host-Tool Label Match)
*   `stdio` 전송 방식의 MCP 도구는 물리적인 호스트 위에 해당 바이너리와 크레덴셜이 실행 가능하게 보존되어 있어야 하므로, 호스트 레이블 매칭 가드를 우회할 수 없습니다:
    $$\text{McpServer.required\_host\_labels} \subseteq \text{Host.labels}$$
*   에이전트 생성(`POST /api/agents`) 및 바인딩 갱신 시, 바인딩 대상 stdio 도구들 중 단 하나라도 호스트의 레이블 가드를 통과하지 못하면 스케줄링 전에 즉시 `409 Conflict` 에러로 실패 처리됩니다. (네트워크 바인드인 HTTP/SSE 도구들은 본 제약을 우회합니다).

### 4.3 스케줄러 (`WorkerSelector`) 디스패치 필터 시퀀스
태스크 디스패치 시, 스케줄러는 후보 워커 목록을 다음 파이프라인 단계에 통과시켜 최종 타깃을 결정합니다.

```
[전체 워커 목록]
      │
      ▼
[1단계: 상태 검사] ────────── Worker.status == Online
      │
      ▼
[2단계: 라벨 필터] ────────── Worker.labels contains Task.required_labels
      │
      ▼
[3단계: 모델 필터] ────────── Worker.labels["model"] == Task.model
      │
      ▼
[4단계: 서킷 브레이커] ────── Worker.circuit_state != Open
      │
      ▼
[5단계: 용량 필터] ────────── Worker.active_tasks < Worker.max_concurrent
      │
      ▼
[6단계: 프로젝트 격리] ────── Worker.project_id == Task.project_id (하드 필터)
      │
      ▼
[7단계: 타깃 매칭] ────────── Task.agent_id 또는 Task.server_hint 고정 타깃 매칭
      │
      ▼
[8단계: 부하 분산] ────────── Least-Loaded (동점 시 이름 순) 정렬 후 1순위 반환
```
