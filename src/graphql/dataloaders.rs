use std::{collections::HashMap, sync::Arc};
use async_graphql::dataloader::Loader;
use sqlx::PgPool;
use crate::models::{creator::Creator, tip::Tip};

pub struct CreatorLoader {
    pub pool: PgPool,
}

impl Loader<String> for CreatorLoader {
    type Value = Creator;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[String]) -> Result<HashMap<String, Creator>, Self::Error> {
        let creators = sqlx::query_as::<_, Creator>(
            "SELECT id, username, wallet_address, email, created_at FROM creators WHERE username = ANY($1)",
        )
        .bind(keys.to_vec())
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        Ok(creators.into_iter().map(|c| (c.username.clone(), c)).collect())
    }
}

pub struct TipLoader {
    pub pool: PgPool,
}

impl Loader<String> for TipLoader {
    type Value = Vec<Tip>;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[String]) -> Result<HashMap<String, Vec<Tip>>, Self::Error> {
        let tips = sqlx::query_as::<_, Tip>(
            "SELECT id, creator_username, amount, transaction_hash, created_at FROM tips WHERE creator_username = ANY($1) ORDER BY created_at DESC",
        )
        .bind(keys.to_vec())
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        let mut map: HashMap<String, Vec<Tip>> = HashMap::new();
        for tip in tips {
            map.entry(tip.creator_username.clone()).or_default().push(tip);
        }
        Ok(map)
    }
}
