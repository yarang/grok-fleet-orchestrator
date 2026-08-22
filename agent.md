# 에이전트 협업 가이드 (Agent Collaboration Guide)

> 작성일: 2026-08-06. 최종 개정: 2026-08-17 — 문서 정책 전문을 Governance 정본으로 분리.
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
*   **로드맵 매핑**: [구현 로드맵](docs/roadmap/roadmap.md)에 등록된 작업이라면 커밋 본문이나 제목에 영구 ID(`#N`)를 명시하여 변경 사유를 증적화합니다.
    *   *예시*: `fix: reject incomplete LLM proxy configuration (#53)`
*   **Git Log 보존**: 로컬 Git Log는 메인 브랜치의 신뢰 원천입니다. 변경 전후의 히스토리가 꼬이지 않도록 이전에 정상 작동하던 커밋을 훼손하지 않습니다.

### 1.3 무결성 커밋 단계
1.  **Stage (`git add`)**: 변경된 소스 및 관련 문서 파일만 정밀하게 추가합니다. 개인/로컬 전용 파일(`.claude/settings.local.json`, `.claude/worktrees/` 등)이 스테이징 영역에 포함되지 않도록 마스킹 처리합니다. `.claude/settings.json`, `.claude/hooks/`, 팀 공유 `.claude/agent-memory/` 항목처럼 저장소에 이미 커밋되어 팀 전체가 공유하는 자동화·지식은 임시 파일이 아니므로 이 규칙의 대상이 아닙니다.
2.  **Commit (`git commit`)**: 상세한 변경 내역(변경 부분, 이유, 연동된 로드맵 이슈 번호)을 본문에 포함하여 커밋합니다.

---

## 2. 개발 로드맵 (Development Roadmap)

현재 구현 순서, 영구 ID, 상태와 완료 조건은 [Roadmap 도메인](docs/roadmap/README.md)이
소유합니다. 에이전트는 작업 전 관련 정본 설계와 Roadmap 완료 게이트를 함께 확인하고,
범위가 바뀌면 설계 정본을 먼저 갱신한 뒤 Roadmap의 순서·상태만 동기화합니다.

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

문서는 코드와 동등한 프로젝트 자산이다. 문서를 생성하거나 크게 수정할 때는 다음 정본을 따른다.

- [문서 관리 정책](./docs/governance/documentation-policy.md): 도메인, 정본 관계, 메타데이터와 부기 원칙
- [문서 재작성 가이드](./docs/governance/documentation-rewrite-guide.md): 대규모 재작성·이동·폐기의 절차와 완료 게이트
- [Reviews](./docs/reviews/README.md): 비교·감사·논의 근거의 보존 위치

에이전트는 문서 작업 전에 대상 도메인 진입점에서 책임과 정본을 확인한다. 현재 구현을 언급하면
코드·테스트·설정과 대조하고, 변경 뒤에는 상대 링크·메타데이터·필요한 색인과 로그를 검증한다.
정책 전문이나 다이어그램 규칙을 이 파일에 복사하지 않는다.

단순 오탈자처럼 책임·정본·탐색 구조를 바꾸지 않는 수정에는 재작성 가이드 전체 절차를 요구하지
않는다. Git 커밋은 사용자가 요청했거나 작업 흐름에서 권한이 명시된 경우에만 수행한다.
