-- Rollback database sharding
DROP INDEX IF EXISTS idx_tips_shard_key;
ALTER TABLE tips DROP COLUMN IF EXISTS shard_key;

DROP INDEX IF EXISTS idx_shard_stats_recorded_at;
DROP INDEX IF EXISTS idx_shard_stats_shard_id;
DROP TABLE IF EXISTS shard_stats;
DROP TABLE IF EXISTS shard_info;
