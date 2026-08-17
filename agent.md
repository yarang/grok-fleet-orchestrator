# 에이전트 협업 가이드 (Agent Collaboration Guide)

> 작성일: 2026-08-06. 최종 개정: 2026-08-17 — §5를 중앙 문서 체계와 재작성 게이트에 맞춰 갱신.
>
> 이 문서는 Grok Fleet Orchestrator 프로젝트에 기여하는 모든 AI 개발 에이전트(Agent)가 개발 과정에서 반드시 지켜야 할 **Git 정책, 개발 시퀀스, 보안baseline 및 테스트 검증 규칙**을 명시합니다. 에이전트는 기여를 시작하기 전에 본 가이드를 완전히 숙지해야 합니다.

---

## 1. Git 정책 (Git Policy)

에이전트는 프로젝트의 변경 이력을 명확히 하고, 코드의 추적성(Traceability)을 확보하기 위해 다음 Git 정책을 철저히 이행해야 합니다.

### 1.1 Conventional Commits 표준 준수
모든 커밋 메시지는 다음 접두사(Prefix) 규칙을 준수하여 작성해야 합니다.

*   `feat:`: 새로운 기능 구현 (예: `feat: add database persistence for circuit breaker`)
*   `fix:`: 버그 수정 (예: `fix: cloudflare jwt verification issue`)
*   `test:`: 테스트 코드 추가 또는 수정 (예: `test: add scale-out sync integration test`)
*   `docs:`: 문서 수정 또는 생성 (예: `docs: update example domains to agentthread.dev`)
*   `refactor:`: 기능 동작은 같으나 코드 구조를 리팩토링하는 경우

### 1.2 로드맵 번호 매핑 및 이력 추적성
*   **로드맵 매핑**: 에이전트가 작업을 수행하고 커밋할 때, 커밋 본문이나 제목에 관련 개발 로드맵 번호(`S1`~`S6` 또는 `#15`, `#25` 등)를 명시하여 변경 사유를 증적화합니다.
    *   *예시*: `fix: record failure only on actual lockout (Fixes S1)`
*   **Git Log 보존**: 로컬 Git Log는 메인 브랜치의 신뢰 원천입니다. 변경 전후의 히스토리가 꼬이지 않도록 이전에 정상 작동하던 커밋을 훼손하지 않습니다.

### 1.3 무결성 커밋 단계
1.  **Stage (`git add`)**: 변경된 소스 및 관련 문서 파일만 정밀하게 추가합니다. 임시 파일(`.claude/` 등)이 스테이징 영역에 포함되지 않도록 마스킹 처리합니다.
2.  **Commit (`git commit`)**: 상세한 변경 내역(변경 부분, 이유, 연동된 로드맵 이슈 번호)을 본문에 포함하여 커밋합니다.

---

## 2. 개발 로드맵 (Development Roadmap)

프로젝트 개발은 기 정의된 단계별 우선순위에 의존하며, 에이전트는 단계를 뛰어넘어 개발을 임의로 진행할 수 없습니다.

```
[1단계: 결함 해결] (완료) ───────► [2단계: 기반 인프라 & 상태 동기화] ───► [3단계: 고급 서버 관리 기능 안착]
- S1~S6 패치 완료                  - Nginx 게이트웨이 및 Certbot 적용       - UFW 방화벽 및 IP 화이트리스트 도입
- Real IP 정상 역추출 확보          - #25 (서킷 브레이커 상태 DB 공유화)     - GPU 오버히트 및 자가 치유 자동화
- Cloudflare JWT 서명 검증         - #15 (설정 파일 밸리데이터 정비)        - SSH Key 교체 및 회수 도구 도입
```

