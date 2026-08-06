# Grok Fleet Orchestrator - 설계 Wiki (Design Wiki)

Grok Fleet Orchestrator 프로젝트의 설계 사상, 배포 가이드라인, 워커 가입 인증 메커니즘 및 자가 치유 아키텍처 문서를 한눈에 찾아볼 수 있는 위키 인덱스 페이지입니다.

---

## 📂 카테고리별 설계 문서 인덱스

### 1. ⚙️ 개발 및 협업 가이드라인 (Developer Guidelines)
프로젝트 기여와 로드맵 관리를 위한 규약 문서입니다.
*   **[에이전트 협업 가이드 (agent.md)](../agent.md)**: AI 에이전트 및 기여자가 준수해야 할 Git 커밋 컨벤션, 브랜칭 정책, 테스트 baseline 규정.
*   **[개발 로드맵 (roadmap.md)](./roadmap.md)**: 우선순위별 배포 차단(P0) 및 보안(P1) 리스크 해소 상태를 관리하는 로드맵 원천 문서.
*   **[개발 로드맵 충돌 분석 보고서 (roadmap_conflict_analysis.md)](./roadmap_conflict_analysis.md)**: 신규 추가 요구사항(방화벽, 자가 치유 등)과 기존 로드맵 백로그 간의 의존성 및 충돌 리스크 분석서.

### 2. 🏛️ 시스템 아키텍처 및 배포 (System Architecture & Deployment)
오케스트레이터와 인프라 게이트웨이의 물리/논리 설계서입니다.
*   **[제안된 서버 아키텍처 (proposed_server_architecture.md)](./proposed_server_architecture.md)**: 오케스트레이터-대시보드-워커 간의 관계를 도식화한 시스템 물리 망 구성도.
*   **[단일 서버 배포 계획 (single_server_deployment_plan.md)](./single_server_deployment_plan.md)**: 단일 Linux 서버 환경에서 서비스들을 Docker Compose 기반으로 배포하기 위한 물리 계획서.
*   **[배포 가이드 (deployment.md)](./deployment.md)**: Nginx 권장 리버스 프록시 하드닝 템플릿, systemd 서비스 등록 및 실서버 배포 구축 가이드.
*   **[Nginx 게이트웨이 전환 제안서 (nginx_transition_proposal.md)](./nginx_transition_proposal.md)**: Caddy를 고성능 Nginx 프록시로 전환한 당위성 비교 및 다중화 로드밸런싱 설정 명세.

### 3. 🔑 워커 부트스트랩 및 가입 인증 (Worker Bootstrap & Join Security)
새로운 GPU 워커가 오케스트레이터 클러스터에 안전하게 조인하기 위한 보안 설계서입니다.
*   **[워커 부트스트랩 및 운영 절차 (fleet_serve_dashboard_and_worker_bootstrap_design.md)](./fleet_serve_dashboard_and_worker_bootstrap_design.md)**: 최초 기동 시 워커 등록 플로우 다이어그램 및 기능 정의.
*   **[워커 조인 인증 및 인가 설계 (worker_join_authentication_design.md)](./worker_join_authentication_design.md)**: 1회용 신뢰 토큰 기반 가입 및 비대칭 키(WSS / mTLS) 보안 핸드셰이크 체계.
*   **[부트스트랩 토큰 배포 방식 비교 (bootstrap_token_delivery_methods.md)](./bootstrap_token_delivery_methods.md)**: 토큰 전달 방안(CLI 자동 복사, 웹 수동, 파일 공유 등)의 장단점 분석.
*   **[SSH 프로비저닝 명세서 (ssh_provisioning_implementation_spec.md)](./ssh_provisioning_implementation_spec.md)**: 오케스트레이터가 워커 노드에 패키지를 자동 배포하고 WSS 에이전트를 가동시키는 프로비저닝 상세 프로토콜.

### 4. 🛠️ 서버 관리 및 자가 치유 (Server Management & Healing)
워커 노드의 제어 및 모니터링 데몬의 기능 요구 정의서입니다.
*   **[리눅스 패키지 관리 설계 (linux_package_management_design.md)](./linux_package_management_design.md)**: 워커 데몬이 UFW 방화벽 및 시스템 패키지를 검사/설치하기 위한 로직 아키텍처.
*   **[고급 서버 관리 기능 제안서 (advanced_server_management_proposals.md)](./advanced_server_management_proposals.md)**: UFW 방화벽 동적 제어, GPU 드라이버 헬스체크 및 SSH Key 로테이션 요구사항 제안.
*   **[클라우드 & 물리 서버 자가 치유 설계 (cloud_and_baremetal_hardware_healing_design.md)](./cloud_and_baremetal_hardware_healing_design.md)**: 가상화 클라우드(AWS, GCP 등)의 GPU Throttling 간접 감지 및 환경별(VM 재시작 vs 베어메탈 쿨링 파워 제한) 차별화 서킷 브레이커 자가 복구 가이드.
