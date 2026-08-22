---
type: review
authority: canonical
implementation: not-applicable
verification: design-reviewed
source: "docs/reviews/bootstrap-automation-review-2026-08-22.md"
last_verified: "2026-08-22"
owners: ["deployment", "security", "architecture"]
---

# 무인 부트스트랩 가능성 검토 (2026-08-22)

> 질문: orchestrator와 seed worker가 자동 설치되고, 다른 호스트가 사용자 간섭 없이
> bootstrap될 수 있는가? 불가능하다면 어떤 작업이 필수인가?
>
> 체계: Bootstrap Chain / Protocol & Procedure / Operational Reality 3인 병렬 감사 후
> 상호 교차검증 1라운드. 아래 사실은 모두 조정자가 코드로 재확인했다.

## 결론

**현재 무인 부트스트랩은 불가능하다.** 그리고 더 중요한 사실은, 무인화 이전에 **이미 운영
중인 fleet을 망가뜨리는 결함**이 있다는 것이다.

## 1. 합의된 사실 (3인 전원 동의 + 코드 재확인)

### C1. 정상 종료가 워커를 파괴한다 — 최우선

`runner.rs:151`이 SIGTERM에서 `client.deregister()`를 호출 → `handlers.rs:480` →
`postgres.rs:479`의 `DELETE FROM workers` 하드 삭제. 연쇄 결과:

| 대상 | FK | 결과 |
|---|---|---|
| `worker_operational_credentials` | `ON DELETE CASCADE` (`018:3`) | 워커 신원 소멸 → 영구 401 |
| `worker_credentials` (암호화된 LLM 키) | `ON DELETE CASCADE` (`005:17`) | **API 키 영구 소실** |
| `hosts.worker_id` | `SET NULL` (`007:17`) | 고아 생존 |

재기동 시 `registration.rs:179-192`가 5초 고정 간격으로 **영원히** 401 재시도하며 영구
실패를 구분하지 않는다. 직관과 반대로 **깨끗하게 종료할수록 망가진다**:

| 종료 방식 | 결과 |
|---|---|
| `systemctl stop` (연결 정상) | 벽돌 + LLM 키 소실 |
| SIGTERM + orchestrator 불가 | 무사 |
| SIGKILL / 전원 상실 | 무사 |

`allow_no_auth` 모드만 살아남지만 `runtime.rs:413-417`이 무인증 비-loopback bind를 거부하므로
그 모드는 loopback 전용이다 — **원격 워커가 존재할 수 있는 모든 구성이 벽돌이 된다.**
`mem.rs:277-280`은 cascade하지 않아 인메모리 테스트가 이 결함을 영원히 통과시킨다.

복구 경로 전수 조사 결과 동작하는 것은 **새 bootstrap token + `fleet-worker join` 재실행**
하나뿐이다. `credential rotate`는 worker row가 없어 404이고, admin `register`는 operational
credential을 발급하지 않는다. `config.rs:411-414`가 사용자에게 안내하는 복구 문구
(`fleet workers credential rotate` 사용)는 **동작하지 않는 막다른 길**이다.

### C2. `fleet provision`이 만든 워커는 기동할 수 없다

`templates.rs:147-148`이 `[worker] bootstrap_token`을 렌더하고 `config.rs:403-419`가 그 키를
fail-closed 거부한다. `operational_token`은 `TemplateContext`에 필드조차 없고, 프로비저너 전체에
`join` 참조가 0건이다. 토큰을 주면 7번째 스텝에서 반드시 실패하고, 주지 않으면 Authorization
헤더 없이 401이 난다. `templates.rs:329,341` 테스트가 이 깨진 출력을 정답으로 고정하고 있다.

### C3. 인벤토리 경로는 어떤 호스트에서도 완주할 수 없다

`runtime.rs:1833`이 `fleet_worker_bin: None`을 하드코딩하고 인벤토리 스키마에 대체 필드가 없다.
커밋된 25노드 인벤토리에는 `grok_secret`도 `api_token`도 없다. 단일 호스트 모드만 인자를 전달한다.

### C4. 아키텍처·OS 감지가 버려진다

`check_prereqs.rs:51`이 `uname -m`을 정확히 읽지만 `runtime.rs:1771-1779`가 결과를 버리고
`os:"ubuntu", arch:"x86_64"`를 하드코딩한다(코드 주석: "단순화"). `install_cloudflared.rs:69-70`은
amd64 바이너리를 무조건 받는다. 커밋된 인벤토리 25대 중 **7대가 arm64**다.

### C5. 원격 실패가 조용히 삼켜진다

