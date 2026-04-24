ALTER TABLE tips DROP COLUMN IF EXISTS indexed_at;
ALTER TABLE tips DROP COLUMN IF EXISTS confirmations;
DROP INDEX IF EXISTS idx_tips_confirmations;
DROP INDEX IF EXISTS idx_indexed_events_created_at;
DROP INDEX IF EXISTS idx_indexed_events_ledger;
DROP INDEX IF EXISTS idx_indexed_events_event_id;
DROP TABLE IF EXISTS indexer_state;
DROP TABLE IF EXISTS indexed_events;
