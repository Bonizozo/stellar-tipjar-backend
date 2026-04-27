use uuid::Uuid;

use crate::db::connection::AppState;
use crate::errors::AppResult;
use crate::models::location::{
    BoundingBox, CreatorLocation, GeofenceQuery, LocationAnalytics, NearbyCreator, NearbyQuery,
    UpsertLocationRequest,
};

/// Upsert a creator's location (insert or update on conflict).
pub async fn upsert_location(
    state: &AppState,
    creator_id: Uuid,
    req: UpsertLocationRequest,
) -> AppResult<CreatorLocation> {
    let row = sqlx::query_as::<_, CreatorLocation>(
        r#"
        INSERT INTO creator_locations (creator_id, location, label, updated_at)
        VALUES ($1, ST_SetSRID(ST_MakePoint($3, $2), 4326)::geography, $4, NOW())
        ON CONFLICT (creator_id) DO UPDATE
            SET location   = EXCLUDED.location,
                label      = EXCLUDED.label,
                updated_at = NOW()
        RETURNING
            id,
            creator_id,
            ST_Y(location::geometry) AS latitude,
            ST_X(location::geometry) AS longitude,
            label,
            updated_at
        "#,
    )
    .bind(creator_id)
    .bind(req.latitude)
    .bind(req.longitude)
    .bind(req.label)
    .fetch_one(&state.db)
    .await?;

    Ok(row)
}

/// Get a creator's stored location.
pub async fn get_location(
    state: &AppState,
    creator_id: Uuid,
) -> AppResult<Option<CreatorLocation>> {
    let row = sqlx::query_as::<_, CreatorLocation>(
        r#"
        SELECT
            id,
            creator_id,
            ST_Y(location::geometry) AS latitude,
            ST_X(location::geometry) AS longitude,
            label,
            updated_at
        FROM creator_locations
        WHERE creator_id = $1
        "#,
    )
    .bind(creator_id)
    .fetch_optional(&state.db)
    .await?;

    Ok(row)
}

/// Find creators within `radius_m` metres of the given point, ordered by distance.
pub async fn nearby_creators(
    state: &AppState,
    query: &NearbyQuery,
) -> AppResult<Vec<NearbyCreator>> {
    let rows = sqlx::query_as::<_, NearbyCreator>(
        r#"
        SELECT
            cl.creator_id,
            c.username,
            ST_Y(cl.location::geometry)                                                    AS latitude,
            ST_X(cl.location::geometry)                                                    AS longitude,
            cl.label,
            ST_Distance(cl.location, ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography)   AS distance_m
        FROM creator_locations cl
        JOIN creators c ON c.id = cl.creator_id
        WHERE ST_DWithin(
            cl.location,
            ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography,
            $3
        )
        ORDER BY distance_m
        LIMIT $4
        "#,
    )
    .bind(query.lat)
    .bind(query.lng)
    .bind(query.radius())
    .bind(query.limit())
    .fetch_all(&state.db)
    .await?;

    Ok(rows)
}

/// Return all creators inside the geofence circle, ordered by distance.
pub async fn creators_in_geofence(
    state: &AppState,
    query: &GeofenceQuery,
) -> AppResult<Vec<NearbyCreator>> {
    let rows = sqlx::query_as::<_, NearbyCreator>(
        r#"
        SELECT
            cl.creator_id,
            c.username,
            ST_Y(cl.location::geometry)                                                    AS latitude,
            ST_X(cl.location::geometry)                                                    AS longitude,
            cl.label,
            ST_Distance(cl.location, ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography)   AS distance_m
        FROM creator_locations cl
        JOIN creators c ON c.id = cl.creator_id
        WHERE ST_DWithin(
            cl.location,
            ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography,
            $3
        )
        ORDER BY distance_m
        "#,
    )
    .bind(query.lat)
    .bind(query.lng)
    .bind(query.radius_m)
    .fetch_all(&state.db)
    .await?;

    Ok(rows)
}

/// Aggregate location analytics: total located creators + bounding box.
pub async fn location_analytics(state: &AppState) -> AppResult<LocationAnalytics> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM creator_locations")
        .fetch_one(&state.db)
        .await?;

    if total == 0 {
        return Ok(LocationAnalytics {
            total_located: 0,
            bounding_box: None,
        });
    }

    let row: (f64, f64, f64, f64) = sqlx::query_as(
        r#"
        SELECT
            MIN(ST_Y(location::geometry)),
            MAX(ST_Y(location::geometry)),
            MIN(ST_X(location::geometry)),
            MAX(ST_X(location::geometry))
        FROM creator_locations
        "#,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(LocationAnalytics {
        total_located: total,
        bounding_box: Some(BoundingBox {
            min_lat: row.0,
            max_lat: row.1,
            min_lng: row.2,
            max_lng: row.3,
        }),
    })
}
