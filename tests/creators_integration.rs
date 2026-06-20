// tests/creators_integration.rs
// Integration tests for creator endpoints

use axum_test::TestServer;
use serde_json::json;
use stellar_tipjar_backend::tests::integration_test_setup::build_app;

#[tokio::test]
async fn test_create_creator_success() {
    let app = build_app().await;
    let server = TestServer::new(app);

    let payload = json!({
        "username": "testcreator",
        "wallet": "GABCDEF1234567890TESTWALLET",
        "bio": "Test bio"
    });

    let resp = server.post("/api/v1/creators").json(&payload).await;
    resp.assert_status_success();
    let body = resp.json::<serde_json::Value>().await;
    assert_eq!(body["username"], "testcreator");
}

#[tokio::test]
async fn test_create_creator_duplicate_username() {
    let app = build_app().await;
    let server = TestServer::new(app);

    let payload = json!({
        "username": "dupuser",
        "wallet": "GABCDEF1234567890TESTWALLET",
        "bio": "First"
    });
    // First creation should succeed
    let resp1 = server.post("/api/v1/creators").json(&payload).await;
    resp1.assert_status_success();
    // Second attempt with same username
    let resp2 = server.post("/api/v1/creators").json(&payload).await;
    assert_eq!(resp2.status_code(), 400);
}

#[tokio::test]
async fn test_create_creator_invalid_wallet() {
    let app = build_app().await;
    let server = TestServer::new(app);

    let payload = json!({
        "username": "badwallet",
        "wallet": "invalid_wallet",
        "bio": "Bad wallet"
    });
    let resp = server.post("/api/v1/creators").json(&payload).await;
    assert_eq!(resp.status_code(), 400);
}

#[tokio::test]
async fn test_get_creator_found() {
    let app = build_app().await;
    let server = TestServer::new(app);

    // Create a creator first
    let payload = json!({
        "username": "fetchuser",
        "wallet": "GABCDEF1234567890TESTWALLET",
        "bio": "Fetch test"
    });
    let _ = server.post("/api/v1/creators").json(&payload).await;

    let resp = server.get("/api/v1/creators/fetchuser").await;
    resp.assert_status_success();
    let body = resp.json::<serde_json::Value>().await;
    assert_eq!(body["username"], "fetchuser");
}

#[tokio::test]
async fn test_get_creator_not_found() {
    let app = build_app().await;
    let server = TestServer::new(app);
    let resp = server.get("/api/v1/creators/nonexistent").await;
    assert_eq!(resp.status_code(), 404);
}
