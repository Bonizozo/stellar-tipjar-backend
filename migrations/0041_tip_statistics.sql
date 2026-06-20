CREATE TABLE IF NOT EXISTS tip_daily_stats (
    creator_username TEXT        NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    stat_date        DATE        NOT NULL,
    tip_count        BIGINT      NOT NULL DEFAULT 0,
    total_amount     NUMERIC(20,7) NOT NULL DEFAULT 0,
    avg_amount       NUMERIC(20,7) NOT NULL DEFAULT 0,
    max_amount       NUMERIC(20,7) NOT NULL DEFAULT 0,
    PRIMARY KEY (creator_username, stat_date)
);

CREATE INDEX IF NOT EXISTS idx_tip_daily_stats_creator ON tip_daily_stats(creator_username, stat_date DESC);
