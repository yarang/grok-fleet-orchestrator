//! 017 마이그레이션 회귀 테스트 — 기존 운영 bootstrap token 이관 (로드맵 #57).
//!
//! `017_bootstrap_token_digest.sql`은 원문(plaintext)을 primary key로 쓰던
//! `bootstrap_tokens.token` 컬럼을 `token_digest`로 rename한 뒤, 이미 저장돼
//! 있던 값을 `digest(token_digest, 'sha256')`으로 원자 치환한다. 따라서
//! **마이그레이션 이전에 발급되어 이미 워커에 배포된 토큰 원문**은
//! 업그레이드 이후에도 그대로 `consume_bootstrap_token` 인증에 성공해야 한다.
//!
//! 이 테스트는 그 전제를 실제 PostgreSQL에서 증명한다:
//!
//! 1. 임시 데이터베이스를 만들고 016까지만 마이그레이션을 적용한다.
//! 2. 옛 스키마(`token TEXT PRIMARY KEY`)에 원문 토큰 행을 직접 INSERT한다.
//! 3. 017/018을 적용한다.
//! 4. 원래 원문으로 `consume_bootstrap_token`이 성공하는지 확인한다.
//!
//! 실제 PostgreSQL이 필요하다. `DATABASE_URL`이 설정되지 않으면 skip하고,
//! 설정돼 있는데 연결/마이그레이션이 실패하면 panic한다 (`tests/integration.rs`
//! 와 동일한 규약).
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --test bootstrap_token_migration -- --test-threads=1
//! ```

use fleet_core::BootstrapToken;
use fleet_store::{PgStore, Store};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

/// 마이그레이션 순서를 지키기 위해 파일 이름 앞의 숫자를 파싱한다.
fn migration_number(file_name: &str) -> Option<u32> {
    let digits: String = file_name
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// `migrations/` 디렉터리의 SQL을 번호 순으로 `range` 안의 것만 적용한다.
///
/// `PgStore::migrate()`(=`sqlx::migrate!`)는 전부 아니면 전무라서 "016까지만"
/// 이라는 중간 상태를 만들 수 없다. 이 테스트만을 위해 파일을 직접 실행하고,
/// `_sqlx_migrations` 원장은 건드리지 않는다(이 임시 DB에서 이후에
/// `migrate()`를 호출하지 않기 때문에 불일치가 문제되지 않는다).
async fn apply_migrations(pool: &PgPool, range: std::ops::RangeInclusive<u32>) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("migrations directory must be readable")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    files.sort();

    for path in files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("migration file name must be UTF-8");
        let Some(number) = migration_number(name) else {
            continue;
        };
        if !range.contains(&number) {
            continue;
        }
        let sql = std::fs::read_to_string(&path).expect("migration file must be readable");
        sqlx::raw_sql(&sql)
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
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
    assert!(
        root.matches('/').count() >= 2,
        "DATABASE_URL must be of the form scheme://host/database (got {url})"
    );
    match query {
        Some(query) => format!("{root}/{database}?{query}"),
        None => format!("{root}/{database}"),
    }
}

/// 마이그레이션 단계를 자유롭게 통제하기 위한 빈 임시 데이터베이스.
///
/// 공용 `fleet_test` 데이터베이스는 이미 최신 스키마가 적용돼 있어서
/// "016 시점의 스키마"를 재현할 수 없다.
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
            "fleet_mig_{}_{}",
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

    /// 임시 데이터베이스 삭제. 테스트가 panic하면 남을 수 있으나 이름에
    /// pid/타임스탬프가 들어가므로 다음 실행과 충돌하지 않는다.
    async fn drop_database(self) {
        let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{}\"", self.name))
            .execute(&self.admin)
            .await;
    }
}

