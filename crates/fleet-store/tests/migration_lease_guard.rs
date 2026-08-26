//! 살아 있는 control plane 밑에서 스키마가 갈리는 것을 막는 기동 게이트
//! (로드맵 #63, 구현 게이트 5의 schema 절반).
//!
//! ## 무엇이 이미 보장돼 있고, 무엇이 뚫려 있었나
//!
//! sqlx 0.8.6의 `Migrator::run_direct`는 **한 방향만** 막는다.
//!
//! | 상황 | sqlx 동작 |
//! |------|-----------|
//! | DB에 있는 버전이 바이너리에 없음 (DB가 앞섬) | `VersionMissing`으로 거절 |
//! | 적용된 버전의 체크섬이 다름 | `VersionMismatch`로 거절 |
//! | 바이너리에만 있는 버전 (바이너리가 앞섬) | **말없이 적용** |
//!
//! 앞의 두 줄은 `ignore_missing`이 기본 `false`이고 이 프로젝트가
//! `set_ignore_missing`을 호출하지 않기 때문에 이미 성립한다. 이 파일의
//! `db_ahead_of_binary_is_refused_by_sqlx_itself`가 그 기존 보장을 회귀
//! 테스트로 고정한다 — 지금까지 어느 테스트도 이것을 확인하지 않았고,
//! `ignore_missing`이 언젠가 켜지면 조용히 사라질 보장이었다.
//!
//! 마지막 줄이 뚫려 있던 부분이다. Cold Standby는 primary와 DB **하나**를
//! 공유하므로(`docs/architecture/control-plane-authority-and-failover.md`),
//! 더 새 바이너리를 든 standby가 기동하는 것만으로 살아 있는 primary 밑에서
//! 스키마가 바뀐다. `PgStore::migrate`가 이제 그 경우에만 거절한다.
//!
//! ## 왜 임시 데이터베이스인가
//!
//! 공용 `fleet_test`는 이미 최신 스키마라 "DB는 024까지, 바이너리는 025까지"
//! 라는 중간 상태를 만들 수 없다. `tests/bootstrap_token_migration.rs`와 같은
//! 이유이며 같은 방식(임시 DB 생성 → 부분 적용 → DROP)을 쓴다.
//!
//! 실제 PostgreSQL이 필요하다. `DATABASE_URL`이 설정되지 않으면 skip하고,
//! 설정돼 있는데 연결이 실패하면 panic한다 (다른 통합 테스트와 동일한 규약).
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test migration_lease_guard -- --test-threads=1
//! ```

use std::borrow::Cow;
use std::time::Duration as StdDuration;

use fleet_store::{PgStore, Store};
use sqlx::migrate::{Migration, Migrator};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// 025가 만드는 인덱스. "거절이 말뿐이 아니라 실제로 적용을 막았는가"를
/// 확인하는 관찰 지점으로 쓴다.
const MIGRATION_025_INDEX: &str = "idx_tasks_dependency_ids";

/// 이 바이너리가 들고 있는 마지막 마이그레이션 버전보다 하나 앞. DB를 여기까지만
/// 올려 두면 정확히 마지막 하나가 pending이 된다.
fn last_two_versions() -> (i64, i64) {
    let mut versions: Vec<i64> = sqlx::migrate!("./migrations")
        .iter()
        .filter(|m| !m.migration_type.is_down_migration())
        .map(|m| m.version)
        .collect();
    versions.sort_unstable();
    let last = *versions.last().expect("migrations must not be empty");
    let previous = versions[versions.len() - 2];
    (previous, last)
}

