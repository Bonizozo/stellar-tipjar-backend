use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

const DEFAULT_PAGE: i64 = 1;
const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

fn default_page() -> i64 {
    DEFAULT_PAGE
}
fn default_limit() -> i64 {
    DEFAULT_LIMIT
}

/// Query parameters for offset-based pagination.
#[derive(Debug, Deserialize, IntoParams)]
pub struct PaginationParams {
    /// Page number, starting at 1 (default: 1)
    #[serde(default = "default_page")]
    pub page: i64,
    /// Items per page, max 100 (default: 20)
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Cursor for cursor-based pagination. This is a base64 encoded string
    /// representing a composite cursor based on `created_at` timestamp and `id`.
    /// Format (base64 of): "<timestamp>|<uuid>" (unix seconds for timestamp).
    #[serde(default)]
    pub cursor: Option<String>,
    /// When using cursor pagination, set to true to fetch the previous page.
    #[serde(default)]
    pub previous: Option<bool>,
}

impl PaginationParams {
    /// Clamp limit to [1, MAX_LIMIT] and page to >= 1.
    pub fn validated(mut self) -> Self {
        self.page = self.page.max(1);
        self.limit = self.limit.clamp(1, MAX_LIMIT);
        self
    }

    pub fn offset(&self) -> i64 {
        (self.page - 1) * self.limit
    }
}

/// Paginated response envelope.
#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    /// Total number of matching records.
    pub total: i64,
    /// Current page number.
    pub page: i64,
    /// Items per page.
    pub limit: i64,
    /// Total number of pages.
    pub total_pages: i64,
    /// Whether a next page exists.
    pub has_next: bool,
    /// Whether a previous page exists.
    pub has_prev: bool,
    /// The active cursor value when using cursor-based pagination.
    pub cursor: Option<String>,
}

impl<T: Serialize> PaginatedResponse<T> {
    pub fn new(data: Vec<T>, total: i64, params: &PaginationParams) -> Self {
        let total_pages = ((total as f64) / (params.limit as f64)).ceil() as i64;
        Self {
            has_next: params.page < total_pages,
            has_prev: params.page > 1,
            data,
            total,
            page: params.page,
            limit: params.limit,
            total_pages,
            cursor: params.cursor.clone(),
        }
    }

    pub fn map<U: Serialize, F: Fn(T) -> U>(self, f: F) -> PaginatedResponse<U> {
        PaginatedResponse {
            data: self.data.into_iter().map(f).collect(),
            total: self.total,
            page: self.page,
            limit: self.limit,
            total_pages: self.total_pages,
            has_next: self.has_next,
            has_prev: self.has_prev,
            cursor: self.cursor,
        }
    }
}
