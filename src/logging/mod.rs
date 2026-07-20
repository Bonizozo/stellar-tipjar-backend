use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialise the global tracing subscriber.
///
/// - `LOG_FORMAT=json`                  → structured JSON output (production)
/// - `OTEL_EXPORTER_OTLP_ENDPOINT=...`  → also exports spans via OTLP
/// - `RUST_LOG`                         → log level filter (default: `info`)
///
/// # Environment variables
///
/// `LOG_FORMAT` is intentionally read here rather than via `AppConfig`.
/// It controls the logging subsystem that must be initialised *before*
/// `AppConfig::from_env()` runs (so that config-validation errors are
/// themselves formatted correctly).  Reading it once at startup does not
/// violate the "no runtime env reads on request paths" rule.
pub fn init() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "stellar_tipjar_backend=debug,tower_http=debug,sqlx=warn".into());

    // Startup-only read: LOG_FORMAT must be known before AppConfig is built.
    let json_format = std::env::var("LOG_FORMAT")
        .map(|v| v.to_lowercase() == "json")
        .unwrap_or(false);

    // Build the optional OTEL layer.  We apply it with `with()` on the
    // base `Registry` (before the `EnvFilter`) so that the inferred
    // subscriber type stays `Registry` — Option<L> satisfies Layer<Registry>
    // as long as L: Layer<Registry>, which OpenTelemetryLayer does.
    let otel_layer = crate::telemetry::init_tracer();

    if json_format {
        tracing_subscriber::registry()
            .with(otel_layer)
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json().flatten_event(true))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(otel_layer)
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }
}
