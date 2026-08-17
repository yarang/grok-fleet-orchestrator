---
type: architecture
authority: canonical
implementation: proposed
verification: design-reviewed
source: "docs/architecture/worker-liveness-policy.md"
last_verified: "2026-08-16"
---

# Worker Liveness와 선택적 Heartbeat 정책

## 결정

정기 heartbeat는 필수가 아니다. Worker별 `liveness_mode`를 명시하고, 기본값은
기존 호환성을 위한 `periodic`으로 둔다. 대규모·저활성 Fleet는 `on_demand`를
선택해 상시 heartbeat를 보내지 않는다.

```mermaid
flowchart TD
    Config["worker.toml: liveness_mode"] --> Mode{Mode}
    Mode -->|periodic| HB["주기적 heartbeat\n상태·자원 텔레메트리"]
    Mode -->|on_demand| Idle["idle 시 무트래픽\n마지막 성공 activity만 기록"]
    HB --> Health["HealthChecker: timeout 시 Offline"]
    Idle --> Dispatch["dispatch 직전 ACP probe / 연결"]
    Dispatch -->|success| Active["activity 갱신 후 실행"]
    Dispatch -->|failure| Unavailable["해당 dispatch 실패·breaker 반영\n다른 Worker 재선택"]
```

## 모드 계약

| 모드 | 설정값 | idle 네트워크 트래픽 | Offline 판단 | 트레이드오프 |
|---|---|---:|---|---|
| `periodic` | 기본값 | `heartbeat_interval_secs`마다 | missed heartbeat threshold | 빠른 장애 감지와 자원 텔레메트리, 규모에 비례한 요청 수 |
| `on_demand` | 명시 opt-in | 없음 | dispatch 직전 ACP probe 실패 또는 실제 command 실패 | idle 부하 없음, 첫 작업의 probe 지연·일시 실패 가능 |

`on_demand` Worker에 일반 HealthChecker의 heartbeat timeout을 적용하면 idle 상태인
정상 Worker를 오프라인으로 잘못 전이시키므로 금지한다. 단순히
`heartbeat_interval_secs = 0`으로 처리하지 않는다. 0은 모호하며 잘못된 타이머·나눗셈
동작을 만들 수 있다.

**Agent control 제약**: 현재 Agent start/stop/capture command는 Worker의 heartbeat
polling으로 전달된다. 따라서 별도 control stream 또는 bounded command poll을 구현하기
전에는 Agent를 호스팅하는 Worker에 `on_demand`를 적용할 수 없다. 이는 heartbeat를
단순히 끄는 기능이 아니라 control plane 전달 방식을 함께 바꾸는 작업이다.

## 설정 및 상태 모델

`[worker]`에 아래 값을 추가한다.

```toml
# 기존 배포와 호환되는 기본값
liveness_mode = "periodic" # "periodic" | "on_demand"
heartbeat_interval_secs = 60 # periodic일 때만 사용
```

등록 요청, `Worker` 도메인 모델, DB에는 `liveness_mode`를 저장한다. Worker가 재등록할
때 이를 갱신하며, API/대시보드는 mode와 `last_activity_at`을 표시한다. `last_seen`은
periodic heartbeat의 마지막 시각이라는 기존 의미를 유지하고, 새로운 `last_activity_at`은
register, 성공한 probe, command ack, task result를 기록한다.

## 스케줄링·보안 불변식

- `on_demand`는 살아 있음의 증명이 아니라 **사전 검증이 필요한 후보**다. probe 성공 전에는 task 실행 성공으로 간주하지 않는다.
- probe와 command는 Worker의 scoped operational identity로 인증한다. 공용 bearer나 bootstrap token을 재사용하지 않는다.
- `periodic`에서만 `worker:heartbeat` capability가 필요하다. 등록·probe·command 결과의 capability는 별도로 분리한다.
- monitoring은 on-demand Worker를 `Unknown/Unchecked`로 표시할 수 있지만, 근거 없이 `Online`으로 표시하지 않는다.
- 고가용성 판단은 지금의 Single Active Primary + Cold Standby 모델에 종속된다. probe session을 두 Orchestrator가 동시에 소유하지 않는다.

## 구현 순서와 완료 기준

1. `WorkerLivenessMode` enum, migration, register/API/OpenAPI/worker.toml을 추가한다.
2. periodic loop를 mode 조건으로 시작하고 HealthChecker가 on-demand Worker를 skip한다.
3. transport에 bounded ACP probe를 추가하고 dispatcher가 on-demand dispatch 전에 호출한다.
4. 상태·이벤트·대시보드에 `last_activity_at`과 probe 결과를 기록한다.
5. 1,000 idle on-demand Worker가 heartbeat 요청 0건을 보내는 테스트와, 죽은 Worker가 probe 실패 뒤 dispatch되지 않는 테스트를 통과한다.