`ssh.rs:542-544`의 `exec`가 원격 exit code를 에러로 승격하지 않는다. `install_fleet_worker.rs`의
`sudo mv`/`daemon-reload`, `start_services.rs:51-58`의 systemd 명령, `push_credentials.rs:123-129`,
`install_cloudflared.rs`의 거의 전부가 `let _ =` 또는 `|| true`다. 스텝은 "Applied"로 보고되고
실패는 뒤에서 드러난다. `start_services.rs:11`의 `wait_timeout_secs`는 참조 0건인 죽은 필드이고,
`inventory.rs:230`의 `retry_failed`도 선언만 있고 참조 0건이다.

### C6. 저장소 예시대로 하면 orchestrator가 기동하지 않는다

`examples/fleet.env:21`은 `FLEET_API_TOKENS=fleet-CHANGE_ME_TOKEN_1` 평면 문자열,
`examples/fleet.service:31` 주석은 `token1,token2`. `runtime.rs:582`의 `parse_scoped_api_tokens`는
JSON 배열만 허용한다. 유일한 릴리스 태그 `v0.1.0`은 2026-07-20이고 이후 커밋 254개,
마이그레이션 18개가 쌓였다. `examples/fleet-worker.service` 헤더 4행은 "`fleet provision`이 자동
배포한다"고 적혀 있으나 실제 배포되는 것은 `templates.rs:243-255`의 `User=root` + 하드닝 0개 유닛이다.

### C7. 최초 admin 토큰의 닭-달걀

`fleet admin-tokens create`와 `fleet token issue` 둘 다 기존 `--api-token`을 요구한다.
`sync_env_admin_tokens_to_store`는 env에 있는 토큰을 DB로 가져올 뿐 생성하지 않는다.
사람이 JSON manifest를 손으로 작성하는 것 외에 경로가 없다.

**dashboard OTP 재사용은 현재 불가능하다**: `rbac.rs:153-157`의 `issue_admin_bootstrap_if_needed`가
`purpose`를 구분하지 않고 모든 usable bootstrap token을 센다(코드 주석이 인정, "Phase 9.1.3"으로
연기). `purpose` 컬럼은 `004_rbac.sql:114`에, 타입은 `auth.rs:569`에 이미 존재하는데 이 함수만
쓰지 않는다. 무인 join을 구현해 호스트마다 단명 토큰을 찍기 시작하면 admin OTP는 **사실상 영원히
발급되지 않는다.**

## 2. 감사관 간 범위 경계 (모순 아님, 명시적 분리)

Bootstrap Chain 감사관은 `/ws` 라우트 부재와 transport를 자기 목록에서 **의도적으로 제외**했다.
근거: 워커는 `/v1`로 join·register·heartbeat를 하므로 `/ws` 없이도 온라인이 된다. `/ws`는
`--transport acp`가 실제로 작업을 디스패치할 때만 문제다.

Operational Reality 감사관은 이를 자기 목록의 핵심 항목으로 두었다. 근거: transport 없이는
fleet이 일을 하지 못한다.

**조정 결과 — 둘 다 맞고, 두 질문이 다르다:**

| 질문 | 답 |
|---|---|
| fleet이 무인으로 **뜨는가** | `/ws` 불필요 |
| 뜬 fleet이 **일을 하는가** | `/ws` 또는 mTLS 필수 |

Bootstrap Chain 감사관 자신이 결론에 명시했다 — 무인 부트스트랩이 완성돼도 transport가 저장소
밖에 남으면 그 fleet은 "작업을 수행하지 못하는 상태로 무인 기동"될 뿐이다.

## 3. Transport 현실 (사용자 결정의 근거)

### 리버스 SSH 터널 = 현재 실운영 모델, 저장소 재현 불가

`config.rs:340-357`이 `{scheme}://{orchestrator_host}/ws/{name}?server-key=`를 광고하고,
코드 주석이 이 모델과 2026-08-11의 24시간 장애를 기록한다. 그런데:

- `fleet-api`에 `WebSocketUpgrade` **0건** — `/ws` 라우트가 없다
- `autossh`/`ssh -R`/`RemoteForward` 검색 결과가 **주석과 문서 서술뿐**
- nginx 설정 파일 **0건**, `/ws/<name>` location 맵 생성 코드 없음
- `docs/credentials/registry.md:122`는 `ec1`/`ec2`의 ACP 리버스 SSH를 **이미 운영 중인 사실**로 기록
- 정본인 `topology.md:26-27`은 그 방식을 **지원 토폴로지에서 배제**

즉 운영 중인 핵심 경로가 정본 문서에서 "쓰지 않는 것"으로 선언되어 있고, 저장소로 재현할 수 없다.

