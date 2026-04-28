CREATE TABLE IF NOT EXISTS webhook_dead_letter_queue (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    webhook_id UUID NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    last_error TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    failed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_webhook_dlq_webhook_id ON webhook_dead_letter_queue(webhook_id);
CREATE INDEX idx_webhook_dlq_failed_at ON webhook_dead_letter_queue(failed_at DESC);
