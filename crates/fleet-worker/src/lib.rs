//! # fleet-worker
//!
//! 워커 노드에서 실행되는 데몬. 두 가지 핵심 책임:
//!
//! 1. **grok 서브프로세스 관리** — `grok agent serve`를 백그라운드에서 실행하고
//!    비정상 종료 시 재시작. 헬스체크는 포트 점검으로 수행.
//!
//! 2. **오케스트레이터 등록/하트비트** — 시작 시 `POST /v1/workers/register`로
//!    자신을 등록하고, 주기적으로 `POST /v1/workers/heartbeat`로 상태 전송.
//!
//! ## 아키텍처
//!
//! ```text
//! [fleet-worker 프로세스]
//!   │
//!   ├── GrokRunner (백그라운드 태스크)
//!   │     └── grok agent serve --bind 127.0.0.1:2419 --secret ...
//!   │           (비정상 종료 시 5초 후 재시작)
//!   │
//!   ├── RegistrationClient (백그라운드 태스크)
//!   │     ├── register (1회, 재시도 포함)
//!   │     └── heartbeat 루프 (15초 간격)
//!   │
//!   └── 신호 핸들러 (SIGTERM/SIGINT)
//!         └── grok 종료 + 등록 해제 (best-effort)
//! ```

#![forbid(unsafe_code)]

pub mod config;
pub mod error;
pub mod grok_process;
pub mod join;
pub mod registration;
pub mod runner;
pub mod skill_loader;

pub use config::WorkerConfig;
pub use error::WorkerError;
pub use join::JoinArgs;
pub use registration::RegistrationClient;
pub use runner::WorkerRunner;
pub use skill_loader::{inject_skills, inject_skills_from_dir};

/// 현재 실행 중인 grok 세션 수. heartbeat의 active_tasks에 사용.
/// GrokRunner가 세션을 시작/종료할 때 이 값을 증감한다.
pub static ACTIVE_SESSIONS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// tracing-subscriber 초기화 + OpenTelemetry 분산 추적 연동 (로드맵 #42).
/// 환경변수 `RUST_LOG`가 없으면 `info` 레벨 적용.
///
/// `fleet-worker`는 오케스트레이터의 `AcpTransport`가 붙는 ACP WebSocket
/// 경로(`grok agent serve`가 직접 종단)에는 관여하지 않는다 — 이 프로세스가
/// 실제로 참여하는 오케스트레이터와의 통신은 `POST /v1/workers/register`·
/// `POST /v1/workers/heartbeat` HTTP 호출뿐이다. 그래서 여기서 하는
/// 분산 추적 연동의 범위도 그 HTTP 경로로 한정한다 — `registration.rs`가
/// 이 함수가 등록한 W3C Trace Context propagator를 이용해 나가는 요청에
/// `traceparent`/`tracestate` 헤더를 실어 보내고, `fleet-api::handlers`가
/// 그 헤더를 받아 자신의 스팬을 이어붙인다.
pub fn init_tracing() {
    use opentelemetry::KeyValue;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::{trace::TracerProvider, Resource};
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

    // W3C Trace Context propagator 전역 등록. OTLP 익스포터 설정 여부와
    // 무관하게 항상 등록한다 — `registration.rs`의 헤더 주입이 이 등록에
    // 의존하며, 등록 자체는 부작용이 없다.
    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,fleet_worker=debug"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false);

    // fleet-cli/src/logging.rs와 동일한 컨벤션: OTEL_EXPORTER_OTLP_ENDPOINT가
    // 설정된 경우에만 실제 OTLP 익스포터/트레이서 레이어를 켠다. 이 env var가
    // 없으면(기본) 워커는 이전과 동일하게 로컬 stderr 로그만 남긴다 — 다만
    // propagator는 위에서 이미 등록했으므로, 헤더 주입 자체는 시도하되
    // 활성 스팬 컨텍스트가 없어 실질적으로 빈 헤더가 된다(무해한 no-op).
    let otel_layer = if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        if !endpoint.is_empty() {
            let resource = Resource::new(vec![
                KeyValue::new("service.name", "grok-fleet-worker"),
                KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            ]);

            match opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint.clone())
                .build()
            {
                Ok(otlp_exporter) => {
                    let provider = TracerProvider::builder()
                        .with_batch_exporter(otlp_exporter, opentelemetry_sdk::runtime::Tokio)
                        .with_resource(resource)
                        .build();
                    opentelemetry::global::set_tracer_provider(provider.clone());
                    let tracer = opentelemetry::trace::TracerProvider::tracer(
                        &provider,
                        "grok-fleet-worker",
                    );
                    eprintln!(
                        "✓ OpenTelemetry tracing layer initialized with endpoint: {endpoint}"
                    );
                    Some(tracing_opentelemetry::layer().with_tracer(tracer))
                }
                Err(e) => {
                    eprintln!("⚠️  Failed to build OpenTelemetry OTLP exporter: {e:#}");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    Registry::default()
        .with(filter)
        .with(fmt_layer)
        .with(otel_layer)
        .init();
}

/// 프로세스 종료 시 잔여 트레이스 버퍼를 OTLP Collector로 flush.
pub fn shutdown_tracing() {
    opentelemetry::global::shutdown_tracer_provider();
}
