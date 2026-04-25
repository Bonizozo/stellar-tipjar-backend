use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::errors::AppResult;
use crate::models::creator::Creator;
use crate::models::tip::Tip;

pub async fn get_all_creators(pool: &PgPool) -> AppResult<Vec<Creator>> {
    let creators = sqlx::query_as::<_, Creator>(
        "SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at FROM creators ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(creators)
}

pub async fn get_all_tips(pool: &PgPool) -> AppResult<Vec<Tip>> {
    let tips = sqlx::query_as::<_, Tip>(
        "SELECT id, creator_username, amount, transaction_hash, message, created_at FROM tips ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(tips)
}

pub async fn get_tips_for_creator(pool: &PgPool, username: &str) -> AppResult<Vec<Tip>> {
    let tips = sqlx::query_as::<_, Tip>(
        "SELECT id, creator_username, amount, transaction_hash, message, created_at FROM tips WHERE creator_username = $1 ORDER BY created_at ASC",
    )
    .bind(username)
    .fetch_all(pool)
    .await?;
    Ok(tips)
}

/// Full creator data package: profile + tips + analytics summary.
#[derive(Debug, Serialize, Deserialize)]
pub struct CreatorDataPackage {
    pub exported_at: String,
    pub creator: CreatorExport,
    pub tips: Vec<TipExport>,
    pub analytics: CreatorAnalyticsExport,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatorExport {
    pub id: String,
    pub username: String,
    pub wallet_address: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TipExport {
    pub id: String,
    pub amount: String,
    pub transaction_hash: String,
    pub message: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CreatorAnalyticsExport {
    pub total_tips: i64,
    pub total_amount: String,
    pub avg_amount: String,
    pub max_amount: String,
}

pub async fn get_creator_data_package(
    pool: &PgPool,
    username: &str,
) -> AppResult<CreatorDataPackage> {
    let creator = sqlx::query_as::<_, Creator>(
        "SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at FROM creators WHERE username = $1",
    )
    .bind(username)
    .fetch_one(pool)
    .await?;

    let tips = get_tips_for_creator(pool, username).await?;

    let analytics = sqlx::query_as::<_, CreatorAnalyticsExport>(
        r#"
        SELECT
            COUNT(*)::BIGINT AS total_tips,
            COALESCE(SUM(amount::NUMERIC), 0)::TEXT AS total_amount,
            COALESCE(AVG(amount::NUMERIC), 0)::TEXT AS avg_amount,
            COALESCE(MAX(amount::NUMERIC), 0)::TEXT AS max_amount
        FROM tips
        WHERE creator_username = $1
        "#,
    )
    .bind(username)
    .fetch_one(pool)
    .await?;

    Ok(CreatorDataPackage {
        exported_at: Utc::now().to_rfc3339(),
        creator: CreatorExport {
            id: creator.id.to_string(),
            username: creator.username,
            wallet_address: creator.wallet_address,
            created_at: creator.created_at.to_rfc3339(),
        },
        tips: tips
            .into_iter()
            .map(|t| TipExport {
                id: t.id.to_string(),
                amount: t.amount,
                transaction_hash: t.transaction_hash,
                message: t.message,
                created_at: t.created_at.to_rfc3339(),
            })
            .collect(),
        analytics,
    })
}

/// Record a backup entry in the database.
pub async fn record_backup(
    pool: &PgPool,
    backup_type: &str,
    status: &str,
    size_bytes: Option<i64>,
    location: Option<&str>,
    checksum: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO backup_records (backup_type, status, size_bytes, location, checksum, created_at)
        VALUES ($1, $2, $3, $4, $5, NOW())
        "#,
    )
    .bind(backup_type)
    .bind(status)
    .bind(size_bytes)
    .bind(location)
    .bind(checksum)
    .execute(pool)
    .await?;
    Ok(())
}

/// List recent backup records.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct BackupRecord {
    pub id: uuid::Uuid,
    pub backup_type: String,
    pub status: String,
    pub size_bytes: Option<i64>,
    pub location: Option<String>,
    pub checksum: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
}

pub async fn list_backups(pool: &PgPool, limit: i64) -> AppResult<Vec<BackupRecord>> {
    let records = sqlx::query_as::<_, BackupRecord>(
        "SELECT id, backup_type, status, size_bytes, location, checksum, created_at FROM backup_records ORDER BY created_at DESC LIMIT $1",
    )
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await?;
    Ok(records)
}
