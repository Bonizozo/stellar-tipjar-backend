//! Production-grade Stellar / Horizon service.
//!
//! Implements the full tip transaction pipeline described in issue #530:
//!
//! 1. **Dynamic fees** — fetches the current base fee from `/fee_stats` with a
//!    sane floor (100 stroops) and ceiling (10 000 stroops).
//! 2. **Memo validation** — rejects memos whose UTF-8 byte length exceeds 28.
//! 3. **Destination account check** — verifies the destination exists via
//!    `/accounts/{address}`; returns `DestinationUnfunded` if not.
//! 4. **Spendable balance** — computes `native_balance − (2 + subentries) × 0.5 XLM
//!    − selling_liabilities` and rejects amounts that would leave the account
//!    below the minimum reserve.
//! 5. **Horizon error mapping** — maps every relevant result code and HTTP
//!    status (429, 504) to a typed `StellarError`; implements one automatic
//!    retry on 504 per Stellar guidance.

use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tracing::Instrument;

use super::circuit_breaker::CircuitBreaker;
use super::retry::{with_retry, RetryConfig};
use crate::errors::{AppError, AppResult, StellarError};
use crate::errors::stellar::{map_op_result_code, map_tx_result_code};
use crate::telemetry::http_client::inject_trace_headers;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Minimum base fee in stroops (Stellar floor; never go below this).
const FEE_FLOOR_STROOPS: u64 = 100;
/// Maximum base fee in stroops we are willing to pay (surge-pricing ceiling).
const FEE_CEILING_STROOPS: u64 = 10_000;
/// Base reserve per account and per subentry, in XLM.
const BASE_RESERVE_XLM: &str = "0.5";
/// Minimum number of base reserves every account must maintain (account + implicit).
const MIN_ACCOUNT_RESERVES: u64 = 2;
/// Maximum UTF-8 byte length for a Stellar text memo.
pub const MEMO_MAX_BYTES: usize = 28;
/// Minimum XLM required when creating a new account (currently 1 XLM = 2 base reserves).
#[allow(dead_code)]
const MIN_CREATE_ACCOUNT_XLM: &str = "1";
/// Default submit timeout in seconds.
#[allow(dead_code)]
const DEFAULT_SUBMIT_TIMEOUT_SECS: u64 = 30;

// ── Horizon response types ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HorizonTransactionResponse {
    pub id: String,
    pub hash: String,
    pub successful: bool,
    /// Present only when the transaction failed.
    pub result_codes: Option<HorizonResultCodes>,
}

#[derive(Debug, Deserialize)]
pub struct HorizonResultCodes {
    pub transaction: Option<String>,
    pub operations: Option<Vec<String>>,
}

/// Horizon `/fee_stats` response — only the fields we care about.
#[derive(Debug, Deserialize)]
pub struct HorizonFeeStats {
    pub last_ledger_base_fee: Option<String>,
    pub fee_charged: Option<HorizonFeeCharged>,
}

#[derive(Debug, Deserialize)]
pub struct HorizonFeeCharged {
    pub p99: Option<String>,
}

/// Horizon `/accounts/{id}` response — only the fields we need.
#[derive(Debug, Deserialize)]
pub struct HorizonAccountResponse {
    pub account_id: String,
    pub subentry_count: u32,
    pub balances: Vec<HorizonBalance>,
}

