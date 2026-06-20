CREATE TABLE IF NOT EXISTS ip_blocks (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ip_address INET        NOT NULL UNIQUE,
    reason     TEXT,
    blocked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS country_blocks (
    country_code CHAR(2) PRIMARY KEY,
    reason       TEXT,
    blocked_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS ip_request_log (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ip_address   INET        NOT NULL,
    country_code CHAR(2),
    city         TEXT,
    path         TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ip_blocks_ip ON ip_blocks(ip_address);
CREATE INDEX IF NOT EXISTS idx_ip_request_log_ip ON ip_request_log(ip_address, created_at DESC);
