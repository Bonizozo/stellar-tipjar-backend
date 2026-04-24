use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::refund::{CreateRefundRequest, ReviewRefundRequest, TipRefund};

pub async fn create_refund(db: &PgPool, req: CreateRefundRequest) -> AppResult<TipRefund> {
    // Verify the tip exists
    let tip_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tips WHERE id = $1)")
            .bind(req.tip_id)
            .fetch_one(db)
            .await?;
    if !tip_exists {
        return Err(AppError::Validation(crate::errors::ValidationError::InvalidRequest {
            message: format!("Tip {} not found", req.tip_id),
        }));
    }

    // Prevent duplicate pending refund for the same tip
    let pending_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tip_refunds WHERE tip_id = $1 AND status = 'pending')",
    )
    .bind(req.tip_id)
    .fetch_one(db)
    .await?;
    if pending_exists {
        return Err(AppError::Conflict {
            code: "REFUND_ALREADY_PENDING",
            message: "A refund request for this tip is already pending".to_string(),
        });
    }

    let refund = sqlx::query_as::<_, TipRefund>(
        "INSERT INTO tip_refunds (id, tip_id, reason)
         VALUES ($1, $2, $3)
         RETURNING id, tip_id, reason, status, refund_tx_hash, reviewed_by, reviewed_at, created_at, updated_at",
    )
    .bind(Uuid::new_v4())
    .bind(req.tip_id)
    .bind(&req.reason)
    .fetch_one(db)
    .await?;

    Ok(refund)
}

pub async fn list_refunds(db: &PgPool, status: Option<&str>) -> AppResult<Vec<TipRefund>> {
    let refunds = if let Some(s) = status {
        sqlx::query_as::<_, TipRefund>(
            "SELECT id, tip_id, reason, status, refund_tx_hash, reviewed_by, reviewed_at, created_at, updated_at
             FROM tip_refunds WHERE status = $1 ORDER BY created_at DESC",
        )
        .bind(s)
        .fetch_all(db)
        .await?
    } else {
        sqlx::query_as::<_, TipRefund>(
            "SELECT id, tip_id, reason, status, refund_tx_hash, reviewed_by, reviewed_at, created_at, updated_at
             FROM tip_refunds ORDER BY created_at DESC",
        )
        .fetch_all(db)
        .await?
    };
    Ok(refunds)
}

pub async fn get_refund(db: &PgPool, id: Uuid) -> AppResult<Option<TipRefund>> {
    let refund = sqlx::query_as::<_, TipRefund>(
        "SELECT id, tip_id, reason, status, refund_tx_hash, reviewed_by, reviewed_at, created_at, updated_at
         FROM tip_refunds WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(refund)
}

pub async fn review_refund(
    db: &PgPool,
    id: Uuid,
    reviewer: &str,
    req: ReviewRefundRequest,
) -> AppResult<Option<TipRefund>> {
    let action = req.action.as_str();
    if action != "approved" && action != "rejected" {
        return Err(AppError::Validation(crate::errors::ValidationError::InvalidRequest {
            message: "action must be 'approved' or 'rejected'".to_string(),
        }));
    }
    if action == "approved" && req.refund_tx_hash.is_none() {
        return Err(AppError::Validation(crate::errors::ValidationError::InvalidRequest {
            message: "refund_tx_hash is required when approving a refund".to_string(),
        }));
    }

    let new_status = if action == "approved" { "completed" } else { "rejected" };

    let refund = sqlx::query_as::<_, TipRefund>(
        "UPDATE tip_refunds
         SET status = $1, refund_tx_hash = COALESCE($2, refund_tx_hash),
             reviewed_by = $3, reviewed_at = NOW(), updated_at = NOW()
         WHERE id = $4 AND status = 'pending'
         RETURNING id, tip_id, reason, status, refund_tx_hash, reviewed_by, reviewed_at, created_at, updated_at",
    )
    .bind(new_status)
    .bind(req.refund_tx_hash.as_deref())
    .bind(reviewer)
    .bind(id)
    .fetch_optional(db)
    .await?;

    Ok(refund)
}
