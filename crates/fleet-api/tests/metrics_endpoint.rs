//! `/metrics` 엔드포인트 통합 테스트.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::task::JoinHandle;

use fleet_api::{ApiTokenCredential, AppState};
use fleet_core::{
    Task, TaskRequest, Worker,
};
use fleet_store::Store;

use fleet_store::mem::MemStore;

struct Server {
    addr: SocketAddr,
    _handle: JoinHandle<()>,
}

async fn spawn_server() -> Server {
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    let state = Arc::new(AppState::new(store));
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = fleet_api::build_app(state);
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Server {
        addr,
        _handle: handle,
    }
}

#[tokio::test]
async fn metrics_returns_prometheus_text() {
    let srv = spawn_server().await;
    let resp = reqwest::get(format!("http://{}/metrics", srv.addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        ct.starts_with("text/plain"),
        "expected text/plain content-type, got: {ct}"
    );
    let body = resp.text().await.unwrap();
    assert!(body.contains("# HELP fleet_up"));
    assert!(body.contains("# TYPE fleet_up gauge"));
    assert!(body.contains("fleet_up 1"));
    assert!(body.contains("fleet_workers_total{status=\"online\"}"));
    assert!(body.contains("fleet_tasks_total{phase=\"pending\"}"));
    assert!(body.contains("fleet_workers_capacity_total"));
    assert!(body.contains("fleet_workers_active_tasks_total"));
    assert!(body.contains("fleet_events_written_total"));
    assert!(body.contains("fleet_task_dispatch_latency_seconds"));
}

#[tokio::test]
async fn metrics_does_not_require_auth() {
    // 인증이 활성화된 AppState에서도 /metrics는 인증 미들웨어 바깥에 있음.
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    let state = Arc::new(AppState::new(store).with_tokens(vec![ApiTokenCredential {
        principal_id: "metrics-test".into(),
        token: "secret".into(),
        capabilities: fleet_core::PermissionKind::all().to_vec(),
    }]));
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = fleet_api::build_app(state);
    let _handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // Authorization 헤더 없이 호출해도 200이어야 함.
    let resp = reqwest::get(format!("http://{}/metrics", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn metrics_reflects_state_changes() {
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    store
        .upsert_worker(&Worker::new("w1", "wss://1"))
        .await
        .unwrap();
    store
        .insert_task(&Task::from_request(TaskRequest {
            prompt: "p".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let state = Arc::new(AppState::new(store));
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = fleet_api::build_app(state);
    let _handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let body = reqwest::get(format!("http://{}/metrics", addr))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("fleet_workers_total{status=\"online\"} 1"));
    assert!(body.contains("fleet_tasks_total{phase=\"pending\"} 1"));
    assert!(body.contains("fleet_workers_capacity_total 4"));
}

/// HTTP 지연 히스토그램이 실제 요청을 통해 누적되는지 검증한다.
///
/// 미들웨어가 라우터에 제대로 걸려 있지 않으면 단위 테스트는 통과해도
/// 값이 영원히 0이므로, 실제 라우터를 거치는 경로로 확인한다.
#[tokio::test]
async fn http_duration_histogram_records_requests() {
    let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
    let state = Arc::new(AppState::new(store));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let app = fleet_api::build_app(state);
    let _handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // 첫 스크랩 — 이 요청 자체는 아직 관측 전이므로 0건일 수 있다.
    let first = reqwest::get(format!("http://{addr}/metrics"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        first.contains("# TYPE fleet_http_request_duration_seconds histogram"),
        "히스토그램 메타데이터가 노출되어야 한다:\n{first}"
    );

    // 몇 건 더 요청한 뒤 다시 스크랩하면 count가 증가해 있어야 한다.
    for _ in 0..3 {
        let _ = reqwest::get(format!("http://{addr}/metrics"))
            .await
            .unwrap();
    }

    let body = reqwest::get(format!("http://{addr}/metrics"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let count_line = body
        .lines()
        .find(|l| l.starts_with("fleet_http_request_duration_seconds_count "))
        .expect("count 라인이 있어야 한다");
    let count: u64 = count_line
        .rsplit(' ')
        .next()
        .unwrap()
        .parse()
        .expect("count는 정수여야 한다");
    assert!(
        count >= 4,
        "이전 요청들이 관측되어야 한다 (실제 count={count})\n{count_line}"
    );

    // +Inf 버킷은 전체 관측 수와 같아야 한다 (Prometheus 규약).
    let inf_line = body
        .lines()
        .find(|l| l.starts_with("fleet_http_request_duration_seconds_bucket{le=\"+Inf\"}"))
        .expect("+Inf 버킷이 있어야 한다");
    let inf: u64 = inf_line.rsplit(' ').next().unwrap().parse().unwrap();
    assert_eq!(inf, count, "+Inf 버킷은 count와 일치해야 한다");
}
