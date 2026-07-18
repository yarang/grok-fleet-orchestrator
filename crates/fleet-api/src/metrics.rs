//! Prometheus 메트릭 익스포터.
//!
//! `/metrics` 엔드포인트에서 Prometheus 텍스트 포맷을 반환합니다.
//! 모든 메트릭은 스크랩 시점에 Store에서 즉시 집계되므로 별도의 글로벌
//! 레지스트리나 백그라운드 수집 스레드가 필요 없습니다.
//!
//! ## 노출 메트릭
//!
//! | 이름                                  | 유형    | 라벨            | 의미                              |
//! |---------------------------------------|---------|-----------------|-----------------------------------|
//! | `fleet_up`                            | gauge   | —               | 항상 1 (스크랩 성공 표시)          |
//! | `fleet_workers_total`                 | gauge   | status          | 상태별 워커 수                    |
//! | `fleet_workers_capacity_total`        | gauge   | —               | 모든 워커의 max_concurrent 합계    |
//! | `fleet_workers_active_tasks_total`    | gauge   | —               | 현재 실행 중인 작업 수 합계       |
//! | `fleet_tasks_total`                   | gauge   | phase           | 위상별 작업 수                    |
//! | `fleet_events_written_total`          | gauge   | —               | 가장 최근 이벤트 seq (단조 증가)  |

use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use fleet_core::{TaskFilter, TaskStatus, WorkerFilter, WorkerStatus};
use fleet_store::Store;
use tracing::debug;

use crate::app::AppState;

/// `/metrics` 핸들러. Prometheus 표준 text 포맷 (`text/plain; version=0.0.4`) 반환.
///
/// 인증 없이 노출되지만, Cloudflare Access 미들웨어가 활성화된 경우에는
/// CF-Access-Jwt-Assertion 검증을 받습니다. 외부망 노출 시 `--cf-audience`
/// 설정을 권장합니다.
pub async fn metrics_handler(state: Arc<AppState>) -> Response {
    match metrics_text(state.store.as_ref()).await {
        Ok(body) => (
            [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
            body,
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = ?e, "metrics scrape failed");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("metrics error: {e}"),
            )
                .into_response()
        }
    }
}

/// Prometheus 텍스트 포맷을 생성.
pub async fn metrics_text(store: &dyn Store) -> Result<String, MetricsError> {
    let workers = store
        .list_workers(&WorkerFilter {
            limit: 10_000,
            ..Default::default()
        })
        .await?;

    let tasks = store
        .list_tasks(&TaskFilter {
            limit: 10_000,
            ..Default::default()
        })
        .await?;

    let events = store.list_events(0, 1).await?;

    let mut w_counts = WorkerCounts::default();
    let mut capacity: u64 = 0;
    let mut active: u64 = 0;
    for w in &workers {
        w_counts.total += 1;
        capacity += w.max_concurrent as u64;
        active += w.active_tasks as u64;
        match w.status {
            WorkerStatus::Online => w_counts.online += 1,
            WorkerStatus::Degraded => w_counts.degraded += 1,
            WorkerStatus::Offline => w_counts.offline += 1,
            WorkerStatus::CircuitOpen => w_counts.circuit_open += 1,
        }
    }

    let mut t_counts = TaskCounts::default();
    for t in &tasks {
        t_counts.total += 1;
        match &t.status {
            TaskStatus::Pending => t_counts.pending += 1,
            TaskStatus::Dispatched { .. } => t_counts.dispatched += 1,
            TaskStatus::Completed(_) => t_counts.completed += 1,
            TaskStatus::Failed(_) => t_counts.failed += 1,
            TaskStatus::Cancelled { .. } => t_counts.cancelled += 1,
        }
    }

    let last_seq = events.last().map(|e| e.seq).unwrap_or(0);

    let mut out = String::with_capacity(2048);

    // fleet_up
    out.push_str("# HELP fleet_up Liveness indicator (always 1 if scrape succeeded).\n");
    out.push_str("# TYPE fleet_up gauge\n");
    out.push_str("fleet_up 1\n\n");

    // fleet_workers_total{status}
    out.push_str("# HELP fleet_workers_total Number of workers by status.\n");
    out.push_str("# TYPE fleet_workers_total gauge\n");
    push_gauge(&mut out, "fleet_workers_total", &[("status", "online")], w_counts.online);
    push_gauge(&mut out, "fleet_workers_total", &[("status", "degraded")], w_counts.degraded);
    push_gauge(&mut out, "fleet_workers_total", &[("status", "offline")], w_counts.offline);
    push_gauge(
        &mut out,
        "fleet_workers_total",
        &[("status", "circuit_open")],
        w_counts.circuit_open,
    );
    push_gauge(&mut out, "fleet_workers_total", &[("status", "total")], w_counts.total);
    out.push('\n');

    // fleet_workers_capacity_total
    out.push_str("# HELP fleet_workers_capacity_total Sum of max_concurrent across all workers.\n");
    out.push_str("# TYPE fleet_workers_capacity_total gauge\n");
    push_gauge(&mut out, "fleet_workers_capacity_total", &[], capacity);
    out.push('\n');

    // fleet_workers_active_tasks_total
    out.push_str(
        "# HELP fleet_workers_active_tasks_total Sum of currently active tasks across workers.\n",
    );
    out.push_str("# TYPE fleet_workers_active_tasks_total gauge\n");
    push_gauge(&mut out, "fleet_workers_active_tasks_total", &[], active);
    out.push('\n');

    // fleet_tasks_total{phase}
    out.push_str("# HELP fleet_tasks_total Number of tasks by lifecycle phase.\n");
    out.push_str("# TYPE fleet_tasks_total gauge\n");
    push_gauge(&mut out, "fleet_tasks_total", &[("phase", "pending")], t_counts.pending);
    push_gauge(&mut out, "fleet_tasks_total", &[("phase", "dispatched")], t_counts.dispatched);
    push_gauge(&mut out, "fleet_tasks_total", &[("phase", "completed")], t_counts.completed);
    push_gauge(&mut out, "fleet_tasks_total", &[("phase", "failed")], t_counts.failed);
    push_gauge(&mut out, "fleet_tasks_total", &[("phase", "cancelled")], t_counts.cancelled);
    push_gauge(&mut out, "fleet_tasks_total", &[("phase", "total")], t_counts.total);
    out.push('\n');

    // fleet_events_written_total
    out.push_str(
        "# HELP fleet_events_written_total Highest event sequence number observed.\n",
    );
    out.push_str("# TYPE fleet_events_written_total gauge\n");
    push_gauge(&mut out, "fleet_events_written_total", &[], last_seq);

    debug!(workers = workers.len(), tasks = tasks.len(), "metrics rendered");
    Ok(out)
}

