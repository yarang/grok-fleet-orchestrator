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

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;
use uuid::Uuid;

use fleet_core::{
    Agent, AgentFilter, AgentId, AgentStatus, AuditEvent, AuditFilter, AuditOutcome,
    BootstrapToken, CircuitState, CloseReason, EventEntry, FleetEvent, IdempotentInsert, Issue,
    IssueComment, IssueFilter, IssueId, IssueSeverity, IssueStatus, IssueTaskLink, Labels,
    LoginAttempt, Permission, PermissionKind, Project, ProjectFilter, ProjectId, ProjectStatus,
    Role, Session, SessionId, Task, TaskDeleteOutcome, TaskFilter, TaskId, TaskOutput,
    TaskOutputChunk, TaskPhase, TaskPriority, TaskStatus, TaskStatusFilter, TransitionOrigin,
    TransitionOutcome, User, UserId, Worker, WorkerFilter, WorkerHeartbeat, WorkerId, WorkerStatus,
};

use crate::error::StoreError;
use crate::{
    AdminApiToken, ControlFence, ControlLease, Store, StoredCredential, WorkerOperationalCredential,
};

/// `migrations/` 디렉터리를 컴파일 타임에 임베드한 마이그레이터.
///
/// 예전에는 [`Store::migrate`] 구현이 `sqlx::migrate!()`를 그 자리에서
/// 호출했다. [`PgStore::guard_migration_against_live_lease`]가 "이번 기동에서
/// 적용될 버전"을 알아야 하므로, 두 곳이 같은 값을 보도록 하나로 끌어냈다.
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Postgres 커넥션 풀 세부 튜닝 옵션 (로드맵 P2 #16).
///
/// 기존에는 `max_connections`만 설정 가능했고, `acquire_timeout`/`max_lifetime`/
/// `idle_timeout`은 sqlx 기본값을 그대로 썼다 — 장수명 서버 프로세스(`fleet serve`)
/// 에서는 방화벽/로드밸런서의 idle connection kill, DB 재시작 후 stale connection,
/// 풀 고갈 시 무한 대기 같은 문제로 이어질 수 있다.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// 풀의 최대 연결 수.
    pub max_connections: u32,
    /// 연결 획득 타임아웃 — 풀이 고갈된 상태로 이 시간을 넘게 대기하면 에러를
    /// 반환한다 (무한정 요청이 쌓이는 것을 방지).
    pub acquire_timeout: Duration,
    /// 연결의 최대 수명. 이 시간을 넘긴 연결은 반납 시점에 재사용하지 않고
    /// 닫는다 — 로드밸런서/방화벽의 장기 커넥션 강제 종료, 커넥션 레벨 메모리
    /// 누수를 예방. `None`이면 수명 제한 없음(sqlx 기본 동작).
    pub max_lifetime: Option<Duration>,
    /// 유휴 연결 타임아웃. 이 시간 이상 미사용 연결은 `min_connections`까지
    /// 줄이며 닫는다 — 트래픽이 적을 때 불필요하게 열린 커넥션을 DB 측에
    /// 남기지 않는다. `None`이면 유휴 타임아웃 없음(sqlx 기본 동작).
    pub idle_timeout: Option<Duration>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            // sqlx 기본값(30s)과 동일 — 명시적으로 고정해 향후 sqlx 기본값이
            // 바뀌어도 우리 동작은 변하지 않도록 함.
            acquire_timeout: Duration::from_secs(30),
            max_lifetime: Some(Duration::from_secs(30 * 60)),
            idle_timeout: Some(Duration::from_secs(10 * 60)),
        }
    }
}

/// `users` 테이블 원시 컬럼 튜플 (`USER_COLUMNS` 순서와 일치).
/// clippy::type_complexity 회피 + 4곳의 중복 인라인 타입 제거용 별칭.
type UserRow = (
    Uuid,
    String,
    Option<String>,
    bool,
    String,
    bool,
    chrono::DateTime<Utc>,
    Option<chrono::DateTime<Utc>>,
);

