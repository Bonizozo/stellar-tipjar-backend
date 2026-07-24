//! Atomic, distributed rate limiting via the Generic Cell Rate Algorithm (GCRA),
//! executed as a single Lua script inside Redis.
//!
//! GCRA folds both a sustained rate and a burst allowance into one rolling
//! "theoretical arrival time" (TAT) value per key, so a single `GET` + `SET`
//! (done atomically inside the script) replaces the previous two-round-trip
//! approach of a separate burst counter and an approximated sliding window.
//! Because the read-check-write happens inside one `EVAL`/`EVALSHA` call,
//! Redis executes it as an atomic unit — there is no window in which two
//! concurrent callers can both observe capacity and both be admitted.

use redis::{aio::ConnectionManager, RedisError, Script};

/// Outcome of a single GCRA admission check.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GcraDecision {
    /// Whether the request is admitted.
    pub allowed: bool,
    /// Requests still available in the current burst allowance.
    pub remaining: i64,
    /// The configured limit (requests per period), echoed back for headers.
    pub limit: i64,
    /// Seconds until the limiter fully drains back to `limit` remaining.
    pub reset_after_secs: u64,
    /// Seconds the caller should wait before retrying. Zero when admitted.
    pub retry_after_secs: u64,
}

/// KEYS[1] = the per-client TAT key
/// ARGV[1] = now_ms
/// ARGV[2] = emission_interval_ms (period_ms / limit)
/// ARGV[3] = burst (delay-variation tolerance, in number of requests)
/// ARGV[4] = ttl_ms (key expiry so idle clients don't leak memory forever)
///
/// Returns: { allowed(0/1), remaining, retry_after_ms, reset_after_ms }
const GCRA_SCRIPT: &str = r#"
local tat = tonumber(redis.call('GET', KEYS[1]))
local now = tonumber(ARGV[1])
local emission_interval = tonumber(ARGV[2])
local burst = tonumber(ARGV[3])
local ttl = tonumber(ARGV[4])

-- burst_offset is (burst - 1) emission intervals, not `burst`: the first
-- request of a burst always costs exactly one emission interval with zero
-- slack, so tolerating (burst - 1) additional intervals of "running ahead"
-- admits exactly `burst` total requests before the next one is rejected.
local burst_offset = emission_interval * (burst - 1)

if tat == nil then
  tat = now
end
if tat < now then
  tat = now
end

local separation = tat - now

if separation > burst_offset then
  -- Reject: not enough of the burst allowance has drained yet.
  -- Do NOT advance tat — a rejected request must not consume capacity.
  local retry_after_ms = separation - burst_offset
  return {0, 0, retry_after_ms, tat - now}
end

local new_tat = tat + emission_interval
redis.call('SET', KEYS[1], new_tat, 'PX', ttl)

-- +1 because `new_separation` already reflects the slot this call just
-- consumed; the boundary case (new_separation == burst_offset) still has
-- exactly one more slot free, not zero.
local new_separation = new_tat - now
local remaining = math.floor((burst_offset - new_separation) / emission_interval) + 1
if remaining < 0 then
  remaining = 0
end

return {1, remaining, 0, new_separation}
"#;

/// Wraps a compiled `Script` so the SHA is cached and `EVALSHA` is used on
/// every call after the first (with automatic fallback to `EVAL` if Redis
/// evicted the script, e.g. after a `SCRIPT FLUSH` or failover to a fresh
/// replica) — this is handled transparently by `redis::Script`.
pub struct GcraLimiter {
    script: Script,
}

impl Default for GcraLimiter {
    fn default() -> Self {
        Self {
            script: Script::new(GCRA_SCRIPT),
        }
    }
}

impl GcraLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run one GCRA admission check for `key`.
    ///
    /// * `limit` — requests allowed per `period_secs`.
    /// * `burst` — additional requests allowed to arrive back-to-back on top
    ///   of the steady rate (delay-variation tolerance).
    /// * `period_secs` — the window the `limit` applies over (e.g. 60 for
    ///   "requests per minute").
    pub async fn check(
        &self,
        conn: &mut ConnectionManager,
        key: &str,
        limit: u64,
        burst: u64,
        period_secs: u64,
        now_ms: i64,
    ) -> Result<GcraDecision, RedisError> {
        let limit = limit.max(1);
        let burst = burst.max(1);
        let period_ms = (period_secs.max(1) * 1000) as i64;
        let emission_interval_ms = (period_ms as f64 / limit as f64).ceil() as i64;
        // Keep the key around for a full burst window past the last hit so
        // idle clients don't linger in Redis forever.
        let ttl_ms = period_ms + emission_interval_ms * burst as i64 + 1000;

        let (allowed, remaining, retry_after_ms, reset_after_ms): (i64, i64, i64, i64) = self
            .script
            .key(key)
            .arg(now_ms)
            .arg(emission_interval_ms)
            .arg(burst)
            .arg(ttl_ms)
            .invoke_async(conn)
            .await?;

        Ok(GcraDecision {
            allowed: allowed == 1,
            remaining,
            limit: limit as i64,
            reset_after_secs: ms_to_secs_ceil(reset_after_ms),
            retry_after_secs: ms_to_secs_ceil(retry_after_ms),
        })
    }
}

