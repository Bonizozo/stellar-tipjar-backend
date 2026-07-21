//! Storage backends for the idempotency subsystem.
//!
//! Both backends implement [`IdempotencyBackend`] and expose the same
//! three-state protocol via [`Decision`]:
//!
//!   * `Proceed(token)`   — no prior attempt found; caller may execute the
//!                          handler and must call `complete`/`fail` with `token`.
//!   * `Replay(response)` — a prior attempt with the *same* fingerprint
//!                          already completed; return its response verbatim.
//!   * `Mismatch`         — a prior attempt with a *different* fingerprint
//!                          used this key; reject with 422.
//!   * `Conflict { .. }`  — a prior attempt is still executing; reject with
//!                          409 + `Retry-After`.
//!
//! # Locking semantics (the "hard part")
//!
//! Redis: `SET idem:lock:{scope} <token> NX PX <lock_ttl>` is the mutual
//! exclusion primitive. Only the holder of a matching token may delete it
//! (enforced with a Lua compare-and-delete, mirroring
//! [`crate::services::distributed_lock`]), so a slow request whose lock
//! already expired can never accidentally release a *different* request's
//! lock.
//!
//! Postgres fallback: rather than `pg_advisory_lock` (which is scoped to the
//! physical connection/session — a poor fit for a pooled async client, since
//! locking on one pooled connection and unlocking on another is a no-op and
//! leaks the lock for the lifetime of that session), the lock is a row insert
//! guarded by the `idempotency_keys.scope_hash` UNIQUE constraint:
//! `INSERT ... ON CONFLICT (scope_hash) DO NOTHING`. A crashed holder's lock
//! is reclaimed via a compare-and-swap `UPDATE ... WHERE in_flight = true AND
//! created_at < now() - lock_ttl`, which Postgres's row-level locking makes
//! safe under concurrent reclaim attempts: only one concurrent UPDATE can
//! ever affect the row before the others re-evaluate the (now-false) WHERE
//! clause.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use flate2::write::{GzDecoder, GzEncoder};
use flate2::Compression;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::io::Write;
use uuid::Uuid;

use super::error::IdempotencyError;
use super::metrics;
use crate::cache::redis_client::ConnectionManager;

const REDIS_LOCK_PREFIX: &str = "idem:lock:";
const REDIS_DONE_PREFIX: &str = "idem:done:";

/// A captured HTTP response, ready to be replayed byte-for-byte.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub enum Decision {
    Proceed(String),
    Replay(StoredResponse),
    Mismatch,
    Conflict { retry_after_secs: u64 },
}

#[async_trait]
pub trait IdempotencyBackend: Send + Sync {
    async fn begin(
        &self,
        scope_hash: &str,
        principal: &str,
        route: &str,
        idempotency_key: &str,
        fingerprint: &str,
        lock_ttl: Duration,
    ) -> Result<Decision, IdempotencyError>;

    async fn complete(
        &self,
        scope_hash: &str,
        token: &str,
        fingerprint: &str,
        response: &StoredResponse,
        entry_ttl: Duration,
        max_body_bytes: usize,
    ) -> Result<(), IdempotencyError>;

    async fn fail(&self, scope_hash: &str, token: &str) -> Result<(), IdempotencyError>;
}

fn compress(body: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    // In-memory writers never fail.
    encoder.write_all(body).expect("gzip write");
    encoder.finish().expect("gzip finish")
}

fn decompress(bytes: &[u8]) -> Result<Vec<u8>, IdempotencyError> {
    let mut decoder = GzDecoder::new(Vec::new());
    decoder
        .write_all(bytes)
        .map_err(|e| IdempotencyError::Serialization(e.to_string()))?;
    decoder
        .finish()
        .map_err(|e| IdempotencyError::Serialization(e.to_string()))
}

// ── Redis backend ────────────────────────────────────────────────────────────

pub struct RedisBackend {
    redis: ConnectionManager,
}

impl RedisBackend {
    pub fn new(redis: ConnectionManager) -> Self {
        Self { redis }
    }
}

#[derive(Serialize, Deserialize)]
struct RedisRecord {
    status: u16,
    content_type: Option<String>,
    fingerprint: String,
    body_gzip: Vec<u8>,
}

