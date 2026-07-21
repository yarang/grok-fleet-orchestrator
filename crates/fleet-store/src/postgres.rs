//! PostgreSQL `Store` 구현.
//!
//! ## DB ↔ 도메인 매핑
//!
//! | DB 칼럼 | 타입 | 도메인 타입 |
//! |---------|------|------------|
//! | `status` (tasks) | JSONB | `TaskStatus` (serde) |
//! | `status_phase` | TEXT (generated) | — (필터링용) |
//! | `priority` | TEXT | `TaskPriority` (snake_case) |
//! | `required_labels` | JSONB | `Vec<String>` |
//! | `labels` (workers) | JSONB | `HashMap<String,String>` |
//! | `status` (workers) | TEXT | `WorkerStatus` (snake_case) |
//! | `circuit_state` | TEXT | `CircuitState` (snake_case) |
//! | `payload` (events) | JSONB | `FleetEvent` (serde) |

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;
use uuid::Uuid;

use fleet_core::{
    BootstrapToken, CircuitState, EventEntry, FleetEvent, Labels, LoginAttempt, Permission, Role,
    Session, SessionId, Task, TaskFilter, TaskId, TaskOutput, TaskOutputChunk, TaskPriority,
    TaskStatus, TaskStatusFilter, User, UserId, Worker, WorkerFilter, WorkerHeartbeat, WorkerId,
    WorkerStatus,
};

use crate::error::StoreError;
use crate::Store;

/// PostgreSQL 기반 `Store` 구현.
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    /// 연결 풀을 생성하고 반환.
    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    /// 기존 풀로부터 생성 (테스트/공유용).
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 내부 풀 참조 (LISTEN/NOTIFY 등 저수준 접근용).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Store trait 구현
// ═══════════════════════════════════════════════════════════════════════

#[async_trait]
impl Store for PgStore {
    // ── Task ───────────────────────────────────────────────────────────

    async fn insert_task(&self, task: &Task) -> Result<(), StoreError> {
        let priority_str = priority_to_str(task.priority);
        let status_json = serde_json::to_value(&task.status)?;
        let labels_json = serde_json::to_value(&task.required_labels)?;

        sqlx::query(
            r#"
            INSERT INTO tasks
                (id, prompt, cwd, model, server_hint, required_labels,
                 max_turns, timeout_secs, created_at, created_by, priority, status)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(task.id.as_uuid())
        .bind(&task.prompt)
        .bind(task.cwd.as_ref())
        .bind(task.model.as_ref())
        .bind(task.server_hint.as_ref())
        .bind(labels_json)
        .bind(task.max_turns.map(|v| v as i32))
        .bind(task.timeout_secs.map(|v| v as i64))
        .bind(task.created_at)
        .bind(&task.created_by)
        .bind(priority_str)
        .bind(status_json)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_task(&self, id: TaskId) -> Result<Option<Task>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, prompt, cwd, model, server_hint, required_labels,
                      max_turns, timeout_secs, created_at, created_by, priority, status
               FROM tasks WHERE id = $1"#,
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_task).transpose()
    }

    async fn update_task_status(&self, id: TaskId, status: &TaskStatus) -> Result<(), StoreError> {
        let status_json = serde_json::to_value(status)?;
        let result = sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(status_json)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        // 단순 필터는 SQL로, 복잡한 것(worker_id)은 Rust로 후처리.
        let limit = filter.limit.min(1000) as i64;

        let rows = if let Some(ref created_by) = filter.created_by {
            sqlx::query(
                r#"SELECT id, prompt, cwd, model, server_hint, required_labels,
                          max_turns, timeout_secs, created_at, created_by, priority, status
                   FROM tasks WHERE created_by = $1
                   ORDER BY created_at DESC LIMIT $2"#,
            )
            .bind(created_by)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"SELECT id, prompt, cwd, model, server_hint, required_labels,
                          max_turns, timeout_secs, created_at, created_by, priority, status
                   FROM tasks
                   ORDER BY created_at DESC LIMIT $1"#,
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };

        let mut tasks: Vec<Task> = rows
            .into_iter()
            .map(row_to_task)
            .collect::<Result<_, _>>()?;

        // 상태 위상 필터 (SQL의 status_phase 대응, 단 Terminal/Active 합성도 처리)
        if let Some(status_filter) = filter.status {
            tasks.retain(|t| status_matches(&t.status, status_filter));
        }

        // 워커 ID 필터 (status JSONB 내부 필드 — 앱 레벨 처리)
        if let Some(worker_id) = filter.worker_id {
            tasks.retain(|t| task_worker_id(&t.status) == Some(worker_id));
        }

        Ok(tasks)
    }

