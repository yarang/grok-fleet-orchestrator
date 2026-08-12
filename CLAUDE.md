# CLAUDE.md

Grok Fleet Orchestrator 프로젝트에서 작업하는 Claude Code(및 기타 AI 에이전트)를 위한 최상위 지침 진입점입니다.

> 커밋/Git 정책, 개발 로드맵 준수, 보안·CI 품질 게이트, LLM-Wiki 문서 규약 등 **전체 개발 협업 규칙은 [`agent.md`](./agent.md)를 정본으로 따릅니다.**

@agent.md

---

## 문서 작성 지침 (Documentation Authoring)

이 저장소의 `docs/` 하위 문서를 작성·수정할 때는 아래 원칙을 항상 적용합니다. 상세 규약은 [`agent.md` §6 다이어그램 및 SVG 리소스 관리 규약](./agent.md#6-다이어그램-및-svg-리소스-관리-규약-diagram--svg-resource-policy)을 정본으로 하며, 핵심만 요약하면 다음과 같습니다.

1. **다이어그램을 기본으로 작성한다.** 아키텍처, 흐름, 상태 전이, 데이터 모델, 컴포넌트 관계 등 구조를 설명할 때는 텍스트 서술만으로 끝내지 않고 다이어그램을 기본 포함한다.
   - 노드-엣지 구조(시퀀스/플로우차트/상태 다이어그램)는 Mermaid 코드 블록을 우선 사용한다.
   - 자유 레이아웃(박스 배치, 모듈 맵, 커스텀 아이콘 배치 등)은 **SVG로 작성**한다. ASCII-art 박스 다이어그램(`┌──┐` 형태)은 신규로 만들지 않는다.
2. **재사용성과 파일 크기를 위해 외부 파일로 임베딩한다.** 재사용 가능성이 있거나 규모가 큰 SVG/다이어그램은 문서 본문에 인라인으로 두지 않고 별도 파일로 분리한 뒤 마크다운 이미지 문법(`![설명](경로/파일명.svg)`)으로 참조만 한다. 문서 하나에서만 쓰이는 소규모(수십 줄 이내) Mermaid 다이어그램만 인라인을 허용한다.
3. **리소스는 전용 디렉토리에 도메인별로 모아 관리한다.**
   ```
   docs/assets/diagrams/<domain>/<diagram-name>.svg   # docs/ 하위 도메인명과 동일한 <domain> (예: worker-bootstrap, architecture)
   docs/assets/diagrams/<domain>/<diagram-name>.mmd   # (선택) Mermaid 원본 소스, 재수정용 보관
   docs/assets/diagrams/shared/                        # 여러 도메인이 공유하는 다이어그램
   ```
   파일명은 kebab-case로 다이어그램 내용을 설명하는 이름을 사용한다.
4. 다이어그램을 갱신할 때는 이를 참조하는 모든 문서를 함께 확인하여 내용이 갈리지 않도록 한다(Canonical-Derived 정합성 동기화 원칙, `agent.md` §5.3 참고).
