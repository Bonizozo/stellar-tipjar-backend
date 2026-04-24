-- Materialized view for leaderboard queries (refreshed by background job)
CREATE TABLE IF NOT EXISTS leaderboard_snapshots (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    period      TEXT NOT NULL,          -- 'daily' | 'weekly' | 'monthly' | 'all_time'
    board_type  TEXT NOT NULL,          -- 'top_creators' | 'top_tippers' | 'trending'
    rank        INT  NOT NULL,
    username    TEXT NOT NULL,
    score       NUMERIC(20, 7) NOT NULL,
    tip_count   INT  NOT NULL DEFAULT 0,
    snapshot_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (period, board_type, rank)
);

CREATE INDEX IF NOT EXISTS idx_leaderboard_period_type ON leaderboard_snapshots (period, board_type);
CREATE INDEX IF NOT EXISTS idx_leaderboard_snapshot_at ON leaderboard_snapshots (snapshot_at);
