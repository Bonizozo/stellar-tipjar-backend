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
/// Stroops in one XLM. Stellar amounts carry exactly 7 decimal places.
pub const STROOPS_PER_XLM: i64 = 10_000_000;
/// Number of decimal places in a Stellar amount.
pub const STELLAR_DECIMAL_PLACES: usize = 7;

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

    /// Convert an XLM amount string (e.g. `"10.5000000"`) to stroops.
    ///
    /// Integer-only: no `f64` is involved at any point, so no amount is ever
    /// rounded. Horizon normally returns exactly 7 decimal places, but this
    /// parses whatever the remote endpoint sends, so every malformed shape is
    /// rejected rather than coerced. Its result is compared against the
    /// caller's expected amount to decide whether a tip is confirmed, so a
    /// value that is silently wrong is worse than an error: it would confirm
    /// or reject the wrong payment.
    ///
    /// Rejected, each for a specific reason:
    ///
    /// - **Negative amounts** (`"-5.5"`). A Horizon payment amount is never
    ///   negative. The sign also cannot survive this representation: the whole
    ///   and fractional parts are parsed separately, so `-5` and `5000000`
    ///   recombine as `-50000000 + 5000000` = `-45000000` (that is, -4.5 XLM),
    ///   and for `"-0.5"` the sign is lost altogether because `"-0"` parses to
    ///   `0`. Rejecting is both correct and consistent with
    ///   [`crate::validation::amount::xlm_to_stroops_str`].
    /// - **More than 7 decimal places** (`"1.12345678"`). Truncating to
    ///   Stellar's precision would understate an amount that the network
    ///   cannot have produced in the first place, so an over-precise value
    ///   means the response is not what we think it is.
    /// - **Values that overflow `i64`.** Total XLM supply is ~10^11, far
    ///   inside `i64` stroops, but the multiplication is applied to a remote
    ///   value, and unchecked it panics in debug and silently wraps to a
    ///   negative amount in release.
    /// - **Anything that is not `digits[.digits]`** — a sign, whitespace,
    ///   exponents (`"1e3"`), a second `.` (`"1.2.3"`, whose tail was
    ///   previously ignored), or non-ASCII digits. Restricting to ASCII digits
    ///   also keeps the fixed-width fractional handling byte-safe.
    pub fn xlm_to_stroops(amount_str: &str) -> AppResult<i64> {
        let invalid =
            |reason: String| AppError::Stellar(StellarError::InvalidTransaction { reason });

        let (whole_str, frac_str) = match amount_str.split_once('.') {
            Some((whole, frac)) => (whole, frac),
            None => (amount_str, ""),
        };

        let is_ascii_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());

        if !is_ascii_digits(whole_str) {
            return Err(invalid(format!(
                "Amount '{}' is not a non-negative decimal number",
                amount_str
            )));
        }

        // `frac_str` is empty when there was no '.' at all, which is fine; it is
        // only invalid when a '.' was present with a non-digit tail — including
        // the second '.' of "1.2.3".
        if !frac_str.is_empty() && !is_ascii_digits(frac_str) {
            return Err(invalid(format!(
                "Amount '{}' has an invalid fractional part",
                amount_str
            )));
        }

        if frac_str.len() > STELLAR_DECIMAL_PLACES {
            return Err(invalid(format!(
                "Amount '{}' has more than {} decimal places",
                amount_str, STELLAR_DECIMAL_PLACES
            )));
        }

        let whole: i64 = whole_str
            .parse()
            .map_err(|_| invalid(format!("Amount '{}' is too large", amount_str)))?;

        // Right-pad to exactly 7 digits so "5" means 0.5 XLM, not 0.0000005.
        // All bytes are ASCII digits by now, so this cannot overflow i64.
        let fractional: i64 = format!("{:0<width$}", frac_str, width = STELLAR_DECIMAL_PLACES)
            .parse()
            .map_err(|_| {
                invalid(format!(
                    "Amount '{}' has an invalid fractional part",
                    amount_str
                ))
            })?;

        whole
            .checked_mul(STROOPS_PER_XLM)
            .and_then(|stroops| stroops.checked_add(fractional))
            .ok_or_else(|| invalid(format!("Amount '{}' overflows i64 stroops", amount_str)))
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

#[cfg(test)]
mod xlm_to_stroops_tests {
    use super::*;

    fn stroops(amount: &str) -> i64 {
        StellarService::xlm_to_stroops(amount)
            .unwrap_or_else(|e| panic!("expected '{}' to parse, got {:?}", amount, e))
    }

    fn err(amount: &str) -> String {
        match StellarService::xlm_to_stroops(amount) {
            Err(AppError::Stellar(StellarError::InvalidTransaction { reason })) => reason,
            other => panic!("expected '{}' to be rejected, got {:?}", amount, other),
        }
    }

    #[test]
    fn parses_horizon_shaped_amounts() {
        assert_eq!(stroops("10.5000000"), 105_000_000);
        assert_eq!(stroops("0.0000001"), 1);
        assert_eq!(stroops("1.0000000"), 10_000_000);
        assert_eq!(stroops("0.0000000"), 0);
    }

    #[test]
    fn pads_short_and_absent_fractional_parts() {
        // A short fraction is left-aligned: "5" is five tenths, not five stroops.
        assert_eq!(stroops("10.5"), 105_000_000);
        assert_eq!(stroops("0.1"), 1_000_000);
        assert_eq!(stroops("100"), 1_000_000_000);
        assert_eq!(stroops("0"), 0);
        // A trailing '.' has an empty fraction, which pads to zero.
        assert_eq!(stroops("7."), 70_000_000);
    }

