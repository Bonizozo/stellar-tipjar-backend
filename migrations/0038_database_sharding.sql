-- Create shard_mapping table for directory-based sharding
CREATE TABLE IF NOT EXISTS shard_mapping (
    id UUID PRIMARY KEY,
    entity_type VARCHAR(50) NOT NULL,
    entity_id UUID NOT NULL,
    shard_id INTEGER NOT NULL,
    created_at TIMESTAMP NOT NULL,
    UNIQUE(entity_type, entity_id)
);

-- Create shard_metadata table for tracking shard state
CREATE TABLE IF NOT EXISTS shard_metadata (
    shard_id INTEGER PRIMARY KEY,
    host VARCHAR(255) NOT NULL,
    port INTEGER NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    creator_count BIGINT NOT NULL DEFAULT 0,
    last_rebalance TIMESTAMP,
    created_at TIMESTAMP NOT NULL
);

-- Create shard_migration_log for tracking data migrations
CREATE TABLE IF NOT EXISTS shard_migration_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_shard INTEGER NOT NULL,
    to_shard INTEGER NOT NULL,
    entity_type VARCHAR(50) NOT NULL,
    migrated_count INTEGER NOT NULL,
    failed_count INTEGER NOT NULL,
    started_at TIMESTAMP NOT NULL,
    completed_at TIMESTAMP,
    status VARCHAR(50) NOT NULL DEFAULT 'in_progress'
);

-- Create indexes for shard queries
CREATE INDEX IF NOT EXISTS idx_shard_mapping_entity ON shard_mapping(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_shard_mapping_shard_id ON shard_mapping(shard_id);
CREATE INDEX IF NOT EXISTS idx_shard_metadata_status ON shard_metadata(status);
CREATE INDEX IF NOT EXISTS idx_shard_migration_log_status ON shard_migration_log(status);
