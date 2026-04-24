use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::subscription::{
    AddBenefitRequest, CreateSubscriptionRequest, CreateTierRequest, Subscription,
    SubscriptionPayment, SubscriptionTier, TierBenefit, UpdateTierRequest,
};

// ── Tiers ─────────────────────────────────────────────────────────────────────

pub async fn create_tier(
    pool: &PgPool,
    creator_username: &str,
    req: CreateTierRequest,
) -> Result<(SubscriptionTier, Vec<TierBenefit>), sqlx::Error> {
    let mut tx = pool.begin().await?;

    let tier = sqlx::query_as::<_, SubscriptionTier>(
        "INSERT INTO subscription_tiers (creator_username, name, description, price_xlm, position)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(creator_username)
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.price_xlm)
    .bind(req.position)
    .fetch_one(&mut *tx)
    .await?;

    let mut benefits = Vec::new();
    for (i, desc) in req.benefits.iter().enumerate() {
        let b = sqlx::query_as::<_, TierBenefit>(
            "INSERT INTO tier_benefits (tier_id, description, position) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(tier.id)
        .bind(desc)
        .bind(i as i32)
        .fetch_one(&mut *tx)
        .await?;
        benefits.push(b);
    }

    tx.commit().await?;
    Ok((tier, benefits))
}

pub async fn get_tier(pool: &PgPool, id: Uuid) -> Result<SubscriptionTier, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionTier>("SELECT * FROM subscription_tiers WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
}

pub async fn list_tiers(
    pool: &PgPool,
    creator_username: &str,
) -> Result<Vec<SubscriptionTier>, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionTier>(
        "SELECT * FROM subscription_tiers WHERE creator_username = $1 ORDER BY position ASC, created_at ASC",
    )
    .bind(creator_username)
    .fetch_all(pool)
    .await
}

pub async fn update_tier(
    pool: &PgPool,
    id: Uuid,
    req: UpdateTierRequest,
) -> Result<SubscriptionTier, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionTier>(
        "UPDATE subscription_tiers SET
            name        = COALESCE($2, name),
            description = COALESCE($3, description),
            price_xlm   = COALESCE($4, price_xlm),
            is_active   = COALESCE($5, is_active),
            position    = COALESCE($6, position),
            updated_at  = NOW()
         WHERE id = $1
         RETURNING *",
    )
    .bind(id)
    .bind(req.name)
    .bind(req.description)
    .bind(req.price_xlm)
    .bind(req.is_active)
    .bind(req.position)
    .fetch_one(pool)
    .await
}

