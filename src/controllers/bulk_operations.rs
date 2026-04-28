use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::errors::AppError;
use crate::models::{Creator, Tip};
use crate::db::DatabasePool;

/// Configuration for bulk operations
#[derive(Debug, Clone)]
pub struct BulkOperationConfig {
    /// Maximum number of items per bulk request
    pub max_items_per_request: usize,
    /// Timeout for bulk operations
    pub operation_timeout: std::time::Duration,
    /// Whether to continue on partial failures
    pub continue_on_failure: bool,
    /// Maximum concurrent operations
    pub max_concurrent_operations: usize,
}

impl Default for BulkOperationConfig {
    fn default() -> Self {
        Self {
            max_items_per_request: 1000,
            operation_timeout: std::time::Duration::from_secs(300), // 5 minutes
            continue_on_failure: true,
            max_concurrent_operations: 10,
        }
    }
}

/// Bulk operation request metadata
#[derive(Debug, Deserialize)]
pub struct BulkOperationRequest {
    /// Optional request ID for tracking
    pub request_id: Option<String>,
    /// Whether to continue processing on individual failures
    pub continue_on_failure: Option<bool>,
    /// Maximum operations to process (for testing/limiting)
    pub max_operations: Option<usize>,
}

/// Bulk operation result for a single item
#[derive(Debug, Serialize, Deserialize)]
pub struct BulkOperationResult<T> {
    /// Index of the item in the original request
    pub index: usize,
    /// Whether the operation succeeded
    pub success: bool,
    /// The created/updated/deleted resource (if successful)
    pub data: Option<T>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Operation ID for tracking
    pub operation_id: String,
}

/// Bulk operation response
#[derive(Debug, Serialize)]
pub struct BulkOperationResponse<T> {
    /// Overall request ID
    pub request_id: String,
    /// Total number of items processed
    pub total_items: usize,
    /// Number of successful operations
    pub successful_count: usize,
    /// Number of failed operations
    pub failed_count: usize,
    /// Individual operation results
    pub results: Vec<BulkOperationResult<T>>,
    /// Operation summary
    pub summary: BulkOperationSummary,
}

/// Summary statistics for bulk operations
#[derive(Debug, Serialize)]
pub struct BulkOperationSummary {
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
    /// Average time per operation in milliseconds
    pub avg_time_per_operation_ms: f64,
    /// Success rate as percentage
    pub success_rate: f64,
    /// Whether any operations failed
    pub has_failures: bool,
}

/// Bulk create request for creators
#[derive(Debug, Deserialize)]
pub struct BulkCreateCreatorsRequest {
    #[serde(flatten)]
    pub metadata: BulkOperationRequest,
    /// List of creators to create
    pub creators: Vec<Creator>,
}

/// Bulk update request for creators
#[derive(Debug, Deserialize)]
pub struct BulkUpdateCreatorsRequest {
    #[serde(flatten)]
    pub metadata: BulkOperationRequest,
    /// List of creator updates with ID
    pub creators: Vec<CreatorUpdate>,
}

/// Creator update data
#[derive(Debug, Deserialize)]
pub struct CreatorUpdate {
    pub id: Uuid,
    pub name: Option<String>,
    pub email: Option<String>,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
}

/// Bulk delete request
#[derive(Debug, Deserialize)]
pub struct BulkDeleteRequest {
    #[serde(flatten)]
    pub metadata: BulkOperationRequest,
    /// List of resource IDs to delete
    pub ids: Vec<Uuid>,
}

/// Query parameters for bulk operations
#[derive(Debug, Deserialize)]
pub struct BulkOperationQuery {
    /// Whether to return detailed results
    pub details: Option<bool>,
    /// Maximum number of results to return
    pub limit: Option<usize>,
    /// Offset for pagination of results
    pub offset: Option<usize>,
}

/// State for bulk operations
#[derive(Clone)]
pub struct BulkOperationState {
    pub db_pool: Arc<DatabasePool>,
    pub config: BulkOperationConfig,
}

impl BulkOperationState {
    pub fn new(db_pool: Arc<DatabasePool>, config: BulkOperationConfig) -> Self {
        Self { db_pool, config }
    }
}

/// Generate a unique operation ID
fn generate_operation_id() -> String {
    format!("bulk_{}", Uuid::new_v4())
}

/// Validate bulk request size
fn validate_bulk_request_size<T>(items: &[T], config: &BulkOperationConfig) -> Result<(), AppError> {
    if items.len() > config.max_items_per_request {
        return Err(AppError::bad_request(format!(
            "Bulk request exceeds maximum size of {} items",
            config.max_items_per_request
        )));
    }
    
    if items.is_empty() {
        return Err(AppError::bad_request("Bulk request cannot be empty"));
    }
    
    Ok(())
}

