---
type: runbook
authority: canonical
implementation: partial
verification: code-checked
source: "docs/deployment/mcp-clients.md"
last_verified: "2026-08-21"
last_verified_commit: "working-tree"
owners: ["deployment", "fleet-mcp"]
---

# MCP client 연결 Runbook

이 문서는 외부 MCP client(Claude Code, Antigravity CLI, Gemini CLI, ChatGPT)가
`fleet serve`의 stdio MCP 서버에 연결하는 절차를 다룬다. MCP 도구 표면·입출력 스키마의
정본은 [MCP 도구 계약](../contracts/mcp-tools.md)이다. 이 문서는 "어떻게 붙이는가"만
다룬다.

## 현재 transport 제약

`fleet serve`의 MCP 구현은 **stdio JSON-RPC 전용**이다(MCP protocol `2024-11-05`).
HTTP/SSE 같은 원격 transport는 구현돼 있지 않다. 이는 client별 지원 여부를 그대로 가른다.

| Client | Transport 요구사항 | 이 문서로 연결 가능한가 |
|---|---|---|
| Claude Code | 로컬 stdio subprocess (`.mcp.json`) | 가능 |
| Antigravity CLI (`agy`) | 로컬 stdio subprocess (`agy mcp add`) | 가능 — Gemini CLI의 후속, 아래 참고 |
| Gemini CLI | 로컬 stdio subprocess (`.gemini/settings.json`) | 가능하지만 **단종 진행 중** — 아래 참고 |
| ChatGPT (Developer Mode custom connector) | **원격 HTTPS + OAuth** 만 지원, 로컬 stdio 불가 | **불가** — 아래 참고 |

## 공통 구조 — SSH로 감싼 stdio

orchestrator 호스트는 이미 `fleet.service`(systemd)로 HTTP API·dashboard·MCP를 동시에
실행 중이다. 같은 포트를 또 열 수는 없으므로, MCP client는 **별도의 임시
`fleet serve` 프로세스**를 SSH로 그때그때 띄워 stdio로 붙는다 — `FLEET_HTTP_BIND`/
`FLEET_DASHBOARD_BIND`를 비워 HTTP/dashboard 서버를 아예 기동하지 않는 모드다
(`fleet-cli/src/runtime.rs`가 두 bind 모두 `Option<String>`이라 `None`이면 해당 서버를
건너뛴다). Postgres 접속, `FLEET_MASTER_KEY`, admin bearer token 같은 시크릿은
전부 orchestrator 호스트 안에 머물고 로컬 client 머신에는 절대 내려오지 않는다.

이 SSH 실행에 쓰는 런처가 [`scripts/fleet-mcp-launch.sh`](../../scripts/fleet-mcp-launch.sh)다
— `/etc/fleet/fleet.env`를 순수 `bash source`가 아니라 systemd `EnvironmentFile=`과
동일한 방식(줄 단위, 셸 재해석 없음)으로 읽는다. `FLEET_API_TOKENS`(JSON, 쉼표·중괄호
포함)나 Google App Password(공백 포함) 같은 값이 있으면 순수 `source`는 깨진다 — 이
스크립트는 그 문제를 피하려고 만들어졌다. orchestrator 호스트의
`/usr/local/bin/fleet-mcp-launch.sh`에 이 파일과 동일한 내용으로 배포돼 있어야 한다.
스크립트를 바꿀 때마다 호스트에도 다시 배포한다(자동 동기화 없음).

`FLEET_MCP_CAPABILITIES` env가 실제로 노출되는 MCP 도구 allow-list를 정한다 — 미설정·빈
값·알 수 없는 값이면 MCP가 fail-closed([MCP 도구 계약](../contracts/mcp-tools.md) 참고).

## Claude Code

프로젝트 루트의 `.mcp.json`(gitignored — 이 머신의 SSH 접근 경로에 종속적이라 repo에
커밋하지 않는다)에 다음을 채운다. `<ssh-host-alias>`는 로컬 `~/.ssh/config`에 등록된
orchestrator 호스트 alias, `<fleet-os-user>`는 `/usr/local/bin/fleet` 실행 권한을 가진
서비스 계정(예: `fleet`)이다.

```json
{
  "mcpServers": {
    "grok-fleet": {
      "command": "ssh",
      "args": [
        "-o", "BatchMode=yes",
        "-o", "ConnectTimeout=10",
        "<ssh-host-alias>",
        "sudo -u <fleet-os-user> /usr/local/bin/fleet-mcp-launch.sh"
      ]
    }
  }
}
```

설정 후 Claude Code를 재시작해야 로드된다(세션 중 갱신 불가).

## Antigravity CLI (`agy`)

