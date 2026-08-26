---
type: architecture
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/agents/tool-catalog.md"
last_verified: "2026-08-27"
last_verified_commit: "working-tree"
---

# Agent 도구 카탈로그

## 책임

이 문서는 Agent가 참조할 수 있는 tool의 정의·승인·capability 경계를 정한다. 호출 API 자체의
권한은 [보안 모델](../../security/control-plane-security-model.md), 실행 조립은
[하네스 구성](harness-composition.md)이 담당한다.

## 카탈로그 규칙

각 항목은 불변 `tool_id`, 버전/digest, 허용 principal·Project 범위, 필요한 capability, 입력·출력
schema, side-effect 분류, 감사 이벤트를 가진다. tool binding 변경은 `AgentManage` 권한과 Project
범위 검사를 요구한다. 고영향 또는 비가역 tool은 별도 승인과 execution snapshot 기록을 요구한다.

전역 catalog, Project grant, Agent binding, Task 요청과 execution snapshot의 관계 및 deny 우선순위는
[배치·맥락 계약](../entity-placement-and-context.md)을 따른다.

비밀 원문, shell command 문자열, 임의 environment 값은 카탈로그에 저장하지 않는다. 비밀은 실행
시점의 좁은 참조로만 주입하고 로그·capture·오류에는 redaction 한다. MCP tool attach는 transport와
권한 격리가 검증되기 전에는 활성화하지 않는다.
