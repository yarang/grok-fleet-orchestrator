//! `insert_task_idempotent`의 백엔드 공유 행동 테스트 (로드맵 #62 stage 2, 게이트 3).
//!
//! 정본 계약(`docs/architecture/tasks/execution-consistency.md`)은 이렇게 말한다:
//! "동일 principal, 동일 key, 동일 hash의 재요청은 기존 Task를 반환한다. 같은
//! key에 다른 payload가 오면 409 Conflict로 거부한다."
//!
//! 실제로 막고 싶은 사고는 이것이다 — 클라이언트가 submit을 보냈는데 응답이
//! 타임아웃으로 유실됐고, 같은 요청을 그대로 재전송한다. 그 사이 원래 Task는
//! 이미 실행을 끝냈을 수도 있다. 두 번째 요청은 **새 Task를 만들면 안 되고**,
//! 이미 끝난 그 Task를 그대로 돌려줘야 한다. 그래서 아래 시나리오 중 하나는
//! 일부러 종료 상태(`Completed`)의 Task에 재제출을 건다.
//!
//! 두 백엔드는 이 계약을 서로 다른 방법으로 구현한다 — PgStore는 부분 유니크
//! 인덱스(`WHERE idempotency_key IS NOT NULL`)와 `ON CONFLICT DO NOTHING`으로,
//! MemStore는 락 안의 선형 탐색으로. 특히 **NULL 취급**이 갈라지기 쉽다:
//! Postgres는 유니크 인덱스에서 NULL을 서로 다른 값으로 보므로 키 없는 제출은
//! 몇 번을 해도 충돌하지 않는다. MemStore가 이걸 흉내내지 않으면 여기서만
//! 통과하는 코드가 생기므로, `no_key_never_deduplicates`가 그 지점을 고정한다.
//!
//! `DATABASE_URL`이 없으면 PgStore 케이스만 skip되고 MemStore 케이스는 항상
//! 실행된다.
//!
//! ## 실행 방법
//!
//! ```bash
//! DATABASE_URL=postgres://$(whoami)@localhost/fleet_test \
//!     cargo test -p fleet-store --all-features --test task_idempotency -- --test-threads=1
//! ```
//!
//! `--all-features` 없이 돌리면 `MemStore`(`test-support` 피처)가 사라진다.

use std::sync::Arc;

use chrono::Utc;
use fleet_core::{
    IdempotentInsert, Task, TaskFilter, TaskRequest, TaskResult, TaskStatus, WorkerId,
};
use fleet_store::mem::MemStore;
use fleet_store::{PgStore, Store};
use sqlx::postgres::PgPoolOptions;

// ── 백엔드 준비 ─────────────────────────────────────────────────────────

async fn mem_backend() -> Arc<dyn Store> {
    Arc::new(MemStore::new())
}

/// `DATABASE_URL`이 없으면 `None` — 호출부가 skip한다.
async fn pg_backend() -> Option<Arc<dyn Store>> {
    let url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but connection failed: {e}"));
    let store = PgStore::from_pool(pool);
    store
        .migrate()
        .await
        .unwrap_or_else(|e| panic!("DATABASE_URL={url} set but migration failed: {e}"));
    sqlx::query("TRUNCATE issue_task_links, issue_comments, issues, tasks, projects CASCADE")
        .execute(store.pool())
        .await
        .expect("truncate");
    Some(Arc::new(store))
}

/// 두 백엔드에 같은 시나리오를 돌린다.
macro_rules! both_backends {
    ($name:ident, $body:expr) => {
        #[tokio::test]
        async fn $name() {
            let scenario: fn(Arc<dyn Store>) -> _ = $body;
            scenario(mem_backend().await).await;
            if let Some(pg) = pg_backend().await {
                scenario(pg).await;
            }
        }
    };
}

// ── 픽스처 ──────────────────────────────────────────────────────────────

/// 클라이언트가 보낸 요청 하나. 같은 인자로 두 번 부르면 **같은 제출의
/// 재전송**을 의미한다 — Task id만 새로 뽑히고 payload hash는 같아야 한다.
fn request(prompt: &str, created_by: &str, key: Option<&str>) -> TaskRequest {
    TaskRequest {
        prompt: prompt.into(),
        created_by: created_by.into(),
        idempotency_key: key.map(String::from),
        ..Default::default()
    }
}

fn submission(prompt: &str, created_by: &str, key: Option<&str>) -> Task {
    Task::from_request(request(prompt, created_by, key))
}

async fn task_count(store: &Arc<dyn Store>) -> usize {
    store
        .list_tasks(&TaskFilter::default())
        .await
        .expect("list_tasks")
        .len()
}

// ── 게이트 3: 재제출이 중복 Task를 만들지 않는다 ────────────────────────

