---
type: runbook
authority: canonical
implementation: partial
verification: code-checked
source: "docs/deployment/install.md"
last_verified: "2026-08-17"
last_verified_commit: "working-tree"
owners: ["deployment"]
---

# 설치 Runbook

이 문서는 `fleet`와 `fleet-worker` 바이너리를 설치하고 실행 전 검증하는 절차다. 운영 환경은
버전 고정 릴리스 artifact와 checksum을 사용한다. `curl | bash`는 개발·검증 환경 외의 기본
운영 절차가 아니다.

## 사전 조건

- 운영자가 설치할 release version과 target architecture를 확인한다.
- PostgreSQL 접근 정보와 서비스 계정을 준비한다.
- Worker 설치 전에는 [Worker enrollment 계약](../contracts/worker-enrollment.md)의 차단 조건을
  확인한다. 현재 self-service join은 일반 프로덕션 절차가 아니다.

## 설치

1. release artifact와 checksums 파일을 내려받아 SHA-256을 검증한다.
2. `fleet`와 `fleet-worker`를 시스템 설치 경로 또는 사용자 설치 경로에 배치한다.
3. `fleet --version`과 `fleet-worker --version`으로 설치한 버전을 기록한다.
4. systemd를 사용할 때는 서비스 계정과 설정 파일을 준비한 뒤 unit을 등록한다.

`install.sh`는 release artifact 설치와 `--build` source-build 경로를 제공한다. 기본 경로는
`/usr/local/bin`이며 `--user`는 사용자 경로를 사용한다. 설치 스크립트의 PATH 변경과 제거 범위는
실행 전에 확인한다.

**승인된 예외가 하나 있다.** MCP stdio 표면을 고치는 개발 반복에 한해
[MCP client 연결](mcp-clients.md)의 "호스트 바이너리 재빌드·교체 절차"가 로컬 크로스 컴파일
산출물을 직접 올리며, 이 경로는 위 checksum 검증을 우회한다. 운영 배포에는 쓰지 않는다.

## 검증과 중단 기준

- artifact checksum 또는 version이 기대값과 다르면 중단한다.
- 설정 파일·서비스 계정·DB 접근이 준비되지 않았으면 서비스를 시작하지 않는다.
- Worker `worker.toml`에 원문 token이 남는 현재 제약을 승인하지 않았다면 Worker 설치를 중단한다.

## 관련 정본

- [Configuration](configuration.md)
- [Operations](operations.md)
- [Worker enrollment](../contracts/worker-enrollment.md)
