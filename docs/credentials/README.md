---
type: wiki
status: canonical
source: "docs/credentials/README.md"
last_verified: "2026-08-15"
---

# 시크릿·크리덴셜 관리 지침

> Fleet이 운영하는 모든 호스트(orchestrator, worker)와 부속 서비스(wiki-mcp 등)의
> API 키·토큰·비밀번호를 **어디에 두고, 어떻게 기록하고, 언제 회전할지** 정하는 문서.
>
> 이 디렉토리는 [문서 관리 정책](../governance/documentation-policy.md)을 따른다. 이 `README.md`가
> secret 메타데이터 정책, `registry.md`가 현재 스냅샷과 append-only 변경 기록을 소유한다.

## 원칙

1. **평문 시크릿은 절대 git에 커밋하지 않는다.** 이 저장소(`docs/credentials/`)에는
   시크릿의 **메타데이터만** 기록한다 — 이름, 목적, 저장 위치, 형식, 생성일, 회전 주기,
   소비자(어떤 서비스가 쓰는가). 값 자체는 절대 여기 적지 않는다.
2. **저장 위치는 호스트의 `/etc/fleet/`(또는 하위 `secrets/`) 아래로 통일한다.**
   이미 존재하던 관례(`arm2:/etc/fleet/master.key`, `arm2:/etc/fleet/fleet.env`,
   `ec1:/etc/fleet/worker.toml`)를 그대로 따르고, fleet 자체 크레이트가 관리하지 않는
   부속 서비스(wiki-mcp, 향후 추가될 도구)의 시크릿은 `/etc/fleet/secrets/<서비스명>.env`에
   둔다.
3. **권한은 최소화한다.** `root:root 600`을 기본으로 한다. systemd `EnvironmentFile=`은
   PID 1(root)이 읽어서 자식 프로세스에 주입하므로, 서비스가 어떤 사용자로 도는지와
   무관하게 파일 자체는 root 전용으로 잠글 수 있다 — 서비스 계정에 read 권한을 별도로
   줄 필요가 없다.
4. **새 시크릿을 만들면 `registry.md`에 append한다.** 사람이든 에이전트든 예외 없음.
   기록 없이 만들어진 시크릿은 다음 사람(혹은 다음 세션의 나 자신)이 존재를 모른 채
   방치되거나 중복 생성된다 — 이번에 wiki-mcp 키를 즉흥적으로 만들었다가 이 문제를
   지적받은 것이 이 문서의 계기.
5. **워커 LLM API 키(grok-build 모델 등)는 예외 — 이미 전용 시스템이 있다.**
   [`fleet-credentials`](../../crates/fleet-credentials/src/lib.rs) 크레이트가
   AES-256-GCM으로 암호화해 Postgres `worker_credentials` 테이블에 저장하고, 마스터 키로만
   복호화한다. 이 문서의 파일 기반 규칙은 **fleet-credentials가 다루지 않는** 시크릿
   (제3자 API 토큰, 부속 서비스 bearer 키, SSH 키 등)에 적용된다.
6. **세션 중 사용자가 채팅으로 붙여준 토큰은 기본적으로 무저장(ephemeral)이다.**
   작업에 필요한 순간에만 임시 파일(스크래치패드, 권한 600)에 두고 작업 종료 즉시 삭제한다.
   반복적으로 필요한 시크릿이라면 사용자에게 영구 저장 여부를 먼저 확인하고,
   동의하면 위 2번 규칙에 따라 호스트에 배치한 뒤 `registry.md`에 기록한다.

## 회전(rotation)

- 시크릿 유출 의심 시: 즉시 발급처(Cloudflare, LLM 프로바이더 등)에서 폐기 → 새 값 발급 →
  해당 호스트 파일 교체 → 의존 서비스 재시작 → `registry.md`에 회전 일자와 사유 기록.
- 정기 회전 주기는 시크릿별로 `registry.md`의 `회전 주기` 칸에 명시한다. 미정이면
  `수동(미정)`으로 적어 누락되지 않게 한다.
- `fleet-credentials`가 관리하는 워커 키는 `WorkerCredentials.rotated_at`에 감사 기록이
  이미 남는다(코드 참조).

## 새 시크릿을 만들 때 체크리스트

1. 정말 새로 필요한가? 기존 항목 재사용 가능한지 `registry.md`를 먼저 확인한다.
2. 최소 권한으로 발급한다(가능하면 스코프 제한된 토큰).
3. `/etc/fleet/secrets/<서비스명>.env`(또는 fleet-credentials 대상이면 해당 시스템)에 저장,
   `chmod 600`.
4. systemd 서비스가 있다면 `EnvironmentFile=`로 연결하고 `daemon-reload` + 재시작 후 동작 검증.
5. `registry.md`에 항목 추가(append, 과거 항목 수정 금지 — 오탈자 제외).
6. 시크릿 값은 커밋 메시지·PR·문서 어디에도 남기지 않는다.

## 알려진 위생 이슈 (조치 필요, 아직 미조치)

- `arm2:/etc/fleet/fleet.env.bak-debug` — 디버깅용으로 남은 백업 파일로 보임. 원본과 동일한
  민감도를 가질 가능성이 높은데 별도 관리 기록이 없다. 정말 더 이상 필요 없다면 삭제하고,
  필요하다면 이 registry에 정식 항목으로 등재할 것.
