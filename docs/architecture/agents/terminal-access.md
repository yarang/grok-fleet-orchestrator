---
type: architecture
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/agents/terminal-access.md"
last_verified: "2026-08-17"
---

# Agent 터미널 접근

## 책임

이 문서는 실행 출력 capture와 대화형 attach의 접근 계약을 분리한다. 실행 lifecycle은
[프로비저닝](provisioning.md), 격리 선택은 [실행 격리](execution-isolation.md)가 담당한다.

## capture

stdout/stderr에는 secret과 개인정보가 들어갈 수 있다. 따라서 capture는 Project 범위 권한,
redaction, 크기 제한, TTL, rate limit, 중복 요청 억제, 감사가 모두 구현되기 전에는 표준 조회
기능으로 활성화하지 않는다. capture는 실행 제어 권한을 부여하지 않으며, 로그 참조와 민감도
등급만 반환한다.

## interactive attach

attach는 host-shell급 영향이 될 수 있는 별도 capability다. `AgentAttach`는 Project 범위의 강한
권한, 승인된 SSH host-key 정책, 단일 writer lease, 짧은 만료, revoke·cancel·drain·role 회수 시
강제 종료, 접속 감사와 WebSocket proxy 시험을 만족할 때만 제공한다. container 실행에는 host tmux
attach를 허용하지 않는다.

## 구현 게이트

민감 출력 redaction, 동시 writer 차단, grant 만료/회수 종료, host key 실패 거절, proxy 단절 뒤
세션 정리, 모든 attach 감사 시험을 통과해야 한다.