/// PostgreSQL 기반 `Store` 구현.
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    /// 연결 풀을 생성하고 반환. `max_connections`만 지정하고 나머지 풀 옵션은
    /// [`PoolConfig::default`]를 사용 — 짧게 실행되고 종료되는 CLI 하위
    /// 명령(예: `fleet tasks list`)처럼 풀 튜닝이 중요하지 않은 경로용.
    /// 장수명 서버 프로세스는 [`PgStore::connect_with_config`]를 사용할 것.
    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self, StoreError> {
        Self::connect_with_config(
            database_url,
            PoolConfig {
                max_connections,
                ..PoolConfig::default()
            },
        )
        .await
    }

    /// 커넥션 풀 세부 옵션(`acquire_timeout`/`max_lifetime`/`idle_timeout`)까지
    /// 지정해 연결. 장수명 서버 프로세스(`fleet serve`)가 사용해야 하는 경로.
    pub async fn connect_with_config(
        database_url: &str,
        config: PoolConfig,
    ) -> Result<Self, StoreError> {
        let mut opts = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(config.acquire_timeout);
        if let Some(max_lifetime) = config.max_lifetime {
            opts = opts.max_lifetime(max_lifetime);
        }
        if let Some(idle_timeout) = config.idle_timeout {
            opts = opts.idle_timeout(idle_timeout);
        }
        let pool = opts.connect(database_url).await?;
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

    /// 릴레이션 존재 여부. `to_regclass`는 없는 이름에 대해 에러 대신 NULL을
    /// 돌려주므로, 존재 확인에 실패 경로를 따로 다룰 필요가 없다.
    async fn relation_exists(&self, name: &str) -> Result<bool, StoreError> {
        let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(name)
            .fetch_one(&self.pool)
            .await?;
        Ok(exists)
    }

    /// 이 바이너리가 들고 있으나 DB에는 아직 적용되지 않은 마이그레이션 버전.
    ///
    /// `_sqlx_migrations`가 없으면(마이그레이션을 한 번도 돌린 적 없는 DB)
    /// `None`을 돌려준다. "전부 pending"과 "원장을 읽을 수 없음"은 다르고,
    /// 후자에서는 `control_plane_lease` 테이블도 존재할 수 없으므로 가드가
    /// 할 일이 없다.
    async fn pending_migration_versions(&self) -> Result<Option<Vec<i64>>, StoreError> {
        if !self.relation_exists("_sqlx_migrations").await? {
            return Ok(None);
        }
        let applied: HashSet<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .collect();
        let mut pending: Vec<i64> = MIGRATOR
            .iter()
            .filter(|m| !m.migration_type.is_down_migration())
            .map(|m| m.version)
            .filter(|version| !applied.contains(version))
            .collect();
        pending.sort_unstable();
        Ok(Some(pending))
    }

    /// 아직 만료되지 않은 control plane lease의 (cluster_id, instance_id,
    /// 남은 초).
    ///
    /// 정상 종료는 `release_control_lease`가 `expires_at = NOW()`로 만들어
    /// 즉시 사라지게 한다 — 따라서 여기 잡히는 것은 **살아 있는** 인스턴스이거나
    /// **크래시로 죽은 지 TTL이 지나지 않은** 인스턴스뿐이다. 시간 비교는
    /// `acquire_control_lease`와 같은 술어(`expires_at` vs `NOW()`)를 쓴다.
    async fn live_control_lease_holder(&self) -> Result<Option<(String, String, i64)>, StoreError> {
        let row: Option<(String, String, i64)> = sqlx::query_as(
            "SELECT cluster_id, \
                    active_instance_id, \
                    CEIL(EXTRACT(EPOCH FROM (expires_at - NOW())))::BIGINT \
               FROM control_plane_lease \
              WHERE expires_at > NOW() \
              ORDER BY cluster_id \
              LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// 살아 있는 control plane 밑에서 스키마가 갈리는 것을 막는다 (로드맵 #63,
    /// 게이트 5의 schema 절반).
    ///
    /// sqlx는 **한 방향만** 막는다. DB에 적용돼 있는데 이 바이너리에는 없는
    /// 마이그레이션은 `ignore_missing`이 기본 `false`라서
    /// `MigrateError::VersionMissing`으로 거절된다(= DB가 바이너리보다 앞선
    /// 경우). 그러나 바이너리에만 있는 마이그레이션은
    /// `Migrator::run_direct`의 `None => conn.apply(...)` 가지에서 **말없이
    /// 적용된다**. Cold Standby는 primary와 DB 하나를 공유하므로
    /// (`docs/architecture/control-plane-authority-and-failover.md`), 더 새
    /// 바이너리를 든 standby가 기동하는 것만으로 살아 있는 primary 밑에서
    /// 스키마가 바뀐다. sqlx 자신의 advisory lock은 이 위험을 덮지 못한다 —
    /// 그것은 동시 마이그레이터끼리를 직렬화할 뿐이고, 여기서 문제는 이미
    /// 돌고 있는 옛 바이너리이지 경쟁하는 마이그레이터가 아니다.
    ///
    /// **적용할 것이 없으면 통과시킨다.** 같은 버전의 재기동이나 동일 버전
    /// standby 기동은 스키마를 바꾸지 않으므로 막을 이유가 없다. 여기서
    /// 무조건 거절하면 평범한 롤링 재기동이 통째로 막히는 운영 함정이 된다.
    ///
    /// **한계 — 이 검사와 실제 적용 사이는 원자적이지 않다.** 검사 직후 다른
    /// 인스턴스가 lease를 획득하면 스키마는 여전히 바뀔 수 있다. 이 게이트는
    /// 배포 실수(더 새 바이너리를 살아 있는 클러스터에 붙이는 것)를 막는
    /// 것이지 분산 합의가 아니다. 원자적으로 만들려면 마이그레이션을 lease
    /// 아래로 넣어야 하는데, `control_plane_lease` 테이블 자체를 만드는 것이
    /// 021 마이그레이션이라 순환이 생긴다.
    async fn guard_migration_against_live_lease(&self) -> Result<(), StoreError> {
        let Some(pending) = self.pending_migration_versions().await? else {
            return Ok(());
        };
        if pending.is_empty() {
            return Ok(());
        }
        if !self.relation_exists("control_plane_lease").await? {
            return Ok(());
        }
        let Some((cluster_id, instance_id, remaining_secs)) =
            self.live_control_lease_holder().await?
        else {
            return Ok(());
        };
        let versions: Vec<String> = pending.iter().map(|v| v.to_string()).collect();
        let versions = versions.join(", ");
        Err(StoreError::Migration(format!(
            "refusing to apply migrations [{versions}] while instance '{instance_id}' holds a \
             live control plane lease for cluster '{cluster_id}' (expires in {remaining_secs}s): \
             this binary is newer than the database and would change the schema underneath the \
             running instance. Stop the active instance — a graceful shutdown releases the lease \
             immediately — or, if it crashed, wait {remaining_secs}s for the lease to expire, \
             then retry."
        )))
    }

    /// User 행을 튜플에서 구조체로 변환하는 공통 헬퍼.
    fn row_to_user(r: UserRow) -> User {
        User {
            id: UserId::from(r.0),
            username: r.1,
            email: r.2,
            email_verified: r.3,
            password_hash: r.4,
            enabled: r.5,
            created_at: r.6,
            last_login_at: r.7,
        }
    }

    /// 공통 SELECT 컬럼 목록.
    const USER_COLUMNS: &'static str =
        "id, username, email, email_verified, password_hash, enabled, created_at, last_login_at";

    /// `tasks` INSERT 본체. `on_conflict`에는 호출부가 고른 **고정 문자열**만
    /// 들어간다(빈 문자열 또는 멱등 삽입용 `ON CONFLICT ... DO NOTHING`) —
    /// 사용자 입력이 이 자리에 닿는 경로는 없다.
    ///
    /// 컬럼 목록이 23개를 넘어 두 벌로 복제되면 한쪽만 고쳐지는 사고가 반드시
    /// 나므로, `insert_task`와 `insert_task_idempotent`가 같은 SQL을 공유한다.
    /// 반환값은 실제로 삽입된 행 수 — `DO NOTHING`이 발동하면 0이다.
    async fn insert_task_row(&self, task: &Task, on_conflict: &str) -> Result<u64, StoreError> {
        let priority_str = priority_to_str(task.priority);
        let status_json = serde_json::to_value(&task.status)?;
        let labels_json = serde_json::to_value(&task.required_labels)?;

        let dep_uuids: Vec<Uuid> = task.dependency_ids.iter().map(|id| id.as_uuid()).collect();
        let sql = format!(
            r#"
            INSERT INTO tasks
                (id, prompt, cwd, model, server_hint, required_labels,
                 max_turns, timeout_secs, created_at, created_by, priority, status, dispatched_at,
                 thread_id, parent_task_id, project_id, dependency_ids, checkpoint_branch, skills_required,
                 requested_profile, resolved_model, token_budget, partial_output,
                 idempotency_key, idempotency_payload_hash)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25)
            {on_conflict}
            "#,
        );
        let result = sqlx::query(&sql)
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
            .bind(task.dispatched_at)
            .bind(task.thread_id.as_uuid())
            .bind(task.parent_task_id.map(|id| id.as_uuid()))
            .bind(task.project_id.map(|id| id.as_uuid()))
            .bind(dep_uuids)
            .bind(task.checkpoint_branch.as_ref())
            .bind(&task.skills_required)
            .bind(task.routing_profile.as_deref())
            .bind(task.resolved_model.as_deref())
            .bind(task.token_budget.map(|v| v as i64))
            .bind(task.partial_output.as_deref())
            .bind(task.idempotency_key.as_deref())
            .bind(task.idempotency_payload_hash.as_deref())
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Store trait 구현
// ═══════════════════════════════════════════════════════════════════════

#[async_trait]
impl Store for PgStore {
    // ── Task ───────────────────────────────────────────────────────────

    async fn insert_task(&self, task: &Task) -> Result<(), StoreError> {
        self.insert_task_row(task, "").await?;
        Ok(())
    }

    async fn insert_task_idempotent(&self, task: &Task) -> Result<IdempotentInsert, StoreError> {
        let Some(key) = task.idempotency_key.as_deref() else {
            // 키가 없으면 유일 인덱스의 부분 조건(`WHERE idempotency_key IS NOT
            // NULL`)에 애초에 걸리지 않으므로 ON CONFLICT는 절대 발동하지 않는다.
            // 그 사실에 기대는 대신 분기를 명시해서, 인덱스 술어가 나중에 바뀌어도
            // "키 없는 제출은 항상 새 Task"라는 계약이 코드에 남게 한다.
            self.insert_task_row(task, "").await?;
            return Ok(IdempotentInsert::Inserted);
        };

        // ON CONFLICT의 부분 인덱스 추론은 인덱스와 같은 술어를 요구한다.
        let inserted = self
            .insert_task_row(
                task,
                "ON CONFLICT (created_by, idempotency_key) \
                 WHERE idempotency_key IS NOT NULL DO NOTHING",
            )
            .await?;
        if inserted > 0 {
            return Ok(IdempotentInsert::Inserted);
        }

        // 삽입이 0행이라는 건 같은 키가 이미 있다는 뜻이다. 이 SELECT는 삽입
        // 시도 뒤에 일어나므로 "먼저 조회하고 없으면 삽입"의 경합 창이 없다 —
        // 유일 인덱스가 이미 승자를 정했고, 우리는 진 쪽에서 승자를 읽을 뿐이다.
        let row = sqlx::query(
            r#"SELECT id, prompt, cwd, model, server_hint, required_labels,
                      max_turns, timeout_secs, created_at, created_by, priority, status, dispatched_at,
                      thread_id, parent_task_id, project_id, retry_count, dependency_ids, checkpoint_branch, skills_required,
                      requested_profile, resolved_model, token_budget, partial_output,
                      idempotency_key, idempotency_payload_hash, dispatch_control_epoch
               FROM tasks WHERE created_by = $1 AND idempotency_key = $2"#,
        )
        .bind(&task.created_by)
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;

        // 승자가 이미 사라졌다면(그 사이의 삭제) 키는 다시 비었다. 재시도하지
        // 않고 Conflict로도 보지 않는다 — 호출자에게 에러로 알려서, 조용히
        // 중복 Task를 만들거나 없는 Task의 id를 돌려주는 일이 없게 한다.
        let Some(row) = row else {
            return Err(StoreError::Conflict(format!(
                "idempotency key '{key}' for '{}' vanished between insert and lookup",
                task.created_by
            )));
        };

        let existing = row_to_task(row)?;
        if existing.idempotency_payload_hash == task.idempotency_payload_hash {
            Ok(IdempotentInsert::Duplicate(Box::new(existing)))
        } else {
            Ok(IdempotentInsert::Conflict {
                existing_task_id: existing.id,
            })
        }
    }

    async fn get_task(&self, id: TaskId) -> Result<Option<Task>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, prompt, cwd, model, server_hint, required_labels,
                      max_turns, timeout_secs, created_at, created_by, priority, status, dispatched_at,
                      thread_id, parent_task_id, project_id, retry_count, dependency_ids, checkpoint_branch, skills_required,
                      requested_profile, resolved_model, token_budget, partial_output,
                      idempotency_key, idempotency_payload_hash, dispatch_control_epoch
               FROM tasks WHERE id = $1"#,
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_task).transpose()
    }

    async fn list_thread_tasks(&self, thread_id: TaskId) -> Result<Vec<Task>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, prompt, cwd, model, server_hint, required_labels,
                      max_turns, timeout_secs, created_at, created_by, priority, status, dispatched_at,
                      thread_id, parent_task_id, project_id, retry_count, dependency_ids, checkpoint_branch, skills_required,
                      requested_profile, resolved_model, token_budget, partial_output,
                      idempotency_key, idempotency_payload_hash, dispatch_control_epoch
               FROM tasks WHERE thread_id = $1 ORDER BY created_at ASC"#,
        )
        .bind(thread_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_task).collect()
    }

    async fn update_task_status(&self, id: TaskId, status: &TaskStatus) -> Result<(), StoreError> {
        let status_json = serde_json::to_value(status)?;
        let is_dispatching = matches!(status, TaskStatus::Dispatched { .. });

        let result = if is_dispatching {
            sqlx::query("UPDATE tasks SET status = $2, dispatched_at = NOW() WHERE id = $1")
                .bind(id.as_uuid())
                .bind(status_json)
                .execute(&self.pool)
                .await?
        } else {
            sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
                .bind(id.as_uuid())
                .bind(status_json)
                .execute(&self.pool)
                .await?
        };

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn compare_and_set_task_status(
        &self,
        id: TaskId,
        expected: &[TaskPhase],
        new: &TaskStatus,
        fence: Option<&ControlFence>,
        origin: TransitionOrigin,
    ) -> Result<TransitionOutcome, StoreError> {
        // 빈 기대 집합은 어떤 현재 상태와도 일치할 수 없다. SQL로 보내면
        // `= ANY('{}')`가 항상 거짓이라 0행이 되고, 아래 재조회가 이를
        // `Rejected`로 보고하므로 동작 자체는 옳다. 다만 왕복 두 번을 쓸
        // 이유가 없고, 호출자의 버그(예: `allowed_predecessors(Pending)`을
        // 그대로 넘김)를 조용히 삼키는 대신 여기서 바로 드러내는 편이 낫다.
        debug_assert!(
            !expected.is_empty(),
            "compare_and_set_task_status: expected가 비어 있으면 어떤 전이도 성립하지 않는다"
        );

        let status_json = serde_json::to_value(new)?;
        let is_dispatching = matches!(new, TaskStatus::Dispatched { .. });
        // `status_phase`는 001_init.sql의 생성 칼럼(`status->>'phase'`)이라 TEXT다.
        // 여기 문자열은 `TaskStatus`의 serde 표현과 반드시 일치해야 하며,
        // 그 계약은 fleet-core의 `phase_str_matches_serialized_status`가 지킨다.
        let expected_phases: Vec<&str> = expected.iter().map(|p| p.as_str()).collect();

        // 문장을 조립하는 이유: `dispatched_at` 유무 x fence 유무로 네 가지
        // 조합이 나오는데, 이걸 네 개의 리터럴로 펼치면 같은 WHERE 절이 네 번
        // 중복돼 한 곳만 고치는 사고가 난다. 바인딩 자리번호($1..$5)는 아래
        // `bind` 순서와 정확히 짝을 이뤄야 하므로 둘을 붙여 둔다.
        //
        // `dispatched_at = NOW()`는 조건 없는 버전과 동일하게 Dispatched 전이에만
        // 붙인다 — 두 분기를 하나로 합치면 이 타임스탬프가 사라지거나 종료
        // 전이에서도 갱신된다.
        let mut sql = String::from("UPDATE tasks SET status = $2");
        if is_dispatching {
            sql.push_str(", dispatched_at = NOW()");
            // epoch는 fence가 있을 때만 적는다. `$5`는 아래에서 fence가
            // `Some`일 때만 바인딩되므로, 이 두 조건이 어긋나면 자리번호가
            // 텍스트에만 남고 값이 없어 Postgres가 문장을 거절한다 —
            // 그래서 여기 조건은 `is_dispatching`이 아니라 그 **교집합**이다.
            //
            // Dispatched 전이에만 붙이는 것도 `dispatched_at`과 같은 이유다.
            // 조건을 떼면 완료/실패 전이가 이 값을 덮어써서 "디스패치한 세대"가
            // "마지막으로 손댄 세대"로 바뀐다.
            if fence.is_some() {
                sql.push_str(", dispatch_control_epoch = $5");
            }
        }
        sql.push_str(" WHERE id = $1 AND status_phase = ANY($3)");
        if fence.is_some() {
            // epoch 술어를 **같은 문장 안에** 넣는 것이 이 변경의 전부다.
            // lease를 먼저 SELECT해서 분기하면 그 사이에 fenced되어도 이미
            // 떠난 UPDATE는 그대로 도착한다 — 정확히 그 창을 닫는다.
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM control_plane_lease \
                  WHERE cluster_id = $4 AND epoch = $5)",
            );
            // 위 술어는 "지금 내가 제어 기관인가"를 묻는다. 이 술어는 "이 결과가
            // 내 세대의 것인가"를 묻는다 — 다른 질문이고, 위 술어가 참이어도
            // 이쪽이 거짓일 수 있다(로드맵 #67 1단계, 불변식 ②).
            //
            // 같은 `$5`를 쓰지만 비교 대상이 다르다: 위는 리스 테이블의 *현재*
            // epoch, 여기는 이 행이 디스패치될 때 적힌 epoch다. 둘이 갈리는
            // 경우가 정확히 닫으려는 창이다.
            //
            // NULL을 통과시키는 것은 026이 규정한 NULL의 의미를 따른 것이다 —
            // "제어 세대 개념이 없는 배포"에는 물어볼 세대가 없다. 이 조건을
            // 떼면 HA를 나중에 켠 배포에서 전환 이전 작업이 전부 종료 불가가
            // 된다.
            if matches!(origin, TransitionOrigin::WorkerOutcome) {
                sql.push_str(
                    " AND (dispatch_control_epoch IS NULL OR dispatch_control_epoch = $5)",
                );
            }
        }

        let mut query = sqlx::query(&sql)
            .bind(id.as_uuid())
            .bind(status_json)
            .bind(&expected_phases);
        if let Some(f) = fence {
            query = query.bind(f.cluster_id.as_str()).bind(f.epoch);
        }
        let result = query.execute(&self.pool).await?;

        if result.rows_affected() > 0 {
            return Ok(TransitionOutcome::Applied);
        }

        // fence를 걸었다면 0행의 원인이 하나 더 있다 — epoch 술어가 깨진 경우다.
        // 아래의 위상 재조회보다 **먼저** 확인한다. fenced 인스턴스가 읽은 Task
        // 상태에는 아무 권위가 없으므로, 그 값을 근거로 `NotFound`나 `Rejected`를
        // 돌려주면 호출자를 "내가 제어 기관이었는데 이 Task만 문제였다"는
        // 잘못된 결론으로 보낸다.
        if let Some(f) = fence {
            let held: Option<i64> =
                sqlx::query_scalar("SELECT epoch FROM control_plane_lease WHERE cluster_id = $1")
                    .bind(f.cluster_id.as_str())
                    .fetch_optional(&self.pool)
                    .await?;
            if held != Some(f.epoch) {
                return Ok(TransitionOutcome::Fenced);
            }

            // 리스는 내 것이 맞다. 그렇다면 `WorkerOutcome`에서 0행이 나온
            // 원인이 하나 더 있다 — dispatch 세대 술어다. 이것도 아래의 위상
            // 재조회보다 **먼저** 확인해야 한다. 위상은 기대와 일치했을 것이
            // 거의 확실하므로, 순서를 바꾸면 `Rejected { current: Dispatched }`가
            // 나와 "다른 writer가 먼저 옮겼다"는 거짓을 보고하게 된다.
            //
            // 이 재조회는 UPDATE와 같은 트랜잭션이 아니지만, `current` 재조회와
            // 달리 값이 흔들리지 않는다 — `dispatch_control_epoch`는 Dispatched
            // 전이에서 한 번 쓰이고 그 뒤로 바뀌지 않는다.
            if matches!(origin, TransitionOrigin::WorkerOutcome) {
                let dispatched_under: Option<Option<i64>> =
                    sqlx::query_scalar("SELECT dispatch_control_epoch FROM tasks WHERE id = $1")
                        .bind(id.as_uuid())
                        .fetch_optional(&self.pool)
                        .await?;
                if let Some(Some(under)) = dispatched_under {
                    if under != f.epoch {
                        return Ok(TransitionOutcome::StaleDispatchEpoch {
                            dispatched_under: under,
                        });
                    }
                }
            }
        }

        // 0행의 나머지 원인은 둘이다: 행이 없거나(NotFound), 있는데 위상이
        // 달랐거나(Rejected). UPDATE만으로는 구분할 수 없어 한 번 더 읽는다.
        //
        // 이 SELECT는 위 UPDATE와 같은 트랜잭션이 아니다. 그 사이에 또 다른
        // writer가 상태를 바꾸면 여기서 읽는 위상은 UPDATE가 거절당한 시점의
        // 위상이 아닐 수 있다. `current`를 로깅·에러 메시지 전용으로 규정하고
        // 제어 흐름에 쓰지 않기로 한 이유이며(`TransitionOutcome` 주석 참조),
        // 그 대가로 트랜잭션 비용을 치르지 않는다. 두 경우 모두 "이 호출은
        // 아무것도 쓰지 않았다"는 결론은 동일하게 참이다.
        let row = sqlx::query("SELECT status FROM tasks WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_optional(&self.pool)
            .await?;

        match row {
            None => Err(StoreError::NotFound),
            Some(row) => {
                let status_json: serde_json::Value = row.try_get("status")?;
                let current: TaskStatus = serde_json::from_value(status_json)?;
                Ok(TransitionOutcome::Rejected {
                    current: current.phase(),
                })
            }
        }
    }

    async fn increment_task_retry_count(&self, id: TaskId) -> Result<u32, StoreError> {
        let row = sqlx::query(
            "UPDATE tasks SET retry_count = retry_count + 1 WHERE id = $1 RETURNING retry_count",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;

        let row = row.ok_or(StoreError::NotFound)?;
        let retry_count: i32 = row.try_get("retry_count")?;
        Ok(retry_count as u32)
    }

    async fn update_task_checkpoint(
        &self,
        id: TaskId,
        checkpoint_branch: Option<&str>,
    ) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE tasks SET checkpoint_branch = $2 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(checkpoint_branch)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, StoreError> {
        // 단순 필터는 SQL로, 복잡한 것(status 위상)은 Rust로 후처리.
        let limit = filter.limit.min(1000) as i64;
        let offset = filter.offset as i64;
        let worker_id_str = filter.worker_id.map(|w| w.0.to_string());

        // worker_id를 SQL JSONB 필터로 푸시하여 LIMIT 전에 올바르게 필터링.
        // status 컬럼은 externallly-tagged enum JSONB:
        //   {"Dispatched": {"worker_id": "..."}}
        //   {"Completed": {"worker_id": "..."}}
        //   {"Failed": {"worker_id": "..."}}
        const SELECT_COLS: &str = r#"SELECT id, prompt, cwd, model, server_hint, required_labels,
                          max_turns, timeout_secs, created_at, created_by, priority, status, dispatched_at,
                          thread_id, parent_task_id, project_id, retry_count, dependency_ids, checkpoint_branch, skills_required,
                          requested_profile, resolved_model, token_budget, partial_output,
                      idempotency_key, idempotency_payload_hash, dispatch_control_epoch
                   FROM tasks"#;
        const WORKER_WHERE: &str = r#"(status->'Dispatched'->>'worker_id' = $1
                       OR status->'Completed'->>'worker_id' = $1
                       OR status->'Failed'->>'worker_id' = $1)"#;

        let rows = match (&filter.created_by, &worker_id_str) {
            (Some(created_by), Some(wid)) => {
                sqlx::query(&format!(
                    "{SELECT_COLS} WHERE created_by = $1 AND {WORKER_WHERE} \
                     ORDER BY created_at DESC LIMIT $2 OFFSET $3"
                ))
                .bind(created_by)
                .bind(wid)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(created_by), None) => {
                sqlx::query(&format!(
                    "{SELECT_COLS} WHERE created_by = $1 \
                     ORDER BY created_at DESC LIMIT $2 OFFSET $3"
                ))
                .bind(created_by)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(wid)) => {
                sqlx::query(&format!(
                    "{SELECT_COLS} WHERE {WORKER_WHERE} \
                     ORDER BY created_at DESC LIMIT $2 OFFSET $3"
                ))
                .bind(wid)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(&format!(
                    "{SELECT_COLS} ORDER BY created_at DESC LIMIT $1 OFFSET $2"
                ))
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
        };

        let mut tasks: Vec<Task> = rows
            .into_iter()
            .map(row_to_task)
            .collect::<Result<_, _>>()?;

        // 상태 위상 필터 (SQL의 status_phase 대응, 단 Terminal/Active 합성도 처리)
        if let Some(status_filter) = filter.status {
            tasks.retain(|t| status_matches(&t.status, status_filter));
        }

        // worker_id 필터는 SQL에서 처리했으므로 Rust 단 후처리 불필요.

        Ok(tasks)
    }

    async fn delete_task(&self, id: TaskId) -> Result<TaskDeleteOutcome, StoreError> {
        // 1. 의존자 선검사. `dependency_ids <> '{}'`을 `@>`와 함께 명시해야
        //    idx_tasks_dependency_ids(025)의 predicate를 플래너가 증명할 수
        //    있다 — 배열 포함 연산자는 등호 비교와 달리 IS NOT NULL/빈 배열
        //    아님을 자동으로 함의하지 않는다 (마이그레이션 025 주석 참고).
        //
        //    이 조회와 아래 DELETE 사이에는 트랜잭션이 없다 — 그 사이 새
        //    Pending 의존자가 삽입되면 통과한다. `TaskDeleteOutcome` 문서에
        //    남긴 대로, 닫지 않기로 한 창이다.
        let dependent_rows = sqlx::query(
            "SELECT id FROM tasks \
             WHERE status_phase = 'pending' \
               AND dependency_ids <> '{}' \
               AND dependency_ids @> ARRAY[$1]::uuid[]",
        )
        .bind(id.as_uuid())
        .fetch_all(&self.pool)
        .await?;

        if !dependent_rows.is_empty() {
            let dependent_ids = dependent_rows
                .into_iter()
                .map(|row| row.try_get::<Uuid, _>("id").map(TaskId::from))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(TaskDeleteOutcome::BlockedByDependents { dependent_ids });
        }

        // 2. terminal 판정과 삭제를 한 조건절에 건다 — 대상 행 자체가
        //    조건이므로 compare_and_set_task_status와 같은 이유로 TOCTOU가
        //    없다. task_outputs/task_telemetry는 ON DELETE CASCADE로 함께
        //    지워지고, events.task_id/issue_task_links.task_id/자식의
        //    parent_task_id는 ON DELETE SET NULL로 컬럼만 NULL이 된다.
        //    events.payload는 FleetEvent 전체를 JSONB로 담고 있어 원본
        //    task_id를 잃지 않는다 — "익명화"가 아니다 (001/016/013/023,
        //    docs/architecture/tasks/management.md "무엇이 함께 사라지는가"
        //    가 이 cascade 표의 정본이다).
        let terminal_phases: Vec<&str> = [
            TaskPhase::Pending,
            TaskPhase::Dispatched,
            TaskPhase::Completed,
            TaskPhase::Failed,
            TaskPhase::Cancelled,
        ]
        .into_iter()
        .filter(|p| p.is_terminal())
        .map(|p| p.as_str())
        .collect();

        let result = sqlx::query("DELETE FROM tasks WHERE id = $1 AND status_phase = ANY($2)")
            .bind(id.as_uuid())
            .bind(&terminal_phases)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() > 0 {
            return Ok(TaskDeleteOutcome::Deleted);
        }

        // 0행은 행이 없거나(NotFound) terminal이 아니었거나(NotTerminal) 둘 중
        // 하나다 — compare_and_set_task_status와 같은 형태로 재조회해 가른다.
        // 이 SELECT도 위 DELETE와 같은 트랜잭션이 아니므로 `current`는 보고
        // 전용이다.
        let row = sqlx::query("SELECT status FROM tasks WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_optional(&self.pool)
            .await?;

        match row {
            None => Err(StoreError::NotFound),
            Some(row) => {
                let status_json: serde_json::Value = row.try_get("status")?;
                let current: TaskStatus = serde_json::from_value(status_json)?;
                Ok(TaskDeleteOutcome::NotTerminal {
                    current: current.phase(),
                })
            }
        }
    }

    async fn list_task_threads(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<TaskId>, StoreError> {
        // 스레드 선정만 한다 — 구성원 적재는 호출부가 각 thread_id에
        // list_thread_tasks를 따로 호출한다(ui-design.md §3.3의 "두 질의"
        // 설계). idx_tasks_thread_id(thread_id, created_at)가 이
        // GROUP BY + MAX(created_at) 집계와 뒤따르는 정렬을 뒷받침한다.
        let rows = sqlx::query(
            "SELECT thread_id FROM tasks \
             GROUP BY thread_id \
             ORDER BY MAX(created_at) DESC, thread_id DESC \
             LIMIT $1 OFFSET $2",
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| row.try_get::<Uuid, _>("thread_id").map(TaskId::from))
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    // ── Worker ─────────────────────────────────────────────────────────

    async fn upsert_worker(&self, worker: &Worker) -> Result<(), StoreError> {
        let labels_json = serde_json::to_value(&worker.labels)?;
        let status_str = worker_status_to_str(worker.status);
        let circuit_str = circuit_state_to_str(worker.circuit_state);
        let liveness_str = liveness_mode_to_str(worker.liveness_mode);

        sqlx::query(
            r#"
            INSERT INTO workers
                (id, name, endpoint, labels, status, circuit_state,
                 last_seen, active_tasks, max_concurrent, worker_version, liveness_mode, registered_at)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (id) DO UPDATE SET
                name            = EXCLUDED.name,
                endpoint        = EXCLUDED.endpoint,
                labels          = EXCLUDED.labels,
                status          = EXCLUDED.status,
                circuit_state   = EXCLUDED.circuit_state,
                last_seen       = EXCLUDED.last_seen,
                active_tasks    = EXCLUDED.active_tasks,
                max_concurrent  = EXCLUDED.max_concurrent,
                worker_version  = EXCLUDED.worker_version,
                liveness_mode   = EXCLUDED.liveness_mode
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
        .bind(liveness_str)
        .bind(worker.registered_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_worker(&self, id: WorkerId) -> Result<Option<Worker>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, name, endpoint, labels, status, circuit_state,
                      last_seen, active_tasks, max_concurrent, worker_version, liveness_mode, registered_at
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
                      last_seen, active_tasks, max_concurrent, worker_version, liveness_mode, registered_at
               FROM workers WHERE name = $1"#,
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_worker).transpose()
    }

    async fn list_workers(&self, filter: &WorkerFilter) -> Result<Vec<Worker>, StoreError> {
        let limit = filter.limit.min(1000) as i64;
        let offset = filter.offset as i64;
        let status_str = filter.status.map(worker_status_to_str);

        // 라벨 필터는 SQL의 JSONB containment(`@>`)로 처리한다.
        // 이전 구현은 LIMIT으로 잘라온 뒤 Rust에서 걸러냈기 때문에,
        // 라벨 필터가 있으면 limit보다 훨씬 적은 행이 반환되고 페이지네이션이
        // 성립하지 않았다. `idx_workers_labels_gin`(jsonb_path_ops)이 `@>`를 지원한다.
        let labels_json = if filter.labels.is_empty() {
            None
        } else {
            Some(serde_json::to_value(&filter.labels)?)
        };

        let rows = sqlx::query(
            r#"SELECT id, name, endpoint, labels, status, circuit_state,
                      last_seen, active_tasks, max_concurrent, worker_version, liveness_mode, registered_at
               FROM workers
              WHERE ($1::text IS NULL OR status = $1)
                AND ($2::jsonb IS NULL OR labels @> $2)
              ORDER BY registered_at DESC
              LIMIT $3 OFFSET $4"#,
        )
        .bind(status_str)
        .bind(labels_json)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_worker).collect()
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

    async fn update_worker_circuit_state(
        &self,
        id: WorkerId,
        state: fleet_core::worker::CircuitState,
    ) -> Result<(), StoreError> {
        let circuit_str = circuit_state_to_str(state);
        let result = sqlx::query(
            r#"UPDATE workers SET
                 circuit_state = $2
               WHERE id = $1"#,
        )
        .bind(id.as_uuid())
        .bind(circuit_str)
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
        self.guard_migration_against_live_lease().await?;
        MIGRATOR
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
                (token_digest, created_at, created_by, expires_at, max_uses, use_count, notes,
                 last_used_by, last_used_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(&token.token_digest)
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
             WHERE token_digest = $1
               AND use_count < max_uses
               AND (expires_at IS NULL OR expires_at > $3)
            RETURNING token_digest
            "#,
        )
        .bind(BootstrapToken::digest_for(token))
        .bind(used_by)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        if result.is_some() {
            Ok(())
        } else {
            // 토큰이 존재하는지 확인하여 적절한 에러 메시지 구성.
            let exists: Option<(String,)> =
                sqlx::query_as("SELECT token_digest FROM bootstrap_tokens WHERE token_digest = $1")
                    .bind(BootstrapToken::digest_for(token))
                    .fetch_optional(&self.pool)
                    .await?;
            let reason = match exists {
                Some(_) => "token is exhausted or expired",
                None => "token not found",
            };
            Err(StoreError::BootstrapTokenInvalid(reason.into()))
        }
    }

    async fn list_bootstrap_tokens(&self) -> Result<Vec<BootstrapToken>, StoreError> {
        let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(
            r#"
            SELECT token_digest, created_at, created_by, expires_at, max_uses, use_count,
                   notes, last_used_by, last_used_at
              FROM bootstrap_tokens
             ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_bootstrap_token).collect()
    }

    async fn revoke_bootstrap_token(&self, token_digest: &str) -> Result<bool, StoreError> {
        let result = sqlx::query("DELETE FROM bootstrap_tokens WHERE token_digest = $1")
            .bind(token_digest)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ── Admin API tokens (로드맵 #72) ────────────────────────────────

    async fn create_admin_token(&self, token: &AdminApiToken) -> Result<(), StoreError> {
        let capabilities_json = serde_json::to_value(&token.capabilities)?;
        sqlx::query(
            "INSERT INTO admin_api_tokens (principal_id, token_digest, capabilities, created_at, rotated_at, revoked_at, rotation_generation) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&token.principal_id)
        .bind(&token.token_digest)
        .bind(capabilities_json)
        .bind(token.created_at)
        .bind(token.rotated_at)
        .bind(token.revoked_at)
        .bind(token.rotation_generation)
        .execute(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => StoreError::Conflict(
                format!("admin token principal or digest already exists: {}", db.message()),
            ),
            other => StoreError::Sqlx(other),
        })?;
        Ok(())
    }

    async fn find_active_admin_token_by_digest(
        &self,
        token_digest: &str,
    ) -> Result<Option<AdminApiToken>, StoreError> {
        let row = sqlx::query(
            "SELECT principal_id, token_digest, capabilities, created_at, rotated_at, revoked_at, rotation_generation FROM admin_api_tokens WHERE token_digest = $1 AND revoked_at IS NULL",
        )
        .bind(token_digest)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_admin_token).transpose()
    }

    async fn list_admin_tokens(&self) -> Result<Vec<AdminApiToken>, StoreError> {
        let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(
            "SELECT principal_id, token_digest, capabilities, created_at, rotated_at, revoked_at, rotation_generation FROM admin_api_tokens ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_admin_token).collect()
    }

    async fn rotate_admin_token(
        &self,
        principal_id: &str,
        new_token_digest: &str,
    ) -> Result<AdminApiToken, StoreError> {
        let row = sqlx::query(
            r#"
            UPDATE admin_api_tokens
               SET token_digest = $2,
                   rotated_at = NOW(),
                   revoked_at = NULL,
                   rotation_generation = rotation_generation + 1
             WHERE principal_id = $1
            RETURNING principal_id, token_digest, capabilities, created_at, rotated_at, revoked_at, rotation_generation
            "#,
        )
        .bind(principal_id)
        .bind(new_token_digest)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)?;
        row_to_admin_token(row)
    }

    async fn revoke_admin_token(&self, principal_id: &str) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE admin_api_tokens SET revoked_at = NOW() WHERE principal_id = $1 AND revoked_at IS NULL",
        )
        .bind(principal_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn upsert_worker_operational_credential(
        &self,
        credential: &WorkerOperationalCredential,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO worker_operational_credentials (worker_id, credential_digest, issued_at, expires_at, revoked_at, rotation_generation) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (worker_id) DO UPDATE SET credential_digest = EXCLUDED.credential_digest, issued_at = EXCLUDED.issued_at, expires_at = EXCLUDED.expires_at, revoked_at = EXCLUDED.revoked_at, rotation_generation = EXCLUDED.rotation_generation",
        )
        .bind(credential.worker_id.0)
        .bind(&credential.credential_digest)
        .bind(credential.issued_at)
        .bind(credential.expires_at)
        .bind(credential.revoked_at)
        .bind(credential.rotation_generation)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn find_active_worker_operational_credential(
        &self,
        credential_digest: &str,
    ) -> Result<Option<WorkerOperationalCredential>, StoreError> {
        let row = sqlx::query("SELECT worker_id, credential_digest, issued_at, expires_at, revoked_at, rotation_generation FROM worker_operational_credentials WHERE credential_digest = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > NOW())")
            .bind(credential_digest)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| {
            Ok(WorkerOperationalCredential {
                worker_id: WorkerId(row.try_get("worker_id")?),
                credential_digest: row.try_get("credential_digest")?,
                issued_at: row.try_get("issued_at")?,
                expires_at: row.try_get("expires_at")?,
                revoked_at: row.try_get("revoked_at")?,
                rotation_generation: row.try_get("rotation_generation")?,
            })
        })
        .transpose()
    }

    async fn revoke_worker_operational_credential(
        &self,
        worker_id: WorkerId,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE worker_operational_credentials SET revoked_at = NOW() WHERE worker_id = $1 AND revoked_at IS NULL",
        )
        .bind(worker_id.0)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn rotate_worker_operational_credential(
        &self,
        worker_id: WorkerId,
        new_credential_digest: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<WorkerOperationalCredential, StoreError> {
        let row = sqlx::query(
            r#"
            UPDATE worker_operational_credentials
               SET credential_digest = $2,
                   issued_at = NOW(),
                   expires_at = $3,
                   revoked_at = NULL,
                   rotation_generation = rotation_generation + 1
             WHERE worker_id = $1
            RETURNING worker_id, credential_digest, issued_at, expires_at, revoked_at, rotation_generation
            "#,
        )
        .bind(worker_id.0)
        .bind(new_credential_digest)
        .bind(expires_at)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)?;
        Ok(WorkerOperationalCredential {
            worker_id: WorkerId(row.try_get("worker_id")?),
            credential_digest: row.try_get("credential_digest")?,
            issued_at: row.try_get("issued_at")?,
            expires_at: row.try_get("expires_at")?,
            revoked_at: row.try_get("revoked_at")?,
            rotation_generation: row.try_get("rotation_generation")?,
        })
    }

    /// bootstrap 토큰 소비 + worker insert + operational credential insert를 단일
    /// DB 트랜잭션으로 묶는다 (로드맵 #60). 세 단계 중 하나라도 실패하면
    /// `tx`가 드롭되며 자동 롤백되어 토큰도 소비되지 않고 worker도 남지 않는다.
    ///
    /// 각 단계의 SQL은 `consume_bootstrap_token`/`upsert_worker`/
    /// `upsert_worker_operational_credential`과 동일한 조건이지만, 트랜잭션
    /// executor(`&mut *tx`)를 대상으로 재실행한다 — 원본 메서드들은
    /// `&self.pool`을 직접 사용하므로 그대로 재사용할 수 없다.
    async fn enroll_worker(
        &self,
        bootstrap_token: &str,
        used_by: &str,
        worker: &Worker,
        credential: &WorkerOperationalCredential,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now();

        // 1. bootstrap 토큰 atomic 소비 (consume_bootstrap_token과 동일한 SQL).
        let digest = fleet_core::BootstrapToken::digest_for(bootstrap_token);
        let consumed = sqlx::query(
            r#"
            UPDATE bootstrap_tokens
               SET use_count = use_count + 1,
                   last_used_by = $2,
                   last_used_at = $3
             WHERE token_digest = $1
               AND use_count < max_uses
               AND (expires_at IS NULL OR expires_at > $3)
            RETURNING token_digest
            "#,
        )
        .bind(&digest)
        .bind(used_by)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;

        if consumed.is_none() {
            let exists: Option<(String,)> =
                sqlx::query_as("SELECT token_digest FROM bootstrap_tokens WHERE token_digest = $1")
                    .bind(&digest)
                    .fetch_optional(&mut *tx)
                    .await?;
            // tx는 여기서 drop되어 자동 롤백 — 토큰/worker/credential 모두 미반영.
            let reason = match exists {
                Some(_) => "token is exhausted or expired",
                None => "token not found",
            };
            return Err(StoreError::BootstrapTokenInvalid(reason.into()));
        }

        // 2. worker insert. `workers.name` UNIQUE 제약 위반은 Conflict로 매핑.
        //    join은 항상 신규 worker_id를 발급하므로 upsert가 아닌 순수 INSERT로
        //    이름 충돌을 확실히 거부한다 (TOCTOU 방지 — 사전 get_worker_by_name
        //    검사와 이 INSERT 사이의 race도 여기서 최종적으로 막힌다).
        let labels_json = serde_json::to_value(&worker.labels)?;
        let status_str = worker_status_to_str(worker.status);
        let circuit_str = circuit_state_to_str(worker.circuit_state);
        let liveness_str = liveness_mode_to_str(worker.liveness_mode);
        sqlx::query(
            r#"
            INSERT INTO workers
                (id, name, endpoint, labels, status, circuit_state,
                 last_seen, active_tasks, max_concurrent, worker_version, liveness_mode, registered_at)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
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
        .bind(liveness_str)
        .bind(worker.registered_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict(format!("worker name already exists: {}", db.message()))
            }
            other => StoreError::Sqlx(other),
        })?;

        // 3. operational credential insert. `credential_digest` UNIQUE 제약
        //    위반(다른 worker_id가 이미 같은 digest를 쓰고 있음)은 Conflict로 매핑.
        sqlx::query(
            "INSERT INTO worker_operational_credentials (worker_id, credential_digest, issued_at, expires_at, revoked_at, rotation_generation) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(credential.worker_id.0)
        .bind(&credential.credential_digest)
        .bind(credential.issued_at)
        .bind(credential.expires_at)
        .bind(credential.revoked_at)
        .bind(credential.rotation_generation)
        .execute(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict(format!("credential digest already exists: {}", db.message()))
            }
            other => StoreError::Sqlx(other),
        })?;

        tx.commit().await?;
        Ok(())
    }

    // ── Worker credentials (Phase 8.6) ─────────────────────────────────

    async fn upsert_worker_credential(
        &self,
        worker_name: &str,
        model_id: &str,
        encrypted_blob: &str,
        base_url: &str,
        api_backend: &str,
        context_window: u32,
        model_name: Option<&str>,
    ) -> Result<(), StoreError> {
        // ON CONFLICT 시 rotated_at만 NOW()로 갱신 (회전 시간 기록).
        sqlx::query(
            r#"
            INSERT INTO worker_credentials
                (worker_name, model_id, encrypted_blob, base_url, api_backend,
                 context_window, model_name, created_at, rotated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())
            ON CONFLICT (worker_name, model_id) DO UPDATE
                SET encrypted_blob = EXCLUDED.encrypted_blob,
                    base_url       = EXCLUDED.base_url,
                    api_backend    = EXCLUDED.api_backend,
                    context_window = EXCLUDED.context_window,
                    model_name     = EXCLUDED.model_name,
                    rotated_at     = NOW()
            "#,
        )
        .bind(worker_name)
        .bind(model_id)
        .bind(encrypted_blob)
        .bind(base_url)
        .bind(api_backend)
        .bind(context_window as i32)
        .bind(model_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_worker_credential(
        &self,
        worker_name: &str,
        model_id: &str,
    ) -> Result<Option<StoredCredential>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT worker_name, model_id, encrypted_blob, base_url, api_backend,
                   context_window, model_name, created_at, rotated_at
              FROM worker_credentials
             WHERE worker_name = $1 AND model_id = $2
            "#,
        )
        .bind(worker_name)
        .bind(model_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(row_to_stored_credential).transpose()?)
    }

    async fn list_worker_credentials(
        &self,
        worker_name: &str,
    ) -> Result<Vec<StoredCredential>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT worker_name, model_id, encrypted_blob, base_url, api_backend,
                   context_window, model_name, created_at, rotated_at
              FROM worker_credentials
             WHERE worker_name = $1
             ORDER BY model_id
            "#,
        )
        .bind(worker_name)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_stored_credential).collect()
    }

    async fn delete_worker_credential(
        &self,
        worker_name: &str,
        model_id: &str,
    ) -> Result<bool, StoreError> {
        let result =
            sqlx::query("DELETE FROM worker_credentials WHERE worker_name = $1 AND model_id = $2")
                .bind(worker_name)
                .bind(model_id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    // ── RBAC: Users ───────────────────────────────────────────────────

    async fn create_user(&self, user: &User) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO users (id, username, email, email_verified, password_hash, enabled, created_at, last_login_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(user.id.as_uuid())
        .bind(&user.username)
        .bind(&user.email)
        .bind(user.email_verified)
        .bind(&user.password_hash)
        .bind(user.enabled)
        .bind(user.created_at)
        .bind(user.last_login_at)
        .execute(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict(format!("user already exists: {}", db.message()))
            }
            other => StoreError::Sqlx(other),
        })?;
        Ok(())
    }

    async fn get_user_by_id(&self, id: UserId) -> Result<Option<User>, StoreError> {
        let sql = format!("SELECT {} FROM users WHERE id = $1", Self::USER_COLUMNS);
        let row: Option<UserRow> = sqlx::query_as(&sql)
            .bind(id.as_uuid())
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(Self::row_to_user))
    }

    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>, StoreError> {
        let sql = format!(
            "SELECT {} FROM users WHERE username = $1",
            Self::USER_COLUMNS
        );
        let row: Option<UserRow> = sqlx::query_as(&sql)
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(Self::row_to_user))
    }

    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, StoreError> {
        let sql = format!("SELECT {} FROM users WHERE email = $1", Self::USER_COLUMNS);
        let row: Option<UserRow> = sqlx::query_as(&sql)
            .bind(email)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(Self::row_to_user))
    }

    async fn list_users(&self) -> Result<Vec<User>, StoreError> {
        let sql = format!(
            "SELECT {} FROM users ORDER BY created_at ASC",
            Self::USER_COLUMNS
        );
        let rows: Vec<UserRow> = sqlx::query_as(&sql).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(Self::row_to_user).collect())
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
            VALUES ($1, $2, $3, $4, $5, $6::inet, $7)
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

    // ── Audit log ─────────────────────────────────────────────────────

    async fn record_audit_event(&self, event: &AuditEvent) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO audit_log
                (id, actor_user_id, actor_label, action, target_type, target_id,
                 outcome, ip_address, detail, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(event.id)
        .bind(event.actor_user_id.map(|id| id.as_uuid()))
        .bind(&event.actor_label)
        .bind(&event.action)
        .bind(event.target_type.as_ref())
        .bind(event.target_id.as_ref())
        .bind(event.outcome.as_str())
        .bind(event.ip_address.as_ref())
        .bind(&event.detail)
        .bind(event.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_audit_events(&self, filter: &AuditFilter) -> Result<Vec<AuditEvent>, StoreError> {
        let limit = filter.limit.min(1000) as i64;
        let offset = filter.offset as i64;

        let rows: Vec<(
            Uuid,
            Option<Uuid>,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            serde_json::Value,
            DateTime<Utc>,
        )> = sqlx::query_as(
            r#"
            SELECT id, actor_user_id, actor_label, action, target_type, target_id,
                   outcome, ip_address, detail, created_at
              FROM audit_log
             WHERE ($1::uuid IS NULL OR actor_user_id = $1)
               AND ($2::text IS NULL OR action = $2)
             ORDER BY created_at DESC
             LIMIT $3 OFFSET $4
            "#,
        )
        .bind(filter.actor_user_id.map(|id| id.as_uuid()))
        .bind(filter.action.as_ref())
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| AuditEvent {
                id: r.0,
                actor_user_id: r.1.map(UserId::from),
                actor_label: r.2,
                action: r.3,
                target_type: r.4,
                target_id: r.5,
                // CHECK 제약이 값을 보증하지만, 방어적으로 실패 취급한다
                // (알 수 없는 값을 성공으로 표시하면 감사에서 더 위험하다).
                outcome: AuditOutcome::parse_str(&r.6).unwrap_or(AuditOutcome::Failure),
                ip_address: r.7,
                detail: r.8,
                created_at: r.9,
            })
            .collect())
    }

    async fn update_session_expiry(
        &self,
        id: SessionId,
        expires_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE sessions SET expires_at = $2 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(expires_at)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
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

    // ── Email verification ───────────────────────────────────────────

    async fn create_email_verification_token(
        &self,
        token: &fleet_core::EmailVerificationToken,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO email_verification_tokens (id, user_id, token_hash, created_at, expires_at, consumed_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(token.id)
        .bind(token.user_id.as_uuid())
        .bind(&token.token_hash)
        .bind(token.created_at)
        .bind(token.expires_at)
        .bind(token.consumed_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_email_verification_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<fleet_core::EmailVerificationToken>, StoreError> {
        let row: Option<(
            Uuid,
            Uuid,
            String,
            chrono::DateTime<Utc>,
            chrono::DateTime<Utc>,
            Option<chrono::DateTime<Utc>>,
        )> = sqlx::query_as(
            r#"
            SELECT id, user_id, token_hash, created_at, expires_at, consumed_at
              FROM email_verification_tokens WHERE token_hash = $1
            "#,
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| fleet_core::EmailVerificationToken {
            id: r.0,
            user_id: UserId::from(r.1),
            token_hash: r.2,
            created_at: r.3,
            expires_at: r.4,
            consumed_at: r.5,
        }))
    }

    async fn consume_email_verification_token(
        &self,
        token_id: Uuid,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE email_verification_tokens SET consumed_at = $2 WHERE id = $1 AND consumed_at IS NULL")
            .bind(token_id)
            .bind(at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_user_email_verified(
        &self,
        user_id: UserId,
        verified: bool,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE users SET email_verified = $2 WHERE id = $1")
            .bind(user_id.as_uuid())
            .bind(verified)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Password reset ────────────────────────────────────────────────

    async fn create_password_reset_token(
        &self,
        token: &fleet_core::PasswordResetToken,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO password_reset_tokens (id, user_id, token_hash, created_at, expires_at, consumed_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(token.id)
        .bind(token.user_id.as_uuid())
        .bind(&token.token_hash)
        .bind(token.created_at)
        .bind(token.expires_at)
        .bind(token.consumed_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_password_reset_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<fleet_core::PasswordResetToken>, StoreError> {
        let row: Option<(
            Uuid,
            Uuid,
            String,
            chrono::DateTime<Utc>,
            chrono::DateTime<Utc>,
            Option<chrono::DateTime<Utc>>,
        )> = sqlx::query_as(
            r#"
            SELECT id, user_id, token_hash, created_at, expires_at, consumed_at
              FROM password_reset_tokens WHERE token_hash = $1
            "#,
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| fleet_core::PasswordResetToken {
            id: r.0,
            user_id: UserId::from(r.1),
            token_hash: r.2,
            created_at: r.3,
            expires_at: r.4,
            consumed_at: r.5,
        }))
    }

    async fn consume_password_reset_token(
        &self,
        token_id: Uuid,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE password_reset_tokens SET consumed_at = $2 WHERE id = $1 AND consumed_at IS NULL",
        )
        .bind(token_id)
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(())
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
               -- ip = NULL → 모든 IP 합산 (identifier 단독 카운트).
               -- ip = Some → 해당 IP로 한정.
               -- 주의: `IS NOT DISTINCT FROM $2`만 쓰면 $2=NULL일 때
               -- ip_address IS NULL 행만 세게 되어 카운트가 항상 0이 된다.
               AND ($2::text IS NULL OR ip_address IS NOT DISTINCT FROM $2)
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

    async fn count_recent_ip_failures(
        &self,
        ip: &str,
        window_secs: i64,
    ) -> Result<u64, StoreError> {
        let (count,): (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM login_attempts
             WHERE ip_address = $1
               AND success = FALSE
               AND (failure_reason IS NULL OR failure_reason NOT IN ('forgot_password', 'resend_verification'))
               AND attempted_at >= NOW() - make_interval(secs => $2)
            "#,
        )
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
               AND ($2::text IS NULL OR ip_address IS NOT DISTINCT FROM $2)
            "#,
        )
        .bind(identifier)
        .bind(ip)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    async fn delete_old_login_attempts(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query("DELETE FROM login_attempts WHERE attempted_at < $1")
            .bind(before)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    // ── Host inventory (Phase P1.5) ───────────────────────────────────

    async fn upsert_host(&self, host: &fleet_core::Host) -> Result<(), StoreError> {
        let os_info_json = host
            .os_info
            .as_ref()
            .map(|oi| {
                serde_json::json!({
                    "os_type": oi.os_type,
                    "distro": oi.distro,
                    "kernel": oi.kernel,
                    "arch": oi.arch,
                    "hostname": oi.hostname,
                })
            })
            .unwrap_or(serde_json::json!({}));

        let load_avg_json = if host.metrics.load_avg.is_empty() {
            None
        } else {
            Some(serde_json::json!(host.metrics.load_avg))
        };

        sqlx::query(
            r#"
            INSERT INTO hosts (
                id, hostname, worker_id, status,
                ssh_host, ssh_port, ssh_user,
                grok_version, fleet_worker_version, os_info,
                load_avg, mem_available_mb, disk_free_mb,
                last_heartbeat_at, provisioned_at, created_at, updated_at,
                cpu_usage, ram_usage
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, NOW(), $17, $18)
            ON CONFLICT (hostname) DO UPDATE SET
                worker_id = EXCLUDED.worker_id,
                status = EXCLUDED.status,
                ssh_host = COALESCE(EXCLUDED.ssh_host, hosts.ssh_host),
                ssh_port = EXCLUDED.ssh_port,
                ssh_user = COALESCE(EXCLUDED.ssh_user, hosts.ssh_user),
                grok_version = COALESCE(EXCLUDED.grok_version, hosts.grok_version),
                fleet_worker_version = COALESCE(EXCLUDED.fleet_worker_version, hosts.fleet_worker_version),
                os_info = EXCLUDED.os_info,
                load_avg = COALESCE(EXCLUDED.load_avg, hosts.load_avg),
                mem_available_mb = COALESCE(EXCLUDED.mem_available_mb, hosts.mem_available_mb),
                disk_free_mb = COALESCE(EXCLUDED.disk_free_mb, hosts.disk_free_mb),
                last_heartbeat_at = COALESCE(EXCLUDED.last_heartbeat_at, hosts.last_heartbeat_at),
                provisioned_at = COALESCE(EXCLUDED.provisioned_at, hosts.provisioned_at),
                cpu_usage = EXCLUDED.cpu_usage,
                ram_usage = EXCLUDED.ram_usage
            "#,
        )
        .bind(host.id)
        .bind(&host.hostname)
        .bind(host.worker_id.map(|w| w.as_uuid()))
        .bind(host.status.as_str())
        .bind(&host.ssh_host)
        .bind(host.ssh_port)
        .bind(&host.ssh_user)
        .bind(&host.grok_version)
        .bind(&host.fleet_worker_version)
        .bind(&os_info_json)
        .bind(load_avg_json)
        .bind(host.metrics.mem_available_mb.map(|v| v as i64))
        .bind(host.metrics.disk_free_mb.map(|v| v as i64))
        .bind(host.last_heartbeat_at)
        .bind(host.provisioned_at)
        .bind(host.created_at)
        .bind(host.metrics.cpu_usage)
        .bind(host.metrics.ram_usage)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_host_by_hostname(
        &self,
        hostname: &str,
    ) -> Result<Option<fleet_core::Host>, StoreError> {
        let row = sqlx::query("SELECT * FROM hosts WHERE hostname = $1")
            .bind(hostname)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| row_to_host(&r)).transpose()
    }

    async fn get_host_by_worker(
        &self,
        worker_id: WorkerId,
    ) -> Result<Option<fleet_core::Host>, StoreError> {
        let row = sqlx::query("SELECT * FROM hosts WHERE worker_id = $1")
            .bind(worker_id.as_uuid())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| row_to_host(&r)).transpose()
    }

    async fn list_hosts(&self) -> Result<Vec<fleet_core::Host>, StoreError> {
        let rows = sqlx::query("SELECT * FROM hosts ORDER BY created_at ASC")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(row_to_host).collect()
    }

    async fn append_host_event(&self, event: &fleet_core::HostEvent) -> Result<(), StoreError> {
        let payload_json = serde_json::to_value(&event.payload).unwrap_or(serde_json::json!({}));
        sqlx::query(
            r#"
            INSERT INTO host_events (id, host_id, event_type, severity, message, payload, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(event.id)
        .bind(event.host_id)
        .bind(&event.event_type)
        .bind(event.severity.as_str())
        .bind(&event.message)
        .bind(&payload_json)
        .bind(event.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_host_events(
        &self,
        host_id: Uuid,
        limit: u32,
    ) -> Result<Vec<fleet_core::HostEvent>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM host_events WHERE host_id = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(host_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_host_event).collect()
    }

    // ── SSH 키 금고 ───────────────────────────────────────────────

    async fn create_ssh_key(&self, key: &fleet_core::SshKey) -> Result<(), StoreError> {
        sqlx::query(
            r#"INSERT INTO ssh_keys (id, name, encrypted_blob, fingerprint, key_type)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (name) DO UPDATE
               SET encrypted_blob = EXCLUDED.encrypted_blob,
                   fingerprint = EXCLUDED.fingerprint,
                   key_type = EXCLUDED.key_type,
                   updated_at = NOW()"#,
        )
        .bind(key.id)
        .bind(&key.name)
        .bind(&key.encrypted_blob)
        .bind(&key.fingerprint)
        .bind(&key.key_type)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_ssh_key(&self, name: &str) -> Result<Option<fleet_core::SshKey>, StoreError> {
        let row = sqlx::query(
            "SELECT id, name, encrypted_blob, fingerprint, key_type, \
             created_at, updated_at FROM ssh_keys WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| fleet_core::SshKey {
            id: r.get("id"),
            name: r.get("name"),
            encrypted_blob: r.get("encrypted_blob"),
            fingerprint: r.get("fingerprint"),
            key_type: r.get("key_type"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        }))
    }

    async fn list_ssh_keys(&self) -> Result<Vec<fleet_core::SshKey>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, name, encrypted_blob, fingerprint, key_type, \
             created_at, updated_at FROM ssh_keys ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| fleet_core::SshKey {
                id: r.get("id"),
                name: r.get("name"),
                encrypted_blob: r.get("encrypted_blob"),
                fingerprint: r.get("fingerprint"),
                key_type: r.get("key_type"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    async fn delete_ssh_key(&self, name: &str) -> Result<bool, StoreError> {
        let result = sqlx::query("DELETE FROM ssh_keys WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ── Control plane lease (로드맵 #63, 1단계) ──────────────────────

    async fn acquire_control_lease(
        &self,
        cluster_id: &str,
        instance_id: &str,
        ttl: std::time::Duration,
    ) -> Result<ControlLease, StoreError> {
        // `ON CONFLICT ... DO UPDATE ... WHERE`가 이 메서드의 CAS 전체를
        // 단일 원자적 statement로 표현한다. WHERE 조건(만료됨)이 거짓이면
        // Postgres는 그 행을 갱신하지도, RETURNING에 포함하지도 않는다 —
        // `fetch_optional`이 `None`을 돌려주는 것과 "다른 instance가 아직
        // 유효한 lease를 쥐고 있다(Refused)"가 정확히 대응한다. `NOW()`(DB
        // 서버 시각)만 시간 비교에 쓴다 — 애플리케이션 서버 시각을 신뢰하면
        // 클럭 스큐만으로 두 instance가 동시에 "내가 유효하다"고 믿을 수
        // 있다.
        let row = sqlx::query(
            r#"
            INSERT INTO control_plane_lease
                (cluster_id, active_instance_id, epoch, acquired_at, expires_at, last_renewed_at)
            VALUES ($1, $2, 1, NOW(), NOW() + $3, NOW())
            ON CONFLICT (cluster_id) DO UPDATE
               SET active_instance_id = EXCLUDED.active_instance_id,
                   epoch = control_plane_lease.epoch + 1,
                   acquired_at = NOW(),
                   expires_at = NOW() + $3,
                   last_renewed_at = NOW()
             WHERE control_plane_lease.expires_at < NOW()
            RETURNING cluster_id, active_instance_id, epoch, acquired_at, expires_at, last_renewed_at
            "#,
        )
        .bind(cluster_id)
        .bind(instance_id)
        .bind(ttl)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "control plane lease for cluster '{cluster_id}' is held by another instance"
            ))
        })?;
        row_to_control_lease(row)
    }

    async fn renew_control_lease(
        &self,
        cluster_id: &str,
        instance_id: &str,
        epoch: i64,
        ttl: std::time::Duration,
    ) -> Result<ControlLease, StoreError> {
        // `(instance_id, epoch)` 일치 + 아직 만료되지 않음을 모두 요구한다.
        // `expires_at > NOW()`를 빼면, 이미 만료돼 다른 instance가 막 가로챈
        // lease를 이 instance가 "갱신"이라는 이름으로 되살려(epoch은 그대로
        // 두고 expires_at만 미래로) 두 instance가 동시에 유효하다고 믿는
        // 경합을 만들 수 있다.
        let row = sqlx::query(
            r#"
            UPDATE control_plane_lease
               SET expires_at = NOW() + $4,
                   last_renewed_at = NOW()
             WHERE cluster_id = $1
               AND active_instance_id = $2
               AND epoch = $3
               AND expires_at > NOW()
            RETURNING cluster_id, active_instance_id, epoch, acquired_at, expires_at, last_renewed_at
            "#,
        )
        .bind(cluster_id)
        .bind(instance_id)
        .bind(epoch)
        .bind(ttl)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)?;
        row_to_control_lease(row)
    }

    async fn release_control_lease(
        &self,
        cluster_id: &str,
        instance_id: &str,
        epoch: i64,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE control_plane_lease
               SET expires_at = NOW()
             WHERE cluster_id = $1
               AND active_instance_id = $2
               AND epoch = $3
            "#,
        )
        .bind(cluster_id)
        .bind(instance_id)
        .bind(epoch)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn get_control_lease(
        &self,
        cluster_id: &str,
    ) -> Result<Option<ControlLease>, StoreError> {
        let row = sqlx::query(
            "SELECT cluster_id, active_instance_id, epoch, acquired_at, expires_at, last_renewed_at \
               FROM control_plane_lease WHERE cluster_id = $1",
        )
        .bind(cluster_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_control_lease).transpose()
    }

    // ── Project (로드맵 #48, 1단계) ───────────────────────────────────

    async fn create_project(&self, project: &Project) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO projects (id, name, description, created_by, status, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(project.id.0)
        .bind(&project.name)
        .bind(project.description.as_ref())
        .bind(project.created_by.as_ref())
        .bind(project.status.as_str())
        .bind(project.created_at)
        .bind(project.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict(format!("project name already exists: {}", db.message()))
            }
            other => StoreError::Sqlx(other),
        })?;
        Ok(())
    }

    async fn get_project(&self, id: ProjectId) -> Result<Option<Project>, StoreError> {
        let row = sqlx::query(
            "SELECT id, name, description, created_by, status, created_at, updated_at \
               FROM projects WHERE id = $1",
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_project).transpose()
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Option<Project>, StoreError> {
        let row = sqlx::query(
            "SELECT id, name, description, created_by, status, created_at, updated_at \
               FROM projects WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_project).transpose()
    }

    async fn list_projects(&self, filter: &ProjectFilter) -> Result<Vec<Project>, StoreError> {
        let limit = filter.limit.clamp(1, 1000) as i64;
        let offset = filter.offset as i64;
        let status_str = filter.status.map(|s| s.as_str());

        let rows = sqlx::query(
            "SELECT id, name, description, created_by, status, created_at, updated_at \
               FROM projects \
              WHERE ($1::text IS NULL OR status = $1) \
              ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(status_str)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_project).collect()
    }

    async fn update_project_status(
        &self,
        id: ProjectId,
        status: ProjectStatus,
    ) -> Result<bool, StoreError> {
        let result =
            sqlx::query("UPDATE projects SET status = $2, updated_at = NOW() WHERE id = $1")
                .bind(id.0)
                .bind(status.as_str())
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn project_has_active_tasks(&self, project_id: ProjectId) -> Result<bool, StoreError> {
        // `status_phase`는 001_init.sql의 생성 칼럼(`status->>'phase'`) —
        // TaskStatus가 `#[serde(tag = "phase")]`라 이 값이 정확히
        // 'pending'/'dispatched'/'completed'/'failed'/'cancelled'다.
        let (exists,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE project_id = $1 AND status_phase IN ('pending', 'dispatched'))",
        )
        .bind(project_id.0)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    // ── Agent (로드맵 #49, 1단계) ─────────────────────────────────────

    async fn create_agent(&self, agent: &Agent) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO agents
                (id, project_id, name, description, created_by, status, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(agent.id.0)
        .bind(agent.project_id.0)
        .bind(&agent.name)
        .bind(agent.description.as_ref())
        .bind(agent.created_by.as_ref())
        .bind(agent.status.as_str())
        .bind(agent.created_at)
        .bind(agent.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict(format!(
                    "agent name already exists in this project: {}",
                    db.message()
                ))
            }
            // `agents.project_id`의 FK 위반 — 호출부의 사전 검사와 INSERT
            // 사이에 Project가 사라진 경우에만 도달한다(오늘은 Project 물리
            // 삭제 경로가 없어 실제로는 도달 불가). `Conflict`로 옮기는 이유는
            // 이것이 서버 결함이 아니라 호출자 입력이 더 이상 유효하지 않다는
            // 뜻이기 때문이다.
            sqlx::Error::Database(ref db) if db.is_foreign_key_violation() => {
                StoreError::Conflict(format!("no such project for agent: {}", db.message()))
            }
            other => StoreError::Sqlx(other),
        })?;
        Ok(())
    }

    async fn get_agent(&self, id: AgentId) -> Result<Option<Agent>, StoreError> {
        let row = sqlx::query(
            "SELECT id, project_id, name, description, created_by, status, created_at, updated_at \
               FROM agents WHERE id = $1",
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_agent).transpose()
    }

    async fn get_agent_by_name(
        &self,
        project_id: ProjectId,
        name: &str,
    ) -> Result<Option<Agent>, StoreError> {
        let row = sqlx::query(
            "SELECT id, project_id, name, description, created_by, status, created_at, updated_at \
               FROM agents WHERE project_id = $1 AND name = $2",
        )
        .bind(project_id.0)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_agent).transpose()
    }

    async fn list_agents(&self, filter: &AgentFilter) -> Result<Vec<Agent>, StoreError> {
        let limit = filter.limit.clamp(1, 1000) as i64;
        let offset = filter.offset as i64;
        let status_str = filter.status.map(|s| s.as_str());
        let project_id = filter.project_id.map(|p| p.0);

        let rows = sqlx::query(
            "SELECT id, project_id, name, description, created_by, status, created_at, updated_at \
               FROM agents \
              WHERE ($1::uuid IS NULL OR project_id = $1) \
                AND ($2::text IS NULL OR status = $2) \
              ORDER BY created_at DESC LIMIT $3 OFFSET $4",
        )
        .bind(project_id)
        .bind(status_str)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_agent).collect()
    }

    async fn update_agent_status(
        &self,
        id: AgentId,
        status: AgentStatus,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query("UPDATE agents SET status = $2, updated_at = NOW() WHERE id = $1")
            .bind(id.0)
            .bind(status.as_str())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn project_has_live_agents(&self, project_id: ProjectId) -> Result<bool, StoreError> {
        // `idx_agents_project_status`(027)가 이 조회를 덮는다.
        let (exists,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM agents WHERE project_id = $1 AND status <> 'stopped')",
        )
        .bind(project_id.0)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    // ── Issue (로드맵 #88) ────────────────────────────────────────────

    async fn create_issue(&self, issue: &Issue) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO issues
                (id, project_id, title, body, status, close_reason, severity, labels,
                 assignee, created_by, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(issue.id.0)
        .bind(issue.project_id.0)
        .bind(&issue.title)
        .bind(&issue.body)
        .bind(issue.status.as_str())
        .bind(issue.close_reason.map(|r| r.as_str()))
        .bind(issue.severity.as_str())
        .bind(serde_json::to_value(&issue.labels)?)
        .bind(issue.assignee.as_ref())
        .bind(&issue.created_by)
        .bind(issue.created_at)
        .bind(issue.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_issue(&self, id: IssueId) -> Result<Option<Issue>, StoreError> {
        let row = sqlx::query(&format!("{ISSUE_SELECT_COLS} WHERE id = $1"))
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_issue).transpose()
    }

    async fn list_issues(&self, filter: &IssueFilter) -> Result<Vec<Issue>, StoreError> {
        let limit = filter.limit.clamp(1, 1000) as i64;
        let offset = filter.offset as i64;
        let rows = sqlx::query(&format!(
            "{ISSUE_SELECT_COLS} \
              WHERE ($1::uuid IS NULL OR project_id = $1) \
                AND ($2::text IS NULL OR status = $2) \
                AND (NOT $3::bool OR status <> 'closed') \
              ORDER BY created_at DESC LIMIT $4 OFFSET $5"
        ))
        .bind(filter.project_id.map(|p| p.0))
        .bind(filter.status.map(|s| s.as_str()))
        .bind(filter.open_only)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_issue).collect()
    }

    async fn update_issue_fields(&self, issue: &Issue) -> Result<bool, StoreError> {
        // status/close_reason은 의도적으로 건드리지 않는다 — 상태 전이는
        // `transition_issue`만 수행한다(`issue:update`와 `issue:close`의
        // capability 분리를 저장소 API에서도 유지).
        let result = sqlx::query(
            r#"
            UPDATE issues
               SET title = $2, body = $3, severity = $4, labels = $5,
                   assignee = $6, updated_at = NOW()
             WHERE id = $1
            "#,
        )
        .bind(issue.id.0)
        .bind(&issue.title)
        .bind(&issue.body)
        .bind(issue.severity.as_str())
        .bind(serde_json::to_value(&issue.labels)?)
        .bind(issue.assignee.as_ref())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn transition_issue(
        &self,
        id: IssueId,
        status: IssueStatus,
        close_reason: Option<CloseReason>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE issues SET status = $2, close_reason = $3, updated_at = NOW() WHERE id = $1",
        )
        .bind(id.0)
        .bind(status.as_str())
        .bind(close_reason.map(|r| r.as_str()))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn add_issue_comment(&self, comment: &IssueComment) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO issue_comments (id, issue_id, author, body, created_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(comment.id)
        .bind(comment.issue_id.0)
        .bind(&comment.author)
        .bind(&comment.body)
        .bind(comment.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_issue_comments(
        &self,
        issue_id: IssueId,
    ) -> Result<Vec<IssueComment>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, issue_id, author, body, created_at FROM issue_comments \
              WHERE issue_id = $1 ORDER BY created_at",
        )
        .bind(issue_id.0)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let issue_id: Uuid = row.try_get("issue_id")?;
                Ok(IssueComment {
                    id: row.try_get("id")?,
                    issue_id: IssueId(issue_id),
                    author: row.try_get("author")?,
                    body: row.try_get("body")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }

    async fn link_issue_task(&self, link: &IssueTaskLink) -> Result<bool, StoreError> {
        // `ON CONFLICT DO NOTHING`으로 멱등 — 유니크 인덱스
        // `(issue_id, task_id)`가 중복을 막는다.
        let result = sqlx::query(
            "INSERT INTO issue_task_links (issue_id, task_id, task_label, linked_by, linked_at) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        )
        .bind(link.issue_id.0)
        .bind(link.task_id.map(|t| t.0))
        .bind(&link.task_label)
        .bind(&link.linked_by)
        .bind(link.linked_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn unlink_issue_task(
        &self,
        issue_id: IssueId,
        task_id: TaskId,
    ) -> Result<bool, StoreError> {
        let result =
            sqlx::query("DELETE FROM issue_task_links WHERE issue_id = $1 AND task_id = $2")
                .bind(issue_id.0)
                .bind(task_id.0)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn list_issue_task_links(
        &self,
        issue_id: IssueId,
    ) -> Result<Vec<IssueTaskLink>, StoreError> {
        let rows = sqlx::query(
            "SELECT issue_id, task_id, task_label, linked_by, linked_at FROM issue_task_links \
              WHERE issue_id = $1 ORDER BY linked_at",
        )
        .bind(issue_id.0)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let issue_id: Uuid = row.try_get("issue_id")?;
                let task_id: Option<Uuid> = row.try_get("task_id")?;
                Ok(IssueTaskLink {
                    issue_id: IssueId(issue_id),
                    task_id: task_id.map(TaskId),
                    task_label: row.try_get("task_label")?,
                    linked_by: row.try_get("linked_by")?,
                    linked_at: row.try_get("linked_at")?,
                })
            })
            .collect()
    }

    async fn issue_has_active_tasks(&self, issue_id: IssueId) -> Result<bool, StoreError> {
        // 파생 "진행 중" 배지 전용 — 이 값을 Issue 상태로 저장하지 않는다
        // (`InProgress` 부재의 이유). `project_has_active_tasks`와 같은
        // `status_phase` 생성 칼럼을 쓴다.
        let (exists,): (bool,) = sqlx::query_as(
            "SELECT EXISTS( \
               SELECT 1 FROM issue_task_links l \
                 JOIN tasks t ON t.id = l.task_id \
                WHERE l.issue_id = $1 AND t.status_phase IN ('pending', 'dispatched'))",
        )
        .bind(issue_id.0)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }
}

const ISSUE_SELECT_COLS: &str = "SELECT id, project_id, title, body, status, close_reason, \
                                        severity, labels, assignee, created_by, created_at, updated_at \
                                   FROM issues";

// ═══════════════════════════════════════════════════════════════════════
//  행 → 도메인 변환 헬퍼
// ═══════════════════════════════════════════════════════════════════════

fn row_to_stored_credential(row: sqlx::postgres::PgRow) -> Result<StoredCredential, StoreError> {
    let context_window: i32 = row.try_get("context_window")?;
    Ok(StoredCredential {
        worker_name: row.try_get("worker_name")?,
        model_id: row.try_get("model_id")?,
        encrypted_blob: row.try_get("encrypted_blob")?,
        base_url: row.try_get("base_url")?,
        api_backend: row.try_get("api_backend")?,
        context_window: context_window as u32,
        model_name: row.try_get("model_name")?,
        created_at: row.try_get("created_at")?,
        rotated_at: row.try_get("rotated_at")?,
    })
}

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
    let dispatched_at: Option<DateTime<Utc>> = row.try_get("dispatched_at")?;
    let thread_id: Uuid = row.try_get("thread_id")?;
    let parent_task_id: Option<Uuid> = row.try_get("parent_task_id")?;
    let project_id: Option<Uuid> = row.try_get("project_id")?;
    let retry_count: i32 = row.try_get("retry_count")?;
    let dependency_uuids: Vec<Uuid> = row.try_get("dependency_ids")?;
    let checkpoint_branch: Option<String> = row.try_get("checkpoint_branch")?;
    let skills_required: Vec<String> = row.try_get("skills_required")?;
    let routing_profile: Option<String> = row.try_get("requested_profile")?;
    let resolved_model: Option<String> = row.try_get("resolved_model")?;
    let token_budget_raw: Option<i64> = row.try_get("token_budget")?;
    let partial_output: Option<String> = row.try_get("partial_output")?;
    let idempotency_key: Option<String> = row.try_get("idempotency_key")?;
    let idempotency_payload_hash: Option<String> = row.try_get("idempotency_payload_hash")?;
    let dispatch_control_epoch: Option<i64> = row.try_get("dispatch_control_epoch")?;

    let required_labels: Vec<String> = serde_json::from_value(labels_json)?;
    let status: TaskStatus = serde_json::from_value(status_json)?;
    let priority = str_to_priority(&priority_str)?;

    let dependency_ids = dependency_uuids.into_iter().map(TaskId::from).collect();

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
        dispatched_at,
        thread_id: TaskId::from(thread_id),
        parent_task_id: parent_task_id.map(TaskId::from),
        project_id: project_id.map(fleet_core::ProjectId::from),
        retry_count: retry_count as u32,
        dependency_ids,
        checkpoint_branch,
        skills_required,
        routing_profile,
        resolved_model,
        token_budget: token_budget_raw.map(|v| v as u64),
        partial_output,
        idempotency_key,
        idempotency_payload_hash,
        dispatch_control_epoch,
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
    let liveness_str: String = row.try_get("liveness_mode")?;
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
        liveness_mode: str_to_liveness_mode(&liveness_str)?,
        registered_at,
    })
}

fn row_to_bootstrap_token(row: sqlx::postgres::PgRow) -> Result<BootstrapToken, StoreError> {
    let token_digest: String = row.try_get("token_digest")?;
    let created_at = row.try_get("created_at")?;
    let created_by: Option<String> = row.try_get("created_by")?;
    let expires_at = row.try_get("expires_at")?;
    let max_uses: i32 = row.try_get("max_uses")?;
    let use_count: i32 = row.try_get("use_count")?;
    let notes: Option<String> = row.try_get("notes")?;
    let last_used_by: Option<String> = row.try_get("last_used_by")?;
    let last_used_at = row.try_get("last_used_at")?;

    Ok(BootstrapToken {
        token_digest,
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

fn row_to_issue(row: sqlx::postgres::PgRow) -> Result<Issue, StoreError> {
    let id: Uuid = row.try_get("id")?;
    let project_id: Uuid = row.try_get("project_id")?;
    let status_str: String = row.try_get("status")?;
    let status = IssueStatus::parse_str(&status_str)
        .ok_or_else(|| StoreError::Decode(format!("unknown issue status in DB: {status_str}")))?;
    let close_reason_str: Option<String> = row.try_get("close_reason")?;
    let close_reason = close_reason_str
        .map(|s| {
            CloseReason::parse_str(&s)
                .ok_or_else(|| StoreError::Decode(format!("unknown close_reason in DB: {s}")))
        })
        .transpose()?;
    let severity_str: String = row.try_get("severity")?;
    let severity = IssueSeverity::parse_str(&severity_str).ok_or_else(|| {
        StoreError::Decode(format!("unknown issue severity in DB: {severity_str}"))
    })?;
    let labels_json: serde_json::Value = row.try_get("labels")?;
    Ok(Issue {
        id: IssueId(id),
        project_id: ProjectId(project_id),
        title: row.try_get("title")?,
        body: row.try_get("body")?,
        status,
        close_reason,
        severity,
        labels: serde_json::from_value(labels_json)?,
        assignee: row.try_get("assignee")?,
        created_by: row.try_get("created_by")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_project(row: sqlx::postgres::PgRow) -> Result<Project, StoreError> {
    let id: Uuid = row.try_get("id")?;
    let status_str: String = row.try_get("status")?;
    let status = ProjectStatus::parse_str(&status_str)
        .ok_or_else(|| StoreError::Decode(format!("unknown project status in DB: {status_str}")))?;
    Ok(Project {
        id: ProjectId(id),
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        created_by: row.try_get("created_by")?,
        status,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_agent(row: sqlx::postgres::PgRow) -> Result<Agent, StoreError> {
    let id: Uuid = row.try_get("id")?;
    let project_id: Uuid = row.try_get("project_id")?;
    let status_str: String = row.try_get("status")?;
    let status = AgentStatus::parse_str(&status_str)
        .ok_or_else(|| StoreError::Decode(format!("unknown agent status in DB: {status_str}")))?;
    Ok(Agent {
        id: AgentId(id),
        project_id: ProjectId(project_id),
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        created_by: row.try_get("created_by")?,
        status,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_control_lease(row: sqlx::postgres::PgRow) -> Result<ControlLease, StoreError> {
    Ok(ControlLease {
        cluster_id: row.try_get("cluster_id")?,
        active_instance_id: row.try_get("active_instance_id")?,
        epoch: row.try_get("epoch")?,
        acquired_at: row.try_get("acquired_at")?,
        expires_at: row.try_get("expires_at")?,
        last_renewed_at: row.try_get("last_renewed_at")?,
    })
}

fn row_to_admin_token(row: sqlx::postgres::PgRow) -> Result<AdminApiToken, StoreError> {
    let capabilities_json: serde_json::Value = row.try_get("capabilities")?;
    let capabilities: Vec<PermissionKind> = serde_json::from_value(capabilities_json)?;
    Ok(AdminApiToken {
        principal_id: row.try_get("principal_id")?,
        token_digest: row.try_get("token_digest")?,
        capabilities,
        created_at: row.try_get("created_at")?,
        rotated_at: row.try_get("rotated_at")?,
        revoked_at: row.try_get("revoked_at")?,
        rotation_generation: row.try_get("rotation_generation")?,
    })
}

/// `TaskStatus`에서 worker_id 추출 (필터링용).
#[allow(dead_code)]
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
        WorkerStatus::Draining => "draining",
        WorkerStatus::Offline => "offline",
        WorkerStatus::CircuitOpen => "circuit_open",
    }
}

fn str_to_worker_status(s: &str) -> Result<WorkerStatus, StoreError> {
    match s {
        "online" => Ok(WorkerStatus::Online),
        "degraded" => Ok(WorkerStatus::Degraded),
        "draining" => Ok(WorkerStatus::Draining),
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

fn liveness_mode_to_str(m: fleet_core::WorkerLivenessMode) -> &'static str {
    match m {
        fleet_core::WorkerLivenessMode::Periodic => "periodic",
        fleet_core::WorkerLivenessMode::OnDemand => "on_demand",
    }
}

fn str_to_liveness_mode(s: &str) -> Result<fleet_core::WorkerLivenessMode, StoreError> {
    match s {
        "periodic" => Ok(fleet_core::WorkerLivenessMode::Periodic),
        "on_demand" => Ok(fleet_core::WorkerLivenessMode::OnDemand),
        other => Err(StoreError::Decode(format!(
            "unknown liveness mode: {other}"
        ))),
    }
}

// ── Host 변환 헬퍼 ────────────────────────────────────────────────────

fn row_to_host(row: &sqlx::postgres::PgRow) -> Result<fleet_core::Host, StoreError> {
    use fleet_core::{HostMetrics, HostStatus};

    let id: Uuid = row.try_get("id")?;
    let hostname: String = row.try_get("hostname")?;
    let worker_id: Option<Uuid> = row.try_get("worker_id")?;
    let status_str: String = row.try_get("status")?;
    let status = HostStatus::parse(&status_str)
        .ok_or_else(|| StoreError::Decode(format!("unknown host status: {status_str}")))?;
    let ssh_host: Option<String> = row.try_get("ssh_host")?;
    let ssh_port: i32 = row.try_get("ssh_port").unwrap_or(22);
    let ssh_user: Option<String> = row.try_get("ssh_user")?;
    let grok_version: Option<String> = row.try_get("grok_version")?;
    let fleet_worker_version: Option<String> = row.try_get("fleet_worker_version")?;

    let os_info_json: serde_json::Value = row.try_get("os_info").unwrap_or(serde_json::json!({}));
    let os_info = if os_info_json.is_null()
        || os_info_json
            .as_object()
            .map(|o| o.is_empty())
            .unwrap_or(true)
    {
        None
    } else {
        Some(fleet_core::OsInfo {
            os_type: os_info_json
                .get("os_type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            distro: os_info_json
                .get("distro")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            kernel: os_info_json
                .get("kernel")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            arch: os_info_json
                .get("arch")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            hostname: os_info_json
                .get("hostname")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
    };

    let load_avg_json: Option<serde_json::Value> = row.try_get("load_avg").unwrap_or(None);
    let load_avg: Vec<f32> = load_avg_json
        .as_ref()
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_f64().map(|f| f as f32))
                .collect()
        })
        .unwrap_or_default();

    let mem_available_mb: Option<i64> = row.try_get("mem_available_mb").unwrap_or(None);
    let disk_free_mb: Option<i64> = row.try_get("disk_free_mb").unwrap_or(None);
    let cpu_usage: Option<f32> = row.try_get("cpu_usage").unwrap_or(None);
    let ram_usage: Option<f32> = row.try_get("ram_usage").unwrap_or(None);

    let last_heartbeat_at: Option<chrono::DateTime<Utc>> = row.try_get("last_heartbeat_at")?;
    let provisioned_at: Option<chrono::DateTime<Utc>> = row.try_get("provisioned_at")?;
    let created_at: chrono::DateTime<Utc> = row.try_get("created_at")?;
    let updated_at: chrono::DateTime<Utc> = row.try_get("updated_at")?;

    Ok(fleet_core::Host {
        id,
        hostname,
        worker_id: worker_id.map(WorkerId::from),
        status,
        ssh_host,
        ssh_port,
        ssh_user,
        grok_version,
        fleet_worker_version,
        os_info,
        metrics: HostMetrics {
            load_avg,
            mem_available_mb: mem_available_mb.map(|v| v as u64),
            disk_free_mb: disk_free_mb.map(|v| v as u64),
            cpu_usage,
            ram_usage,
        },
        last_heartbeat_at,
        provisioned_at,
        created_at,
        updated_at,
    })
}

fn row_to_host_event(row: &sqlx::postgres::PgRow) -> Result<fleet_core::HostEvent, StoreError> {
    use fleet_core::{EventSeverity, HostEvent};

    let id: Uuid = row.try_get("id")?;
    let host_id: Uuid = row.try_get("host_id")?;
    let event_type: String = row.try_get("event_type")?;
    let severity_str: String = row.try_get("severity")?;
    let severity = match severity_str.as_str() {
        "info" => EventSeverity::Info,
        "warn" => EventSeverity::Warn,
        "error" => EventSeverity::Error,
        other => {
            return Err(StoreError::Decode(format!(
                "unknown event severity: {other}"
            )))
        }
    };
    let message: Option<String> = row.try_get("message")?;
    let payload_json: serde_json::Value = row.try_get("payload").unwrap_or(serde_json::json!({}));
    let payload: std::collections::HashMap<String, serde_json::Value> =
        serde_json::from_value(payload_json).unwrap_or_default();
    let created_at: chrono::DateTime<Utc> = row.try_get("created_at")?;

    Ok(HostEvent {
        id,
        host_id,
        event_type,
        severity,
        message,
        payload,
        created_at,
    })
}
