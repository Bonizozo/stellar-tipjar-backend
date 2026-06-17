use rust_decimal::Decimal;
use sqlx::PgPool;
use std::{net::IpAddr, str::FromStr};

use crate::errors::{AppError, AppResult};
use crate::models::tip::RecordTipRequest;

/// Configurable limits for tip validation. Defaults are loaded from environment
/// variables so they can be tuned without recompiling.
pub struct ValidationRules {
    pub min_tip_xlm: Decimal,
    pub max_tip_xlm: Decimal,
    pub tips_per_minute: i64,
}

impl Default for ValidationRules {
    fn default() -> Self {
        let min = decimal_env("MIN_TIP_AMOUNT")
            .or_else(|| decimal_env("TIP_MIN_XLM"))
            .unwrap_or_else(|| Decimal::from_str("0.01").unwrap());
        let max = decimal_env("MAX_TIP_AMOUNT")
            .or_else(|| decimal_env("TIP_MAX_XLM"))
            .unwrap_or_else(|| Decimal::from_str("10000").unwrap());
        let tips_per_minute = std::env::var("TIP_RATE_LIMIT_PER_MINUTE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        Self {
            min_tip_xlm: min,
            max_tip_xlm: max,
            tips_per_minute,
        }
    }
}

fn decimal_env(key: &str) -> Option<Decimal> {
    std::env::var(key)
        .ok()
        .and_then(|v| Decimal::from_str(&v).ok())
}

pub struct TipValidationService {
    rules: ValidationRules,
}

struct CreatorTipLimits {
    min_tip_amount: Option<String>,
    max_tip_amount: Option<String>,
    max_tips_per_minute: Option<i32>,
}

impl TipValidationService {
    pub fn new(rules: ValidationRules) -> Self {
        Self { rules }
    }

    /// Run all business-logic checks before a tip is persisted.
    /// Call this after input validation (ValidatedJson) but before Stellar verification.
    pub async fn validate(&self, pool: &PgPool, req: &RecordTipRequest) -> AppResult<()> {
        self.validate_with_client(pool, req, None).await
    }

    pub async fn validate_with_client(
        &self,
        pool: &PgPool,
        req: &RecordTipRequest,
        ip: Option<IpAddr>,
    ) -> AppResult<()> {
        let creator_limits = self.fetch_creator_limits(pool, &req.username).await?;
        self.check_amount(&req.amount, &creator_limits)?;
        self.check_rate_limits(pool, req, ip, &creator_limits)
            .await?;
        self.check_duplicate(pool, &req.transaction_hash).await?;
        self.check_fraud_indicators(pool, req).await;
        Ok(())
    }

    async fn fetch_creator_limits(
        &self,
        pool: &PgPool,
        username: &str,
    ) -> AppResult<CreatorTipLimits> {
        let limits = sqlx::query_as::<_, (Option<String>, Option<String>, Option<i32>)>(
            "SELECT min_tip_amount, max_tip_amount, max_tips_per_minute FROM creators WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(pool)
        .await?;

        match limits {
            Some((min_tip_amount, max_tip_amount, max_tips_per_minute)) => Ok(CreatorTipLimits {
                min_tip_amount,
                max_tip_amount,
                max_tips_per_minute,
            }),
            None => Err(AppError::CreatorNotFound {
                username: username.to_string(),
            }),
        }
    }

    fn check_amount(&self, amount: &str, creator_limits: &CreatorTipLimits) -> AppResult<()> {
        let value = Decimal::from_str(amount).map_err(|_| AppError::Conflict {
            code: "INVALID_AMOUNT",
            message: "Tip amount is not a valid decimal".to_string(),
        })?;

        let min = creator_limits
            .min_tip_amount
            .as_deref()
            .and_then(|v| Decimal::from_str(v).ok())
            .unwrap_or(self.rules.min_tip_xlm);
        let max = creator_limits
            .max_tip_amount
            .as_deref()
            .and_then(|v| Decimal::from_str(v).ok())
            .unwrap_or(self.rules.max_tip_xlm);

        if value < min {
            return Err(AppError::Conflict {
                code: "AMOUNT_TOO_LOW",
                message: format!("Minimum tip amount is {min} XLM"),
            });
        }
        if value > max {
            return Err(AppError::Conflict {
                code: "AMOUNT_TOO_HIGH",
                message: format!("Maximum tip amount is {max} XLM"),
            });
        }
        Ok(())
    }

    async fn check_rate_limits(
        &self,
        pool: &PgPool,
        req: &RecordTipRequest,
        ip: Option<IpAddr>,
        creator_limits: &CreatorTipLimits,
    ) -> AppResult<()> {
        let limit = creator_limits
            .max_tips_per_minute
            .map(i64::from)
            .unwrap_or(self.rules.tips_per_minute);
        let retry_after_secs = 60;

        if let Some(ip) = ip {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM tips WHERE tipper_ip = $1 AND created_at > NOW() - INTERVAL '1 minute'",
            )
            .bind(ip.to_string())
            .fetch_one(pool)
            .await?;
            if count >= limit {
                log_rate_limit_violation(req, Some(ip), "ip", count, limit);
                return Err(AppError::rate_limited_with_retry(
                    "Too many tips from this IP address. Please slow down.",
                    retry_after_secs,
                ));
            }
        }

        if let Some(wallet) = req.tipper_wallet.as_deref() {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM tips WHERE tipper_wallet = $1 AND created_at > NOW() - INTERVAL '1 minute'",
            )
            .bind(wallet)
            .fetch_one(pool)
            .await?;
            if count >= limit {
                log_rate_limit_violation(req, ip, "wallet", count, limit);
                return Err(AppError::rate_limited_with_retry(
                    "Too many tips from this wallet. Please slow down.",
                    retry_after_secs,
                ));
            }
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
                    wallet = ?req.tipper_wallet,
                    amount = %req.amount,
                    count = count,
                    "Fraud signal: repeated same-amount tips within 1 hour"
                );
            }
        }
    }
}

fn log_rate_limit_violation(
    req: &RecordTipRequest,
    ip: Option<IpAddr>,
    limit_type: &'static str,
    count: i64,
    limit: i64,
) {
    tracing::warn!(
        creator = %req.username,
        wallet = ?req.tipper_wallet,
        ip = ?ip,
        limit_type,
        count,
        limit,
        "Tip rate limit exceeded"
    );
}
