-- Add schema version column to events table
ALTER TABLE events ADD COLUMN IF NOT EXISTS version INTEGER NOT NULL DEFAULT 1;

-- Snapshot table for aggregate state at a given sequence number
CREATE TABLE IF NOT EXISTS event_snapshots (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    aggregate_id    UUID        NOT NULL,
    sequence_number BIGINT      NOT NULL,
    snapshot_data   JSONB       NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (aggregate_id, sequence_number)
);

CREATE INDEX IF NOT EXISTS idx_snapshots_aggregate_id ON event_snapshots (aggregate_id);
