CREATE TABLE IF NOT EXISTS feature_flags (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name         TEXT NOT NULL UNIQUE,
    description  TEXT,
    enabled      BOOLEAN NOT NULL DEFAULT FALSE,
    rollout_pct  SMALLINT NOT NULL DEFAULT 0 CHECK (rollout_pct BETWEEN 0 AND 100),
    targeting    JSONB NOT NULL DEFAULT '[]',  -- array of usernames/rules
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_feature_flags_name ON feature_flags(name);
