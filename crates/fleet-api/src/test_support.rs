//! 테스트 전용 인메모리 Store. 모든 fleet-api 단위 테스트가 공유.
//!
//! `cfg(test)`이므로 릴리즈 바이너리에 포함되지 않음.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fleet_core::{
    EventEntry, FleetEvent, Task, TaskFilter, TaskId, TaskOutput, TaskStatus, Worker, WorkerFilter,
    WorkerHeartbeat, WorkerId,
};
use fleet_store::{Store, StoreError};

/// 테스트용 인메모리 Store. 모든 메서드가 실제로 동작하며 데이터를 보관.
pub struct MemStore {
    workers: Mutex<HashMap<WorkerId, Worker>>,
    tasks: Mutex<HashMap<TaskId, Task>>,
    events: Mutex<Vec<EventEntry>>,
    outputs: Mutex<HashMap<TaskId, Vec<String>>>,
}

impl MemStore {
    pub fn new() -> Self {
        Self {
            workers: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            events: Mutex::new(Vec::new()),
            outputs: Mutex::new(HashMap::new()),
        }
    }

    /// `Arc<dyn Store>`으로 바로 래핑.
    pub fn new_arc() -> Arc<dyn Store> {
        Arc::new(Self::new())
    }
}

impl Default for MemStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Store for MemStore {
    async fn insert_task(&self, t: &Task) -> Result<(), StoreError> {
        self.tasks.lock().unwrap().insert(t.id, t.clone());
        Ok(())
    }

    async fn get_task(&self, id: TaskId) -> Result<Option<Task>, StoreError> {
        Ok(self.tasks.lock().unwrap().get(&id).cloned())
    }

    async fn update_task_status(&self, id: TaskId, status: &TaskStatus) -> Result<(), StoreError> {
        if let Some(t) = self.tasks.lock().unwrap().get_mut(&id) {
            t.status = status.clone();
        }
        Ok(())
    }

    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        let mut all: Vec<Task> = self.tasks.lock().unwrap().values().cloned().collect();
        if let Some(status) = &filter.status {
            use fleet_core::TaskStatusFilter;
            // 위상 매칭 — filter.status가 위상 enum.
            all.retain(|t| match (status, &t.status) {
                (TaskStatusFilter::Pending, TaskStatus::Pending) => true,
                (TaskStatusFilter::Dispatched, TaskStatus::Dispatched { .. }) => true,
                (TaskStatusFilter::Completed, TaskStatus::Completed(_)) => true,
                (TaskStatusFilter::Failed, TaskStatus::Failed(_)) => true,
                (TaskStatusFilter::Cancelled, TaskStatus::Cancelled { .. }) => true,
                (TaskStatusFilter::Terminal, terminal) => matches!(
                    terminal,
                    TaskStatus::Completed(_)
                        | TaskStatus::Failed(_)
                        | TaskStatus::Cancelled { .. }
                ),
                (TaskStatusFilter::Active, active) => matches!(
                    active,
                    TaskStatus::Pending | TaskStatus::Dispatched { .. }
                ),
                _ => false,
            });
        }
        all.sort_by_key(|t| t.created_at);
        all.truncate(filter.limit);
        Ok(all)
    }

    async fn upsert_worker(&self, w: &Worker) -> Result<(), StoreError> {
        self.workers.lock().unwrap().insert(w.id, w.clone());
        Ok(())
    }

    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
        Ok(self.workers.lock().unwrap().get(&id).cloned())
    }

    async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError> {
        Ok(self
            .workers
            .lock()
            .unwrap()
            .values()
            .find(|w| w.name == name)
            .cloned())
    }

    async fn list_workers(&self, f: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
        let mut all: Vec<Worker> = self.workers.lock().unwrap().values().cloned().collect();
        if let Some(status) = f.status {
            all.retain(|w| w.status == status);
        }
        all.retain(|w| {
            f.labels
                .iter()
                .all(|(k, v)| w.labels.get(k) == Some(v))
        });
        all.sort_by_key(|w| w.registered_at);
        all.truncate(f.limit);
        Ok(all)
    }

    async fn delete_worker(&self, id: WorkerId) -> Result<(), StoreError> {
        self.workers.lock().unwrap().remove(&id);
        Ok(())
    }

    async fn update_worker_heartbeat(
        &self,
        id: WorkerId,
        hb: &WorkerHeartbeat,
    ) -> Result<(), StoreError> {
        if let Some(w) = self.workers.lock().unwrap().get_mut(&id) {
            w.active_tasks = hb.active_tasks;
            w.last_seen = Some(chrono::Utc::now());
        }
        Ok(())
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

    async fn list_events(&self, after_seq: u64, limit: u32) -> Result<Vec<EventEntry>, StoreError> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.seq > after_seq)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn append_output(&self, task_id: TaskId, chunk: &str) -> Result<u64, StoreError> {
        let mut outputs = self.outputs.lock().unwrap();
        let entry = outputs.entry(task_id).or_default();
        entry.push(chunk.to_string());
        Ok(entry.len() as u64)
    }

    async fn get_output(&self, task_id: TaskId, from_offset: u64) -> Result<TaskOutput, StoreError> {
        let outputs = self.outputs.lock().unwrap();
        let chunks: Vec<_> = outputs
            .get(&task_id)
            .map(|v| {
                v.iter()
                    .skip(from_offset as usize)
                    .enumerate()
                    .map(|(i, chunk)| fleet_core::TaskOutputChunk {
                        task_id,
                        seq: from_offset + i as u64,
                        chunk: chunk.clone(),
                        written_at: chrono::Utc::now(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let next_offset = from_offset + chunks.len() as u64;
        Ok(TaskOutput {
            task_id,
            chunks,
            next_offset,
        })
    }

    async fn migrate(&self) -> Result<(), StoreError> {
        Ok(())
    }
}
