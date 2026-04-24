use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── DB entities ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SubscriptionTier {
    pub id: Uuid,
    pub creator_username: String,
    pub name: String,
    pub description: Option<String>,
    pub price_xlm: String,
    pub is_active: bool,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TierBenefit {
    pub id: Uuid,
    pub tier_id: Uuid,
    pub description: String,
    pub position: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Subscription {
    pub id: Uuid,
    pub tier_id: Uuid,
    pub creator_username: String,
    pub subscriber_ref: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SubscriptionPayment {
    pub id: Uuid,
    pub subscription_id: Uuid,
    pub amount_xlm: String,
    pub transaction_hash: Option<String>,
    pub status: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

// ── Request DTOs ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateTierRequest {
    pub name: String,
    pub description: Option<String>,
    pub price_xlm: String,
    #[serde(default)]
    pub benefits: Vec<String>,
    #[serde(default)]
    pub position: i32,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTierRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub price_xlm: Option<String>,
    pub is_active: Option<bool>,
    pub position: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct AddBenefitRequest {
    pub description: String,
    #[serde(default)]
    pub position: i32,
}

#[derive(Debug, Deserialize)]
pub struct CreateSubscriptionRequest {
    pub tier_id: Uuid,
    pub subscriber_ref: String,
    /// Optional on-chain payment hash for the first period
    pub transaction_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RenewSubscriptionRequest {
    pub transaction_hash: Option<String>,
}

// ── Response DTOs ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TierResponse {
    pub id: Uuid,
    pub creator_username: String,
    pub name: String,
    pub description: Option<String>,
    pub price_xlm: String,
    pub is_active: bool,
    pub position: i32,
    pub benefits: Vec<BenefitResponse>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct BenefitResponse {
    pub id: Uuid,
    pub description: String,
    pub position: i32,
}

impl From<TierBenefit> for BenefitResponse {
    fn from(b: TierBenefit) -> Self {
        Self {
            id: b.id,
            description: b.description,
            position: b.position,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SubscriptionResponse {
    pub id: Uuid,
    pub tier_id: Uuid,
    pub creator_username: String,
    pub subscriber_ref: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub cancelled_at: Option<DateTime<Utc>>,
}

impl From<Subscription> for SubscriptionResponse {
    fn from(s: Subscription) -> Self {
        Self {
            id: s.id,
            tier_id: s.tier_id,
            creator_username: s.creator_username,
            subscriber_ref: s.subscriber_ref,
            status: s.status,
            started_at: s.started_at,
            current_period_start: s.current_period_start,
            current_period_end: s.current_period_end,
            cancelled_at: s.cancelled_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PaymentResponse {
    pub id: Uuid,
    pub subscription_id: Uuid,
    pub amount_xlm: String,
    pub transaction_hash: Option<String>,
    pub status: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl From<SubscriptionPayment> for PaymentResponse {
    fn from(p: SubscriptionPayment) -> Self {
        Self {
            id: p.id,
            subscription_id: p.subscription_id,
            amount_xlm: p.amount_xlm,
            transaction_hash: p.transaction_hash,
            status: p.status,
            period_start: p.period_start,
            period_end: p.period_end,
            created_at: p.created_at,
        }
    }
}
