use std::sync::Arc;

use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::api_key::{ApiKey, ApiKeyCreated, ApiKeyView, CreateApiKeyRequest};

pub async fn create_api_key(
    state: &Arc<AppState>,
    req: CreateApiKeyRequest,
) -> Result<ApiKeyCreated, AppError> {
    ApiKey::create(&state.db, &req.name, &req.permissions)
        .await
        .map_err(AppError::from)
}

pub async fn list_api_keys(state: &Arc<AppState>) -> Result<Vec<ApiKeyView>, AppError> {
    let keys = ApiKey::list(&state.db).await.map_err(AppError::from)?;
    Ok(keys.into_iter().map(ApiKeyView::from).collect())
}

pub async fn get_api_key(state: &Arc<AppState>, key: &str) -> Result<ApiKeyView, AppError> {
    ApiKey::get_by_key(&state.db, key)
        .await
        .map(ApiKeyView::from)
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::unauthorized("API key not found"),
            other => AppError::from(other),
        })
}

pub async fn rotate_api_key(
    state: &Arc<AppState>,
    key: &str,
) -> Result<ApiKeyCreated, AppError> {
    ApiKey::rotate(&state.db, key).await.map_err(|e| match e {
        sqlx::Error::RowNotFound => AppError::unauthorized("API key not found or already inactive"),
        other => AppError::from(other),
    })
}

pub async fn revoke_api_key(state: &Arc<AppState>, key: &str) -> Result<(), AppError> {
    ApiKey::revoke(&state.db, key).await.map_err(|e| match e {
        sqlx::Error::RowNotFound => AppError::unauthorized("API key not found"),
        other => AppError::from(other),
    })
}
