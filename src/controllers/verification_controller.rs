use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::verification::{
    CreatorVerification, ReviewVerificationRequest, SubmitVerificationRequest,
};

/// Submit a new verification request (or update a rejected one).
pub async fn submit_verification(
    pool: &PgPool,
    username: &str,
    req: SubmitVerificationRequest,
) -> AppResult<CreatorVerification> {
    // Ensure creator exists
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM creators WHERE username = $1)",
    )
    .bind(username)
    .fetch_one(pool)
    .await?;

    if !exists {
        return Err(AppError::CreatorNotFound {
            username: username.to_string(),
        });
    }

    // Upsert: allow re-submission if previously rejected
    let row = sqlx::query_as::<_, CreatorVerification>(
        r#"
        INSERT INTO creator_verifications
            (id, creator_username, status, identity_doc_url, twitter_handle, github_handle, website_url, submitted_at)
        VALUES ($1, $2, 'pending', $3, $4, $5, $6, NOW())
        ON CONFLICT (creator_username)
        DO UPDATE SET
            status           = CASE WHEN creator_verifications.status = 'rejected' THEN 'pending' ELSE creator_verifications.status END,
            identity_doc_url = EXCLUDED.identity_doc_url,
            twitter_handle   = EXCLUDED.twitter_handle,
            github_handle    = EXCLUDED.github_handle,
            website_url      = EXCLUDED.website_url,
            submitted_at     = NOW(),
            rejection_reason = NULL,
            reviewed_by      = NULL,
            reviewed_at      = NULL
        RETURNING *
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(username)
    .bind(&req.identity_doc_url)
    .bind(&req.twitter_handle)
    .bind(&req.github_handle)
    .bind(&req.website_url)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Get verification status for a creator.
pub async fn get_verification(
    pool: &PgPool,
    username: &str,
) -> AppResult<Option<CreatorVerification>> {
    let row = sqlx::query_as::<_, CreatorVerification>(
        "SELECT * FROM creator_verifications WHERE creator_username = $1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Admin: list pending verification requests.
pub async fn list_pending(pool: &PgPool) -> AppResult<Vec<CreatorVerification>> {
    let rows = sqlx::query_as::<_, CreatorVerification>(
        "SELECT * FROM creator_verifications WHERE status = 'pending' ORDER BY submitted_at ASC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Admin: approve or reject a verification request.
pub async fn review_verification(
    pool: &PgPool,
    verification_id: Uuid,
    admin_username: &str,
    req: ReviewVerificationRequest,
) -> AppResult<CreatorVerification> {
    let new_status = if req.approved { "approved" } else { "rejected" };

    let row = sqlx::query_as::<_, CreatorVerification>(
        r#"
        UPDATE creator_verifications
        SET status           = $1,
            reviewed_by      = $2,
            rejection_reason = $3,
            reviewed_at      = NOW()
        WHERE id = $4
        RETURNING *
        "#,
    )
    .bind(new_status)
    .bind(admin_username)
    .bind(&req.rejection_reason)
    .bind(verification_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Validation(crate::errors::ValidationError::InvalidRequest {
        message: format!("Verification request {} not found", verification_id),
    }))?;

    // If approved, set the badge on the creator
    if req.approved {
        sqlx::query("UPDATE creators SET is_verified = TRUE WHERE username = $1")
            .bind(&row.creator_username)
            .execute(pool)
            .await?;
    }

    Ok(row)
}
