use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum StellarError {
    #[error("transaction not found or not successful")]
    TransactionNotFound { hash: String },
    #[error("invalid Stellar transaction")]
    InvalidTransaction { reason: String },
    #[error("Stellar network unavailable")]
    NetworkUnavailable,
    #[error("Stellar circuit breaker is open")]
    CircuitBreakerOpen,
}

impl StellarError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TransactionNotFound { .. } => "STELLAR_TX_NOT_FOUND",
            Self::InvalidTransaction { .. } => "STELLAR_INVALID_TX",
            Self::NetworkUnavailable => "STELLAR_NETWORK_UNAVAILABLE",
            Self::CircuitBreakerOpen => "STELLAR_CIRCUIT_OPEN",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::TransactionNotFound { .. } => {
                "Transaction not found or unsuccessful on the Stellar network".to_string()
            }
            Self::InvalidTransaction { reason } => format!("Invalid Stellar transaction: {}", reason),
            Self::NetworkUnavailable => {
                "Unable to verify transaction on the Stellar network".to_string()
            }
            Self::CircuitBreakerOpen => {
                "Transaction verification is temporarily unavailable".to_string()
            }
        }
    }

    pub fn details(&self) -> serde_json::Value {
        match self {
            Self::TransactionNotFound { hash } => json!({ "transaction_hash": hash }),
            Self::InvalidTransaction { reason } => json!({ "reason": reason }),
            Self::NetworkUnavailable | Self::CircuitBreakerOpen => json!({}),
        }
    }
}

