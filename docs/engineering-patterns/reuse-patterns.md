# Grok Build 재사용 패턴 — Fleet Orchestrator 적용 가이드

> **출처**: grok-build `xai-grok-shell/src/agent/subagent/` 및 `xai-circuit-breaker` 심층 분석.
> **목적**: `fleet-scheduler` 구현 시 직접 활용할 수 있는 10가지 검증된 패턴 요약.

## 패턴 요약표

| # | 패턴 | 소스 위치 | 적용 크레이트 | 우선순위 |
|---|------|-----------|--------------|---------|
| 1 | RAII PendingGuard | `subagent/mod.rs:1456` | fleet-scheduler | ★★★ |
| 2 | Context Bag | `subagent/mod.rs:139` | fleet-transport | ★★★ |
| 3 | sync_running_gauge | `coordinator_lifecycle.rs:73` | fleet-scheduler | ★★★ |
| 4 | SnapshotLookup 3-way | `coordinator_query.rs:30` | fleet-scheduler | ★★ |
| 5 | Auto-backgrounding | `handle_request.rs:1325` | fleet-mcp (wait_for_task) | ★★ |
| 6 | Per-worker CircuitBreaker | `circuit-breaker/registry.rs:12` | fleet-scheduler | ★★★ |
| 7 | Single Channel Fan-in | `task/types.rs:653` | fleet-scheduler | ★★ |
| 8 | Per-event spawn | `subagent_coordinator.rs:29` | fleet-scheduler | ★★ |
| 9 | Block-wait slot | `coordinator_query.rs:144` | fleet-mcp (wait_for_task) | ★ |
| 10 | Contributor Registry | `agent-lifecycle/send/registry.rs` | fleet-scheduler (확장 훅) | ★ |

---

## 1. RAII PendingGuard (★★★)

**문제**: 작업 할당 실패 시 누락 없이 "failed" 상태로 전환해야 함.
**해결**: `Drop` 트레이트가 `defused` 플래그를 확인.

```rust
struct PendingGuard<'a> {
    tracker: &'a TaskTracker,
    task_id: TaskId,
    defused: bool,
}
impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        if !self.defused {
            self.tracker.mark_failed(self.task_id, "spawn failed");
        }
    }
}
```

**적용**: `fleet-scheduler/src/tracker.rs` — `Dispatcher::run_task`에서 워커 선택/연결 실패를 자동 기록.

---

## 2. Context Bag (★★★)

**문제**: 코디네이터가 거대한 부모 구조체를 알아야 하는 결합도 문제.
**해결**: 스폰에 필요한 모든 데이터를 값으로 패키징.

```rust
pub struct WorkerSpawnContext {
    pub endpoint: String,
    pub oidc_token: SecretString,
    pub labels: Labels,
    pub resource_limits: ResourceLimits,
    pub timeout: Duration,
    // 부모 State를 빌리지 않음 — 값으로 전달
}
```

**적용**: `fleet-transport/src/pool.rs` — `FleetConnectionPool::dispatch(ctx)` 시그니처.

---

## 3. sync_running_gauge (★★★)

**문제**: increment/decrement 방식은 동시성 버그로 drift 발생.
**해결**: 매 상태 변화마다 `pending.len() + active.len()`을 재계산.

```rust
fn sync_running_gauge(&self) {
    self.running_gauge.store(
        self.pending.len() + self.active.len(),
        Ordering::Relaxed,
    );
}
```

**적용**: `fleet-scheduler/src/tracker.rs` — 모든 상태 전이 메서드에서 호출.

---

## 4. SnapshotLookup 3-way (★★)

**문제**: 동기 맵 검색과 비동기 라이브 메트릭 조회를 어떻게 분리할 것인가.
**해결**: 3-way enum으로 조회 결과 분류.

```rust
enum TaskLookup {
    Ready(TaskSnapshot),              // 완료 — 동기 반환
    NeedsWorkerSignals(WorkerSeed),   // 실행 중 — 비동기 메트릭 필요
    Pending,                          // 대기 중
}
```

**적용**: `fleet-mcp/src/handlers/get_status.rs` — `RefCell` borrow를 짧게 유지하면서 라이브 메트릭 병렬 수집.

---

## 5. Auto-backgrounding (★★)

**문제**: MCP `fleet_wait_for_task`가 타임아웃 시 작업을 중단하면 안 됨.
**해결**: 블로킹 예산 초과 시 클라이언트에 "백그라운드로 전환" 알림, 작업은 계속 실행.

```rust
const TASK_AWAIT_BUDGET: Duration = Duration::from_secs(600);

tokio::select! {
    result = task_future => ReturnResult(result),
    _ = sleep(TASK_AWAIT_BUDGET) => ReturnTaskStillRunning(task_id),
}
```

**적용**: `fleet-mcp/src/handlers/wait_for_task.rs` — `fleet_get_task_status`로 폴링 유도.

---

## 6. Per-worker CircuitBreaker (★★★)

