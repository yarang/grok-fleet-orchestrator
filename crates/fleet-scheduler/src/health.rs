//! 워커 헬스체크 루프.
//!
//! 백그라운드 태스크로 주기적으로 모든 워커의 `last_seen`을 검사하여,
//! `missed_heartbeat_threshold` 회 이상 하트비트가 누락된 워커를
//! `Online`/`Degraded` → `Offline`으로 전환합니다.
//!
//! ## 알고리즘
//!
//! 1. `health_check_interval`마다 `store.list_workers(status=Online|Degraded)` 조회
//! 2. 각 워커에 대해 `now - last_seen` 계산
//! 3. `last_seen + grace > now`인 경우 아직 유효 (grace = threshold * interval)
//! 4. 만료된 워커는 `status = Offline`으로 업데이트 + `WorkerLeft` 이벤트 발행
//!
//! ## 설계 노트
//!
//! - grok-build의 `keepalive` 패턴과 유사하지만, 오케스트레이터가 주도적으로 폴링.
//! - 하트비트 수신은 Phase 3의 HTTP API에서 처리 — 여기서는 감시만.
//! - CircuitOpen 상태는 건드리지 않음 (브레이커가 자체적으로 관리).

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use fleet_core::{FleetEvent, WorkerFilter, WorkerStatus};

use crate::state::FleetState;

/// 헬스체크 설정.
#[derive(Debug, Clone)]
pub struct HealthConfig {
    /// 폴링 주기.
    pub check_interval: Duration,
    /// 하트비트 누락 허용 횟수. `last_seen + threshold * check_interval < now`이면 offline.
    pub missed_heartbeat_threshold: u32,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(15),
            missed_heartbeat_threshold: 3,
        }
    }
}

/// 헬스체커. spawn하면 백그라운드 루프를 반환.
pub struct HealthChecker {
    state: Arc<FleetState>,
    config: HealthConfig,
}

/// 백그라운드 헬스체크 루프 핸들. `abort()`로 종료.
pub struct HealthCheckerHandle {
    inner: JoinHandle<()>,
}

impl HealthCheckerHandle {
    /// 백그라운드 루프를 취소하고 종료 대기.
    pub async fn abort(self) {
        self.inner.abort();
        let _ = self.inner.await;
    }
}

impl HealthChecker {
    pub fn new(state: Arc<FleetState>, config: HealthConfig) -> Self {
        Self { state, config }
    }

    /// 백그라운드 루프 시작. 첫 체크는 `check_interval` 후 수행.
    pub fn spawn(self) -> HealthCheckerHandle {
        let handle = tokio::spawn(async move {
            self.run().await;
        });
        HealthCheckerHandle { inner: handle }
    }

    async fn run(&self) {
        let mut interval = tokio::time::interval(self.config.check_interval);
        // 첫 틱은 즉시 발생 — 초기화 직후 상태 한 번 정리.
        interval.tick().await;

        info!(
            interval = ?self.config.check_interval,
            threshold = self.config.missed_heartbeat_threshold,
            "health checker started"
        );

        loop {
            interval.tick().await;
            if let Err(e) = self.scan_once().await {
                warn!(error = %e, "health scan failed");
            }
        }
    }

    /// 단일 스캔 사이클. 테스트에서 직접 호출 가능.
    pub async fn scan_once(&self) -> Result<usize, crate::health::ScanError> {
        let candidates = self
            .state
            .store
            .list_workers(&WorkerFilter {
                status: Some(WorkerStatus::Online),
                ..Default::default()
            })
            .await
            .map_err(|e| ScanError::Store(e.to_string()))?;

        // Degraded 워커도 검사 — 둘 다 last_seen이 유효해야 함.
        let degraded = self
            .state
            .store
            .list_workers(&WorkerFilter {
                status: Some(WorkerStatus::Degraded),
                ..Default::default()
            })
            .await
            .map_err(|e| ScanError::Store(e.to_string()))?;

        let mut all = candidates;
        all.extend(degraded);

        let now = Utc::now();
        let grace = chrono::Duration::from_std(self.config.grace_duration())
            .unwrap_or_else(|_| chrono::Duration::seconds(60));

        let mut expired = 0usize;
        for worker in all {
            let Some(last_seen) = worker.last_seen else {
                // last_seen이 없으면 (등록 직후 예외 케이스) 보수적으로 스킵.
                // Phase 3 HTTP API에서는 등록 시 last_seen = now로 강제.
                debug!(worker = %worker.name, "no last_seen, skipping");
                continue;
            };

            if now - last_seen > grace {
                // 만료 — Offline으로 전이
                let mut updated = worker.clone();
                updated.status = WorkerStatus::Offline;

                if let Err(e) = self.state.store.upsert_worker(&updated).await {
                    warn!(worker = %worker.name, error = %e, "failed to mark worker offline");
                    continue;
                }

                let _ = self
                    .state
                    .store
                    .append_event(&FleetEvent::worker_left(
                        worker.id,
                        format!(
                            "heartbeat missed for more than {:?} (last_seen={})",
                            self.config.grace_duration(),
                            last_seen.to_rfc3339()
                        ),
                    ))
                    .await;

                info!(worker = %worker.name, "marked worker offline due to missed heartbeats");
                expired += 1;
            }
        }

        Ok(expired)
    }
}

impl HealthConfig {
    /// 허용 가능한 최대 하트비트 누락 시간.
    pub fn grace_duration(&self) -> Duration {
        self.check_interval * self.missed_heartbeat_threshold
    }
}