    // ── Worker ─────────────────────────────────────────────────────────

    async fn upsert_worker(&self, worker: &Worker) -> Result<(), StoreError> {
        let labels_json = serde_json::to_value(&worker.labels)?;
        let status_str = worker_status_to_str(worker.status);
        let circuit_str = circuit_state_to_str(worker.circuit_state);

        sqlx::query(
            r#"
            INSERT INTO workers
                (id, name, endpoint, labels, status, circuit_state,
                 last_seen, active_tasks, max_concurrent, worker_version, registered_at)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (id) DO UPDATE SET
                name            = EXCLUDED.name,
                endpoint        = EXCLUDED.endpoint,
                labels          = EXCLUDED.labels,
                status          = EXCLUDED.status,
                circuit_state   = EXCLUDED.circuit_state,
                last_seen       = EXCLUDED.last_seen,
                active_tasks    = EXCLUDED.active_tasks,
                max_concurrent  = EXCLUDED.max_concurrent,
                worker_version  = EXCLUDED.worker_version
            "#,
        )
        .bind(worker.id.as_uuid())
        .bind(&worker.name)
        .bind(&worker.endpoint)
        .bind(labels_json)
        .bind(status_str)
        .bind(circuit_str)
        .bind(worker.last_seen)
        .bind(worker.active_tasks as i32)
        .bind(worker.max_concurrent as i32)
        .bind(worker.worker_version.as_ref())
        .bind(worker.registered_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, name, endpoint, labels, status, circuit_state,
                      last_seen, active_tasks, max_concurrent, worker_version, registered_at
               FROM workers WHERE id = $1"#,
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_worker).transpose()
    }

