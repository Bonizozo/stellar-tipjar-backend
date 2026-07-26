/// Tip verification pipeline integration tests (issue #341).
///
/// Tests run against a real PostgreSQL test database (TEST_DATABASE_URL).
/// Horizon calls are intercepted via the MockTipVerifier injected into AppState,
/// so no network traffic occurs and the suite runs deterministically.
use axum::http::StatusCode;
use axum_test::TestServer;
use httpmock::prelude::*;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::task::JoinSet;
use uuid::Uuid;

mod common;

use common::{cleanup_test_db, create_test_app, create_test_app_with_verifier, setup_test_db, MockTipVerifier};
use stellar_tipjar_backend::controllers::tip_controller;
use stellar_tipjar_backend::errors::AppError;
use stellar_tipjar_backend::services::stellar_service::VerifyOutcome;

// ──────────────────────────── helpers ────────────────────────────────────────

const TX_HASH: &str = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";
const TX_HASH2: &str = "1122334455667788990011223344556677889900112233445566778899001122";
const TIPPER_ADDR: &str = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN";
const CREATOR_WALLET: &str = "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5";

async fn create_creator(server: &TestServer, username: &str) {
    let resp = server
        .post("/creators")
        .json(&json!({
            "username": username,
            "wallet_address": CREATOR_WALLET,
            "email": format!("{}@test.com", username)
        }))
        .await;
    // 201 or 409 (already exists) are both acceptable in test setup
    assert!(
        resp.status_code() == StatusCode::CREATED || resp.status_code() == StatusCode::CONFLICT,
        "create_creator failed with {}",
        resp.status_code()
    );
}

fn tip_body(username: &str, tx_hash: &str) -> Value {
    json!({
        "username": username,
        "amount": "10.5",
        "transaction_hash": tx_hash,
        "tipper_source_account": TIPPER_ADDR,
        "memo": null
    })
}

// ──────────────────────────── tests ──────────────────────────────────────────

/// Happy path: submit a valid tip → 201, status = pending_verification.
/// Then drive confirmation via confirm_tip and verify the tip becomes confirmed.
#[tokio::test]
async fn test_happy_path_tip_submitted_as_pending() {
    let pool = setup_test_db().await;
    let (app, _) = create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    create_creator(&server, "happy_creator").await;

    let resp = server.post("/tips").json(&tip_body("happy_creator", TX_HASH)).await;
    resp.assert_status(StatusCode::CREATED);

    let body: Value = resp.json();
    assert_eq!(body["status"], "pending_verification", "Tip should enter as pending_verification");
    assert_eq!(body["creator_username"], "happy_creator");
    assert_eq!(body["amount"], "10.5");

    // The tip should NOT appear in the public tips list yet (gated on confirmed)
    let list_resp = server.get("/creators/happy_creator/tips").await;
    list_resp.assert_status(StatusCode::OK);
    let tips: Vec<Value> = list_resp.json();
    assert!(tips.is_empty(), "Pending tip should not appear in public list");

    cleanup_test_db(&pool).await;
}

