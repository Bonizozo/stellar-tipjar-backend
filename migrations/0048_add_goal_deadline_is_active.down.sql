DROP INDEX IF EXISTS idx_tip_goals_is_active;
ALTER TABLE tip_goals
    DROP COLUMN IF EXISTS deadline,
    DROP COLUMN IF EXISTS is_active;
