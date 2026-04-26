-- Add message and visibility support to tips
ALTER TABLE tips
    ADD COLUMN IF NOT EXISTS message            TEXT,
    ADD COLUMN IF NOT EXISTS message_visibility TEXT NOT NULL DEFAULT 'public'
        CHECK (message_visibility IN ('public', 'private', 'hidden'));

CREATE INDEX IF NOT EXISTS idx_tips_message_visibility ON tips(message_visibility);
