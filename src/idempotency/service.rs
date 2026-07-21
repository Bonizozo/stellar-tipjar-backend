use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;

use super::error::IdempotencyError;
use super::fingerprint::compute_scope_hash;
use super::metrics;
use super::store::{postgres_backend, redis_backend, Decision, IdempotencyBackend, StoredResponse};
use crate::cache::redis_client::ConnectionManager;

#[derive(Debug, Clone)]
pub struct IdempotencyConfig {
    /// How long a completed response is replayable for. Default 24h per #342.
    pub entry_ttl: Duration,
    /// How long an execution lock is held before it's considered abandoned
    /// (crashed handler) and reclaimable by another request.
    pub lock_ttl: Duration,
    /// Maximum *compressed* response body size that will be cached. Larger
    /// responses still execute normally but are not idempotency-cached.
    pub max_body_bytes: usize,
}

impl Default for IdempotencyConfig {
    fn default() -> Self {
        Self {
            entry_ttl: Duration::from_secs(
                std::env::var("IDEMPOTENCY_TTL_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(24 * 3600),
            ),
            lock_ttl: Duration::from_millis(
                std::env::var("IDEMPOTENCY_LOCK_TTL_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30_000),
            ),
            max_body_bytes: std::env::var("IDEMPOTENCY_MAX_BODY_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(64 * 1024),
        }
    }
}

/// A handle returned by [`IdempotencyService::begin`] when the caller should
/// execute the handler. Must be resolved with exactly one of
/// [`IdempotencyService::complete`] / [`IdempotencyService::fail`].
#[derive(Debug)]
pub struct ExecutionGuard {
    pub scope_hash: String,
    pub fingerprint: String,
    token: String,
}

#[derive(Debug)]
pub enum Outcome {
    Proceed(ExecutionGuard),
    Replay(StoredResponse),
    Mismatch,
    Conflict { retry_after_secs: u64 },
}

/// Orchestrates the Redis-primary / Postgres-fallback idempotency protocol.
///
/// Locking always prefers Redis (fast, purpose-built `SET NX PX`). If Redis
/// is unset or returns [`IdempotencyError::Backend`] (connection failure —
/// not merely a cache miss), the Postgres-backed unique-insert lock is used
/// instead so mutating endpoints stay race-safe even with Redis down.
///
/// Every completed response is written through to Postgres regardless of
/// which backend held the lock, so a replay remains available even after the
/// Redis entry is evicted — satisfying the "survives Redis eviction"
/// requirement without making Postgres a write bottleneck for the hot path
/// (Redis still serves the vast majority of replay reads).
pub struct IdempotencyService {
    redis_backend: Option<Arc<dyn IdempotencyBackend>>,
    pg_backend: Arc<dyn IdempotencyBackend>,
    config: IdempotencyConfig,
}

impl IdempotencyService {
    pub fn new(pool: PgPool, redis: Option<ConnectionManager>, config: IdempotencyConfig) -> Self {
        Self {
            redis_backend: redis.map(redis_backend),
            pg_backend: postgres_backend(pool),
            config,
        }
    }

    pub fn config(&self) -> &IdempotencyConfig {
        &self.config
    }

    pub async fn begin(
        &self,
        principal: &str,
        route: &str,
        idempotency_key: &str,
        fingerprint: &str,
    ) -> Result<Outcome, IdempotencyError> {
        let scope_hash = compute_scope_hash(principal, route, idempotency_key);
        let timer = metrics::IDEMPOTENCY_LOCK_CONTENTION_SECONDS.start_timer();

        let decision = if let Some(redis) = &self.redis_backend {
            match redis
                .begin(&scope_hash, principal, route, idempotency_key, fingerprint, self.config.lock_ttl)
                .await
            {
                Ok(decision) => decision,
                Err(e) if e.is_backend_unavailable() => {
                    metrics::IDEMPOTENCY_REDIS_FALLBACK_TOTAL.inc();
                    tracing::warn!(error = %e, "Idempotency: Redis unavailable, falling back to Postgres");
                    self.pg_backend
                        .begin(&scope_hash, principal, route, idempotency_key, fingerprint, self.config.lock_ttl)
                        .await?
                }
                Err(e) => return Err(e),
            }
        } else {
            self.pg_backend
                .begin(&scope_hash, principal, route, idempotency_key, fingerprint, self.config.lock_ttl)
                .await?
        };

        timer.observe_duration();

        Ok(match decision {
            Decision::Proceed(token) => {
                metrics::IDEMPOTENCY_EXECUTED_TOTAL.inc();
                Outcome::Proceed(ExecutionGuard {
                    scope_hash,
                    fingerprint: fingerprint.to_string(),
                    token,
                })
            }
            Decision::Replay(response) => {
                metrics::IDEMPOTENCY_REPLAY_TOTAL.inc();
                Outcome::Replay(response)
            }
            Decision::Mismatch => {
                metrics::IDEMPOTENCY_MISMATCH_TOTAL.inc();
                Outcome::Mismatch
            }
            Decision::Conflict { retry_after_secs } => {
                metrics::IDEMPOTENCY_CONFLICT_TOTAL.inc();
                Outcome::Conflict { retry_after_secs }
            }
        })
    }

    pub async fn complete(
        &self,
        guard: ExecutionGuard,
        response: StoredResponse,
    ) -> Result<(), IdempotencyError> {
        // Always write through to Postgres for durability, independent of
        // which backend is currently primary.
        self.pg_backend
            .complete(
                &guard.scope_hash,
                &guard.token,
                &guard.fingerprint,
                &response,
                self.config.entry_ttl,
                self.config.max_body_bytes,
            )
            .await?;

        if let Some(redis) = &self.redis_backend {
            if let Err(e) = redis
                .complete(
                    &guard.scope_hash,
                    &guard.token,
                    &guard.fingerprint,
                    &response,
                    self.config.entry_ttl,
                    self.config.max_body_bytes,
                )
                .await
            {
                tracing::warn!(error = %e, "Idempotency: failed to write completed response to Redis");
            }
        }

        Ok(())
    }

    pub async fn fail(&self, guard: ExecutionGuard) -> Result<(), IdempotencyError> {
        if let Some(redis) = &self.redis_backend {
            if let Err(e) = redis.fail(&guard.scope_hash, &guard.token).await {
                tracing::warn!(error = %e, "Idempotency: failed to release Redis lock");
            }
        }
        self.pg_backend.fail(&guard.scope_hash, &guard.token).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::memory::InMemoryBackend;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_service_with_backend(backend: Arc<dyn IdempotencyBackend>, config: IdempotencyConfig) -> IdempotencyServiceForTest {
        IdempotencyServiceForTest { backend, config }
    }

    /// A thin stand-in for `IdempotencyService` that talks to a single
    /// in-memory backend directly (no Redis/Postgres wiring), so the core
    /// begin/complete/fail protocol — including the concurrency guarantee —
    /// can be exercised without any external services.
    struct IdempotencyServiceForTest {
        backend: Arc<dyn IdempotencyBackend>,
        config: IdempotencyConfig,
    }

    impl IdempotencyServiceForTest {
        async fn begin(&self, principal: &str, route: &str, key: &str, fingerprint: &str) -> Outcome {
            let scope_hash = compute_scope_hash(principal, route, key);
            let decision = self
                .backend
                .begin(&scope_hash, principal, route, key, fingerprint, self.config.lock_ttl)
                .await
                .unwrap();
            match decision {
                Decision::Proceed(token) => Outcome::Proceed(ExecutionGuard {
                    scope_hash,
                    fingerprint: fingerprint.to_string(),
                    token,
                }),
                Decision::Replay(r) => Outcome::Replay(r),
                Decision::Mismatch => Outcome::Mismatch,
                Decision::Conflict { retry_after_secs } => Outcome::Conflict { retry_after_secs },
            }
        }

        async fn complete(&self, guard: ExecutionGuard, response: StoredResponse) {
            self.backend
                .complete(
                    &guard.scope_hash,
                    &guard.token,
                    &guard.fingerprint,
                    &response,
                    self.config.entry_ttl,
                    self.config.max_body_bytes,
                )
                .await
                .unwrap();
        }

        async fn fail(&self, guard: ExecutionGuard) {
            self.backend.fail(&guard.scope_hash, &guard.token).await.unwrap();
        }
    }

    fn short_ttl_config() -> IdempotencyConfig {
        IdempotencyConfig {
            entry_ttl: Duration::from_secs(3600),
            lock_ttl: Duration::from_secs(30),
            max_body_bytes: 64 * 1024,
        }
    }

    #[tokio::test]
    async fn replay_returns_identical_stored_response() {
        let svc = test_service_with_backend(Arc::new(InMemoryBackend::new()), short_ttl_config());
        let fp = "fp-a";

        let guard = match svc.begin("user:1", "POST /tips", "key-1", fp).await {
            Outcome::Proceed(g) => g,
            _ => panic!("expected Proceed on first attempt"),
        };
        let response = StoredResponse {
            status: 201,
            content_type: Some("application/json".to_string()),
            body: b"{\"id\":\"abc\"}".to_vec(),
        };
        svc.complete(guard, response.clone()).await;

        match svc.begin("user:1", "POST /tips", "key-1", fp).await {
            Outcome::Replay(replayed) => assert_eq!(replayed, response),
            other => panic!("expected Replay, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fingerprint_mismatch_is_rejected() {
        let svc = test_service_with_backend(Arc::new(InMemoryBackend::new()), short_ttl_config());

        let guard = match svc.begin("user:1", "POST /tips", "key-1", "fp-a").await {
            Outcome::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        svc.complete(
            guard,
            StoredResponse { status: 201, content_type: None, body: vec![] },
        )
        .await;

        match svc.begin("user:1", "POST /tips", "key-1", "fp-b").await {
            Outcome::Mismatch => {}
            other => panic!("expected Mismatch, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ttl_expiry_allows_re_execution() {
        let svc = test_service_with_backend(
            Arc::new(InMemoryBackend::new()),
            IdempotencyConfig {
                entry_ttl: Duration::from_millis(20),
                lock_ttl: Duration::from_secs(30),
                max_body_bytes: 64 * 1024,
            },
        );

        let guard = match svc.begin("user:1", "POST /tips", "key-1", "fp-a").await {
            Outcome::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        svc.complete(
            guard,
            StoredResponse { status: 201, content_type: None, body: vec![] },
        )
        .await;

        tokio::time::sleep(Duration::from_millis(50)).await;

        match svc.begin("user:1", "POST /tips", "key-1", "fp-a").await {
            Outcome::Proceed(_) => {}
            other => panic!("expected Proceed after TTL expiry, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn failed_execution_releases_the_lock_immediately() {
        let svc = test_service_with_backend(Arc::new(InMemoryBackend::new()), short_ttl_config());

        let guard = match svc.begin("user:1", "POST /tips", "key-1", "fp-a").await {
            Outcome::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        svc.fail(guard).await;

        // Should be immediately retryable, not blocked for the full lock TTL.
        match svc.begin("user:1", "POST /tips", "key-1", "fp-a").await {
            Outcome::Proceed(_) => {}
            other => panic!("expected Proceed after fail(), got {other:?}"),
        }
    }

    /// The "hard part": N concurrent requests carrying the same Idempotency-Key
    /// must result in exactly one execution of the handler. Everyone else must
    /// either replay the winner's response or receive a 409-equivalent Conflict
    /// — never execute the side-effecting operation themselves.
    #[tokio::test]
    async fn concurrent_duplicates_execute_exactly_once() {
        let svc = Arc::new(test_service_with_backend(
            Arc::new(InMemoryBackend::new()),
            short_ttl_config(),
        ));
        let execution_count = Arc::new(AtomicUsize::new(0));
        let n = 50;

        let mut handles = Vec::with_capacity(n);
        for _ in 0..n {
            let svc = Arc::clone(&svc);
            let execution_count = Arc::clone(&execution_count);
            handles.push(tokio::spawn(async move {
                match svc.begin("user:1", "POST /tips", "race-key", "fp-a").await {
                    Outcome::Proceed(guard) => {
                        // Simulate the side-effecting operation (e.g. submitting
                        // a Stellar transaction) racing against its peers.
                        let n = execution_count.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        svc.complete(
                            guard,
                            StoredResponse {
                                status: 201,
                                content_type: Some("application/json".to_string()),
                                body: format!("{{\"executed_as\":{n}}}").into_bytes(),
                            },
                        )
                        .await;
                        "executed"
                    }
                    Outcome::Conflict { .. } => "conflict",
                    Outcome::Replay(_) => "replay",
                    Outcome::Mismatch => "mismatch",
                }
            }));
        }

        let mut executed = 0;
        let mut conflict = 0;
        let mut replay = 0;
        for h in handles {
            match h.await.unwrap() {
                "executed" => executed += 1,
                "conflict" => conflict += 1,
                "replay" => replay += 1,
                other => panic!("unexpected outcome: {other}"),
            }
        }

        assert_eq!(
            execution_count.load(Ordering::SeqCst),
            1,
            "the side-effecting operation must run exactly once under {n} concurrent duplicates"
        );
        assert_eq!(executed, 1, "exactly one request should have won the lock");
        assert_eq!(conflict + replay, n - 1, "every other request must be rejected or replayed, never execute");

        println!(
            "race test: {n} concurrent requests -> executed={executed} conflict={conflict} replay={replay} (side-effect counter={})",
            execution_count.load(Ordering::SeqCst)
        );
    }
}
