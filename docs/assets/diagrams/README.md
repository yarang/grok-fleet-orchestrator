# 다이어그램/SVG 리소스 디렉토리

`docs/` 전역에서 재사용되거나 규모가 큰 다이어그램·SVG 파일을 모아두는 곳입니다. 규약 정본은
[문서 관리 정책](../../governance/documentation-policy.md#5-링크와-시각-자료)을 참조하세요.

## 구조

```
docs/assets/diagrams/
  <domain>/              # docs/ 하위 도메인 디렉토리명과 동일
    <diagram-name>.svg   # 자유 레이아웃 다이어그램 (박스 배치, 모듈 맵 등)
    <diagram-name>.mermaid   # Mermaid 소스 (시퀀스/플로우차트/상태 다이어그램)
  shared/                # 여러 도메인이 공유하는 다이어그램 (예: 전체 시스템 개요도)
```

## 현재 도메인 목록

| 도메인 | 대응 문서 디렉토리 | 리소스 수 |
|---|---|---|
| `architecture/` | `docs/architecture/` | 1 (`.svg`) + 28 (`.mermaid`) |
| `deployment/` | `docs/deployment/` | 5 (`.mermaid`) |
| `ui-dashboard/` | `docs/ui-dashboard/` | 1 (`.svg`) + 9 (`.mermaid`) |

## 규칙 요약

- 문서 하나에서만 쓰이는 소규모(수십 줄 이내) Mermaid 다이어그램은 여기로 옮기지 않고 해당 문서에 인라인으로 유지해도 됩니다.
- 그 외 SVG, 대형(100줄+) Mermaid, 재사용되는 다이어그램은 모두 여기에 저장하고 문서에서는 `![설명](../assets/diagrams/<domain>/<파일명>.{svg,mermaid})` 형태로 참조만 합니다.
- 파일명은 kebab-case로 작성하고 내용을 설명하는 이름을 사용합니다.
- Mermaid 원본(`.mermaid`)에서 SVG를 별도로 export한 경우, 재수정을 위해 `.mermaid`와 `.svg`를 같은 디렉토리에 함께 보관합니다.
- 다이어그램을 갱신하면 이를 참조하는 모든 문서를 함께 확인합니다.
