use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::json;
mod common;

// ── Creator validation ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_username_too_short_rejected() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let resp = server
        .post("/creators")
        .json(&json!({ "username": "ab", "wallet_address": "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN" }))
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);
    let body = resp.json::<serde_json::Value>();
    assert_eq!(body["code"], "VALIDATION_ERROR");

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_username_invalid_chars_rejected() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let resp = server
        .post("/creators")
        .json(&json!({ "username": "bad user!", "wallet_address": "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN" }))
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_invalid_stellar_address_rejected() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    // Wrong prefix
    let resp = server
        .post("/creators")
        .json(&json!({ "username": "validuser", "wallet_address": "XAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN" }))
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);

    // Too short
    let resp = server
        .post("/creators")
        .json(&json!({ "username": "validuser", "wallet_address": "GSHORT" }))
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_invalid_email_rejected() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let resp = server
        .post("/creators")
        .json(&json!({
            "username": "validuser",
            "wallet_address": "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN",
            "email": "not-an-email"
        }))
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);
    let body = resp.json::<serde_json::Value>();
    assert_eq!(body["code"], "VALIDATION_ERROR");

    common::cleanup_test_db(&pool).await;
}

// ── Tip validation ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_invalid_tx_hash_rejected() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    // Not 64 chars
    let resp = server
        .post("/tips")
        .json(&json!({ "username": "alice", "amount": "1.0", "transaction_hash": "abc123" }))
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);

    // 64 chars but non-hex
    let resp = server
        .post("/tips")
        .json(&json!({
            "username": "alice",
            "amount": "1.0",
            "transaction_hash": "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"
        }))
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_invalid_amount_rejected() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let valid_hash = "a".repeat(64);

    // Negative amount
    let resp = server
        .post("/tips")
        .json(&json!({ "username": "alice", "amount": "-1.0", "transaction_hash": valid_hash }))
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);

    // Too many decimal places
    let resp = server
        .post("/tips")
        .json(&json!({ "username": "alice", "amount": "1.12345678", "transaction_hash": valid_hash }))
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);

    // Non-numeric
    let resp = server
        .post("/tips")
        .json(&json!({ "username": "alice", "amount": "abc", "transaction_hash": valid_hash }))
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_tip_message_too_long_rejected() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let resp = server
        .post("/tips")
        .json(&json!({
            "username": "alice",
            "amount": "1.0",
            "transaction_hash": "a".repeat(64),
            "message": "x".repeat(281)
        }))
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_malformed_json_rejected() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let resp = server
        .post("/creators")
        .content_type("application/json")
        .bytes(b"{not valid json}".as_ref().into())
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);
    let body = resp.json::<serde_json::Value>();
    assert_eq!(body["code"], "INVALID_JSON");

    common::cleanup_test_db(&pool).await;
}