#[derive(Debug, Deserialize)]
pub struct HorizonBalance {
    pub asset_type: String,
    pub balance: String,
    /// Non-native assets may have selling liabilities; native too.
    pub selling_liabilities: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SorobanRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

// ── Service ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct StellarService {
    client: Client,
    /// Horizon base URL — injected so tests can point at `httpmock`.
    pub horizon_url: String,
    /// Soroban RPC URL (may be the same host or different).
    pub rpc_url: String,
    pub network: String,
    #[allow(dead_code)]
    pub submit_timeout: Duration,
    retry_config: RetryConfig,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl StellarService {
    /// Construct with explicit Horizon + RPC URLs and network name.
    /// Tests pass `mock_server.base_url()` here.
    pub fn new(rpc_url: String, network: String) -> Self {
        let horizon_url = if network == "mainnet" {
            "https://horizon.stellar.org".to_string()
        } else {
            "https://horizon-testnet.stellar.org".to_string()
        };
        Self {
            client: Client::new(),
            horizon_url,
            rpc_url,
            network,
            submit_timeout: Duration::from_secs(30),
            retry_config: RetryConfig::default(),
            circuit_breaker: Arc::new(CircuitBreaker::new(5, Duration::from_secs(60))),
        }
    }

    /// Construct with an explicit Horizon base URL (used in tests).
    pub fn with_horizon_url(mut self, url: String) -> Self {
        self.horizon_url = url;
        self
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Validate a memo by its UTF-8 byte length.
    ///
    /// Stellar text memos are limited to **28 bytes** (not characters).
    /// A single emoji like 🎉 is 4 bytes, so a 7-emoji memo already exceeds
    /// the limit.  Returns `Err(MemoTooLong)` if the byte length exceeds 28.
    pub fn validate_memo(memo: &str) -> AppResult<()> {
        let byte_len = memo.len(); // Rust `str::len()` returns UTF-8 byte count
        if byte_len > MEMO_MAX_BYTES {
            return Err(AppError::Stellar(StellarError::MemoTooLong {
                actual_bytes: byte_len,
            }));
        }
        Ok(())
    }

    /// Fetch the current recommended base fee from Horizon `/fee_stats`.
    ///
    /// Returns a value clamped to [`FEE_FLOOR_STROOPS`]..=[`FEE_CEILING_STROOPS`].
    /// On any error (network, parse) it falls back to `FEE_FLOOR_STROOPS` so
    /// the calling code always has a usable fee.
    pub async fn fetch_base_fee(&self) -> u64 {
        let url = format!("{}/fee_stats", self.horizon_url);
        let client = self.client.clone();

        let span = tracing::info_span!("horizon.fee_stats", "http.url" = %url);

        let raw: Option<u64> = async move {
            let mut headers = reqwest::header::HeaderMap::new();
            inject_trace_headers(&mut headers);

            let resp = client.get(&url).headers(headers).send().await.ok()?;
            let stats = resp.json::<HorizonFeeStats>().await.ok()?;

            // Prefer p99 during surge; fall back to last ledger base fee.
            stats
                .fee_charged
                .as_ref()
                .and_then(|fc| fc.p99.as_deref())
                .or(stats.last_ledger_base_fee.as_deref())
                .and_then(|s| s.parse::<u64>().ok())
        }
        .instrument(span)
        .await;

        raw.unwrap_or(FEE_FLOOR_STROOPS)
            .clamp(FEE_FLOOR_STROOPS, FEE_CEILING_STROOPS)
    }

    /// Check whether a Stellar account exists on the network.
    ///
    /// Returns `Ok(account)` if found, `Err(DestinationUnfunded)` if 404,
    /// or a network error if Horizon is unreachable.
    pub async fn check_account_exists(
        &self,
        address: &str,
    ) -> AppResult<HorizonAccountResponse> {
        let url = format!("{}/accounts/{}", self.horizon_url, address);
        let client = self.client.clone();
        let address_owned = address.to_string();

        let span = tracing::info_span!(
            "horizon.check_account",
            "http.url"     = %url,
            "http.method"  = "GET",
            "peer.service" = "horizon",
        );

        async move {
            let mut headers = reqwest::header::HeaderMap::new();
            inject_trace_headers(&mut headers);

            let resp = client
                .get(&url)
                .headers(headers)
                .send()
                .await
                .map_err(|_| AppError::Stellar(StellarError::NetworkUnavailable))?;

            match resp.status().as_u16() {
                200 => resp.json::<HorizonAccountResponse>().await.map_err(|_| {
                    AppError::Stellar(StellarError::InvalidTransaction {
                        reason: "Malformed account response from Horizon".to_string(),
                    })
                }),
                404 => Err(AppError::Stellar(StellarError::DestinationUnfunded {
                    address: address_owned,
                })),
                429 => {
                    let retry = resp
                        .headers()
                        .get("Retry-After")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(60);
                    Err(AppError::Stellar(StellarError::RateLimited {
                        retry_after_secs: retry,
                    }))
                }
                _ => Err(AppError::Stellar(StellarError::NetworkUnavailable)),
            }
        }
        .instrument(span)
        .await
    }

    /// Compute the **spendable** XLM balance for a sender account.
    ///
    /// Formula (per Stellar documentation):
    /// ```text
    /// spendable = native_balance
    ///           − (2 + subentry_count) × 0.5 XLM   ← minimum reserve
    ///           − selling_liabilities_native
    /// ```
    ///
    /// Returns `Err(InsufficientBalance)` when `amount_xlm` exceeds the
    /// spendable balance.
    pub async fn validate_spendable_balance(
        &self,
        sender_address: &str,
        amount_xlm: &str,
    ) -> AppResult<()> {
        let account = self.check_account_exists(sender_address).await?;

        let native = account
            .balances
            .iter()
            .find(|b| b.asset_type == "native")
            .ok_or_else(|| {
                AppError::Stellar(StellarError::InvalidTransaction {
                    reason: "Account has no native XLM balance".to_string(),
                })
            })?;

        let balance = Decimal::from_str(&native.balance).map_err(|_| {
            AppError::Stellar(StellarError::InvalidTransaction {
                reason: "Cannot parse native balance".to_string(),
            })
        })?;

        let base_reserve = Decimal::from_str(BASE_RESERVE_XLM).unwrap();
        let reserves =
            base_reserve * Decimal::from(MIN_ACCOUNT_RESERVES + account.subentry_count as u64);

        let selling_liabilities = native
            .selling_liabilities
            .as_deref()
            .and_then(|s| Decimal::from_str(s).ok())
            .unwrap_or(Decimal::ZERO);

        let spendable = balance - reserves - selling_liabilities;
        let amount = Decimal::from_str(amount_xlm).map_err(|_| {
            AppError::Validation(crate::errors::ValidationError::InvalidRequest {
                message: "Invalid tip amount".to_string(),
            })
        })?;

        // Also deduct the maximum fee we might pay (ceiling / 1e7 XLM).
        let max_fee_xlm = Decimal::from(FEE_CEILING_STROOPS) / Decimal::from(10_000_000u64);
        let required = amount + max_fee_xlm;

        if spendable < required {
            return Err(AppError::Stellar(StellarError::InsufficientBalance {
                available_xlm: format!("{:.7}", spendable),
                required_xlm: format!("{:.7}", required),
            }));
        }
        Ok(())
    }

    /// Verify that a transaction exists and was successful on the Stellar network.
    ///
    /// - Maps Horizon result codes to typed `StellarError` variants.
    /// - Automatically retries once on HTTP 504 (per Stellar guidance).
    /// - Returns `Ok(true)` for a successful transaction, `Ok(false)` if not
    ///   found (404), and appropriate errors for failures.
    pub async fn verify_transaction(&self, transaction_hash: &str) -> AppResult<bool> {
        if !self.circuit_breaker.allow_request() {
            tracing::warn!(
                hash = transaction_hash,
                "Circuit breaker open; skipping Stellar verification"
            );
            return Err(AppError::Stellar(StellarError::CircuitBreakerOpen));
        }

        let url = format!("{}/transactions/{}", self.horizon_url, transaction_hash);
        let client = self.client.clone();
        let cb = self.circuit_breaker.clone();

        // One automatic retry for 504 (Stellar guideline: the tx might be in
        // flight; wait a few seconds then re-query).
        // The delay is read from STELLAR_504_RETRY_DELAY_MS env var so tests
        // can set it to 0 for fast execution.
        let retry_delay_ms = std::env::var("STELLAR_504_RETRY_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(3_000);

        let retry_config = RetryConfig {
            max_retries: 1,
            base_delay: Duration::from_millis(retry_delay_ms),
            max_delay: Duration::from_millis(retry_delay_ms),
        };

        let result = with_retry(&retry_config, || {
            let client = client.clone();
            let url = url.clone();
            async move {
                let span = tracing::info_span!(
                    "horizon.get_transaction",
                    "http.url"     = %url,
                    "http.method"  = "GET",
                    "peer.service" = "horizon",
                );

                async move {
                    let mut extra_headers = reqwest::header::HeaderMap::new();
                    inject_trace_headers(&mut extra_headers);

                    let resp = client
                        .get(&url)
                        .headers(extra_headers)
                        .send()
                        .await
                        .map_err(|_| AppError::Stellar(StellarError::NetworkUnavailable))?;

                    match resp.status().as_u16() {
                        200 => {
                            let tx = resp
                                .json::<HorizonTransactionResponse>()
                                .await
                                .map_err(|_| {
                                    AppError::Stellar(StellarError::InvalidTransaction {
                                        reason: "Malformed Horizon response".to_string(),
                                    })
                                })?;

                            if tx.successful {
                                Ok(true)
                            } else {
                                // Map result codes to a typed error.
                                let err = map_horizon_result_codes(tx.result_codes.as_ref());
                                Err(err)
                            }
                        }
                        404 => Ok(false),
                        429 => {
                            let retry = resp
                                .headers()
                                .get("Retry-After")
                                .and_then(|v| v.to_str().ok())
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(60);
                            Err(AppError::Stellar(StellarError::RateLimited {
                                retry_after_secs: retry,
                            }))
                        }
                        504 => Err(AppError::Stellar(StellarError::GatewayTimeout)),
                        _ => Err(AppError::Stellar(StellarError::NetworkUnavailable)),
                    }
                }
                .instrument(span)
                .await
            }
        })
        .await;

        match &result {
            Ok(_) => cb.record_success(),
            Err(_) => cb.record_failure(),
        }

        result
    }

    /// Get the current health of the Stellar network connection.
    #[allow(dead_code)]
    pub async fn get_network_health(&self) -> AppResult<serde_json::Value> {
        let req = SorobanRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "getHealth".to_string(),
            params: serde_json::Value::Null,
        };

        let mut extra_headers = reqwest::header::HeaderMap::new();
        inject_trace_headers(&mut extra_headers);

        let response = self
            .client
            .post(&self.rpc_url)
            .headers(extra_headers)
            .json(&req)
            .send()
            .await
            .map_err(|_| AppError::Stellar(StellarError::NetworkUnavailable))?
            .json::<serde_json::Value>()
            .await
            .map_err(|_| AppError::Stellar(StellarError::NetworkUnavailable))?;

        Ok(response)
    }
}

/// Convert Horizon `result_codes` into a typed `AppError`.
fn map_horizon_result_codes(codes: Option<&HorizonResultCodes>) -> AppError {
    let Some(rc) = codes else {
        return AppError::Stellar(StellarError::TransactionFailed {
            code: "tx_failed".to_string(),
            message: "Transaction failed with unknown result code.".to_string(),
        });
    };

    // Check operation-level codes first (more specific).
    if let Some(ops) = rc.operations.as_ref() {
        for op_code in ops {
            if op_code == "op_success" {
                continue;
            }
            let msg = map_op_result_code(op_code)
                .unwrap_or("Operation failed with an unrecognised result code.")
                .to_string();
            return AppError::Stellar(StellarError::OperationFailed {
                code: op_code.clone(),
                message: msg,
            });
        }
    }

    // Fall back to transaction-level code.
    let tx_code = rc
        .transaction
        .as_deref()
        .unwrap_or("tx_failed")
        .to_string();
    let msg = map_tx_result_code(&tx_code)
        .unwrap_or("Transaction failed with an unrecognised result code.")
        .to_string();

    AppError::Stellar(StellarError::TransactionFailed {
        code: tx_code,
        message: msg,
    })
}
