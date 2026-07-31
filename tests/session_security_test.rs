// tests/session_security_test.rs — integration coverage for #345:
// refresh-token rotation with reuse detection, access-token revocation via
// logout/password-change, and session listing/revocation.
//
// Requires a reachable Postgres (TEST_DATABASE_URL/DATABASE_URL) *and* Redis
// (REDIS_URL) — revocation and reuse-detection can't be verified without
// both. See tests/common/mod.rs::create_test_app_with_redis.

use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::json;
use uuid::Uuid;

mod common;

const VALID_WALLET: &str = "GBCKL5SJHHGOU6JJVXIFVBV2VH6I5QMZ4Y2X6M27W6C3MJQZYMGMKGDN";

fn unique_username(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..10])
}

async fn register_and_login(
    server: &TestServer,
    username: &str,
    password: &str,
) -> (String, String) {
    let register = server
        .post("/auth/register")
        .json(&json!({
            "username": username,
            "wallet_address": VALID_WALLET,
            "password": password,
        }))
        .await;
    register.assert_status(StatusCode::CREATED);
    let body: serde_json::Value = register.json();
    (
        body["access_token"].as_str().unwrap().to_string(),
        body["refresh_token"].as_str().unwrap().to_string(),
    )
}

#[tokio::test]
async fn refresh_rotation_happy_path_issues_a_new_pair_each_time() {
    let pool = common::setup_test_db().await;
    let (app, _redis) =
        common::create_test_app_with_redis(pool.clone(), &common::test_redis_url()).await;
    let server = common::test_server(app);

    let username = unique_username("rot");
    let (_access, refresh) =
        register_and_login(&server, &username, "correct horse battery staple").await;

    let resp = server
        .post("/auth/refresh")
        .json(&json!({ "refresh_token": refresh }))
        .await;
    resp.assert_status(StatusCode::OK);
    let body: serde_json::Value = resp.json();
    let new_refresh = body["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(
        new_refresh, refresh,
        "rotation must issue a fresh refresh token"
    );

    // The rotated token keeps working for the next rotation.
    let resp2 = server
        .post("/auth/refresh")
        .json(&json!({ "refresh_token": new_refresh }))
        .await;
    resp2.assert_status(StatusCode::OK);

    common::cleanup_test_db(&pool).await;
}

/// The critical test: replaying a refresh token that was already rotated
/// away must be detected as theft and kill the whole family — including the
/// legitimate, currently-valid token.
#[tokio::test]
async fn reuse_of_rotated_refresh_token_is_detected_and_revokes_the_family() {
    let pool = common::setup_test_db().await;
    let (app, _redis) =
        common::create_test_app_with_redis(pool.clone(), &common::test_redis_url()).await;
    let server = common::test_server(app);

    let username = unique_username("reuse");
    let (_access, original_refresh) =
        register_and_login(&server, &username, "correct horse battery staple").await;

    // Legitimate client rotates.
    let rotated = server
        .post("/auth/refresh")
        .json(&json!({ "refresh_token": original_refresh }))
        .await;
    rotated.assert_status(StatusCode::OK);
    let rotated_body: serde_json::Value = rotated.json();
    let current_refresh = rotated_body["refresh_token"].as_str().unwrap().to_string();

    // Attacker replays the stolen (already-rotated-away) original token.
    let replay = server
        .post("/auth/refresh")
        .json(&json!({ "refresh_token": original_refresh }))
        .await;
    replay.assert_status(StatusCode::UNAUTHORIZED);

    // The family is now revoked entirely: even the legitimate, currently
    // rotated token is rejected — forcing full re-authentication.
    let legit_after_reuse = server
        .post("/auth/refresh")
        .json(&json!({ "refresh_token": current_refresh }))
        .await;
    legit_after_reuse.assert_status(StatusCode::UNAUTHORIZED);

    common::cleanup_test_db(&pool).await;
}

/// Access-token revocation: logout denylists the current access token, and
/// the very next request with it is rejected (single Redis round trip).
#[tokio::test]
async fn logout_revokes_the_access_token_immediately() {
    let pool = common::setup_test_db().await;
    let (app, _redis) =
        common::create_test_app_with_redis(pool.clone(), &common::test_redis_url()).await;
    let server = common::test_server(app);

    let username = unique_username("logout");
    let (access, refresh) =
        register_and_login(&server, &username, "correct horse battery staple").await;

    // Token works against a protected endpoint before logout.
    let before = server
        .get("/auth/sessions")
        .add_header("Authorization", format!("Bearer {access}"))
        .await;
    before.assert_status(StatusCode::OK);

    let logout = server
        .post("/auth/logout")
        .add_header("Authorization", format!("Bearer {access}"))
        .json(&json!({ "refresh_token": refresh }))
        .await;
    logout.assert_status(StatusCode::NO_CONTENT);

    // Same access token, immediately rejected.
    let after = server
        .get("/auth/sessions")
        .add_header("Authorization", format!("Bearer {access}"))
        .await;
    after.assert_status(StatusCode::UNAUTHORIZED);

    common::cleanup_test_db(&pool).await;
}

