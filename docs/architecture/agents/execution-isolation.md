---
type: architecture
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/agents/execution-isolation.md"
last_verified: "2026-08-17"
---

# Agent 실행 격리

## 책임

이 문서는 위험도에 따른 실행 격리 선택, immutable snapshot, cleanup 범위를 정한다. Agent
생성 절차는 [프로비저닝](provisioning.md), terminal 접근은 [터미널 접근](terminal-access.md)이
담당한다.

## 결정

| isolation | 선택 조건 | 필수 경계 |
|---|---|---|
| `host_trusted` | 신뢰된 단일 Project의 저위험 작업 | 전용 `fleet` 사용자, Project별 workspace·tmux socket, 최소 권한 |
| `container_required` | 다중 Project, 신뢰되지 않은 입력, 높은 영향도 중 하나 | rootless·비특권 container, read-only base, 제한 mount/egress, 자원 제한 |

스케줄러와 Worker는 더 약한 격리로 fallback하지 않는다. `container_required` 실행에 host tmux
attach를 제공하지 않는다. 더 강한 격리로 다시 시도하려면 새 TaskAttempt를 만들고 이유를 남긴다.

## 실행 snapshot과 cleanup

각 TaskAttempt는 `execution_isolation`, `isolation_policy_version`, 결정 이유와 요청 principal,
Worker capability, workspace 식별자, runtime/image digest, Skill revision/hash를 고정한다.
Worker는 요구 capability가 없으면 거절한다.

cleanup은 Fleet가 만든 container·socket·session·workspace 식별자에만 적용한다. host 전체
`tmux kill-server`와 공유 workspace 삭제는 금지한다. 취소·TTL 만료·권한 회수 때 실행과 attach
grant를 함께 닫고, Worker는 cleanup 증거를 ACK하기 전 `Stopped`로 전이하지 않는다.

## 구현 게이트

1. dispatcher capability 검증과 snapshot 고정 통합 시험
2. container의 host socket·권한 상승·비허용 egress 거절 시험
3. cancel·timeout·crash 뒤 다른 attempt 자원을 건드리지 않는 시험
4. 감사 기록만으로 isolation 결정과 실행 위치를 재구성하는 시험
