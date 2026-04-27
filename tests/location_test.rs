use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::json;
use uuid::Uuid;

mod common;

/// Insert a creator directly and return its UUID.
async fn insert_creator(pool: &sqlx::PgPool, username: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO creators (id, username, wallet_address, password_hash, totp_enabled, backup_code_hashes, created_at)
         VALUES ($1, $2, $3, '', false, '{}', NOW())",
    )
    .bind(id)
    .bind(username)
    .bind(format!("G{}", username.to_uppercase()))
    .execute(pool)
    .await
    .unwrap();
    id
}

/// Insert a location directly for a creator.
async fn insert_location(pool: &sqlx::PgPool, creator_id: Uuid, lat: f64, lng: f64) {
    sqlx::query(
        "INSERT INTO creator_locations (creator_id, location, updated_at)
         VALUES ($1, ST_SetSRID(ST_MakePoint($3, $2), 4326)::geography, NOW())
         ON CONFLICT (creator_id) DO UPDATE
             SET location = EXCLUDED.location, updated_at = NOW()",
    )
    .bind(creator_id)
    .bind(lat)
    .bind(lng)
    .execute(pool)
    .await
    .unwrap();
}

// ── Upsert location ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_upsert_location_creates_new() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let creator_id = insert_creator(&pool, "loc_creator1").await;

    let resp = server
        .put(&format!("/creators/{}/location", creator_id))
        .json(&json!({ "latitude": 48.8566, "longitude": 2.3522 }))
        .await;

    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    assert_eq!(body["creator_id"], creator_id.to_string());
    assert!((body["latitude"].as_f64().unwrap() - 48.8566).abs() < 0.0001);
    assert!((body["longitude"].as_f64().unwrap() - 2.3522).abs() < 0.0001);

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_upsert_location_updates_existing() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let creator_id = insert_creator(&pool, "loc_creator2").await;

    // First upsert
    server
        .put(&format!("/creators/{}/location", creator_id))
        .json(&json!({ "latitude": 40.7128, "longitude": -74.0060 }))
        .await;

    // Second upsert — should update
    let resp = server
        .put(&format!("/creators/{}/location", creator_id))
        .json(&json!({ "latitude": 51.5074, "longitude": -0.1278, "label": "London" }))
        .await;

    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    assert!((body["latitude"].as_f64().unwrap() - 51.5074).abs() < 0.0001);
    assert_eq!(body["label"], "London");

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_upsert_location_invalid_coords() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let creator_id = Uuid::new_v4();

    let resp = server
        .put(&format!("/creators/{}/location", creator_id))
        .json(&json!({ "latitude": 999.0, "longitude": 0.0 }))
        .await;

    resp.assert_status(StatusCode::BAD_REQUEST);

    common::cleanup_test_db(&pool).await;
}

// ── Get location ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_location_found() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let creator_id = insert_creator(&pool, "loc_creator3").await;
    insert_location(&pool, creator_id, 35.6762, 139.6503).await;

    let resp = server
        .get(&format!("/creators/{}/location", creator_id))
        .await;

    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    assert!((body["latitude"].as_f64().unwrap() - 35.6762).abs() < 0.0001);
    assert!((body["longitude"].as_f64().unwrap() - 139.6503).abs() < 0.0001);

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_get_location_not_found() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let resp = server
        .get(&format!("/creators/{}/location", Uuid::new_v4()))
        .await;

    resp.assert_status(StatusCode::NOT_FOUND);

    common::cleanup_test_db(&pool).await;
}

// ── Nearby ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_nearby_returns_creators_within_radius() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    // Paris ~0 km away, Tokyo ~9700 km away
    let paris_id = insert_creator(&pool, "loc_paris").await;
    let tokyo_id = insert_creator(&pool, "loc_tokyo").await;
    insert_location(&pool, paris_id, 48.8566, 2.3522).await;
    insert_location(&pool, tokyo_id, 35.6762, 139.6503).await;

    // Query from Paris with 50 km radius
    let resp = server
        .get("/locations/nearby")
        .add_query_param("lat", "48.8566")
        .add_query_param("lng", "2.3522")
        .add_query_param("radius_m", "50000")
        .await;

    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    let results = body.as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["username"], "loc_paris");

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_nearby_returns_empty_when_none_in_radius() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let id = insert_creator(&pool, "loc_far").await;
    insert_location(&pool, id, 35.6762, 139.6503).await; // Tokyo

    // Query from New York with 100 km radius
    let resp = server
        .get("/locations/nearby")
        .add_query_param("lat", "40.7128")
        .add_query_param("lng", "-74.0060")
        .add_query_param("radius_m", "100000")
        .await;

    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    assert_eq!(body.as_array().unwrap().len(), 0);

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_nearby_ordered_by_distance() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let close_id = insert_creator(&pool, "loc_close").await;
    let far_id = insert_creator(&pool, "loc_far2").await;
    // close: ~1 km from query point, far: ~5 km
    insert_location(&pool, close_id, 48.8566, 2.3522).await;
    insert_location(&pool, far_id, 48.8100, 2.3522).await;

    let resp = server
        .get("/locations/nearby")
        .add_query_param("lat", "48.8566")
        .add_query_param("lng", "2.3522")
        .add_query_param("radius_m", "20000")
        .await;

    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    let results = body.as_array().unwrap();
    assert!(results.len() >= 2);
    // First result should be closer
    let d0 = results[0]["distance_m"].as_f64().unwrap();
    let d1 = results[1]["distance_m"].as_f64().unwrap();
    assert!(d0 <= d1);

    common::cleanup_test_db(&pool).await;
}

// ── Geofence ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_geofence_returns_creators_inside() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let inside_id = insert_creator(&pool, "loc_inside").await;
    let outside_id = insert_creator(&pool, "loc_outside").await;
    insert_location(&pool, inside_id, 48.8566, 2.3522).await; // Paris
    insert_location(&pool, outside_id, 51.5074, -0.1278).await; // London

    let resp = server
        .get("/locations/geofence")
        .add_query_param("lat", "48.8566")
        .add_query_param("lng", "2.3522")
        .add_query_param("radius_m", "10000")
        .await;

    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    let results = body.as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["username"], "loc_inside");

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_geofence_invalid_radius() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let resp = server
        .get("/locations/geofence")
        .add_query_param("lat", "48.8566")
        .add_query_param("lng", "2.3522")
        .add_query_param("radius_m", "999999")
        .await;

    resp.assert_status(StatusCode::BAD_REQUEST);

    common::cleanup_test_db(&pool).await;
}

// ── Analytics ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_analytics_empty() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let resp = server.get("/locations/analytics").await;
    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    assert_eq!(body["total_located"], 0);
    assert!(body["bounding_box"].is_null());

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_analytics_with_data() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let id1 = insert_creator(&pool, "loc_ana1").await;
    let id2 = insert_creator(&pool, "loc_ana2").await;
    insert_location(&pool, id1, 48.8566, 2.3522).await;
    insert_location(&pool, id2, 35.6762, 139.6503).await;

    let resp = server.get("/locations/analytics").await;
    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    assert_eq!(body["total_located"], 2);
    let bb = &body["bounding_box"];
    assert!(!bb.is_null());
    assert!(bb["min_lat"].as_f64().unwrap() < bb["max_lat"].as_f64().unwrap());

    common::cleanup_test_db(&pool).await;
}