/// 헬스체크 중 발생 가능한 에러.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("store error: {0}")]
    Store(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::FleetState;
    use async_trait::async_trait;
    use fleet_core::{
        CircuitBreakerConfig, EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput,
        TaskStatus, Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
    };
    use fleet_store::{Store, StoreError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// 인메모리 Store — health checker 테스트 전용.
    struct MemStore {
        workers: Mutex<HashMap<WorkerId, Worker>>,
        events: Mutex<Vec<EventEntry>>,
    }

    impl MemStore {
        fn new() -> Self {
            Self {
                workers: Mutex::new(HashMap::new()),
                events: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl Store for MemStore {
        async fn insert_task(&self, _: &Task) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_task(&self, _: TaskId) -> Result<Option<Task>, StoreError> {
            unimplemented!()
        }
        async fn update_task_status(&self, _: TaskId, _: &TaskStatus) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn list_tasks(&self, _: &TaskFilter) -> Result<Vec<Task>, StoreError> {
            unimplemented!()
        }
        async fn upsert_worker(&self, w: &Worker) -> Result<(), StoreError> {
            self.workers.lock().unwrap().insert(w.id, w.clone());
            Ok(())
        }
        async fn get_worker(&self, _: WorkerId) -> Result<Option<Worker>, StoreError> {
            unimplemented!()
        }
        async fn get_worker_by_name(&self, _: &str) -> Result<Option<Worker>, StoreError> {
            unimplemented!()
        }
        async fn list_workers(&self, f: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
            let workers = self.workers.lock().unwrap();
            let out: Vec<Worker> = workers
                .values()
                .filter(|w| f.status.map_or(true, |s| w.status == s))
                .cloned()
                .collect();
            Ok(out)
        }
        async fn delete_worker(&self, _: WorkerId) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn update_worker_heartbeat(
            &self,
            _: WorkerId,
            _: &WorkerHeartbeat,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn append_event(&self, e: &FleetEvent) -> Result<u64, StoreError> {
            let mut events = self.events.lock().unwrap();
            let seq = (events.len() + 1) as u64;
            events.push(EventEntry {
                seq,
                event: e.clone(),
            });
            Ok(seq)
        }
        async fn list_events(&self, _: u64, _: u32) -> Result<Vec<EventEntry>, StoreError> {
            Ok(self.events.lock().unwrap().clone())
        }
        async fn append_output(&self, _: TaskId, _: &str) -> Result<u64, StoreError> {
            unimplemented!()
        }
        async fn get_output(&self, _: TaskId, _: u64) -> Result<TaskOutput, StoreError> {
            unimplemented!()
        }
        async fn migrate(&self) -> Result<(), StoreError> {
            Ok(())
        }
    }

    fn make_state(store: Arc<dyn Store>) -> Arc<FleetState> {
        let transport = fleet_transport::MockTransport::new();
        let transport: Arc<dyn fleet_transport::WorkerTransport> = Arc::new(transport);
        Arc::new(FleetState::new(
            store,
            transport,
            CircuitBreakerConfig::default(),
        ))
    }

    #[tokio::test]
    async fn stale_worker_marked_offline() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        // 매우 오래된 last_seen
        let ancient = chrono::Utc::now() - chrono::Duration::hours(1);
        let mut w = Worker::new("stale-1", "wss://stale/ws");
        w.status = WorkerStatus::Online;
        w.last_seen = Some(ancient);
        store.upsert_worker(&w).await.unwrap();

        let state = make_state(store);
        let checker = HealthChecker::new(
            state.clone(),
            HealthConfig {
                check_interval: Duration::from_secs(1),
                missed_heartbeat_threshold: 3,
            },
        );

        let expired = checker.scan_once().await.unwrap();
        assert_eq!(expired, 1);

        let offline = state
            .store
            .list_workers(&WorkerFilter::default())
            .await
            .unwrap();
        assert_eq!(offline[0].status, WorkerStatus::Offline);

        // WorkerLeft 이벤트 발행 확인
        let events = state.store.list_events(0, 10).await.unwrap();
        assert!(events.iter().any(|e| e.event.event_type() == "worker_left"));
    }

    #[tokio::test]
    async fn fresh_worker_not_marked() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let fresh = chrono::Utc::now();
        let mut w = Worker::new("fresh-1", "wss://fresh/ws");
        w.status = WorkerStatus::Online;
        w.last_seen = Some(fresh);
        store.upsert_worker(&w).await.unwrap();

        let state = make_state(store);
        let checker = HealthChecker::new(
            state.clone(),
            HealthConfig {
                check_interval: Duration::from_secs(1),
                missed_heartbeat_threshold: 3,
            },
        );

        let expired = checker.scan_once().await.unwrap();
        assert_eq!(expired, 0, "fresh worker should not be marked offline");
    }

    #[tokio::test]
    async fn no_last_seen_skipped() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let mut w = Worker::new("mystery-1", "wss://mystery/ws");
        w.status = WorkerStatus::Online;
        w.last_seen = None; // last_seen 없음
        store.upsert_worker(&w).await.unwrap();

        let state = make_state(store.clone());
        let checker = HealthChecker::new(
            state,
            HealthConfig::default(),
        );

        let expired = checker.scan_once().await.unwrap();
        assert_eq!(expired, 0);

        // 상태는 그대로 Online
        let workers = store.list_workers(&WorkerFilter::default()).await.unwrap();
        assert_eq!(workers[0].status, WorkerStatus::Online);
    }

    #[test]
    fn grace_duration_calculation() {
        let cfg = HealthConfig {
            check_interval: Duration::from_secs(10),
            missed_heartbeat_threshold: 3,
        };
        assert_eq!(cfg.grace_duration(), Duration::from_secs(30));
    }
}
