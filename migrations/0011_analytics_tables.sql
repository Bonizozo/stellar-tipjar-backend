CREATE TABLE IF NOT EXISTS creator_stats (
    creator_username TEXT PRIMARY KEY,
    tip_count BIGINT NOT NULL DEFAULT 0,
    total_amount_stroops BIGINT NOT NULL DEFAULT 0,
    avg_amount_stroops BIGINT NOT NULL DEFAULT 0,
    last_tip_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS anomaly_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_username TEXT NOT NULL,
    amount_stroops BIGINT NOT NULL,
    baseline_stroops BIGINT NOT NULL,
    detected_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_anomaly_log_creator ON anomaly_log(creator_username);
