use std::time::Duration;

use super::injectors::{ChaosInjector, Result};
use super::metrics::{Metrics, MetricsCollector};
use super::ChaosError;

/// Result of a single chaos experiment run.
#[derive(Debug)]
pub struct ExperimentResult {
    pub name: String,
    pub baseline: Metrics,
    pub chaos_metrics: Metrics,
    pub recovery_metrics: Metrics,
    /// `true` when the system met all resilience thresholds.
    pub passed: bool,
}

/// Thresholds used to evaluate whether an experiment passed.
#[derive(Debug, Clone)]
pub struct ResilienceThresholds {
    /// Maximum acceptable error rate during chaos (e.g. 0.05 = 5 %).
    pub max_error_rate_during_chaos: f64,
    /// Maximum acceptable p99 latency multiplier vs baseline.
    pub max_latency_multiplier: f64,
    /// Maximum acceptable error rate after recovery (relative to baseline).
    pub max_recovery_error_rate_multiplier: f64,
}

impl Default for ResilienceThresholds {
    fn default() -> Self {
        Self {
            max_error_rate_during_chaos: 0.05,
            max_latency_multiplier: 2.0,
            max_recovery_error_rate_multiplier: 1.1,
        }
    }
}

/// A single chaos experiment: inject failures, observe, recover, evaluate.
pub struct ChaosExperiment {
    pub name: String,
    pub injectors: Vec<Box<dyn ChaosInjector>>,
    pub duration: Duration,
    pub thresholds: ResilienceThresholds,
}

impl ChaosExperiment {
    pub fn new(name: impl Into<String>, duration: Duration) -> Self {
        Self {
            name: name.into(),
            injectors: Vec::new(),
            duration,
            thresholds: ResilienceThresholds::default(),
        }
    }

    pub fn with_injector(mut self, injector: Box<dyn ChaosInjector>) -> Self {
        self.injectors.push(injector);
        self
    }

    pub fn with_thresholds(mut self, thresholds: ResilienceThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    /// Run the experiment: baseline → inject → chaos window → recover → evaluate.
    pub async fn run(
        &self,
        baseline_collector: &mut MetricsCollector,
        chaos_collector: &mut MetricsCollector,
        recovery_collector: &mut MetricsCollector,
    ) -> Result<ExperimentResult> {
        tracing::info!(experiment = %self.name, "Chaos experiment starting");

        let baseline = baseline_collector.snapshot();

        // Inject all failures.
        for injector in &self.injectors {
            injector.inject().await?;
        }

        tokio::time::sleep(self.duration).await;
        let chaos_metrics = chaos_collector.snapshot();

        // Recover.
        for injector in &self.injectors {
            injector.recover().await?;
        }

        // Allow system to stabilise.
        tokio::time::sleep(Duration::from_secs(5)).await;
        let recovery_metrics = recovery_collector.snapshot();

        let passed = self.evaluate(&baseline, &chaos_metrics, &recovery_metrics);

        tracing::info!(
            experiment = %self.name,
            passed,
            chaos_error_rate = chaos_metrics.error_rate,
            "Chaos experiment complete"
        );

        Ok(ExperimentResult {
            name: self.name.clone(),
            baseline,
            chaos_metrics,
            recovery_metrics,
            passed,
        })
    }

    fn evaluate(&self, baseline: &Metrics, chaos: &Metrics, recovery: &Metrics) -> bool {
        let error_ok = chaos.error_rate < self.thresholds.max_error_rate_during_chaos;

        let latency_ok = baseline.p99_latency_ms == 0.0
            || chaos.p99_latency_ms
                < baseline.p99_latency_ms * self.thresholds.max_latency_multiplier;

        let recovery_ok = recovery.error_rate
            < baseline.error_rate * self.thresholds.max_recovery_error_rate_multiplier + 0.01;

        error_ok && latency_ok && recovery_ok
    }
}

/// Runs multiple experiments sequentially and produces a summary report.
pub struct ChaosRunner {
    pub cool_down: Duration,
}

impl Default for ChaosRunner {
    fn default() -> Self {
        Self { cool_down: Duration::from_secs(10) }
    }
}

impl ChaosRunner {
    pub fn new(cool_down: Duration) -> Self {
        Self { cool_down }
    }

    pub async fn run_all(
        &self,
        experiments: Vec<(
            ChaosExperiment,
            MetricsCollector,
            MetricsCollector,
            MetricsCollector,
        )>,
    ) -> Result<Vec<ExperimentResult>> {
        let mut results = Vec::new();

        for (experiment, mut baseline, mut chaos, mut recovery) in experiments {
            let result = experiment.run(&mut baseline, &mut chaos, &mut recovery).await?;
            results.push(result);
            tokio::time::sleep(self.cool_down).await;
        }

        Ok(results)
    }

    pub fn generate_report(&self, results: &[ExperimentResult]) -> String {
        let passed = results.iter().filter(|r| r.passed).count();
        let total = results.len();
        let mut report = format!(
            "# Chaos Engineering Report\n\nPassed: {}/{}\n\n",
            passed, total
        );

        for r in results {
            let status = if r.passed { "✅ PASS" } else { "❌ FAIL" };
            report.push_str(&format!(
                "## {} — {}\n\
                 - Chaos error rate:    {:.2}%\n\
                 - Chaos p99 latency:   {:.1}ms\n\
                 - Recovery error rate: {:.2}%\n\n",
                r.name,
                status,
                r.chaos_metrics.error_rate * 100.0,
                r.chaos_metrics.p99_latency_ms,
                r.recovery_metrics.error_rate * 100.0,
            ));
        }

        report
    }
}

/// Convenience: assert all experiments passed (for use in tests).
pub fn assert_all_passed(results: &[ExperimentResult]) {
    let failures: Vec<&str> = results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| r.name.as_str())
        .collect();

    assert!(
        failures.is_empty(),
        "Chaos experiments failed: {:?}",
        failures
    );
}