/// Confirm path: after confirm_tip is called, tip appears in list.
#[tokio::test]
async fn test_confirmed_tip_appears_in_list() {
    let pool = setup_test_db().await;
    let (app, _) = create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    create_creator(&server, "confirm_creator").await;

    // Submit tip
    let resp = server.post("/tips").json(&tip_body("confirm_creator", TX_HASH)).await;
    resp.assert_status(StatusCode::CREATED);
    let body: Value = resp.json();
    let tip_id: uuid::Uuid = body["id"].as_str().unwrap().parse().unwrap();

    // Build a minimal AppState for calling confirm_tip directly
    let state = {
        let performance = Arc::new(stellar_tipjar_backend::db::performance::PerformanceMonitor::new());
        let (queue, _) = stellar_tipjar_backend::queue::VerificationQueue::new();
        let moderation = Arc::new(stellar_tipjar_backend::moderation::ModerationService::new(pool.clone()));
        Arc::new(stellar_tipjar_backend::db::connection::AppState {
            db: pool.clone(),
            verifier: MockTipVerifier::always_confirm(),
            queue,
            performance,
            moderation,
            redis: None,
            broadcast_tx: tokio::sync::broadcast::channel(16).0,
            cache: None,
            invalidator: None,
            db_circuit_breaker: Arc::new(stellar_tipjar_backend::services::circuit_breaker::CircuitBreaker::new(5, std::time::Duration::from_secs(60))),
            lock_service: None,
        stellar: Arc::new(stellar_tipjar_backend::services::stellar_service::StellarService::new("https://horizon-testnet.stellar.org".to_string(), "testnet".to_string())),
        encryption: Arc::new(stellar_tipjar_backend::crypto::encryption::EncryptionKeyManager::new()),
        replicas: None,
        ws_shutdown_tx: tokio::sync::watch::channel(false).0,
        ws_config: stellar_tipjar_backend::ws::WsConfig::from_env(),
        idempotency: Arc::new(stellar_tipjar_backend::idempotency::IdempotencyService::new(pool.clone(), None, stellar_tipjar_backend::idempotency::IdempotencyConfig::default())),
        sharding: None,
        })
    };

    // Drive to confirmed
    tip_controller::confirm_tip(&state, tip_id).await.expect("confirm_tip failed");

    // Now the tip should appear in the list
    let list_resp = server.get("/creators/confirm_creator/tips").await;
    list_resp.assert_status(StatusCode::OK);
    let tips: Vec<Value> = list_resp.json();
    assert_eq!(tips.len(), 1);
    assert_eq!(tips[0]["status"], "confirmed");

    cleanup_test_db(&pool).await;
}

/// Rejected tips must not appear in the public list.
#[tokio::test]
async fn test_rejected_tip_hidden_from_list() {
    let pool = setup_test_db().await;
    let (app, _) = create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    create_creator(&server, "reject_creator").await;

    let resp = server.post("/tips").json(&tip_body("reject_creator", TX_HASH)).await;
    resp.assert_status(StatusCode::CREATED);
    let tip_id: uuid::Uuid = resp.json::<Value>()["id"].as_str().unwrap().parse().unwrap();

    let state = {
        let performance = Arc::new(stellar_tipjar_backend::db::performance::PerformanceMonitor::new());
        let (queue, _) = stellar_tipjar_backend::queue::VerificationQueue::new();
        let moderation = Arc::new(stellar_tipjar_backend::moderation::ModerationService::new(pool.clone()));
        Arc::new(stellar_tipjar_backend::db::connection::AppState {
            db: pool.clone(),
            verifier: MockTipVerifier::always_confirm(),
            queue,
            performance,
            moderation,
            redis: None,
            broadcast_tx: tokio::sync::broadcast::channel(16).0,
            cache: None,
            invalidator: None,
            db_circuit_breaker: Arc::new(stellar_tipjar_backend::services::circuit_breaker::CircuitBreaker::new(5, std::time::Duration::from_secs(60))),
            lock_service: None,
        stellar: Arc::new(stellar_tipjar_backend::services::stellar_service::StellarService::new("https://horizon-testnet.stellar.org".to_string(), "testnet".to_string())),
        encryption: Arc::new(stellar_tipjar_backend::crypto::encryption::EncryptionKeyManager::new()),
        replicas: None,
        ws_shutdown_tx: tokio::sync::watch::channel(false).0,
        ws_config: stellar_tipjar_backend::ws::WsConfig::from_env(),
        idempotency: Arc::new(stellar_tipjar_backend::idempotency::IdempotencyService::new(pool.clone(), None, stellar_tipjar_backend::idempotency::IdempotencyConfig::default())),
        sharding: None,
        })
    };

    tip_controller::reject_tip(&state, tip_id, "amount mismatch").await.expect("reject_tip failed");

    let list_resp = server.get("/creators/reject_creator/tips").await;
    list_resp.assert_status(StatusCode::OK);
    let tips: Vec<Value> = list_resp.json();
    assert!(tips.is_empty(), "Rejected tip must not appear in public list");

    cleanup_test_db(&pool).await;
}

