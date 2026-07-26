//! Access-token revocation (#345): a Redis-backed jti denylist plus a
//! per-user token-version epoch, checked together in a single round trip.
//!
//! - `revoke_jti` denylists one specific access token (used by logout).
//! - `bump_epoch` invalidates *every* access token already issued to a user
//!   (used by password change / admin revocation) without having to track
//!   individual jtis — tokens embed the epoch that was current when they
//!   were minted (`Claims::tv`); any token whose `tv` is behind the user's
//!   current epoch is rejected.
//!
//! `check_not_revoked` is the middleware's fail-closed check: if Redis can't
//! be reached, the token is treated as untrusted rather than allowed through.

use redis::aio::ConnectionManager;

use crate::errors::{AppError, AppResult};

fn denylist_key(jti: &str) -> String {
    format!("revoked_jti:{jti}")
}

fn epoch_key(username: &str) -> String {
    format!("token_epoch:{username}")
}

/// Denylists a single access-token jti until it would have expired anyway.
/// `ttl_secs <= 0` means the token is already expired — nothing to do.
pub async fn revoke_jti(conn: &ConnectionManager, jti: &str, ttl_secs: i64) {
    if ttl_secs <= 0 {
        return;
    }
    let mut conn = conn.clone();
    if let Err(e) = redis::cmd("SETEX")
        .arg(denylist_key(jti))
        .arg(ttl_secs as u64)
        .arg("1")
        .query_async::<()>(&mut conn)
        .await
    {
        tracing::warn!(error = %e, jti = %jti, "Failed to denylist jti");
    }
}

/// Advances a user's token-version epoch, invalidating every access token
/// already issued to them (logout-all, password change, admin revocation).
pub async fn bump_epoch(conn: &ConnectionManager, username: &str) {
    let mut conn = conn.clone();
    if let Err(e) = redis::cmd("INCR")
        .arg(epoch_key(username))
        .query_async::<i64>(&mut conn)
        .await
    {
        tracing::warn!(error = %e, username = %username, "Failed to bump token epoch");
    }
}

/// Reads a user's current token-version epoch (0 if never bumped). Used at
/// token-issuance time to stamp `Claims::tv`.
pub async fn current_epoch(conn: &ConnectionManager, username: &str) -> i64 {
    let mut conn = conn.clone();
    redis::cmd("GET")
        .arg(epoch_key(username))
        .query_async::<Option<i64>>(&mut conn)
        .await
        .ok()
        .flatten()
        .unwrap_or(0)
}

/// Single-round-trip (pipelined) check of both revocation signals. Fails
/// closed: a Redis error is treated as "cannot verify the token is still
/// valid" and rejected, rather than silently allowing the request through.
pub async fn check_not_revoked(
    conn: &ConnectionManager,
    jti: &str,
    username: &str,
    token_tv: i64,
) -> AppResult<()> {
    let mut conn = conn.clone();
    let (revoked, epoch): (Option<String>, Option<i64>) = redis::pipe()
        .get(denylist_key(jti))
        .get(epoch_key(username))
        .query_async(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Revocation check failed; failing closed");
            AppError::service_unavailable("Unable to verify session status")
        })?;

    if revoked.is_some() {
        return Err(AppError::unauthorized("Token has been revoked"));
    }
    if token_tv < epoch.unwrap_or(0) {
        return Err(AppError::unauthorized("Token has been revoked"));
    }
    Ok(())
}
