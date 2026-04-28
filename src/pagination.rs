pub mod helpers;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::errors::AppError;

/// Pagination configuration
#[derive(Debug, Clone)]
pub struct PaginationConfig {
    /// Default page size
    pub default_page_size: usize,
    /// Maximum page size allowed
    pub max_page_size: usize,
    /// Default pagination type
    pub default_pagination_type: PaginationType,
    /// Whether to include total count in responses
    pub include_total_count: bool,
}

impl Default for PaginationConfig {
    fn default() -> Self {
        Self {
            default_page_size: 20,
            max_page_size: 100,
            default_pagination_type: PaginationType::Offset,
            include_total_count: true,
        }
    }
}

/// Pagination types supported
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaginationType {
    /// Traditional offset-based pagination (page/limit)
    Offset,
    /// Cursor-based pagination (cursor/limit)
    Cursor,
}

/// Pagination query parameters for offset-based pagination
#[derive(Debug, Deserialize)]
pub struct OffsetPaginationQuery {
    /// Page number (1-based)
    pub page: Option<usize>,
    /// Number of items per page
    pub limit: Option<usize>,
    /// Sort order
    pub sort: Option<String>,
    /// Sort direction
    pub order: Option<SortDirection>,
}

/// Pagination query parameters for cursor-based pagination
#[derive(Debug, Deserialize)]
pub struct CursorPaginationQuery {
    /// Cursor for the next page
    pub cursor: Option<String>,
    /// Number of items per page
    pub limit: Option<usize>,
    /// Sort order
    pub sort: Option<String>,
    /// Sort direction
    pub order: Option<SortDirection>,
    /// Whether to fetch previous page
    pub previous: Option<bool>,
}

/// Combined pagination query parameters
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    /// Pagination type (offset or cursor)
    #[serde(rename = "type")]
    pub pagination_type: Option<PaginationType>,
    /// Offset pagination parameters
    #[serde(flatten)]
    pub offset_params: OffsetPaginationQuery,
    /// Cursor pagination parameters
    #[serde(flatten)]
    pub cursor_params: CursorPaginationQuery,
}

/// Sort direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    Asc,
    Desc,
}

impl Default for SortDirection {
    fn default() -> Self {
        Self::Asc
    }
}

/// Pagination metadata for offset-based pagination
#[derive(Debug, Serialize, Deserialize)]
pub struct OffsetPaginationMeta {
    /// Current page number (1-based)
    pub page: usize,
    /// Number of items per page
    pub limit: usize,
    /// Total number of items
    pub total_items: Option<usize>,
    /// Total number of pages
    pub total_pages: Option<usize>,
    /// Whether there's a next page
    pub has_next: bool,
    /// Whether there's a previous page
    pub has_previous: bool,
    /// Next page number
    pub next_page: Option<usize>,
    /// Previous page number
    pub previous_page: Option<usize>,
}

/// Pagination metadata for cursor-based pagination
#[derive(Debug, Serialize, Deserialize)]
pub struct CursorPaginationMeta {
    /// Current cursor
    pub cursor: Option<String>,
    /// Number of items per page
    pub limit: usize,
    /// Whether there's a next page
    pub has_next: bool,
    /// Whether there's a previous page
    pub has_previous: bool,
    /// Next cursor
    pub next_cursor: Option<String>,
    /// Previous cursor
    pub previous_cursor: Option<String>,
}

/// Generic pagination metadata
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PaginationMeta {
    Offset(OffsetPaginationMeta),
    Cursor(CursorPaginationMeta),
}

/// Pagination links for navigation
#[derive(Debug, Serialize, Deserialize)]
pub struct PaginationLinks {
    /// Link to self
    pub self_link: Option<String>,
    /// Link to first page
    pub first: Option<String>,
    /// Link to previous page
    pub prev: Option<String>,
    /// Link to next page
    pub next: Option<String>,
    /// Link to last page
    pub last: Option<String>,
}