fn ms_to_secs_ceil(ms: i64) -> u64 {
    if ms <= 0 {
        0
    } else {
        ((ms as u64) + 999) / 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pure-Rust mirror of the Lua script's arithmetic, used to unit-test the
    /// GCRA math (burst, sustained rate, window rollover) without requiring a
    /// live Redis instance. The Redis-backed concurrency/atomicity behavior
    /// itself is covered separately in `tests/gcra_redis_test.rs`.
    fn gcra_step(
        tat: Option<i64>,
        now: i64,
        emission_interval: i64,
        burst: i64,
    ) -> (bool, i64, i64) {
        let burst_offset = emission_interval * (burst - 1);
        let mut tat = tat.unwrap_or(now);
        if tat < now {
            tat = now;
        }
        let separation = tat - now;
        if separation > burst_offset {
            return (false, tat, separation - burst_offset);
        }
        let new_tat = tat + emission_interval;
        let new_separation = new_tat - now;
        let remaining = (((burst_offset - new_separation) / emission_interval) + 1).max(0);
        (true, new_tat, remaining)
    }

    #[test]
    fn allows_full_burst_back_to_back() {
        // 10 req/s => emission_interval = 100ms, burst of 5.
        let emission_interval = 100;
        let burst = 5;
        let mut tat: Option<i64> = None;
        let now = 0;

        for i in 0..5 {
            let (allowed, new_tat, _remaining) = gcra_step(tat, now, emission_interval, burst);
            assert!(allowed, "request {} within burst should be allowed", i);
            tat = Some(new_tat);
        }

        // The 6th immediate request exceeds the burst allowance.
        let (allowed, _, retry_after) = gcra_step(tat, now, emission_interval, burst);
        assert!(!allowed, "request beyond burst should be rejected");
        assert!(retry_after > 0);
    }

    #[test]
    fn sustained_rate_is_enforced_after_burst_drains() {
        let emission_interval = 100; // 10 req/s
        let burst = 1;
        let mut tat: Option<i64> = None;

        // First request at t=0 consumes the single burst slot.
        let (allowed, new_tat, _) = gcra_step(tat, 0, emission_interval, burst);
        assert!(allowed);
        tat = Some(new_tat);

        // Immediately retrying at t=0 must be rejected (burst exhausted).
        let (allowed, _, _) = gcra_step(tat, 0, emission_interval, burst);
        assert!(!allowed);

        // Waiting a full emission interval (100ms) should admit exactly one more.
        let (allowed, _, _) = gcra_step(tat, 100, emission_interval, burst);
        assert!(allowed);
    }

    #[test]
    fn window_rollover_recovers_capacity_over_time() {
        let emission_interval = 100;
        let burst = 5;
        let mut tat: Option<i64> = None;
        let now = 0;

        for _ in 0..5 {
            let (allowed, new_tat, _) = gcra_step(tat, now, emission_interval, burst);
            assert!(allowed);
            tat = Some(new_tat);
        }

        // After waiting the full burst_offset (5 * 100ms = 500ms), the
        // limiter should have fully recovered and allow another full burst.
        let recovered_now = 500;
        for i in 0..5 {
            let (allowed, new_tat, _) = gcra_step(tat, recovered_now, emission_interval, burst);
            assert!(allowed, "recovered request {} should be allowed", i);
            tat = Some(new_tat);
        }
    }

    #[test]
    fn rejected_requests_do_not_consume_capacity() {
        let emission_interval = 100;
        let burst = 1;
        let mut tat: Option<i64> = None;
        let now = 0;

        let (allowed, new_tat, _) = gcra_step(tat, now, emission_interval, burst);
        assert!(allowed);
        tat = Some(new_tat);

        // Two rapid, rejected retries.
        for _ in 0..2 {
            let (allowed, new_tat, _) = gcra_step(tat, now, emission_interval, burst);
            assert!(!allowed);
            tat = Some(new_tat); // tat is unchanged by a rejection
        }

        // A single emission interval later, exactly one request is admitted —
        // proving the earlier rejections never ate into capacity.
        let (allowed, new_tat, _) =
            gcra_step(tat, now + emission_interval, emission_interval, burst);
        assert!(allowed);
        tat = Some(new_tat);
        let (allowed, _, _) = gcra_step(tat, now + emission_interval, emission_interval, burst);
        assert!(!allowed);
    }

    #[test]
    fn ms_to_secs_ceil_rounds_up() {
        assert_eq!(ms_to_secs_ceil(0), 0);
        assert_eq!(ms_to_secs_ceil(1), 1);
        assert_eq!(ms_to_secs_ceil(1000), 1);
        assert_eq!(ms_to_secs_ceil(1001), 2);
    }
}
