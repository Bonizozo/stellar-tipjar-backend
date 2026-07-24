//! First-class `Idempotency-Key` semantics for mutating endpoints (#342).
//!
//! Replaces the earlier best-effort `deduplication` module, which cached
//! responses per-request-fingerprint but had no concurrency guarantee (two
//! in-flight duplicates would both execute) and no durable fallback once a
//! Redis entry was evicted. See [`store`] for the locking design and
//! [`middleware`] for how routes opt in.
//!
//! ```ignore
//! use axum::{routing::post, Router};
//!
//! Router::new()
//!     .route("/tips", post(record_tip))
//!     .route_layer(axum::middleware::from_fn_with_state(
//!         state,
//!         idempotency::middleware::idempotency_middleware,
//!     ))
//! ```

pub mod error;
pub mod fingerprint;
pub mod metrics;
pub mod middleware;
pub mod service;
pub mod store;

pub use error::IdempotencyError;
pub use service::{ExecutionGuard, IdempotencyConfig, IdempotencyService, Outcome};
pub use store::StoredResponse;