both_backends!(
    same_key_same_payload_returns_the_original,
    |store| async move {
        let first = submission("build", "alice", Some("k-1"));
        assert!(
            store
                .insert_task_idempotent(&first)
                .await
                .unwrap()
                .inserted(),
            "첫 제출은 삽입돼야 한다"
        );

        // 같은 요청의 재전송 — Task id는 새로 뽑히지만 payload hash는 같다.
        let retry = submission("build", "alice", Some("k-1"));
        assert_ne!(retry.id, first.id, "재전송은 새 Task 객체로 만들어진다");
        assert_eq!(
            retry.idempotency_payload_hash, first.idempotency_payload_hash,
            "같은 요청이면 payload hash가 같아야 한다 — 다르면 아래 단정이 무의미해진다"
        );

        match store.insert_task_idempotent(&retry).await.unwrap() {
            IdempotentInsert::Duplicate(existing) => {
                assert_eq!(existing.id, first.id, "원래 Task를 그대로 돌려줘야 한다");
            }
            other => panic!("Duplicate를 기대했는데 {other:?}"),
        }

        assert_eq!(task_count(&store).await, 1, "행이 하나만 남아야 한다");
    }
);

// 게이트 3이 진짜로 막으려는 시나리오: 응답이 유실된 사이 원래 Task가 이미
// 끝나 있는 경우. 재제출은 그 종료된 Task를 돌려줘야지, 같은 일을 한 번 더
// 실행시켜서는 안 된다.
both_backends!(
    retry_after_completion_returns_the_finished_task,
    |store| async move {
        let first = submission("deploy", "alice", Some("k-terminal"));
        store.insert_task_idempotent(&first).await.unwrap();

        let worker = WorkerId::new();
        let done = TaskStatus::Completed(TaskResult {
            output: "ok".into(),
            exit_code: 0,
            duration_secs: 1.0,
            token_usage: None,
            worker_id: worker,
            finished_at: Utc::now(),
        });
        store.update_task_status(first.id, &done).await.unwrap();

        let retry = submission("deploy", "alice", Some("k-terminal"));
        match store.insert_task_idempotent(&retry).await.unwrap() {
            IdempotentInsert::Duplicate(existing) => {
                assert_eq!(existing.id, first.id);
                assert!(
                    existing.is_terminal(),
                    "저장된 현재 상태를 읽어야 한다 — 삽입 시점의 Pending 사본이 아니라"
                );
            }
            other => panic!("Duplicate를 기대했는데 {other:?}"),
        }

        assert_eq!(
            task_count(&store).await,
            1,
            "재실행용 행이 새로 생기면 안 된다"
        );
    }
);

both_backends!(same_key_different_payload_conflicts, |store| async move {
    let first = submission("build", "alice", Some("k-2"));
    store.insert_task_idempotent(&first).await.unwrap();

    // 같은 키인데 프롬프트가 다르다 — 클라이언트가 키를 재사용한 버그다.
    let clashing = submission("rm -rf /", "alice", Some("k-2"));
    match store.insert_task_idempotent(&clashing).await.unwrap() {
        IdempotentInsert::Conflict { existing_task_id } => {
            assert_eq!(existing_task_id, first.id);
        }
        other => panic!("Conflict를 기대했는데 {other:?}"),
    }

    assert_eq!(
        task_count(&store).await,
        1,
        "거절된 제출이 행을 남기면 안 된다"
    );
});

// ── 유일성의 범위 ───────────────────────────────────────────────────────

// 키 네임스페이스는 `created_by` 단위다. 서로 다른 제출자가 우연히 같은 문자열
// 키를 골랐다고 해서 한쪽 요청이 삼켜지면 안 된다.
both_backends!(key_namespace_is_scoped_to_the_creator, |store| async move {
    let alice = submission("build", "alice", Some("shared"));
    let bob = submission("build", "bob", Some("shared"));

    assert!(store
        .insert_task_idempotent(&alice)
        .await
        .unwrap()
        .inserted());
    assert!(
        store.insert_task_idempotent(&bob).await.unwrap().inserted(),
        "다른 제출자의 같은 키는 충돌이 아니다"
    );

    assert_eq!(task_count(&store).await, 2);
});

// 키 없는 제출은 절대 합쳐지지 않는다. Postgres가 유니크 인덱스에서 NULL을
// 서로 다른 값으로 보는 것과 같은 규칙이며, MemStore도 이 규칙을 따라야 한다.
both_backends!(no_key_never_deduplicates, |store| async move {
    let first = submission("build", "alice", None);
    let second = submission("build", "alice", None);
    assert!(
        first.idempotency_payload_hash.is_none(),
        "키가 없으면 해시도 없다"
    );

    assert!(store
        .insert_task_idempotent(&first)
        .await
        .unwrap()
        .inserted());
    assert!(
        store
            .insert_task_idempotent(&second)
            .await
            .unwrap()
            .inserted(),
        "같은 내용이라도 키가 없으면 별개의 제출이다"
    );

    assert_eq!(task_count(&store).await, 2);
});

// 서로 다른 키는 페이로드가 같아도 각각 별개의 Task다 — 의도적인 재실행이다.
both_backends!(different_keys_are_independent, |store| async move {
    assert!(store
        .insert_task_idempotent(&submission("build", "alice", Some("a")))
        .await
        .unwrap()
        .inserted());
    assert!(store
        .insert_task_idempotent(&submission("build", "alice", Some("b")))
        .await
        .unwrap()
        .inserted());

    assert_eq!(task_count(&store).await, 2);
});
