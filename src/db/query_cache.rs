use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;

pub struct CachedQuery {
    pub sql: String,
    pub hits: u64,
    pub last_used: Instant,
}

/// In-memory prepared-statement cache keyed by a logical query name.
pub struct QueryCache {
    inner: RwLock<HashMap<String, CachedQuery>>,
}

impl QueryCache {
    pub fn new() -> Self {
        Self { inner: RwLock::new(HashMap::new()) }
    }

    /// Register or touch a query. Returns the stored SQL.
    pub fn get_or_insert(&self, name: &str, sql: &str) -> String {
        // Fast path: already cached
        {
            let mut map = self.inner.write().unwrap();
            let entry = map.entry(name.to_string()).or_insert_with(|| CachedQuery {
                sql: sql.to_string(),
                hits: 0,
                last_used: Instant::now(),
            });
            entry.hits += 1;
            entry.last_used = Instant::now();
            entry.sql.clone()
        }
    }

    pub fn hit_count(&self, name: &str) -> u64 {
        self.inner.read().unwrap().get(name).map_or(0, |e| e.hits)
    }
}

impl Default for QueryCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_hit_counting() {
        let cache = QueryCache::new();
        let sql = "SELECT * FROM creators WHERE username = $1";
        cache.get_or_insert("get_creator", sql);
        cache.get_or_insert("get_creator", sql);
        assert_eq!(cache.hit_count("get_creator"), 2);
    }

    #[test]
    fn test_cache_returns_original_sql() {
        let cache = QueryCache::new();
        let sql = "SELECT id FROM tips WHERE creator_id = $1";
        let result = cache.get_or_insert("list_tips", sql);
        assert_eq!(result, sql);
    }
}
