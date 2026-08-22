---
type: governance-method
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/governance/design-document-reading-and-judgment.md"
last_verified: "2026-08-18"
owners: ["architecture", "security", "planning"]
---

# 설계 문서를 읽고 판단하는 방법

## 목적

설계 문서는 설명문이 아니라 의사결정의 근거다. 이 방법은 문서를 읽을 때 현재 구현, 목표 계약,
미결정 사항, 위험 및 실행 순서를 섞지 않고 판단하도록 한다. 기능을 구현하거나 문서를 재작성하기
전에 적용하며, 결론은 정본·Roadmap·변경 로그에 남긴다.

```mermaid
flowchart LR
    A["범위와 질문 고정"] --> B["정본과 권위 확인"]
    B --> C["현재 구현 증거 대조"]
    C --> D["상태·권한·실패 경로 모델링"]
    D --> E["모순·위험·미결정 분류"]
    E --> F["결정 또는 사용자 선택"]
    F --> G["정본·Roadmap·log 반영"]
    G --> H["코드·테스트·운영 검증"]
    H --> I["legacy 제거 또는 다음 gate"]
```

## 1. 질문과 범위를 먼저 고정한다

문서 전체를 요약하는 대신 판단할 질문을 한 문장으로 쓴다. 예를 들어 “Worker가 join 뒤 어떤
credential으로 어떤 API를 호출하는가?”처럼 주체·행동·경계를 포함한다. 다음을 함께 정한다.

- 대상 도메인, API, 엔티티, lifecycle 구간
- 현재 동작을 확인할 코드·migration·테스트·운영 설정
- 결정이 필요한 범위와 이번 검토에서 결정하지 않을 범위
- 보안·가용성·비용·호환성 중 우선 기준

질문이 불명확하면 구현이나 재작성으로 넘어가지 않는다.

## 2. 문서의 권위와 시간을 구분한다

각 문서를 읽을 때 front matter와 도메인 README에서 다음을 확인한다.

| 구분 | 판단 방법 | 사용 방식 |
|---|---|---|
| 정본 | `authority: canonical` 및 도메인 진입점 | 현재 승인된 목표를 결정하는 근거 |
| 현재 계약 | `implementation: implemented/partial`와 코드 참조 | 실제 동작을 설명하는 근거 |
| 제안 | `proposed` 또는 Roadmap 설계 항목 | 구현 전 선택지·완료 조건 |
| 사본·역사 | derived, historical, review | 배경·근거만 제공; 값의 우선권 없음 |

문서의 “목표”를 현재 구현으로, 현재 API 동작을 최종 설계로 해석하지 않는다. 충돌하면 정본을
고치거나 Roadmap에 migration을 추가한다. 조용한 문서 동기화로 해결하지 않는다.

## 3. 엔티티·권한·상태 전이를 표로 만든다

설계 판단은 명사와 동사를 분리하면 명확해진다. 최소한 아래 표를 작성한다.

| 항목 | 확인할 질문 |
|---|---|
| 소유자 | 누가 생성·변경·폐기할 수 있는가? |
| durable state | 재시작 뒤 남아야 하는 사실은 무엇인가? |
| ephemeral state | process·lease·session처럼 관측으로 복구할 것은 무엇인가? |
| credential | 원문은 어디서 한 번만 보이며, digest/reference는 어디에 남는가? |
| lifecycle | 정상·실패·취소·재시도·archive의 전이 조건은 무엇인가? |
| authority | 어떤 principal/capability/scope/fencing이 필요한가? |
| effect | 외부 부작용, 멱등성 key, 보상·hold 규칙은 무엇인가? |

모든 write에는 actor, 대상, precondition, 성공 증거, 실패 뒤 복구 책임을 연결한다.

## 4. 정상 흐름보다 실패·경쟁·회복을 먼저 검토한다

다음 질문에 답할 수 없으면 설계는 불완전하다.

- 요청이 timeout 되었지만 서버는 성공했을 때 재시도는 무엇을 하는가?
- control plane, Worker, Agent 중 하나가 재시작하거나 partition 되었을 때 누가 fencing하는가?
- credential·lease·process·effect가 서로 다른 시점에 성공하면 어떤 상태가 진실인가?
- API/DB/외부 서비스 중간 실패가 rollback 가능한가, 아니면 hold/보상이 필요한가?
- 오래된 client·config·token을 어떤 기간, 어떤 오류로 거절하는가?

Mermaid state/sequence diagram은 세 개 이상 상태 또는 비동기 주체가 얽힐 때 기본으로 쓴다.

## 5. 발견 사항을 네 종류로 분류한다

| 분류 | 의미 | 후속 조치 |
|---|---|---|
| 버그·보안 결함 | 현재 구현이 승인된 계약 또는 안전 경계를 위반 | 우선순위·완화·테스트를 Roadmap에 등록 |
| 설계 모순 | 정본끼리 또는 정본과 lifecycle이 양립 불가 | 정본을 하나로 정정하고 이유를 log 기록 |
| 미결정 | 선택이 결과·권한·데이터 보존을 바꿈 | 선택지와 trade-off를 사용자/owner에게 제시 |
| 구현 공백 | 목표는 승인됐지만 코드·migration·runbook이 없음 | 완료 gate를 가진 increment로 분해 |

현재 변경만으로 해결 가능한 것은 실행한다. 권한 모델, 데이터 삭제, 외부 배포, 호환 기간처럼
결정권자의 선택이 필요한 것은 가정으로 확정하지 않는다.

## 6. 결론을 구현 가능한 규칙으로 쓴다

좋은 결론은 “안전하게 한다”가 아니라 다음 형식이다.

```text
When <trigger>, <principal> may perform <operation> on <resource>
only if <preconditions>. Persist <evidence>; on <failure>, transition to <state>.
```

각 규칙에는 API/DB schema, validation, audit, metric/alert, 정상·거절·장애 테스트를 연결한다.
새 경로와 이전 경로가 충돌하면 migration window, fallback 금지 여부, 삭제 gate를 명시한다.

## 7. 기록과 검증

판단 결과는 다음 위치에만 기록한다.

- **정본:** 바뀐 설계 규칙과 diagram
- **Roadmap:** 구현 순서, 영구 ID, 완료 gate
- **`docs/log.md`:** 결정 이유와 변경 사실(append-only)
- **코드·migration·test:** 현재 동작의 증거

검증은 최소 `git diff --check`, 관련 unit/integration test, migration 검토를 포함한다. production
credential·권한·DB migration은 staging rehearsal과 rollback/복구 절차까지 확인해야 완료로 판정한다.

## 빠른 검토 체크리스트

- [ ] 질문·범위·결정권자가 명확한가?
- [ ] 정본, 현재 구현, 제안 문서를 분리했는가?
- [ ] state, authority, credential, effect, failure/recovery를 모두 확인했는가?
- [ ] 데이터·token·로그에 원문 secret이 없는가?
- [ ] retry/timeout/partition/rolling upgrade의 동작이 정의됐는가?
- [ ] 필요한 사용자 결정을 구현 가정으로 숨기지 않았는가?
- [ ] 정본·Roadmap·log·테스트가 함께 갱신됐는가?
