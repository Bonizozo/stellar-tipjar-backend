use crate::errors::app_error::AppError;
use crate::models::tip::Tip;
use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Features {
    pub amount: f64,
    pub amount_normalized: f32,
    pub hour_of_day: f32,
    pub day_of_week: f32,
    pub creator_total_tips: i32,
    pub creator_avg_tip: f64,
    pub creator_account_age_days: i32,
    pub sender_total_sent: f64,
    pub sender_tip_count: i32,
    pub sender_first_tip_days_ago: i32,
    pub tips_last_hour: i32,
    pub tips_last_day: i32,
    pub previous_tips_to_creator: i32,
}

pub struct FeatureExtractor {
    pool: PgPool,
}

impl FeatureExtractor {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn extract_features(&self, tip: &Tip) -> Result<Features, AppError> {
        let creator_history = self.get_creator_history(&tip.creator_username).await?;
        let sender_history = self.get_sender_history(&tip.transaction_hash).await?;

        let tips_last_hour = self.count_recent_tips(&tip.transaction_hash, 3600).await?;
        let tips_last_day = self.count_recent_tips(&tip.transaction_hash, 86400).await?;
        let previous_tips = self
            .count_tips_to_creator(&tip.transaction_hash, &tip.creator_username)
            .await?;

        let amount: f64 = tip.amount.parse().unwrap_or(0.0);

        Ok(Features {
            amount,
            amount_normalized: (amount / 100.0) as f32,
            hour_of_day: tip.created_at.hour() as f32,
            day_of_week: tip.created_at.weekday().number_from_monday() as f32,
            creator_total_tips: creator_history.0,
            creator_avg_tip: creator_history.1,
            creator_account_age_days: creator_history.2,
            sender_total_sent: sender_history.0,
            sender_tip_count: sender_history.1,
            sender_first_tip_days_ago: sender_history.2,
            tips_last_hour,
            tips_last_day,
            previous_tips_to_creator: previous_tips,
        })
    }

    async fn get_creator_history(&self, username: &str) -> Result<(i32, f64, i32), AppError> {
        let result: (i64, Option<f64>, Option<i64>) = sqlx::query_as(
            "SELECT COUNT(*), AVG(amount), EXTRACT(DAY FROM NOW() - MIN(created_at))::int 
             FROM tips WHERE creator_username = $1",
        )
        .bind(username)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))?;

        Ok((
            result.0 as i32,
            result.1.unwrap_or(0.0),
            result.2.unwrap_or(0) as i32,
        ))
    }

    async fn get_sender_history(&self, tx_hash: &str) -> Result<(f64, i32, i32), AppError> {
        let result: (Option<f64>, i64, Option<i64>) = sqlx::query_as(
            "SELECT SUM(amount), COUNT(*), EXTRACT(DAY FROM NOW() - MIN(created_at))::int 
             FROM tips WHERE transaction_hash = $1",
        )
        .bind(tx_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))?;

        Ok((
            result.0.unwrap_or(0.0),
            result.1 as i32,
            result.2.unwrap_or(0) as i32,
        ))
    }

    async fn count_recent_tips(&self, tx_hash: &str, seconds: i64) -> Result<i32, AppError> {
        let result: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM tips WHERE transaction_hash = $1 
             AND created_at > NOW() - INTERVAL '1 second' * $2",
        )
        .bind(tx_hash)
        .bind(seconds)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))?;

        Ok(result.0 as i32)
    }

    async fn count_tips_to_creator(&self, tx_hash: &str, creator: &str) -> Result<i32, AppError> {
        let result: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM tips WHERE transaction_hash = $1 AND creator_username = $2",
        )
        .bind(tx_hash)
        .bind(creator)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))?;

        Ok(result.0 as i32)
    }
}
