use rust_decimal::Decimal;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::campaign::{Campaign, CampaignMatchResult};

/// Find the highest-value active campaign for a creator and lock it for the duration of the tip update.
pub async fn find_active_campaign_for_creator(
    tx: &mut Transaction<'_, Postgres>,
    creator_username: &str,
) -> AppResult<Option<Campaign>> {
    let campaign = sqlx::query_as::<_, Campaign>(
        r#"
        SELECT id, sponsor_name, creator_username, match_ratio, per_tip_cap,
               total_budget, remaining_budget, active, starts_at, ends_at, created_at
        FROM campaigns
        WHERE creator_username = $1
          AND active = TRUE
          AND (starts_at IS NULL OR starts_at <= NOW())
          AND (ends_at IS NULL OR ends_at >= NOW())
          AND (remaining_budget::numeric > 0)
        ORDER BY match_ratio::numeric DESC, created_at ASC
        LIMIT 1
        FOR UPDATE
        "#,
    )
    .bind(creator_username)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(campaign)
}

/// Attempt to apply a matching campaign to a tip, tracking funds and storing the match row.
pub async fn apply_tip_matching_campaign(
    tx: &mut Transaction<'_, Postgres>,
    creator_username: &str,
    tip_id: Uuid,
    tip_amount: &str,
) -> AppResult<Option<CampaignMatchResult>> {
    let Some(campaign) = find_active_campaign_for_creator(tx, creator_username).await? else {
        return Ok(None);
    };

    let tip_amount = Decimal::from_str(tip_amount)
        .map_err(|_| crate::errors::AppError::internal())?;
    let match_ratio = Decimal::from_str(&campaign.match_ratio)
        .map_err(|_| crate::errors::AppError::internal())?;
    let mut matched_amount = tip_amount * match_ratio;

    let per_tip_cap = Decimal::from_str(&campaign.per_tip_cap)
        .map_err(|_| crate::errors::AppError::internal())?;
    if per_tip_cap > Decimal::ZERO {
        matched_amount = matched_amount.min(per_tip_cap);
    }

    let remaining_budget = Decimal::from_str(&campaign.remaining_budget)
        .map_err(|_| crate::errors::AppError::internal())?;
    matched_amount = matched_amount.min(remaining_budget);

    if matched_amount <= Decimal::ZERO {
        return Ok(None);
    }

    let matched_amount_str = matched_amount.to_string();
    sqlx::query(
        r#"
        INSERT INTO campaign_matches (campaign_id, tip_id, matched_amount)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(campaign.id)
    .bind(tip_id)
    .bind(&matched_amount_str)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE campaigns
        SET remaining_budget = (remaining_budget::numeric - $1::numeric)::text
        WHERE id = $2
        "#,
    )
    .bind(&matched_amount_str)
    .bind(campaign.id)
    .execute(&mut **tx)
    .await?;

    Ok(Some(CampaignMatchResult {
        campaign_id: campaign.id,
        sponsor_name: campaign.sponsor_name,
        matched_amount: matched_amount_str,
    }))
}