/// Password change invalidates every outstanding access token via the
/// token-version epoch, and revokes refresh-token families too.
#[tokio::test]
async fn password_change_invalidates_outstanding_access_tokens() {
    let pool = common::setup_test_db().await;
    let (app, _redis) =
        common::create_test_app_with_redis(pool.clone(), &common::test_redis_url()).await;
    let server = common::test_server(app);

    let username = unique_username("pwchange");
    let (access, refresh) =
        register_and_login(&server, &username, "correct horse battery staple").await;

    let change = server
        .post("/auth/password/change")
        .add_header("Authorization", format!("Bearer {access}"))
        .json(&json!({
            "old_password": "correct horse battery staple",
            "new_password": "a totally different passphrase",
        }))
        .await;
    change.assert_status(StatusCode::NO_CONTENT);

    // The access token issued before the password change is now rejected.
    let stale = server
        .get("/auth/sessions")
        .add_header("Authorization", format!("Bearer {access}"))
        .await;
    stale.assert_status(StatusCode::UNAUTHORIZED);

    // Its refresh-token family was revoked too.
    let refresh_after = server
        .post("/auth/refresh")
        .json(&json!({ "refresh_token": refresh }))
        .await;
    refresh_after.assert_status(StatusCode::UNAUTHORIZED);

    common::cleanup_test_db(&pool).await;
}

/// Session listing and single-session revocation.
#[tokio::test]
async fn sessions_can_be_listed_and_individually_revoked() {
    let pool = common::setup_test_db().await;
    let (app, _redis) =
        common::create_test_app_with_redis(pool.clone(), &common::test_redis_url()).await;
    let server = common::test_server(app);

    let username = unique_username("sess");
    let (access, _refresh) =
        register_and_login(&server, &username, "correct horse battery staple").await;

    let list_before = server
        .get("/auth/sessions")
        .add_header("Authorization", format!("Bearer {access}"))
        .await;
    list_before.assert_status(StatusCode::OK);
    let list_before_body: serde_json::Value = list_before.json();
    let first_family_id = list_before_body["sessions"][0]["family_id"]
        .as_str()
        .unwrap()
        .to_string();

    // A second login creates a second session/family for the same user.
    let login2 = server
        .post("/auth/login")
        .json(&json!({ "username": username, "password": "correct horse battery staple" }))
        .await;
    login2.assert_status(StatusCode::OK);
    let login2_body: serde_json::Value = login2.json();
    let second_refresh = login2_body["refresh_token"].as_str().unwrap().to_string();

    let list = server
        .get("/auth/sessions")
        .add_header("Authorization", format!("Bearer {access}"))
        .await;
    list.assert_status(StatusCode::OK);
    let list_body: serde_json::Value = list.json();
    let sessions = list_body["sessions"].as_array().unwrap();
    assert_eq!(
        sessions.len(),
        2,
        "expected two active sessions after two logins"
    );

    // Whichever family isn't the one created by the very first login is the
    // one belonging to `second_refresh`.
    let second_family_id = sessions
        .iter()
        .map(|s| s["family_id"].as_str().unwrap().to_string())
        .find(|id| *id != first_family_id)
        .expect("second login must have created a distinct family");

    let revoke = server
        .delete(&format!("/auth/sessions/{second_family_id}"))
        .add_header("Authorization", format!("Bearer {access}"))
        .await;
    revoke.assert_status(StatusCode::NO_CONTENT);

    let list_after = server
        .get("/auth/sessions")
        .add_header("Authorization", format!("Bearer {access}"))
        .await;
    let list_after_body: serde_json::Value = list_after.json();
    assert_eq!(list_after_body["sessions"].as_array().unwrap().len(), 1);

    // The revoked family's refresh token no longer works.
    let refresh_revoked = server
        .post("/auth/refresh")
        .json(&json!({ "refresh_token": second_refresh }))
        .await;
    refresh_revoked.assert_status(StatusCode::UNAUTHORIZED);

    common::cleanup_test_db(&pool).await;
}