/// Create bulk creators endpoint
pub async fn bulk_create_creators(
    State(state): State<Arc<BulkOperationState>>,
    Json(request): Json<BulkCreateCreatorsRequest>,
    Query(query): Query<BulkOperationQuery>,
) -> Result<Json<BulkOperationResponse<Creator>>, AppError> {
    let start_time = std::time::Instant::now();
    let request_id = request.metadata.request_id.unwrap_or_else(generate_operation_id);
    let continue_on_failure = request.metadata.continue_on_failure.unwrap_or(state.config.continue_on_failure);
    let max_operations = request.metadata.max_operations.unwrap_or(request.creators.len());

    // Validate request
    validate_bulk_request_size(&request.creators, &state.config)?;

    let mut results = Vec::new();
    let mut successful_count = 0;
    let mut failed_count = 0;

    // Process creators in batches to avoid overwhelming the database
    let mut creators_to_process = request.creators.into_iter().take(max_operations).enumerate();

    while let Some((index, creator)) = creators_to_process.next() {
        let operation_id = generate_operation_id();
        
        match create_single_creator(&state.db_pool, creator).await {
            Ok(created_creator) => {
                results.push(BulkOperationResult {
                    index,
                    success: true,
                    data: Some(created_creator),
                    error: None,
                    operation_id,
                });
                successful_count += 1;
            }
            Err(e) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                    operation_id,
                });
                failed_count += 1;
                
                if !continue_on_failure {
                    break;
                }
            }
        }
    }

    let processing_time = start_time.elapsed();
    let total_items = results.len();
    let success_rate = if total_items > 0 {
        successful_count as f64 / total_items as f64 * 100.0
    } else {
        0.0
    };

    let summary = BulkOperationSummary {
        processing_time_ms: processing_time.as_millis() as u64,
        avg_time_per_operation_ms: if total_items > 0 {
            processing_time.as_millis() as f64 / total_items as f64
        } else {
            0.0
        },
        success_rate,
        has_failures: failed_count > 0,
    };

    // Apply query filters
    let mut results = results;
    if let Some(offset) = query.offset {
        results = results.into_iter().skip(offset).collect();
    }
    if let Some(limit) = query.limit {
        results = results.into_iter().take(limit).collect();
    }

    let response = BulkOperationResponse {
        request_id,
        total_items,
        successful_count,
        failed_count,
        results,
        summary,
    };

    Ok(Json(response))
}

/// Update bulk creators endpoint
pub async fn bulk_update_creators(
    State(state): State<Arc<BulkOperationState>>,
    Json(request): Json<BulkUpdateCreatorsRequest>,
    Query(query): Query<BulkOperationQuery>,
) -> Result<Json<BulkOperationResponse<Creator>>, AppError> {
    let start_time = std::time::Instant::now();
    let request_id = request.metadata.request_id.unwrap_or_else(generate_operation_id);
    let continue_on_failure = request.metadata.continue_on_failure.unwrap_or(state.config.continue_on_failure);
    let max_operations = request.metadata.max_operations.unwrap_or(request.creators.len());

    // Validate request
    validate_bulk_request_size(&request.creators, &state.config)?;

    let mut results = Vec::new();
    let mut successful_count = 0;
    let mut failed_count = 0;

    for (index, update) in request.creators.into_iter().take(max_operations).enumerate() {
        let operation_id = generate_operation_id();
        
        match update_single_creator(&state.db_pool, update).await {
            Ok(updated_creator) => {
                results.push(BulkOperationResult {
                    index,
                    success: true,
                    data: Some(updated_creator),
                    error: None,
                    operation_id,
                });
                successful_count += 1;
            }
            Err(e) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                    operation_id,
                });
                failed_count += 1;
                
                if !continue_on_failure {
                    break;
                }
            }
        }
    }

    let processing_time = start_time.elapsed();
    let total_items = results.len();
    let success_rate = if total_items > 0 {
        successful_count as f64 / total_items as f64 * 100.0
    } else {
        0.0
    };

    let summary = BulkOperationSummary {
        processing_time_ms: processing_time.as_millis() as u64,
        avg_time_per_operation_ms: if total_items > 0 {
            processing_time.as_millis() as f64 / total_items as f64
        } else {
            0.0
        },
        success_rate,
        has_failures: failed_count > 0,
    };

    // Apply query filters
    let mut results = results;
    if let Some(offset) = query.offset {
        results = results.into_iter().skip(offset).collect();
    }
    if let Some(limit) = query.limit {
        results = results.into_iter().take(limit).collect();
    }

    let response = BulkOperationResponse {
        request_id,
        total_items,
        successful_count,
        failed_count,
        results,
        summary,
    };

    Ok(Json(response))
}