/// Standard paginated response
#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    /// Data items
    pub data: Vec<T>,
    /// Pagination metadata
    pub pagination: PaginationMeta,
    /// Navigation links
    pub links: PaginationLinks,
}

/// Cursor information for cursor-based pagination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cursor {
    /// The actual cursor value
    pub value: String,
    /// Timestamp for ordering
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Unique identifier
    pub id: Uuid,
}

impl Cursor {
    pub fn new(value: String, timestamp: chrono::DateTime<chrono::Utc>, id: Uuid) -> Self {
        Self { value, timestamp, id }
    }

    /// Encode cursor to base64 string
    pub fn encode(&self) -> String {
        let cursor_data = format!("{}|{}|{}", self.value, self.timestamp.timestamp(), self.id);
        base64::encode(cursor_data)
    }

    /// Decode cursor from base64 string
    pub fn decode(encoded: &str) -> Result<Self, AppError> {
        let decoded = base64::decode(encoded)
            .map_err(|_| AppError::bad_request("Invalid cursor format"))?;
        
        let cursor_str = String::from_utf8(decoded)
            .map_err(|_| AppError::bad_request("Invalid cursor encoding"))?;
        
        let parts: Vec<&str> = cursor_str.split('|').collect();
        if parts.len() != 3 {
            return Err(AppError::bad_request("Invalid cursor structure"));
        }

        let value = parts[0].to_string();
        let timestamp = chrono::DateTime::from_timestamp(
            parts[1].parse::<i64>()
                .map_err(|_| AppError::bad_request("Invalid cursor timestamp"))?,
            0,
        ).ok_or_else(|| AppError::bad_request("Invalid cursor timestamp"))?;
        
        let id = Uuid::parse_str(parts[2])
            .map_err(|_| AppError::bad_request("Invalid cursor ID"))?;

        Ok(Cursor { value, timestamp, id })
    }
}

/// Pagination state
#[derive(Clone)]
pub struct PaginationState {
    pub config: PaginationConfig,
}

impl PaginationState {
    pub fn new(config: PaginationConfig) -> Self {
        Self { config }
    }
}

/// Process pagination query and return standardized pagination info
pub fn process_pagination_query(
    query: PaginationQuery,
    config: &PaginationConfig,
) -> Result<PaginationInfo, AppError> {
    let pagination_type = query.pagination_type.unwrap_or(config.default_pagination_type);

    match pagination_type {
        PaginationType::Offset => {
            let page = query.offset_params.page.unwrap_or(1);
            let limit = std::cmp::min(
                query.offset_params.limit.unwrap_or(config.default_page_size),
                config.max_page_size,
            );

            if page == 0 {
                return Err(AppError::bad_request("Page number must be greater than 0"));
            }

            Ok(PaginationInfo::Offset(OffsetPaginationInfo {
                page,
                limit,
                sort: query.offset_params.sort.unwrap_or_else(|| "created_at".to_string()),
                order: query.offset_params.order.unwrap_or(SortDirection::Desc),
            }))
        }
        PaginationType::Cursor => {
            let limit = std::cmp::min(
                query.cursor_params.limit.unwrap_or(config.default_page_size),
                config.max_page_size,
            );
            let cursor = query.cursor_params.cursor;
            let previous = query.cursor_params.previous.unwrap_or(false);

            Ok(PaginationInfo::Cursor(CursorPaginationInfo {
                cursor,
                limit,
                sort: query.cursor_params.sort.unwrap_or_else(|| "created_at".to_string()),
                order: query.cursor_params.order.unwrap_or(SortDirection::Desc),
                previous,
            }))
        }
    }
}

/// Pagination information
#[derive(Debug, Clone)]
pub enum PaginationInfo {
    Offset(OffsetPaginationInfo),
    Cursor(CursorPaginationInfo),
}

