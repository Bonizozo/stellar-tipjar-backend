CREATE TABLE IF NOT EXISTS creator_verifications (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_username    TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    status              TEXT NOT NULL DEFAULT 'pending',  -- pending | approved | rejected
    identity_doc_url    TEXT,
    twitter_handle      TEXT,
    github_handle       TEXT,
    website_url         TEXT,
    rejection_reason    TEXT,
    reviewed_by         TEXT,  -- admin username
    submitted_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reviewed_at         TIMESTAMPTZ,
    UNIQUE (creator_username)
);

-- Add verification badge column to creators
ALTER TABLE creators ADD COLUMN IF NOT EXISTS is_verified BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX IF NOT EXISTS idx_verifications_status ON creator_verifications (status);
CREATE INDEX IF NOT EXISTS idx_verifications_creator ON creator_verifications (creator_username);
