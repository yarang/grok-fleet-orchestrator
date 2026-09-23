---
type: review
authority: derived
implementation: not-applicable
verification: design-reviewed
source: "docs/reviews/effect-ledger-research-2026-09-23.md"
last_verified: "2026-09-23"
last_verified_commit: "working-tree"
owners: ["architecture", "tasks"]
---

# effect ledger 설계의 문헌 대조 (2026-09-23)

대상 정본: [실행 일관성](../architecture/tasks/execution-consistency.md) ·
[관측성과 재조정](../architecture/observability-and-reconciliation.md) 게이트 4 ·
로드맵 `#70`

## 왜 조사했는가

게이트 4(`Started` effect·archive hold 자동 redrive 금지)는 2026-09-14에 **의도적 보류**로
판정됐다. 사유는 "이 fleet은 아직 비가역 외부 부작용을 내는 워크로드를 돌리지 않으므로
원장이 증명할 대상 자체가 없다"였다. 이 판정은 우리 코드만 보고 내린 것이어서, 같은 모양의
문제를 다룬 문헌이 이 판단을 지지하는지 반박하는지 확인하지 않은 상태였다.

조사 대상 질문 다섯:

1. outbox/saga 보상이 **보상 불가능한 effect**(메일 발송, 과금, `git push`)에 적용될 때 무엇을 포기하는가
2. LLM 에이전트의 도구 호출에 idempotency key를 부여하는 설계가 실제로 쓰이는가 — 키를 호출부가 아니라 **제어면**이 파생해야 할 근거가 문헌에 있는가
3. exactly-once가 원리적으로 불가능한 자리에서 실무가 어디까지 가는가
4. `PartiallyApplied`/`OutcomeUnknown` 같은 비terminal 상태를 실제로 운영하는 사례가 있는가 — 무한정 쌓이지 않게 하는 장치는
5. 원장에 무엇을 기록하고 무엇을 기록하지 않는가 — 페이로드인가 해시인가

## 방법과 그 한계

5개 각도로 확장해 22개 소스를 수집, 109개 주장을 추출한 뒤 25개를 적대적 검증에 올려
**12 확정 / 13 기각**, 병합 후 10개 finding으로 정리했다.

**기각이 확정만큼 많다는 것이 이번 조사의 성격을 정한다.** 기각은 "반대가 참"이 아니라
**"근거 미확보"**를 뜻한다. 따라서 아래 「기각된 것」에 있는 영역의 설계 결정은 문헌
인용이 아니라 자체 논증으로 정당화해야 한다.

