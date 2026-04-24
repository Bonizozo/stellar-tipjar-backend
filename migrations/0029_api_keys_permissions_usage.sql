ALTER TABLE api_keys
    ADD COLUMN IF NOT EXISTS permissions TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS usage_count  BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS revoked_at   TIMESTAMPTZ;

-- partial index: only active, non-revoked keys
CREATE INDEX IF NOT EXISTS idx_api_keys_active ON api_keys(key)
    WHERE active = true AND revoked_at IS NULL;
