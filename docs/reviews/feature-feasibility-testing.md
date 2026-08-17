---
type: review
authority: derived
implementation: not-applicable
verification: design-reviewed
source: "docs/reviews/feature-feasibility-testing.md"
last_verified: "2026-08-15"
---

# 핵심 기능 검증 방안 보존 (Derived)

> **지위: Derived verification plan.** 현재 설계 규칙은
> [아키텍처 정본 지도](../architecture/canonical-map.md)가 가리키는 정본을 따른다.

> 작성일: 2026-08-15  
> **보고 주체**: 기술 타당성 평가 에이전트 (`tech_evaluator`)  
> **분석 목적**: Gitea 기반 Git 이관 및 다중 에이전트 협업 체계의 구현 기술 도출 및 로컬 테스트 시나리오 정의

---

## 1. 핵심 시스템 기능 명세 (Architectural Functions)

본 프로젝트의 하이브리드(macOS/Linux) 분산 환경을 가동하기 위해 구현되어야 할 4대 핵심 기능입니다.

### 1.1 워커 드레인 및 선점 (Worker Draining & Preemption)
*   **기능**: 노드 과부하 또는 자가 치유가 감지되면 해당 워커의 DB 상태를 `Draining`으로 플립하고 스케줄러(`WorkerSelector`) 배정 대상에서 즉시 제외합니다. 실행 중인 작업은 체크포인트 도달 시점에 안전하게 중단됩니다.
*   **상태 전이**: `Online` ➔ `Draining` ➔ `Quarantined/Offline` (완료 후)

### 1.2 Git 기반 작업 공간 이관 (Git-Based Workspace Migration)
*   **기능**: 드레인된 소스 워커의 작업 공간 파일 변경분을 로컬 Git에 커밋한 뒤, 해당 태스크용 Gitea 임시 브랜치(`tmp/task-{id}`)로 강제 푸시(Force Push)합니다. 배정받은 타깃 워커는 해당 브랜치를 Shallow pull 하여 중단 지점부터 연산을 즉시 재개합니다.

### 1.3 동적 스킬 페칭 (Dynamic Skill Fetching)
*   **기능**: 에이전트가 동작 중 특정 스킬 지침이 필요할 때, 전체 지침을 프롬프트에 미리 바인딩하지 않고, Gitea API를 통해 스킬 리포지토리의 해당 마크다운 파일 내용만 온디맨드로 로컬 캐시에 페치해 와 도구 호출(Tool Call) 컨텍스트로 노출합니다.

### 1.4 다중 에이전트 DAG 체이닝 (Multi-Agent DAG Chaining)
*   **기능**: 여러 에이전트가 협업하는 파이프라인(예: 코드 검토 ➔ 보안 감사 ➔ 빌드)을 구성하기 위해, 선행 에이전트의 Git 커밋 해시(Commit Hash)와 변경 분(Diff)을 후속 에이전트의 태스크 입력 컨텍스트(DAG)로 전달해 협업 흐름을 완성합니다.

---

## 2. 소요 기술 및 인터페이스 (Required Technologies)

| 기술 요소 | 용도 | 구현 상세 |
|---|---|---|
| **POSIX Git CLI** | 워크스페이스 변경분 버저닝 및 델타 전송 | `tokio::process::Command` 비동기 래퍼로 호출하며, 병목 방지를 위해 `--depth 1` shallow clone 강제 적용. |
| **Gitea REST API** | 임시 브랜치 생성/삭제 및 배포 키 주입 | Gitea HTTP API 클라이언트를 Rust 내부에 연동 (브랜치 삭제: `DELETE /repos/{owner}/{repo}/branches/{branch}`). |
| **SSH Key 격리** | 비대화형 보안 Git 인증 | 워커 메모리 내에 일회성 Ed25519 deploy key를 로드하여 파일 유출 없이 Gitea와 SSH 인증 처리. |
| **시스템 자원 감지** | 드레인 전환 조건 모니터링 | `sysinfo` 및 `NVML` 라이브러리를 통해 호스트 로드 및 메모리 고갈(OOM 위험) 임계치 실시간 계산. |

---

## 3. 로컬 검증 및 테스트 시나리오 (Local Testing Feasibility)

실제 분산 인프라나 Gitea 서버를 배포하지 않고도, 로컬 개발/CI 환경에서 해당 기능들의 타당성을 100% 검증하기 위해 수립된 **테스트 전략**입니다.

```
                  [ 로컬 통합 테스트 실행기 ]
                              │
            ┌─────────────────┴─────────────────┐
            ▼                                   ▼
   [ 1. Git Push/Pull 검증 ]          [ 2. Gitea API 모킹 ]
   - Local Bare Repository            - wiremock (HTTP Mock)
   - git init --bare <tmp_dir>        - Gitea API 응답 가짜 반환
   - 로컬 디렉토리 간 push/pull         - 토큰 검증/가입 데드락 재현
            │                                   │
            └─────────────────┬─────────────────┘
                              ▼
                  [ 3. 부하 시뮬레이션 ]
                  - sysinfo 모크 주입 / stress 툴 기동
                  - Load Threshold ➔ Draining 전이 검증
```

### 3.1 Git 이관 기능 로컬 테스트: Local Bare Repositories
*   **테스트 기법**: 실제 Gitea 서버를 띄우지 않고, 테스트 프레임워크가 임시 디렉토리에 로컬 베어 저장소(`git init --bare <tmp_dir>`)를 생성하여 원격 Git 서버처럼 동작하도록 모킹합니다.
*   **검증 항목**: 호스트 A 디렉토리에서 커밋 후 이 베어 저장소로 push하고, 호스트 B 디렉토리에서 pull 했을 때 uncommitted 파일들이 충돌 없이 정상 병합되는지 확인합니다.

### 3.2 Gitea API 모킹: `wiremock` 연동
*   **테스트 기법**: Rust의 `wiremock` 크레이트를 활용하여 Gitea의 REST API 엔드포인트들을 모킹 서버로 기동합니다.
*   **검증 항목**: 임시 브랜치 생성 및 삭제 호출 시의 HTTP 상태 코드 파싱, 가입인증 토큰(`Join Token`)의 트랜잭션 충돌(409) 시 토큰 보존 로직이 정상 작동하는지 검증합니다.

### 3.3 리소스 고갈 및 드레인 시뮬레이션: Metrics Injection
*   **테스트 기법**:
    1.  `WorkerSelector` 테스트 코드 내에 인위적으로 임계치 임계 범위를 초과하는 가짜 메트릭(`WorkerMetrics`)을 주입하여 스케줄러가 해당 노드를 스케줄링 대상에서 즉시 제외하는지 유닛 테스트합니다.
    2.  로컬 통합 테스트에서 가벼운 스트레스 스크립트(CPU/Memory 강제 점유)를 구동하여, 워커의 감시 백그라운드 스레드가 이를 감지하고 에이전트를 `Draining` 상태로 트리거하는지 실측 검증합니다.

### 3.4 다중 에이전트 체이닝 E2E 테스트: Mock Worker Actors
*   **테스트 기법**: [`crates/fleet-scheduler/tests/dispatch_e2e.rs`](../../crates/fleet-scheduler/tests/dispatch_e2e.rs) 내에 가짜 워커 행위자(Mock Worker)들을 배치하여, 1단계 에이전트가 완료한 작업 커밋 해시가 2단계 에이전트의 입력 프롬프트와 Git Checkout 브랜치 인풋으로 정상 이관되는지 DAG 흐름을 검증합니다.
