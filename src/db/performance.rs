use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

pub struct QueryStats {
    pub count: u64,
    pub total_duration: Duration,
    pub max_duration: Duration,
}

pub struct PerformanceMonitor {
    stats: Mutex<HashMap<String, QueryStats>>,
}

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self {
            stats: Mutex::new(HashMap::new()),
        }
    }

    pub fn track_query(&self, query: &str, duration: Duration) {
        // Simple normalization: take the first few words or the whole query string
        let pattern = query
            .trim()
            .split_whitespace()
            .take(10)
            .collect::<Vec<_>>()
            .join(" ");

        let mut stats = self.stats.lock().unwrap();
        let entry = stats.entry(pattern).or_insert(QueryStats {
            count: 0,
            total_duration: Duration::from_secs(0),
            max_duration: Duration::from_secs(0),
        });

        entry.count += 1;
        entry.total_duration += duration;
        if duration > entry.max_duration {
            entry.max_duration = duration;
        }
    }

    pub fn get_stats(&self) -> HashMap<String, (u64, f64, u64)> {
        let stats = self.stats.lock().unwrap();
        stats
            .iter()
            .map(|(k, v)| {
                let avg = v.total_duration.as_secs_f64() / v.count as f64 * 1000.0;
                (k.clone(), (v.count, avg, v.max_duration.as_millis() as u64))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_tracking() {
        let monitor = PerformanceMonitor::new();
        let query = "SELECT * FROM users WHERE id = $1";

        monitor.track_query(query, Duration::from_millis(10));
        monitor.track_query(query, Duration::from_millis(20));
        monitor.track_query("SELECT 1", Duration::from_millis(5));

        let stats = monitor.get_stats();

        // Pattern normalization should make the first query have 2 hits
        let user_query_pattern = "SELECT * FROM users WHERE id = $1";
        assert_eq!(stats.get(user_query_pattern).unwrap().0, 2);
        assert_eq!(stats.get(user_query_pattern).unwrap().1, 15.0); // avg (10+20)/2
        assert_eq!(stats.get(user_query_pattern).unwrap().2, 20); // max

        assert_eq!(stats.get("SELECT 1").unwrap().0, 1);
    }
}
