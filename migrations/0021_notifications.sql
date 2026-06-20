-- Notification preferences per creator (one row per creator)
CREATE TABLE IF NOT EXISTS notification_preferences (
    creator_username TEXT PRIMARY KEY REFERENCES creators(username) ON DELETE CASCADE,
    notify_on_tip     BOOLEAN NOT NULL DEFAULT TRUE,
    notify_on_milestone BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Persistent notification history with read tracking
CREATE TABLE IF NOT EXISTS notifications (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    type             TEXT NOT NULL,   -- 'tip_received' | 'milestone'
    payload          JSONB NOT NULL DEFAULT '{}',
    read             BOOLEAN NOT NULL DEFAULT FALSE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_notifications_creator ON notifications(creator_username, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_notifications_unread  ON notifications(creator_username) WHERE read = FALSE;
