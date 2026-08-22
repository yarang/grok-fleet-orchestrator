---
type: architecture
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/agents/runtime-adapters.md"
last_verified: "2026-08-17"
---

# Agent 런타임 어댑터

## 책임

이 문서는 허용된 Agent runtime을 실행 계층에 연결하는 adapter 계약만 다룬다. runtime 선택은
Project 정책, 실행 격리는 [실행 격리](execution-isolation.md), prompt·tool 조립은
[하네스 구성](harness-composition.md)이 담당한다.

## adapter 계약

각 adapter는 이름·버전·binary/image digest·지원 transport·isolation capability·필요한 비밀
참조 종류를 선언한다. 입력은 immutable execution snapshot이고, 출력은 구조화된 상태·로그 참조·
종료 분류다. `NetworkBind`와 `StdioBridge`는 transport 유형일 뿐 권한 모델이 아니다.

실행 command, argument, mount, environment key는 allow-list된 구조화 필드로만 전달한다. container
profile의 rootless 여부, image digest, mount manifest, egress profile, privileged tool allow-list revision은
[실행 격리](execution-isolation.md)의 immutable Attempt snapshot을 따른다.
비밀 원문과 임의 shell 조각을 catalog 또는 Agent 레코드에 저장하지 않는다. 새 runtime은 digest와
capability 검증, 격리 시험, 취소/cleanup 시험을 통과하기 전 배정할 수 없다.

## 현재 상태

현재 Worker는 하나의 Grok runner 중심으로 동작한다. 다중 runtime catalog와 adapter 협상은
구현되지 않았으므로 이 문서의 계약을 현재 기능으로 표현하지 않는다.
