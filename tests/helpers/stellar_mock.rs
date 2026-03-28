//! Stellar API mocking utilities

use httpmock::prelude::*;
use serde_json::json;

/// Create a mock Stellar Horizon server for testing
pub struct StellarMockServer {
    pub mock_server: MockServer,
}

impl StellarMockServer {
    pub fn new() -> Self {
        Self {
            mock_server: MockServer::start(),
        }
    }

    pub fn base_url(&self) -> String {
        self.mock_server.base_url()
    }

    /// Mock successful transaction with payment operation
    pub fn mock_payment_transaction(
        &self,
        tx_hash: &str,
        amount: &str,
        from: &str,
        to: &str,
    ) -> Mock {
        self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(200).json_body(json!({
                "id": tx_hash,
                "hash": tx_hash,
                "successful": true,
                "source_account": from,
                "operations": [{
                    "id": format!("{}-1", tx_hash),
                    "type": "payment",
                    "amount": amount,
                    "asset_type": "native",
                    "from": from,
                    "to": to
                }],
                "fee_charged": "100",
                "max_fee": "100",
                "operation_count": 1,
                "created_at": "2024-01-01T00:00:00Z"
            }));
        })
    }

    /// Mock failed transaction
    pub fn mock_failed_transaction(&self, tx_hash: &str) -> Mock {
        self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(200).json_body(json!({
                "id": tx_hash,
                "hash": tx_hash,
                "successful": false,
                "result_code": -1,
                "result_code_s": "tx_failed"
            }));
        })
    }

    /// Mock transaction not found
    pub fn mock_transaction_not_found(&self, tx_hash: &str) -> Mock {
        self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(404).json_body(json!({
                "type": "https://stellar.org/horizon-errors/not_found",
                "title": "Resource Missing",
                "status": 404,
                "detail": "The resource at the url requested was not found."
            }));
        })
    }

    /// Mock network error (500)
    pub fn mock_network_error(&self, tx_hash: &str) -> Mock {
        self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(500).json_body(json!({
                "type": "https://stellar.org/horizon-errors/server_error",
                "title": "Internal Server Error",
                "status": 500
            }));
        })
    }

    /// Mock timeout (delayed response)
    pub fn mock_timeout(&self, tx_hash: &str, delay_seconds: u64) -> Mock {
        self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(200)
                .delay(std::time::Duration::from_secs(delay_seconds))
                .json_body(json!({
                    "id": tx_hash,
                    "hash": tx_hash,
                    "successful": true
                }));
        })
    }

    /// Mock rate limiting (429)
    pub fn mock_rate_limit(&self, tx_hash: &str) -> Mock {
        self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(429)
                .header("Retry-After", "60")
                .json_body(json!({
                    "type": "https://stellar.org/horizon-errors/rate_limit_exceeded",
                    "title": "Rate Limit Exceeded",
                    "status": 429
                }));
        })
    }

    /// Mock malformed transaction data
    pub fn mock_malformed_transaction(&self, tx_hash: &str) -> Mock {
        self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(200).json_body(json!({
                "id": tx_hash,
                // Missing required fields to test error handling
                "successful": true
            }));
        })
    }
}