    // ── Negative amounts ──────────────────────────────────────────────────
    // Decision: reject. Horizon never reports a negative payment amount, and
    // the whole/fractional split cannot represent one — see the doc comment on
    // xlm_to_stroops for the arithmetic. These assertions pin the rejection so
    // nobody reintroduces the silently-wrong magnitude.

    #[test]
    fn negative_amounts_are_rejected() {
        for amount in ["-5.0000000", "-5.5", "-0.5", "-0.0000001", "-1", "-0"] {
            assert!(
                err(amount).contains("non-negative"),
                "unexpected rejection reason for '{}'",
                amount
            );
        }
    }

    #[test]
    fn negative_amount_is_not_coerced_to_a_wrong_magnitude() {
        // Parsing whole and fractional parts independently would yield
        // -5 * 10_000_000 + 5_000_000 = -45_000_000, i.e. -4.5 XLM for -5.5,
        // and "-0.5" would lose its sign entirely because "-0" parses to 0.
        assert!(StellarService::xlm_to_stroops("-5.5").is_err());
        assert!(StellarService::xlm_to_stroops("-0.5").is_err());
    }

    // ── Over-precision ────────────────────────────────────────────────────
    // Decision: reject rather than truncate. Truncation silently understates
    // an amount the network cannot have produced, and this value is compared
    // against the expected amount to confirm a tip.

    #[test]
    fn more_than_seven_decimal_places_is_rejected_not_truncated() {
        let reason = err("1.12345678");
        assert!(reason.contains("more than 7 decimal places"), "{}", reason);

        // Even a trailing zero past the 7th place is refused: the string is
        // not a shape Horizon produces, so we do not guess at its intent.
        assert!(StellarService::xlm_to_stroops("1.00000000").is_err());
        // Exactly 7 places remains the boundary that is accepted.
        assert_eq!(stroops("1.1234567"), 11_234_567);
    }

    // ── Overflow ──────────────────────────────────────────────────────────
    // Decision: reject. Unchecked, `whole * 10_000_000` panics in debug and
    // wraps to a negative amount in release.

    #[test]
    fn amounts_that_overflow_i64_stroops_are_rejected() {
        // Parses as i64 but overflows once scaled to stroops.
        let reason = err(&i64::MAX.to_string());
        assert!(reason.contains("overflows"), "{}", reason);
        assert!(StellarService::xlm_to_stroops("1000000000000.0000000").is_err());

        // Overflow in the addition rather than the multiplication:
        // 922_337_203_685 * 10^7 leaves less than 10^7 of headroom.
        assert!(StellarService::xlm_to_stroops("922337203685.4775808").is_err());

        // Too large for i64 before scaling at all.
        assert!(StellarService::xlm_to_stroops("99999999999999999999").is_err());
    }

    #[test]
    fn largest_representable_amount_is_accepted() {
        // 922_337_203_685.4775807 XLM is i64::MAX stroops exactly — orders of
        // magnitude above total XLM supply (~10^11), but it must not error.
        assert_eq!(stroops("922337203685.4775807"), i64::MAX);
    }

    // ── Malformed input ───────────────────────────────────────────────────
    // Decision: reject anything that is not `digits[.digits]`.

    #[test]
    fn malformed_amounts_are_rejected() {
        for amount in [
            "",            // empty
            ".",           // no digits at all
            ".5",          // no whole part
            "1.2.3",       // second '.' was previously ignored, silently parsing as 1.2
            "1e3",         // exponent notation
            "+5.0",        // explicit sign
            " 5.0",        // leading whitespace
            "5.0 ",        // trailing whitespace
            "abc",         // not a number
            "1.abcdefg",   // non-digit fraction
            "1.٥",         // non-ASCII digit (Arabic-Indic five)
            "1.5\u{00e9}", // non-ASCII byte in the fraction
        ] {
            assert!(
                StellarService::xlm_to_stroops(amount).is_err(),
                "expected '{}' to be rejected",
                amount
            );
        }
    }

    #[test]
    fn multibyte_fraction_does_not_panic() {
        // The previous implementation right-padded to 7 chars and then sliced
        // `padded[..7]` by byte index, which panics when byte 7 falls inside a
        // multi-byte character. Each of these is now a plain error.
        for amount in ["1.ééééééé", "1.\u{10348}\u{10348}", "0.é"] {
            assert!(
                StellarService::xlm_to_stroops(amount).is_err(),
                "expected '{}' to be rejected",
                amount
            );
        }
    }

    #[test]
    fn agrees_with_the_validation_helper() {
        // The two converters must not disagree: one parses our own request
        // amounts, the other the on-chain amount they are compared against.
        for amount in ["10.5", "0.0000001", "100", "1.1234567", "0"] {
            assert_eq!(
                stroops(amount),
                crate::validation::amount::xlm_to_stroops_str(amount).unwrap(),
                "mismatch for '{}'",
                amount
            );
        }
        for amount in ["-1.5", "1.12345678", ".5", ""] {
            assert!(StellarService::xlm_to_stroops(amount).is_err());
            assert!(crate::validation::amount::xlm_to_stroops_str(amount).is_err());
        }
    }
}
