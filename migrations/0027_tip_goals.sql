CREATE TABLE IF NOT EXISTS tip_goals (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    title            TEXT NOT NULL,
    description      TEXT,
    target_amount    NUMERIC(20, 7) NOT NULL,
    current_amount   NUMERIC(20, 7) NOT NULL DEFAULT 0,
    status           TEXT NOT NULL DEFAULT 'active',  -- active | completed | cancelled
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at     TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS goal_milestones (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    goal_id          UUID NOT NULL REFERENCES tip_goals(id) ON DELETE CASCADE,
    creator_username TEXT NOT NULL,
    threshold_pct    INT  NOT NULL,  -- e.g. 25, 50, 75, 100
    reached_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tip_goals_creator ON tip_goals (creator_username, status);
CREATE INDEX IF NOT EXISTS idx_goal_milestones_goal ON goal_milestones (goal_id);
