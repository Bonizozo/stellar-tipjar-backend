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
    server
        .post("/creators")
        .json(&json!({
            "username": "fetchme",
            "wallet_address": "GDEF456",
            "email": "fetch@example.com"
        }))
        .await;

    // Then get
    let response = server.get("/creators/fetchme").await;
    response.assert_status(StatusCode::OK);

    let body = response.json::<serde_json::Value>();
    assert_eq!(body["username"], "fetchme");

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_create_creator_duplicate_email() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    // First creator with email
    let response = server
        .post("/creators")
        .json(&json!({
            "username": "user_one",
            "wallet_address": "GBCKL5SJHHGOU6JJVXIFVBV2VH6I5QMZ4Y2X6M27W6C3MJQZYMGMKGDN",
            "email": "dup@example.com"
        }))
        .await;
    response.assert_status(StatusCode::CREATED);

    // Second creator with same email
    let response = server
        .post("/creators")
        .json(&json!({
            "username": "user_two",
            "wallet_address": "GDZ5M7Y7V3ZKSJJXF7KCZ5Z5VZ3Y7V3ZKSJJXF7KCZ5Z5VZ3Y7V3ZKSA",
            "email": "dup@example.com"
        }))
        .await;

    response.assert_status(StatusCode::CONFLICT);

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_create_creator_duplicate_email_without_email_ok() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    // First creator without email
    let response = server
        .post("/creators")
        .json(&json!({
            "username": "no_email_user",
            "wallet_address": "GAX3B5XK7J3G6JJVXIFVBV2VH6I5QMZ4Y2X6M27W6C3MJQZYMGMKGDN",
        }))
        .await;
    response.assert_status(StatusCode::CREATED);

    // Second creator also without email should succeed (NULLs are distinct)
    let response = server
        .post("/creators")
        .json(&json!({
            "username": "no_email_user2",
            "wallet_address": "GBY4C6YL8K4H7KKWYJGWCW3WIX7J6PZ5Z3X7N38X7D4NKQZWMGMKGDN",
        }))
        .await;
    response.assert_status(StatusCode::CREATED);

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
