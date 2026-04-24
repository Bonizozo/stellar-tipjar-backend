use std::time::Duration;

use stellar_tipjar_backend::chaos::{
    experiments::{assert_all_passed, ChaosExperiment, ChaosRunner, ResilienceThresholds},
    injectors::{
        ChaosInjector, DatabaseFailureInjector, LatencyInjector, NetworkPartitionInjector,
        ServiceCrashInjector,
    },
    metrics::MetricsCollector,
    scenarios::ChaosScenarios,
};

// ── Injector unit tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn latency_injector_activates_and_recovers() {
    let inj = LatencyInjector::new("test-service", Duration::from_millis(10));
    assert!(!inj.is_active());

    inj.inject().await.unwrap();
    assert!(inj.is_active());

    inj.recover().await.unwrap();
    assert!(!inj.is_active());
}

#[tokio::test]
async fn latency_injector_applies_delay_when_active() {
    let inj = LatencyInjector::new("test-service", Duration::from_millis(50));
    inj.inject().await.unwrap();

    let start = std::time::Instant::now();
    inj.maybe_delay().await;
    assert!(start.elapsed() >= Duration::from_millis(40));

    inj.recover().await.unwrap();
    let start = std::time::Instant::now();
    inj.maybe_delay().await;
    assert!(start.elapsed() < Duration::from_millis(10));
}

#[tokio::test]
async fn database_failure_injector_activates_and_recovers() {
    let inj = DatabaseFailureInjector::new(1.0);
    inj.inject().await.unwrap();

    // With 100 % failure rate, maybe_fail must always return Err.
    assert!(inj.maybe_fail().is_err());

    inj.recover().await.unwrap();
    // After recovery, maybe_fail must always return Ok.
    assert!(inj.maybe_fail().is_ok());
}

#[tokio::test]
async fn database_failure_injector_zero_rate_never_fails() {
    let inj = DatabaseFailureInjector::new(0.0);
    inj.inject().await.unwrap();
    for _ in 0..100 {
        assert!(inj.maybe_fail().is_ok());
    }
}

#[tokio::test]
async fn network_partition_injector_toggles() {
    let inj = NetworkPartitionInjector::new("stellar-horizon");
    assert!(!inj.is_partitioned());

    inj.inject().await.unwrap();
    assert!(inj.is_partitioned());

    inj.recover().await.unwrap();
    assert!(!inj.is_partitioned());
}

#[tokio::test]
async fn service_crash_injector_toggles() {
    let inj = ServiceCrashInjector::new("tip-service");
    assert!(!inj.is_crashed());

    inj.inject().await.unwrap();
    assert!(inj.is_crashed());

    inj.recover().await.unwrap();
    assert!(!inj.is_crashed());
}

// ── Metrics tests ─────────────────────────────────────────────────────────────

#[test]
fn metrics_collector_computes_error_rate() {
    let mut col = MetricsCollector::new();
    col.record_success(Duration::from_millis(10));
    col.record_success(Duration::from_millis(20));
    col.record_error();

    let m = col.snapshot();
    assert!((m.error_rate - 1.0 / 3.0).abs() < 0.01);
    assert_eq!(m.success_count, 2);
    assert_eq!(m.failure_count, 1);
}

#[test]
fn metrics_collector_p99_latency() {
    let mut col = MetricsCollector::new();
    for ms in 1u64..=100 {
        col.record_success(Duration::from_millis(ms));
    }
    let m = col.snapshot();
    // p99 of 1..=100 ms should be close to 99 ms.
    assert!(m.p99_latency_ms >= 95.0 && m.p99_latency_ms <= 100.0);
}

#[test]
fn metrics_collector_empty_snapshot() {
    let mut col = MetricsCollector::new();
    let m = col.snapshot();
    assert_eq!(m.error_rate, 0.0);
    assert_eq!(m.p99_latency_ms, 0.0);
}

// ── Experiment evaluation tests ───────────────────────────────────────────────

#[tokio::test]
async fn experiment_passes_when_thresholds_met() {
    let experiment = ChaosExperiment::new("test", Duration::from_millis(1))
        .with_injector(Box::new(LatencyInjector::new("svc", Duration::from_millis(1))))
        .with_thresholds(ResilienceThresholds {
            max_error_rate_during_chaos: 0.10,
            max_latency_multiplier: 10.0,
            max_recovery_error_rate_multiplier: 2.0,
        });

    let mut baseline = MetricsCollector::new();
    let mut chaos = MetricsCollector::new();
    let mut recovery = MetricsCollector::new();

    // Simulate healthy metrics.
    baseline.record_success(Duration::from_millis(5));
    chaos.record_success(Duration::from_millis(8));
    recovery.record_success(Duration::from_millis(5));

    let result = experiment.run(&mut baseline, &mut chaos, &mut recovery).await.unwrap();
    assert!(result.passed, "Experiment should pass with healthy metrics");
}

#[tokio::test]
async fn experiment_fails_when_error_rate_exceeded() {
    let experiment = ChaosExperiment::new("test", Duration::from_millis(1))
        .with_injector(Box::new(DatabaseFailureInjector::new(0.0)))
        .with_thresholds(ResilienceThresholds {
            max_error_rate_during_chaos: 0.05,
            max_latency_multiplier: 2.0,
            max_recovery_error_rate_multiplier: 1.1,
        });

    let mut baseline = MetricsCollector::new();
    let mut chaos = MetricsCollector::new();
    let mut recovery = MetricsCollector::new();

    // Simulate 50 % error rate during chaos.
    chaos.record_success(Duration::from_millis(5));
    chaos.record_error();

    let result = experiment.run(&mut baseline, &mut chaos, &mut recovery).await.unwrap();
    assert!(!result.passed, "Experiment should fail with high error rate");
}

// ── Scenario smoke tests ──────────────────────────────────────────────────────

#[test]
fn scenarios_all_returns_six_experiments() {
    assert_eq!(ChaosScenarios::all().len(), 6);
}

#[test]
fn scenario_names_are_unique() {
    let names: Vec<_> = ChaosScenarios::all().iter().map(|e| e.name.clone()).collect();
    let unique: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(names.len(), unique.len());
}

// ── Runner report test ────────────────────────────────────────────────────────

#[tokio::test]
async fn runner_generates_report() {
    let runner = ChaosRunner::new(Duration::from_millis(1));

    let experiment = ChaosExperiment::new("Report Test", Duration::from_millis(1))
        .with_injector(Box::new(LatencyInjector::new("svc", Duration::from_millis(1))));

    let experiments = vec![(
        experiment,
        MetricsCollector::new(),
        MetricsCollector::new(),
        MetricsCollector::new(),
    )];

    let results = runner.run_all(experiments).await.unwrap();
    let report = runner.generate_report(&results);

    assert!(report.contains("Chaos Engineering Report"));
    assert!(report.contains("Report Test"));
    assert_eq!(results.len(), 1);
}

#[test]
fn assert_all_passed_panics_on_failure() {
    use stellar_tipjar_backend::chaos::experiments::ExperimentResult;
    use stellar_tipjar_backend::chaos::metrics::Metrics;

    let results = vec![ExperimentResult {
        name: "failing".into(),
        baseline: Metrics::default(),
        chaos_metrics: Metrics::default(),
        recovery_metrics: Metrics::default(),
        passed: false,
    }];

    let result = std::panic::catch_unwind(|| assert_all_passed(&results));
    assert!(result.is_err(), "Should panic when an experiment failed");
}
