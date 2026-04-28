use crate::pagination::{
    PaginationInfo, OffsetPaginationInfo, CursorPaginationInfo, PaginationType,
    PaginatedResponse, create_paginated_response, Cursor
};
use crate::errors::AppError;
use std::sync::Arc;
use uuid::Uuid;

/// Trait for types that can be paginated
pub trait Paginatable {
    /// Get the cursor value for this item
    fn get_cursor_value(&self, sort_field: &str) -> Result<String, AppError>;
    
    /// Get the ID for cursor generation
    fn get_id(&self) -> Uuid;
    
    /// Get the timestamp for cursor ordering
    fn get_timestamp(&self) -> chrono::DateTime<chrono::Utc>;
}

/// Trait for database queries that support pagination
#[async_trait::async_trait]
pub trait PaginatedQuery<T> {
    /// Execute a paginated query with offset pagination
    async fn execute_offset_query(
        &self,
        info: &OffsetPaginationInfo,
    ) -> Result<(Vec<T>, Option<usize>), AppError>;
    
    /// Execute a paginated query with cursor pagination
    async fn execute_cursor_query(
        &self,
        info: &CursorPaginationInfo,
    ) -> Result<(Vec<T>, bool), AppError>;
}

/// Helper to build paginated responses
pub struct PaginationHelper<T> {
    items: Vec<T>,
    pagination_info: PaginationInfo,
    base_url: String,
}

impl<T: Paginatable> PaginationHelper<T> {
    pub fn new(items: Vec<T>, pagination_info: PaginationInfo, base_url: String) -> Self {
        Self {
            items,
            pagination_info,
            base_url,
        }
    }

    /// Build the final paginated response
    pub async fn build_response(
        self,
        total_items: Option<usize>,
    ) -> Result<PaginatedResponse<T>, AppError> {
        let (has_next, next_cursor, previous_cursor) = match &self.pagination_info {
            PaginationInfo::Offset(_) => {
                // For offset pagination, we can determine has_next from total_items
                let has_next = if let (Some(total), OffsetPaginationInfo { page, limit, .. }) = 
                    (total_items, &self.pagination_info) {
                    (page * limit) < total
                } else {
                    false
                };
                (has_next, None, None)
            }
            PaginationInfo::Cursor(info) => {
                // For cursor pagination, we need to generate cursors from the items
                let (has_next, next_cursor, previous_cursor) = self.generate_cursor_info(info)?;
                (has_next, Some(next_cursor), Some(previous_cursor))
            }
        };

        Ok(create_paginated_response(
            self.items,
            self.pagination_info,
            total_items,
            &self.base_url,
            Some(has_next),
            next_cursor,
            previous_cursor,
        ))
    }

    /// Generate cursor information for cursor-based pagination
    fn generate_cursor_info(
        &self,
        info: &CursorPaginationInfo,
    ) -> Result<(bool, String, String), AppError> {
        if self.items.is_empty() {
            return Ok((false, String::new(), String::new()));
        }

        // Determine if there are more items
        let has_next = self.items.len() == info.limit + 1;
        
        // Take only the requested number of items
        let items = if has_next {
            &self.items[..info.limit]
        } else {
            &self.items
        };

        // Generate next cursor from the last item
        let next_cursor = if has_next && !items.is_empty() {
            let last_item = items.last().unwrap();
            let cursor = Cursor::new(
                last_item.get_cursor_value(&info.sort)?,
                last_item.get_timestamp(),
                last_item.get_id(),
            );
            cursor.encode()
        } else {
            String::new()
        };

        // Generate previous cursor from the first item
        let previous_cursor = if !items.is_empty() {
            let first_item = items.first().unwrap();
            let cursor = Cursor::new(
                first_item.get_cursor_value(&info.sort)?,
                first_item.get_timestamp(),
                first_item.get_id(),
            );
            cursor.encode()
        } else {
            String::new()
        };

        Ok((has_next, next_cursor, previous_cursor))
    }
}

/// Extension trait to make pagination easier
pub trait PaginationExt<T> {
    /// Convert a query result to a paginated response
    fn to_paginated_response(
        self,
        pagination_info: PaginationInfo,
        base_url: &str,
        total_items: Option<usize>,
    ) -> Result<PaginatedResponse<T>, AppError>;
}

impl<T: Paginatable> PaginationExt<T> for Vec<T> {
    fn to_paginated_response(
        self,
        pagination_info: PaginationInfo,
        base_url: &str,
        total_items: Option<usize>,
    ) -> Result<PaginatedResponse<T>, AppError> {
        let helper = PaginationHelper::new(self, pagination_info, base_url.to_string());
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(helper.build_response(total_items))
        })
    }
}

/// Builder for constructing pagination queries
pub struct PaginationQueryBuilder {
    base_query: String,
    sort_field: String,
    sort_order: String,
}

impl PaginationQueryBuilder {
    pub fn new(base_query: &str) -> Self {
        Self {
            base_query: base_query.to_string(),
            sort_field: "created_at".to_string(),
            sort_order: "DESC".to_string(),
        }
    }

