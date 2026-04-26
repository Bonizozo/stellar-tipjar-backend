use serde_json::{json, Value};
use uuid::Uuid;

use crate::mocking::generators;

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

/// Render template placeholders in a JSON value.
///
/// Supported placeholders:
/// - `{{request.path}}`
/// - `{{request.method}}`
/// - `{{random.uuid}}`
/// - `{{random.email}}`
/// - `{{random.wallet}}`
/// - `{{random.tx_hash}}`
/// - `{{now.rfc3339}}`
pub fn render_response_template(
    template: &Value,
    method: &str,
    path: &str,
    replacements: &std::collections::HashMap<String, String>,
) -> Value {
    match template {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        render_response_template(v, method, path, replacements),
                    )
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| render_response_template(v, method, path, replacements))
                .collect(),
        ),
        Value::String(s) => Value::String(render_template_string(s, method, path, replacements)),
        _ => template.clone(),
    }
}

fn render_template_string(
    input: &str,
    method: &str,
    path: &str,
    replacements: &std::collections::HashMap<String, String>,
) -> String {
    let mut out = input.to_string();
    out = out.replace("{{request.method}}", method);
    out = out.replace("{{request.path}}", path);
    out = out.replace("{{random.uuid}}", &generators::random_uuid());
    out = out.replace("{{random.email}}", &generators::random_email("mock_"));
    out = out.replace("{{random.wallet}}", &generators::random_wallet_address());
    out = out.replace("{{random.tx_hash}}", &generators::random_tx_hash());
    out = out.replace("{{now.rfc3339}}", &chrono::Utc::now().to_rfc3339());

    for (k, v) in replacements {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}
