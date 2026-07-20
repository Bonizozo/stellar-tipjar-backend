use opentelemetry_sdk::trace as sdktrace;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::Layer;

/// Builds an in-process OpenTelemetry tracing layer.
///
/// Returns `None` when `OTEL_EXPORTER_OTLP_ENDPOINT` is absent so the app
/// starts normally without OTel overhead.  When the variable is present the
/// layer records spans in-process; for export to a collector, run an
/// OpenTelemetry Collector sidecar and configure it via the standard
/// `OTEL_*` environment variables using the SDK's auto-configuration.
///
/// # Environment variables
///
/// `OTEL_SERVICE_NAME` is intentionally read here rather than via `AppConfig`.
/// It follows the OpenTelemetry standard naming convention that tools and
/// sidecars set automatically; adding it to `AppConfig` would create friction.
/// It is read exactly once, at startup — no request-path env reads occur.
pub fn init_tracer() -> Option<impl Layer<tracing_subscriber::Registry> + Send + Sync + 'static> {
    // Only activate OTel tracing when the endpoint env var is set.
    // The actual exporter transport (OTLP/stdout/etc.) can be configured
    // via the OpenTelemetry Collector sidecar.
    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_err() {
        return None;
    }

    let service_name = std::env::var("OTEL_SERVICE_NAME")
        .unwrap_or_else(|_| "stellar-tipjar-backend".to_string());

    let provider = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_config(
            sdktrace::config().with_resource(opentelemetry_sdk::Resource::new(vec![
                opentelemetry::KeyValue::new(
                    opentelemetry_semantic_conventions::resource::SERVICE_NAME,
                    service_name,
                ),
            ])),
        )
        .build();

    let tracer = opentelemetry::trace::TracerProvider::tracer(&provider, "stellar-tipjar-backend");
    Some(OpenTelemetryLayer::new(tracer))
}
