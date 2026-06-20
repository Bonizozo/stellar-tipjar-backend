CREATE TABLE IF NOT EXISTS creator_follows (
    follower_username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    followed_username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (follower_username, followed_username),
    CHECK (follower_username <> followed_username)
);

CREATE INDEX IF NOT EXISTS idx_follows_follower ON creator_follows(follower_username);
CREATE INDEX IF NOT EXISTS idx_follows_followed ON creator_follows(followed_username);