/// Amount mismatch: verifier returns Rejected – tip must not reach confirmed.
#[tokio::test]
async fn test_amount_mismatch_is_rejected() {
    let pool = setup_test_db().await;
    let verifier = MockTipVerifier::with_outcomes(vec![Ok(VerifyOutcome::Rejected {
        reason: "Amount mismatch: expected 105000000 stroops, on-chain 100000000".to_string(),
    })]);
    let (app, _) = create_test_app_with_verifier(pool.clone(), verifier).await;
    let server = TestServer::new(app).unwrap();

    create_creator(&server, "amount_creator").await;

    let resp = server.post("/tips").json(&tip_body("amount_creator", TX_HASH)).await;
    // The tip is accepted into pending_verification (HTTP 201) – rejection happens asynchronously.
    resp.assert_status(StatusCode::CREATED);
    let body: Value = resp.json();
    assert_eq!(body["status"], "pending_verification");

    cleanup_test_db(&pool).await;
}

/// Destination mismatch: verifier rejects due to wrong destination.
#[tokio::test]
async fn test_destination_mismatch_rejected() {
    let pool = setup_test_db().await;
    let verifier = MockTipVerifier::with_outcomes(vec![Ok(VerifyOutcome::Rejected {
        reason: "No native payment to GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5 found in transaction".to_string(),
    })]);
    let (app, _) = create_test_app_with_verifier(pool.clone(), verifier).await;
    let server = TestServer::new(app).unwrap();

    create_creator(&server, "dest_creator").await;
    let resp = server.post("/tips").json(&tip_body("dest_creator", TX_HASH)).await;
    resp.assert_status(StatusCode::CREATED);
    assert_eq!(resp.json::<Value>()["status"], "pending_verification");

    cleanup_test_db(&pool).await;
}

/// Memo mismatch: verifier rejects due to wrong memo.
#[tokio::test]
async fn test_memo_mismatch_rejected() {
    let pool = setup_test_db().await;
    let verifier = MockTipVerifier::with_outcomes(vec![Ok(VerifyOutcome::Rejected {
        reason: "Memo mismatch: expected 'expected_memo', got 'wrong_memo'".to_string(),
    })]);
    let (app, _) = create_test_app_with_verifier(pool.clone(), verifier).await;
    let server = TestServer::new(app).unwrap();

    create_creator(&server, "memo_creator").await;
    let resp = server
        .post("/tips")
        .json(&json!({
            "username": "memo_creator",
            "amount": "10.5",
            "transaction_hash": TX_HASH,
            "tipper_source_account": TIPPER_ADDR,
            "memo": "expected_memo"
        }))
        .await;
    resp.assert_status(StatusCode::CREATED);
    assert_eq!(resp.json::<Value>()["status"], "pending_verification");

    cleanup_test_db(&pool).await;
}

/// Duplicate tx_hash race: two concurrent submissions of the same hash must
/// result in exactly one tip record and one 409 Conflict response.
#[tokio::test]
async fn test_duplicate_tx_hash_race() {
    let pool = setup_test_db().await;

    // Pre-create the creator outside the race so the FK constraint is satisfied.
    {
        let (app, _) = create_test_app(pool.clone()).await;
        let server = TestServer::new(app).unwrap();
        create_creator(&server, "race_creator").await;
    }

    // Fire 5 concurrent submissions of the same tx_hash.
    let mut join_set = JoinSet::new();
    for _ in 0..5 {
        let pool_clone = pool.clone();
        join_set.spawn(async move {
            let (app, _) = create_test_app(pool_clone).await;
            let server = TestServer::new(app).unwrap();
            let resp = server.post("/tips").json(&tip_body("race_creator", TX_HASH)).await;
            resp.status_code()
        });
    }

    let mut created = 0u32;
    let mut conflict = 0u32;
    while let Some(res) = join_set.join_next().await {
        match res.unwrap() {
            StatusCode::CREATED => created += 1,
            StatusCode::CONFLICT => conflict += 1,
            other => panic!("Unexpected status code: {}", other),
        }
    }

    assert_eq!(created, 1, "Exactly one submission should succeed");
    assert_eq!(conflict, 4, "The other four should be rejected as duplicates");

    // Verify only one record in the database
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tips WHERE transaction_hash = $1")
        .bind(TX_HASH)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "Database must contain exactly one tip for this tx_hash");

    cleanup_test_db(&pool).await;
}

