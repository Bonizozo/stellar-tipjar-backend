use crate::models::pagination::PaginationParams;
use crate::models::tip::{TipFilters, TipSortParams};

/// Cache key for a creator profile. TTL: 5 minutes.
pub fn creator(username: &str) -> String {
    format!("creator:{}", username)
}

/// Base pattern for a creator's tip list cache entries.
pub fn creator_tips_pattern(username: &str) -> String {
    format!("creator:{}:tips:*", username)
}

/// Cache key for a creator's tip list. TTL: 1 minute.
pub fn creator_tips(
    username: &str,
    params: &PaginationParams,
    filters: &TipFilters,
    sort: &TipSortParams,
) -> String {
    let filter_key = serde_json::to_string(filters).unwrap_or_default();
    let sort_key = serde_json::to_string(sort).unwrap_or_default();
    format!(
        "creator:{}:tips:{}:{}:{}:{}",
        username,
        params.page,
        params.per_page,
        filter_key,
        sort_key
    )
}

/// Cache key for leaderboard snapshots.
pub fn leaderboard(board_type: &str, period: &str, limit: i64) -> String {
    format!("leaderboard:{}:{}:{}", board_type, period, limit)
}

/// HTTP response cache key from request components.
pub fn http_response(method: &str, path: &str, query: &str) -> String {
    let query_digest = if query.is_empty() {
        String::from("none")
    } else {
        format!(
            "{:x}",
            sha2::Sha256::digest(query.as_bytes())
        )
    };
    format!("http:{}:{}:{}", method, path, query_digest)
}

/// Pattern to invalidate all HTTP response entries for a path prefix.
pub fn http_response_pattern(path_prefix: &str) -> String {
    format!("http:*:{}:*", path_prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::pagination::PaginationParams;
    use crate::models::tip::{TipFilters, TipSortParams};

    #[test]
    fn creator_tips_key_is_deterministic() {
        let params = PaginationParams { page: 1, per_page: 20 };
        let filters = TipFilters {
            min_amount: None,
            max_amount: None,
            from_date: None,
            to_date: None,
        };
        let sort = TipSortParams {
            sort_by: "created_at".to_string(),
            sort_order: "desc".to_string(),
        };

        let key = creator_tips("alice", &params, &filters, &sort);
        assert!(key.starts_with("creator:alice:tips:"));
        assert!(key.contains("created_at"));
    }
}
