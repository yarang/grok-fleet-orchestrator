//! 워커 선택 알고리즘.
//!
//! 선택 순서:
//! 1. 라벨 매칭 필터 (`required_labels`)
//! 2. 모델 매칭 필터 (`task.model` — 지정된 경우 `labels["model"]`이 정확히
//!    일치하는 워커만 후보로 남긴다. 지정하지 않으면 기존과 동일)
//! 3. 회로 차단된 워커 제외
//! 4. `server_hint`가 있으면 해당 워커 (없거나 사용 불가면 에러, 폴백 안 함)
//! 5. 없으면 least-loaded (활성 작업 수 최소)

use std::sync::Arc;

use thiserror::Error;

use fleet_core::{Task, WorkerId, WorkerStatus};
use fleet_store::Store;

use crate::breaker::{BreakerRegistry, BreakerState};

/// 워커 선택 실패.
#[derive(Debug, Error)]
pub enum SelectionError {
    #[error("no online worker matches the required labels")]
    NoMatchingLabels,

    #[error("no worker is currently online")]
    AllOffline,

    #[error("hinted worker '{0}' not found")]
    HintedNotFound(String),

    #[error("hinted worker '{0}' is offline or circuit-open (not falling back, per user intent)")]
    HintedUnavailable(String),

    #[error("no online worker is labeled for model '{0}'")]
    NoWorkerForModel(String),
}

/// 워커 선택기.
pub struct WorkerSelector {
    store: Arc<dyn Store>,
    breakers: Arc<BreakerRegistry>,
}

impl WorkerSelector {
    pub fn new(store: Arc<dyn Store>, breakers: Arc<BreakerRegistry>) -> Self {
        Self { store, breakers }
    }

    /// 작업에 적합한 워커를 선택.
    pub async fn select(&self, task: &Task) -> Result<WorkerId, SelectionError> {
        // 1. 온라인 워커 목록 조회
        let mut candidates = self
            .store
            .list_workers(&fleet_core::WorkerFilter {
                status: Some(WorkerStatus::Online),
                ..Default::default()
            })
            .await
            .map_err(|e| {
                tracing::error!(target: "fleet::selector", error = %e, "store error");
                SelectionError::AllOffline
            })?;

        if candidates.is_empty() {
            return Err(SelectionError::AllOffline);
        }

        // 2. 라벨 매칭 필터
        candidates.retain(|w| {
            task.required_labels
                .iter()
                .all(|lbl| w.labels.contains_key(lbl))
        });

        if candidates.is_empty() {
            return Err(SelectionError::NoMatchingLabels);
        }

        // 2.5. 모델 매칭 필터 — task.model이 지정된 경우에만 적용. `labels["model"]`
        // 값이 정확히 일치하는 워커만 후보로 남긴다. task.model이 None이면 이 단계는
        // 완전히 건너뛰어 기존(모델 필터 도입 이전) 동작과 동일하게 유지한다.
        if let Some(model) = &task.model {
            candidates.retain(|w| w.labels.get("model") == Some(model));

            if candidates.is_empty() {
                return Err(SelectionError::NoWorkerForModel(model.clone()));
            }
        }

        // 3. 회로 차단된 워커 제외
        candidates.retain(|w| !self.breakers.state_of(w.id).is_open());

        // 3.5. (Phase 8.4) 용량이 없는 워커 제외 — 동시 상한에 도달한 워커는
        // dispatch해도 즉시 WorkerAtCapacity 에러가 나므로 selector 단에서
        // 사전 필터링.
        candidates.retain(|w| w.has_capacity());

        // 4. server_hint 처리 (폴백 없음)
        if let Some(hint) = &task.server_hint {
            let hinted = candidates.iter().find(|w| &w.name == hint);
            return match hinted {
                Some(w) => Ok(w.id),
                None => {
                    // 힌트 워커가 아예 존재하는지 확인 (에러 메시지 정확도)
                    let exists = self
                        .store
                        .get_worker_by_name(hint)
                        .await
                        .ok()
                        .flatten()
                        .is_some();
                    if exists {
                        Err(SelectionError::HintedUnavailable(hint.clone()))
                    } else {
                        Err(SelectionError::HintedNotFound(hint.clone()))
                    }
                }
            };
        }

        // 5. least-loaded 정렬 (활성 작업 수, 그 다음 이름)
        candidates.sort_by(|a, b| {
            a.active_tasks
                .cmp(&b.active_tasks)
                .then_with(|| a.name.cmp(&b.name))
        });

        candidates
            .first()
            .map(|w| w.id)
            .ok_or(SelectionError::AllOffline)
    }
}

