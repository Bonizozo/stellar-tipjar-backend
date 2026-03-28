use async_graphql::{Context, Object, Result, SimpleObject};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::context::GraphQLContext;

#[derive(SimpleObject, Clone)]
pub struct GqlCreator {
    pub id: Uuid,
    pub username: String,
    pub wallet_address: String,
    pub email: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<crate::models::creator::Creator> for GqlCreator {
    fn from(c: crate::models::creator::Creator) -> Self {
        Self {
            id: c.id,
            username: c.username,
            wallet_address: c.wallet_address,
            email: c.email,
            created_at: c.created_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlTip {
    pub id: Uuid,
    pub creator_username: String,
    pub amount: String,
    pub transaction_hash: String,
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<crate::models::tip::Tip> for GqlTip {
    fn from(t: crate::models::tip::Tip) -> Self {
        Self {
            id: t.id,
            creator_username: t.creator_username,
            amount: t.amount,
            transaction_hash: t.transaction_hash,
            message: t.message,
            created_at: t.created_at,
        }
    }
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// Look up a single creator by username. Uses DataLoader to batch DB calls.
    async fn creator(&self, ctx: &Context<'_>, username: String) -> Result<Option<GqlCreator>> {
        let gql_ctx = ctx.data::<GraphQLContext>()?;
        let result = gql_ctx.creator_loader.load_one(username).await?;
        Ok(result.map(GqlCreator::from))
    }

    /// List all tips for a creator. Uses DataLoader to batch DB calls.
    async fn tips_for_creator(&self, ctx: &Context<'_>, username: String) -> Result<Vec<GqlTip>> {
        let gql_ctx = ctx.data::<GraphQLContext>()?;
        let tips = gql_ctx
            .tip_loader
            .load_one(username)
            .await?
            .unwrap_or_default();
        Ok(tips.into_iter().map(GqlTip::from).collect())
    }

    /// List creators with optional pagination.
    async fn creators(
        &self,
        ctx: &Context<'_>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<GqlCreator>> {
        let gql_ctx = ctx.data::<GraphQLContext>()?;
        let limit = limit.unwrap_or(20).min(100);
        let offset = offset.unwrap_or(0);

        let creators = sqlx::query_as::<_, crate::models::creator::Creator>(
            "SELECT id, username, wallet_address, email, created_at FROM creators ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&gql_ctx.state.db)
        .await?;

        Ok(creators.into_iter().map(GqlCreator::from).collect())
    }
}
