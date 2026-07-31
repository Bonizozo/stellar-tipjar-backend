use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::ip_block::{BlockCountryRequest, BlockIpRequest, CountryBlock, IpBlock};

pub async fn block_ip(pool: &PgPool, req: BlockIpRequest) -> AppResult<IpBlock> {
    let block = sqlx::query_as::<_, IpBlock>(
        r#"
        INSERT INTO ip_blocks (id, ip_address, reason, expires_at)
        VALUES ($1, $2::INET, $3, $4)
        ON CONFLICT (ip_address) DO UPDATE SET
            reason     = EXCLUDED.reason,
            expires_at = EXCLUDED.expires_at,
            blocked_at = NOW()
        RETURNING id, ip_address::TEXT, reason, blocked_at, expires_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(&req.ip_address)
    .bind(&req.reason)
    .bind(req.expires_at)
    .fetch_one(pool)
    .await?;
    Ok(block)
}

pub async fn unblock_ip(pool: &PgPool, ip: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM ip_blocks WHERE ip_address = $1::INET")
        .bind(ip)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_blocked_ips(pool: &PgPool) -> AppResult<Vec<IpBlock>> {
    let blocks = sqlx::query_as::<_, IpBlock>(
        "SELECT id, ip_address::TEXT, reason, blocked_at, expires_at FROM ip_blocks ORDER BY blocked_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(blocks)
}

pub async fn is_ip_blocked(pool: &PgPool, ip: &str) -> AppResult<bool> {
    let blocked = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM ip_blocks
            WHERE ip_address = $1::INET
              AND (expires_at IS NULL OR expires_at > NOW())
        )
        "#,
    )
    .bind(ip)
    .fetch_one(pool)
    .await?;
    Ok(blocked)
}

pub async fn block_country(pool: &PgPool, req: BlockCountryRequest) -> AppResult<CountryBlock> {
    let block = sqlx::query_as::<_, CountryBlock>(
        r#"
        INSERT INTO country_blocks (country_code, reason)
        VALUES ($1, $2)
        ON CONFLICT (country_code) DO UPDATE SET reason = EXCLUDED.reason, blocked_at = NOW()
        RETURNING country_code, reason, blocked_at
        "#,
    )
    .bind(req.country_code.to_uppercase())
    .bind(&req.reason)
    .fetch_one(pool)
    .await?;
    Ok(block)
}

pub async fn unblock_country(pool: &PgPool, code: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM country_blocks WHERE country_code = $1")
        .bind(code.to_uppercase())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_blocked_countries(pool: &PgPool) -> AppResult<Vec<CountryBlock>> {
    let blocks = sqlx::query_as::<_, CountryBlock>(
        "SELECT country_code, reason, blocked_at FROM country_blocks ORDER BY country_code",
    )
    .fetch_all(pool)
    .await?;
    Ok(blocks)
}

pub async fn log_request(
    pool: &PgPool,
    ip: &str,
    country: Option<&str>,
    city: Option<&str>,
    path: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO ip_request_log (id, ip_address, country_code, city, path) VALUES ($1, $2::INET, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(ip)
    .bind(country)
    .bind(city)
    .bind(path)
    .execute(pool)
    .await?;
    Ok(())
}