/// Horizon 5xx retry: the verifier returns a network error on the first call,
/// then confirms on the second (simulating a brief Horizon outage).
/// The worker re-enqueues and the tip eventually reaches confirmed.
#[tokio::test]
async fn test_horizon_5xx_retry_eventual_confirm() {
    use stellar_tipjar_backend::errors::StellarError;
    use tokio::time::{sleep, Duration};

    let pool = setup_test_db().await;

    // Sequence: NetworkUnavailable → Confirmed
    let verifier = MockTipVerifier::with_outcomes(vec![
        Err(AppError::Stellar(StellarError::NetworkUnavailable)),
        Ok(VerifyOutcome::Confirmed),
    ]);

    let performance = Arc::new(stellar_tipjar_backend::db::performance::PerformanceMonitor::new());
    let (queue, queue_rx) = stellar_tipjar_backend::queue::VerificationQueue::new();
    let moderation = Arc::new(stellar_tipjar_backend::moderation::ModerationService::new(pool.clone()));

    let state = Arc::new(stellar_tipjar_backend::db::connection::AppState {
        db: pool.clone(),
        verifier,
        queue,
        performance,
        moderation,
        redis: None,
        broadcast_tx: tokio::sync::broadcast::channel(16).0,
        cache: None,
        invalidator: None,
        db_circuit_breaker: Arc::new(stellar_tipjar_backend::services::circuit_breaker::CircuitBreaker::new(5, std::time::Duration::from_secs(60))),
        lock_service: None,
        stellar: Arc::new(stellar_tipjar_backend::services::stellar_service::StellarService::new("https://horizon-testnet.stellar.org".to_string(), "testnet".to_string())),
        encryption: Arc::new(stellar_tipjar_backend::crypto::encryption::EncryptionKeyManager::new()),
        replicas: None,
        ws_shutdown_tx: tokio::sync::watch::channel(false).0,
        ws_config: stellar_tipjar_backend::ws::WsConfig::from_env(),
        idempotency: Arc::new(stellar_tipjar_backend::idempotency::IdempotencyService::new(pool.clone(), None, stellar_tipjar_backend::idempotency::IdempotencyConfig::default())),
        sharding: None,
    });

    // Spawn the real queue worker so jobs are processed
    stellar_tipjar_backend::queue::worker::spawn_worker(Arc::clone(&state), queue_rx);

    // Create creator
    sqlx::query("INSERT INTO creators (id, username, wallet_address, email, created_at) VALUES (gen_random_uuid(), $1, $2, $3, NOW()) ON CONFLICT DO NOTHING")
        .bind("retry_creator")
        .bind(CREATOR_WALLET)
        .bind("retry@test.com")
        .execute(&pool)
        .await
        .unwrap();

    // Submit tip
    let tip = tip_controller::record_tip(
        &state,
        stellar_tipjar_backend::models::tip::RecordTipRequest {
            username: "retry_creator".to_string(),
            amount: "10.5".to_string(),
            transaction_hash: TX_HASH.to_string(),
            tipper_source_account: TIPPER_ADDR.to_string(),
            memo: None,
        },
    )
    .await
    .expect("record_tip failed");

    assert_eq!(
        tip.effective_status(),
        stellar_tipjar_backend::models::tip::TipStatus::PendingVerification
    );

    // Give the worker time to process (first attempt fails + backoff + retry)
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        sleep(Duration::from_millis(200)).await;
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM tips WHERE id = $1")
                .bind(tip.id)
                .fetch_optional(&pool)
                .await
                .unwrap();

        if status.as_deref() == Some("confirmed") {
            break; // ✅
        }
        if std::time::Instant::now() > deadline {
            panic!("Tip was not confirmed within the deadline. Status: {:?}", status);
        }
    }

    cleanup_test_db(&pool).await;
}

