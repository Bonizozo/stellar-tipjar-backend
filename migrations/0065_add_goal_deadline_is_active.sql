-- Add deadline and is_active columns to tip_goals as required by issue #310
ALTER TABLE tip_goals
    ADD COLUMN IF NOT EXISTS deadline     TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS is_active    BOOLEAN NOT NULL DEFAULT true;

-- Backfill is_active from existing status column
UPDATE tip_goals SET is_active = (status = 'active');

-- Index for active goals queries
CREATE INDEX IF NOT EXISTS idx_tip_goals_is_active ON tip_goals (creator_username, is_active);
