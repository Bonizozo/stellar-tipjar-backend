use serde_json::json;

/// All typed errors that can arise from the Stellar / Horizon pipeline.
#[derive(Debug, thiserror::Error)]
pub enum StellarError {
    // ── Transaction verification ────────────────────────────────────────────
    #[error("transaction not found or not successful")]
    TransactionNotFound { hash: String },
    #[error("invalid Stellar transaction")]
    InvalidTransaction { reason: String },

    // ── Network / infrastructure ────────────────────────────────────────────
    #[error("Stellar network unavailable")]
    NetworkUnavailable,
    #[error("Stellar circuit breaker is open")]
    CircuitBreakerOpen,

    // ── Horizon result-code mapping ─────────────────────────────────────────
    /// Submission failed with a specific transaction result code.
    #[error("transaction failed: {code}")]
    TransactionFailed { code: String, message: String },
    /// One or more operations in the transaction failed.
    #[error("operation failed: {code}")]
    OperationFailed { code: String, message: String },
    /// Horizon returned HTTP 429 (rate limited).
    #[error("Horizon rate limit exceeded; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    /// Horizon returned HTTP 504 or timed out; the client should retry.
    #[error("Horizon gateway timeout; submission may have succeeded — check status")]
    GatewayTimeout,

    // ── Pre-submission validation ────────────────────────────────────────────
    /// The destination account does not exist on the network.
    #[error("destination account does not exist on the Stellar network")]
    DestinationUnfunded { address: String },
    /// Sender's spendable balance is insufficient for the tip amount plus fees.
    #[error("insufficient spendable balance: available {available_xlm} XLM, required {required_xlm} XLM")]
    InsufficientBalance {
        available_xlm: String,
        required_xlm: String,
    },
    /// Memo exceeds the 28-byte UTF-8 limit imposed by the Stellar protocol.
    #[error("memo exceeds 28-byte UTF-8 limit ({actual_bytes} bytes)")]
    MemoTooLong { actual_bytes: usize },
}

impl StellarError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TransactionNotFound { .. } => "STELLAR_TX_NOT_FOUND",
            Self::InvalidTransaction { .. } => "STELLAR_INVALID_TX",
            Self::NetworkUnavailable => "STELLAR_NETWORK_UNAVAILABLE",
            Self::CircuitBreakerOpen => "STELLAR_CIRCUIT_OPEN",
            Self::TransactionFailed { .. } => "STELLAR_TX_FAILED",
            Self::OperationFailed { .. } => "STELLAR_OP_FAILED",
            Self::RateLimited { .. } => "STELLAR_RATE_LIMITED",
            Self::GatewayTimeout => "STELLAR_GATEWAY_TIMEOUT",
            Self::DestinationUnfunded { .. } => "STELLAR_DESTINATION_UNFUNDED",
            Self::InsufficientBalance { .. } => "STELLAR_INSUFFICIENT_BALANCE",
            Self::MemoTooLong { .. } => "STELLAR_MEMO_TOO_LONG",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::TransactionNotFound { .. } => {
                "Transaction not found or unsuccessful on the Stellar network".to_string()
            }
            Self::InvalidTransaction { reason } => {
                format!("Invalid Stellar transaction: {}", reason)
            }
            Self::NetworkUnavailable => {
                "Unable to verify transaction on the Stellar network".to_string()
            }
            Self::CircuitBreakerOpen => {
                "Transaction verification is temporarily unavailable".to_string()
            }
            Self::TransactionFailed { message, .. } => message.clone(),
            Self::OperationFailed { message, .. } => message.clone(),
            Self::RateLimited { retry_after_secs } => format!(
                "Too many requests to Stellar network. Please retry after {} seconds.",
                retry_after_secs
            ),
            Self::GatewayTimeout => {
                "Stellar network timed out. Your tip may have been submitted — \
                 please check your transaction status before retrying."
                    .to_string()
            }
            Self::DestinationUnfunded { address } => format!(
                "The destination account {} does not exist on the Stellar network. \
                 It must be funded with at least 1 XLM before receiving a tip.",
                &address[..8.min(address.len())]
            ),
            Self::InsufficientBalance {
                available_xlm,
                required_xlm,
            } => format!(
                "Insufficient spendable balance. You have {} XLM available but {} XLM is required.",
                available_xlm, required_xlm
            ),
            Self::MemoTooLong { actual_bytes } => format!(
                "Tip memo is {} bytes (UTF-8), but Stellar memos are limited to 28 bytes. \
                 Please shorten your memo.",
                actual_bytes
            ),
        }
    }

    pub fn details(&self) -> serde_json::Value {
        match self {
            Self::TransactionNotFound { hash } => json!({ "transaction_hash": hash }),
            Self::InvalidTransaction { reason } => json!({ "reason": reason }),
            Self::NetworkUnavailable | Self::CircuitBreakerOpen => json!({}),
            Self::TransactionFailed { code, .. } => json!({ "result_code": code }),
            Self::OperationFailed { code, .. } => json!({ "result_code": code }),
            Self::RateLimited { retry_after_secs } => {
                json!({ "retry_after_secs": retry_after_secs })
            }
            Self::GatewayTimeout => json!({ "action": "check_transaction_status_before_retry" }),
            Self::DestinationUnfunded { address } => json!({ "destination": address }),
            Self::InsufficientBalance {
                available_xlm,
                required_xlm,
            } => json!({
                "available_xlm": available_xlm,
                "required_xlm": required_xlm,
            }),
            Self::MemoTooLong { actual_bytes } => {
                json!({ "actual_bytes": actual_bytes, "max_bytes": 28 })
            }
        }
    }
}