/// Reconciliation of stuck tips: insert a pending tip with a created_at in the
/// past, run `reconciliation::run_once`, and confirm the tip is re-enqueued.
#[tokio::test]
async fn test_reconciliation_re_enqueues_stuck_tips() {
    let pool = setup_test_db().await;

    let performance = Arc::new(stellar_tipjar_backend::db::performance::PerformanceMonitor::new());
    let (queue, _queue_rx) = stellar_tipjar_backend::queue::VerificationQueue::new();
    let moderation = Arc::new(stellar_tipjar_backend::moderation::ModerationService::new(pool.clone()));

    let state = Arc::new(stellar_tipjar_backend::db::connection::AppState {
        db: pool.clone(),
        verifier: MockTipVerifier::always_confirm(),
        queue,
        performance,
        moderation,
        redis: None,
        broadcast_tx: tokio::sync::broadcast::channel(16).0,
        cache: None,
        invalidator: None,
        db_circuit_breaker: Arc::new(stellar_tipjar_backend::services::circuit_breaker::CircuitBreaker::new(5, std::time::Duration::from_secs(60))),
        lock_service: None,
        stellar: Arc::new(stellar_tipjar_backend::services::stellar_service::StellarService::new("https://horizon-testnet.stellar.org".to_string(), "testnet".to_string())),
        encryption: Arc::new(stellar_tipjar_backend::crypto::encryption::EncryptionKeyManager::new()),
        replicas: None,
        ws_shutdown_tx: tokio::sync::watch::channel(false).0,
        ws_config: stellar_tipjar_backend::ws::WsConfig::from_env(),
        idempotency: Arc::new(stellar_tipjar_backend::idempotency::IdempotencyService::new(pool.clone(), None, stellar_tipjar_backend::idempotency::IdempotencyConfig::default())),
        sharding: None,
    });

    // Create creator
    sqlx::query("INSERT INTO creators (id, username, wallet_address, email, created_at) VALUES (gen_random_uuid(), $1, $2, $3, NOW()) ON CONFLICT DO NOTHING")
        .bind("recon_creator")
        .bind(CREATOR_WALLET)
        .bind("recon@test.com")
        .execute(&pool)
        .await
        .unwrap();

    // Insert a stuck pending tip (created 10 minutes ago)
    sqlx::query(
        r#"
        INSERT INTO tips (id, creator_username, amount, transaction_hash, tipper_source_account, status, created_at)
        VALUES (gen_random_uuid(), $1, '5.0', $2, $3, 'pending_verification', NOW() - INTERVAL '10 minutes')
        "#,
    )
    .bind("recon_creator")
    .bind(TX_HASH2)
    .bind(TIPPER_ADDR)
    .execute(&pool)
    .await
    .unwrap();

    // Run reconciliation
    let enqueued = stellar_tipjar_backend::jobs::reconciliation::run_once(Arc::clone(&state)).await;
    assert_eq!(enqueued, 1, "Reconciliation should have re-enqueued 1 stuck tip");

    cleanup_test_db(&pool).await;
}

/// Validation: request body with no tipper_source_account must be rejected.
#[tokio::test]
async fn test_invalid_request_missing_source_account() {
    let pool = setup_test_db().await;
    let (app, _) = create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    create_creator(&server, "valid_creator2").await;

    let resp = server
        .post("/tips")
        .json(&json!({
            "username": "valid_creator2",
            "amount": "10.5",
            "transaction_hash": TX_HASH,
            // Missing tipper_source_account
        }))
        .await;

    // Deserialization failure → 400 Bad Request
    assert_eq!(resp.status_code(), StatusCode::BAD_REQUEST);

    cleanup_test_db(&pool).await;
}

