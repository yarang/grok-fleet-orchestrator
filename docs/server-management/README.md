# 서버 관리 & 자가 치유 (Server Management & Self-Healing)

> 전체 문서 카탈로그는 [`../index.md`](../index.md). **이 도메인의 세 문서는 모두 로드맵
> 제안서이며 아직 구현되지 않았다** — 상태는 [`../roadmap/roadmap.md`](../roadmap/roadmap.md)와
> [`../roadmap/conflict-analysis.md`](../roadmap/conflict-analysis.md)에서 추적한다.

```mermaid
flowchart LR
    subgraph 감지
        M1[UFW/방화벽 드리프트] --> T[중앙 이상 신호 수집]
        M2[GPU 스로틀링/스톨] --> T
        M3[설정파일 드리프트] --> T
        M4[SMART/디스크 수명] --> T
        M5[네트워크 레이턴시 편차] --> T
    end
    T --> J{서킷 브레이커 트립?}
    J -->|Yes| K[worker_status DB 갱신 + 오케스트레이터 동기화]
    J -->|No| L[대시보드 경고만 표시]
    K --> N[환경별 자가치유 액션]
```

| 문서 | 다루는 범위 |
|---|---|
| [`advanced-management-proposals.md`](./advanced-management-proposals.md) | SSH 키 회수, UFW/Fail2ban, 설정 드리프트 감지, 네트워크 지연 진단, SMART 헬스체크 |
| [`linux-package-management.md`](./linux-package-management.md) | APT/DNF 래퍼, PackageKit D-Bus vs sudoers 권한 위임 |
| [`hardware-healing.md`](./hardware-healing.md) | GPU 스로틀/스톨 감지(NVML), 클라우드/베어메탈 차등 자가치유, 서킷브레이커 DB 공유(로드맵 #25) |
