-- Refresh-token rotation with reuse detection (#345).
-- Each login/refresh cycle belongs to a "family". Every refresh rotates
-- `current_jti` forward; presenting a jti that no longer matches
-- `current_jti` means the token was already rotated away (stolen/replayed),
-- so the whole family is revoked and re-authentication is forced.
CREATE TABLE IF NOT EXISTS refresh_token_families (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    current_jti UUID NOT NULL,
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    revoked_reason TEXT,
    user_agent TEXT,
    ip_address TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    rotated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_refresh_token_families_username ON refresh_token_families(username);
CREATE INDEX IF NOT EXISTS idx_refresh_token_families_current_jti ON refresh_token_families(current_jti);
CREATE INDEX IF NOT EXISTS idx_refresh_token_families_active ON refresh_token_families(username, revoked, expires_at);