/// Offset pagination information
#[derive(Debug, Clone)]
pub struct OffsetPaginationInfo {
    pub page: usize,
    pub limit: usize,
    pub sort: String,
    pub order: SortDirection,
}

/// Cursor pagination information
#[derive(Debug, Clone)]
pub struct CursorPaginationInfo {
    pub cursor: Option<String>,
    pub limit: usize,
    pub sort: String,
    pub order: SortDirection,
    pub previous: bool,
}

/// Create offset-based pagination metadata
pub fn create_offset_pagination_meta(
    info: &OffsetPaginationInfo,
    total_items: Option<usize>,
) -> OffsetPaginationMeta {
    let total_items = total_items.unwrap_or(0);
    let total_pages = if total_items > 0 {
        ((total_items - 1) / info.limit) + 1
    } else {
        0
    };

    let has_next = info.page < total_pages;
    let has_previous = info.page > 1;

    OffsetPaginationMeta {
        page: info.page,
        limit: info.limit,
        total_items: Some(total_items),
        total_pages: Some(total_pages),
        has_next,
        has_previous,
        next_page: if has_next { Some(info.page + 1) } else { None },
        previous_page: if has_previous { Some(info.page - 1) } else { None },
    }
}

/// Create cursor-based pagination metadata
pub fn create_cursor_pagination_meta(
    info: &CursorPaginationInfo,
    has_next: bool,
    has_previous: bool,
    next_cursor: Option<String>,
    previous_cursor: Option<String>,
) -> CursorPaginationMeta {
    CursorPaginationMeta {
        cursor: info.cursor.clone(),
        limit: info.limit,
        has_next,
        has_previous,
        next_cursor,
        previous_cursor,
    }
}

/// Generate pagination links
pub fn generate_pagination_links(
    base_url: &str,
    pagination_info: &PaginationInfo,
    meta: &PaginationMeta,
) -> PaginationLinks {
    match (pagination_info, meta) {
        (PaginationInfo::Offset(info), PaginationMeta::Offset(meta)) => {
            PaginationLinks {
                self_link: Some(format!(
                    "{}?page={}&limit={}&sort={}&order={:?}",
                    base_url, meta.page, meta.limit, info.sort, info.order
                )),
                first: Some(format!(
                    "{}?page=1&limit={}&sort={}&order={:?}",
                    base_url, meta.limit, info.sort, info.order
                )),
                prev: if meta.has_previous {
                    Some(format!(
                        "{}?page={}&limit={}&sort={}&order={:?}",
                        base_url, meta.previous_page.unwrap_or(1), meta.limit, info.sort, info.order
                    ))
                } else {
                    None
                },
                next: if meta.has_next {
                    Some(format!(
                        "{}?page={}&limit={}&sort={}&order={:?}",
                        base_url, meta.next_page.unwrap_or(1), meta.limit, info.sort, info.order
                    ))
                } else {
                    None
                },
                last: if let Some(total_pages) = meta.total_pages {
                    Some(format!(
                        "{}?page={}&limit={}&sort={}&order={:?}",
                        base_url, total_pages, meta.limit, info.sort, info.order
                    ))
                } else {
                    None
                },
            }
        }
        (PaginationInfo::Cursor(info), PaginationMeta::Cursor(meta)) => {
            PaginationLinks {
                self_link: Some(format!(
                    "{}?type=cursor&cursor={}&limit={}&sort={}&order={:?}",
                    base_url,
                    meta.cursor.as_deref().unwrap_or(""),
                    meta.limit,
                    info.sort,
                    info.order
                )),
                first: Some(format!(
                    "{}?type=cursor&limit={}&sort={}&order={:?}",
                    base_url, meta.limit, info.sort, info.order
                )),
                prev: if let Some(prev_cursor) = &meta.previous_cursor {
                    Some(format!(
                        "{}?type=cursor&cursor={}&limit={}&sort={}&order={:?}&previous=true",
                        base_url, prev_cursor, meta.limit, info.sort, info.order
                    ))
                } else {
                    None
                },
                next: if let Some(next_cursor) = &meta.next_cursor {
                    Some(format!(
                        "{}?type=cursor&cursor={}&limit={}&sort={}&order={:?}",
                        base_url, next_cursor, meta.limit, info.sort, info.order
                    ))
                } else {
                    None
                },
                last: None, // Cursor pagination doesn't have a concept of "last page"
            }
        }
        _ => PaginationLinks {
            self_link: None,
            first: None,
            prev: None,
            next: None,
            last: None,
        },
    }
}

