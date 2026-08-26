---
type: review
authority: canonical
implementation: not-applicable
verification: code-checked
source: "docs/reviews/task-retry-policy-decision-2026-08-26.md"
last_verified: "2026-08-26"
last_verified_commit: "working-tree"
owners: ["architecture", "tasks"]
---

# 실행 재시도 정책 결정과 `TaskAttempt` 범위 재조정 (2026-08-26)

대상 정본: [실행 일관성](../architecture/tasks/execution-consistency.md) · 로드맵 `#62` 4단계

## 질문

로드맵 `#62` 4단계는 `TaskAttempt` 엔티티 도입이었다. 착수 도중 다음 정책이 결정됐다.

> 재시도를 하지 않는다. Task가 실패하면 새 Task를 만든다.

이 결정이 `TaskAttempt`의 범위에 무엇을 하는가가 이 문서의 질문이다.

## 사실 확인 — 두 종류의 "재시도"

코드에는 이름이 같고 성격이 다른 두 가지가 있었고, 둘을 섞으면 판단이 어긋난다.

| | dispatch 재시도 | 실행 재시도 |
|---|---|---|
| 존재 여부 | 있음 (로드맵 `#38`) | **없음** |
| 무엇이 실패했나 | **워커 선정** (`NoWorker`/`CircuitOpen`) | 워커에 도달한 실행 |
| Task 위상 | `Pending`에 머무름 | — |
| 카운터 | `tasks.retry_count`, `max_dispatch_retries` 도달 시 `Failed` dead-letter | — |
| 시도 행 | **아직 만들어지지 않음** | — |

dispatch 재시도는 시도가 만들어지기 **전에** 끝난다. 그리고 orphan/offline sweep은
`Dispatched` Task를 곧바로 터미널 `Failed`로 보낸다 — 되살리지 않는다.

## 결정적 실측 — Task당 시도는 최대 하나다

`crates/` 전체에서 `TaskStatus::Pending`을 **새 상태로 쓰는 지점이 없다.** 등장하는 곳은
전부 `expected` 슬라이스, 테스트의 `matches!` 단언, 또는 표시·메트릭 매핑
(`fleet-api/src/metrics.rs`, `fleet-cli/src/runtime.rs`)이다. Task는 생성 시 `Pending`으로
태어나 떠날 뿐 돌아오지 않는다.

따라서 `Pending -> Dispatched` 전이는 Task당 **최대 한 번** 성립한다. 무재시도 정책 아래에서
이것은 우연이 아니라 **설계상 영구히** 그렇다.

## 그 결과 무너진 것

`task_attempts` 테이블은 `tasks`와 1:0..1이 된다. 계획했던 컬럼을 하나씩 세면:

| 컬럼 | 새로운 사실인가 |
|---|---|
| `worker_id` | 아니다 — `status->'Dispatched'->>'worker_id'`의 복사본 |
| `created_at` | 아니다 — `tasks.dispatched_at`과 같은 값 |
| `generation` | 아니다 — 영원히 1 |
| `id` (`AttemptId`) | 아니다 — 아래 이유로 소비자가 없다 |
| `control_epoch` | **그렇다 — 유일하게 새롭다** |

`AttemptId`를 CAS 술어로 쓰려던 계획도 함께 무너진다. 무재시도에서 "재시도"는 **다른 TaskId**
이므로, 늦은 이벤트의 충돌 자체가 표현 불가능하다. 터미널 `Failed` Task에 늦은 `Completed`가
오는 경우는 위상 술어 하나로 거절된다 — `AttemptId` 술어는 막을 경쟁이 없다.

`AttemptTransition{Started, Rejected, Fenced}` 역시 기존 `TransitionOutcome`의 중복이었다.

## 대안 비교

| 안 | 내용 | 판정 |
|---|---|---|
| A. 테이블 유지, `generation`만 삭제 | 진단용 1:0..1 테이블 존치 | **기각.** 컬럼 5개 중 4개가 복사본이다. 그리고 다음 사람이 술어를 보면 그것이 막는 경쟁이 존재한다고 추론한다 |
| B. 술어를 남기고 "무력함"을 주석으로 기록 | 코드 유지 + 문서화 | **기각.** 같은 이유. 주석은 술어보다 약한 신호다 |
| C. 전면 되돌리고 아무것도 남기지 않음 | 4단계를 통째로 유예 | 기각. epoch는 사후 복원이 **불가능**하다 (아래) |
| **D. 컬럼 하나로 축소** | `tasks.dispatch_control_epoch BIGINT NULL` | **채택** |

D를 고른 근거는 비대칭이다. 위 표의 복사본 네 개는 지금도 `tasks`에서 읽을 수 있지만,
epoch는 그렇지 않다. [`021_control_plane_lease.sql`](../../crates/fleet-store/migrations/021_control_plane_lease.sql)의
`control_plane_lease`는 *현재* epoch만 들고 있어서, 리스가 한 번 넘어가면 "이 Task를 어느 제어
세대가 디스패치했는가"는 어디에도 남지 않는다.