    async fn get_worker_by_name(&self, name: &str) -> Result<Option<Worker>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, name, endpoint, labels, status, circuit_state,
                      last_seen, active_tasks, max_concurrent, worker_version, registered_at
               FROM workers WHERE name = $1"#,
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_worker).transpose()
    }

    async fn list_workers(&self, filter: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
        let limit = filter.limit.min(1000) as i64;

        let rows = if let Some(status) = filter.status {
            let status_str = worker_status_to_str(status);
            sqlx::query(
                r#"SELECT id, name, endpoint, labels, status, circuit_state,
                          last_seen, active_tasks, max_concurrent, worker_version, registered_at
                   FROM workers WHERE status = $1
                   ORDER BY registered_at DESC LIMIT $2"#,
            )
            .bind(status_str)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"SELECT id, name, endpoint, labels, status, circuit_state,
                          last_seen, active_tasks, max_concurrent, worker_version, registered_at
                   FROM workers
                   ORDER BY registered_at DESC LIMIT $1"#,
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };

        let mut workers: Vec<Worker> = rows
            .into_iter()
            .map(row_to_worker)
            .collect::<Result<_, _>>()?;

        // 라벨 필터 (GIN 인덱스 활용 가능하지만, 단순 containment로 처리)
        if !filter.labels.is_empty() {
            workers.retain(|w| {
                filter
                    .labels
                    .iter()
                    .all(|(k, v)| w.labels.get(k).is_some_and(|val| val == v))
            });
        }

        Ok(workers)
    }

    async fn delete_worker(&self, id: WorkerId) -> Result<(), StoreError> {
        let result = sqlx::query("DELETE FROM workers WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn update_worker_heartbeat(
        &self,
        id: WorkerId,
        heartbeat: &WorkerHeartbeat,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            r#"UPDATE workers SET
                 last_seen    = NOW(),
                 active_tasks = $2
               WHERE id = $1"#,
        )
        .bind(id.as_uuid())
        .bind(heartbeat.active_tasks as i32)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    // ── Event log ──────────────────────────────────────────────────────

    async fn append_event(&self, event: &FleetEvent) -> Result<u64, StoreError> {
        let event_type = event.event_type();
        let payload = serde_json::to_value(event)?;
        let task_id = event.task_id().map(|t| t.as_uuid());
        let worker_id = event.worker_id().map(|w| w.as_uuid());

        let row = sqlx::query(
            r#"INSERT INTO events (task_id, worker_id, event_type, payload)
               VALUES ($1, $2, $3, $4)
               RETURNING seq"#,
        )
        .bind(task_id)
        .bind(worker_id)
        .bind(event_type)
        .bind(payload)
        .fetch_one(&self.pool)
        .await?;

        let seq: i64 = row.try_get("seq")?;
        Ok(seq as u64)
    }

    async fn list_events(&self, after_seq: u64, limit: u32) -> Result<Vec<EventEntry>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT seq, payload FROM events
               WHERE seq > $1 ORDER BY seq ASC LIMIT $2"#,
        )
        .bind(after_seq as i64)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let seq: i64 = row.try_get("seq")?;
                let payload: serde_json::Value = row.try_get("payload")?;
                let event: FleetEvent = serde_json::from_value(payload)?;
                Ok(EventEntry {
                    seq: seq as u64,
                    event,
                })
            })
            .collect()
    }

    // ── Output buffer ──────────────────────────────────────────────────

    async fn append_output(&self, task_id: TaskId, chunk: &str) -> Result<u64, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO task_outputs (task_id, chunk)
               VALUES ($1, $2) RETURNING seq"#,
        )
        .bind(task_id.as_uuid())
        .bind(chunk)
        .fetch_one(&self.pool)
        .await?;

        let seq: i64 = row.try_get("seq")?;
        Ok(seq as u64)
    }

    async fn get_output(&self, task_id: TaskId, after_seq: u64) -> Result<TaskOutput, StoreError> {
        let rows = sqlx::query(
            r#"SELECT seq, chunk, written_at FROM task_outputs
               WHERE task_id = $1 AND seq > $2
               ORDER BY seq ASC"#,
        )
        .bind(task_id.as_uuid())
        .bind(after_seq as i64)
        .fetch_all(&self.pool)
        .await?;

        let chunks: Vec<TaskOutputChunk> = rows
            .into_iter()
            .map(|row| {
                let seq: i64 = row.try_get("seq")?;
                let chunk: String = row.try_get("chunk")?;
                let written_at = row.try_get("written_at")?;
                Ok(TaskOutputChunk {
                    task_id,
                    seq: seq as u64,
                    chunk,
                    written_at,
                })
            })
            .collect::<Result<_, StoreError>>()?;

        let next_offset = chunks.last().map(|c| c.seq).unwrap_or(after_seq);

        Ok(TaskOutput {
            task_id,
            chunks,
            next_offset,
        })
    }

    // ── Migration ──────────────────────────────────────────────────────

    async fn migrate(&self) -> Result<(), StoreError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| StoreError::Migration(e.to_string()))?;
        Ok(())
    }

    // ── Bootstrap tokens (Phase 8.3) ───────────────────────────────────

    async fn create_bootstrap_token(&self, token: &BootstrapToken) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO bootstrap_tokens
                (token, created_at, created_by, expires_at, max_uses, use_count, notes,
                 last_used_by, last_used_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(&token.token)
        .bind(token.created_at)
        .bind(&token.created_by)
        .bind(token.expires_at)
        .bind(token.max_uses as i32)
        .bind(token.use_count as i32)
        .bind(&token.notes)
        .bind(&token.last_used_by)
        .bind(token.last_used_at)
        .execute(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict(format!("bootstrap token already exists: {}", db.message()))
            }
            other => StoreError::Sqlx(other),
        })?;
        Ok(())
    }

    async fn consume_bootstrap_token(&self, token: &str, used_by: &str) -> Result<(), StoreError> {
        // 단일 UPDATE로 atomic하게 검사 + 증가.
        // 조건: token 일치 + use_count < max_uses + (expires_at IS NULL OR > NOW()).
        let now = Utc::now();
        let result = sqlx::query(
            r#"
            UPDATE bootstrap_tokens
               SET use_count = use_count + 1,
                   last_used_by = $2,
                   last_used_at = $3
             WHERE token = $1
               AND use_count < max_uses
               AND (expires_at IS NULL OR expires_at > $3)
            RETURNING token
            "#,
        )
        .bind(token)
        .bind(used_by)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        if result.is_some() {
            Ok(())
        } else {
            // 토큰이 존재하는지 확인하여 적절한 에러 메시지 구성.
            let exists: Option<(String,)> =
                sqlx::query_as("SELECT token FROM bootstrap_tokens WHERE token = $1")
                    .bind(token)
                    .fetch_optional(&self.pool)
                    .await?;
            let reason = match exists {
                Some(_) => "token is exhausted or expired",
                None => "token not found",
            };
            Err(StoreError::BootstrapTokenInvalid(format!(
                "{reason}: {token}"
            )))
        }
    }

    async fn list_bootstrap_tokens(&self) -> Result<Vec<BootstrapToken>, StoreError> {
        let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(
            r#"
            SELECT token, created_at, created_by, expires_at, max_uses, use_count,
                   notes, last_used_by, last_used_at
              FROM bootstrap_tokens
             ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_bootstrap_token).collect()
    }

    async fn revoke_bootstrap_token(&self, token: &str) -> Result<bool, StoreError> {
        let result = sqlx::query("DELETE FROM bootstrap_tokens WHERE token = $1")
            .bind(token)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ── RBAC: Users ───────────────────────────────────────────────────

    async fn create_user(&self, user: &User) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO users (id, username, email, password_hash, enabled, created_at, last_login_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(user.id.as_uuid())
        .bind(&user.username)
        .bind(&user.email)
        .bind(&user.password_hash)
        .bind(user.enabled)
        .bind(user.created_at)
        .bind(user.last_login_at)
        .execute(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict(format!("username already exists: {}", db.message()))
            }
            other => StoreError::Sqlx(other),
        })?;
        Ok(())
    }

    async fn get_user_by_id(&self, id: UserId) -> Result<Option<User>, StoreError> {
        let row: Option<(
            Uuid,
            String,
            Option<String>,
            String,
            bool,
            chrono::DateTime<Utc>,
            Option<chrono::DateTime<Utc>>,
        )> = sqlx::query_as(
            r#"
                SELECT id, username, email, password_hash, enabled, created_at, last_login_at
                  FROM users WHERE id = $1
                "#,
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| User {
            id: UserId::from(r.0),
            username: r.1,
            email: r.2,
            password_hash: r.3,
            enabled: r.4,
            created_at: r.5,
            last_login_at: r.6,
        }))
    }

    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>, StoreError> {
        let row: Option<(
            Uuid,
            String,
            Option<String>,
            String,
            bool,
            chrono::DateTime<Utc>,
            Option<chrono::DateTime<Utc>>,
        )> = sqlx::query_as(
            r#"
                SELECT id, username, email, password_hash, enabled, created_at, last_login_at
                  FROM users WHERE username = $1
                "#,
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| User {
            id: UserId::from(r.0),
            username: r.1,
            email: r.2,
            password_hash: r.3,
            enabled: r.4,
            created_at: r.5,
            last_login_at: r.6,
        }))
    }

    async fn list_users(&self) -> Result<Vec<User>, StoreError> {
        let rows: Vec<(
            Uuid,
            String,
            Option<String>,
            String,
            bool,
            chrono::DateTime<Utc>,
            Option<chrono::DateTime<Utc>>,
        )> = sqlx::query_as(
            r#"
                SELECT id, username, email, password_hash, enabled, created_at, last_login_at
                  FROM users ORDER BY created_at ASC
                "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| User {
                id: UserId::from(r.0),
                username: r.1,
                email: r.2,
                password_hash: r.3,
                enabled: r.4,
                created_at: r.5,
                last_login_at: r.6,
            })
            .collect())
    }

    async fn count_users(&self) -> Result<u64, StoreError> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?;
        Ok(count as u64)
    }

    async fn update_user_password(&self, id: UserId, hash: &str) -> Result<(), StoreError> {
        sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_user_last_login(
        &self,
        id: UserId,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE users SET last_login_at = $2 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_user_enabled(&self, id: UserId, enabled: bool) -> Result<(), StoreError> {
        sqlx::query("UPDATE users SET enabled = $2 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(enabled)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn delete_user(&self, id: UserId) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── RBAC: Roles & Permissions ─────────────────────────────────────

    async fn create_role(&self, role: &Role) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO roles (id, name, description, builtin, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(role.id.as_uuid())
        .bind(&role.name)
        .bind(&role.description)
        .bind(role.builtin)
        .bind(role.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                // idempotent for builtin roles — 재시도 시 무시
                StoreError::Conflict(format!("role already exists: {}", db.message()))
            }
            other => StoreError::Sqlx(other),
        })?;
        Ok(())
    }

    async fn get_role_by_name(&self, name: &str) -> Result<Option<Role>, StoreError> {
        let row: Option<(Uuid, String, Option<String>, bool, chrono::DateTime<Utc>)> =
            sqlx::query_as(
                "SELECT id, name, description, builtin, created_at FROM roles WHERE name = $1",
            )
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| Role {
            id: fleet_core::RoleId::from(r.0),
            name: r.1,
            description: r.2,
            builtin: r.3,
            created_at: r.4,
        }))
    }

    async fn list_roles(&self) -> Result<Vec<Role>, StoreError> {
        let rows: Vec<(Uuid, String, Option<String>, bool, chrono::DateTime<Utc>)> =
            sqlx::query_as(
                "SELECT id, name, description, builtin, created_at FROM roles ORDER BY name",
            )
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| Role {
                id: fleet_core::RoleId::from(r.0),
                name: r.1,
                description: r.2,
                builtin: r.3,
                created_at: r.4,
            })
            .collect())
    }

    async fn create_permission(&self, perm: &Permission) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO permissions (id, name, description)
            VALUES ($1, $2, $3)
            ON CONFLICT (name) DO NOTHING
            "#,
        )
        .bind(perm.id.as_uuid())
        .bind(&perm.name)
        .bind(&perm.description)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_permission_by_name(&self, name: &str) -> Result<Option<Permission>, StoreError> {
        let row: Option<(Uuid, String, Option<String>)> =
            sqlx::query_as("SELECT id, name, description FROM permissions WHERE name = $1")
                .bind(name)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| Permission {
            id: fleet_core::PermissionId::from(r.0),
            name: r.1,
            description: r.2,
        }))
    }

    async fn list_permissions(&self) -> Result<Vec<Permission>, StoreError> {
        let rows: Vec<(Uuid, String, Option<String>)> =
            sqlx::query_as("SELECT id, name, description FROM permissions ORDER BY name")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|r| Permission {
                id: fleet_core::PermissionId::from(r.0),
                name: r.1,
                description: r.2,
            })
            .collect())
    }

    async fn assign_user_role(
        &self,
        user_id: UserId,
        role_id: fleet_core::RoleId,
        granted_by: Option<UserId>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO user_roles (user_id, role_id, granted_at, granted_by)
            VALUES ($1, $2, NOW(), $3)
            ON CONFLICT (user_id, role_id) DO NOTHING
            "#,
        )
        .bind(user_id.as_uuid())
        .bind(role_id.as_uuid())
        .bind(granted_by.map(|u| u.as_uuid()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn revoke_user_role(
        &self,
        user_id: UserId,
        role_id: fleet_core::RoleId,
    ) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM user_roles WHERE user_id = $1 AND role_id = $2")
            .bind(user_id.as_uuid())
            .bind(role_id.as_uuid())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_user_roles(&self, user_id: UserId) -> Result<Vec<Role>, StoreError> {
        let rows: Vec<(Uuid, String, Option<String>, bool, chrono::DateTime<Utc>)> =
            sqlx::query_as(
                r#"
                SELECT r.id, r.name, r.description, r.builtin, r.created_at
                  FROM roles r
                  JOIN user_roles ur ON ur.role_id = r.id
                 WHERE ur.user_id = $1
                 ORDER BY r.name
                "#,
            )
            .bind(user_id.as_uuid())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| Role {
                id: fleet_core::RoleId::from(r.0),
                name: r.1,
                description: r.2,
                builtin: r.3,
                created_at: r.4,
            })
            .collect())
    }

    async fn list_user_permissions(&self, user_id: UserId) -> Result<Vec<Permission>, StoreError> {
        let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
            r#"
            SELECT DISTINCT p.id, p.name, p.description
              FROM permissions p
              JOIN role_permissions rp ON rp.permission_id = p.id
              JOIN user_roles ur ON ur.role_id = rp.role_id
             WHERE ur.user_id = $1
             ORDER BY p.name
            "#,
        )
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| Permission {
                id: fleet_core::PermissionId::from(r.0),
                name: r.1,
                description: r.2,
            })
            .collect())
    }

    async fn grant_role_permission(
        &self,
        role_id: fleet_core::RoleId,
        permission_id: fleet_core::PermissionId,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO role_permissions (role_id, permission_id)
            VALUES ($1, $2)
            ON CONFLICT (role_id, permission_id) DO NOTHING
            "#,
        )
        .bind(role_id.as_uuid())
        .bind(permission_id.as_uuid())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Sessions ──────────────────────────────────────────────────────

    async fn create_session(&self, session: &Session) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO sessions (id, user_id, token_hash, created_at, expires_at, ip_address, user_agent)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(session.id.as_uuid())
        .bind(session.user_id.as_uuid())
        .bind(&session.token_hash)
        .bind(session.created_at)
        .bind(session.expires_at)
        .bind(session.ip_address.as_deref())
        .bind(session.user_agent.as_ref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_session_by_token_hash(&self, hash: &str) -> Result<Option<Session>, StoreError> {
        let row: Option<(
            Uuid,
            Uuid,
            String,
            chrono::DateTime<Utc>,
            chrono::DateTime<Utc>,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            r#"
            SELECT id, user_id, token_hash, created_at, expires_at,
                   host(ip_address)::text AS ip_address, user_agent
              FROM sessions WHERE token_hash = $1
            "#,
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| Session {
            id: SessionId::from(r.0),
            user_id: UserId::from(r.1),
            token_hash: r.2,
            created_at: r.3,
            expires_at: r.4,
            ip_address: r.5,
            user_agent: r.6,
        }))
    }

    async fn delete_session(&self, id: SessionId) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn delete_expired_sessions(&self) -> Result<u64, StoreError> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= NOW()")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    async fn delete_user_sessions(&self, user_id: UserId) -> Result<u64, StoreError> {
        let result = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id.as_uuid())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    // ── Login attempts ────────────────────────────────────────────────

    async fn record_login_attempt(&self, attempt: &LoginAttempt) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO login_attempts (id, identifier, ip_address, success, failure_reason, attempted_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(attempt.id)
        .bind(&attempt.identifier)
        .bind(attempt.ip_address.as_ref())
        .bind(attempt.success)
        .bind(attempt.failure_reason.as_ref())
        .bind(attempt.attempted_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn count_recent_failed_attempts(
        &self,
        identifier: &str,
        ip: Option<&str>,
        window_secs: i64,
    ) -> Result<u64, StoreError> {
        let (count,): (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM login_attempts
             WHERE identifier = $1
               AND (ip_address IS NOT DISTINCT FROM $2)
               AND success = FALSE
               AND attempted_at >= NOW() - make_interval(secs => $3)
            "#,
        )
        .bind(identifier)
        .bind(ip)
        .bind(window_secs as f64)
        .fetch_one(&self.pool)
        .await?;
        Ok(count as u64)
    }

    async fn clear_login_attempts(
        &self,
        identifier: &str,
        ip: Option<&str>,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM login_attempts
             WHERE identifier = $1
               AND (ip_address IS NOT DISTINCT FROM $2)
            "#,
        )
        .bind(identifier)
        .bind(ip)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  행 → 도메인 변환 헬퍼
// ═══════════════════════════════════════════════════════════════════════

fn row_to_task(row: sqlx::postgres::PgRow) -> Result<Task, StoreError> {
    let id: Uuid = row.try_get("id")?;
    let prompt: String = row.try_get("prompt")?;
    let cwd: Option<String> = row.try_get("cwd")?;
    let model: Option<String> = row.try_get("model")?;
    let server_hint: Option<String> = row.try_get("server_hint")?;
    let labels_json: serde_json::Value = row.try_get("required_labels")?;
    let max_turns: Option<i32> = row.try_get("max_turns")?;
    let timeout_secs: Option<i64> = row.try_get("timeout_secs")?;
    let created_at = row.try_get("created_at")?;
    let created_by: String = row.try_get("created_by")?;
    let priority_str: String = row.try_get("priority")?;
    let status_json: serde_json::Value = row.try_get("status")?;

    let required_labels: Vec<String> = serde_json::from_value(labels_json)?;
    let status: TaskStatus = serde_json::from_value(status_json)?;
    let priority = str_to_priority(&priority_str)?;

    Ok(Task {
        id: TaskId::from(id),
        prompt,
        cwd,
        model,
        server_hint,
        required_labels,
        max_turns: max_turns.map(|v| v as u32),
        timeout_secs: timeout_secs.map(|v| v as u64),
        created_at,
        created_by,
        priority,
        status,
    })
}

fn row_to_worker(row: sqlx::postgres::PgRow) -> Result<Worker, StoreError> {
    let id: Uuid = row.try_get("id")?;
    let name: String = row.try_get("name")?;
    let endpoint: String = row.try_get("endpoint")?;
    let labels_json: serde_json::Value = row.try_get("labels")?;
    let status_str: String = row.try_get("status")?;
    let circuit_str: String = row.try_get("circuit_state")?;
    let last_seen = row.try_get("last_seen")?;
    let active_tasks: i32 = row.try_get("active_tasks")?;
    let max_concurrent: i32 = row.try_get("max_concurrent")?;
    let worker_version: Option<String> = row.try_get("worker_version")?;
    let registered_at = row.try_get("registered_at")?;

    let labels: Labels = serde_json::from_value(labels_json).unwrap_or_else(|_| HashMap::new());

    Ok(Worker {
        id: WorkerId::from(id),
        name,
        endpoint,
        labels,
        status: str_to_worker_status(&status_str)?,
        last_seen,
        active_tasks: active_tasks as u32,
        max_concurrent: max_concurrent as u32,
        circuit_state: str_to_circuit_state(&circuit_str)?,
        worker_version,
        registered_at,
    })
}

fn row_to_bootstrap_token(row: sqlx::postgres::PgRow) -> Result<BootstrapToken, StoreError> {
    let token: String = row.try_get("token")?;
    let created_at = row.try_get("created_at")?;
    let created_by: Option<String> = row.try_get("created_by")?;
    let expires_at = row.try_get("expires_at")?;
    let max_uses: i32 = row.try_get("max_uses")?;
    let use_count: i32 = row.try_get("use_count")?;
    let notes: Option<String> = row.try_get("notes")?;
    let last_used_by: Option<String> = row.try_get("last_used_by")?;
    let last_used_at = row.try_get("last_used_at")?;

    Ok(BootstrapToken {
        token,
        created_at,
        created_by,
        expires_at,
        max_uses: max_uses as u32,
        use_count: use_count as u32,
        notes,
        last_used_by,
        last_used_at,
    })
}

/// `TaskStatus`에서 worker_id 추출 (필터링용).
fn task_worker_id(status: &TaskStatus) -> Option<WorkerId> {
    match status {
        TaskStatus::Dispatched { worker_id, .. } => Some(*worker_id),
        TaskStatus::Completed(result) => Some(result.worker_id),
        TaskStatus::Failed(failure) => failure.worker_id,
        _ => None,
    }
}

/// `TaskStatusFilter` 매칭.
fn status_matches(status: &TaskStatus, filter: TaskStatusFilter) -> bool {
    matches!(
        (status, filter),
        (TaskStatus::Pending, TaskStatusFilter::Pending)
            | (TaskStatus::Pending, TaskStatusFilter::Active)
            | (TaskStatus::Dispatched { .. }, TaskStatusFilter::Dispatched)
            | (TaskStatus::Dispatched { .. }, TaskStatusFilter::Active)
            | (TaskStatus::Completed(_), TaskStatusFilter::Completed)
            | (TaskStatus::Completed(_), TaskStatusFilter::Terminal)
            | (TaskStatus::Failed(_), TaskStatusFilter::Failed)
            | (TaskStatus::Failed(_), TaskStatusFilter::Terminal)
            | (TaskStatus::Cancelled { .. }, TaskStatusFilter::Cancelled)
            | (TaskStatus::Cancelled { .. }, TaskStatusFilter::Terminal)
    )
}

// ═══════════════════════════════════════════════════════════════════════
//  Enum ↔ TEXT 변환
// ═══════════════════════════════════════════════════════════════════════

fn priority_to_str(p: TaskPriority) -> &'static str {
    match p {
        TaskPriority::Low => "low",
        TaskPriority::Normal => "normal",
        TaskPriority::High => "high",
    }
}

fn str_to_priority(s: &str) -> Result<TaskPriority, StoreError> {
    match s {
        "low" => Ok(TaskPriority::Low),
        "normal" => Ok(TaskPriority::Normal),
        "high" => Ok(TaskPriority::High),
        other => Err(StoreError::Decode(format!("unknown priority: {other}"))),
    }
}

fn worker_status_to_str(s: WorkerStatus) -> &'static str {
    match s {
        WorkerStatus::Online => "online",
        WorkerStatus::Degraded => "degraded",
        WorkerStatus::Offline => "offline",
        WorkerStatus::CircuitOpen => "circuit_open",
    }
}

fn str_to_worker_status(s: &str) -> Result<WorkerStatus, StoreError> {
    match s {
        "online" => Ok(WorkerStatus::Online),
        "degraded" => Ok(WorkerStatus::Degraded),
        "offline" => Ok(WorkerStatus::Offline),
        "circuit_open" => Ok(WorkerStatus::CircuitOpen),
        other => Err(StoreError::Decode(format!(
            "unknown worker status: {other}"
        ))),
    }
}

fn circuit_state_to_str(c: CircuitState) -> &'static str {
    match c {
        CircuitState::Closed => "closed",
        CircuitState::Open => "open",
        CircuitState::HalfOpen => "half_open",
    }
}

fn str_to_circuit_state(s: &str) -> Result<CircuitState, StoreError> {
    match s {
        "closed" => Ok(CircuitState::Closed),
        "open" => Ok(CircuitState::Open),
        "half_open" => Ok(CircuitState::HalfOpen),
        other => Err(StoreError::Decode(format!(
            "unknown circuit state: {other}"
        ))),
    }
}
