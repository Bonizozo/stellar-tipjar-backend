/// Sampling configuration helpers.
///
/// The actual sampler is built inside `tracer::init_tracer` from the
/// `OTEL_SAMPLE_RATIO` environment variable.  This module documents the
/// supported values and provides a helper for tests.

/// Returns the configured sample ratio from `OTEL_SAMPLE_RATIO`, clamped to
/// `[0.0, 1.0]`.  Defaults to `1.0` (always sample) when the variable is
/// absent or unparseable.
pub fn configured_ratio() -> f64 {
    std::env::var("OTEL_SAMPLE_RATIO")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_one() {
        // Ensure the env var is absent for this test.
        std::env::remove_var("OTEL_SAMPLE_RATIO");
        assert_eq!(configured_ratio(), 1.0);
    }

    #[test]
    fn parses_valid_ratio() {
        std::env::set_var("OTEL_SAMPLE_RATIO", "0.25");
        assert_eq!(configured_ratio(), 0.25);
        std::env::remove_var("OTEL_SAMPLE_RATIO");
    }

    #[test]
    fn clamps_above_one() {
        std::env::set_var("OTEL_SAMPLE_RATIO", "2.5");
        assert_eq!(configured_ratio(), 1.0);
        std::env::remove_var("OTEL_SAMPLE_RATIO");
    }

    #[test]
    fn clamps_below_zero() {
        std::env::set_var("OTEL_SAMPLE_RATIO", "-0.5");
        assert_eq!(configured_ratio(), 0.0);
        std::env::remove_var("OTEL_SAMPLE_RATIO");
    }
}
