use sqlx::PgPool;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilters {
    pub date_from: Option<chrono::NaiveDateTime>,
    pub date_to: Option<chrono::NaiveDateTime>,
    pub min_amount: Option<rust_decimal::Decimal>,
    pub max_amount: Option<rust_decimal::Decimal>,
    pub creator_id: Option<Uuid>,
    pub verified_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatorSearchResult {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub rank: f32,
    pub tip_count: i64,
    pub total_received: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct TipSearchResult {
    pub id: Uuid,
    pub creator_username: String,
    pub amount: rust_decimal::Decimal,
    pub message: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub rank: f32,
}

pub struct SearchEngine {
    pool: PgPool,
}

impl SearchEngine {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn search_creators(
        &self,
        query: &str,
        filters: &SearchFilters,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CreatorSearchResult>, sqlx::Error> {
        let search_query = Self::prepare_search_query(query);
        
        let results = sqlx::query_as!(
            CreatorSearchResult,
            r#"
            SELECT 
                c.id,
                c.username,
                c.display_name,
                c.bio,
                ts_rank(c.search_vector, to_tsquery('english', $1)) as "rank!",
                COUNT(DISTINCT t.id) as "tip_count!",
                COALESCE(SUM(t.amount::decimal), 0) as "total_received!"
            FROM creators c
            LEFT JOIN tips t ON c.id = t.creator_id
            WHERE 
                c.search_vector @@ to_tsquery('english', $1)
                AND ($2::uuid IS NULL OR c.id = $2)
                AND ($3::boolean IS NULL OR c.verified = $3)
            GROUP BY c.id, c.username, c.display_name, c.bio, c.search_vector
            ORDER BY "rank!" DESC, "tip_count!" DESC
            LIMIT $4 OFFSET $5
            "#,
            search_query,
            filters.creator_id,
            filters.verified_only,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(results)
    }

    pub async fn search_creators_fuzzy(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<CreatorSearchResult>, sqlx::Error> {
        let results = sqlx::query_as!(
            CreatorSearchResult,
            r#"
            SELECT 
                c.id,
                c.username,
                c.display_name,
                c.bio,
                similarity(c.username, $1) as "rank!",
                COUNT(DISTINCT t.id) as "tip_count!",
                COALESCE(SUM(t.amount::decimal), 0) as "total_received!"
            FROM creators c
            LEFT JOIN tips t ON c.id = t.creator_id
            WHERE 
                c.username % $1
                OR c.display_name % $1
            GROUP BY c.id, c.username, c.display_name, c.bio
            ORDER BY "rank!" DESC
            LIMIT $2
            "#,
            query,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(results)
    }

    pub async fn search_tips(
        &self,
        query: &str,
        filters: &SearchFilters,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TipSearchResult>, sqlx::Error> {
        let search_query = Self::prepare_search_query(query);
        
        let results = sqlx::query_as!(
            TipSearchResult,
            r#"
            SELECT
                t.id,
                c.username as creator_username,
                t.amount::decimal as "amount!",
                t.message,
                t.created_at,
                ts_rank(t.search_vector, to_tsquery('english', $1)) as "rank!"
            FROM tips t
            JOIN creators c ON t.creator_id = c.id
            WHERE
                t.search_vector @@ to_tsquery('english', $1)
                AND ($2::uuid IS NULL OR t.creator_id = $2)
                AND ($3::timestamp IS NULL OR t.created_at >= $3)
                AND ($4::timestamp IS NULL OR t.created_at <= $4)
                AND ($5::decimal IS NULL OR t.amount::decimal >= $5)
                AND ($6::decimal IS NULL OR t.amount::decimal <= $6)
            ORDER BY "rank!" DESC, t.created_at DESC
            LIMIT $7 OFFSET $8
            "#,
            search_query,
            filters.creator_id,
            filters.date_from,
            filters.date_to,
            filters.min_amount,
            filters.max_amount,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(results)
    }

    fn prepare_search_query(query: &str) -> String {
        query
            .split_whitespace()
            .map(|word| format!("{}:*", word))
            .collect::<Vec<_>>()
            .join(" & ")
    }

    pub async fn get_search_suggestions(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<String>, sqlx::Error> {
        let results = sqlx::query_scalar!(
            r#"
            SELECT username
            FROM creators
            WHERE username ILIKE $1
            ORDER BY username
            LIMIT $2
            "#,
            format!("{}%", query),
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(results)
    }
}
