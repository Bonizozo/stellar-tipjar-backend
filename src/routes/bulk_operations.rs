use axum::{
    routing::{get, post, put, delete},
    Router,
};
use std::sync::Arc;

use crate::controllers::bulk_operations::{
    bulk_create_creators, bulk_update_creators, bulk_delete_creators,
    BulkOperationState, BulkOperationConfig,
};
use crate::db::DatabasePool;

/// Create bulk operations router
pub fn create_bulk_operations_router(
    db_pool: Arc<DatabasePool>,
    config: BulkOperationConfig,
) -> Router {
    let state = Arc::new(BulkOperationState::new(db_pool, config));

    Router::new()
        .route("/creators/bulk", post(bulk_create_creators))
        .route("/creators/bulk", put(bulk_update_creators))
        .route("/creators/bulk", delete(bulk_delete_creators))
        .with_state(state)
}

/// Get bulk operations configuration with sensible defaults
pub fn default_bulk_config() -> BulkOperationConfig {
    BulkOperationConfig::default()
}