### 2.1 1단계: 보안 결함 해결 (완료)
*   **S1**: `/login`·bootstrap 락아웃 증폭 DoS 차단
*   **S2**: Cloudflare Access JWT 서명 & 체인 검증 구현 (JWKS 동적 캐시)
*   **S3**: Real Client IP 역추출 구조 구현 (`FLEET_TRUSTED_PROXIES` 연동)
*   **S4**: IP 실패 카운터 스코프 분리 (비밀번호 재설정/인증 재발송 로그 제외)
*   **S5**: `clear_login_attempts` SQL NULL 바인딩 오류 수정 및 로그인 성공 시 전체 IP 누적 실패 초기화
*   **S6**: 이메일 전송 엔드포인트 시간당 3회 레이트 리밋 제한 적용

### 2.2 2단계: 기반 인프라 & 상태 동기화 (진행 완료)
*   **Nginx 게이트웨이 표준화**: Caddy를 완전히 대체하여 HTTPS TLS 종단, X-Real-IP 포워딩, SSE 버퍼링 방지(`proxy_buffering off`) 하드닝 완료.
*   **#15. 시작 시 설정 검증**: 오케스트레이터 기동 시 `FLEET_TRUSTED_PROXIES` 환경변수 유효성을 엄격하게 파싱하여 잘못된 형식 기입 시 즉시 종료하는 Fail-Fast 밸리데이터 구현.
*   **#25. 서킷 브레이커 DB 영속화**: 다중 오케스트레이터 배포 시 상태 동기화를 보장하기 위해, DB `worker` 테이블의 `circuit_state` 컬럼을 갱신하는 쿼리를 추가하고, Postgres LISTEN/NOTIFY를 통한 실시간 메모리 브레이커 상태 동동화 연동 및 스케일아웃 통합 테스트 통과 완료.

### 2.3 3단계: 고급 서버 관리 기능 안착 (대기)
*   **UFW 방화벽 제어 및 IP 화이트리스팅**: 워커 가입 시 해당 워커 IP를 오케스트레이터 방화벽에 자동 등록하는 룰 제어 구현.
*   **하드웨어 오버히트 및 자가 치유 자동화**: 클라우드 가상 VM 환경에서도 작동 가능한 NVML Throttling 감지 메커니즘 구축 및 서킷 브레이커 오픈 시 인프라 제어 연동.
*   **SSH Key 보안**: 주기적 SSH Key 순환(Rotation) 및 접근 제어 자동화.

### 2.4 로드맵 무결성 규칙
*   항목 번호는 팀 내 참조 키이므로 **완료되어도 번호를 재사용하거나 삭제하지 않습니다.** 완료된 것은 ✅로 표시하고 원인/커밋 해시를 남겨둡니다.
*   로드맵 상태의 갱신 오너는 에이전트(Planner)이며, 한 단계 완료 시점마다 코드 실측 대조 후 `docs/roadmap/roadmap.md`를 일괄 업데이트해야 합니다.

---

## 3. 보안 및 품질 검증 기준

에이전트는 코드를 병합하기 전에 반드시 로컬 하네스를 가동하여 완전성을 자체 증명해야 합니다.

1.  **Real IP 헤더 무결성**:
    모든 프록시 헤더를 신뢰하기 전에 `FLEET_TRUSTED_PROXIES` allow-list에 선언된 신뢰 프록시 대역인지 1차로 필터링해야 하며, 임의의 헤더 주입으로 인한 IP 위조 공격(IP Spoofing)에 대응할 수 있도록 아키텍처를 견고하게 구현합니다.
2.  **E2E 단위 및 통합 테스트**:
    *   에이전트는 수정 완료 후 반드시 `cargo test`를 실행하여 컴파일 경고나 테스트 실패가 없음을 확인해야 합니다.
    *   특히 SQL 쿼리가 수정되었거나 DB 트레이트가 변경되었을 경우, `DATABASE_URL`을 주입하여 `fleet-store` 통합 테스트 및 스케일아웃 동기화 테스트(`scaleout_sync`)를 직렬 가동해 확인해야 합니다.
