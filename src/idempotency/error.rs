/// Errors internal to the idempotency subsystem.
///
/// `Backend` specifically means "this storage backend could not be reached"
/// (e.g. Redis connection refused) — [`crate::idempotency::service::IdempotencyService`]
/// uses this variant to decide whether to fall back from Redis to Postgres.
/// Any other error is a hard failure that should surface as a 500.
#[derive(Debug, thiserror::Error)]
pub enum IdempotencyError {
    #[error("idempotency backend unavailable: {0}")]
    Backend(String),
    #[error("idempotency record serialization failed: {0}")]
    Serialization(String),
}

impl IdempotencyError {
    pub fn is_backend_unavailable(&self) -> bool {
        matches!(self, Self::Backend(_))
    }
}
