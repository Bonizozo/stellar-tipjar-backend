use async_trait::async_trait;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tracing::Instrument;

use super::circuit_breaker::CircuitBreaker;
use super::retry::{with_retry, RetryConfig};
use crate::errors::stellar::{map_op_result_code, map_tx_result_code};
use crate::errors::{AppError, AppResult, StellarError};
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

// ─────────────────────────── Horizon Response Types ─────────────────────────

/// Full Horizon transaction response with all fields needed for tip verification.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct HorizonTransactionResponse {
    pub id: String,
    pub hash: String,
    pub successful: bool,
    /// The Stellar account that submitted (signed) the transaction.
    pub source_account: String,
    /// Base64-encoded XDR memo value; may be absent.
    #[serde(default)]
    pub memo: Option<String>,
    /// Memo type: "none", "text", "id", "hash", "return"
    #[serde(default)]
    pub memo_type: Option<String>,
}

/// A single operation embedded in a transaction (from the Horizon operations endpoint).
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct HorizonOperation {
    #[serde(rename = "type")]
    pub op_type: String,
    /// Payment destination, if this is a payment operation.
    #[serde(default)]
    pub to: Option<String>,
    /// Amount as a string (e.g. "10.5000000"), if present.
    #[serde(default)]
    pub amount: Option<String>,
    /// Asset type: "native" for XLM.
    #[serde(default)]
    pub asset_type: Option<String>,
}

/// Horizon paginated response wrapper for operations.
#[derive(Debug, Deserialize)]
pub struct HorizonOperationsPage {
    #[serde(rename = "_embedded")]
    pub embedded: HorizonOperationsEmbedded,
}

#[derive(Debug, Deserialize)]
pub struct HorizonOperationsEmbedded {
    pub records: Vec<HorizonOperation>,
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

// ─────────────────────────── TipVerifier Trait ──────────────────────────────

/// All the fields the verifier must check to approve a tip.
#[derive(Debug, Clone)]
pub struct TipVerifyRequest {
    /// The Stellar transaction hash to look up.
    pub transaction_hash: String,
    /// Claimed payment amount in stroops (1 XLM = 10,000,000 stroops).
    /// Must be compared as integers – no floating-point arithmetic.
    pub amount_stroops: i64,
    /// Creator's Stellar wallet address (payment destination).
    pub destination: String,
    /// Optional memo that the tipper was supposed to include.
    pub expected_memo: Option<String>,
    /// Claimed source account (tipper's Stellar address).
    pub source_account: String,
}

/// The outcome of tip verification.
#[derive(Debug, Clone, PartialEq)]
pub enum VerifyOutcome {
    Confirmed,
    Rejected { reason: String },
}

/// Injectable verifier abstraction.  
/// Production code uses `StellarService`; tests inject `MockTipVerifier`.
#[async_trait]
pub trait TipVerifier: Send + Sync + 'static {
    async fn verify_tip(&self, req: &TipVerifyRequest) -> AppResult<VerifyOutcome>;
}

// ─────────────────────────── StellarService ─────────────────────────────────

