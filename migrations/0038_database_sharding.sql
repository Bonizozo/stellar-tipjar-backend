-- Create sharding metadata tables
CREATE TABLE IF NOT EXISTS shard_info (
    shard_id SMALLINT PRIMARY KEY,
    min_key BIGINT NOT NULL,
    max_key BIGINT NOT NULL,
    connection_string VARCHAR(500) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS shard_stats (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    shard_id SMALLINT NOT NULL REFERENCES shard_info(shard_id),
    row_count BIGINT DEFAULT 0,
    size_bytes BIGINT DEFAULT 0,
    recorded_at TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE INDEX idx_shard_stats_shard_id ON shard_stats(shard_id);
CREATE INDEX idx_shard_stats_recorded_at ON shard_stats(recorded_at);

-- Add shard_key to tips table for sharding
ALTER TABLE tips ADD COLUMN IF NOT EXISTS shard_key VARCHAR(255);
CREATE INDEX idx_tips_shard_key ON tips(shard_key);