3.  **Flaky 테스트 방치 금지**:
    테스트 도중 간헐적으로 깨지는 테스트가 식별되면 환경적 영향(OS 소켓 점유 등)인지, 락(Lock) 경쟁 등의 런타임 버그인지 명확히 규명하고 가이드라인에 기록해야 합니다.

---

## 4. CI 및 피처 게이트(Feature Gate) 검증 기준

GitHub Actions CI 환경에서 조건부 컴파일 및 코드 검사가 깨지지 않도록 하기 위해 다음 규칙을 필수로 이행합니다.

1.  **조건부 임포트와 네임 리졸브 방지**:
    *   특정 피처(예: `acp`, `mtls` 등)가 꺼진 최소 기능 빌드(`--no-default-features`)를 돌릴 때, 해당 피처 하위에만 존재하는 타깃 타입(예: `AcpTransport`, `ClientTlsConfig`)을 코드 상에서 직접 지칭(예: `fleet_transport::AcpTransport`)하여 Name resolution 오류가 유발되지 않도록 주의해야 합니다.
    *   관련 외부 크레이트 바인딩은 상단 임포트 시 `#[cfg(feature = "...")]` 로 가드하고, 코드 내에서는 네임스페이스 접두사 없이 직접 가져온 타입을 바인딩하여 컴파일러 파싱 영역에서 안전하게 제거되도록 구현해야 합니다.
2.  **Clippy 경고 무결성 준수**:
    *   코드 빌드 시 clippy 경고가 발생하면 CI 파이프라인이 즉시 실패합니다.
    *   특히 `#[cfg(test)] mod tests` 모듈 뒤에 임의의 공개/비공개 헬퍼 함수를 정의하는 것은 `clippy::items_after_test_module` 경고를 유발하므로, 테스트 모듈 뒤에는 어떠한 일반 구현체도 배치하지 말고 항상 파일의 최하단에 테스트 모듈을 두도록 코드를 구조화합니다.
3.  **원격 Push 전 로컬 자가 진단**:
    *   수정 사항을 `origin` 원격지로 푸시하기 전에, 에이전트와 개발자는 로컬 터미널에서 다음 2가지 검증 명령을 반드시 통과시켜야 합니다:
        ```bash
        # 1. 최소 기능 빌드에서의 컴파일 오류 유무 확인
        cargo check --no-default-features

        # 2. 전체 타깃/피처 하에서의 Clippy 경고 유무 확인
        cargo clippy --all-targets --all-features
        ```

---

## 5. 문서 정본성 및 재작성 규약

문서는 코드와 동등한 프로젝트 자산이다. 일반 문서의 정본 위치·상태는
[`docs/index.md`](./docs/index.md), 세부 정책은
[`docs/governance/documentation-policy.md`](./docs/governance/documentation-policy.md),
전체 재작성 절차는
[`docs/governance/documentation-rewrite-guide.md`](./docs/governance/documentation-rewrite-guide.md)를
따른다. 논의·감사·비교 부기는 [`docs/reviews/README.md`](./docs/reviews/README.md)에만 둔다.
`llm-wiki/`와 `credentials/`만 자체 부기 체계를 유지한다.

### 5.1 작업 전 분류

문서를 새로 만들거나 크게 고치기 전에 다음을 먼저 확인한다.

1. 도메인 진입점(`README.md` 또는 `index.md`)에서 독자가 이 문서에 도달할 **읽기 순서**와, 문서가 소유할 **질문 하나·기능상 계약 하나**를 적는다.
2. 문서를 `canonical`, `derived`, `runbook`, `proposed` 중 하나로 분류한다. 정본은 현재 결정·불변식·상태 전이·완료 조건만 소유한다.
3. 구현 상태를 `proposed`, `partial`, `implemented`, `retired` 중 하나로, 검증 강도를 근거에 맞게 표기한다. 목표 설계를 현재 동작처럼 쓰지 않는다.
4. 기존 정본과 책임이 겹치면 새 문서를 만들지 않는다. 기존 정본을 보강하거나, 책임 경계를 먼저 정본 지도에 기록한다.

