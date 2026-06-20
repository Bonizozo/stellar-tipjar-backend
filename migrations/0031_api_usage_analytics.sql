CREATE TABLE IF NOT EXISTS api_usage_logs (
    id           BIGSERIAL PRIMARY KEY,
    method       TEXT NOT NULL,
    path         TEXT NOT NULL,
    status_code  SMALLINT NOT NULL,
    duration_ms  INTEGER NOT NULL,
    logged_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_api_usage_path ON api_usage_logs(path, logged_at DESC);
CREATE INDEX IF NOT EXISTS idx_api_usage_logged_at ON api_usage_logs(logged_at DESC);