// ── Horizon result-code tables ────────────────────────────────────────────────

/// Map a Horizon transaction result code string to a human-readable message.
/// Returns `None` for unknown codes so callers can fall back gracefully.
pub fn map_tx_result_code(code: &str) -> Option<&'static str> {
    match code {
        "tx_success" => Some("Transaction succeeded."),
        "tx_failed" => Some("One or more operations failed."),
        "tx_too_early" => Some("Transaction was submitted before its valid time window."),
        "tx_too_late" => Some("Transaction has expired — please resubmit with a new timebounds."),
        "tx_missing_operation" => Some("Transaction has no operations."),
        "tx_bad_seq" => {
            Some("Sequence number mismatch — another transaction was submitted concurrently.")
        }
        "tx_bad_auth" => Some("Insufficient or invalid signatures on the transaction."),
        "tx_insufficient_balance" => {
            Some("The source account has insufficient XLM to pay the fee.")
        }
        "tx_no_source_account" => Some("The source account does not exist."),
        "tx_insufficient_fee" => {
            Some("Fee is too low for current network surge pricing. Please increase the fee.")
        }
        "tx_bad_auth_extra" => Some("Transaction has extraneous signatures."),
        "tx_internal_error" => Some("Horizon internal error — please retry."),
        _ => None,
    }
}

/// Map a Horizon operation result code string to a human-readable message.
pub fn map_op_result_code(code: &str) -> Option<&'static str> {
    match code {
        "op_success" => Some("Operation succeeded."),
        "op_malformed" => Some("Operation is malformed or invalid."),
        "op_underfunded" => Some("Insufficient balance to send this amount."),
        "op_src_no_trust" => Some("Source account is missing a trustline for this asset."),
        "op_src_not_authorized" => {
            Some("Source account is not authorized to send this asset.")
        }
        "op_no_destination" => {
            Some("Destination account does not exist and must be created with at least 1 XLM.")
        }
        "op_no_trust" => Some("Destination account is missing a trustline for this asset."),
        "op_not_authorized" => Some("Destination is not authorized to hold this asset."),
        "op_line_full" => Some("Destination account has reached its trustline limit."),
        "op_no_issuer" => Some("Asset issuer account does not exist."),
        "op_already_exists" => Some("Destination account already exists (for createAccount)."),
        "op_not_enough_native" => {
            Some("Insufficient native XLM for the destination's minimum reserve.")
        }
        _ => None,
    }
}
