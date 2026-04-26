use serde_json::{json, Value};
use uuid::Uuid;

pub fn creator_template(username: &str, wallet: &str) -> Value {
    json!({
        "id": Uuid::new_v4(),
        "username": username,
        "wallet_address": wallet,
        "created_at": chrono::Utc::now()
    })
}

pub fn tip_template(creator_username: &str, amount: &str, tx_hash: &str) -> Value {
    json!({
        "id": Uuid::new_v4(),
        "creator_username": creator_username,
        "amount": amount,
        "transaction_hash": tx_hash,
        "created_at": chrono::Utc::now()
    })
}

pub fn stellar_transaction_template(tx_hash: &str, successful: bool) -> Value {
    json!({
        "id": tx_hash,
        "hash": tx_hash,
        "successful": successful,
        "source_account": "GABC123TESTACCOUNT",
        "operations": [{
            "type": "payment",
            "amount": "10.0000000",
            "asset_type": "native"
        }],
        "created_at": chrono::Utc::now()
    })
}

pub fn error_template(status: u16, message: &str) -> Value {
    json!({ "error": message, "status": status })
}
