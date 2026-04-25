ALTER TABLE tips ADD COLUMN IF NOT EXISTS verified BOOLEAN NOT NULL DEFAULT false;

CREATE INDEX IF NOT EXISTS idx_tips_unverified ON tips(verified) WHERE verified = false;
