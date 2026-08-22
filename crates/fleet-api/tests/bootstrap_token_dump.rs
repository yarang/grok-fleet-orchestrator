//! Bootstrap token 원문 미저장 회귀 테스트 (로드맵 #59).
//!
//! 신선한 PostgreSQL 데이터베이스에 마이그레이션을 전부 적용하고, HTTP API로
//! bootstrap token을 발급한 뒤 `pg_dump`(schema + data) 출력에 **발급된 원문
//! 문자열이 전혀 나타나지 않는지** 확인한다. 저장소에는 SHA-256 digest만
//! 남아야 하므로, DB 백업이 유출되어도 join 토큰을 재사용할 수 없다.
//!
//! join으로 발급되는 worker operational token(`fwo_...`)도 같은 성질을
//! 가져야 하므로 함께 검사한다.
//!
//! 실제 PostgreSQL이 필요하다. `DATABASE_URL`이 설정되지 않으면 skip하고,
//! 설정돼 있는데 연결/마이그레이션이 실패하면 panic한다 (fleet-store 통합
//! 테스트와 동일한 규약). `pg_dump` 바이너리가 없으면 그 사실을 출력하고
//! skip한다.
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-api --test bootstrap_token_dump -- --test-threads=1
//! ```

use std::process::Command;
use std::sync::Arc;

use fleet_api::{build_app, AppState};
use fleet_store::{PgStore, Store};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;

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

/// 다른 테스트 데이터가 섞이지 않은 신선한 데이터베이스.
struct FreshDatabase {
    admin: PgPool,
    name: String,
    url: String,
}

impl FreshDatabase {
    async fn create() -> Option<Self> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but connection failed: {e}"));
        let name = format!(
            "fleet_dump_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        sqlx::query(&format!("CREATE DATABASE \"{name}\""))
            .execute(&admin)
            .await
            .unwrap_or_else(|e| panic!("failed to create temporary database {name}: {e}"));
        let fresh_url = with_database(&url, &name);
        Some(Self {
            admin,
            name,
            url: fresh_url,
        })
    }

    async fn connect(&self) -> PgPool {
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&self.url)
            .await
            .unwrap_or_else(|e| panic!("failed to connect to temporary database: {e}"))
    }

    async fn drop_database(self) {
        let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{}\"", self.name))
            .execute(&self.admin)
            .await;
    }
}

/// `pg_dump`로 스키마 + 데이터를 텍스트로 받는다.
///
/// 바이너리가 없으면 `None`(skip). 있는데 실패하면 panic — 조용한 초록불로
/// 회귀를 숨기지 않는다.
fn pg_dump(url: &str) -> Option<String> {
    let output = match Command::new("pg_dump").arg("--dbname").arg(url).output() {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("pg_dump binary not found — skipping dump regression test");
            return None;
        }
        Err(e) => panic!("failed to run pg_dump: {e}"),
    };
    assert!(
        output.status.success(),
        "pg_dump failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// API 호출 헬퍼 — 인증 없이(개발 모드) 라우터에 직접 요청한다.
async fn api_call(
    store: Arc<dyn Store>,
    method: axum::http::Method,
    path: &str,
    body: serde_json::Value,
) -> (axum::http::StatusCode, serde_json::Value) {
    let app = build_app(Arc::new(AppState::new(store)));
    let request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn issued_bootstrap_token_never_appears_in_a_database_dump() {
    let Some(fresh) = FreshDatabase::create().await else {
        return;
    };
    let pool = fresh.connect().await;
    let store = PgStore::from_pool(pool.clone());

    // 001~018 전체 마이그레이션을 신선한 DB에 적용.
    store
        .migrate()
        .await
        .unwrap_or_else(|e| panic!("migration on a fresh database failed: {e}"));

    let store: Arc<dyn Store> = Arc::new(store);

    // API로 bootstrap token 발급.
    let (status, issued) = api_call(
        store.clone(),
        axum::http::Method::POST,
        "/v1/bootstrap-tokens",
        serde_json::json!({"prefix": "fleet", "bytes": 32, "max_uses": 1, "notes": "dump regression"}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let raw_token = issued["token"]
        .as_str()
        .expect("token in response")
        .to_string();
    let token_id = issued["token_id"]
        .as_str()
        .expect("token_id in response")
        .to_string();
    let digest = token_id
        .strip_prefix("bt_")
        .expect("token_id must be bt_<digest>")
        .to_string();

    // 발급된 토큰으로 join하여 worker operational token까지 만들어 둔다.
    let (status, joined) = api_call(
        store.clone(),
        axum::http::Method::POST,
        "/v1/workers/join",
        serde_json::json!({
            "token": raw_token,
            "name": "dump-regression-worker",
            "agent_endpoint": "ws://dump-worker.local:2419/ws?server-key=sekret",
        }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let worker_config = joined["worker_config_toml"]
        .as_str()
        .expect("worker_config_toml in response")
        .to_string();
    let operational_token = worker_config
        .split("operational_token = \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("join response must carry an operational token")
        .to_string();
    assert!(operational_token.starts_with("fwo_"));

    let Some(dump) = pg_dump(&fresh.url) else {
        pool.close().await;
        fresh.drop_database().await;
        return;
    };

    // 핵심 단언 — 원문은 스키마에도 데이터에도 없다.
    assert!(
        !dump.contains(&raw_token),
        "raw bootstrap token leaked into the database dump"
    );
    assert!(
        !dump.contains(&operational_token),
        "raw worker operational token leaked into the database dump"
    );

    // 반대로 digest는 남아 있어야 한다 — 덤프를 제대로 떴는지에 대한 대조군.
    // (토큰 행 자체가 없으면 위 단언은 자동으로 통과해버린다.)
    assert!(
        dump.contains(&digest),
        "bootstrap token digest is missing from the dump — the dump may be empty"
    );
    assert!(
        dump.contains("dump-regression-worker"),
        "joined worker row is missing from the dump — the dump may be empty"
    );
    assert!(
        dump.contains("token_digest"),
        "bootstrap_tokens.token_digest column is missing from the dumped schema"
    );

    // 덤프 밖에서도 원문이 남지 않았는지 직접 확인.
    let stored: String = sqlx::query_scalar("SELECT token_digest FROM bootstrap_tokens")
        .fetch_one(&pool)
        .await
        .expect("issued token row must exist");
    assert_eq!(stored, digest);
    assert_ne!(stored, raw_token);

    pool.close().await;
    fresh.drop_database().await;
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
