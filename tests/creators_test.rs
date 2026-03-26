use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::json;
mod common;

#[tokio::test]
async fn test_create_creator() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let response = server
        .post("/creators")
        .json(&json!({
            "username": "testuser",
            "wallet_address": "GABC123",
            "email": "test@example.com"
        }))
        .await;

    response.assert_status(StatusCode::CREATED);
    
    let body = response.json::<serde_json::Value>();
    assert_eq!(body["username"], "testuser");
    assert_eq!(body["email"], "test@example.com");

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_get_creator() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    // First create
    server.post("/creators").json(&json!({
        "username": "fetchme",
        "wallet_address": "GDEF456",
        "email": "fetch@example.com"
    })).await;

    // Then get
    let response = server.get("/creators/fetchme").await;
    response.assert_status(StatusCode::OK);
    
    let body = response.json::<serde_json::Value>();
    assert_eq!(body["username"], "fetchme");

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_creator_not_found() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let response = server.get("/creators/nobody").await;
    response.assert_status(StatusCode::NOT_FOUND);

    common::cleanup_test_db(&pool).await;
}
