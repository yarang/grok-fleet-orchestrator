# 배포 & 인프라 (Deployment & Infra)

> 전체 문서 카탈로그는 [`../index.md`](../index.md). 운영 규칙(정본/사본)은
> [`../llm-wiki/README.md`](../llm-wiki/README.md) 스키마를 따른다.

| 문서 | 상태 | 역할 |
|---|---|---|
| [`deployment.md`](./deployment.md) | 🟢 정본 | 설치~로컬 개발~프로덕션~분산 배포~모니터링 전 과정 가이드. §2.3이 Nginx 리버스 프록시의 정본 |
| [`server-topology.md`](./server-topology.md) | 🟢 정본(토폴로지) | 오케스트레이터-대시보드-워커 물리/논리 망 구성도 |
| [`nginx-gateway.md`](./nginx-gateway.md) | 🟢 정본(결정 기록) | Caddy→Nginx 전환 결정서, nginx.conf 상세 스펙 |
| [`single-server.md`](./single-server.md) | 🔵 사본 | 단일 VM Docker Compose 가이드. 리버스 프록시는 `nginx-gateway.md`, liteLLM 스펙은 `../llm-wiki/`를 인용 |
| [`historical/`](./historical/) | ⚪ 역사적 기록 | 특정 시점 배포 일지. 현재 지침으로 사용하지 않음 |

## 정합화 이력

`single-server.md`는 두 차례 사본 동기화 누락이 있었다(liteLLM/One API — 2026-08-06/07 수정,
Caddy/Nginx — 2026-08-11 수정). 이 문서를 고칠 때는 **먼저 `deployment.md`나
`nginx-gateway.md`(정본)를 고친 뒤 사본을 동기화**한다. 상세는 [`../log.md`](../log.md) 참고.
