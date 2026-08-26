---
type: architecture
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/agents/context-and-memory.md"
last_verified: "2026-08-27"
last_verified_commit: "working-tree"
---

# Agent 컨텍스트와 메모리

## 책임

이 문서는 한 실행의 thread context와 여러 실행에 걸친 장기 메모리를 구분한다. artifact 보존과
Project lifecycle은 [교차 lifecycle](../project-task-agent-lifecycle.md)이 담당한다.

## 규칙

thread context는 Task 실행에 속하며 입력 출처·요약 version·thread 식별자와 함께 고정한다.
현재 thread history가 존재해도 그것은 Agent 메모리 기능이 아니다. 장기 메모리는 Project 범위와
소유 principal, retention, 민감도, 삭제 요청, 접근 감사가 명시된 별도 레코드여야 한다.

다른 Project 또는 principal의 메모리는 기본 거부다. 요약은 원문을 대체하거나 권한을 확대하지
않으며, 장기 메모리 조회·주입은 snapshot과 감사 기록 없이 수행하지 않는다.

## 구현 게이트

Project 경계 누출 거절, retention 만료 삭제, 요약 재현, 민감도별 redaction·감사 시험이 필요하다.
