//! Test data generators and fixtures

use serde_json::{json, Value};
use uuid::Uuid;

/// Generate test creator data
pub struct CreatorBuilder {
    username: String,
    wallet_address: String,
    email: String,
}

impl CreatorBuilder {
    pub fn new() -> Self {
        let id = Uuid::new_v4().to_string()[..8].to_string();
        Self {
            username: format!("creator_{}", id),
            wallet_address: generate_stellar_address(),
            email: format!("creator_{}@test.com", id),
        }
    }

    pub fn username(mut self, username: &str) -> Self {
        self.username = username.to_string();
        self
    }

    pub fn wallet_address(mut self, address: &str) -> Self {
        self.wallet_address = address.to_string();
        self
    }

    pub fn email(mut self, email: &str) -> Self {
        self.email = email.to_string();
        self
    }

    pub fn build(self) -> Value {
        json!({
            "username": self.username,
            "wallet_address": self.wallet_address,
            "email": self.email
        })
    }
}

/// Generate test tip data
pub struct TipBuilder {
    username: String,
    amount: String,
    transaction_hash: String,
}

impl TipBuilder {
    pub fn new() -> Self {
        Self {
            username: "default_creator".to_string(),
            amount: "10.0".to_string(),
            transaction_hash: generate_transaction_hash(),
        }
    }

    pub fn username(mut self, username: &str) -> Self {
        self.username = username.to_string();
        self
    }

    pub fn amount(mut self, amount: &str) -> Self {
        self.amount = amount.to_string();
        self
    }

    pub fn transaction_hash(mut self, hash: &str) -> Self {
        self.transaction_hash = hash.to_string();
        self
    }

    pub fn build(self) -> Value {
        json!({
            "username": self.username,
            "amount": self.amount,
            "transaction_hash": self.transaction_hash
        })
    }
}

/// Generate valid Stellar address
pub fn generate_stellar_address() -> String {
    // Generate a valid-looking Stellar address (starts with G, 56 chars total)
    let mut address = "G".to_string();
    let suffix = Uuid::new_v4().to_string().replace("-", "").to_uppercase();
    address.push_str(&suffix[..55]); // Take 55 chars to make 56 total
    address
}

/// Generate transaction hash
pub fn generate_transaction_hash() -> String {
    format!(
        "TX{}",
        Uuid::new_v4().to_string().replace("-", "").to_uppercase()
    )
}

/// Generate multiple creators for bulk testing
pub fn generate_creators(count: usize) -> Vec<Value> {
    (0..count)
        .map(|i| {
            CreatorBuilder::new()
                .username(&format!("bulk_creator_{}", i))
                .build()
        })
        .collect()
}

/// Generate multiple tips for bulk testing
pub fn generate_tips(creator_username: &str, count: usize) -> Vec<Value> {
    (0..count)
        .map(|i| {
            TipBuilder::new()
                .username(creator_username)
                .amount(&format!("{}.{:02}", i + 1, (i * 37) % 100)) // Varied amounts
                .build()
        })
        .collect()
}

/// Edge case test data
pub struct EdgeCaseData;

impl EdgeCaseData {
    /// Invalid creator data
    pub fn invalid_creators() -> Vec<Value> {
        vec![
            // Missing username
            json!({
                "wallet_address": generate_stellar_address(),
                "email": "test@example.com"
            }),
            // Missing wallet address
            json!({
                "username": "test_user",
                "email": "test@example.com"
            }),
            // Missing email
            json!({
                "username": "test_user",
                "wallet_address": generate_stellar_address()
            }),
            // Empty username
            json!({
                "username": "",
                "wallet_address": generate_stellar_address(),
                "email": "test@example.com"
            }),
            // Invalid email format
            json!({
                "username": "test_user",
                "wallet_address": generate_stellar_address(),
                "email": "invalid-email"
            }),
            // Invalid wallet address (too short)
            json!({
                "username": "test_user",
                "wallet_address": "GINVALID",
                "email": "test@example.com"
            }),
            // Username with special characters
            json!({
                "username": "test@user#123",
                "wallet_address": generate_stellar_address(),
                "email": "test@example.com"
            }),
        ]
    }

    /// Invalid tip data
    pub fn invalid_tips() -> Vec<Value> {
        vec![
            // Missing username
            json!({
                "amount": "10.0",
                "transaction_hash": generate_transaction_hash()
            }),
            // Missing amount
            json!({
                "username": "test_creator",
                "transaction_hash": generate_transaction_hash()
            }),
            // Missing transaction hash
            json!({
                "username": "test_creator",
                "amount": "10.0"
            }),
            // Negative amount
            json!({
                "username": "test_creator",
                "amount": "-5.0",
                "transaction_hash": generate_transaction_hash()
            }),
            // Zero amount
            json!({
                "username": "test_creator",
                "amount": "0.0",
                "transaction_hash": generate_transaction_hash()
            }),
            // Invalid amount format
            json!({
                "username": "test_creator",
                "amount": "not_a_number",
                "transaction_hash": generate_transaction_hash()
            }),
            // Empty transaction hash
            json!({
                "username": "test_creator",
                "amount": "10.0",
                "transaction_hash": ""
            }),
            // Non-existent creator
            json!({
                "username": "nonexistent_creator",
                "amount": "10.0",
                "transaction_hash": generate_transaction_hash()
            }),
        ]
    }

    /// Boundary value test cases
    pub fn boundary_values() -> Vec<Value> {
        vec![
            // Very small amount
            json!({
                "username": "test_creator",
                "amount": "0.0000001",
                "transaction_hash": generate_transaction_hash()
            }),
            // Very large amount
            json!({
                "username": "test_creator",
                "amount": "999999999.9999999",
                "transaction_hash": generate_transaction_hash()
            }),
            // Maximum precision
            json!({
                "username": "test_creator",
                "amount": "1.1234567",
                "transaction_hash": generate_transaction_hash()
            }),
            // Long username (at limit)
            json!({
                "username": "a".repeat(50), // Assuming 50 char limit
                "amount": "10.0",
                "transaction_hash": generate_transaction_hash()
            }),
        ]
    }
}
