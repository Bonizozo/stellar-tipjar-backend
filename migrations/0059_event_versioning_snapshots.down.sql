DROP INDEX IF EXISTS idx_snapshots_aggregate_id;
DROP TABLE IF EXISTS event_snapshots;
ALTER TABLE events DROP COLUMN IF EXISTS version;
