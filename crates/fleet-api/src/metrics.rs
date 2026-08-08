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
//! | `fleet_task_tokens_total`             | counter | type            | 완료된 작업의 누적 토큰 사용량    |
//! | `fleet_task_duration_seconds`         | histogram | —             | 완료된 작업의 실행 시간 분포      |

use std::sync::atomic::{AtomicU64, Ordering};
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
        Ok(mut body) => {
            // HTTP 지연은 스토어에서 계산할 수 없으므로 인프로세스 누산기에서 붙인다.
            body.push('\n');
            state.http_metrics.render(&mut body);
            (
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; version=0.0.4",
                )],
                body,
            )
                .into_response()
        }
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
    let mut tok_counts = TokenCounts::default();
    let mut duration_hist = Histogram::new(TASK_DURATION_BUCKETS);
    let mut dispatch_latency_hist = Histogram::new(TASK_DISPATCH_LATENCY_BUCKETS);

    for t in &tasks {
        t_counts.total += 1;
        if let Some(dispatched) = t.dispatched_at {
            let latency = ((dispatched - t.created_at).num_milliseconds() as f64 / 1000.0).max(0.0);
            dispatch_latency_hist.observe(latency);
        }

        match &t.status {
            TaskStatus::Pending => t_counts.pending += 1,
            TaskStatus::Dispatched { .. } => t_counts.dispatched += 1,
            TaskStatus::Completed(result) => {
                t_counts.completed += 1;
                duration_hist.observe(result.duration_secs);
                if let Some(usage) = &result.token_usage {
                    tok_counts.input += usage.input_tokens;
                    tok_counts.output += usage.output_tokens;
                    tok_counts.cache_read += usage.cache_read_tokens;
                    tok_counts.total += usage.total();
                }
            }
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
    push_gauge(
        &mut out,
        "fleet_workers_total",
        &[("status", "online")],
        w_counts.online,
    );
    push_gauge(
        &mut out,
        "fleet_workers_total",
        &[("status", "degraded")],
        w_counts.degraded,
    );
    push_gauge(
        &mut out,
        "fleet_workers_total",
        &[("status", "offline")],
        w_counts.offline,
    );
    push_gauge(
        &mut out,
        "fleet_workers_total",
        &[("status", "circuit_open")],
        w_counts.circuit_open,
    );
    push_gauge(
        &mut out,
        "fleet_workers_total",
        &[("status", "total")],
        w_counts.total,
    );
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
    push_gauge(
        &mut out,
        "fleet_tasks_total",
        &[("phase", "pending")],
        t_counts.pending,
    );
    push_gauge(
        &mut out,
        "fleet_tasks_total",
        &[("phase", "dispatched")],
        t_counts.dispatched,
    );
    push_gauge(
        &mut out,
        "fleet_tasks_total",
        &[("phase", "completed")],
        t_counts.completed,
    );
    push_gauge(
        &mut out,
        "fleet_tasks_total",
        &[("phase", "failed")],
        t_counts.failed,
    );
    push_gauge(
        &mut out,
        "fleet_tasks_total",
        &[("phase", "cancelled")],
        t_counts.cancelled,
    );
    push_gauge(
        &mut out,
        "fleet_tasks_total",
        &[("phase", "total")],
        t_counts.total,
    );
    out.push('\n');

    // fleet_events_written_total
    out.push_str("# HELP fleet_events_written_total Highest event sequence number observed.\n");
    out.push_str("# TYPE fleet_events_written_total gauge\n");
    push_gauge(&mut out, "fleet_events_written_total", &[], last_seq);
    out.push('\n');

    // fleet_task_tokens_total{type}
    out.push_str(
        "# HELP fleet_task_tokens_total Cumulative LLM token usage across completed tasks.\n",
    );
    out.push_str("# TYPE fleet_task_tokens_total counter\n");
    push_gauge(
        &mut out,
        "fleet_task_tokens_total",
        &[("type", "input")],
        tok_counts.input,
    );
    push_gauge(
        &mut out,
        "fleet_task_tokens_total",
        &[("type", "output")],
        tok_counts.output,
    );
    push_gauge(
        &mut out,
        "fleet_task_tokens_total",
        &[("type", "cache_read")],
        tok_counts.cache_read,
    );
    push_gauge(
        &mut out,
        "fleet_task_tokens_total",
        &[("type", "total")],
        tok_counts.total,
    );

    // fleet_task_duration_seconds — 완료된 작업의 실행 시간 분포.
    out.push_str(
        "# HELP fleet_task_duration_seconds Execution time of completed tasks in seconds.\n",
    );
    out.push_str("# TYPE fleet_task_duration_seconds histogram\n");
    duration_hist.render(&mut out, "fleet_task_duration_seconds");
    out.push('\n');

    // fleet_task_dispatch_latency_seconds — 작업 디스패치 지연 시간 분포.
    out.push_str(
        "# HELP fleet_task_dispatch_latency_seconds Queue wait time of tasks before being dispatched to a worker in seconds.\n",
    );
    out.push_str("# TYPE fleet_task_dispatch_latency_seconds histogram\n");
    dispatch_latency_hist.render(&mut out, "fleet_task_dispatch_latency_seconds");

    debug!(
        workers = workers.len(),
        tasks = tasks.len(),
        "metrics rendered"
    );
    Ok(out)
}

/// `fleet_http_request_duration_seconds` 버킷 경계 (초).
///
/// HTTP API는 밀리초 단위 응답이 정상이므로 작업 실행 시간과 달리 촘촘하게
/// 잡는다. 10초를 넘는 요청은 사실상 장애이므로 마지막 유한 버킷으로 충분하다.
const HTTP_DURATION_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// 프로세스 내에서 누적되는 HTTP 요청 지연 히스토그램.
///
/// 다른 메트릭과 달리 스토어에서 계산할 수 없다 — 요청 처리 시간은 그 순간에만
/// 관측 가능하므로 미들웨어가 기록해 두어야 한다. 그래서 이것만 인프로세스
/// 상태이며, **프로세스 재시작 시 0으로 돌아간다**(Prometheus counter의 통상
/// 동작이므로 `rate()` 계산에는 문제가 없다).
///
/// 잠금 없이 원자적 연산만 사용한다 — 모든 요청 경로에서 갱신되므로 뮤텍스를
/// 두면 고부하에서 그 자체가 병목이 된다. 합계는 f64 원자 타입이 없어
/// 밀리초 정수로 누적한 뒤 렌더링 시 초로 환산한다.
#[derive(Debug)]
pub struct HttpMetrics {
    buckets: Vec<AtomicU64>,
    count: AtomicU64,
    sum_millis: AtomicU64,
}

impl Default for HttpMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpMetrics {
    pub fn new() -> Self {
        Self {
            buckets: HTTP_DURATION_BUCKETS
                .iter()
                .map(|_| AtomicU64::new(0))
                .collect(),
            count: AtomicU64::new(0),
            sum_millis: AtomicU64::new(0),
        }
    }

    /// 요청 1건의 처리 시간을 기록.
    pub fn observe(&self, elapsed: std::time::Duration) {
        let secs = elapsed.as_secs_f64();
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum_millis
            .fetch_add(elapsed.as_millis() as u64, Ordering::Relaxed);
        if let Some(idx) = HTTP_DURATION_BUCKETS.iter().position(|&b| secs <= b) {
            self.buckets[idx].fetch_add(1, Ordering::Relaxed);
        }
        // 상한 초과 관측은 어떤 유한 버킷에도 넣지 않는다 — `+Inf`는 count로
        // 렌더링하므로 자동으로 포함된다.
    }

    /// Prometheus 히스토그램 라인 렌더링.
    fn render(&self, out: &mut String) {
        const NAME: &str = "fleet_http_request_duration_seconds";
        out.push_str(
            "# HELP fleet_http_request_duration_seconds HTTP request handling time in seconds.\n",
        );
        out.push_str("# TYPE fleet_http_request_duration_seconds histogram\n");

        let mut cumulative = 0u64;
        for (i, bound) in HTTP_DURATION_BUCKETS.iter().enumerate() {
            cumulative += self.buckets[i].load(Ordering::Relaxed);
            out.push_str(&format!("{NAME}_bucket{{le=\"{bound}\"}} {cumulative}\n"));
        }
        let count = self.count.load(Ordering::Relaxed);
        out.push_str(&format!("{NAME}_bucket{{le=\"+Inf\"}} {count}\n"));
        let sum_secs = self.sum_millis.load(Ordering::Relaxed) as f64 / 1000.0;
        out.push_str(&format!("{NAME}_sum {sum_secs}\n"));
        out.push_str(&format!("{NAME}_count {count}\n"));
    }
}

/// `fleet_task_duration_seconds` 버킷 경계 (초).
///
/// 이 플릿의 작업은 수 초짜리 명령부터 수십 분짜리 빌드까지 분포하므로
/// 초 단위부터 1시간까지 넓게 잡는다. 기본 타임아웃이 3600초라 마지막 유한
/// 버킷을 3600으로 두어 "타임아웃 근처" 구간이 드러나게 했다.
const TASK_DURATION_BUCKETS: &[f64] = &[1.0, 5.0, 15.0, 30.0, 60.0, 300.0, 900.0, 1800.0, 3600.0];

/// `fleet_task_dispatch_latency_seconds` 버킷 경계 (초).
///
/// 작업이 큐에서 대기하는 시간으로, 스케줄링 성능 향상 분석을 위해 0.1초 수준부터
/// 15분 대기까지 촘촘하고 폭넓게 구성합니다.
const TASK_DISPATCH_LATENCY_BUCKETS: &[f64] = &[0.1, 0.5, 1.0, 5.0, 15.0, 30.0, 60.0, 300.0, 900.0];

/// Prometheus 히스토그램 누산기.
///
/// 이 크레이트의 메트릭은 별도 레지스트리 없이 스크레이프 시점에 스토어에서
/// 계산하는 구조다. 히스토그램도 같은 방식으로, 조회한 작업들을 그 자리에서
/// 버킷에 넣어 렌더링한다 (프로세스 재시작과 무관하게 값이 일관됨).
struct Histogram {
    /// 버킷 상한 경계 (오름차순).
    bounds: &'static [f64],
    /// 각 버킷의 관측 수 (누적 아님 — 렌더링 시 누적으로 변환).
    counts: Vec<u64>,
    /// 상한을 넘는 관측 수 (`+Inf` 버킷에만 포함).
    overflow: u64,
    sum: f64,
    count: u64,
}

impl Histogram {
    fn new(bounds: &'static [f64]) -> Self {
        Self {
            bounds,
            counts: vec![0; bounds.len()],
            overflow: 0,
            sum: 0.0,
            count: 0,
        }
    }

    fn observe(&mut self, value: f64) {
        // NaN/음수 같은 비정상 값은 합계를 오염시키므로 버린다.
        if !value.is_finite() || value < 0.0 {
            return;
        }
        self.count += 1;
        self.sum += value;
        match self.bounds.iter().position(|&b| value <= b) {
            Some(idx) => self.counts[idx] += 1,
            None => self.overflow += 1,
        }
    }

    /// `_bucket{le=...}` / `_sum` / `_count` 라인을 출력한다.
    fn render(&self, out: &mut String, name: &str) {
        let mut cumulative = 0u64;
        for (i, bound) in self.bounds.iter().enumerate() {
            cumulative += self.counts[i];
            out.push_str(&format!("{name}_bucket{{le=\"{bound}\"}} {cumulative}\n"));
        }
        // `+Inf` 버킷은 전체 관측 수와 같아야 한다 (Prometheus 규약).
        out.push_str(&format!("{name}_bucket{{le=\"+Inf\"}} {}\n", self.count));
        out.push_str(&format!("{name}_sum {}\n", self.sum));
        out.push_str(&format!("{name}_count {}\n", self.count));
    }
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

#[derive(Default, Debug)]
struct TokenCounts {
    input: u64,
    output: u64,
    cache_read: u64,
    total: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MemStore;
    use fleet_core::{Task, TaskRequest, TaskResult, TokenUsage, Worker};

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

    #[tokio::test]
    async fn token_metrics_zero_on_empty_store() {
        let store = MemStore::new_arc();
        let out = metrics_text(store.as_ref()).await.unwrap();
        assert!(out.contains("# HELP fleet_task_tokens_total"));
        assert!(out.contains("# TYPE fleet_task_tokens_total counter"));
        assert!(out.contains("fleet_task_tokens_total{type=\"input\"} 0"));
        assert!(out.contains("fleet_task_tokens_total{type=\"output\"} 0"));
        assert!(out.contains("fleet_task_tokens_total{type=\"cache_read\"} 0"));
        assert!(out.contains("fleet_task_tokens_total{type=\"total\"} 0"));
    }

    #[tokio::test]
    async fn token_metrics_aggregate_from_completed_tasks() {
        let store = MemStore::new_arc();

        let w = Worker::new("w1", "wss://1");
        let worker_id = w.id;
        store.upsert_worker(&w).await.unwrap();

        let mut t1 = Task::from_request(TaskRequest {
            prompt: "hello".into(),
            ..Default::default()
        });
        t1.status = TaskStatus::Completed(TaskResult {
            output: "world".into(),
            exit_code: 0,
            duration_secs: 1.0,
            token_usage: Some(TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: 20,
            }),
            worker_id,
            finished_at: chrono::Utc::now(),
        });
        store.insert_task(&t1).await.unwrap();

        let mut t2 = Task::from_request(TaskRequest {
            prompt: "bye".into(),
            ..Default::default()
        });
        t2.status = TaskStatus::Completed(TaskResult {
            output: "done".into(),
            exit_code: 0,
            duration_secs: 2.0,
            token_usage: Some(TokenUsage {
                input_tokens: 200,
                output_tokens: 80,
                cache_read_tokens: 0,
            }),
            worker_id,
            finished_at: chrono::Utc::now(),
        });
        store.insert_task(&t2).await.unwrap();

        let out = metrics_text(store.as_ref()).await.unwrap();
        assert!(out.contains("fleet_task_tokens_total{type=\"input\"} 300"));
        assert!(out.contains("fleet_task_tokens_total{type=\"output\"} 130"));
        assert!(out.contains("fleet_task_tokens_total{type=\"cache_read\"} 20"));
        assert!(out.contains("fleet_task_tokens_total{type=\"total\"} 430"));
    }

    #[tokio::test]
    async fn token_metrics_ignore_tasks_without_usage() {
        let store = MemStore::new_arc();

        let w = Worker::new("w1", "wss://1");
        let worker_id = w.id;
        store.upsert_worker(&w).await.unwrap();

        let mut t = Task::from_request(TaskRequest {
            prompt: "x".into(),
            ..Default::default()
        });
        t.status = TaskStatus::Completed(TaskResult {
            output: "y".into(),
            exit_code: 0,
            duration_secs: 0.5,
            token_usage: None,
            worker_id,
            finished_at: chrono::Utc::now(),
        });
        store.insert_task(&t).await.unwrap();

        let out = metrics_text(store.as_ref()).await.unwrap();
        assert!(out.contains("fleet_task_tokens_total{type=\"total\"} 0"));
    }
}

#[cfg(test)]
mod histogram_tests {
    use super::*;

    #[test]
    fn buckets_are_cumulative_and_inf_matches_count() {
        let mut h = Histogram::new(TASK_DURATION_BUCKETS);
        for v in [0.5, 3.0, 3.0, 45.0, 7200.0] {
            h.observe(v);
        }

        let mut out = String::new();
        h.render(&mut out, "t");

        // le="1" → 0.5 하나.
        assert!(out.contains("t_bucket{le=\"1\"} 1"), "{out}");
        // le="5" → 0.5, 3.0, 3.0 (누적 3).
        assert!(out.contains("t_bucket{le=\"5\"} 3"), "{out}");
        // le="60" → 위 3개 + 45.0 (누적 4).
        assert!(out.contains("t_bucket{le=\"60\"} 4"), "{out}");
        // 7200은 마지막 유한 버킷(3600)을 넘으므로 +Inf에만 포함.
        assert!(out.contains("t_bucket{le=\"3600\"} 4"), "{out}");
        assert!(out.contains("t_bucket{le=\"+Inf\"} 5"), "{out}");
        assert!(out.contains("t_count 5"), "{out}");
        assert!(out.contains("t_sum 7251.5"), "{out}");
    }

    #[test]
    fn empty_histogram_renders_zeros() {
        let h = Histogram::new(TASK_DURATION_BUCKETS);
        let mut out = String::new();
        h.render(&mut out, "t");
        assert!(out.contains("t_bucket{le=\"+Inf\"} 0"));
        assert!(out.contains("t_count 0"));
        assert!(out.contains("t_sum 0"));
    }

    /// NaN/음수는 합계를 오염시키므로 관측에서 제외되어야 한다.
    #[test]
    fn invalid_observations_are_ignored() {
        let mut h = Histogram::new(TASK_DURATION_BUCKETS);
        h.observe(f64::NAN);
        h.observe(-1.0);
        h.observe(f64::INFINITY);
        h.observe(2.0);

        let mut out = String::new();
        h.render(&mut out, "t");
        assert!(out.contains("t_count 1"), "{out}");
        assert!(out.contains("t_sum 2"), "{out}");
    }

    /// 경계값은 해당 버킷에 포함되어야 한다 (`le` = less-or-equal).
    #[test]
    fn boundary_value_falls_into_its_own_bucket() {
        let mut h = Histogram::new(TASK_DURATION_BUCKETS);
        h.observe(5.0);
        let mut out = String::new();
        h.render(&mut out, "t");
        assert!(out.contains("t_bucket{le=\"5\"} 1"), "{out}");
        assert!(out.contains("t_bucket{le=\"1\"} 0"), "{out}");
    }
}

#[cfg(test)]
mod http_metrics_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn observations_land_in_expected_buckets() {
        let m = HttpMetrics::new();
        m.observe(Duration::from_millis(3)); // le=0.005
        m.observe(Duration::from_millis(30)); // le=0.05
        m.observe(Duration::from_secs(30)); // 상한 초과 → +Inf에만

        let mut out = String::new();
        m.render(&mut out);

        assert!(
            out.contains("fleet_http_request_duration_seconds_bucket{le=\"0.005\"} 1"),
            "{out}"
        );
        // 누적이므로 le=0.05에는 3ms + 30ms 두 건.
        assert!(
            out.contains("fleet_http_request_duration_seconds_bucket{le=\"0.05\"} 2"),
            "{out}"
        );
        // 30초는 마지막 유한 버킷(10)에 포함되지 않는다.
        assert!(
            out.contains("fleet_http_request_duration_seconds_bucket{le=\"10\"} 2"),
            "{out}"
        );
        assert!(
            out.contains("fleet_http_request_duration_seconds_bucket{le=\"+Inf\"} 3"),
            "{out}"
        );
        assert!(
            out.contains("fleet_http_request_duration_seconds_count 3"),
            "{out}"
        );
    }

    #[test]
    fn empty_http_metrics_render_zeros() {
        let m = HttpMetrics::new();
        let mut out = String::new();
        m.render(&mut out);
        assert!(out.contains("fleet_http_request_duration_seconds_count 0"));
        assert!(out.contains("fleet_http_request_duration_seconds_sum 0"));
        assert!(out.contains("# TYPE fleet_http_request_duration_seconds histogram"));
    }

    /// 동시 갱신에서 관측이 누락되지 않아야 한다 (원자적 누산 확인).
    #[test]
    fn concurrent_observations_are_not_lost() {
        let m = Arc::new(HttpMetrics::new());
        let mut handles = Vec::new();
        for _ in 0..8 {
            let m = m.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    m.observe(Duration::from_millis(1));
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let mut out = String::new();
        m.render(&mut out);
        assert!(
            out.contains("fleet_http_request_duration_seconds_count 800"),
            "{out}"
        );
    }
}
