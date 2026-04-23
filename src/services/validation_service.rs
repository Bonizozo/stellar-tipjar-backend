use rust_decimal::Decimal;
use sqlx::PgPool;
use std::str::FromStr;

use crate::errors::{AppError, AppResult};
use crate::models::tip::RecordTipRequest;

/// Configurable limits for tip validation. Defaults are loaded from environment
/// variables so they can be tuned without recompiling.
pub struct ValidationRules {
    pub min_tip_xlm: Decimal,
    pub max_tip_xlm: Decimal,
}

impl Default for ValidationRules {
    fn default() -> Self {
        let min = std::env::var("TIP_MIN_XLM")
            .ok()
            .and_then(|v| Decimal::from_str(&v).ok())
            .unwrap_or_else(|| Decimal::from_str("0.01").unwrap());
        let max = std::env::var("TIP_MAX_XLM")
            .ok()
            .and_then(|v| Decimal::from_str(&v).ok())
            .unwrap_or_else(|| Decimal::from_str("10000").unwrap());
        Self { min_tip_xlm: min, max_tip_xlm: max }
    }
}

pub struct TipValidationService {
    rules: ValidationRules,
}

impl TipValidationService {
    pub fn new(rules: ValidationRules) -> Self {
        Self { rules }
    }

    /// Run all business-logic checks before a tip is persisted.
    /// Call this after input validation (ValidatedJson) but before Stellar verification.
    pub async fn validate(&self, pool: &PgPool, req: &RecordTipRequest) -> AppResult<()> {
        self.check_amount(&req.amount)?;
        self.check_duplicate(pool, &req.transaction_hash).await?;
        self.check_creator_exists(pool, &req.username).await?;
        self.check_fraud_indicators(pool, req).await;
        Ok(())
    }

    fn check_amount(&self, amount: &str) -> AppResult<()> {
        let value = Decimal::from_str(amount).map_err(|_| AppError::Conflict {
            code: "INVALID_AMOUNT",
            message: "Tip amount is not a valid decimal".to_string(),
        })?;

        if value < self.rules.min_tip_xlm {
            return Err(AppError::Conflict {
                code: "AMOUNT_TOO_LOW",
                message: format!("Minimum tip amount is {} XLM", self.rules.min_tip_xlm),
            });
        }
        if value > self.rules.max_tip_xlm {
            return Err(AppError::Conflict {
                code: "AMOUNT_TOO_HIGH",
                message: format!("Maximum tip amount is {} XLM", self.rules.max_tip_xlm),
            });
        }
        Ok(())
    }

    async fn check_duplicate(&self, pool: &PgPool, tx_hash: &str) -> AppResult<()> {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tips WHERE transaction_hash = $1)")
                .bind(tx_hash)
                .fetch_one(pool)
                .await?;

        if exists {
            return Err(AppError::Conflict {
                code: "DUPLICATE_TRANSACTION",
                message: "This transaction has already been recorded".to_string(),
            });
        }
        Ok(())
    }

    async fn check_creator_exists(&self, pool: &PgPool, username: &str) -> AppResult<()> {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM creators WHERE username = $1)")
                .bind(username)
                .fetch_one(pool)
                .await?;

        if !exists {
            return Err(AppError::CreatorNotFound { username: username.to_string() });
        }
        Ok(())
    }

    /// Logs a warning when the same amount has been tipped to the same creator
    /// more than 5 times in the last hour — a heuristic fraud signal.
    async fn check_fraud_indicators(&self, pool: &PgPool, req: &RecordTipRequest) {
        let result: Result<i64, _> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tips \
             WHERE creator_username = $1 AND amount = $2 \
             AND created_at > NOW() - INTERVAL '1 hour'",
        )
        .bind(&req.username)
        .bind(&req.amount)
        .fetch_one(pool)
        .await;

        if let Ok(count) = result {
            if count > 5 {
                tracing::warn!(
                    creator = %req.username,
                    amount = %req.amount,
                    count = count,
                    "Fraud signal: repeated same-amount tips within 1 hour"
                );
            }
        }
    }
}
