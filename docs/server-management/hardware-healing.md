# 클라우드 및 온프레미스 혼합 환경의 하드웨어 자가 치유 및 서킷 브레이커 설계서
# (Hardware Self-Healing & Circuit Breaker Design for Cloud & Bare-Metal)

> 작성일: 2026-08-06. 담당: Antigravity.
> 
> 클라우드 가상 서버 환경(AWS, GCP, RunPod 등)에서는 하이퍼바이저 레이어의 제한으로 직접적인 GPU 물리 온도 제어 및 팬 속도 조절이 불가능합니다. 본 문서는 이러한 환경적 한계를 극복하고, 클라우드 가상 노드와 온프레미스 베어메탈 노드를 모두 포용하는 **범용적 자가 치유 및 서킷 브레이커 연동 전략**을 정의합니다.

---

## 1. 아키텍처 핵심 요약

NVIDIA GPU 환경에서 직접적인 온도 조절이 불가능한 클라우드 가상 서버를 위해, **물리 온도 센서 의존성을 낮추고 하드웨어 클럭 스로틀링(Throttling) 및 작업 지연(Stall)을 간접 감지**하는 모델을 채택합니다.

![하드웨어 자가 치유 아키텍처 다이어그램](../assets/diagrams/server-management/hardware-healing.mmd)

---

## 2. 세부 설계 사항 (Detailed Design)

### 2.1 하드웨어 메트릭 추상화 및 폴백 로직
워커 데몬은 온도를 수집할 수 없는 환경(VM 또는 권한 미달)에서도 안전하게 동작하도록 하드웨어 메트릭 수집부를 다형화(Polymorphism)하고 폴백을 적용합니다.

* **베어메탈 (물리 노드)**: NVML 혹은 `/sys/class/hwmon`을 통해 직접적인 GPU 온도 수집.
* **가상화 VM (클라우드 노드)**: 온도를 얻어오지 못할 경우 에러로 크래시되지 않고, `temperature: null`로 기록하며 `virtualized: true` 플래그를 오케스트레이터에 송신.

### 2.2 클라우드 환경에서의 간접 장애 감지 (Indirect Diagnostics)
물리 온도를 측정할 수 없더라도, GPU의 과열 및 성능 저하 상태는 다음 두 가지 지표로 100% 식별할 수 있습니다.

1. **NVML Clock Throttling 감지**:
   * NVML API의 `nvmlDeviceGetCurrentClocksThrottleReasons`를 호출하여 클럭이 다운된 원인을 역추적합니다.
   * `nvmlClocksThrottleReasonThermalSlowdown` (열화 슬로우다운) 또는 `nvmlClocksThrottleReasonHwSlowdown` (하드웨어 서멀 스로틀링) 플래그가 참(True)이면 물리 온도 센서 정보가 없어도 **하드웨어가 과열로 인해 스스로 클럭을 깎아내리고 있는 위험 상태**임을 감지할 수 있습니다.
2. **작업 실행 속도 편차 (Execution Drift) & 하트비트 스톨(Stall)**:
   * 워커가 작업을 수주받아 처리할 때, 동일 모델 대비 연산 처리 속도가 특정 임계값(예: 30%) 이상 느려지거나 하트비트 주기가 늘어지면(Stall) 하드웨어 과열로 인한 시스템 다운 직전 상태로 판정합니다.

---

## 3. 환경별 차별화된 자가 치유 액션 매트릭스

장애가 감지되어 서킷 브레이커가 열려(Open) 해당 워커로의 작업 할당이 차단되었을 때, 워커 및 인프라가 취할 자가 치유 조치입니다.

| 구분 | 온프레미스 베어메탈 노드 | 클라우드 가상 노드 (VM) |
|---|---|---|
| **1차 방어 (소프트)** | * GPU 전력 제한(Power Limit) 강제 인하<br>* GPU 팬 속도 최대 가동 (NVML 제어) | * 실행 중인 LLM / 에이전트 추론 프로세스 즉시 강제 종료 (쿨다운 유도)<br>* VM 내 Docker 데몬 / NVIDIA 드라이버 재기동 |
| **2차 방어 (하드)** | * 물리 서버 호스트 리부트 (`systemctl reboot`) | * **인프라 통합 자동화**: 오케스트레이터가 AWS EC2 / GCP API를 호출하여 VM 인스턴스 소프트 리부트 수행 |
| **최종 복구 (회복)** | * 온도가 임계값 이하로 복구되면 서킷 브레이커를 Half-Open으로 전이 후 검증 작업 배치 | * 상태가 복구되지 않으면 오토스케일링 그룹(ASG)에 노드 교체(Terminate & Replace) 트리거 요청 |

---

## 4. 서킷 브레이커 분산화(#25)와의 연동 스펙

다중 오케스트레이터 배포 환경에서 클라우드 노드가 자가 치유 상태로 돌입해 서킷 브레이커가 트립되면, 모든 오케스트레이터 인스턴스가 이 상태를 공유해야 합니다.

1. **상태 레지스트리 영속화**:
   * 기존 인메모리 서킷 브레이커 상태를 Postgres DB의 `worker_status` 테이블에 `degraded` 혹은 `circuit_open` 상태로 갱신하여 영속화합니다.
2. **이벤트 전파**:
   * 워커가 자가 치유를 트리거하면 오케스트레이터 API에 `POST /v1/workers/{id}/circuit-breaker` 이벤트를 송신합니다.
   * 이 이벤트를 수신한 오케스트레이터는 DB를 업데이트하고, 타 오케스트레이터 인스턴스들로 SSE(Server-Sent Events) 또는 DB Listen/Notify를 통해 해당 노드에 더 이상 작업을 배포하지 않도록 실시간 동기화합니다.
