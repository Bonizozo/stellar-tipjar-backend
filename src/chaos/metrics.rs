use std::time::{Duration, Instant};

/// Snapshot of system health metrics collected during an experiment phase.
#[derive(Debug, Clone, Default)]
pub struct Metrics {
    /// Fraction of requests that returned an error (0.0–1.0).
    pub error_rate: f64,
    /// 99th-percentile latency in milliseconds.
    pub p99_latency_ms: f64,
    /// Number of successful operations observed.
    pub success_count: u64,
    /// Number of failed operations observed.
    pub failure_count: u64,
}

impl Metrics {
    pub fn total(&self) -> u64 {
        self.success_count + self.failure_count
    }
}

/// Collects latency samples and computes summary statistics.
#[derive(Debug, Default)]
pub struct MetricsCollector {
    samples: Vec<Duration>,
    errors: u64,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_success(&mut self, latency: Duration) {
        self.samples.push(latency);
    }

    pub fn record_error(&mut self) {
        self.errors += 1;
    }

    /// Snapshot current state as a `Metrics` value.
    pub fn snapshot(&mut self) -> Metrics {
        let total = self.samples.len() as u64 + self.errors;
        let error_rate = if total == 0 {
            0.0
        } else {
            self.errors as f64 / total as f64
        };

        let p99 = percentile_ms(&mut self.samples, 99);

        Metrics {
            error_rate,
            p99_latency_ms: p99,
            success_count: self.samples.len() as u64,
            failure_count: self.errors,
        }
    }

    pub fn reset(&mut self) {
        self.samples.clear();
        self.errors = 0;
    }
}

/// Measures elapsed time for a synchronous block.
pub struct LatencyTimer {
    start: Instant,
}

impl LatencyTimer {
    pub fn start() -> Self {
        Self { start: Instant::now() }
    }

    pub fn elapsed(self) -> Duration {
        self.start.elapsed()
    }
}

fn percentile_ms(samples: &mut Vec<Duration>, p: usize) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.sort_unstable();
    let idx = ((p as f64 / 100.0) * (samples.len() - 1) as f64).round() as usize;
    samples[idx].as_secs_f64() * 1000.0
}
