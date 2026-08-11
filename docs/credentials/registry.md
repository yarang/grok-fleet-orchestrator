# 시크릿·크리덴셜 레지스트리

> 정책은 [`README.md`](./README.md) 참고. 이 문서는 **메타데이터만** 기록한다 — 값은 절대 넣지 않는다.
>
> 상단 표는 "현재 유효한 항목의 스냅샷"(항목 폐기/교체 시 갱신), 하단 [변경 이력](#변경-이력)은
> append-only 기록(새 줄만 추가, 과거 줄은 오탈자 외 수정 금지)이다.

## 현재 항목

| 이름 | 목적 | 저장 위치 | 형식 | 소비자 | 회전 주기 | 상태 |
|---|---|---|---|---|---|---|
| `FLEET_MASTER_KEY` | `fleet-credentials`(워커 LLM API 키 암호화)의 AES-256-GCM 마스터 키 | `arm2:/etc/fleet/master.key` (`r--------`, fleet:fleet) | 32B, hex 또는 base64url | `fleet-api`, `fleet-cli`(provision/rotate) | 수동(미정) — 회전 시 `worker_credentials` 테이블 전체 재암호화 필요 | 사용 중 |
| `DATABASE_URL` 등 오케스트레이터 환경변수 | Postgres 접속·기타 orchestrator 설정 | `arm2:/etc/fleet/fleet.env` (`rw-r-----`, root:fleet) | dotenv | `fleet.service`(EnvironmentFile) | 수동(미정) | 사용 중 |
| 워커별 LLM API 키 (`grok-build` 모델 등: GLM, Gemini 등) | grok agent가 워커에서 실행 시 사용하는 모델 프로바이더 키 | Postgres `worker_credentials` 테이블 (AES-256-GCM 암호화, `fleet-credentials` 크레이트) | `WorkerCredentials` 구조체 → `~/.grok/config.toml` `[model.X]` 섹션 렌더링 | 각 워커 호스트의 grok 클라이언트 | `WorkerCredentials.rotated_at`로 감사 추적 (자동 회전 없음) | 사용 중 — 이미 전용 시스템으로 관리됨, 이 파일 기반 규칙 대상 아님 |
| `WIKI_MCP_API_KEY` | `wiki.agentthread.dev/mcp` Streamable HTTP 엔드포인트 Bearer 인증 | `ec1:/etc/fleet/secrets/wiki-mcp.env` (`rw-------`, root:root) | `secrets.token_urlsafe(32)`, base64url 문자열 | `wiki-mcp.service`(EnvironmentFile) | 수동(미정) | 사용 중 — 2026-08-10 생성 |
| Cloudflare API 토큰 (`agentthread.dev` 존, DNS 편집 권한) | DNS 레코드 자동화(A 레코드 생성 등) | `arm2:/etc/fleet/secrets/cloudflare.env` (예정, `rw-------` root:root) | Cloudflare API Token (`cfut_...`) | 수동 실행(현재 자동화된 서비스 소비자 없음 — ad-hoc 작업용) | 수동(미정) | **대기 — 사용자가 값을 다시 전달하면 저장 완료 처리** |
| ec1 워커 부트스트랩 시크릿 | 워커 등록/조인 인증 | `ec1:/etc/fleet/worker.toml` | `worker_join_authentication_design.md` 참고 | `fleet-worker.service` | 수동(미정) | 사용 중 — 상세는 [`worker_join_authentication_design.md`](../worker_join_authentication_design.md) 참고, 값 위치만 여기 등재 |
| SSH 호스트 접근 키 (`oci-yarangdev-arm1/arm2/ec1/ec2`) | 운영자(사람/Claude 세션)의 프로덕션 호스트 SSH 접근 | 로컬 Mac `~/.ssh/`(config·개인키), 각 호스트 `~/.ssh/authorized_keys` | OpenSSH 개인/공개키 | 사람 운영자 SSH 클라이언트 | 수동(미정) | 사용 중 — 이 registry의 관리 범위 밖(로컬 머신 전용, 서버 배치 대상 아님) |

## 알려진 미정 항목 (조치 필요)

- `arm2:/etc/fleet/fleet.env.bak-debug` — 소유자/생성 경위 불명. `README.md` "알려진 위생
  이슈" 참고.

## 변경 이력

### 2026-08-10 — wiki-mcp API 키 신규 발급 및 위치 확정

- `wiki-mcp`를 ec1에 systemd 서비스로 배포하며 `WIKI_MCP_API_KEY` 신규 생성.
- 최초에는 `/opt/wiki-mcp/app/.env`(앱 디렉토리 내부)에 뒀다가, fleet 표준 시크릿 위치
  관례(`/etc/fleet/`)를 따르지 않는다는 지적에 따라 `/etc/fleet/secrets/wiki-mcp.env`로
  이관. systemd 유닛의 `EnvironmentFile`도 함께 갱신, 외부 엔드포인트
  (`https://wiki.agentthread.dev/mcp`) 재검증 완료.
- 이 이관을 계기로 `docs/credentials/` 디렉토리(본 registry + `README.md` 정책) 신설.

### 2026-08-10 — Cloudflare API 토큰 저장 정책 결정 (영구 저장)

- `wiki.agentthread.dev` DNS 레코드 생성 작업에 사용자가 채팅으로 Cloudflare API 토큰을
  전달, 세션 내 스크래치패드에 임시 보관 후 작업 완료 즉시 삭제(무저장 원칙).
- 이후 반복 사용 가능성을 고려해 영구 저장 여부를 사용자에게 확인 → **영구 저장 결정**
  (`arm2:/etc/fleet/secrets/cloudflare.env` 예정).
- 삭제 원칙 때문에 세션에는 값이 남아있지 않아, 사용자에게 재전달을 요청한 상태.
  값 저장이 완료되면 이 항목에 후속 로그를 추가할 것.
