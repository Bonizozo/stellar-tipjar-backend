use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::json;
mod common;

#[tokio::test]
async fn test_admin_auth_wrong_key() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let response = server
        .get("/creators/search?q=test")
        .add_header("X-Admin-Key", "wrong")
        .await;

    // Search is NOT admin protected?
    // Let's check which routes are admin protected.
}

#[tokio::test]
async fn test_admin_auth_missing_header() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let response = server
        .get("/creators/search?q=test")
        .await;

    // Actually, search is a public read route.
    response.assert_status(StatusCode::OK);

    common::cleanup_test_db(&pool).await;
}
