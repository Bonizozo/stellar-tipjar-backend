## Summary

This PR implements four platform capabilities across testing, architecture, distributed workflows, and observability:

1. API mocking infrastructure for integration tests and local development.
2. CQRS improvements with explicit command handlers and read-model synchronization.
3. Saga orchestration hardening for distributed transactions.
4. Real-time metrics dashboard improvements with Prometheus recording rules and Grafana aggregation.

## What Changed

### #240 API Mocking for Testing
- Added a dedicated mock server engine with runtime registration and replay:
  - `src/mocking/server.rs`
- Added mock data generators for creators, tips, tx hashes, IDs, emails:
  - `src/mocking/generators.rs`
- Upgraded request matching:
  - wildcard/path-param matching (`*`, `:param`)
  - query subset matching
  - optional header constraints via `x-mock-match-*`
- Added response template rendering:
  - request-aware placeholders (`{{request.method}}`, `{{request.path}}`, path params)
  - dynamic data placeholders (`{{random.uuid}}`, `{{random.tx_hash}}`, etc.)
- Extended mock recording:
  - JSON export/import
  - replay into registry as mock entries
- Added/extended tests in `tests/feature_tests.rs`.

### #229 CQRS Pattern
- Added explicit command handler abstraction and concrete handlers:
  - `src/cqrs/handlers.rs`
- Refactored command dispatch to use handler registry:
  - `src/cqrs/command_bus.rs`
- Added read-model sync report + incremental synchronization:
  - `src/cqrs/projections.rs`
  - `src/cqrs/synchronizer.rs`
- Added read-optimized `CreatorSummaryView` query path:
  - `src/cqrs/queries.rs`
  - `src/cqrs/query_bus.rs`
- Exported new CQRS primitives in `src/cqrs/mod.rs`.

### #234 Saga Pattern for Distributed Transactions
- Added saga step retry controls (`max_retries`, `retry_backoff_ms`):
  - `src/saga/step.rs`
- Updated tip saga to define retry/compensation behavior per step:
  - `src/saga/tip_saga.rs`
- Enhanced orchestrator:
  - step retry loop
  - explicit failed/compensated state transitions
  - partial-failure compensation execution in reverse order
  - improved failure reason propagation
  - `src/saga/orchestrator.rs`
- Improved compensation handler with pluggable compensation hooks:
  - `src/saga/compensation.rs`
- Added status aggregation query in saga monitoring:
  - `src/saga/monitoring.rs`
- Cleaned duplicate saga module declaration:
  - `src/saga/mod.rs`

### #236 Real-time Metrics Dashboard
- Added business metric instrumentation in write paths:
  - creator registration counter (`CREATORS_REGISTERED_TOTAL`)
  - tip success/failure counters and amount histogram
  - corrected DB query histogram labeling usage
  - `src/controllers/creator_controller.rs`
  - `src/controllers/tip_controller.rs`
- Added Prometheus recording rules for metric aggregation:
  - `monitoring/prometheus/recording_rules.yml`
  - wired into `monitoring/prometheus/prometheus.yml`
  - mounted in `compose.yml`
- Updated Grafana dashboard to consume aggregated time series:
  - `monitoring/grafana/dashboards/tipjar_overview.json`

## Validation

- Targeted test command attempted: `cargo test --test feature_tests api_mocking -- --nocapture`
- Full compile/test is currently blocked by pre-existing repository issues unrelated to this PR:
  - duplicate module declarations in other areas
  - existing SQLx online macro requirements without prepared cache/DB
  - existing unrelated type/trait errors in untouched modules

## Issue Closure

Closes #229  
Closes #234  
Closes #236  
Closes #240

