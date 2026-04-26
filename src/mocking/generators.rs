use rand::distributions::{Alphanumeric, DistString};
use serde_json::json;
use uuid::Uuid;

/// Deterministic-friendly random string generator for mock payloads.
pub fn random_token(len: usize) -> String {
    Alphanumeric.sample_string(&mut rand::thread_rng(), len)
}

pub fn random_uuid() -> String {
    Uuid::new_v4().to_string()
}

pub fn random_email(prefix: &str) -> String {
    format!("{}{}@example.test", prefix, random_token(8).to_lowercase())
}

pub fn random_wallet_address() -> String {
    format!("G{}", random_token(55))
}

pub fn random_tx_hash() -> String {
    random_token(64)
}

pub fn creator_payload(username: Option<&str>) -> serde_json::Value {
    let name = username
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("creator_{}", random_token(6).to_lowercase()));
    json!({
        "id": random_uuid(),
        "username": name,
        "wallet_address": random_wallet_address(),
        "email": random_email("creator_"),
        "created_at": chrono::Utc::now(),
    })
}

pub fn tip_payload(username: Option<&str>, amount: Option<&str>) -> serde_json::Value {
    json!({
        "id": random_uuid(),
        "creator_username": username.unwrap_or("creator_demo"),
        "amount": amount.unwrap_or("5.00"),
        "transaction_hash": random_tx_hash(),
        "created_at": chrono::Utc::now(),
    })
}
