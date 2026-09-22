---
type: architecture
authority: canonical
implementation: partial
verification: design-reviewed
source: "docs/architecture/agents/harness-composition.md"
last_verified: "2026-09-22"
last_verified_commit: "working-tree"
---

# Agent 하네스 구성

## 책임

하네스는 실행 직전에 prompt, Project 규칙, Skill, tool binding을 하나의 immutable snapshot으로
조립한다. 이것은 권한 강제 장치가 아니며, 권한은 [보안 모델](../../security/control-plane-security-model.md),
실행 경계는 [실행 격리](execution-isolation.md)가 강제한다.

## 조립 규칙

필수 Skill은 시작 전에 revision/hash까지 고정하고 누락 시 실행을 거절한다. 선택 Skill은 명시된
조회 시점과 실제 revision/hash를 Task의 execution snapshot에 기록한다. Project constitution과 사용자 입력은 출처를
구분해 포함하며, 사용자 입력이 시스템 규칙·권한·tool allow-list를 바꿀 수 없다.

Tool/Skill의 catalog → Project grant → Agent binding → Task request → execution snapshot 우선순위는
[배치·맥락 계약](../entity-placement-and-context.md)이 정본이다. Tool binding은 [도구 카탈로그](tool-catalog.md)의 허용된 식별자와 capability만 참조한다. prompt,
Skill, tool, runtime revision 중 하나가 바뀌면 실행 중인 Task를 변형하지 않고 새 Task를 만든다.

## 구현 게이트

필수 Skill 누락 거절, revision 재현, prompt-injection이 권한을 올리지 못함, 재시도 snapshot
동일성 시험을 통과해야 한다.

### 게이트별 실측 상태 (2026-09-21)

| 게이트 | 상태 | 근거 / 막고 있는 것 |
| --- | --- | --- |
| 1. 필수 Skill 누락 거절 | **닫힘** | `Dispatcher::dispatch_existing`의 4.7단계가 CAS **앞에서** 하네스를 조립하고, `SkillInjection::missing`이 비어 있지 않으면 `Failed(SkillMissing)`로 거절한다. 시험 3건(거절·대조군·브레이커 면제)이며 구별력을 실측했다 |
| 2. revision 재현 | **닫힘** | `044`가 `tasks.skill_snapshot`(JSONB)을 만들고, dispatch가 조립한 본문의 sha256을 `Store::record_task_skill_snapshot`으로 **한 번만** 쓴다(`record_task_acp_session`과 같은 write-once + fence 구조). `NULL`("조립 기록 없음")과 `[]`("조립했고 Skill 없음")을 가르며 시험이 두 백엔드에서 그것을 단정한다. `fleet task show`가 이름·해시·바이트를 낸다 |
| 3. prompt-injection이 권한을 올리지 못함 | 차단 | 스킬 본문은 `<SKILL: name>`…`</SKILL>`로 감싸기만 하고 **본문 안의 같은 태그를 이스케이프하지 않는다**. 다만 이 파일들은 운영자가 오케스트레이터에 배포하는 것이라 오늘의 위협 모델에서 신뢰 경계 **안**이다. 사용자 입력이 스킬이 되는 경로가 생기면 그때 강제가 필요하다 |
| 4. 재시도 snapshot 동일성 | 사실상 해당 없음 | 무재시도 정책(`#97`) 아래에서 Task당 실행이 최대 하나라 "재시도 간 동일성"을 가를 두 번째 실행이 존재하지 않는다. 게이트 2가 닫히면 이 칸은 그 문장으로 대체돼야 한다 |

**2026-09-22 — 게이트 2가 닫혔다. 이름은 신원이 아니다.**
`skills_required`는 **이름 목록**이고, 이름은 그때 무엇이 실행됐는지를 말해 주지 않는다 —
같은 `security-audit`라도 어제의 본문과 오늘의 본문이 다르면 다른 실행이며, 파일은
오케스트레이터의 디스크에 있어 언제든 바뀐다. `044`의 `tasks.skill_snapshot`이 조립 시점에
실제로 주입된 본문의 sha256을 고정한다.

**해시가 재는 것은 frontmatter를 뺀 뒤의 본문이다.** 그것이 실제로 프롬프트에 들어간
바이트이기 때문이다 — 파일 전체를 재면 메타데이터만 바뀌어도 다른 실행으로 보이고, "같은
입력이었는가"라는 질문에 거짓으로 답한다. 본문 자체를 담지 않은 이유는 둘이다: 주입된 본문은
`tasks.prompt`에 이미 있고, Skill이 커지면 Task 행이 그만큼 커진다. 해시는 "같은가"를
답하기에 충분하고 "무엇이었는가"는 그 해시를 가진 파일을 찾는 문제다.

**쓰기 지점은 CAS 뒤다.** 이 Task를 `Dispatched`로 차지하지 못했다면 이 인스턴스의 조립은
아무것도 아니고, 그 기록을 남기면 실행하지 않은 조립이 "그때 실행된 것"으로 읽힌다. 반대로
게이트 1의 **거절**은 CAS 앞이어야 한다 — 두 지점이 다른 이유가 서로 반대라는 점이 이
설계의 요지다.

**`NULL`과 `[]`를 가른다.** 앞은 "조립 기록이 없다"(044 이전 행, dispatch된 적 없는 행),
뒤는 "조립했고 Skill이 없었다"이다. 접으면 게이트 2가 답하려는 질문이 "dispatch 됐는가"와
섞인다. 두 백엔드 시험이 JSONB 왕복에서 그 구분이 살아남는지를 단정한다.

**2026-09-21 — 게이트 1은 코드가 정본을 정면으로 어기고 있었다.** 위 「조립 규칙」이
"필수 Skill은 … 누락 시 실행을 **거절**한다"고 적는 동안, `skill_loader.rs`는 파일이 없으면
`warn!` 한 줄을 찍고 **건너뛰었다**(독스트링에 "soft-fail"이라고 적혀 있었다). 그래서
`skills_required`에 보안 감사 스킬을 선언한 Task가 그 스킬 없이 실행됐고, 아무 신호도 나지
않았다. 반환형이 `-> String`이라 호출부에는 거절할 방법이 **원리적으로 없었다** — 누락을
실을 자리가 없었기 때문이다. 이제 `SkillInjection { prompt, missing }`이 누락을 값으로
돌려주고, 거절은 상태 전이를 동반하므로 `Dispatcher`가 한다.

**조립 지점이 둘이었다.** `fleet task submit --skill X`는 제출 시점에 주입한 프롬프트를
저장하면서 `skills_required`도 함께 저장했고, dispatch가 그것을 보고 **다시** 주입했다 —
같은 본문이 두 번 실리고 `<TASK>`가 중첩됐다. 조립 지점은 하나여야 하고 그 자리는 이 문서가
적은 대로 **실행 직전**이다. 제출 시점에 구우면 그 뒤 스킬 파일이 바뀌어도 낡은 사본이
실행되고, 저장된 `prompt`가 사용자가 쓴 것과 달라져 게이트 4의 입력 동일성도 깨진다.
`fleet-worker`에도 거의 같은 로더가 하나 더 있었으나 **호출부가 한 곳도 없었고**(grep 0건)
soft-fail 동작만 달랐다. "required"의 정의가 두 개 있으면 고친 쪽이 무의미해지므로 지웠다.
