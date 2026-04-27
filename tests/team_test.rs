mod common;

use bcrypt::hash;
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use stellar_tipjar_backend::controllers::{team_controller, tip_controller};
use stellar_tipjar_backend::db::connection::AppState;
use stellar_tipjar_backend::db::performance::PerformanceMonitor;
use stellar_tipjar_backend::moderation::ModerationService;
use stellar_tipjar_backend::models::team::{CreateTeamRequest, TeamMemberRequest};
use stellar_tipjar_backend::models::tip::RecordTipRequest;
use stellar_tipjar_backend::services::stellar_service::StellarService;

fn make_state(pool: PgPool) -> Arc<AppState> {
    let stellar = StellarService::new("https://horizon-testnet.stellar.org".to_string(), "testnet".to_string());
    let performance = Arc::new(PerformanceMonitor::new());
    let moderation = Arc::new(ModerationService::new(pool.clone()));
    Arc::new(AppState {
        db: pool,
        stellar,
        performance,
        moderation,
        redis: None,
        broadcast_tx: tokio::sync::broadcast::channel(16).0,
        cache: None,
        invalidator: None,
        db_circuit_breaker: Arc::new(stellar_tipjar_backend::services::circuit_breaker::CircuitBreaker::new(5, std::time::Duration::from_secs(60))),
        lock_service: None,
    })
}

#[tokio::test]
async fn test_team_tip_split_history_and_recording() {
    let pool = common::setup_test_db().await;
    let state = make_state(pool.clone());

    let owner_username = format!("owner{}", &Uuid::new_v4().to_simple().to_string()[..16]);
    let member_username = format!("member{}", &Uuid::new_v4().to_simple().to_string()[..16]);
    let team_name = format!("DreamTeam-{}", &Uuid::new_v4().to_simple().to_string()[..16]);

    let owner_hash = hash("password123", bcrypt::DEFAULT_COST).unwrap();
    let member_hash = hash("password123", bcrypt::DEFAULT_COST).unwrap();

    let owner_wallet = format!("GOWNER{}", &Uuid::new_v4().to_simple().to_string()[..16]);
    let member_wallet = format!("GMEMBER{}", &Uuid::new_v4().to_simple().to_string()[..16]);

    sqlx::query(
        "INSERT INTO creators (id, username, wallet_address, password_hash, created_at) VALUES ($1, $2, $3, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(&owner_username)
    .bind(&owner_wallet)
    .bind(&owner_hash)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO creators (id, username, wallet_address, password_hash, created_at) VALUES ($1, $2, $3, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(&member_username)
    .bind(&member_wallet)
    .bind(&member_hash)
    .execute(&pool)
    .await
    .unwrap();

    let create_request = CreateTeamRequest {
        name: team_name.clone(),
        owner_username: owner_username.clone(),
        members: Some(vec![TeamMemberRequest {
            creator_username: owner_username.clone(),
            share_percentage: 70,
        }, TeamMemberRequest {
            creator_username: member_username.clone(),
            share_percentage: 30,
        }]),
    };

    let team = team_controller::create_team(&state, create_request)
        .await
        .expect("create team");

    let mut tx = pool.begin().await.unwrap();
    let record_req = RecordTipRequest {
        username: member_username.clone(),
        amount: "10".to_string(),
        transaction_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
        message: None,
    };

    let (tip, _) = tip_controller::record_tip_in_tx(&state, &mut tx, &record_req)
        .await
        .expect("record tip in tx");
    tx.commit().await.unwrap();

    let splits: Vec<(String, String)> = sqlx::query_as(
        "SELECT member_username, amount FROM tip_splits WHERE tip_id = $1 ORDER BY member_username ASC",
    )
    .bind(tip.id)
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(splits.len(), 2);
    let splits_map: HashMap<_, _> = splits
        .into_iter()
        .map(|(username, amount)| (username, amount.parse::<Decimal>().unwrap()))
        .collect();

    assert_eq!(splits_map.get(&member_username).unwrap().clone(), Decimal::from(3));
    assert_eq!(splits_map.get(&owner_username).unwrap().clone(), Decimal::from(7));

    let history = team_controller::get_split_history(&state, team.id)
        .await
        .expect("get split history");
    assert_eq!(history.len(), 2);
    assert!(history.iter().any(|entry| entry.member_username == owner_username));
    assert!(history.iter().any(|entry| entry.member_username == member_username));
}
