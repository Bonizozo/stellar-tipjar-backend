use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A sliding time window that evicts events older than `duration`.
pub struct TimeWindow {
    duration: Duration,
    /// Each entry is (insertion_instant, amount_in_stroops).
    events: VecDeque<(Instant, u64)>,
}

pub struct WindowMetrics {
    pub count: usize,
    pub total_stroops: u64,
    pub avg_stroops: u64,
}

impl TimeWindow {
    pub fn new(duration: Duration) -> Self {
        Self { duration, events: VecDeque::new() }
    }

    pub fn push(&mut self, amount_stroops: u64) {
        self.evict();
        self.events.push_back((Instant::now(), amount_stroops));
    }

    fn evict(&mut self) {
        let cutoff = Instant::now() - self.duration;
        while self.events.front().map_or(false, |(t, _)| *t < cutoff) {
            self.events.pop_front();
        }
    }

    pub fn metrics(&mut self) -> WindowMetrics {
        self.evict();
        let count = self.events.len();
        let total: u64 = self.events.iter().map(|(_, a)| a).sum();
        WindowMetrics {
            count,
            total_stroops: total,
            avg_stroops: if count > 0 { total / count as u64 } else { 0 },
        }
    }
}
