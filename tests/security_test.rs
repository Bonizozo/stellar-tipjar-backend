use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::json;
mod common;

/// All SQL injection payloads must be rejected at the validation layer (400)
/// or return no data (404) — never a 500 or unexpected data leak.
#[tokio::test]
async fn test_sql_injection_in_username_path() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let payloads = [
        "' OR '1'='1",
        "'; DROP TABLE creators; --",
        "1' UNION SELECT * FROM creators--",
        "admin'--",
    ];

    for payload in payloads {
        let response = server.get(&format!("/creators/{}", payload)).await;
        // Must not be a server error — injection was either rejected or returned not-found
        assert_ne!(
            response.status_code(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "Payload caused 500: {payload}"
        );
        assert_ne!(
            response.status_code(),
            StatusCode::OK,
            "Payload unexpectedly matched a creator: {payload}"
        );
    }

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_sql_injection_in_create_creator_body() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let payloads = [
        "'; DROP TABLE creators; --",
        "admin' OR '1'='1",
        "x' UNION SELECT id,username,wallet_address,email,created_at FROM creators--",
    ];

    for payload in payloads {
        let response = server
            .post("/creators")
            .json(&json!({
                "username": payload,
                "wallet_address": "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN"
            }))
            .await;

        // Validation layer must reject these with 422 (ValidatedJson)
        assert_eq!(
            response.status_code(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Expected 422 for injection payload: {payload}"
        );
    }

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_sql_injection_in_record_tip_body() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    let payloads = [
        "'; DROP TABLE tips; --",
        "admin' OR '1'='1",
    ];

    for payload in payloads {
        let response = server
            .post("/tips")
            .json(&json!({
                "username": payload,
                "amount": "10.0",
                "transaction_hash": "a".repeat(64)
            }))
            .await;

        assert_eq!(
            response.status_code(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Expected 422 for injection payload: {payload}"
        );
    }

    common::cleanup_test_db(&pool).await;
}
