use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::portfolio::{
    CreatePortfolioItemRequest, MediaType, PortfolioItem, UpdatePortfolioItemRequest,
};
use sqlx::PgPool;

pub async fn list_items(pool: &PgPool, creator_username: &str) -> AppResult<Vec<PortfolioItem>> {
    let items = sqlx::query_as::<_, PortfolioItem>(
        r#"SELECT id, creator_username, title, description, media_type, url, thumbnail_url,
                  display_order, is_featured, created_at, updated_at
           FROM portfolio_items
           WHERE creator_username = $1
           ORDER BY display_order ASC, created_at ASC"#,
    )
    .bind(creator_username)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

pub async fn get_item(pool: &PgPool, id: Uuid, creator_username: &str) -> AppResult<PortfolioItem> {
    sqlx::query_as::<_, PortfolioItem>(
        r#"SELECT id, creator_username, title, description, media_type, url, thumbnail_url,
                  display_order, is_featured, created_at, updated_at
           FROM portfolio_items WHERE id = $1 AND creator_username = $2"#,
    )
    .bind(id)
    .bind(creator_username)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Database(crate::errors::DatabaseError::NotFound {
        entity: "portfolio_item",
        identifier: id.to_string(),
    }))
}

pub async fn create_item(
    pool: &PgPool,
    creator_username: &str,
    req: CreatePortfolioItemRequest,
) -> AppResult<PortfolioItem> {
    let media_type = req.media_type.unwrap_or(MediaType::Link);
    let display_order = req.display_order.unwrap_or(0);
    let is_featured = req.is_featured.unwrap_or(false);

    let item = sqlx::query_as::<_, PortfolioItem>(
        r#"INSERT INTO portfolio_items
               (id, creator_username, title, description, media_type, url, thumbnail_url, display_order, is_featured, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())
           RETURNING id, creator_username, title, description, media_type, url, thumbnail_url, display_order, is_featured, created_at, updated_at"#,
    )
    .bind(Uuid::new_v4())
    .bind(creator_username)
    .bind(&req.title)
    .bind(&req.description)
    .bind(media_type)
    .bind(&req.url)
    .bind(&req.thumbnail_url)
    .bind(display_order)
    .bind(is_featured)
    .fetch_one(pool)
    .await?;

    Ok(item)
}

pub async fn update_item(
    pool: &PgPool,
    id: Uuid,
    creator_username: &str,
    req: UpdatePortfolioItemRequest,
) -> AppResult<PortfolioItem> {
    let existing = get_item(pool, id, creator_username).await?;

    let item = sqlx::query_as::<_, PortfolioItem>(
        r#"UPDATE portfolio_items
           SET title = $1, description = $2, media_type = $3, url = $4,
               thumbnail_url = $5, display_order = $6, is_featured = $7, updated_at = NOW()
           WHERE id = $8 AND creator_username = $9
           RETURNING id, creator_username, title, description, media_type, url, thumbnail_url, display_order, is_featured, created_at, updated_at"#,
    )
    .bind(req.title.unwrap_or(existing.title))
    .bind(req.description.or(existing.description))
    .bind(req.media_type.unwrap_or(existing.media_type))
    .bind(req.url.unwrap_or(existing.url))
    .bind(req.thumbnail_url.or(existing.thumbnail_url))
    .bind(req.display_order.unwrap_or(existing.display_order))
    .bind(req.is_featured.unwrap_or(existing.is_featured))
    .bind(id)
    .bind(creator_username)
    .fetch_one(pool)
    .await?;

    Ok(item)
}

pub async fn delete_item(pool: &PgPool, id: Uuid, creator_username: &str) -> AppResult<()> {
    let result = sqlx::query(
        "DELETE FROM portfolio_items WHERE id = $1 AND creator_username = $2",
    )
    .bind(id)
    .bind(creator_username)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::Database(crate::errors::DatabaseError::NotFound {
            entity: "portfolio_item",
            identifier: id.to_string(),
        }));
    }
    Ok(())
}

/// Reorder items by assigning display_order based on the provided ID list.
pub async fn reorder_items(
    pool: &PgPool,
    creator_username: &str,
    ids: Vec<Uuid>,
) -> AppResult<Vec<PortfolioItem>> {
    let mut tx = pool.begin().await?;
    for (order, id) in ids.iter().enumerate() {
        sqlx::query(
            "UPDATE portfolio_items SET display_order = $1, updated_at = NOW() WHERE id = $2 AND creator_username = $3",
        )
        .bind(order as i32)
        .bind(id)
        .bind(creator_username)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    list_items(pool, creator_username).await
}