1차 문헌: Garcia-Molina & Salem, *Sagas* (SIGMOD'87) · Helland, *Life beyond Distributed
Transactions* (CIDR 2007) · Azure Architecture Center, Compensating Transaction pattern
(2026-08-15 갱신) · SagaLLM (arXiv:2503.11951) · Atomix (arXiv:2602.14849) · IETF
`draft-ietf-httpapi-idempotency-key-header-07` · Stripe idempotent requests.

## 확정된 것

### 1. `Cancelled`는 "아무 일도 없었음"과 동치가 아니다 — 설계를 바꿔야 한다

보상(compensation)은 되돌리기가 아니라 **새로운 forward 부작용**이고, 그 보상자의 존재
자체가 애플리케이션이 공급해야 하는 전제조건이다.

1987 원문이 직접 적는다 — *"if a transaction fires a missile, it may not be possible to
undo this action"*. 처방도 undo가 아니라 추가 실세계 행위다: *"to compensate for the
letter, send a second letter explaining the problem; to compensate for the check, send a
stop-payment message to the bank"*. 정의부도 *"does not necessarily return the database to
the state that existed when the execution of T_i began"*이며, 보상 뒤에 *"no effort is made
to notify or abort transactions that might have seen the results"*이다. Azure가 현행
문서로 같은 의미론을 재확인한다 — *"A compensating transaction doesn't necessarily return
the system data to its state at the start of the original operation"*, *"It's not easy to
generalize compensation logic. A compensating transaction is application specific"*.

**정정 하나를 함께 적는다**: 1987 원문은 '편지 보내기'를 보상 *가능* 쪽에 둔다. 그러므로
이 finding을 "saga는 이메일에 적용 불가"로 인용하면 과장이다. 옳은 인용은 **"보상은
삭제가 아니라 전진형 추가 행위"**다.

> **정본에 미칠 영향**: 우리 해소 규칙의 `CancelUnconfirmed → Cancelled`는 **"`Started`
> effect 없음이 증명됨"에만** 주어야 하고, 보상으로 도달한 상태는 별도 terminal
> (`Compensated`)로 갈라야 한다. 지금 설계는 둘을 한 이름에 담고 있다.

### 2. `PartiallyApplied`는 결함이 아니라 원리적 경계 조건이다

이기종 외부 엔드포인트에 걸친 원자적 커밋은 tool 쪽 TCC/2PC를 요구하는데, 임의의
REST API·SMTP·파일시스템 syscall은 이를 제공하지 않는다. Atomix §4가 명시한다 —
*"Saga, Temporal, SAFEFLOW, and OCC hit the same boundary. Only tool-side commit protocols
remove it."* 같은 논문의 Appendix C.3이 두 번째 환원 불가 원천을 추가하는데, 그것이 정확히
우리 `OutcomeUnknown`의 모양이다: *"Provider-side commits that happen before the adapter
records the effect are outside the preventive guarantee; the runtime surfaces them as
unresolved residue."*

대응도 명시적이다 — bounded retry, 지속 실패 시 fail-stop, 그리고 *"a partial-commit
record naming what externalized and what failed"*. 잔여를 지우는 것이 아니라 **무엇이
외부화됐는지 이름을 적는다**.

### 3. Helland가 우리 접근을 licence한다 — 단, 절반만

분산 트랜잭션이 없으면 불확실성을 인프라(락/트랜잭션 매니저)가 들고 있을 수 없고,
**애플리케이션 수준의 파트너별 durable 상태로 reify**해야 한다. CIDR 2007 §Uncertainty at
a Distance: *"The management of uncertainty must be implemented in the business logic. The
uncertainty of the outcome is held in the business semantics rather than in the record
lock. This is simply workflow."*

즉 `OutcomeUnknown`/`PartiallyApplied`/`CancelUnconfirmed`는 결함이 아니라 **의도된
설계**다.

**전이 한계가 명시된다.** Helland의 uncertainty는 자기 쪽 activity 상태를 함께 들고 있는
*협조적* 파트너를 전제하고, at-least-once 재시도 + 수신자 멱등을 깐다. 우리 오케스트레이터는
**관측 전용·무재시도**이고 상대(메일 provider, git remote)는 activity를 들지 않는다.
논문 스스로 *"This paper is not asserting that activities can solve the well known
challenges to reaching agreement"*라고 면책한다. 따라서 이 문헌은 **"불확실성을 durable
상태로 만들라"까지만 주고 비협조 provider에 대한 해소 프로토콜은 주지 않는다.**

### 4. exactly-once는 달성되는 것이 아니라 포기된다

Helland 각주 8: *"I am a big fan of 'exactly-once in-order' messaging but to provide it for
durable data requires a long-lived programmatic abstraction similar to a TCP connection.
The assertion here is that these facilities are rarely available..."* 실무는 at-least-once
+ **수신 측** 멱등으로 가고, 멱등을 제공하지 않는 provider에서 남는 것은 중복 위험이거나
미해결 상태다.

우리 무재시도 정책은 여기서 축 하나를 지운다 — **중복 축이 사라지는 대신 "한 번의 실행이
어디까지 갔는가"라는 불확실성 축만 남는다.** 이것이 우리 문제가 문헌의 표준 문제와
다른 모양인 이유다.

### 5. 자동 redrive 금지는 표준 처분이다

1987년의 *"the system is stuck... manual intervention"*과 2026년 Azure의 *"manual
intervention is the only way to recover... raise an alert"*가 같은 처분을 적는다. 보상이
결정론적으로 실패하면 시스템은 abort도 complete도 못 하는 명시적 stuck 상태에 놓이고,
문서화된 처분은 (a) alternate/recovery-block 코드 (b) **사람의 수리**다.

우리 게이트 문언의 "자동 중복 실행 없이"는 이 계보 안에 있다.

### 6. 원장은 "있으면 좋은 것"이 아니라 전제조건이다

Azure가 인프라의 must로 규정하는 셋: 보상에 필요한 정보를 **절대 유실하지 않을 것**,
보상 진행을 신뢰성 있게 감시할 것, 원 operation과 그 보상을 **양쪽 다** end-to-end로
상관·감사할 수 있을 것. 그리고 보상 단계 자체를 멱등 커맨드로 설계할 것 — 즉 멱등성은
forward path가 아니라 **복구 path**에 대해 명시적으로 처방된다.

### 7. 문헌의 예방 레버는 우리에게 전이되지 않는다

문헌의 예방 장치는 거의 전부 **제어면이 effect를 발행/게이팅한다**는 전제 위에 있다.
사후 알림으로만 관측하는 제어면에는 그 레버가 없다. 그러므로 **우리 원장은 예방 장치가
아니라 탐지·증적·에스컬레이션 장치로만 성립한다.** 게이트 4에 착수할 때 이 구분을
잃으면, 원장을 만들어 놓고 막지 못하는 것을 막았다고 착각하게 된다.

### 8. 핵심 판정 — 보류는 문헌과 모순되지 않는다, **단 조건이 붙는다**

*"증명할 비가역 부작용이 아직 없으면 원장을 만들 이유도 없다"*는 판단은 유지해도 된다.
Azure가 원장을 하드 전제조건으로 규정하지만, 그 전제는 **보상·증명 대상 effect가 존재할
때** 발동하기 때문이다.

> **붙는 조건**: 기록이 외부화보다 **늦으면** 그 건은 영구 미증명 잔여가 된다. 따라서
> 원장은 **첫 비가역 effect를 도입하는 바로 그 변경과 동시에** 존재해야 한다. "나중에
> 소급해서" 만들 수 있는 것이 아니다.

사전 도입과 소급 도입 중 어느 쪽이 실무에서 덜 비쌌는가에 대한 **비용 비교 증거는 어떤
소스에도 없었다.**

## 기각된 것 — 연구 질문 2가 통째로 무너졌다

**"멱등 키를 호출부가 아니라 제어면이 파생해야 한다"는 가설의 근거가 하나도 살아남지
못했다.** 네 갈래 전부 0-3 기각:

| 세우려던 근거 | 소스가 실제로 말하는 것 |
|---|---|
| Atomix | 키 생성을 adapter(통합 계층)에 위임 — *adapter-defined, optional* |
| IETF 초안 | **클라이언트** 생성 + 서버측 복합 키 |
| Stripe | **클라이언트** 생성, 서버는 키 생성 규칙을 강제하지 않음 |
| Helland | **수신 엔티티**가 소유하는 dedup |

기각된 13건에는 이 밖에도 다음이 포함된다 — Atomix의 abort 결과 taxonomy를 독립 주장으로
세우려던 시도, SagaLLM이 in-doubt 상태를 정의한다는 주장(오히려 full commit / full
rollback 둘만 인정한다), Stripe가 멱등 레코드를 **실행 개시 후에만** 저장한다는 근거,
IETF가 payload digest 저장을 요구한다는 근거.

**따라서 우리 정본이 `Project ID + Task ID + effect scope + policy revision`의 HMAC를
제어면에서 파생하기로 한 것은, 문헌이 지지하는 설계가 아니라 우리 제약(무재시도·관측
전용)에서 자체 도출한 설계다.** 그 정당화는 자체 논증으로 서야 하며, 인용으로 대신할 수
없다.

## 읽을 때 조심할 것

1. **Atomix의 80%/40%/0/500 수치는 동료심사 전 preprint의 자체 벤치마크**다. 저자들이
   직접 재구현한 mechanism-matched 베이스라인이지 실제 Temporal 배포 측정치가 아니고,
   독립 재현이 없다. `0/500`도 유일하지 않으며(TCC-Confirm, Mutex+WAL+Rollback 동률),
   그 0은 effect 분류가 옳을 때만 성립한다 — 오분류 ablation은 60% 누출한다.
2. **질적 결론은 강하고 숫자는 약하다.** 보상의 비가역성 한계·이기종 2PC 불가·at-least-once
   수용은 1987/2007 1차 문헌과 현행 벤더 지침이 교차 확인하는 합의지만, 누출률·비용
   숫자는 전부 단일 출처 자체 실험이다.
3. **Helland는 불가능성 증명이 아니라 실용적 불가용성 논증**이다. 2007년의 *"rarely
   available"* 전제는 Kafka EOS·Flink 2PC sink·durable execution으로 트랜잭션 경계 *안에서는*
   상당히 약해졌다. **외부 비트랜잭션 경계에만 인용해야 한다.**
4. **Azure 페이지는 처방적 지침이지 현장 보고가 아니고**, 그 예시는 오케스트레이터가 각
   단계를 *발행*한다는 전제 위에 있다. 기록 대상도 '보상 가능한 단계의 되돌리는 법'이지
   '비가역 effect의 증적'이 아니어서, MS가 effect ledger를 승인했다고 읽으면 과대 인용이다.
5. **SagaLLM은 배포 사례도 오버헤드 실측도 없는 설계 제안**이며 모든 연산이 보상 가능하다고
   가정한다(irreversible/idempotent/side effect 언급 0회). 스키마 참조로만 쓸 수 있다.
6. **우리 조합(사후 관측 전용 + 무재시도 + 비협조 provider)을 직접 다룬 1차 문헌이 수집분에
   없다.** finding 7은 3-0 통과 주장들의 검증 노트에서 합성한 것이다.

## 미답 — 문헌이 답을 주지 않은 것

1. **멱등 키 파생 주체**의 1차 근거. SagaLLM 저자군의 후속작 ALAS(arXiv:2511.03094)가
   멱등 키·타임아웃·백오프·보상 정책을 추가했다고 언급되는데, 키를 호출부에 두는지
   제어면에 두는지 1차 검증되지 않았다 — 이번 조사에서 남은 유일한 직접 단서다.
2. **비terminal 상태의 누적 억제 장치**. 확보된 증거는 '사람에게 올린다'까지이고, 1987
   Sagas는 오히려 수리 완료까지 pending이 무한정 유지된다고 적는다. 체류 시간 타임아웃,
   alert 임계, 에스컬레이션 SLA, 잔여 큐 상한 — **구체적 수치 장치는 전부 미답**이다.
   우리 `OutcomeUnknown`/`CancelUnconfirmed`가 "출구 없는 상태" 함정을 피하려면 이
   수치를 우리가 정해야 한다.
3. **원장의 저장 비용과 보존 정책 실측**. 프로덕션 durable execution 엔진(Temporal,
   Restate, DBOS)이 페이로드를 담는지 해시를 담는지, 보존·압축·아카이빙이 어떻게 설계됐는지.
   유일한 스키마 문헌 SagaLLM은 전체 페이로드 + LLM 추론 체인을 불변 로그에 담으면서
   비용·보존·카디널리티를 **한 줄도** 다루지 않는다. (연구 질문 5는 사실상 미답이다.)
4. **고카디널리티 상관 키**(`task_id`, `effect_id`, 멱등 키)를 metric/로그 레이블로 흘리지
   않으면서 원장과 관측 데이터를 상관시키는 확립된 패턴. 수집분의 어떤 소스도 언급하지
   않았다.
5. **관측 전용 제어면에서 원장이 예방 없이 탐지·증적만으로 사고를 줄인 사례 증거**.
   문헌의 모든 예방 레버가 발행권을 전제하므로, 우리 체제의 ROI는 현재 추론일 뿐이다.

## 정본에 반영한 것

- [관측성과 재조정](../architecture/observability-and-reconciliation.md) 게이트 4의 보류
  사유에 **"첫 비가역 effect를 도입하는 변경과 동시에"** 조건을 붙였다(finding 8).
- 같은 절에 `Cancelled`/`Compensated` 분리를 **미결 항목**으로 추가했다(finding 1).

`#65` 게이트 2 착수 중 `jev`로 제출한 결정 셋(skill snapshot 해시 범위, 저장 형태)은 이
조사와 별개이며, 그 기록은 로컬 decision store에 있다 —
[jev 설치와 사용 경계](../engineering-patterns/jev-decision-records.md) 참고.