/// 게이지 라인을 버퍼에 추가. 라벨이 있으면 `key="val",...` 형태로 출력.
fn push_gauge(out: &mut String, name: &str, labels: &[(&str, &str)], value: u64) {
    out.push_str(name);
    if !labels.is_empty() {
        out.push('{');
        for (i, (k, v)) in labels.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(k);
            out.push_str("=\"");
            out.push_str(v);
            out.push('"');
        }
        out.push('}');
    }
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

#[derive(Debug, thiserror::Error)]
pub enum MetricsError {
    #[error("store error: {0}")]
    Store(#[from] fleet_store::StoreError),
}

#[derive(Default, Debug)]
struct WorkerCounts {
    total: u64,
    online: u64,
    degraded: u64,
    offline: u64,
    circuit_open: u64,
}

#[derive(Default, Debug)]
struct TaskCounts {
    total: u64,
    pending: u64,
    dispatched: u64,
    completed: u64,
    failed: u64,
    cancelled: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MemStore;
    use fleet_core::{Task, TaskRequest, Worker};

    #[tokio::test]
    async fn empty_store_renders_skeleton() {
        let store = MemStore::new_arc();
        let out = metrics_text(store.as_ref()).await.unwrap();
        assert!(out.contains("fleet_up 1"));
        assert!(out.contains("fleet_workers_total{status=\"online\"} 0"));
        assert!(out.contains("fleet_tasks_total{phase=\"pending\"} 0"));
        // 모든 위상이 0으로 출력되어야 함.
        for phase in ["pending", "dispatched", "completed", "failed", "cancelled"] {
            assert!(
                out.contains(&format!("fleet_tasks_total{{phase=\"{phase}\"}} 0")),
                "missing phase={phase}"
            );
        }
    }

    #[tokio::test]
    async fn counts_reflect_store_state() {
        let store = MemStore::new_arc();
        store
            .upsert_worker(&Worker::new("w1", "wss://1"))
            .await
            .unwrap();
        store
            .upsert_worker(&Worker::new("w2", "wss://2"))
            .await
            .unwrap();

        // 두 개의 작업 (pending)
        let t1 = Task::from_request(TaskRequest {
            prompt: "a".into(),
            ..Default::default()
        });
        let t2 = Task::from_request(TaskRequest {
            prompt: "b".into(),
            ..Default::default()
        });
        store.insert_task(&t1).await.unwrap();
        store.insert_task(&t2).await.unwrap();

        let out = metrics_text(store.as_ref()).await.unwrap();
        assert!(out.contains("fleet_workers_total{status=\"online\"} 2"));
        assert!(out.contains("fleet_tasks_total{phase=\"pending\"} 2"));
        assert!(out.contains("fleet_tasks_total{phase=\"total\"} 2"));
        assert!(out.contains("fleet_workers_capacity_total 8")); // 2 * default 4
    }

    #[tokio::test]
    async fn prometheus_format_is_text_v004() {
        let store = MemStore::new_arc();
        let out = metrics_text(store.as_ref()).await.unwrap();
        // HELP/TYPE 라인이 모든 메트릭에 존재해야 함.
        assert!(out.contains("# HELP fleet_up"));
        assert!(out.contains("# TYPE fleet_up gauge"));
        assert!(out.contains("# HELP fleet_workers_total"));
        assert!(out.contains("# TYPE fleet_workers_total gauge"));
        assert!(out.contains("# TYPE fleet_events_written_total gauge"));
    }

    #[tokio::test]
    async fn labels_quote_escaping_safe() {
        // 라벨 값에 큰따옴표/백슬래시가 들어가는 케이스는 현재 메트릭 라벨이
        // 모두 정적이므로 발생하지 않음. 다만 라벨 라인 포맷 검증.
        let store = MemStore::new_arc();
        let out = metrics_text(store.as_ref()).await.unwrap();
        // 정적 라벨은 모두 `key="value"` 형태.
        assert!(out.contains("status=\"online\""));
    }
}