#[tokio::test]
async fn legacy_plaintext_token_still_authenticates_after_digest_migration() {
    let Some(temp) = TempDatabase::create().await else {
        return;
    };
    let pool = temp.connect().await;

    // 1) 017 이전 상태(016까지)를 재현.
    apply_migrations(&pool, 1..=16).await;

    // 옛 스키마 확인 — 원문 컬럼 `token`이 존재해야 한다.
    let legacy_column: Option<String> =
        sqlx::query_scalar("SELECT column_name FROM information_schema.columns WHERE table_name = 'bootstrap_tokens' AND column_name = 'token'")
            .fetch_optional(&pool)
            .await
            .expect("column lookup must succeed");
    assert_eq!(
        legacy_column.as_deref(),
        Some("token"),
        "migrations up to 016 must still store the raw token column"
    );

    // 2) 마이그레이션 이전에 발급되어 이미 워커에 배포된 토큰(원문 저장).
    let legacy_raw_token = "fleet_legacy_operational_token_9f3c";
    sqlx::query(
        "INSERT INTO bootstrap_tokens (token, created_by, max_uses, use_count, notes) \
         VALUES ($1, $2, $3, 0, $4)",
    )
    .bind(legacy_raw_token)
    .bind("pre-upgrade-admin")
    .bind(3_i32)
    .bind("issued before 017")
    .execute(&pool)
    .await
    .expect("legacy plaintext token insert must succeed");

    // 3) 017(digest 전환) + 018 적용.
    apply_migrations(&pool, 17..=u32::MAX).await;

    // 저장된 값은 더 이상 원문이 아니어야 한다.
    let stored: String = sqlx::query("SELECT token_digest FROM bootstrap_tokens")
        .fetch_one(&pool)
        .await
        .expect("row must survive the migration")
        .get("token_digest");
    assert_ne!(stored, legacy_raw_token, "raw token must not survive 017");
    assert_eq!(
        stored,
        BootstrapToken::digest_for(legacy_raw_token),
        "017 must replace the raw token with its SHA-256 digest"
    );

    // 메타데이터는 보존되어야 한다 (회수/감사 이력 유실 금지).
    let row = sqlx::query("SELECT created_by, max_uses, use_count, notes FROM bootstrap_tokens")
        .fetch_one(&pool)
        .await
        .expect("row must survive the migration");
    assert_eq!(row.get::<String, _>("created_by"), "pre-upgrade-admin");
    assert_eq!(row.get::<i32, _>("max_uses"), 3);
    assert_eq!(row.get::<i32, _>("use_count"), 0);
    assert_eq!(row.get::<String, _>("notes"), "issued before 017");

    // 4) 업그레이드 후에도 원래 원문으로 join 인증이 성공해야 한다.
    let store = PgStore::from_pool(pool.clone());
    store
        .consume_bootstrap_token(legacy_raw_token, "worker-upgraded-in-place")
        .await
        .expect("pre-migration token must keep working after 017");

    let after = store
        .list_bootstrap_tokens()
        .await
        .expect("list must succeed");
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].use_count, 1);
    assert_eq!(
        after[0].last_used_by.as_deref(),
        Some("worker-upgraded-in-place")
    );
    assert_eq!(
        after[0].public_id(),
        BootstrapToken::public_id_for(legacy_raw_token),
        "public identifier must be derivable from the pre-migration raw token"
    );

    // 원문이 틀리면 여전히 거부되어야 한다 (digest 치환이 검증을 무력화하지 않음).
    let wrong = store
        .consume_bootstrap_token("fleet_legacy_operational_token_9f3d", "impostor")
        .await;
    assert!(wrong.is_err(), "unknown token must still be rejected");

    pool.close().await;
    temp.drop_database().await;
}

#[test]
fn migration_number_parses_prefix() {
    assert_eq!(migration_number("017_bootstrap_token_digest.sql"), Some(17));
    assert_eq!(migration_number("001_init.sql"), Some(1));
    assert_eq!(migration_number("readme.md"), None);
}

#[test]
fn with_database_replaces_only_the_database_segment() {
    assert_eq!(
        with_database("postgres://me@localhost/fleet_test", "tmp"),
        "postgres://me@localhost/tmp"
    );
    assert_eq!(
        with_database(
            "postgres://u:p@127.0.0.1:5432/fleet_test?sslmode=disable",
            "tmp"
        ),
        "postgres://u:p@127.0.0.1:5432/tmp?sslmode=disable"
    );
}
