-- Extend moderation_queue with richer action tracking
ALTER TABLE moderation_queue
    ADD COLUMN IF NOT EXISTS action      TEXT CHECK (action IN ('warn', 'ban', 'dismiss', 'approve', 'reject')),
    ADD COLUMN IF NOT EXISTS flagged_by  TEXT,
    ADD COLUMN IF NOT EXISTS flag_reason TEXT;

-- Manual flags submitted by users or admins
CREATE TABLE IF NOT EXISTS moderation_flags (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    content_type TEXT NOT NULL,
    content_id   UUID NOT NULL,
    content_text TEXT NOT NULL,
    reason       TEXT NOT NULL,
    flagged_by   TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_moderation_flags_content ON moderation_flags(content_type, content_id);
CREATE INDEX IF NOT EXISTS idx_moderation_flags_created ON moderation_flags(created_at DESC);

-- Full audit trail for every action taken on a queue item
CREATE TABLE IF NOT EXISTS moderation_history (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    queue_item_id UUID NOT NULL REFERENCES moderation_queue(id) ON DELETE CASCADE,
    action       TEXT NOT NULL,
    performed_by TEXT NOT NULL,
    note         TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_moderation_history_item ON moderation_history(queue_item_id);
CREATE INDEX IF NOT EXISTS idx_moderation_history_created ON moderation_history(created_at DESC);
