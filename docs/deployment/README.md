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

**2026-08-12 코드 대조 검증**: 전 문서를 CLI 플래그·환경변수·포트·systemd/nginx 설정
기준으로 실측 대조했다. `server-topology.md`가 (당시엔 동기화되지 않은) liteLLM
Docker Compose 구조를 최신으로 서술 중이던 것을 `single-server.md`와 동일 기준으로
정정했고, `nginx-gateway.md`의 단일 서버 예시에 `/v1/`(오케스트레이터 API, 8081)
라우팅이 통째로 빠져있던 것을 추가했다(방치 시 워커 셀프 서비스 등록이 외부에서
불가능). `deployment.md`의 `install.sh` 기본 설치 경로·`--purge` 플래그 오적용·
`/v1/health` 응답 예시·systemd stdin-EOF 워크어라운드 누락도 함께 정정했다.
