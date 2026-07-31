use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::category::{
    AssignCategoriesRequest, AssignTagsRequest, Category, CreateCategoryRequest, Tag,
};

pub async fn list_categories(pool: &PgPool) -> AppResult<Vec<Category>> {
    let cats = sqlx::query_as::<_, Category>(
        "SELECT id, name, slug, description, created_at FROM categories ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    Ok(cats)
}

pub async fn create_category(pool: &PgPool, req: CreateCategoryRequest) -> AppResult<Category> {
    let cat = sqlx::query_as::<_, Category>(
        r#"
        INSERT INTO categories (id, name, slug, description)
        VALUES ($1, $2, $3, $4)
        RETURNING id, name, slug, description, created_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(&req.name)
    .bind(&req.slug)
    .bind(&req.description)
    .fetch_one(pool)
    .await?;
    Ok(cat)
}

pub async fn get_creators_by_category(pool: &PgPool, slug: &str) -> AppResult<Vec<String>> {
    let rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT cc.creator_username
        FROM creator_categories cc
        JOIN categories c ON c.id = cc.category_id
        WHERE c.slug = $1
        ORDER BY cc.creator_username
        "#,
    )
    .bind(slug)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn assign_categories(
    pool: &PgPool,
    username: &str,
    req: AssignCategoriesRequest,
) -> AppResult<()> {
    sqlx::query("DELETE FROM creator_categories WHERE creator_username = $1")
        .bind(username)
        .execute(pool)
        .await?;

    for cat_id in &req.category_ids {
        sqlx::query(
            "INSERT INTO creator_categories (creator_username, category_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(username)
        .bind(cat_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn get_creator_categories(pool: &PgPool, username: &str) -> AppResult<Vec<Category>> {
    let cats = sqlx::query_as::<_, Category>(
        r#"
        SELECT c.id, c.name, c.slug, c.description, c.created_at
        FROM categories c
        JOIN creator_categories cc ON cc.category_id = c.id
        WHERE cc.creator_username = $1
        ORDER BY c.name
        "#,
    )
    .bind(username)
    .fetch_all(pool)
    .await?;
    Ok(cats)
}

pub async fn assign_tags(
    pool: &PgPool,
    username: &str,
    req: AssignTagsRequest,
) -> AppResult<Vec<Tag>> {
    sqlx::query("DELETE FROM creator_tags WHERE creator_username = $1")
        .bind(username)
        .execute(pool)
        .await?;

    for tag_name in &req.tags {
        let tag_name = tag_name.trim().to_lowercase();
        sqlx::query(
            r#"
            WITH ins AS (
                INSERT INTO tags (id, name) VALUES ($1, $2)
                ON CONFLICT (name) DO NOTHING
                RETURNING id
            )
            INSERT INTO creator_tags (creator_username, tag_id)
            SELECT $3, id FROM ins
            UNION ALL
            SELECT $3, id FROM tags WHERE name = $2
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(&tag_name)
        .bind(username)
        .execute(pool)
        .await?;
    }

    get_creator_tags(pool, username).await
}

pub async fn get_creator_tags(pool: &PgPool, username: &str) -> AppResult<Vec<Tag>> {
    let tags = sqlx::query_as::<_, Tag>(
        r#"
        SELECT t.id, t.name
        FROM tags t
        JOIN creator_tags ct ON ct.tag_id = t.id
        WHERE ct.creator_username = $1
        ORDER BY t.name
        "#,
    )
    .bind(username)
    .fetch_all(pool)
    .await?;
    Ok(tags)
}

pub async fn search_by_tag(pool: &PgPool, tag: &str) -> AppResult<Vec<String>> {
    let rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT ct.creator_username
        FROM creator_tags ct
        JOIN tags t ON t.id = ct.tag_id
        WHERE t.name = $1
        ORDER BY ct.creator_username
        "#,
    )
    .bind(tag.trim().to_lowercase())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