Google이 2026-05-19부터 Gemini CLI를 Antigravity CLI로 통합 전환했다(아래 Gemini CLI
항목 참고) — 신규로 Google 계열 터미널 에이전트를 붙인다면 이쪽이 정본이다. 설치·인증
절차는 [`docs/deployment/install.md`](./install.md)가 아니라 이 세션에서 `ajou-ec1`에
실제로 설치·검증한 절차를 참고(공식: `curl -fsSL https://antigravity.google/cli/install.sh
| bash`). `agy`는 `~/.gemini/config/mcp_config.json`에 설정을 저장하는 자체 MCP client를
갖고 있어(설정 경로가 `.gemini/`를 그대로 재사용 — Gemini CLI와 config 네임스페이스를
공유한다), JSON을 직접 편집하는 대신 CLI로 등록한다:

```bash
agy mcp add grok-fleet ssh -- \
  -o BatchMode=yes -o ConnectTimeout=10 \
  <ssh-host-alias> \
  "sudo -u <fleet-os-user> /usr/local/bin/fleet-mcp-launch.sh"
```

`<ssh-host-alias>`/`<fleet-os-user>`는 Claude Code 항목과 동일하다. `agy mcp list`로
등록 확인, `agy mcp remove grok-fleet`로 제거. `fleet-mcp-launch.sh`가 그대로
재사용되므로 client마다 별도 서버 구현이 필요 없다 — 이 명령 자체는 ajou-ec1에서
`agy mcp add`/`list`/`remove`로 실제 등록·조회·삭제까지 검증했다(2026-08-21).

## Gemini CLI — 단종 진행 중, 신규 연동엔 권장하지 않음

Google이 2026-05-19 [Gemini CLI를 Antigravity CLI로 통합 전환](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/)한다고
발표했고, **2026-06-18부로 무료·Google AI Pro/Ultra 계정의 Gemini CLI 요청 서빙이
중단**됐다(유료 Gemini Code Assist 라이선스 보유 조직만 예외적으로 계속 접근 가능).
오늘(2026-08-21) 기준 이미 그 기한이 지났다 — 즉 대부분의 계정에서 Gemini CLI 자체가
더 이상 동작하지 않을 가능성이 높다. 기존에 이 방식으로 연결해뒀다면 위 Antigravity
CLI로 옮기는 걸 권장한다. 아래는 참고용으로만 남긴다.

[Gemini CLI MCP 문서](https://geminicli.com/docs/tools/mcp-server/)가 정한 형식을
따른다 — project-level `.gemini/settings.json`(**이 파일도 SSH 접근 경로에 종속적이므로
Claude Code의 `.mcp.json`과 동일하게 커밋하지 않는다** — `.gitignore`에 등록돼 있다).

```json
{
  "mcpServers": {
    "grok-fleet": {
      "command": "ssh",
      "args": [
        "-o", "BatchMode=yes",
        "-o", "ConnectTimeout=10",
        "<ssh-host-alias>",
        "sudo -u <fleet-os-user> /usr/local/bin/fleet-mcp-launch.sh"
      ],
      "timeout": 30000,
      "trust": false
    }
  }
}
```

## ChatGPT — 현재 지원하지 않음

ChatGPT의 custom connector(Developer Mode)는 **원격 HTTPS 엔드포인트 + OAuth**만
받는다. `command`/`args`로 로컬(또는 SSH로 띄운) 프로세스를 실행하는 개념 자체가 없다.
`fleet serve`의 stdio MCP를 그대로 재사용할 방법이 없다는 뜻이다.

지원하려면 다음이 실제로 구현돼야 한다 — 설정 파일 몇 줄이 아니라 새 기능이다:

1. `fleet-mcp`에 HTTP(Streamable HTTP 또는 SSE) transport 추가. 현재는 stdio
   JSON-RPC뿐이다([MCP 도구 계약](../contracts/mcp-tools.md) "전송과 오류" 참고).
2. OAuth 인증(ChatGPT가 요구하는 인증 모드에 맞춰야 함 — admin bearer token을
   그대로 노출하는 방식은 ChatGPT의 커넥터 인증 계약과 맞지 않는다).
3. 그 endpoint를 `fleet.agentthread.dev` 같은 공개 도메인에 얹기 — 지금 MCP는
   orchestrator 호스트 밖으로 전혀 노출되지 않는 설계인데, 이건 공격 표면을
   새로 여는 결정이라 별도 위협 모델 검토가 필요하다
   ([Control-plane security model](../security/control-plane-security-model.md) 참고).

이 세 가지를 로드맵 항목으로 등록할지는 [로드맵](../roadmap/roadmap.md) 갱신을 통해
운영자가 결정한다 — 이 문서는 그 결정 전까지 "안 된다"는 사실과 이유만 기록한다.

## 관련 정본

- [MCP 도구 계약](../contracts/mcp-tools.md)
- [Control-plane security model](../security/control-plane-security-model.md)
- [운영 토폴로지](./topology.md)