### 5.2 재작성·폐기·논의 분리 규칙

* 도메인 진입점은 그 도메인의 파일 책임·정본 지위·읽기 순서만 관리한다. 세부 계약이나 절차를 다시 쓰지 않는다.
* 재작성은 현재 계약을 명확히 하는 작업이다. 긴 코드 인용은 Derived에 두고, 논의·감사·대안 비교·과거 정정은 `docs/reviews/`에만 둔다.
* 논의로 확정한 결과만 정본의 “결정” 절에 기록한다. 찬반, 회의 대화, 결정을 바꾸기 전의 대안은 정본에 남기지 않는다.
* `overview.md`는 시스템 경계·현재 구현 상태·정본 탐색을 제공하는 얇은 Derived 입문 지도다. 세부 코드 대조는 `implementation-reference.md`에, Worker join·bootstrap은 `worker-bootstrap/`에, 미구현 Self-Healing은 `operations/proposals/`에 둔다.
* 폐기 문서는 Deprecated 포인터로 남기지 않는다. 대체 정본·모든 inbound link·도메인 진입점·`docs/index.md`를 먼저 갱신한 뒤 파일을 삭제하고, 이유는 `docs/log.md`와 필요할 때 `docs/reviews/`에 남긴다.

### 5.3 작성 형식과 문체

* 설계 문서는 `목적과 범위 → 현재 상태 → 결정 → 계약 → 실패와 복구 → 검증 → 미결정 사항 → 관련 정본` 순서를 기본으로 사용한다.
* 현재 상태, 목표 설계, 미결정 사항을 같은 문단이나 표에서 섞지 않는다. 미결정 사항에는 심각도, 담당자, 결정 기한, 차단 단계, 정본 소유자를 붙인다.
* 단정은 코드·테스트·운영 근거가 있을 때만 사용한다. 근거가 없으면 `제안`, `가정`, `미구현`을 명시한다.
* 한 문단은 하나의 주장만 담고, 능동형·현재형의 짧은 문장을 사용한다. "향후 검토한다"처럼 주체·조건·완료 기준이 없는 문장을 쓰지 않는다.
* 구조, 상태, 흐름을 설명할 때는 Mermaid 또는 SVG를 사용한다. ASCII-art 박스 다이어그램과 로컬 절대 `file:///` 링크는 새로 작성하지 않는다.

### 5.4 완료 게이트

문서 재작성 완료 전에는 다음을 확인한다.

1. frontmatter에 authority, implementation, verification, 문서 경로, 검증 시점·근거·소유자를 기록한다.
2. 코드 심볼·API·환경변수·포트·상태 전이를 실제 코드와 대조하고, 문서의 구현 상태와 일치시킨다.
3. 중복된 현재 규칙을 정본 링크로 교체하고, 폐기 문서는 링크 정리 뒤 삭제한다.
4. 상대 링크와 다이어그램 참조를 점검하고 `docs/index.md` 및 `docs/log.md`를 갱신한다.
5. 논의·비교 부기는 `docs/reviews/`에, 임시 분석·초안은 `.tmp/`에 둔다. `.tmp/`는 최종 문서와 섞거나 커밋하지 않는다.
6. 링크·형식 검증 후 변경 범위를 다시 확인하고, 코드 변경과 분리된 `docs:` Conventional Commit을 남긴다.

---

## 6. 다이어그램 및 SVG 리소스 관리 규약 (Diagram & SVG Resource Policy)

`docs/` 하위 전체(도메인 구분 없이)의 모든 마크다운 문서 작성/수정에 적용되는 규약입니다. 아키텍처·흐름·구조를 설명하는 문서에서 다이어그램 없이 텍스트만으로 서술하는 것을 지양합니다.

