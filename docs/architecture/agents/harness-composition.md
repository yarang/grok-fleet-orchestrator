---
type: architecture
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/agents/harness-composition.md"
last_verified: "2026-08-17"
---

# Agent 하네스 구성

## 책임

하네스는 실행 직전에 prompt, Project 규칙, Skill, tool binding을 하나의 immutable snapshot으로
조립한다. 이것은 권한 강제 장치가 아니며, 권한은 [보안 모델](../../security/control-plane-security-model.md),
실행 경계는 [실행 격리](execution-isolation.md)가 강제한다.

## 조립 규칙

필수 Skill은 시작 전에 revision/hash까지 고정하고 누락 시 실행을 거절한다. 선택 Skill은 명시된
조회 시점과 실제 revision/hash를 attempt에 기록한다. Project constitution과 사용자 입력은 출처를
구분해 포함하며, 사용자 입력이 시스템 규칙·권한·tool allow-list를 바꿀 수 없다.

Tool binding은 [도구 카탈로그](tool-catalog.md)의 허용된 식별자와 capability만 참조한다. prompt,
Skill, tool, runtime revision 중 하나가 바뀌면 기존 attempt를 변형하지 않고 새 attempt를 만든다.

## 구현 게이트

필수 Skill 누락 거절, revision 재현, prompt-injection이 권한을 올리지 못함, 재시도 snapshot
동일성 시험을 통과해야 한다.
