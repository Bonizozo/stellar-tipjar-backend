use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{runtime, trace as sdktrace};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::Layer;

/// Builds an OTLP tracing layer when `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
///
/// Returns `None` when the env var is absent so the app starts normally
/// without a collector.
pub fn init_tracer() -> Option<impl Layer<tracing_subscriber::Registry> + Send + Sync + 'static> {
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok()?;

    let service_name = std::env::var("OTEL_SERVICE_NAME")
        .unwrap_or_else(|_| "stellar-tipjar-backend".to_string());

    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint);

    let provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            sdktrace::config().with_resource(opentelemetry_sdk::Resource::new(vec![
                opentelemetry::KeyValue::new(
                    opentelemetry_semantic_conventions::resource::SERVICE_NAME,
                    service_name,
                ),
            ])),
        )
        .install_batch(runtime::Tokio)
        .ok()?;

    let tracer = provider.tracer("stellar-tipjar-backend");
    Some(OpenTelemetryLayer::new(tracer))
}