### 6.1 다이어그램 우선 원칙
*   아키텍처, 시퀀스/흐름, 상태 전이, 데이터 모델(ER), 컴포넌트 관계 등 "구조"를 설명하는 절에는 다이어그램을 기본으로 포함합니다. 텍스트 서술만으로 끝내지 않습니다.
*   표현 방식은 목적에 따라 구분합니다:
    *   **노드-엣지 구조**(시퀀스, 플로우차트, 상태 다이어그램 등)로 텍스트 기반 렌더러(Mermaid)가 감당 가능한 경우 → Mermaid 소스(`.mermaid`)를 우선 사용합니다. 버전 관리 diff에 유리하고 렌더러가 자동 배치합니다.
    *   **자유 레이아웃**(박스 배치, 픽셀 단위 위치 지정, 아이콘/도형 혼합, 커스텀 모듈 맵 등)이 필요한 경우 → **SVG로 작성**합니다. `┌──┐` 형태의 ASCII-art 박스 다이어그램은 신규 작성을 지양하고 SVG로 대체합니다.

### 6.2 외부 파일 임베딩 원칙
*   문서 파일이 인라인 다이어그램 코드로 비대해지는 것을 방지하고, 동일 다이어그램을 여러 문서에서 재사용할 수 있도록, **재사용 가능성이 있거나 규모가 큰 다이어그램/SVG는 반드시 별도 파일로 분리하여 저장하고, 문서에서는 참조(임베딩)만 합니다.**
*   참조는 표준 마크다운 이미지 문법을 사용합니다: `![<설명>](<상대경로>/<파일명>.{svg,mermaid})`
*   해당 문서 하나에만 쓰이는 소규모 Mermaid 다이어그램(대략 수십 줄 이내)은 인라인 유지가 가능하나, 여러 문서에서 재사용되거나 대형(100줄 이상)인 다이어그램은 반드시 외부 파일로 분리합니다.

### 6.3 리소스 디렉토리 구조
모든 SVG/다이어그램 리소스는 아래와 같이 도메인별로 모아 관리합니다. **`docs/assets/diagrams/`가 정본 디렉토리입니다** — `docs/resources/`, `docs/layouts/` 등 다른 이름의 병렬 디렉토리를 새로 만들지 않습니다.

```
docs/
  assets/
    diagrams/
      <domain>/              # docs/ 하위 도메인 디렉토리명과 동일 (예: worker-bootstrap, architecture, ui-dashboard, deployment, server-management, llm-wiki)
        <diagram-name>.svg
        <diagram-name>.mermaid   # Mermaid 소스
      shared/                # 여러 도메인이 공유하는 다이어그램 (예: 전체 시스템 개요도)
```

*   `<domain>`은 다이어그램이 속한 `docs/` 하위 도메인 디렉토리명과 동일하게 맞춥니다.
*   파일명은 kebab-case로, 다이어그램의 내용을 설명하는 이름을 사용합니다 (예: `ssh-provisioning-sequence.mermaid`, `fleet-serve-module-map.svg`). 파일명에 도메인명을 접두사로 반복하지 않습니다 — 도메인은 이미 디렉토리로 표현됩니다.

### 6.4 유지보수 규칙
*   SVG를 Mermaid나 다른 도구로 생성한 경우, 재수정이 가능하도록 원본 소스(`.mermaid` 등)를 SVG와 같은 디렉토리에 함께 보관합니다.
*   다이어그램을 갱신할 때는 이를 참조하는 모든 문서를 확인하여 내용이 갈리지 않도록 합니다 (§5.3 Canonical-Derived 정합성 동기화 규칙과 동일한 원칙을 다이어그램에도 적용).
*   동일한 다이어그램이 2개 이상의 문서에서 필요한 경우, 문서마다 복사본을 만들지 않고 하나의 파일을 여러 문서가 함께 참조합니다(예: `worker-bootstrap/fleet-serve-module-map.svg`는 `bootstrap-release-v0.2.md`와 `serve-and-bootstrap-design.md`가 공유).
