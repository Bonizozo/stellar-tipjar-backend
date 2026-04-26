use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    runtime,
    trace::{self as sdktrace, Sampler},
    Resource,
};
use opentelemetry_semantic_conventions::resource::{
    DEPLOYMENT_ENVIRONMENT, HOST_NAME, SERVICE_NAME, SERVICE_VERSION,
};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::Layer;

/// Build the `Resource` that identifies this service in every exported span.
///
/// Reads:
/// - `OTEL_SERVICE_NAME`        (default: `"stellar-tipjar-backend"`)
/// - `OTEL_SERVICE_VERSION`     (default: Cargo package version)
/// - `DEPLOYMENT_ENVIRONMENT`   (default: `"development"`)
fn build_resource() -> Resource {
    let service_name = std::env::var("OTEL_SERVICE_NAME")
        .unwrap_or_else(|_| "stellar-tipjar-backend".to_string());

    let service_version = std::env::var("OTEL_SERVICE_VERSION")
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());

    let environment = std::env::var("DEPLOYMENT_ENVIRONMENT")
        .unwrap_or_else(|_| "development".to_string());

    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());

    Resource::new(vec![
        opentelemetry::KeyValue::new(SERVICE_NAME, service_name),
        opentelemetry::KeyValue::new(SERVICE_VERSION, service_version),
        opentelemetry::KeyValue::new(DEPLOYMENT_ENVIRONMENT, environment),
        opentelemetry::KeyValue::new(HOST_NAME, hostname),
    ])
}

/// Build a `Sampler` from the `OTEL_SAMPLE_RATIO` environment variable.
///
/// - `1.0`  → always sample (default)
/// - `0.0`  → never sample
/// - `0.1`  → sample 10 % of traces
///
/// Values outside `[0.0, 1.0]` are clamped.
fn build_sampler() -> Sampler {
    let ratio = std::env::var("OTEL_SAMPLE_RATIO")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);

    if ratio >= 1.0 {
        Sampler::AlwaysOn
    } else if ratio <= 0.0 {
        Sampler::AlwaysOff
    } else {
        Sampler::TraceIdRatioBased(ratio)
    }
}

/// Initialise the global OpenTelemetry tracer and return a `tracing` layer.
///
/// Behaviour depends on environment variables:
///
/// | Variable                        | Effect                                              |
/// |---------------------------------|-----------------------------------------------------|
/// | `OTEL_EXPORTER_OTLP_ENDPOINT`   | Export spans via gRPC OTLP to this endpoint         |
/// | `OTEL_EXPORTER_STDOUT=true`     | Print spans to stdout (useful in development)       |
/// | `OTEL_SAMPLE_RATIO`             | Fraction of traces to sample (default `1.0`)        |
/// | `OTEL_SERVICE_NAME`             | Service name tag on every span                      |
/// | `OTEL_SERVICE_VERSION`          | Service version tag on every span                   |
/// | `DEPLOYMENT_ENVIRONMENT`        | Environment tag (`production`, `staging`, …)        |
///
/// Both `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_STDOUT` can be set
/// simultaneously — spans will be sent to both destinations.
///
/// Returns `None` when neither variable is set so the app starts cleanly
/// without a collector.
pub fn init_tracer() -> Option<impl Layer<tracing_subscriber::Registry> + Send + Sync + 'static> {
    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
    let stdout_enabled = std::env::var("OTEL_EXPORTER_STDOUT")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    if otlp_endpoint.is_none() && !stdout_enabled {
        return None;
    }

    // Register W3C TraceContext propagator globally so outbound HTTP clients
    // (reqwest, etc.) can inject the `traceparent` header automatically.
    global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );

    let trace_config = sdktrace::config()
        .with_sampler(build_sampler())
        .with_resource(build_resource());

    let mut builder = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_config(trace_config);

    // Attach OTLP batch exporter when an endpoint is configured.
    if let Some(endpoint) = otlp_endpoint {
        let otlp_exporter = opentelemetry_otlp::new_exporter()
            .tonic()
            .with_endpoint(endpoint)
            .build_span_exporter()
            .expect("Failed to build OTLP span exporter");

        builder = builder.with_batch_exporter(otlp_exporter, runtime::Tokio);
    }

    // Attach stdout simple exporter when requested (dev/debug).
    if stdout_enabled {
        let stdout_exporter = opentelemetry_stdout::SpanExporter::default();
        builder = builder.with_simple_exporter(stdout_exporter);
    }

    let provider = builder.build();

    // Register as the global provider so code that calls
    // `opentelemetry::global::tracer(...)` directly also works.
    global::set_tracer_provider(provider.clone());

    let tracer = provider.tracer("stellar-tipjar-backend");
    Some(OpenTelemetryLayer::new(tracer))
}

/// Flush all pending spans and shut down the global tracer provider.
///
/// Call this during graceful shutdown **before** the process exits to ensure
/// all buffered spans are exported.
pub fn shutdown_tracer() {
    global::shutdown_tracer_provider();
}