### mTLS 직접 다이얼 = 런타임 완성, 배포 파이프라인만 결손

| 구성 요소 | 상태 |
|---|---|
| CA·서버·클라이언트 인증서 발급 CLI | 구현됨 (`main.rs:1009-1064`) |
| SAN 누락 시 발급 거부 | 구현됨 (`mtls.rs:108-112`) |
| 워커측 TLS 종단 + grok 포워딩 | 구현됨 (`runner.rs:195-255`, `mtls_proxy.rs:88-120`) |
| 인증서 무중단 회전 | 구현됨 (`RotatingCertResolver`) |
| orchestrator측 클라이언트 인증 | 구현됨 (`runtime.rs:70-105`) |
| 워커 바이너리 mTLS 포함 | 무조건 (`fleet-worker/Cargo.toml:29`) |
| **PEM 파일을 워커에 업로드** | **없음** (`inventory.rs:144-146` 주석이 명시) |
| **SAN ↔ `advertised_host` 일치 보장** | **없음** |
| advertised_host DNS 레코드 | 없음 |
| 인바운드 2420 방화벽 | 없음 |

**터널 경로는 저장소 밖 인프라 전체가 필요하지만, mTLS 경로는 업로드 스텝 하나와 SAN 일관성
규칙 하나로 저장소 안에서 닫힌다.** 단 사설 IP 호스트(`mini01`, `172.16.1.101`)는 이 경로로
영구 불가능하다.

## 4. 코드로 없앨 수 없는 수동 단계

무인화를 어디까지 밀어도 남는 것:

- **계정·결제**: x.ai 계정과 API 키 최초 발급, 클라우드 계정·결제 수단
- **DNS**: 존 소유와 위임, `advertised_host` 명명 정책, ACME 도메인 통제권 증명
- **신뢰 판단**: SSH 호스트키 지문의 대역 외 검증(`scan-host-keys`는 지문을 출력할 뿐), 사설 CA
  루트 키의 생성·보관 매체 결정, join 승인 정책의 최초 규정, 워커 격리 등급 위험 수용
- **물리·네트워크**: 온프레미스 호스트 전원·랙, 클라우드 보안그룹 인바운드 개방, 사설 IP 도달 설계
- **상태**: PostgreSQL 인스턴스와 서비스 계정, 백업 오프호스트 사본 위치·암호화 키·RPO, NTP 소스
- **릴리스**: "이 커밋을 릴리스로 승격한다"는 판단과 태그

## 5. 합의된 최소 순서

두 감사관이 독립 작성한 목록의 교집합과 순서 근거를 병합한 결과:

| 순서 | 항목 | 근거 |
|---|---|---|
| 0 | `examples/fleet.env`·`fleet.service` 형식 정정 | 이걸 안 고치면 저장소만 보고 뜨는 orchestrator를 재현할 수 없다 — 이후 모든 검증의 전제 |
| 1 | graceful shutdown의 hard-delete 중단 + `MemStore` 동작 일치 | 유일하게 **이미 동작 중인 fleet을 망가뜨리는** 항목. 남아 있으면 아래 전부가 무가치 |
| 2 | `ssh.rs` exit code 전파 + `let _ =`/`\|\| true` 제거 | 가장 싸면서, 없으면 이후 모든 스텝의 성공 여부를 검증할 수 없다 |
| 3 | 최초 admin 토큰 발급 경로 | 프로비저너의 모든 API 호출 스텝(5번 포함)을 게이팅 |
| 4 | `check_prereqs` 결과 전달 + arch 분기 | 앞 스텝이 조용히 틀린 일을 한 호스트에 join을 얹으면 원인이 뒤섞인다. 인벤토리 25대 중 7대가 arm64 |
| 5 | provision→join 배선 | `exec_with_stdin`, `StartServices` 앞 `JoinWorker` 스텝, `bootstrap_token` 방출 제거, worker.toml 렌더러를 orchestrator로 단일화, 신원 보존형 `is_applied` |
| 6 | 인벤토리 완결성 (`fleet_worker_bin` 배선, `grok_secret` 제거) | 없으면 인벤토리 모드는 아무것도 설치하지 못한다 |
| 7 | 바이너리 업로드 방식 교체 (base64-in-argv → 청크/SFTP) | 진짜 맨 호스트에서는 hard blocker |
| 8 | transport 확정 (사용자 결정 — 아래) | 여기까지 해도 fleet은 "일하지 못하는 상태로 무인 기동"된다 |

## 6. 무인 join 설계 (결정 시 채택안)