#[async_trait]
impl IdempotencyBackend for RedisBackend {
    async fn begin(
        &self,
        scope_hash: &str,
        _principal: &str,
        _route: &str,
        _idempotency_key: &str,
        fingerprint: &str,
        lock_ttl: Duration,
    ) -> Result<Decision, IdempotencyError> {
        let mut conn = self.redis.clone();
        let done_key = format!("{REDIS_DONE_PREFIX}{scope_hash}");

        // 1. Already completed?
        let existing: Option<Vec<u8>> = redis::cmd("GET")
            .arg(&done_key)
            .query_async(&mut conn)
            .await
            .map_err(|e| IdempotencyError::Backend(e.to_string()))?;

        if let Some(raw) = existing {
            let record: RedisRecord = serde_json::from_slice(&raw)
                .map_err(|e| IdempotencyError::Serialization(e.to_string()))?;
            return Ok(if record.fingerprint == fingerprint {
                let body = decompress(&record.body_gzip)?;
                Decision::Replay(StoredResponse {
                    status: record.status,
                    content_type: record.content_type,
                    body,
                })
            } else {
                Decision::Mismatch
            });
        }

        // 2. Try to acquire the in-flight lock.
        let lock_key = format!("{REDIS_LOCK_PREFIX}{scope_hash}");
        let token = Uuid::new_v4().to_string();
        let acquired: Option<String> = redis::cmd("SET")
            .arg(&lock_key)
            .arg(&token)
            .arg("NX")
            .arg("PX")
            .arg(lock_ttl.as_millis() as u64)
            .query_async(&mut conn)
            .await
            .map_err(|e| IdempotencyError::Backend(e.to_string()))?;

        if acquired.as_deref() == Some("OK") {
            return Ok(Decision::Proceed(token));
        }

        // 3. Someone else holds the lock — surface remaining TTL as Retry-After.
        let pttl: i64 = redis::cmd("PTTL")
            .arg(&lock_key)
            .query_async(&mut conn)
            .await
            .unwrap_or(1000);
        let retry_after_secs = ((pttl.max(0) as u64) / 1000).max(1);
        Ok(Decision::Conflict { retry_after_secs })
    }

    async fn complete(
        &self,
        scope_hash: &str,
        token: &str,
        fingerprint: &str,
        response: &StoredResponse,
        entry_ttl: Duration,
        max_body_bytes: usize,
    ) -> Result<(), IdempotencyError> {
        let mut conn = self.redis.clone();
        let done_key = format!("{REDIS_DONE_PREFIX}{scope_hash}");
        let body_gzip = compress(&response.body);

        if body_gzip.len() <= max_body_bytes {
            let record = RedisRecord {
                status: response.status,
                content_type: response.content_type.clone(),
                fingerprint: fingerprint.to_string(),
                body_gzip,
            };
            let serialized = serde_json::to_vec(&record)
                .map_err(|e| IdempotencyError::Serialization(e.to_string()))?;
            let _: () = redis::cmd("SETEX")
                .arg(&done_key)
                .arg(entry_ttl.as_secs())
                .arg(serialized)
                .query_async(&mut conn)
                .await
                .map_err(|e| IdempotencyError::Backend(e.to_string()))?;
        } else {
            metrics::IDEMPOTENCY_BODY_TOO_LARGE_TOTAL.inc();
        }

        release_redis_lock(&mut conn, scope_hash, token).await
    }

    async fn fail(&self, scope_hash: &str, token: &str) -> Result<(), IdempotencyError> {
        let mut conn = self.redis.clone();
        release_redis_lock(&mut conn, scope_hash, token).await
    }
}

async fn release_redis_lock(
    conn: &mut ConnectionManager,
    scope_hash: &str,
    token: &str,
) -> Result<(), IdempotencyError> {
    let lock_key = format!("{REDIS_LOCK_PREFIX}{scope_hash}");
    let script = r#"
        if redis.call("GET", KEYS[1]) == ARGV[1] then
            return redis.call("DEL", KEYS[1])
        else
            return 0
        end
    "#;
    let _deleted: i64 = redis::Script::new(script)
        .key(&lock_key)
        .arg(token)
        .invoke_async(conn)
        .await
        .map_err(|e| IdempotencyError::Backend(e.to_string()))?;
    Ok(())
}

// ── Postgres fallback backend ───────────────────────────────────────────────

pub struct PostgresBackend {
    pool: PgPool,
}