/// Create a paginated response
pub fn create_paginated_response<T>(
    data: Vec<T>,
    pagination_info: PaginationInfo,
    total_items: Option<usize>,
    base_url: &str,
    has_next_page: Option<bool>,
    next_cursor: Option<String>,
    previous_cursor: Option<String>,
) -> PaginatedResponse<T> {
    let (meta, links) = match &pagination_info {
        PaginationInfo::Offset(info) => {
            let meta = create_offset_pagination_meta(info, total_items);
            let links = generate_pagination_links(base_url, &pagination_info, &PaginationMeta::Offset(meta.clone()));
            (PaginationMeta::Offset(meta), links)
        }
        PaginationType::Cursor => {
            let meta = create_cursor_pagination_meta(
                info,
                has_next_page.unwrap_or(false),
                !info.cursor.is_none(),
                next_cursor,
                previous_cursor,
            );
            let links = generate_pagination_links(base_url, &pagination_info, &PaginationMeta::Cursor(meta.clone()));
            (PaginationMeta::Cursor(meta), links)
        }
    };

    PaginatedResponse {
        data,
        pagination: meta,
        links,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_encoding_decoding() {
        let cursor = Cursor::new(
            "test_value".to_string(),
            chrono::Utc::now(),
            Uuid::new_v4(),
        );

        let encoded = cursor.encode();
        let decoded = Cursor::decode(&encoded).unwrap();

        assert_eq!(cursor.value, decoded.value);
        assert_eq!(cursor.id, decoded.id);
    }

    #[test]
    fn test_offset_pagination_info() {
        let query = PaginationQuery {
            pagination_type: Some(PaginationType::Offset),
            offset_params: OffsetPaginationQuery {
                page: Some(2),
                limit: Some(10),
                sort: Some("name".to_string()),
                order: Some(SortDirection::Asc),
            },
            cursor_params: CursorPaginationQuery::default(),
        };

        let config = PaginationConfig::default();
        let info = process_pagination_query(query, &config).unwrap();

        match info {
            PaginationInfo::Offset(offset_info) => {
                assert_eq!(offset_info.page, 2);
                assert_eq!(offset_info.limit, 10);
                assert_eq!(offset_info.sort, "name");
                assert_eq!(offset_info.order, SortDirection::Asc);
            }
            _ => panic!("Expected offset pagination info"),
        }
    }

    #[test]
    fn test_cursor_pagination_info() {
        let query = PaginationQuery {
            pagination_type: Some(PaginationType::Cursor),
            offset_params: OffsetPaginationQuery::default(),
            cursor_params: CursorPaginationQuery {
                cursor: Some("test_cursor".to_string()),
                limit: Some(5),
                sort: Some("created_at".to_string()),
                order: Some(SortDirection::Desc),
                previous: Some(false),
            },
        };

        let config = PaginationConfig::default();
        let info = process_pagination_query(query, &config).unwrap();

        match info {
            PaginationInfo::Cursor(cursor_info) => {
                assert_eq!(cursor_info.cursor, Some("test_cursor".to_string()));
                assert_eq!(cursor_info.limit, 5);
                assert_eq!(cursor_info.sort, "created_at");
                assert_eq!(cursor_info.order, SortDirection::Desc);
                assert!(!cursor_info.previous);
            }
            _ => panic!("Expected cursor pagination info"),
        }
    }
}
