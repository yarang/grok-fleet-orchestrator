---
type: runbook
authority: canonical
implementation: implemented
verification: integration-tested
source: "docs/engineering-patterns/jev-decision-records.md"
last_verified: "2026-09-23"
last_verified_commit: "working-tree"
owners: ["engineering-patterns"]
---

# jev — 설계 결정 기록 MCP 서버

이 문서는 개발 에이전트가 설계 결정을 기록·평가하는 데 쓰는 `jev` MCP 서버의 설치와
사용 경계를 다룬다. 등록 절차의 실행체는 [`scripts/setup-jev-mcp.sh`](../../scripts/setup-jev-mcp.sh)이고,
이 문서는 **왜 그 모양인가**와 **무엇이 머신 밖으로 나가는가**를 적는다.

출처 저장소: `git.agentthread.dev/yarang/jev-mcp` (비공개, `GITEA_TOKEN` 필요)

## 설치

```bash
scripts/setup-jev-mcp.sh          # 기본 경로로 등록
JEV_MCP_DIR=/path/to/jev-mcp scripts/setup-jev-mcp.sh
```

재실행해도 안전하다 — 기존 등록을 지우고 다시 넣는다. 확인은 `claude mcp list`.

## 왜 `.mcp.json`을 커밋하지 않는가

`claude mcp add --scope project`는 저장소 루트의 `.mcp.json`에 쓰는데, 이 저장소는
그 파일을 **의도적으로** `.gitignore`에 두고 있다(23행). 사유가 파일 주석에 적혀 있다 —
같은 파일이 이 머신에서 프로덕션 orchestrator 호스트로 가는 SSH 접근 경로를 함께
담기 때문이다. jev 항목 하나를 올리자고 그 무시를 풀면, 다음 `--scope project` 등록이
호스트 경로를 추적 대상 설정으로 되돌린다.

`.claude/settings.json`에 서버 정의를 넣는 우회도 없다. Claude Code 2.1의 settings
스키마에는 `mcpServers` 키가 없고, 있는 것은 `.mcp.json`을 **참조**하는
`enabledMcpjsonServers`/`disabledMcpjsonServers`/`enableAllProjectMcpServers`뿐이다.

그래서 저장소가 공유하는 것은 설정 파일이 아니라 **절차**다. 각 머신은 한 번 스크립트를
돌리고 자기 클라이언트 설정을 로컬 scope로 갖는다. 머신별 경로도 시크릿도 트리에
들어오지 않는다.

## 머신 밖으로 나가는 것 — 쓰기 전에 읽을 것

`TYPESAFE_API_KEY`가 환경에 있으면 서버는 결정의 **제목·맥락·선택지·근거·결과**를
`api.typesafe.ai`로 보내 fit/reversibility 판정을 받는다. 키가 없으면 기록만 로컬
SQLite(`JEV_DB_PATH`)에 남고 평가는 건너뛴다.

따라서 **시크릿·고객 데이터·내부 호스트명이 들어간 결정은 제출하지 않는다.** 설계
선택지와 그 근거처럼 저장소에 커밋될 내용과 같은 수준의 것만 보낸다. 키 자체는 어떤
설정 파일에도 쓰지 않고 환경에서만 읽는다 — 스크립트도 그것을 기록하지 않는다.

## 도구 표면

| 도구 | 성격 |
|---|---|
| `jev_decision_shapes` | 읽기 — 제출 가능한 결정 shape 목록 |
| `jev_submit_decision` | 기록 + (키가 있으면) 외부 평가 |
| `jev_update_decision` | 기록 갱신 + 재평가 |
| `jev_evaluate` | 기존 기록의 평가만 재요청 |
| `jev_get_decision` / `jev_list_decisions` | 읽기 |
| `jev_process_queue` / `jev_queue_status` | 평가 실패분의 재처리 큐 |
| `jev_export_adr` | 기록을 ADR 마크다운으로 내보내기 |

## 알려진 함정 — README와 코드의 스키마가 다르다

상류 README는 평가 요청 필드를 `primitive`/`instruction`/`options`로 적지만, 실제
클라이언트 코드가 요구하는 것은 `type`/`instructions`/`criteria`다. 클라이언트 클래스
이름도 README의 `SystemOneClient`가 아니라 `JevClient`다. **코드 쪽이 맞다** — 2026-09-23
실측. README를 보고 직접 호출을 짜면 400으로 튕긴다.

## 관련

- [에이전트 협업 가이드](../../agent.md) — 커밋·게이트 정책
- [Reviews](../reviews/README.md) — 결정의 근거 문서를 남기는 자리. jev 기록은 근거
  **요약**이고, 비교 과정과 기각된 대안의 정본은 여전히 `docs/reviews/`다.
