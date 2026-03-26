use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialise the global tracing subscriber.
///
/// - `LOG_FORMAT=json`                  → structured JSON output (production)
/// - `OTEL_EXPORTER_OTLP_ENDPOINT=...`  → also exports spans via OTLP
/// - `RUST_LOG`                         → log level filter (default: `info`)
pub fn init() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "stellar_tipjar_backend=debug,tower_http=debug,sqlx=warn".into());

    let json_format = std::env::var("LOG_FORMAT")
        .map(|v| v.to_lowercase() == "json")
        .unwrap_or(false);

    let otel_layer = crate::telemetry::init_tracer();

    let registry = tracing_subscriber::registry().with(filter).with(otel_layer);

    if json_format {
        registry
            .with(tracing_subscriber::fmt::layer().json().flatten_event(true))
            .init();
    } else {
        registry
            .with(tracing_subscriber::fmt::layer())
            .init();
    }
}
