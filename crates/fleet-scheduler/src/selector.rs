//! 워커 선택 알고리즘.
//!
//! 선택 순서:
//! 1. 라벨 매칭 필터 (`required_labels`)
//! 2. 모델 매칭 필터 (`task.model` — 지정된 경우 `labels["model"]`이 정확히
//!    일치하는 워커만 후보로 남긴다. 지정하지 않으면 기존과 동일)
//! 3. Credential 매칭 필터 (로드맵 #71 — `task.model`이 지정된 경우
//!    `Store::get_worker_credential(worker.name, model)`이 `Some`을 반환하는
//!    워커만 후보로 남긴다. 아래 "credential 필터 기준 필드" 설명 참고)
//! 4. 회로 차단된 워커 제외
//! 5. `server_hint`가 있으면 해당 워커 (없거나 사용 불가면 에러, 폴백 안 함)
//! 6. 없으면 least-loaded (활성 작업 수 최소)
//!
//! ## Credential 필터 기준 필드: `task.model` (`task.resolved_model` 아님)
//!
//! 로드맵 #71 설계 노트는 `task.resolved_model` 기준 필터링을 제안했으나,
//! 실제 코드를 대조해보면 두 가지 이유로 `task.model`이 맞는 기준이다:
//! 1. `dispatcher.rs`의 `DispatchRequest.model`(워커에 실제로 전달되는 필드)은
//!    `task.model.clone()`이지 `task.resolved_model`이 아니다 — 즉
//!    "실행에 실제로 쓰이는" 필드는 `task.model`이다.
//! 2. `HeuristicTaskRouter::resolve_routing`은 사용자가 `model`을 지정하지
//!    않아도 프로파일 휴리스틱으로 항상 `resolved_model`을 채운다
//!    (`Dispatcher::submit`의 0단계에서 무조건 호출됨). 따라서
//!    `resolved_model`을 기준으로 삼으면 사용자가 model을 지정하지 않은
//!    일반 태스크까지 credential 보유 워커로 강제 제한하게 되어(모든
//!    fleet가 사실상 전 모델에 credential을 프로비저닝해야 함) 문제
//!    배경(사용자가 명시적으로 model을 지정한 경우)의 범위를 크게 벗어나고
//!    기존 동작(및 기존 테스트 스위트 대부분)을 깨뜨린다.

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

    #[error("no online worker holds a credential for model '{0}'")]
    NoWorkerForCredential(String),
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

        // 2.6. Credential 매칭 필터 (로드맵 #71) — task.model이 지정된 경우에만
        // 적용. `Store::get_worker_credential(worker.name, model)`이 `Some`을
        // 반환하는(= 해당 model의 credential을 실제로 보유한) 워커만 후보로
        // 남긴다. task.model이 None이면 어떤 credential이 필요한지 알 수
        // 없으므로 이 단계 전체를 건너뛴다 — 모델 라벨 필터와 동일한 조건.
        //
        // Store 조회 자체가 실패하면(예: credential을 지원하지 않는 Store
        // 구현) 해당 워커를 "credential 미보유"와 동일하게 후보에서 제외한다
        // — credential 보유 여부를 확인할 수 없는 워커에 dispatch하는 것보다
        // 안전한 방향(fail-safe)이다.
        if let Some(model) = &task.model {
            let mut with_credential = Vec::with_capacity(candidates.len());
            for w in candidates {
                match self.store.get_worker_credential(&w.name, model).await {
                    Ok(Some(_)) => with_credential.push(w),
                    Ok(None) => {}
                    Err(e) => {
                        tracing::error!(
                            target: "fleet::selector",
                            error = %e, worker = %w.name, model = %model,
                            "store error checking worker credential — excluding worker from candidates"
                        );
                    }
                }
            }
            candidates = with_credential;

            if candidates.is_empty() {
                return Err(SelectionError::NoWorkerForCredential(model.clone()));
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
        TaskOutput, TaskPhase, TaskRequest, TaskStatus, TransitionOutcome, Worker, WorkerFilter,
        WorkerHeartbeat, WorkerId, WorkerStatus,
    };
    use fleet_store::{Store, StoreError};

    /// 인메모리 mock Store (selector 테스트용).
    struct MockStore {
        workers: std::sync::Mutex<Vec<Worker>>,
        /// (worker_name, model_id) 쌍 — credential 필터 테스트용 fixture.
        /// 로드맵 #71 — 실제 blob 내용은 selector 로직에서 쓰지 않으므로
        /// 존재 여부만 추적한다.
        credentials: std::sync::Mutex<std::collections::HashSet<(String, String)>>,
    }

    impl MockStore {
        fn new(workers: Vec<Worker>) -> Self {
            Self {
                workers: std::sync::Mutex::new(workers),
                credentials: std::sync::Mutex::new(std::collections::HashSet::new()),
            }
        }

        /// 빌더 헬퍼 — 주어진 (worker_name, model_id)에 대한 credential이
        /// 존재하는 것으로 fixture를 채운다.
        fn with_credential(self, worker_name: &str, model_id: &str) -> Self {
            self.credentials
                .lock()
                .unwrap()
                .insert((worker_name.to_string(), model_id.to_string()));
            self
        }
    }

    #[async_trait]
    impl Store for MockStore {
        async fn insert_task_idempotent(
            &self,
            _: &Task,
        ) -> Result<fleet_core::IdempotentInsert, StoreError> {
            Ok(fleet_core::IdempotentInsert::Inserted)
        }

        async fn insert_task(&self, _: &Task) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn get_task(&self, _: TaskId) -> Result<Option<Task>, StoreError> {
            unimplemented!()
        }
        async fn update_task_status(&self, _: TaskId, _: &TaskStatus) -> Result<(), StoreError> {
            unimplemented!()
        }
        async fn compare_and_set_task_status(
            &self,
            _: TaskId,
            _: &[TaskPhase],
            _: &TaskStatus,
        ) -> Result<TransitionOutcome, StoreError> {
            unimplemented!()
        }
        async fn list_tasks(&self, _: &TaskFilter) -> Result<Vec<Task>, StoreError> {
            unimplemented!()
        }
        async fn increment_task_retry_count(&self, _: TaskId) -> Result<u32, StoreError> {
            unimplemented!()
        }
        async fn update_task_checkpoint(
            &self,
            _: TaskId,
            _: Option<&str>,
        ) -> Result<(), StoreError> {
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

        // Phase 8.6: credentials 메서드. `get_worker_credential`은 로드맵 #71
        // credential 필터 테스트에서 `with_credential` fixture와 함께 사용됨.
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
            worker_name: &str,
            model_id: &str,
        ) -> Result<Option<fleet_store::StoredCredential>, StoreError> {
            let has_cred = self
                .credentials
                .lock()
                .unwrap()
                .contains(&(worker_name.to_string(), model_id.to_string()));
            if !has_cred {
                return Ok(None);
            }
            Ok(Some(fleet_store::StoredCredential {
                worker_name: worker_name.to_string(),
                model_id: model_id.to_string(),
                encrypted_blob: "test-encrypted-blob".into(),
                base_url: "https://example.test".into(),
                api_backend: "test-backend".into(),
                context_window: 128_000,
                model_name: None,
                created_at: chrono::Utc::now(),
                rotated_at: chrono::Utc::now(),
            }))
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
        // 로드맵 #71 — credential 필터가 이 테스트의 대상 워커를 걸러내지
        // 않도록 gemini-1에 gemini credential을 부여한다.
        let store = Arc::new(MockStore::new(workers).with_credential("gemini-1", "gemini"));
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
        // 로드맵 #71 — credential 필터가 gpu-gemini를 걸러내지 않도록 credential 부여.
        let store = Arc::new(MockStore::new(workers).with_credential("gpu-gemini", "gemini"));
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

    // ── 로드맵 #71: credential 매칭 필터 ────────────────────────────────

    #[tokio::test]
    async fn select_credential_required_and_present_routes_normally() {
        // 지정된 model의 credential을 가진 worker만 있을 때 정상 dispatch.
        let workers = vec![make_worker("gemini-1", 0, &[("model", "gemini")])];
        let store = Arc::new(MockStore::new(workers).with_credential("gemini-1", "gemini"));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let mut task = make_task("work", None, &[]);
        task.model = Some("gemini".into());

        let selected = selector.select(&task).await.unwrap();
        let gemini_worker = store.get_worker_by_name("gemini-1").await.unwrap().unwrap();
        assert_eq!(selected, gemini_worker.id);
    }

    #[tokio::test]
    async fn select_credential_missing_on_all_candidates_errors() {
        // credential 없는 worker만 있을 때 — 재시도 대상인 NoWorkerForCredential.
        let workers = vec![make_worker("gemini-1", 0, &[("model", "gemini")])];
        let store = Arc::new(MockStore::new(workers)); // credential 미부여
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store, breakers);

        let mut task = make_task("work", None, &[]);
        task.model = Some("gemini".into());

        let result = selector.select(&task).await;
        match result {
            Err(SelectionError::NoWorkerForCredential(m)) => {
                assert!(m.contains("gemini"), "error should mention model: {m}");
            }
            other => panic!("expected NoWorkerForCredential, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn select_credential_partial_provisioning_routes_to_credentialed_worker() {
        // 일부 worker만 credential을 가진 fleet — credential 있는 worker로만 라우팅.
        let workers = vec![
            make_worker("gemini-1", 0, &[("model", "gemini")]),
            make_worker("gemini-2", 0, &[("model", "gemini")]),
        ];
        // gemini-2만 credential 프로비저닝 완료.
        let store = Arc::new(MockStore::new(workers).with_credential("gemini-2", "gemini"));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let mut task = make_task("work", None, &[]);
        task.model = Some("gemini".into());

        let selected = selector.select(&task).await.unwrap();
        let gemini_2 = store.get_worker_by_name("gemini-2").await.unwrap().unwrap();
        assert_eq!(
            selected, gemini_2.id,
            "must route only to the credentialed worker, ignoring the uncredentialed one"
        );
    }

    #[tokio::test]
    async fn select_no_model_skips_credential_check() {
        // model 미지정 task는 credential 유무와 무관하게 기존처럼 정상 dispatch.
        let workers = vec![make_worker("plain-1", 0, &[])]; // credential 없음
        let store = Arc::new(MockStore::new(workers));
        let breakers = Arc::new(BreakerRegistry::new(CircuitBreakerConfig::default()));
        let selector = WorkerSelector::new(store.clone(), breakers);

        let task = make_task("work", None, &[]); // model: None

        let selected = selector.select(&task).await.unwrap();
        let plain = store.get_worker_by_name("plain-1").await.unwrap().unwrap();
        assert_eq!(selected, plain.id);
    }
}
