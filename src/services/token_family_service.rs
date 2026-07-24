//! Refresh-token family persistence: rotation and reuse detection (#345).
//!
//! Every refresh token belongs to a "family" created at login. Each
//! successful `/auth/refresh` call advances `current_jti` to a freshly
//! minted value. If a token whose `jti` no longer matches `current_jti` is
//! ever presented, it means a token was replayed after the legitimate client
//! already rotated past it — a strong signal of theft — so the entire
//! family is revoked and the user must re-authenticate.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

pub const FAMILY_TTL_DAYS: i64 = 7;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct TokenFamily {
    pub id: Uuid,
    pub username: String,
    pub current_jti: Uuid,
    pub revoked: bool,
    pub revoked_reason: Option<String>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub rotated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl From<TokenFamily> for crate::models::auth::SessionSummary {
    fn from(f: TokenFamily) -> Self {
        Self {
            family_id: f.id,
            user_agent: f.user_agent,
            ip_address: f.ip_address,
            created_at: f.created_at,
            rotated_at: f.rotated_at,
            expires_at: f.expires_at,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationOutcome {
    /// The presented refresh token was the current one for its family; the
    /// family has been advanced to the newly issued jti.
    Rotated,
    /// The presented refresh token had already been rotated away — a stolen
    /// token replayed after the legitimate client rotated. The family has
    /// been revoked as a result of this call.
    ReuseDetected,
    /// The family was already revoked (prior reuse detection, logout, or
    /// password change).
    AlreadyRevoked,
    /// The family's absolute lifetime has elapsed.
    Expired,
    /// No such family exists.
    NotFound,
}

/// Pure decision function — no I/O — so the critical reuse-detection path is
/// exhaustively unit-testable without a database.
fn decide_rotation(family: &TokenFamily, presented_jti: Uuid, now: DateTime<Utc>) -> RotationOutcome {
    if family.revoked {
        return RotationOutcome::AlreadyRevoked;
    }
    if now > family.expires_at {
        return RotationOutcome::Expired;
    }
    if family.current_jti == presented_jti {
        RotationOutcome::Rotated
    } else {
        RotationOutcome::ReuseDetected
    }
}

/// Creates a new refresh-token family (called once, at login/register/recover).
pub async fn create_family(
    pool: &PgPool,
    username: &str,
    refresh_jti: Uuid,
    user_agent: Option<&str>,
    ip_address: Option<&str>,
) -> AppResult<Uuid> {
    let id = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(FAMILY_TTL_DAYS);
    sqlx::query(
        "INSERT INTO refresh_token_families (id, username, current_jti, user_agent, ip_address, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(username)
    .bind(refresh_jti)
    .bind(user_agent)
    .bind(ip_address)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(AppError::from)?;
    Ok(id)
}

/// Attempts to rotate `family_id` from `presented_jti` to `new_jti`. Locks
/// the row for the duration of the check-and-update so concurrent refresh
/// attempts on the same family can't race past the reuse check.
pub async fn rotate_or_detect_reuse(
    pool: &PgPool,
    family_id: Uuid,
    presented_jti: Uuid,
    new_jti: Uuid,
) -> AppResult<RotationOutcome> {
    let mut tx = pool.begin().await.map_err(AppError::from)?;

    let family = sqlx::query_as::<_, TokenFamily>(
        "SELECT * FROM refresh_token_families WHERE id = $1 FOR UPDATE",
    )
    .bind(family_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?;

    let Some(family) = family else {
        tx.rollback().await.ok();
        return Ok(RotationOutcome::NotFound);
    };

    let outcome = decide_rotation(&family, presented_jti, Utc::now());

    match outcome {
        RotationOutcome::Rotated => {
            sqlx::query(
                "UPDATE refresh_token_families SET current_jti = $1, rotated_at = NOW() WHERE id = $2",
            )
            .bind(new_jti)
            .bind(family_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }
        RotationOutcome::ReuseDetected => {
            sqlx::query(
                "UPDATE refresh_token_families SET revoked = TRUE, revoked_reason = 'reuse_detected' WHERE id = $1",
            )
            .bind(family_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            tracing::error!(
                family_id = %family_id,
                username = %family.username,
                "Refresh token reuse detected — family revoked"
            );
        }
        RotationOutcome::AlreadyRevoked | RotationOutcome::Expired | RotationOutcome::NotFound => {}
    }

    tx.commit().await.map_err(AppError::from)?;
    Ok(outcome)
}

/// Revokes a single family. Returns `true` if a row was actually revoked.
pub async fn revoke_family(pool: &PgPool, family_id: Uuid, reason: &str) -> AppResult<bool> {
    let result = sqlx::query(
        "UPDATE refresh_token_families SET revoked = TRUE, revoked_reason = $1 WHERE id = $2 AND revoked = FALSE",
    )
    .bind(reason)
    .bind(family_id)
    .execute(pool)
    .await
    .map_err(AppError::from)?;
    Ok(result.rows_affected() > 0)
}

/// Revokes every active family for a user (logout-all, password change,
/// admin revocation, or a compromised-family fanout).
pub async fn revoke_all_for_user(pool: &PgPool, username: &str, reason: &str) -> AppResult<u64> {
    let result = sqlx::query(
        "UPDATE refresh_token_families SET revoked = TRUE, revoked_reason = $1 WHERE username = $2 AND revoked = FALSE",
    )
    .bind(reason)
    .bind(username)
    .execute(pool)
    .await
    .map_err(AppError::from)?;
    Ok(result.rows_affected())
}

pub async fn get_family(pool: &PgPool, family_id: Uuid) -> AppResult<Option<TokenFamily>> {
    sqlx::query_as::<_, TokenFamily>("SELECT * FROM refresh_token_families WHERE id = $1")
        .bind(family_id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)
}

/// Lists a user's active (non-revoked, non-expired) sessions.
pub async fn list_active_for_user(pool: &PgPool, username: &str) -> AppResult<Vec<TokenFamily>> {
    sqlx::query_as::<_, TokenFamily>(
        "SELECT * FROM refresh_token_families
         WHERE username = $1 AND revoked = FALSE AND expires_at > NOW()
         ORDER BY created_at DESC",
    )
    .bind(username)
    .fetch_all(pool)
    .await
    .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn family(current_jti: Uuid, revoked: bool, expires_at: DateTime<Utc>) -> TokenFamily {
        TokenFamily {
            id: Uuid::new_v4(),
            username: "alice".to_string(),
            current_jti,
            revoked,
            revoked_reason: None,
            user_agent: None,
            ip_address: None,
            created_at: Utc::now(),
            rotated_at: Utc::now(),
            expires_at,
        }
    }

    #[test]
    fn rotation_happy_path_advances_family() {
        let current = Uuid::new_v4();
        let f = family(current, false, Utc::now() + Duration::days(1));
        assert_eq!(decide_rotation(&f, current, Utc::now()), RotationOutcome::Rotated);
    }

    /// The critical test: a refresh token that was already rotated away
    /// (stolen and replayed) must be detected as reuse, and once the family
    /// is revoked, *no* token for that family — old or new — is accepted again.
    #[test]
    fn reuse_of_already_rotated_token_is_detected_and_kills_family() {
        let stolen = Uuid::new_v4(); // the token an attacker captured
        let current = Uuid::new_v4(); // what the legitimate client rotated to
        let f = family(current, false, Utc::now() + Duration::days(1));

        let outcome = decide_rotation(&f, stolen, Utc::now());
        assert_eq!(outcome, RotationOutcome::ReuseDetected);

        // Simulate the caller applying the revocation triggered by that outcome.
        let revoked_family = family(current, true, Utc::now() + Duration::days(1));
        assert_eq!(
            decide_rotation(&revoked_family, current, Utc::now()),
            RotationOutcome::AlreadyRevoked
        );
        assert_eq!(
            decide_rotation(&revoked_family, stolen, Utc::now()),
            RotationOutcome::AlreadyRevoked
        );
    }

    #[test]
    fn expired_family_rejected_even_with_current_jti() {
        let current = Uuid::new_v4();
        let f = family(current, false, Utc::now() - Duration::seconds(1));
        assert_eq!(decide_rotation(&f, current, Utc::now()), RotationOutcome::Expired);
    }

    #[test]
    fn mismatched_jti_on_a_fresh_family_is_reuse_not_a_silent_pass() {
        let current = Uuid::new_v4();
        let other = Uuid::new_v4();
        let f = family(current, false, Utc::now() + Duration::days(1));
        assert_eq!(decide_rotation(&f, other, Utc::now()), RotationOutcome::ReuseDetected);
    }
}
