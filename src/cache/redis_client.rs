pub use redis::aio::ConnectionManager;
use redis::{AsyncCommands, RedisError};

pub const TTL_CREATOR: u64 = 300; // 5 minutes
pub const TTL_TIPS: u64 = 60;     // 1 minute

/// Attempt to get a cached JSON value. Returns None on miss or any Redis error.
pub async fn get<T>(conn: &mut ConnectionManager, key: &str) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    match conn.get::<_, String>(key).await {
        Ok(raw) => serde_json::from_str(&raw).ok(),
        Err(e) => {
            tracing::warn!("Redis GET failed for key '{}': {}", key, e);
            None
        }
    }
}

/// Attempt to set a JSON value with a TTL. Logs and swallows errors so Redis
/// failures never break the request path.
pub async fn set<T>(conn: &mut ConnectionManager, key: &str, value: &T, ttl_secs: u64)
where
    T: serde::Serialize,
{
    match serde_json::to_string(value) {
        Ok(raw) => {
            if let Err(e) = conn.set_ex::<_, _, ()>(key, raw, ttl_secs).await {
                tracing::warn!("Redis SET failed for key '{}': {}", key, e);
            }
        }
        Err(e) => tracing::warn!("Cache serialization failed for key '{}': {}", key, e),
    }
}

/// Delete one or more keys (cache invalidation). Swallows errors.
pub async fn del(conn: &mut ConnectionManager, keys: &[&str]) {
    for key in keys {
        if let Err(e) = conn.del::<_, ()>(*key).await {
            tracing::warn!("Redis DEL failed for key '{}': {}", key, e);
        }
    }
}

/// Build a Redis ConnectionManager from a URL. Returns None if Redis is unavailable
/// so the app can start without Redis (graceful degradation).
pub async fn connect(url: &str) -> Option<ConnectionManager> {
    match redis::Client::open(url) {
        Ok(client) => match ConnectionManager::new(client).await {
            Ok(mgr) => {
                tracing::info!("Redis connected at {}", url);
                Some(mgr)
            }
            Err(e) => {
                tracing::warn!("Redis connection failed (caching disabled): {}", e);
                None
            }
        },
        Err(e) => {
            tracing::warn!("Invalid Redis URL (caching disabled): {}", e);
            None
        }
    }
}

pub type RedisError_ = RedisError;
