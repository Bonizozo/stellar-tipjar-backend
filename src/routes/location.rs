use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
    Json, Router,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::location_controller;
use crate::db::connection::AppState;
use crate::errors::{AppError, ValidationError};
use crate::models::location::{GeofenceQuery, NearbyQuery, UpsertLocationRequest};
use crate::validation::ValidatedJson;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/locations/nearby", get(nearby))
        .route("/locations/geofence", get(geofence))
        .route("/locations/analytics", get(analytics))
        .route("/creators/:creator_id/location", get(get_location))
        .route("/creators/:creator_id/location", put(upsert_location))
}

async fn upsert_location(
    State(state): State<Arc<AppState>>,
    Path(creator_id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<UpsertLocationRequest>,
) -> Result<impl IntoResponse, AppError> {
    let loc = location_controller::upsert_location(&state, creator_id, body).await?;
    Ok((StatusCode::OK, Json(loc)))
}

async fn get_location(
    State(state): State<Arc<AppState>>,
    Path(creator_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    match location_controller::get_location(&state, creator_id).await? {
        Some(loc) => Ok((StatusCode::OK, Json(serde_json::json!(loc))).into_response()),
        None => Ok((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "location not found"}))).into_response()),
    }
}

async fn nearby(
    State(state): State<Arc<AppState>>,
    Query(query): Query<NearbyQuery>,
) -> Result<impl IntoResponse, AppError> {
    validate_nearby(&query)?;
    let creators = location_controller::nearby_creators(&state, &query).await?;
    Ok((StatusCode::OK, Json(creators)))
}

async fn geofence(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GeofenceQuery>,
) -> Result<impl IntoResponse, AppError> {
    validate_geofence(&query)?;
    let creators = location_controller::creators_in_geofence(&state, &query).await?;
    Ok((StatusCode::OK, Json(creators)))
}

async fn analytics(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let stats = location_controller::location_analytics(&state).await?;
    Ok((StatusCode::OK, Json(stats)))
}

fn validate_nearby(q: &NearbyQuery) -> Result<(), AppError> {
    if q.lat < -90.0 || q.lat > 90.0 || q.lng < -180.0 || q.lng > 180.0 {
        return Err(AppError::Validation(ValidationError::InvalidRequest {
            message: "Invalid coordinates".to_string(),
        }));
    }
    Ok(())
}

fn validate_geofence(q: &GeofenceQuery) -> Result<(), AppError> {
    if q.lat < -90.0 || q.lat > 90.0 || q.lng < -180.0 || q.lng > 180.0 {
        return Err(AppError::Validation(ValidationError::InvalidRequest {
            message: "Invalid coordinates".to_string(),
        }));
    }
    if q.radius_m <= 0.0 || q.radius_m > 100_000.0 {
        return Err(AppError::Validation(ValidationError::InvalidRequest {
            message: "radius_m must be between 1 and 100000".to_string(),
        }));
    }
    Ok(())
}
