use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShardingStrategy {
    Range,
    Hash,
    Directory,
}

pub struct ShardKey {
    pub key: String,
    pub shard_id: u32,
}

impl ShardKey {
    pub fn from_tenant_id(tenant_id: Uuid, num_shards: u32) -> Self {
        let hash = Self::hash_value(&tenant_id.to_string(), num_shards);
        Self {
            key: tenant_id.to_string(),
            shard_id: hash,
        }
    }

    pub fn from_creator_id(creator_id: Uuid, num_shards: u32) -> Self {
        let hash = Self::hash_value(&creator_id.to_string(), num_shards);
        Self {
            key: creator_id.to_string(),
            shard_id: hash,
        }
    }

    fn hash_value(value: &str, num_shards: u32) -> u32 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};
        value.hash(&mut hasher);
        (hasher.finish() % num_shards as u64) as u32
    }
}

pub struct RangeShardingStrategy {
    pub shard_ranges: Vec<(u64, u64)>,
}

impl RangeShardingStrategy {
    pub fn new(num_shards: u32) -> Self {
        let range_size = u64::MAX / num_shards as u64;
        let mut ranges = Vec::new();
        
        for i in 0..num_shards {
            let start = i as u64 * range_size;
            let end = if i == num_shards - 1 {
                u64::MAX
            } else {
                (i as u64 + 1) * range_size - 1
            };
            ranges.push((start, end));
        }
        
        Self {
            shard_ranges: ranges,
        }
    }

    pub fn get_shard_id(&self, value: u64) -> u32 {
        for (i, (start, end)) in self.shard_ranges.iter().enumerate() {
            if value >= *start && value <= *end {
                return i as u32;
            }
        }
        0
    }
}
