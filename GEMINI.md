# GEMINI.md

Grok Fleet Orchestrator 프로젝트에서 작업하는 Gemini / Google Antigravity (agy) 에이전트를 위한 최상위 지침 진입점입니다.

> 커밋/Git 정책, 개발 로드맵 준수, 보안·CI 품질 게이트, LLM-Wiki 문서 규약 등 **전체 개발 협업 규칙은 [`agent.md`](./agent.md)를 정본으로 따릅니다.**

@agent.md

---

## 문서 작성 및 시각화 지침 (Documentation & Visual Authoring)

이 저장소의 `docs/` 하위 문서를 작성·수정할 때는 아래 원칙을 항상 적용합니다. 상세 규약은 [`agent.md` §6 다이어그램 및 SVG 리소스 관리 규약](./agent.md#6-다이어그램-및-svg-리소스-관리-규약-diagram--svg-resource-policy)을 정본으로 합니다.

1. **다이어그램을 기본으로 작성한다 (Mermaid & SVG 적극 활용)**:
   - 아키텍처, 흐름, 상태 전이, 데이터 모델, 컴포넌트 관계 등 구조를 설명할 때는 텍스트 서술만으로 끝내지 않고 다이어그램을 기본 포함한다.
   - **노드-엣지 구조**(시퀀스/플로우차트/상태 다이어그램)는 **Mermaid 소스(`.mermaid` 또는 ` ```mermaid `)**를 우선 사용한다.
   - **자유 레이아웃**(박스 배치, 모듈 맵, 시스템 레이아웃 등)은 **벡터 SVG로 작성**한다. ASCII-art 박스 다이어그램(`┌──┐` 형태)은 신규로 만들지 않는다.

2. **재사용성과 파일 크기를 위해 외부 파일로 임베딩한다**:
   - 재사용 가능성이 있거나 규모가 큰(100줄 이상) SVG/다이어그램은 문서 본문에 인라인으로 두지 않고 별도 파일로 분리한 뒤 마크다운 이미지 문법(`![설명](경로/파일명.{svg,mermaid})`)으로 참조만 한다.
   - 문서 하나에서만 쓰이는 소규모(수십 줄 이내) Mermaid 다이어그램만 인라인 코드 블록을 허용한다.

3. **리소스는 `docs/assets/diagrams/` 아래 도메인별로 모아 관리한다**:
   ```
   docs/assets/diagrams/<domain>/<diagram-name>.svg       # 자유 레이아웃 다이어그램
   docs/assets/diagrams/<domain>/<diagram-name>.mermaid   # Mermaid 소스
   docs/assets/diagrams/shared/                          # 여러 도메인이 공유하는 다이어그램
   ```
   - `<domain>`은 `docs/` 하위 도메인명(`architecture`, `deployment`, `llm-wiki` 등)과 동일하게 맞춘다.
   - 파일명은 `kebab-case`로 다이어그램 내용을 설명하는 이름을 사용한다.

4. **다이어그램 정합성 동기화**:
   - 다이어그램을 갱신할 때는 이를 참조하는 모든 문서를 함께 확인하여 내용이 어긋나지 않도록 한다.
