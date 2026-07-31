pub mod cache;
pub mod engine;

use serde::Deserialize;
use utoipa::IntoParams;

pub use cache::SearchCache;
pub use engine::{CreatorSearchResult, SearchEngine, SearchFilters, TipSearchResult};

/// Query parameters for the creator search endpoint.
#[derive(Debug, Deserialize, IntoParams)]
pub struct SearchQuery {
    /// Search term matched against username via full-text + trigram fuzzy search.
    pub q: String,
    /// Maximum results to return (default 20, max 100).
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    20
}

impl SearchQuery {
    pub fn clamped_limit(&self) -> i64 {
        self.limit.clamp(1, 100)
    }
}
