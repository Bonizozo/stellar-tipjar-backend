use std::hash::{Hash, Hasher};
use uuid::Uuid;

// ── Shard key ─────────────────────────────────────────────────────────────────

/// The resolved shard assignment for a given logical key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardKey {
    /// The raw string value that was hashed (for logging / debugging).
    pub raw: String,
    /// The resolved shard index (0-based, always < `num_shards`).
    pub shard_id: u32,
}

impl ShardKey {
    /// Shard by creator username (primary sharding key for tips).
    pub fn from_creator_username(username: &str, num_shards: u32) -> Self {
        let shard_id = fnv1a_shard(username, num_shards);
        Self {
            raw: username.to_string(),
            shard_id,
        }
    }

    /// Shard by UUID (creator_id, tip_id, etc.).
    pub fn from_uuid(id: Uuid, num_shards: u32) -> Self {
        // Use the lower 64 bits of the UUID for a stable, fast hash.
        let bytes = id.as_bytes();
        let low = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let shard_id = (low % num_shards as u64) as u32;
        Self {
            raw: id.to_string(),
            shard_id,
        }
    }

    /// Shard by an arbitrary string key.
    pub fn from_str(key: &str, num_shards: u32) -> Self {
        let shard_id = fnv1a_shard(key, num_shards);
        Self {
            raw: key.to_string(),
            shard_id,
        }
    }
}

/// FNV-1a 64-bit hash, reduced to a shard index.
///
/// FNV-1a is deterministic across processes and platforms, unlike
/// `std::collections::hash_map::DefaultHasher` which is randomised.
pub fn fnv1a_shard(key: &str, num_shards: u32) -> u32 {
    const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
    const FNV_PRIME: u64 = 1_099_511_628_211;

    let mut hash = FNV_OFFSET;
    for byte in key.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    (hash % num_shards as u64) as u32
}

// ── Strategy enum ─────────────────────────────────────────────────────────────

/// The sharding algorithm to use when routing a key to a shard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShardingStrategy {
    /// Consistent hash — distributes keys uniformly across shards.
    /// Best for write-heavy workloads with no natural range locality.
    Hash,
    /// Range — each shard owns a contiguous range of the key space.
    /// Best when range scans are common (e.g. time-series data).
    Range,
    /// Directory — an explicit lookup table maps keys to shards.
    /// Best when you need manual control over placement.
    Directory,
}

impl Default for ShardingStrategy {
    fn default() -> Self {
        Self::Hash
    }
}

// ── Range strategy ────────────────────────────────────────────────────────────

/// Divides the u64 key space into equal-width ranges, one per shard.
#[derive(Debug, Clone)]
pub struct RangeShardingStrategy {
    /// `(inclusive_start, inclusive_end)` for each shard, indexed by shard_id.
    pub ranges: Vec<(u64, u64)>,
}

impl RangeShardingStrategy {
    pub fn new(num_shards: u32) -> Self {
        assert!(num_shards > 0, "num_shards must be > 0");
        let range_size = u64::MAX / num_shards as u64;
        let ranges = (0..num_shards)
            .map(|i| {
                let start = i as u64 * range_size;
                let end = if i == num_shards - 1 {
                    u64::MAX
                } else {
                    (i as u64 + 1) * range_size - 1
                };
                (start, end)
            })
            .collect();
        Self { ranges }
    }

    /// Map a numeric key to a shard id.
    pub fn shard_for(&self, value: u64) -> u32 {
        self.ranges
            .iter()
            .position(|(start, end)| value >= *start && value <= *end)
            .unwrap_or(0) as u32
    }

    /// Map a string key to a shard id via FNV-1a hash.
    pub fn shard_for_str(&self, key: &str) -> u32 {
        let hash = {
            const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
            const FNV_PRIME: u64 = 1_099_511_628_211;
            let mut h = FNV_OFFSET;
            for b in key.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            h
        };
        self.shard_for(hash)
    }
}

// ── Directory strategy ────────────────────────────────────────────────────────

/// Explicit key → shard_id mapping.  Keys not in the directory fall back to
/// hash-based routing.
#[derive(Debug, Clone, Default)]
pub struct DirectoryShardingStrategy {
    directory: std::collections::HashMap<String, u32>,
    num_shards: u32,
}

impl DirectoryShardingStrategy {
    pub fn new(num_shards: u32) -> Self {
        Self {
            directory: std::collections::HashMap::new(),
            num_shards,
        }
    }

    pub fn insert(&mut self, key: impl Into<String>, shard_id: u32) {
        self.directory.insert(key.into(), shard_id);
    }

    pub fn shard_for(&self, key: &str) -> u32 {
        self.directory
            .get(key)
            .copied()
            .unwrap_or_else(|| fnv1a_shard(key, self.num_shards))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_is_deterministic() {
        // Same key must always map to the same shard.
        let a = fnv1a_shard("alice", 4);
        let b = fnv1a_shard("alice", 4);
        assert_eq!(a, b);
    }

    #[test]
    fn fnv1a_distributes_across_shards() {
        let num_shards = 4;
        let keys = ["alice", "bob", "carol", "dave", "eve", "frank", "grace", "heidi"];
        let mut counts = vec![0u32; num_shards as usize];
        for k in &keys {
            counts[fnv1a_shard(k, num_shards) as usize] += 1;
        }
        // Every shard should have at least one key for this set.
        assert!(counts.iter().all(|&c| c > 0));
    }

    #[test]
    fn shard_key_from_uuid_is_stable() {
        let id = Uuid::new_v4();
        let a = ShardKey::from_uuid(id, 8);
        let b = ShardKey::from_uuid(id, 8);
        assert_eq!(a.shard_id, b.shard_id);
    }

    #[test]
    fn range_strategy_covers_full_space() {
        let s = RangeShardingStrategy::new(4);
        assert_eq!(s.shard_for(0), 0);
        assert_eq!(s.shard_for(u64::MAX), 3);
    }

    #[test]
    fn directory_falls_back_to_hash() {
        let mut dir = DirectoryShardingStrategy::new(4);
        dir.insert("pinned_key", 2);
        assert_eq!(dir.shard_for("pinned_key"), 2);
        // Unknown key falls back to hash — just check it's in range.
        let s = dir.shard_for("unknown_key");
        assert!(s < 4);
    }
}
