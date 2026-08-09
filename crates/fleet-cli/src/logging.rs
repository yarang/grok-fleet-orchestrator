//! tracing 초기화 및 OpenTelemetry 분산 추적 연동.
//! stderr로 로깅 — stdout은 MCP JSON-RPC가 독점.

use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{trace::TracerProvider, Resource};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

/// 로깅 및 OpenTelemetry 분산 추적 초기화.
/// `log_level`은 기본 필터이나 `RUST_LOG`가 있으면 덮어씀.
pub fn init(log_level: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    // 표준 stderr 포맷팅 레이어 (기존 동작 유지)
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false);

    // OTLP Endpoint 환경변수 감지하여 조건부 OpenTelemetry 레이어 빌드
    let otel_layer = if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        if !endpoint.is_empty() {
            let resource = Resource::new(vec![
                KeyValue::new("service.name", "grok-fleet-orchestrator"),
                KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            ]);

            // SpanExporter::builder()로 Tonic gRPC 엑스포터 인스턴스 생성
            match opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint.clone())
                .build()
            {
                Ok(otlp_exporter) => {
                    // TracerProvider를 이용하여 배치 트레이스 배출 설정
                    let provider = TracerProvider::builder()
                        .with_batch_exporter(otlp_exporter, opentelemetry_sdk::runtime::Tokio)
                        .with_resource(resource)
                        .build();

                    // 전역 트레이서 프로바이더로 지정
                    opentelemetry::global::set_tracer_provider(provider.clone());

                    // tracer 빌드 후 tracing_opentelemetry 레이어로 매핑
                    let tracer = opentelemetry::trace::TracerProvider::tracer(&provider, "grok-fleet-orchestrator");
                    eprintln!("✓ OpenTelemetry tracing layer initialized with endpoint: {endpoint}");
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

    // 레이어들을 구독자(Subscriber)에 조립하여 등록
    Registry::default()
        .with(filter)
        .with(fmt_layer)
        .with(otel_layer)
        .init();
}

/// 어플리케이션 종료 시 잔여 버퍼를 OTLP Collector로 flush하고 정리.
pub fn shutdown() {
    opentelemetry::global::shutdown_tracer_provider();
}