**핵심**: `CircuitBreakerRegistry`의 `get(key)` 메서드로 워커별 독립 격리.

```rust
// fleet-scheduler에서 직접 재사용
use xai_circuit_breaker::CircuitBreakerRegistry;

let registry = CircuitBreakerRegistry::new(breaker_config_from_fleet_core());

// 각 워커 dispatch 시:
let cb = registry.get(&worker_id.to_string());
if let Some(cb) = &cb {
    cb.check()?;  // BreakerOpen 에러 시 dispatch 거부
}
// 실행 후:
cb.record(if success { Outcome::Success } else { Outcome::Failure });
```

**Observer 훅**: 상태 변화 시 `WorkerCircuitChanged` 이벤트 발행.

```rust
struct FleetCircuitObserver {
    event_tx: mpsc::Sender<FleetEvent>,
}
impl Observer for FleetCircuitObserver {
    fn on_state_change(&self, old: BreakerState, new: BreakerState, reason: &str) {
        let _ = self.event_tx.try_send(FleetEvent::worker_circuit_changed(...));
    }
}
```

---

## 7. Single Channel Fan-in (★★)

**문제**: 여러 도구(dispatch, cancel, query)가 코디네이터를 동시에 호출.
**해결**: 하나의 enum 채널로 퍼널링.

```rust
enum OrchestrationEvent {
    Submit(Task),
    Cancel { task_id: TaskId, reason: String },
    QueryStatus { task_id: TaskId, reply: oneshot::Sender<TaskStatus> },
    ListWorkers { filter: WorkerFilter, reply: oneshot::Sender<Vec<Worker>> },
    WorkerHeartbeat(WorkerHeartbeat),
}
```

**이점**: `#[non_exhaustive]` 없이 컴파일 타임 완전성 보장.

---

## 8. Per-event spawn (★★)

**문제**: 드레인 루프가 긴 작업으로 블로킹되면 안 됨.
**해결**: 각 이벤트를 별도 `tokio::spawn` 태스크로 분리.

```rust
while let Some(event) = rx.recv().await {
    tokio::spawn(async move {
        match event {
            OrchestrationEvent::Submit(task) => dispatcher.run_task(task).await,
            // ...
        }
    });
}
```

---

## 9. Block-wait slot (★)

**문제**: 완료 대기 시 레이스 컨디션 (취소된 수신자에게 전송 시도).
**해결**: `oneshot::Sender`를 `Rc<RefCell<Option<Sender>>>`로 래핑, `is_closed()`로 검증.

```rust
type WaitSlot = Arc<Mutex<Option<oneshot::Sender<TaskResult>>>>;

fn deliver(slot: &WaitSlot, result: TaskResult) {
    if let Some(tx) = slot.lock().unwrap().take() {
        if !tx.is_closed() {
            let _ = tx.send(result);
        }
    }
}
```

**적용**: `fleet-mcp/src/handlers/wait_for_task.rs`.

---

## 10. Contributor Registry (★)

**문제**: 라이프사이클 훅(헬스체크, 메트릭, 감사 로그)을 확장 가능하게 주입.
**해결**: Builder → Freeze 패턴으로 등록.

```rust
use xai_agent_lifecycle::send::{ExtensionRegistryBuilder, TurnLifecycleContributor};

let mut builder = ExtensionRegistryBuilder::new();
builder.add_turn_lifecycle(Arc::new(HealthCheckContributor::new(...)));
builder.add_turn_lifecycle(Arc::new(MetricsContributor::new(...)));
let registry = builder.build();
```

**적용**: `fleet-scheduler`의 확장 포인트. Phase 3+.

---

## 추가 발견사항

### MAX_COMPLETED_ENTRIES = 1024

grok-build의 `SubagentCoordinator`는 완료된 작업을 1024개까지만 메모리에 보관 (LRU 제거). `fleet-scheduler`의 `TaskTracker`도 동일 값 채택 권장. 디스크 폴백은 PostgreSQL이 담당하므로 메모리 캡은 안전.

### MAX_SUBAGENT_DEPTH = 1

워커가 자체적으로 하위 워커를 스폰하지 못하게 하는 데드락 방지 장치. Fleet 오케스트레이터는 워커 간 중첩 디스패치를 지원하지 않으므로, 이 제한은 자연스럽게 충족됨 (모든 dispatch는 오케스트레이터를 경유).

### SUBAGENT_AWAIT_BUDGET = 600s

`fleet_wait_for_task`의 타임아웃 상한선과 정확히 일치. plan.md 섹션 7.2에서 이미 600초로 설계됨.

### PowerEvent (xai-system-power)

워커 머신이 절전 모드에 진입하면 인플라이트 작업 손실 가능. `fleet-worker-sidecar`에서 `SystemPowerListener`를 사용해 절직 전 오케스트레이터에 알림 → 작업 핸드오프 또는 graceful shutdown 트리거. Phase 3+ 고려사항.