pub async fn delete_tier(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let rows = sqlx::query("DELETE FROM subscription_tiers WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(rows > 0)
}

// ── Benefits ──────────────────────────────────────────────────────────────────

pub async fn list_benefits(pool: &PgPool, tier_id: Uuid) -> Result<Vec<TierBenefit>, sqlx::Error> {
    sqlx::query_as::<_, TierBenefit>(
        "SELECT * FROM tier_benefits WHERE tier_id = $1 ORDER BY position ASC",
    )
    .bind(tier_id)
    .fetch_all(pool)
    .await
}

pub async fn add_benefit(
    pool: &PgPool,
    tier_id: Uuid,
    req: AddBenefitRequest,
) -> Result<TierBenefit, sqlx::Error> {
    sqlx::query_as::<_, TierBenefit>(
        "INSERT INTO tier_benefits (tier_id, description, position) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(tier_id)
    .bind(&req.description)
    .bind(req.position)
    .fetch_one(pool)
    .await
}

pub async fn remove_benefit(pool: &PgPool, benefit_id: Uuid) -> Result<bool, sqlx::Error> {
    let rows = sqlx::query("DELETE FROM tier_benefits WHERE id = $1")
        .bind(benefit_id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(rows > 0)
}

// ── Subscriptions ─────────────────────────────────────────────────────────────

pub async fn subscribe(
    pool: &PgPool,
    req: CreateSubscriptionRequest,
) -> Result<Subscription, sqlx::Error> {
    let tier = get_tier(pool, req.tier_id).await?;
    let period_end = Utc::now() + Duration::days(30);

    let mut tx = pool.begin().await?;

    let sub = sqlx::query_as::<_, Subscription>(
        "INSERT INTO subscriptions
            (tier_id, creator_username, subscriber_ref, current_period_end)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(req.tier_id)
    .bind(&tier.creator_username)
    .bind(&req.subscriber_ref)
    .bind(period_end)
    .fetch_one(&mut *tx)
    .await?;

    // Record the initial payment
    sqlx::query(
        "INSERT INTO subscription_payments
            (subscription_id, amount_xlm, transaction_hash, status, period_start, period_end)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(sub.id)
    .bind(&tier.price_xlm)
    .bind(&req.transaction_hash)
    .bind(if req.transaction_hash.is_some() { "completed" } else { "pending" })
    .bind(sub.current_period_start)
    .bind(period_end)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(sub)
}

pub async fn get_subscription(pool: &PgPool, id: Uuid) -> Result<Subscription, sqlx::Error> {
    sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
}

pub async fn list_subscriptions_for_subscriber(
    pool: &PgPool,
    subscriber_ref: &str,
) -> Result<Vec<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        "SELECT * FROM subscriptions WHERE subscriber_ref = $1 ORDER BY created_at DESC",
    )
    .bind(subscriber_ref)
    .fetch_all(pool)
    .await
}

pub async fn list_subscriptions_for_creator(
    pool: &PgPool,
    creator_username: &str,
) -> Result<Vec<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        "SELECT * FROM subscriptions WHERE creator_username = $1 AND status = 'active' ORDER BY created_at DESC",
    )
    .bind(creator_username)
    .fetch_all(pool)
    .await
}

pub async fn cancel_subscription(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let rows = sqlx::query(
        "UPDATE subscriptions SET status = 'cancelled', cancelled_at = NOW(), updated_at = NOW()
         WHERE id = $1 AND status = 'active'",
    )
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rows > 0)
}

/// Renew a subscription by advancing the period by 30 days and recording payment.
pub async fn renew_subscription(
    pool: &PgPool,
    id: Uuid,
    transaction_hash: Option<String>,
) -> Result<Subscription, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let sub = sqlx::query_as::<_, Subscription>(
        "UPDATE subscriptions SET
            current_period_start = current_period_end,
            current_period_end   = current_period_end + INTERVAL '30 days',
            status               = 'active',
            updated_at           = NOW()
         WHERE id = $1
         RETURNING *",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;

    let tier = get_tier(pool, sub.tier_id).await?;

    sqlx::query(
        "INSERT INTO subscription_payments
            (subscription_id, amount_xlm, transaction_hash, status, period_start, period_end)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(sub.id)
    .bind(&tier.price_xlm)
    .bind(&transaction_hash)
    .bind(if transaction_hash.is_some() { "completed" } else { "pending" })
    .bind(sub.current_period_start)
    .bind(sub.current_period_end)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(sub)
}

// ── Payments ──────────────────────────────────────────────────────────────────

pub async fn list_payments(
    pool: &PgPool,
    subscription_id: Uuid,
) -> Result<Vec<SubscriptionPayment>, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionPayment>(
        "SELECT * FROM subscription_payments WHERE subscription_id = $1 ORDER BY created_at DESC",
    )
    .bind(subscription_id)
    .fetch_all(pool)
    .await
}

/// Fetch all active subscriptions whose period has expired — used by the renewal processor.
pub async fn due_renewals(pool: &PgPool) -> Result<Vec<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        "SELECT * FROM subscriptions
         WHERE status = 'active' AND current_period_end <= NOW()
         ORDER BY current_period_end ASC
         LIMIT 100",
    )
    .fetch_all(pool)
    .await
}

/// Mark a subscription as past_due when automatic renewal fails.
pub async fn mark_past_due(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE subscriptions SET status = 'past_due', updated_at = NOW() WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}