/// Bulk delete creators endpoint
pub async fn bulk_delete_creators(
    State(state): State<Arc<BulkOperationState>>,
    Json(request): Json<BulkDeleteRequest>,
    Query(query): Query<BulkOperationQuery>,
) -> Result<Json<BulkOperationResponse<Uuid>>, AppError> {
    let start_time = std::time::Instant::now();
    let request_id = request.metadata.request_id.unwrap_or_else(generate_operation_id);
    let continue_on_failure = request.metadata.continue_on_failure.unwrap_or(state.config.continue_on_failure);
    let max_operations = request.metadata.max_operations.unwrap_or(request.ids.len());

    // Validate request
    validate_bulk_request_size(&request.ids, &state.config)?;

    let mut results = Vec::new();
    let mut successful_count = 0;
    let mut failed_count = 0;

    for (index, id) in request.ids.into_iter().take(max_operations).enumerate() {
        let operation_id = generate_operation_id();
        
        match delete_single_creator(&state.db_pool, id).await {
            Ok(_) => {
                results.push(BulkOperationResult {
                    index,
                    success: true,
                    data: Some(id),
                    error: None,
                    operation_id,
                });
                successful_count += 1;
            }
            Err(e) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                    operation_id,
                });
                failed_count += 1;
                
                if !continue_on_failure {
                    break;
                }
            }
        }
    }

    let processing_time = start_time.elapsed();
    let total_items = results.len();
    let success_rate = if total_items > 0 {
        successful_count as f64 / total_items as f64 * 100.0
    } else {
        0.0
    };

    let summary = BulkOperationSummary {
        processing_time_ms: processing_time.as_millis() as u64,
        avg_time_per_operation_ms: if total_items > 0 {
            processing_time.as_millis() as f64 / total_items as f64
        } else {
            0.0
        },
        success_rate,
        has_failures: failed_count > 0,
    };

    // Apply query filters
    let mut results = results;
    if let Some(offset) = query.offset {
        results = results.into_iter().skip(offset).collect();
    }
    if let Some(limit) = query.limit {
        results = results.into_iter().take(limit).collect();
    }

    let response = BulkOperationResponse {
        request_id,
        total_items,
        successful_count,
        failed_count,
        results,
        summary,
    };

    Ok(Json(response))
}

/// Helper function to create a single creator
async fn create_single_creator(
    db_pool: &DatabasePool,
    creator: Creator,
) -> Result<Creator, AppError> {
    // This would typically use SQLx to insert into the database
    // For now, we'll simulate the operation
    tracing::info!("Creating creator: {}", creator.name);
    
    // Simulate database operation
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    
    // In a real implementation, this would be:
    // let query = sqlx::query_as!(
    //     Creator,
    //     "INSERT INTO creators (id, name, email, bio, avatar_url, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *",
    //     creator.id,
    //     creator.name,
    //     creator.email,
    //     creator.bio,
    //     creator.avatar_url,
    //     chrono::Utc::now(),
    //     chrono::Utc::now()
    // )
    // .fetch_one(db_pool)
    // .await?;
    
    Ok(creator)
}

/// Helper function to update a single creator
async fn update_single_creator(
    db_pool: &DatabasePool,
    update: CreatorUpdate,
) -> Result<Creator, AppError> {
    tracing::info!("Updating creator: {}", update.id);
    
    // Simulate database operation
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    
    // In a real implementation, this would fetch the existing creator,
    // apply updates, and save it back to the database
    
    // For now, return a mock creator
    Ok(Creator {
        id: update.id,
        name: update.name.unwrap_or_default(),
        email: update.email,
        bio: update.bio,
        avatar_url: update.avatar_url,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    })
}

/// Helper function to delete a single creator
async fn delete_single_creator(
    db_pool: &DatabasePool,
    id: Uuid,
) -> Result<(), AppError> {
    tracing::info!("Deleting creator: {}", id);
    
    // Simulate database operation
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    
    // In a real implementation, this would be:
    // sqlx::query!("DELETE FROM creators WHERE id = $1", id)
    //     .execute(db_pool)
    //     .await?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_bulk_request_size() {
        let config = BulkOperationConfig {
            max_items_per_request: 100,
            ..Default::default()
        };

        let items: Vec<i32> = vec![1; 50];
        assert!(validate_bulk_request_size(&items, &config).is_ok());

        let items: Vec<i32> = vec![1; 150];
        assert!(validate_bulk_request_size(&items, &config).is_err());

        let items: Vec<i32> = vec![];
        assert!(validate_bulk_request_size(&items, &config).is_err());
    }

    #[test]
    fn test_generate_operation_id() {
        let id1 = generate_operation_id();
        let id2 = generate_operation_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("bulk_"));
    }
}