#[allow(dead_code)]
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
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("failed to build reqwest client"),
            horizon_url,
            rpc_url,
            network,
            submit_timeout: Duration::from_secs(30),
            retry_config: RetryConfig::default(),
            circuit_breaker: Arc::new(CircuitBreaker::new(5, Duration::from_secs(60))),
        }
    }

    fn horizon_base(&self) -> &'static str {
        if self.network == "mainnet" {
            "https://horizon.stellar.org"
        } else {
            "https://horizon-testnet.stellar.org"
        }
    }

    /// Construct with an explicit Horizon base URL (used in tests to point at `httpmock`).
    pub fn with_horizon_url(mut self, url: String) -> Self {
        self.horizon_url = url;
        self
    }

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
    pub async fn check_account_exists(&self, address: &str) -> AppResult<HorizonAccountResponse> {
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

    /// Low-level: fetch a transaction record from Horizon with retry + circuit-breaker.
    async fn fetch_transaction(&self, hash: &str) -> AppResult<Option<HorizonTransactionResponse>> {
        if !self.circuit_breaker.allow_request() {
            tracing::warn!("Circuit breaker open; skipping Horizon call for {}", hash);
            return Err(AppError::Stellar(StellarError::CircuitBreakerOpen));
        }

        let url = format!("{}/transactions/{}", self.horizon_base(), hash);
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
                let resp = client
                    .get(&url)
                    .send()
                    .await
                    .map_err(|_| AppError::Stellar(StellarError::NetworkUnavailable))?;

                match resp.status().as_u16() {
                    200 => {
                        let tx: HorizonTransactionResponse = resp.json().await.map_err(|_| {
                            AppError::Stellar(StellarError::InvalidTransaction {
                                reason: "Malformed Horizon response".to_string(),
                            })
                        })?;
                        Ok(Some(tx))
                    }
                    404 => Ok(None),
                    429 | 500..=599 => Err(AppError::Stellar(StellarError::NetworkUnavailable)),
                    other => Err(AppError::Stellar(StellarError::InvalidTransaction {
                        reason: format!("Unexpected Horizon status: {}", other),
                    })),
                }
            }
        })
        .await;

        match &result {
            Ok(_) => cb.record_success(),
            Err(_) => cb.record_failure(),
        }

        result
    }

    /// Fetch the list of operations for a transaction from Horizon.
    async fn fetch_operations(&self, hash: &str) -> AppResult<Vec<HorizonOperation>> {
        let url = format!("{}/transactions/{}/operations", self.horizon_base(), hash);
        let client = self.client.clone();

        let result = with_retry(&self.retry_config, || {
            let client = client.clone();
            let url = url.clone();
            async move {
                let resp = client
                    .get(&url)
                    .send()
                    .await
                    .map_err(|_| AppError::Stellar(StellarError::NetworkUnavailable))?;

                if resp.status().is_success() {
                    let page: HorizonOperationsPage = resp.json().await.map_err(|_| {
                        AppError::Stellar(StellarError::InvalidTransaction {
                            reason: "Malformed Horizon operations response".to_string(),
                        })
                    })?;
                    Ok(page.embedded.records)
                } else {
                    Err(AppError::Stellar(StellarError::NetworkUnavailable))
                }
            }
        })
        .await;

        result
    }

    /// Legacy helper kept for backward compatibility – returns true if transaction
    /// exists and is successful. Does not verify amounts or destinations.
    pub async fn verify_transaction(&self, transaction_hash: &str) -> AppResult<bool> {
        let tx = self.fetch_transaction(transaction_hash).await?;
        Ok(tx.map(|t| t.successful).unwrap_or(false))
    }

    /// Convert XLM amount string (e.g. "10.5000000") to stroops (integer).
    /// Horizon always returns 7 decimal places.
    pub fn xlm_to_stroops(amount_str: &str) -> AppResult<i64> {
        // Parse as a decimal with exactly 7 decimal places.
        let parts: Vec<&str> = amount_str.split('.').collect();
        let whole: i64 = parts[0].parse().map_err(|_| {
            AppError::Stellar(StellarError::InvalidTransaction {
                reason: format!("Could not parse amount whole part: {}", amount_str),
            })
        })?;
        let fractional_str = parts.get(1).copied().unwrap_or("0");
        // Pad or truncate to exactly 7 digits
        let padded = format!("{:0<7}", fractional_str);
        let fractional: i64 = padded[..7].parse().map_err(|_| {
            AppError::Stellar(StellarError::InvalidTransaction {
                reason: format!("Could not parse amount fractional part: {}", amount_str),
            })
        })?;
        Ok(whole * 10_000_000 + fractional)
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

// ────────────────── TipVerifier implementation for StellarService ────────────

#[async_trait]
impl TipVerifier for StellarService {
    /// Full on-chain verification:
    /// 1. Transaction exists and succeeded.
    /// 2. Source account matches claimed tipper.
    /// 3. A payment operation exists with:
    ///    - asset_type = "native" (XLM)
    ///    - destination = creator wallet
    ///    - amount (in stroops) == claimed amount (integer comparison)
    /// 4. Memo matches expected_memo if provided.
    async fn verify_tip(&self, req: &TipVerifyRequest) -> AppResult<VerifyOutcome> {
        // ── Step 1: Fetch transaction ──────────────────────────────────────
        let tx = match self.fetch_transaction(&req.transaction_hash).await? {
            None => {
                return Ok(VerifyOutcome::Rejected {
                    reason: "Transaction not found on Stellar network".to_string(),
                });
            }
            Some(tx) => tx,
        };

        if !tx.successful {
            return Ok(VerifyOutcome::Rejected {
                reason: "Transaction did not succeed on the Stellar network".to_string(),
            });
        }

        // ── Step 2: Source account ─────────────────────────────────────────
        if tx.source_account != req.source_account {
            return Ok(VerifyOutcome::Rejected {
                reason: format!(
                    "Source account mismatch: expected {}, got {}",
                    req.source_account, tx.source_account
                ),
            });
        }

        // ── Step 3: Memo ───────────────────────────────────────────────────
        if let Some(expected) = &req.expected_memo {
            let actual = tx.memo.as_deref().unwrap_or("");
            if actual != expected.as_str() {
                return Ok(VerifyOutcome::Rejected {
                    reason: format!("Memo mismatch: expected '{}', got '{}'", expected, actual),
                });
            }
        }

        // ── Step 4: Operations ─────────────────────────────────────────────
        let operations = self.fetch_operations(&req.transaction_hash).await?;

        let matching_payment = operations.iter().find(|op| {
            op.op_type == "payment"
                && op.asset_type.as_deref() == Some("native")
                && op.to.as_deref() == Some(req.destination.as_str())
        });

        match matching_payment {
            None => Ok(VerifyOutcome::Rejected {
                reason: format!(
                    "No native payment to {} found in transaction",
                    req.destination
                ),
            }),
            Some(op) => {
                // Amount comparison in stroops – no float arithmetic
                let on_chain_amount_str = op.amount.as_deref().unwrap_or("0");
                let on_chain_stroops = Self::xlm_to_stroops(on_chain_amount_str)?;

                if on_chain_stroops != req.amount_stroops {
                    Ok(VerifyOutcome::Rejected {
                        reason: format!(
                            "Amount mismatch: expected {} stroops, on-chain {}",
                            req.amount_stroops, on_chain_stroops
                        ),
                    })
                } else {
                    Ok(VerifyOutcome::Confirmed)
                }
            }
        }
    }
}