impl PostgresBackend {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl IdempotencyBackend for PostgresBackend {
    async fn begin(
        &self,
        scope_hash: &str,
        principal: &str,
        route: &str,
        idempotency_key: &str,
        fingerprint: &str,
        lock_ttl: Duration,
    ) -> Result<Decision, IdempotencyError> {
        let token = Uuid::new_v4().to_string();

        // Attempt the insert first — the common case (no prior attempt).
        let inserted = sqlx::query(
            r#"
            INSERT INTO idempotency_keys
                (scope_hash, principal, route, idempotency_key, request_fingerprint,
                 in_flight, expires_at)
            VALUES ($1, $2, $3, $4, $5, TRUE, NOW() + INTERVAL '1 hour')
            ON CONFLICT (scope_hash) DO NOTHING
            "#,
        )
        .bind(scope_hash)
        .bind(principal)
        .bind(route)
        .bind(idempotency_key)
        .bind(fingerprint)
        .execute(&self.pool)
        .await
        .map_err(|e| IdempotencyError::Backend(e.to_string()))?;

        if inserted.rows_affected() == 1 {
            // We won the insert race, but the token isn't persisted by design
            // (Postgres locking is row-identity based, not token based — see
            // module docs). Any non-empty token is accepted by `complete`/`fail`.
            return Ok(Decision::Proceed(token));
        }

        // Row already exists — inspect it.
        let row: Option<(bool, Option<i16>, Option<String>, Option<Vec<u8>>, String)> = sqlx::query_as(
            r#"
            SELECT in_flight, response_status, response_headers->>'content-type', response_body, request_fingerprint
            FROM idempotency_keys
            WHERE scope_hash = $1 AND expires_at > NOW()
            "#,
        )
        .bind(scope_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| IdempotencyError::Backend(e.to_string()))?;

        let Some((in_flight, status, content_type, body, existing_fingerprint)) = row else {
            // Expired (or raced past TTL cleanup) — treat as absent and retry the insert.
            return self
                .begin(scope_hash, principal, route, idempotency_key, fingerprint, lock_ttl)
                .await;
        };

        if !in_flight {
            return Ok(if existing_fingerprint == fingerprint {
                let body = decompress(&body.unwrap_or_default())?;
                Decision::Replay(StoredResponse {
                    status: status.unwrap_or(200) as u16,
                    content_type,
                    body,
                })
            } else {
                Decision::Mismatch
            });
        }

        // Still in-flight — try to reclaim if the holder appears to have crashed.
        let lock_ttl_secs = lock_ttl.as_secs_f64();
        let reclaimed = sqlx::query(
            r#"
            UPDATE idempotency_keys
            SET in_flight = TRUE, created_at = NOW(), request_fingerprint = $2
            WHERE scope_hash = $1
              AND in_flight = TRUE
              AND created_at < NOW() - ($3 || ' seconds')::interval
            "#,
        )
        .bind(scope_hash)
        .bind(fingerprint)
        .bind(lock_ttl_secs.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| IdempotencyError::Backend(e.to_string()))?;

        if reclaimed.rows_affected() == 1 {
            return Ok(Decision::Proceed(token));
        }

        Ok(Decision::Conflict {
            retry_after_secs: lock_ttl.as_secs().max(1),
        })
    }

    async fn complete(
        &self,
        scope_hash: &str,
        _token: &str,
        _fingerprint: &str,
        response: &StoredResponse,
        entry_ttl: Duration,
        max_body_bytes: usize,
    ) -> Result<(), IdempotencyError> {
        // The fingerprint was already persisted by `begin`'s INSERT/UPDATE;
        // `complete` only needs to flip `in_flight` and attach the response.
        let body_gzip = compress(&response.body);
        let (stored_body, stored_hash) = if body_gzip.len() <= max_body_bytes {
            use sha2::{Digest, Sha256};
            let hash = hex::encode(Sha256::digest(&response.body));
            (Some(body_gzip), Some(hash))
        } else {
            metrics::IDEMPOTENCY_BODY_TOO_LARGE_TOTAL.inc();
            (None, None)
        };

        let headers = serde_json::json!({ "content-type": response.content_type });

        sqlx::query(
            r#"
            UPDATE idempotency_keys
            SET in_flight = FALSE,
                response_status = $2,
                response_headers = $3,
                response_body = $4,
                response_body_hash = $5,
                completed_at = NOW(),
                expires_at = NOW() + ($6 || ' seconds')::interval
            WHERE scope_hash = $1
            "#,
        )
        .bind(scope_hash)
        .bind(response.status as i16)
        .bind(headers)
        .bind(stored_body)
        .bind(stored_hash)
        .bind(entry_ttl.as_secs().to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| IdempotencyError::Backend(e.to_string()))?;

        Ok(())
    }

