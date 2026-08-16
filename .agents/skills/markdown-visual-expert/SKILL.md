---
name: markdown-visual-expert
description: Guidelines for authoring technical Markdown specifications with Mermaid diagrams, vector SVGs, and docs/assets/diagrams/ asset hierarchy.
---

# Markdown & Visual Architecture Expert Skill

이 스킬은 Markdown 기술 문서 작성 시 **Mermaid 다이어그램과 벡터 SVG**를 적극 활용하고, **`docs/assets/diagrams/<domain>/` 자산 관리 규약**을 준수하도록 지침을 제공합니다.

## 1. 다이어그램 및 시각화 작성 원칙

1. **Visual-First 설명**:
   - 아키텍처, 상태 머신, 프로토콜 핸드셰이크, 다중 티어 워크플로우를 서술할 때는 항상 다이어그램을 동반합니다.
2. **도구 선택 기준**:
   - **Mermaid Flowchart / Sequence / State / Class / ER**: 로직 흐름, 시퀀스 상호작용, 상태 전이에 사용.
   - **벡터 SVG**: 고수준 시스템 모듈 맵, 하드웨어/네트워크 토폴로지 레이아웃, UI 목업에 사용. ASCII-art 박스 다이어그램(`┌──┐`)은 지양하고 SVG/Mermaid로 대체합니다.
3. **정확한 코드 그라운딩**:
   - 다이어그램의 노드 라벨, 구조체명, 엔드포인트는 실제 코드베이스 정의와 100% 일치해야 합니다.

## 2. 에셋 디렉토리 관리 규칙 (`docs/assets/diagrams/`)

* **소규모 다이어그램 (< 50줄)**: 문서 본문 내 인라인 ` ```mermaid ` 블록으로 작성 가능.
* **대형 다이어그램 (100줄+) / 재사용 다이어그램 / SVG**:
  - `docs/assets/diagrams/<domain>/` 하위에 별도 파일(`.mermaid`, `.svg`)로 저장.
  - 마크다운 본문에서는 상대경로 링크로 참조:
    ```markdown
    ![설명](../assets/diagrams/<domain>/<diagram-name>.{svg,mermaid})
    ```
* **파일명 규칙**: 소문자 `kebab-case`로 작성 (예: `task-dispatch-lifecycle.mermaid`, `token-budget-flow.svg`).
* **동기화**: 다이어그램 추가/수정 시 `docs/assets/diagrams/README.md` 통계를 갱신하고 참조하는 모든 문서의 정합성을 확인합니다.
