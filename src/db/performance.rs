use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Per-query aggregated statistics.
pub struct QueryStats {
    pub count: u64,
    pub total_duration: Duration,
    pub max_duration: Duration,
    /// Timestamp (unix seconds) of the most recent execution.
    pub last_seen: u64,
    /// Whether this query pattern was flagged as part of an N+1 sequence.
    pub n_plus_one_suspect: bool,
}

/// A single recorded query execution — kept for N+1 detection.
struct QueryRecord {
    pattern: String,
    executed_at: Instant,
}

pub struct PerformanceMonitor {
    stats: Mutex<HashMap<String, QueryStats>>,
    /// Ring-buffer of recent executions used for N+1 detection.
    recent: Mutex<Vec<QueryRecord>>,
}

/// How many milliseconds a query must exceed to be considered "slow".
const SLOW_THRESHOLD_MS: u64 = 200;
/// Window (ms) within which repeated identical patterns are flagged as N+1.
const N_PLUS_ONE_WINDOW_MS: u128 = 100;
/// Minimum repetitions within the window to flag N+1.
const N_PLUS_ONE_MIN_COUNT: usize = 5;
/// Max entries kept in the recent ring-buffer.
const RECENT_BUFFER_SIZE: usize = 500;

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self {
            stats: Mutex::new(HashMap::new()),
            recent: Mutex::new(Vec::with_capacity(RECENT_BUFFER_SIZE)),
        }
    }

    /// Normalize a raw SQL string into a stable pattern (first 10 tokens).
    fn normalize(query: &str) -> String {
        query
            .trim()
            .split_whitespace()
            .take(10)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Record a query execution and update all profiling state.
    pub fn track_query(&self, query: &str, duration: Duration) {
        let pattern = Self::normalize(query);
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // --- update aggregate stats ---
        {
            let mut stats = self.stats.lock().unwrap();
            let entry = stats.entry(pattern.clone()).or_insert(QueryStats {
                count: 0,
                total_duration: Duration::ZERO,
                max_duration: Duration::ZERO,
                last_seen: now_unix,
                n_plus_one_suspect: false,
            });
            entry.count += 1;
            entry.total_duration += duration;
            if duration > entry.max_duration {
                entry.max_duration = duration;
            }
            entry.last_seen = now_unix;
        }

        // --- N+1 detection ---
        let now = Instant::now();
        {
            let mut recent = self.recent.lock().unwrap();
            // Evict old entries beyond the ring-buffer size.
            if recent.len() >= RECENT_BUFFER_SIZE {
                recent.drain(0..RECENT_BUFFER_SIZE / 2);
            }
            recent.push(QueryRecord {
                pattern: pattern.clone(),
                executed_at: now,
            });

            // Count how many times this pattern appeared in the last window.
            let count_in_window = recent
                .iter()
                .rev()
                .take_while(|r| now.duration_since(r.executed_at).as_millis() < N_PLUS_ONE_WINDOW_MS)
                .filter(|r| r.pattern == pattern)
                .count();

            if count_in_window >= N_PLUS_ONE_MIN_COUNT {
                let mut stats = self.stats.lock().unwrap();
                if let Some(entry) = stats.get_mut(&pattern) {
                    entry.n_plus_one_suspect = true;
                }
                tracing::warn!(
                    target: "query_profiling",
                    pattern = %pattern,
                    count = count_in_window,
                    "Potential N+1 query detected"
                );
            }
        }

        // --- slow query log ---
        let ms = duration.as_millis();
        if ms >= SLOW_THRESHOLD_MS as u128 {
            tracing::warn!(
                target: "query_profiling",
                duration_ms = ms,
                query = %pattern,
                "Slow query detected"
            );
        } else {
            tracing::debug!(
                target: "query_profiling",
                duration_ms = ms,
                query = %pattern,
                "Query executed"
            );
        }
    }

    /// Returns aggregated stats: (count, avg_ms, max_ms, last_seen, n_plus_one_suspect).
    pub fn get_stats(&self) -> HashMap<String, (u64, f64, u64, u64, bool)> {
        let stats = self.stats.lock().unwrap();
        stats
            .iter()
            .map(|(k, v)| {
                let avg = v.total_duration.as_secs_f64() / v.count as f64 * 1000.0;
                (
                    k.clone(),
                    (
                        v.count,
                        avg,
                        v.max_duration.as_millis() as u64,
                        v.last_seen,
                        v.n_plus_one_suspect,
                    ),
                )
            })
            .collect()
    }

    /// Returns only queries flagged as N+1 suspects.
    pub fn get_n_plus_one_suspects(&self) -> Vec<String> {
        let stats = self.stats.lock().unwrap();
        stats
            .iter()
            .filter(|(_, v)| v.n_plus_one_suspect)
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Returns queries whose max execution time exceeds `threshold_ms`.
    pub fn get_slow_queries(&self, threshold_ms: u64) -> Vec<(String, u64, f64, u64)> {
        let stats = self.stats.lock().unwrap();
        let mut slow: Vec<_> = stats
            .iter()
            .filter(|(_, v)| v.max_duration.as_millis() as u64 >= threshold_ms)
            .map(|(k, v)| {
                let avg = v.total_duration.as_secs_f64() / v.count as f64 * 1000.0;
                (k.clone(), v.count, avg, v.max_duration.as_millis() as u64)
            })
            .collect();
        slow.sort_by(|a, b| b.3.cmp(&a.3));
        slow
    }

    /// Generates a plain-text optimization report.
    pub fn generate_report(&self) -> String {
        let stats = self.stats.lock().unwrap();
        let total_queries: u64 = stats.values().map(|v| v.count).sum();
        let slow_count = stats
            .values()
            .filter(|v| v.max_duration.as_millis() as u64 >= SLOW_THRESHOLD_MS)
            .count();
        let n1_count = stats.values().filter(|v| v.n_plus_one_suspect).count();

        let mut lines = vec![
            "=== Database Query Profiling Report ===".to_string(),
            format!("Total query patterns tracked : {}", stats.len()),
            format!("Total executions             : {}", total_queries),
            format!("Slow queries (>{}ms)         : {}", SLOW_THRESHOLD_MS, slow_count),
            format!("N+1 suspects                 : {}", n1_count),
            String::new(),
            "--- Top 10 Slowest Queries ---".to_string(),
        ];

        let mut sorted: Vec<_> = stats.iter().collect();
        sorted.sort_by(|a, b| b.1.max_duration.cmp(&a.1.max_duration));
        for (pattern, v) in sorted.iter().take(10) {
            let avg = v.total_duration.as_secs_f64() / v.count as f64 * 1000.0;
            lines.push(format!(
                "  [{:>6}ms max | {:>7.1}ms avg | {:>5}x] {}{}",
                v.max_duration.as_millis(),
                avg,
                v.count,
                if v.n_plus_one_suspect { "[N+1] " } else { "" },
                pattern
            ));
        }

        if n1_count > 0 {
            lines.push(String::new());
            lines.push("--- N+1 Query Suspects ---".to_string());
            for (pattern, v) in stats.iter().filter(|(_, v)| v.n_plus_one_suspect) {
                lines.push(format!("  {} ({}x)", pattern, v.count));
            }
            lines.push(String::new());
            lines.push("  Recommendation: Use JOIN or batch loading (e.g. SELECT ... WHERE id = ANY($1)) to eliminate N+1 patterns.".to_string());
        }

        lines.join("\n")
    }
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tracking() {
        let m = PerformanceMonitor::new();
        m.track_query("SELECT * FROM users WHERE id = $1", Duration::from_millis(10));
        m.track_query("SELECT * FROM users WHERE id = $1", Duration::from_millis(20));
        let stats = m.get_stats();
        let (count, avg, max, _, _) = stats["SELECT * FROM users WHERE id = $1"];
        assert_eq!(count, 2);
        assert_eq!(avg, 15.0);
        assert_eq!(max, 20);
    }

    #[test]
    fn test_slow_query_flagged() {
        let m = PerformanceMonitor::new();
        m.track_query("SELECT * FROM tips", Duration::from_millis(500));
        let slow = m.get_slow_queries(200);
        assert_eq!(slow.len(), 1);
        assert_eq!(slow[0].3, 500);
    }

    #[test]
    fn test_report_generation() {
        let m = PerformanceMonitor::new();
        m.track_query("SELECT 1", Duration::from_millis(5));
        m.track_query("SELECT * FROM tips", Duration::from_millis(300));
        let report = m.generate_report();
        assert!(report.contains("Total query patterns tracked"));
        assert!(report.contains("Slow queries"));
    }
}
