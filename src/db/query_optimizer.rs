use std::sync::Arc;
use crate::db::query_cache::QueryCache;

/// Applies simple, safe SQL rewrites before a query is executed.
pub struct QueryOptimizer {
    cache: Arc<QueryCache>,
}

impl QueryOptimizer {
    pub fn new(cache: Arc<QueryCache>) -> Self {
        Self { cache }
    }

    /// Rewrite a query and return the (possibly modified) SQL.
    pub fn optimize(&self, name: &str, sql: &str) -> String {
        let rewritten = apply_rewrites(sql);
        self.cache.get_or_insert(name, &rewritten)
    }
}

/// Applies a small set of deterministic, safe SQL rewrites.
fn apply_rewrites(sql: &str) -> String {
    REWRITES.iter().fold(sql.to_string(), |acc, (from, to)| acc.replace(from, to))
}

/// (pattern, replacement) pairs applied in order.
static REWRITES: &[(&str, &str)] = &[
    // Normalise redundant double-spaces
    ("  ", " "),
    // Replace SELECT * with a reminder comment (non-destructive marker)
    // Real projects would expand columns; here we just annotate.
    ("SELECT *", "SELECT * /* consider explicit columns */"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimize_caches_query() {
        let cache = Arc::new(QueryCache::new());
        let optimizer = QueryOptimizer::new(Arc::clone(&cache));
        optimizer.optimize("list_creators", "SELECT * FROM creators");
        assert_eq!(cache.hit_count("list_creators"), 1);
    }

    #[test]
    fn test_rewrite_select_star() {
        let result = apply_rewrites("SELECT * FROM tips");
        assert!(result.contains("/* consider explicit columns */"));
    }

    #[test]
    fn test_rewrite_idempotent_for_clean_query() {
        let sql = "SELECT id, username FROM creators WHERE id = $1";
        assert_eq!(apply_rewrites(sql), sql);
    }
}