    pub fn sort_by(mut self, field: &str) -> Self {
        self.sort_field = field.to_string();
        self
    }

    pub fn order_by(mut self, order: &str) -> Self {
        self.sort_order = order.to_string();
        self
    }

    /// Build offset pagination query
    pub fn build_offset_query(&self, info: &OffsetPaginationInfo) -> String {
        let offset = (info.page - 1) * info.limit;
        format!(
            "{} ORDER BY {} {} LIMIT {} OFFSET {}",
            self.base_query,
            info.sort,
            if info.order == crate::pagination::SortDirection::Asc { "ASC" } else { "DESC" },
            info.limit,
            offset
        )
    }

    /// Build cursor pagination query
    pub fn build_cursor_query(&self, info: &CursorPaginationInfo) -> Result<String, AppError> {
        let mut query = self.base_query.clone();
        
        if let Some(cursor_str) = &info.cursor {
            let cursor = Cursor::decode(cursor_str)?;
            
            let operator = if info.previous { "<" } else { ">" };
            let order = if info.previous { "DESC" } else { "ASC" };
            
            query.push_str(&format!(
                " AND ({} {} '{}' OR ({} = '{}' AND id {} '{}'))",
                info.sort, operator, cursor.value,
                info.sort, cursor.value,
                if info.previous { ">" } else { "<" },
                cursor.id
            ));
            
            query.push_str(&format!(" ORDER BY {} {} LIMIT {}", info.sort, order, info.limit + 1));
        } else {
            query.push_str(&format!(
                " ORDER BY {} {} LIMIT {}",
                info.sort,
                if info.order == crate::pagination::SortDirection::Asc { "ASC" } else { "DESC" },
                info.limit + 1
            ));
        }

        Ok(query)
    }
}

/// Helper to validate pagination parameters
pub fn validate_pagination_params(
    page: Option<usize>,
    limit: Option<usize>,
    max_limit: usize,
) -> Result<(usize, usize), AppError> {
    let page = page.unwrap_or(1);
    let limit = limit.unwrap_or(20);

    if page == 0 {
        return Err(AppError::bad_request("Page number must be greater than 0"));
    }

    if limit == 0 {
        return Err(AppError::bad_request("Limit must be greater than 0"));
    }

    if limit > max_limit {
        return Err(AppError::bad_request(format!(
            "Limit cannot exceed {}",
            max_limit
        )));
    }

    Ok((page, limit))
}

/// Helper to extract sort information from query parameters
pub fn extract_sort_info(
    sort: Option<String>,
    order: Option<crate::pagination::SortDirection>,
    default_sort: &str,
) -> (String, crate::pagination::SortDirection) {
    let sort = sort.unwrap_or_else(|| default_sort.to_string());
    let order = order.unwrap_or(crate::pagination::SortDirection::Desc);
    (sort, order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pagination::{OffsetPaginationInfo, CursorPaginationInfo, SortDirection};

    // Mock item for testing
    #[derive(Debug, Clone)]
    struct MockItem {
        id: Uuid,
        name: String,
        created_at: chrono::DateTime<chrono::Utc>,
    }

    impl Paginatable for MockItem {
        fn get_cursor_value(&self, sort_field: &str) -> Result<String, AppError> {
            match sort_field {
                "name" => Ok(self.name.clone()),
                "created_at" => Ok(self.created_at.to_rfc3339()),
                _ => Err(AppError::bad_request("Invalid sort field"))
            }
        }

        fn get_id(&self) -> Uuid {
            self.id
        }

        fn get_timestamp(&self) -> chrono::DateTime<chrono::Utc> {
            self.created_at
        }
    }

    #[test]
    fn test_pagination_query_builder() {
        let builder = PaginationQueryBuilder::new("SELECT * FROM users")
            .sort_by("name")
            .order_by("ASC");

        let info = OffsetPaginationInfo {
            page: 2,
            limit: 10,
            sort: "name".to_string(),
            order: SortDirection::Asc,
        };

        let query = builder.build_offset_query(&info);
        assert!(query.contains("ORDER BY name ASC"));
        assert!(query.contains("LIMIT 10"));
        assert!(query.contains("OFFSET 10"));
    }

    #[test]
    fn test_validate_pagination_params() {
        assert!(validate_pagination_params(Some(1), Some(10), 100).is_ok());
        assert!(validate_pagination_params(Some(0), Some(10), 100).is_err());
        assert!(validate_pagination_params(Some(1), Some(0), 100).is_err());
        assert!(validate_pagination_params(Some(1), Some(200), 100).is_err());
    }

    #[test]
    fn test_extract_sort_info() {
        let (sort, order) = extract_sort_info(
            Some("name".to_string()),
            Some(SortDirection::Asc),
            "created_at",
        );
        assert_eq!(sort, "name");
        assert_eq!(order, SortDirection::Asc);

        let (sort, order) = extract_sort_info(None, None, "created_at");
        assert_eq!(sort, "created_at");
        assert_eq!(order, SortDirection::Desc);
    }
}
