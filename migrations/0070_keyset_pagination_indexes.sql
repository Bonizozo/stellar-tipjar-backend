-- Composite indexes matching stable keyset order (created_at, id) for hot list endpoints.
CREATE INDEX IF NOT EXISTS idx_tips_created_at_id_desc ON tips (created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_tips_creator_created_at_id_desc ON tips (creator_username, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_creators_created_at_id_desc ON creators (created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_leaderboard_snapshot_at_id_desc ON leaderboard_snapshots (snapshot_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_comments_created_at_id_desc ON comments (created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_comments_tip_created_at_id_desc ON comments (tip_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at_id_desc ON event_audit_logs (created_at DESC, id DESC);
-- Optional hot transaction/audit-style tables may not exist in older installs.
DO $$
BEGIN
    IF to_regclass('public.indexer_transactions') IS NOT NULL THEN
        EXECUTE 'CREATE INDEX IF NOT EXISTS idx_indexer_transactions_created_at_id_desc ON indexer_transactions (created_at DESC, id DESC)';
    END IF;
    IF to_regclass('public.tx_pool') IS NOT NULL THEN
        EXECUTE 'CREATE INDEX IF NOT EXISTS idx_tx_pool_created_at_id_desc ON tx_pool (created_at DESC, id DESC)';
    END IF;
END $$;
