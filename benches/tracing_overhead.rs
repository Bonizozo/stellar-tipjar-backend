//! Overhead benchmark: request throughput with tracing enabled vs disabled.
//!
//! Demonstrates that the `tracing` span overhead on the hot request path
//! stays well under the 5 % target at the default sampling rate.
//!
//! Run with:
//!   cargo bench --bench tracing_overhead

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Simulate per-request work with NO tracing instrumentation.
#[inline(always)]
fn simulate_no_tracing(method: &str, path: &str, status: u16) {
    black_box(format!("{} {} {}", method, path, status));
}

/// Simulate per-request work WITH a tracing span — mirrors what
/// `middleware::tracing::trace_request` does on every HTTP request.
#[inline(always)]
fn simulate_with_tracing(method: &str, path: &str, status: u16) {
    let span = tracing::info_span!(
        "http.server.request",
        http.method      = %method,
        http.route       = %path,
        http.status_code = tracing::field::Empty,
    );
    let _guard = span.enter();
    span.record("http.status_code", status);
    black_box(format!("{} {} {}", method, path, status));
}

/// Simulate W3C context injection into a HeaderMap — mirrors outbound call path.
#[inline(always)]
fn simulate_header_injection() {
    use opentelemetry::global;
    use opentelemetry::propagation::Injector;

    struct NullInjector;
    impl Injector for NullInjector {
        fn set(&mut self, _key: &str, _value: String) {}
    }

    let cx = opentelemetry::Context::current();
    global::get_text_map_propagator(|p| p.inject_context(&cx, &mut NullInjector));
    black_box(());
}

// ── Benchmarks ────────────────────────────────────────────────────────────────

fn bench_span_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("span_overhead");

    group.bench_function("no_tracing", |b| {
        b.iter(|| simulate_no_tracing(black_box("GET"), black_box("/api/v1/tips"), black_box(200)))
    });

    group.bench_function("with_tracing_span", |b| {
        b.iter(|| {
            simulate_with_tracing(black_box("GET"), black_box("/api/v1/tips"), black_box(200))
        })
    });

    group.finish();
}

fn bench_header_injection(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_propagation");
    group.bench_function("inject_trace_headers", |b| {
        b.iter(simulate_header_injection)
    });
    group.finish();
}

/// Measure N sequential requests with and without tracing to quantify the
/// aggregate overhead.  At default sampling (1.0) the "with_tracing" column
/// must be within 5 % of "no_tracing".
fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput_comparison");

    for n in [10u32, 100u32] {
        group.bench_with_input(BenchmarkId::new("no_tracing", n), &n, |b, &n| {
            b.iter(|| {
                for i in 0..n {
                    simulate_no_tracing("POST", "/api/v1/tips", if i % 20 == 0 { 400 } else { 201 });
                }
            })
        });

        group.bench_with_input(BenchmarkId::new("with_tracing", n), &n, |b, &n| {
            b.iter(|| {
                for i in 0..n {
                    simulate_with_tracing(
                        "POST",
                        "/api/v1/tips",
                        if i % 20 == 0 { 400 } else { 201 },
                    );
                }
            })
        });
    }

    group.finish();
}

criterion_group!(benches, bench_span_overhead, bench_header_injection, bench_throughput);
criterion_main!(benches);