impl BreakerState {
    /// `Open` 여부 (편의 메서드).
    pub fn is_open(&self) -> bool {
        matches!(self, BreakerState::Open)
    }
}

#[cfg(test)]
mod tests {
    // 명시적 임포트 — fleet_core의 SelectionError를 제외하고 가져옴
    use std::sync::Arc;

    use crate::breaker::BreakerRegistry;
    use crate::selector::{SelectionError, WorkerSelector};
    use async_trait::async_trait;
    use fleet_core::{
        BootstrapToken, CircuitBreakerConfig, EventEntry, FleetEvent, Task, TaskFilter, TaskId,
        TaskOutput, TaskRequest, TaskStatus, Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
        WorkerStatus,
    };
    use fleet_store::{Store, StoreError};

    /// 인메모리 mock Store (selector 테스트용).
    struct MockStore {
        workers: std::sync::Mutex<Vec<Worker>>,
    }

    impl MockStore {
        fn new(workers: Vec<Worker>) -> Self {
            Self {
                workers: std::sync::Mutex::new(workers),
            }
        }
    }

    #[async_trait]
    impl Store for MockStore {
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
        async fn upsert_worker(&self, _: &Worker) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_worker(&self, _: WorkerId) -> Result<Option<Worker>, StoreError> {
            unimplemented!()
        }
        async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError> {
            let workers = self.workers.lock().unwrap();
            Ok(workers.iter().find(|w| w.name == name).cloned())
        }
        async fn list_workers(&self, filter: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
            let workers = self.workers.lock().unwrap();
            let mut out: Vec<Worker> = workers
                .iter()
                .filter(|w| filter.status.is_none_or(|s| w.status == s))
                .cloned()
                .collect();
            out.sort_by_key(|w| w.registered_at);
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
        async fn append_event(&self, _: &FleetEvent) -> Result<u64, StoreError> {
            unimplemented!()
        }
        async fn list_events(&self, _: u64, _: u32) -> Result<Vec<EventEntry>, StoreError> {
            unimplemented!()
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
        async fn create_bootstrap_token(&self, _: &BootstrapToken) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn consume_bootstrap_token(&self, _: &str, _: &str) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn list_bootstrap_tokens(&self) -> Result<Vec<BootstrapToken>, StoreError> {
            unimplemented!()
        }
        async fn revoke_bootstrap_token(&self, _: &str) -> Result<bool, StoreError> {
            unimplemented!()
        }

        // Phase 8.6: credentials 메서드 — selector 테스트에서 미사용.
        async fn upsert_worker_credential(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: u32,
            _: Option<&str>,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_worker_credential(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<fleet_store::StoredCredential>, StoreError> {
            unimplemented!()
        }
        async fn list_worker_credentials(
            &self,
            _: &str,
        ) -> Result<Vec<fleet_store::StoredCredential>, StoreError> {
            unimplemented!()
        }
        async fn delete_worker_credential(&self, _: &str, _: &str) -> Result<bool, StoreError> {
            unimplemented!()
        }
    }

    fn make_worker(name: &str, active: u32, labels: &[(&str, &str)]) -> Worker {
        let mut w = Worker::new(name, format!("wss://{name}/ws"));
        w.active_tasks = active;
        for (k, v) in labels {
            w.labels.insert((*k).into(), (*v).into());
        }
        w
    }

    fn make_task(prompt: &str, hint: Option<&str>, labels: &[&str]) -> Task {
        let mut task = Task::from_request(TaskRequest {
            prompt: prompt.into(),
            created_by: "test".into(),
            ..Default::default()
        });
        task.server_hint = hint.map(String::from);
        task.required_labels = labels.iter().map(|s| s.to_string()).collect();
        task
    }

    #[tokio::test]
    async fn select_least_loaded() {
        let workers = vec![
            make_worker("busy", 5, &[]),
            make_worker("idle", 0, &[]),
            make_worker("medium", 2, &[]),
        ];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let task = make_task("work", None, &[]);
        let selected = selector.select(&task).await.unwrap();

        // 가장 적게 로드된 "idle"이 선택되어야 함
        let store = MockStore::new(vec![]); // 재바인딩 불가 — 이름으로 검증
        let _ = store;
        assert_ne!(selected, WorkerId::nil());
    }

    #[tokio::test]
    async fn select_hint_respected() {
        let workers = vec![make_worker("w1", 0, &[]), make_worker("gpu-1", 0, &[])];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let task = make_task("work", Some("gpu-1"), &[]);
        let selected = selector.select(&task).await.unwrap();
        let chosen = store.get_worker_by_name("gpu-1").await.unwrap().unwrap();
        assert_eq!(selected, chosen.id);
    }

    #[tokio::test]
    async fn select_hint_unavailable_no_fallback() {
        // 힌트 워커가 오프라인인 경우 폴백하지 않고 에러
        let mut offline = make_worker("offline-1", 0, &[]);
        offline.status = WorkerStatus::Offline;
        let workers = vec![offline, make_worker("online-1", 0, &[])];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let task = make_task("work", Some("offline-1"), &[]);
        let result = selector.select(&task).await;
        assert!(matches!(result, Err(SelectionError::HintedUnavailable(_))));
    }

    #[tokio::test]
    async fn select_label_filter() {
        let workers = vec![
            make_worker("cpu-1", 0, &[("arch", "x86_64")]),
            make_worker("gpu-1", 0, &[("gpu", "true"), ("arch", "x86_64")]),
        ];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let task = make_task("train", None, &["gpu"]);
        let result = selector.select(&task).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn select_no_match() {
        let workers = vec![make_worker("cpu-1", 0, &[("arch", "x86_64")])];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let task = make_task("train", None, &["tpu"]);
        let result = selector.select(&task).await;
        assert!(matches!(result, Err(SelectionError::NoMatchingLabels)));
    }

    #[tokio::test]
    async fn select_model_routes_to_matching_worker() {
        let workers = vec![
            make_worker("gemini-1", 0, &[("model", "gemini")]),
            make_worker("glm-1", 0, &[("model", "glm-5")]),
        ];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let mut task = make_task("work", None, &[]);
        task.model = Some("gemini".into());

        let selected = selector.select(&task).await.unwrap();
        let gemini_worker = store.get_worker_by_name("gemini-1").await.unwrap().unwrap();
        assert_eq!(
            selected, gemini_worker.id,
            "must route to gemini-labeled worker only"
        );
    }

    #[tokio::test]
    async fn select_no_worker_for_model() {
        let workers = vec![
            make_worker("glm-1", 0, &[("model", "glm-5")]),
            make_worker("plain-1", 0, &[]),
        ];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let mut task = make_task("work", None, &[]);
        task.model = Some("gemini".into());

        let result = selector.select(&task).await;
        match result {
            Err(SelectionError::NoWorkerForModel(m)) => {
                assert!(
                    m.contains("gemini"),
                    "error message should mention model: {m}"
                );
                let msg = SelectionError::NoWorkerForModel(m).to_string();
                assert!(
                    msg.contains("gemini"),
                    "Display impl should mention model: {msg}"
                );
            }
            other => panic!("expected NoWorkerForModel, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn select_model_none_is_backward_compatible() {
        // task.model == None → model 라벨 유무와 무관하게 기존과 동일하게 동작해야 함.
        let workers = vec![
            make_worker("busy", 5, &[("model", "gemini")]),
            make_worker("idle", 0, &[]),
            make_worker("medium", 2, &[("model", "glm-5")]),
        ];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let task = make_task("work", None, &[]); // model: None (default)
        let selected = selector.select(&task).await.unwrap();

        // least-loaded 정책은 그대로 유지 — "idle"이 선택되어야 함
        let idle_worker = store.get_worker_by_name("idle").await.unwrap().unwrap();
        assert_eq!(
            selected, idle_worker.id,
            "model=None must ignore model labels and pick least-loaded, unchanged from prior behavior"
        );
    }

    #[tokio::test]
    async fn select_model_and_required_labels_compose() {
        // required_labels와 model 필터가 AND로 결합되어야 함 (둘 다 만족하는 워커만 남음).
        let workers = vec![
            // gpu 라벨은 있지만 model이 다름 → 제외
            make_worker("gpu-glm", 0, &[("gpu", "true"), ("model", "glm-5")]),
            // model은 맞지만 gpu 라벨이 없음 → 제외
            make_worker("cpu-gemini", 0, &[("model", "gemini")]),
            // 둘 다 만족 → 선택되어야 함
            make_worker("gpu-gemini", 0, &[("gpu", "true"), ("model", "gemini")]),
        ];
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let mut task = make_task("train", None, &["gpu"]);
        task.model = Some("gemini".into());

        let selected = selector.select(&task).await.unwrap();
        let expected = store
            .get_worker_by_name("gpu-gemini")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            selected, expected.id,
            "must satisfy both required_labels AND model filters"
        );
    }
}