/// `migrate!()`가 만든 마이그레이터에서 `max_version` 이하만 남긴 사본.
///
/// 원본은 전부-아니면-전무라서 중간 상태를 만들 수 없다. `Migrator`의 필드는
/// `migrate!()` 매크로가 초기화할 수 있도록 공개돼 있으므로(sqlx-core
/// `migrate/migrator.rs`의 주석), 잘라낸 사본을 조립하면 **sqlx 자신의 원장
/// (`_sqlx_migrations`)과 체크섬을 그대로 갖춘** 부분 적용 상태를 만들 수
/// 있다. `_sqlx_migrations` 행을 손으로 위조하는 것과 달리, 이후의 진짜
/// `migrate()` 호출이 이 상태를 정상적인 것으로 읽는다.
fn migrator_up_to(max_version: i64) -> Migrator {
    let kept: Vec<Migration> = sqlx::migrate!("./migrations")
        .iter()
        .filter(|m| m.version <= max_version)
        .cloned()
        .collect();
    Migrator {
        migrations: Cow::Owned(kept),
        ..Migrator::DEFAULT
    }
}

async fn relation_exists(pool: &PgPool, name: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT to_regclass($1) IS NOT NULL")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("to_regclass lookup must succeed")
}

/// `postgres://user@host/db?opts`의 데이터베이스 이름만 교체한다.
fn with_database(url: &str, database: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let (root, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    match query {
        Some(query) => format!("{root}/{database}?{query}"),
        None => format!("{root}/{database}"),
    }
}

/// 마이그레이션 단계를 자유롭게 통제하기 위한 빈 임시 데이터베이스.
struct TempDatabase {
    admin: PgPool,
    name: String,
    url: String,
}

impl TempDatabase {
    async fn create() -> Option<Self> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but connection failed: {e}"));
        let name = format!(
            "fleet_guard_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        sqlx::query(&format!("CREATE DATABASE \"{name}\""))
            .execute(&admin)
            .await
            .unwrap_or_else(|e| panic!("failed to create temporary database {name}: {e}"));
        let temp_url = with_database(&url, &name);
        Some(Self {
            admin,
            name,
            url: temp_url,
        })
    }

    async fn connect(&self) -> PgPool {
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&self.url)
            .await
            .unwrap_or_else(|e| panic!("failed to connect to temporary database: {e}"))
    }

    /// 테스트가 panic하면 남을 수 있으나 이름에 pid/타임스탬프가 들어가므로
    /// 다음 실행과 충돌하지 않는다.
    async fn drop_database(self, pool: PgPool) {
        pool.close().await;
        let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{}\"", self.name))
            .execute(&self.admin)
            .await;
    }
}

/// 더 새 바이너리를 든 standby가 살아 있는 primary 밑에서 스키마를 바꾸려는
/// 경우 — 이번 변경이 닫는 구멍.
#[tokio::test]
async fn migration_is_refused_while_another_instance_holds_a_live_lease() {
    let Some(temp) = TempDatabase::create().await else {
        return;
    };
    let pool = temp.connect().await;
    let (previous, last) = last_two_versions();

    // DB는 마지막 직전까지만. 021(control_plane_lease)은 이미 들어 있다.
    migrator_up_to(previous)
        .run(&pool)
        .await
        .expect("partial migration must succeed");
    assert!(
        relation_exists(&pool, "control_plane_lease").await,
        "partial migration must already include 021"
    );
    assert!(
        !relation_exists(&pool, MIGRATION_025_INDEX).await,
        "the last migration must not be applied yet"
    );

    let store = PgStore::from_pool(pool.clone());
    // 살아 있는 primary.
    store
        .acquire_control_lease("guard-cluster", "primary-1", StdDuration::from_secs(60))
        .await
        .expect("first acquire must succeed");

    let err = store
        .migrate()
        .await
        .expect_err("migrate() must refuse to change the schema under a live lease");
    let msg = err.to_string();
    assert!(
        msg.contains("refusing to apply migrations"),
        "unexpected error: {msg}"
    );
    assert!(
        msg.contains(&last.to_string()),
        "error must name the pending version {last}: {msg}"
    );
    assert!(
        msg.contains("primary-1"),
        "error must name the lease holder so the operator knows what to stop: {msg}"
    );
    assert!(
        msg.contains("guard-cluster"),
        "error must name the cluster: {msg}"
    );

    // 거절이 말뿐이 아니어야 한다 — 실제로 적용되지 않았는지 확인.
    assert!(
        !relation_exists(&pool, MIGRATION_025_INDEX).await,
        "a refused migration must not have applied anything"
    );

    temp.drop_database(pool).await;
}

