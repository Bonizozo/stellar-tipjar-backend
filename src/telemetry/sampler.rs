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
    use std::sync::Mutex;

    /// `OTEL_SAMPLE_RATIO` is process-global, so these tests must not run
    /// concurrently: one clearing the variable while another has just set it
    /// makes both read the wrong value.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Run `body` with `OTEL_SAMPLE_RATIO` set to `value` (or unset for `None`),
    /// serialized against the other tests in this module.
    fn with_ratio(value: Option<&str>, body: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        match value {
            Some(v) => std::env::set_var("OTEL_SAMPLE_RATIO", v),
            None => std::env::remove_var("OTEL_SAMPLE_RATIO"),
        }
        body();
        std::env::remove_var("OTEL_SAMPLE_RATIO");
    }

    #[test]
    fn defaults_to_one() {
        with_ratio(None, || assert_eq!(configured_ratio(), 1.0));
    }

    #[test]
    fn parses_valid_ratio() {
        with_ratio(Some("0.25"), || assert_eq!(configured_ratio(), 0.25));
    }

    #[test]
    fn clamps_above_one() {
        with_ratio(Some("2.5"), || assert_eq!(configured_ratio(), 1.0));
    }

    #[test]
    fn clamps_below_zero() {
        with_ratio(Some("-0.5"), || assert_eq!(configured_ratio(), 0.0));
    }
}
