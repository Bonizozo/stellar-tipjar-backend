use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct SearchCache {
    redis: redis::aio::ConnectionManager,
}

impl SearchCache {
    pub fn new(redis: redis::aio::ConnectionManager) -> Self {
        Self { redis }
    }

    pub async fn get<T: for<'de> Deserialize<'de>>(
        &mut self,
        key: &str,
    ) -> Result<Option<T>, redis::RedisError> {
        let data: Option<String> = self.redis.get(key).await?;
        
        match data {
            Some(json) => {
                let result = serde_json::from_str(&json).ok();
                Ok(result)
            }
            None => Ok(None),
        }
    }

    pub async fn set<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        ttl: Duration,
    ) -> Result<(), redis::RedisError> {
        let json = serde_json::to_string(value).map_err(|e| {
            redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "Serialization error",
                e.to_string(),
            ))
        })?;
        
        self.redis.set_ex::<_, _, ()>(key, json, ttl.as_secs() as u64).await?;
        Ok(())
    }

    pub fn make_key(prefix: &str, query: &str, filters: &str) -> String {
        format!("search:{}:{}:{}", prefix, query, filters)
    }

    pub async fn invalidate_pattern(&mut self, pattern: &str) -> Result<(), redis::RedisError> {
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(pattern)
            .query_async(&mut self.redis)
            .await?;
        
        if !keys.is_empty() {
            self.redis.del::<_, ()>(keys).await?;
        }
        
        Ok(())
    }
}
