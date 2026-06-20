-- Add optional message field to tips for content moderation
ALTER TABLE tips ADD COLUMN IF NOT EXISTS message TEXT;

-- Moderation review queue
CREATE TABLE moderation_queue (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    content_type TEXT NOT NULL CHECK (content_type IN ('username', 'tip_message', 'creator_bio')),
    content_id UUID,
    content_text TEXT NOT NULL,
    flags JSONB NOT NULL DEFAULT '[]'::jsonb,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected')),
    ai_score DOUBLE PRECISION,
    ai_reasoning TEXT,
    reviewed_by TEXT,
    reviewed_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_moderation_queue_status ON moderation_queue(status);
CREATE INDEX idx_moderation_queue_created_at ON moderation_queue(created_at DESC);
CREATE INDEX idx_moderation_queue_content_type ON moderation_queue(content_type);