749 insertions가 142 insertions + 마이그레이션 한 줄이 됐다.

## 남은 정직한 한계

- **읽는 코드가 아직 없다.** 이 컬럼을 근거로 판단하는 경로는 없으며, 후보 소비자는
  reconciler의 orphan sweep이다. 기록하는 이유는 위의 "사후 복원 불가" 하나뿐이다.
- **단일 인스턴스 배포에서는 항상 NULL이다.** `SchedulerState::control_fence()`가
  HA 리스가 없으면 `None`을 돌려주기 때문이다. "값을 못 구했다"가 아니라 "제어 세대라는
  개념이 없는 배포"라는 뜻이다.
- **026 이전 행은 소급 불가.** 지나간 epoch는 복원되지 않는다.
- **`#38`의 `retry_count`/`max_dispatch_retries`는 건드리지 않았다.** 그것은 실행 재시도가
  아니라 dispatch 재시도이며, 없애면 `Pending`에 갇힌 Task의 dead-letter 경로가 사라진다.
- **이 결정의 가장 큰 대가: 외부 멱등성 키가 redrive를 넘어가지 못한다.** 정본의 파생식은
  **Project ID + Task ID + effect scope + 제출 시 정책 revision**의 HMAC이다. Task ID가 안정된
  앵커였던 이유는 재시도가 한 Task **안에** 머물렀기 때문인데, 실패를 새 Task로 옮기면 ID가
  바뀌어 키가 달라지고 외부 provider는 새 작업으로 본다 — 그 파생식이 막으려던 이중 적용이
  그대로 성립한다. 새 Task 경계를 넘는 앵커(승계된 effect scope 또는 redrive 계보)가 필요하지만
  **그런 필드는 없다.** 없는 것을 미리 만들지 않는다는 원칙에 따라 앵커 부재를 사실로 기록하고
  정본의 유예 표에 **설계 미결**로 올렸다. 다만 이것은 미구현 게이트가 아니라 **정책이 만든
  새 구멍**이므로, 무재시도 정책을 채택하면서 같이 받아들인 대가로 읽어야 한다.
- **`#95`의 대기 사유가 무효가 된다.** [`authorization-and-audit.md`](../security/authorization-and-audit.md)
  202-204는 `AuditEvent`에 `attempt_id`가 없는 이유를 "Project/Attempt 엔티티가 아직 없어
  (`#48`·`#62` 계열 선행)"로 적는다. 4단계가 "생산자 없음"으로 닫히면 그 선행 조건은 `#62`로는
  **영원히 충족되지 않는다.** `#95`의 대기 사유를 다시 적어야 하며, 그 재작성은 security 도메인
  정본의 몫이라 이 결정에 포함하지 않았다.
- **`TaskAttempt`의 문서 발자국은 tasks 도메인보다 훨씬 넓다 — 처음엔 그 크기를 잘못 적었다.**
  최초 작성 시 정본의 드리프트 표는 security 문서 2개만 열거했다. 이후 전수 측정(`grep -rn
  --include='*.md' 'TaskAttempt\|attempt_id\|RetryWaiting\|AttemptId\|Attempt' docs`)에서
  **30개 파일 161행**이 나왔고(원장인 `docs/roadmap/roadmap.md`는 사유를 적어 제외 — 포함하면
  `#97` 행을 추가하는 이번 수정 자체가 숫자를 늘려 곧바로 썩는다), architecture 도메인만 20개
  파일 120행이었다. 2개짜리 표는 없는 것보다 나빴다 — **전수 목록처럼 읽히면서 28개를 감췄다.**
  표를 도메인별 실측 집계로 교체하고,
  교차 도메인 판정이 필요한 부분을 [`#97`](../roadmap/roadmap.md)로 분리했다. 이 결정 자체는
  바뀌지 않는다(코드의 1:0..1은 그대로 성립한다). 바뀐 것은 **그 결정이 문서에 남긴 빚의 크기**다.

## 정본 반영

- [실행 일관성](../architecture/tasks/execution-consistency.md) — 상태 모델의 `RetryWaiting`
  제거, 재시도 계약을 실패 처리 계약으로 교체, CAS 술어 목록 정정, 유예 표의 control epoch 및
  `CancelUnconfirmed` 행 갱신, 멱등성 키 파생 문단에 redrive 미해결 명시, 다른 도메인에 남은
  `Attempt` 표현의 위치와 미수정 사유 표
- [로드맵 `#97`](../roadmap/roadmap.md) — `TaskAttempt` 목표 엔티티의 존치 여부 재판정을
  교차 도메인 항목으로 신설(개명이 아니라 판정 작업임을 명시, reviews는 범위 제외)
- [로드맵 `#62`](../roadmap/roadmap.md) — 4단계를 "`TaskAttempt` 구현"이 아니라 "정책상 생산자
  부재 확정 + epoch 기록"으로 재범위화