- **관리자 bearer는 `fleet provision` CLI 프로세스만 보유한다.** 대상 호스트에 절대 전달하지
  않는다. `PushCredentials`가 이미 이 패턴이다 — HTTP 호출은 CLI가, SSH로는 결과만.
- **호스트마다 직전에 `max_uses: 1` + 짧은 TTL 토큰을 1개씩 발급한다.** 현재의 전 워커 공유
  단일 토큰(`inventory.rs:240-242`)은 삭제한다.
- **전달은 stdin으로만.** `RemoteExecutor::exec_with_stdin`을 신설해 `fleet-worker join
  --token-file -`에 채널로 직접 쓴다. argv·쉘 히스토리·디스크에 남지 않는다.
  `write_file`(`ssh.rs:616-622`)은 base64를 명령행에 보간하므로 **써서는 안 된다.**
- **`operational_token`은 대상 호스트가 스스로 쓴다.** 프로비저너는 `fwo_` 값을 보지도 저장하지도
  않는다 — 이 설계의 가장 강한 성질이므로 명시적으로 유지한다.
- **`is_applied`는 파일 존재가 아니라 신원 검사다.** `existing_worker_id`를 읽어
  `GET /v1/workers/{id}`가 200이면 skip. **`existing_worker_id`를 가진 worker.toml은 절대
  덮어쓰지 않는다** — 현재 `install_fleet_worker.rs:91-95`의 무조건 `mv`가 제거 대상이다.
- **롤백하지 않는다.** 실패 보상은 항상 전진(재시도/re-key)이며 후퇴(워커 삭제)가 아니다 —
  정리한다고 워커 행을 지우는 것이 정확히 C1 결함이다.
- 전제: orchestrator에 `FLEET_BASE_URL`이 설정돼 있어야 한다(`runtime.rs:426-431`). 없으면
  렌더된 `orchestrator_url`이 플레이스홀더로 남는다.

## 7. 결정 (2026-08-22, 프로젝트 소유자)

| 질문 | 결정 | 반영 위치 |
|---|---|---|
| Agent dispatch transport | **mTLS 직접 다이얼**. Cloudflare Tunnel과 reverse SSH tunnel은 지원 토폴로지가 아니다 | [Topology](../deployment/topology.md), Roadmap `#85` |
| C1 수정 형태 | **워커의 자기 deregister 제거** + `MemStore` 동작 일치. soft-delete는 채택하지 않는다 | Roadmap `#78` |
| 무인 join 범위 | **`fleet provision`이 join을 대행한다** (완전 무인) | [Worker provisioning](../deployment/worker-provisioning.md), Roadmap `#82` |
| 이기종 fleet | **지원한다** — `check_prereqs` 결과를 후속 스텝에 전달하고 아키텍처별 자산을 선택 | Roadmap `#81` |

### 결정에 따라 확정된 것

- **`/ws` 라우트는 만들지 않는다.** mTLS 직접 다이얼에서는 control plane이 Worker에 연결하므로
  orchestrator측 WebSocket 라우트가 필요 없다. 리버스 터널 자동화(터널 데몬, 워커별 포트 할당,
  nginx location 맵)도 범위 밖이다.
- **사설 IP 뒤 Worker는 이 토폴로지에서 제외된다.** 커밋된 인벤토리의 `mini01`(`172.16.1.101`)은
  별도 네트워크 설계 없이는 Worker가 될 수 없다.
- **`InstallCloudflared`를 표준 playbook에서 제거한다**(`#85`).
- **`ProvisionOptions.bootstrap_token`(전 워커 공유 단일 토큰)을 삭제한다** — 호스트마다
  `max_uses: 1` 단명 토큰으로 대체(`#82`).
- **dashboard OTP 재사용은 금지한다** — `purpose` 필터링이 선행되지 않는 한 `#82` 적용 후
  admin OTP가 영구 미발급된다(`#80`).

### 아직 결정되지 않은 것

- **SSH host-key 정책**: 코드 기본값은 `tofu`(무인 가능), `worker-provisioning.md`는 운영에
  `strict`를 요구(사람의 대역 외 지문 검증 필요). 두 정본이 상충한 채로 남아 있다.
- **Worker 신뢰 등급**: 배포되는 유닛은 `User=root` + 하드닝 0개인데
  `execution-isolation.md`는 root 금지·container 격리를 정본으로 선언한다. 또한 LLM API 키를
  워커 디스크에 평문으로 둘지(현행), orchestrator의 LLM gateway를 경유해 워커가 키를 아예 갖지
  않게 할지(`grok_process.rs`에 배선 존재) 미결.
- **릴리스 정책**: HEAD로 새 태그를 찍을지, `install.md`를 소스 빌드 기준으로 고칠지.
