use chrono::{DateTime, Duration as ChronoDuration, Utc};

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub key: String,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub access_count: u64,
    pub ttl: ChronoDuration,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum EvictionStrategy {
    LRU,
    LFU,
    TTL,
    Adaptive,
}

#[derive(Debug, Clone)]
pub struct EvictionPolicy {
    pub strategy: EvictionStrategy,
    pub max_entries: usize,
    pub lru_max_idle: ChronoDuration,
    pub min_access_count: u64,
}

impl Default for EvictionPolicy {
    fn default() -> Self {
        Self {
            strategy: EvictionStrategy::Adaptive,
            max_entries: 10_000,
            lru_max_idle: ChronoDuration::hours(1),
            min_access_count: 5,
        }
    }
}

impl EvictionPolicy {
    pub fn should_evict(&self, entry: &CacheEntry) -> bool {
        match self.strategy {
            EvictionStrategy::LRU => entry.last_accessed < Utc::now() - self.lru_max_idle,
            EvictionStrategy::LFU => entry.access_count < self.min_access_count,
            EvictionStrategy::TTL => entry.created_at + entry.ttl < Utc::now(),
            EvictionStrategy::Adaptive => self.adaptive_eviction(entry),
        }
    }

    pub fn select_victims(&self, entries: &[CacheEntry], count: usize) -> Vec<String> {
        let mut ordered = entries.to_vec();
        match self.strategy {
            EvictionStrategy::LRU => ordered.sort_by_key(|e| e.last_accessed),
            EvictionStrategy::LFU => ordered.sort_by_key(|e| e.access_count),
            EvictionStrategy::TTL => ordered.sort_by_key(|e| e.created_at + e.ttl),
            EvictionStrategy::Adaptive => ordered.sort_by_key(|e| {
                (
                    e.access_count,
                    e.last_accessed,
                    std::cmp::Reverse(e.size_bytes),
                )
            }),
        }

        ordered
            .into_iter()
            .take(count)
            .map(|e| e.key)
            .collect::<Vec<_>>()
    }

    fn adaptive_eviction(&self, entry: &CacheEntry) -> bool {
        let ttl_expired = entry.created_at + entry.ttl < Utc::now();
        let stale = entry.last_accessed < Utc::now() - ChronoDuration::minutes(30);
        let low_value = entry.access_count < self.min_access_count;
        ttl_expired || (stale && low_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(access_count: u64, last_minutes_ago: i64, ttl_minutes: i64) -> CacheEntry {
        let now = Utc::now();
        CacheEntry {
            key: "k".to_string(),
            created_at: now - ChronoDuration::minutes(90),
            last_accessed: now - ChronoDuration::minutes(last_minutes_ago),
            access_count,
            ttl: ChronoDuration::minutes(ttl_minutes),
            size_bytes: 64,
        }
    }

    #[test]
    fn ttl_policy_evicts_expired_entries() {
        let policy = EvictionPolicy {
            strategy: EvictionStrategy::TTL,
            ..EvictionPolicy::default()
        };
        assert!(policy.should_evict(&entry(10, 2, 30)));
    }

    #[test]
    fn lfu_policy_evicts_low_frequency_entries() {
        let policy = EvictionPolicy {
            strategy: EvictionStrategy::LFU,
            min_access_count: 5,
            ..EvictionPolicy::default()
        };
        assert!(policy.should_evict(&entry(2, 2, 300)));
    }
}