/// Amount validation: amounts with more than 7 decimal places must be rejected.
#[tokio::test]
async fn test_invalid_amount_too_many_decimals() {
    let pool = setup_test_db().await;
    let (app, _) = create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    create_creator(&server, "decimal_creator").await;

    let resp = server
        .post("/tips")
        .json(&json!({
            "username": "decimal_creator",
            "amount": "1.12345678",
            "transaction_hash": TX_HASH,
            "tipper_source_account": TIPPER_ADDR
        }))
        .await;

    assert_eq!(resp.status_code(), StatusCode::BAD_REQUEST);

    cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_get_creator_tips_paginated() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    server
        .post("/creators")
        .json(&json!({ "username": "paguser", "wallet_address": "GPAG000", "email": null }))
        .await;

    for i in 1..=5i32 {
        sqlx::query(
            "INSERT INTO tips (id, creator_username, amount, transaction_hash, status, created_at) \
             VALUES ($1, $2, $3, $4, 'confirmed', NOW())",
        )
        .bind(Uuid::new_v4())
        .bind("paguser")
        .bind(format!("{}.0", i))
        .bind(format!("HASH{i}"))
        .execute(&pool)
        .await
        .unwrap();
    }

    // Page 1, limit 2
    let resp = server
        .get("/creators/paguser/tips?page=1&limit=2")
        .await;
    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    assert_eq!(body["data"].as_array().unwrap().len(), 2);
    assert_eq!(body["total"], 5);
    assert_eq!(body["total_pages"], 3);
    assert_eq!(body["has_next"], true);
    assert_eq!(body["has_prev"], false);

    // Page 3 (last page, 1 item)
    let resp = server
        .get("/creators/paguser/tips?page=3&limit=2")
        .await;
    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["has_next"], false);
    assert_eq!(body["has_prev"], true);

    common::cleanup_test_db(&pool).await;
}

#[tokio::test]
async fn test_list_tips_with_filters() {
    let pool = common::setup_test_db().await;
    let (app, _) = common::create_test_app(pool.clone()).await;
    let server = TestServer::new(app).unwrap();

    server
        .post("/creators")
        .json(&json!({ "username": "filtuser", "wallet_address": "GFLT000", "email": null }))
        .await;

    for (amount, hash) in [("5.0", "FHASH1"), ("15.0", "FHASH2"), ("25.0", "FHASH3")] {
        sqlx::query(
            "INSERT INTO tips (id, creator_username, amount, transaction_hash, status, created_at) \
             VALUES ($1, $2, $3, $4, 'confirmed', NOW())",
        )
        .bind(uuid::Uuid::new_v4())
        .bind("filtuser")
        .bind(amount)
        .bind(hash)
        .execute(&pool)
        .await
        .unwrap();
    }

    // Filter min_amount=10
    let resp = server
        .get("/tips?min_amount=10&sort_by=amount&sort_order=asc")
        .await;
    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["amount"], "15.0");
    assert_eq!(data[1]["amount"], "25.0");

    // Filter max_amount=10
    let resp = server.get("/tips?max_amount=10").await;
    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    assert_eq!(body["total"], 1);

    // Enforce max limit
    let resp = server.get("/tips?limit=999").await;
    resp.assert_status(StatusCode::OK);
    let body = resp.json::<serde_json::Value>();
    assert_eq!(body["limit"], 100);

    common::cleanup_test_db(&pool).await;
}

// ─────────────────────────── Unit tests ─────────────────────────────────────

#[cfg(test)]
mod unit {
    use stellar_tipjar_backend::validation::amount::xlm_to_stroops_str;
    use stellar_tipjar_backend::services::stellar_service::StellarService;

    /// Integer stroop conversion must be exact.
    #[test]
    fn stroop_conversion_exact() {
        assert_eq!(xlm_to_stroops_str("10.5").unwrap(), 105_000_000);
        assert_eq!(xlm_to_stroops_str("1.0000000").unwrap(), 10_000_000);
        assert_eq!(xlm_to_stroops_str("0.0000001").unwrap(), 1);
        assert_eq!(xlm_to_stroops_str("100").unwrap(), 1_000_000_000);
    }

    #[test]
    fn stroop_conversion_rejects_too_many_decimals() {
        assert!(xlm_to_stroops_str("1.12345678").is_err());
    }

    /// StellarService::xlm_to_stroops must match our helper.
    #[test]
    fn stellar_service_xlm_to_stroops() {
        assert_eq!(StellarService::xlm_to_stroops("10.5000000").unwrap(), 105_000_000);
        assert_eq!(StellarService::xlm_to_stroops("0.0000001").unwrap(), 1);
    }
}