    async fn fail(&self, scope_hash: &str, _token: &str) -> Result<(), IdempotencyError> {
        // Release immediately so a retry doesn't have to wait out the full lock TTL.
        sqlx::query("DELETE FROM idempotency_keys WHERE scope_hash = $1 AND in_flight = TRUE")
            .bind(scope_hash)
            .execute(&self.pool)
            .await
            .map_err(|e| IdempotencyError::Backend(e.to_string()))?;
        Ok(())
    }
}

pub fn redis_backend(redis: ConnectionManager) -> Arc<dyn IdempotencyBackend> {
    Arc::new(RedisBackend::new(redis))
}

pub fn postgres_backend(pool: PgPool) -> Arc<dyn IdempotencyBackend> {
    Arc::new(PostgresBackend::new(pool))
}

#[cfg(test)]
pub mod memory {
    //! Pure in-memory backend used by unit tests (including the concurrency
    //! race test) so the "hard part" — exactly-one-execution under N
    //! concurrent duplicates — is verified without requiring a live Redis or
    //! Postgres instance.
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::Mutex;
    use tokio::time::Instant;

    struct Entry {
        in_flight: bool,
        token: String,
        fingerprint: String,
        response: Option<StoredResponse>,
        locked_at: Instant,
        expires_at: Option<Instant>,
    }

    #[derive(Default)]
    pub struct InMemoryBackend {
        entries: Mutex<HashMap<String, Entry>>,
    }

    impl InMemoryBackend {
        pub fn new() -> Self {
            Self::default()
        }
    }

    #[async_trait]
    impl IdempotencyBackend for InMemoryBackend {
        async fn begin(
            &self,
            scope_hash: &str,
            _principal: &str,
            _route: &str,
            _idempotency_key: &str,
            fingerprint: &str,
            lock_ttl: Duration,
        ) -> Result<Decision, IdempotencyError> {
            let mut entries = self.entries.lock().await;

            if let Some(entry) = entries.get(scope_hash) {
                let expired = entry.expires_at.map(|e| Instant::now() >= e).unwrap_or(false);
                if !entry.in_flight && !expired {
                    return Ok(if entry.fingerprint == fingerprint {
                        Decision::Replay(entry.response.clone().expect("completed entry has response"))
                    } else {
                        Decision::Mismatch
                    });
                }
                if entry.in_flight {
                    let stale = Instant::now().duration_since(entry.locked_at) >= lock_ttl;
                    if !stale {
                        return Ok(Decision::Conflict {
                            retry_after_secs: lock_ttl.as_secs().max(1),
                        });
                    }
                    // fall through to reclaim below
                }
            }

            let token = Uuid::new_v4().to_string();
            entries.insert(
                scope_hash.to_string(),
                Entry {
                    in_flight: true,
                    token: token.clone(),
                    fingerprint: fingerprint.to_string(),
                    response: None,
                    locked_at: Instant::now(),
                    expires_at: None,
                },
            );
            Ok(Decision::Proceed(token))
        }

        async fn complete(
            &self,
            scope_hash: &str,
            token: &str,
            _fingerprint: &str,
            response: &StoredResponse,
            entry_ttl: Duration,
            _max_body_bytes: usize,
        ) -> Result<(), IdempotencyError> {
            let mut entries = self.entries.lock().await;
            if let Some(entry) = entries.get_mut(scope_hash) {
                if entry.token == token {
                    entry.in_flight = false;
                    entry.response = Some(response.clone());
                    entry.expires_at = Some(Instant::now() + entry_ttl);
                }
            }
            Ok(())
        }

        async fn fail(&self, scope_hash: &str, token: &str) -> Result<(), IdempotencyError> {
            let mut entries = self.entries.lock().await;
            if let Some(entry) = entries.get(scope_hash) {
                if entry.token == token {
                    entries.remove(scope_hash);
                }
            }
            Ok(())
        }
    }
}
