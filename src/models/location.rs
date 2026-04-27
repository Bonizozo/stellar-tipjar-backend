use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

/// Stored creator location row (PostGIS returns lat/lon as separate columns via ST_Y/ST_X).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CreatorLocation {
    pub id: Uuid,
    pub creator_id: Uuid,
    pub latitude: f64,
    pub longitude: f64,
    pub label: Option<String>,
    pub updated_at: DateTime<Utc>,
}

/// Upsert a creator's location.
#[derive(Debug, Deserialize, Validate)]
pub struct UpsertLocationRequest {
    #[validate(range(min = -90.0, max = 90.0, message = "latitude must be between -90 and 90"))]
    pub latitude: f64,
    #[validate(range(min = -180.0, max = 180.0, message = "longitude must be between -180 and 180"))]
    pub longitude: f64,
    #[validate(length(max = 100))]
    pub label: Option<String>,
}

/// Query params for nearby search.
#[derive(Debug, Deserialize, Validate)]
pub struct NearbyQuery {
    #[validate(range(min = -90.0, max = 90.0))]
    pub lat: f64,
    #[validate(range(min = -180.0, max = 180.0))]
    pub lng: f64,
    /// Radius in metres (default 10 000, max 100 000).
    pub radius_m: Option<f64>,
    pub limit: Option<i64>,
}

impl NearbyQuery {
    pub fn radius(&self) -> f64 {
        self.radius_m.unwrap_or(10_000.0).min(100_000.0)
    }
    pub fn limit(&self) -> i64 {
        self.limit.unwrap_or(20).min(100)
    }
}

/// Query params for geofence check.
#[derive(Debug, Deserialize, Validate)]
pub struct GeofenceQuery {
    #[validate(range(min = -90.0, max = 90.0))]
    pub lat: f64,
    #[validate(range(min = -180.0, max = 180.0))]
    pub lng: f64,
    /// Radius in metres.
    #[validate(range(min = 1.0, max = 100_000.0))]
    pub radius_m: f64,
}

/// A creator with its distance from the query point.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct NearbyCreator {
    pub creator_id: Uuid,
    pub username: String,
    pub latitude: f64,
    pub longitude: f64,
    pub label: Option<String>,
    /// Distance in metres.
    pub distance_m: f64,
}

/// Location analytics summary.
#[derive(Debug, Serialize)]
pub struct LocationAnalytics {
    pub total_located: i64,
    pub bounding_box: Option<BoundingBox>,
}

#[derive(Debug, Serialize)]
pub struct BoundingBox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lng: f64,
    pub max_lng: f64,
}