/// 정상 종료한 primary는 즉시 길을 비켜야 한다. `release_control_lease`가
/// 행을 지우지 않고 `expires_at = NOW()`로 만들기 때문에, TTL을 기다릴 필요가
/// 없다 — 이것이 성립하지 않으면 계획된 업그레이드마다 최대 TTL만큼 막힌다.
#[tokio::test]
async fn migration_proceeds_immediately_after_the_holder_releases() {
    let Some(temp) = TempDatabase::create().await else {
        return;
    };
    let pool = temp.connect().await;
    let (previous, _last) = last_two_versions();
    migrator_up_to(previous)
        .run(&pool)
        .await
        .expect("partial migration must succeed");

    let store = PgStore::from_pool(pool.clone());
    let lease = store
        .acquire_control_lease("guard-cluster", "primary-1", StdDuration::from_secs(60))
        .await
        .expect("first acquire must succeed");
    assert!(store.migrate().await.is_err(), "live lease must block");

    let released = store
        .release_control_lease("guard-cluster", "primary-1", lease.epoch)
        .await
        .expect("release must succeed");
    assert!(released, "release must affect the row");

    store
        .migrate()
        .await
        .expect("a released lease must not block the upgrade at all");
    assert!(
        relation_exists(&pool, MIGRATION_025_INDEX).await,
        "the pending migration must actually have been applied"
    );

    temp.drop_database(pool).await;
}

/// **운영 함정 방지** — 적용할 것이 없으면 lease가 살아 있어도 통과해야 한다.
/// 평범한 재기동과 동일 버전 standby 기동이 여기에 해당하며, 이쪽을 막으면
/// 게이트가 클러스터를 통째로 세운다.
#[tokio::test]
async fn a_live_lease_does_not_block_a_migration_that_changes_nothing() {
    let Some(temp) = TempDatabase::create().await else {
        return;
    };
    let pool = temp.connect().await;

    let store = PgStore::from_pool(pool.clone());
    store
        .migrate()
        .await
        .expect("initial migration must succeed");

    store
        .acquire_control_lease("guard-cluster", "primary-1", StdDuration::from_secs(60))
        .await
        .expect("first acquire must succeed");

    // 같은 바이너리의 standby가 기동하는 상황 — pending이 없으므로 통과.
    store
        .migrate()
        .await
        .expect("an up-to-date binary must start even while another instance holds the lease");

    temp.drop_database(pool).await;
}

/// 반대 방향(DB가 바이너리보다 앞섬)은 sqlx가 이미 막는다. 이 프로젝트가
/// `set_ignore_missing(true)`를 호출하지 않는 한 성립하는 보장이며, 지금까지
/// 어느 테스트도 그것을 고정하지 않았다.
#[tokio::test]
async fn db_ahead_of_binary_is_refused_by_sqlx_itself() {
    let Some(temp) = TempDatabase::create().await else {
        return;
    };
    let pool = temp.connect().await;
    let store = PgStore::from_pool(pool.clone());
    store
        .migrate()
        .await
        .expect("initial migration must succeed");

    // 더 새 바이너리가 남기고 간 원장 행 — 옛 바이너리로 롤백한 배포의 모습.
    sqlx::query(
        "INSERT INTO _sqlx_migrations \
             (version, description, installed_on, success, checksum, execution_time) \
         VALUES (99999, 'left behind by a newer binary', NOW(), TRUE, '\\x00'::bytea, 0)",
    )
    .execute(&pool)
    .await
    .expect("ledger insert must succeed");

    let err = store
        .migrate()
        .await
        .expect_err("a binary older than the database must refuse to start");
    let msg = err.to_string();
    assert!(
        msg.contains("99999"),
        "sqlx must name the version it cannot resolve: {msg}"
    );

    temp.drop_database(pool).await;
}
