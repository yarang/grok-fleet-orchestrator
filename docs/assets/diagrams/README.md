# 다이어그램/SVG 리소스 디렉토리

`docs/` 전역에서 재사용되거나 규모가 큰 다이어그램·SVG 파일을 모아두는 곳입니다. 규약 전문은 [`/CLAUDE.md`](../../../CLAUDE.md)와 [`/agent.md` §6](../../../agent.md#6-다이어그램-및-svg-리소스-관리-규약-diagram--svg-resource-policy)을 참조하세요.

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
| `architecture/` | `docs/architecture/` | 12 (`.mermaid`) |
| `deployment/` | `docs/deployment/` | 4 (`.mermaid`) |
| `llm-wiki/` | `docs/llm-wiki/` | 2 (`.mermaid`) |
| `server-management/` | `docs/server-management/` | 4 (`.mermaid`) |
| `ui-dashboard/` | `docs/ui-dashboard/` | 7 (`.mermaid`) |
| `worker-bootstrap/` | `docs/worker-bootstrap/` | 4 (`.svg`) + 4 (`.mermaid`) |

## 규칙 요약

- 문서 하나에서만 쓰이는 소규모(수십 줄 이내) Mermaid 다이어그램은 여기로 옮기지 않고 해당 문서에 인라인으로 유지해도 됩니다.
- 그 외 SVG, 대형(100줄+) Mermaid, 재사용되는 다이어그램은 모두 여기에 저장하고 문서에서는 `![설명](../assets/diagrams/<domain>/<파일명>.{svg,mermaid})` 형태로 참조만 합니다.
- 파일명은 kebab-case, 내용을 설명하는 이름을 사용합니다 (예: `ssh-provisioning-sequence.mermaid`, `fleet-serve-module-map.svg`).
- Mermaid 원본(`.mermaid`)에서 SVG를 별도로 export한 경우, 재수정을 위해 `.mermaid`와 `.svg`를 같은 디렉토리에 함께 보관합니다.
- 다이어그램을 갱신하면 이를 참조하는 모든 문서를 함께 확인합니다.
