// tests/tips_integration.rs
// Integration tests for tip endpoints

use axum_test::TestServer;
use serde_json::json;
use stellar_tipjar_backend::tests::integration_test_setup::build_app;

#[tokio::test]
async fn test_create_tip_success() {
    let app = build_app().await;
    let server = TestServer::new(app);

    // Ensure creator exists
    let creator_payload = json!({
        "username": "tipcreator",
        "wallet": "GABCDEF1234567890TESTWALLET",
        "bio": "Creator for tip"
    });
    server.post("/api/v1/creators").json(&creator_payload).await;

    let tip_payload = json!({
        "creator_username": "tipcreator",
        "amount": "10",
        "tx_hash": "abcdef123456",
        "message": "Great work!"
    });
    let resp = server.post("/api/v1/tips").json(&tip_payload).await;
    resp.assert_status_success();
    let body = resp.json::<serde_json::Value>().await;
    assert_eq!(body["tx_hash"], "abcdef123456");
}

#[tokio::test]
async fn test_create_tip_duplicate_tx_hash() {
    let app = build_app().await;
    let server = TestServer::new(app);

    // Create creator
    let creator_payload = json!({
        "username": "dupcreator",
        "wallet": "GABCDEF1234567890TESTWALLET",
        "bio": "Dup creator"
    });
    server.post("/api/v1/creators").json(&creator_payload).await;

    let tip_payload = json!({
        "creator_username": "dupcreator",
        "amount": "5",
        "tx_hash": "duptxhash",
        "message": "First tip"
    });
    // First tip should succeed
    let resp1 = server.post("/api/v1/tips").json(&tip_payload).await;
    resp1.assert_status_success();
    // Second tip with same tx_hash should be rejected
    let resp2 = server.post("/api/v1/tips").json(&tip_payload).await;
    assert_eq!(resp2.status_code(), 400);
}

#[tokio::test]
async fn test_create_tip_creator_not_found() {
    let app = build_app().await;
    let server = TestServer::new(app);

    let tip_payload = json!({
        "creator_username": "nonexistent",
        "amount": "5",
        "tx_hash": "somehash",
        "message": "Tip"
    });
    let resp = server.post("/api/v1/tips").json(&tip_payload).await;
    assert_eq!(resp.status_code(), 404);
}

#[tokio::test]
async fn test_create_tip_moderation_rejection() {
    let app = build_app().await;
    let server = TestServer::new(app);

    // Create creator
    let creator_payload = json!({
        "username": "modcreator",
        "wallet": "GABCDEF1234567890TESTWALLET",
        "bio": "Mod test"
    });
    server.post("/api/v1/creators").json(&creator_payload).await;

    // Assume that a tip containing the word "spam" is rejected by moderation
    let tip_payload = json!({
        "creator_username": "modcreator",
        "amount": "1",
        "tx_hash": "modhash",
        "message": "spam content"
    });
    let resp = server.post("/api/v1/tips").json(&tip_payload).await;
    // Expect bad request due to moderation failure
    assert_eq!(resp.status_code(), 400);
}
