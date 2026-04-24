use redis::AsyncCommands;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Exchange rates relative to USD (1 USD = N <currency>).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeRates {
    /// Base currency (always "USD").
    pub base: String,
    /// Map of currency code → rate vs USD.
    pub rates: HashMap<String, f64>,
    /// Unix timestamp of when rates were fetched.
    pub fetched_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConversionResult {
    pub from_currency: String,
    pub to_currency: String,
    pub from_amount: f64,
    pub to_amount: f64,
    pub rate: f64,
}

// ---------------------------------------------------------------------------
// In-process cache (fallback when Redis is unavailable)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct CurrencyService {
    client: Client,
    /// API key for exchangerate.host (free tier, no key needed) or Open Exchange Rates.
    api_key: Option<String>,
    /// In-memory fallback cache.
    memory_cache: Arc<RwLock<Option<ExchangeRates>>>,
}

const REDIS_KEY: &str = "currency:exchange_rates";
const CACHE_TTL_SECS: u64 = 3600; // 1 hour

impl CurrencyService {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("reqwest client"),
            api_key: std::env::var("EXCHANGE_RATE_API_KEY").ok(),
            memory_cache: Arc::new(RwLock::new(None)),
        }
    }

    // -----------------------------------------------------------------------
    // Fetch from external API
    // -----------------------------------------------------------------------

    /// Fetch fresh rates from exchangerate.host (free, no key required).
    /// Falls back to Open Exchange Rates if `EXCHANGE_RATE_API_KEY` is set.
    async fn fetch_from_api(&self) -> anyhow::Result<ExchangeRates> {
        let url = if let Some(key) = &self.api_key {
            format!(
                "https://openexchangerates.org/api/latest.json?app_id={}&base=USD",
                key
            )
        } else {
            "https://api.exchangerate.host/latest?base=USD".to_string()
        };

        #[derive(Deserialize)]
        struct ApiResponse {
            rates: HashMap<String, f64>,
        }

        let resp: ApiResponse = self.client.get(&url).send().await?.json().await?;

        Ok(ExchangeRates {
            base: "USD".to_string(),
            rates: resp.rates,
            fetched_at: chrono::Utc::now().timestamp(),
        })
    }

    // -----------------------------------------------------------------------
    // Cache helpers
    // -----------------------------------------------------------------------

    async fn get_from_redis(
        &self,
        redis: &mut redis::aio::ConnectionManager,
    ) -> Option<ExchangeRates> {
        match redis.get::<_, String>(REDIS_KEY).await {
            Ok(raw) => serde_json::from_str(&raw).ok(),
            Err(_) => None,
        }
    }

    async fn set_in_redis(
        &self,
        redis: &mut redis::aio::ConnectionManager,
        rates: &ExchangeRates,
    ) {
        if let Ok(raw) = serde_json::to_string(rates) {
            let _ = redis
                .set_ex::<_, _, ()>(REDIS_KEY, raw, CACHE_TTL_SECS)
                .await;
        }
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Get exchange rates, using Redis → memory cache → API in that order.
    pub async fn get_rates(
        &self,
        redis: Option<&mut redis::aio::ConnectionManager>,
    ) -> anyhow::Result<ExchangeRates> {
        // 1. Try Redis.
        if let Some(conn) = redis {
            if let Some(cached) = self.get_from_redis(conn).await {
                tracing::debug!("Exchange rates served from Redis cache");
                return Ok(cached);
            }
        }

        // 2. Try in-memory cache.
        {
            let guard = self.memory_cache.read().await;
            if let Some(cached) = guard.as_ref() {
                let age = chrono::Utc::now().timestamp() - cached.fetched_at;
                if age < CACHE_TTL_SECS as i64 {
                    tracing::debug!("Exchange rates served from memory cache");
                    return Ok(cached.clone());
                }
            }
        }

        // 3. Fetch fresh.
        let rates = self.fetch_from_api().await?;
        tracing::info!("Fetched fresh exchange rates ({} currencies)", rates.rates.len());

        // Update memory cache.
        *self.memory_cache.write().await = Some(rates.clone());

        Ok(rates)
    }

    /// Refresh rates and store in Redis. Called by the background task.
    pub async fn refresh(
        &self,
        redis: Option<&mut redis::aio::ConnectionManager>,
    ) -> anyhow::Result<ExchangeRates> {
        let rates = self.fetch_from_api().await?;
        tracing::info!(
            currencies = rates.rates.len(),
            "Exchange rates refreshed"
        );

        *self.memory_cache.write().await = Some(rates.clone());

        if let Some(conn) = redis {
            self.set_in_redis(conn, &rates).await;
        }

        Ok(rates)
    }

    /// Convert `amount` from `from_currency` to `to_currency`.
    /// All conversions go through USD as the base.
    pub fn convert(
        &self,
        rates: &ExchangeRates,
        from_currency: &str,
        to_currency: &str,
        amount: f64,
    ) -> anyhow::Result<ConversionResult> {
        let from_upper = from_currency.to_uppercase();
        let to_upper = to_currency.to_uppercase();

        // Amount in USD.
        let usd_amount = if from_upper == "USD" {
            amount
        } else {
            let from_rate = rates
                .rates
                .get(&from_upper)
                .ok_or_else(|| anyhow::anyhow!("Unknown currency: {}", from_upper))?;
            amount / from_rate
        };

        let to_amount = if to_upper == "USD" {
            usd_amount
        } else {
            let to_rate = rates
                .rates
                .get(&to_upper)
                .ok_or_else(|| anyhow::anyhow!("Unknown currency: {}", to_upper))?;
            usd_amount * to_rate
        };

        // Effective rate: how many `to_currency` per 1 `from_currency`.
        let rate = to_amount / amount;

        Ok(ConversionResult {
            from_currency: from_upper,
            to_currency: to_upper,
            from_amount: amount,
            to_amount,
            rate,
        })
    }
}

impl Default for CurrencyService {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Background refresh task
// ---------------------------------------------------------------------------

/// Spawn a background task that refreshes exchange rates every `interval`.
/// Requires a `CurrencyService` and an optional Redis connection.
pub fn spawn_refresh_task(
    service: CurrencyService,
    redis: Option<redis::aio::ConnectionManager>,
    interval: Duration,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await; // skip first immediate tick
        loop {
            ticker.tick().await;
            let mut conn = redis.clone();
            if let Err(e) = service.refresh(conn.as_mut()).await {
                tracing::warn!("Exchange rate refresh failed: {}", e);
            }
        }
    });
}
