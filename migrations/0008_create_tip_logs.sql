CREATE TABLE IF NOT EXISTS tip_logs (
    id SERIAL PRIMARY KEY,
    tip_id UUID NOT NULL REFERENCES tips(id) ON DELETE CASCADE,
    creator_username TEXT NOT NULL REFERENCES creators(username),
    action TEXT NOT NULL DEFAULT 'recorded',
    logged_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
