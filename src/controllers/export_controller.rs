use sqlx::PgPool;

use crate::errors::AppResult;
use crate::models::creator::Creator;
use crate::models::tip::Tip;

pub async fn get_all_creators(pool: &PgPool) -> AppResult<Vec<Creator>> {
    let creators = sqlx::query_as::<_, Creator>(
        "SELECT id, username, wallet_address, created_at FROM creators ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(creators)
}

pub async fn get_all_tips(pool: &PgPool) -> AppResult<Vec<Tip>> {
    let tips = sqlx::query_as::<_, Tip>(
        "SELECT id, creator_username, amount, transaction_hash, created_at FROM tips ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(tips)
}

pub async fn get_tips_for_creator(pool: &PgPool, username: &str) -> AppResult<Vec<Tip>> {
    let tips = sqlx::query_as::<_, Tip>(
        "SELECT id, creator_username, amount, transaction_hash, created_at FROM tips WHERE creator_username = $1 ORDER BY created_at ASC",
    )
    .bind(username)
    .fetch_all(pool)
    .await?;
    Ok(tips)
}
