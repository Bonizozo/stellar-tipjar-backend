-- Migration 0071: Webhook Security Overhaul (#338)
--
-- Adds:
--   1. webhook_secrets table — versioned secrets for dual-signing during rotation
--   2. delivery_id column on webhook_dead_letter_queue — stable across retries
--   3. delivery_id unique index — prevents duplicate DLQ entries per delivery
--   4. Backfills existing webhooks into webhook_secrets as the primary secret

-- 1. Versioned secrets table
CREATE TABLE IF NOT EXISTS webhook_secrets (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    webhook_id  UUID        NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    secret      TEXT        NOT NULL,
    -- TRUE = current primary; FALSE = retiring (kept during rotation overlap)
    is_primary  BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_webhook_secrets_webhook_id
    ON webhook_secrets(webhook_id);
CREATE INDEX IF NOT EXISTS idx_webhook_secrets_primary
    ON webhook_secrets(webhook_id, is_primary)
    WHERE is_primary = TRUE;

-- 2. Backfill existing webhooks so active_secrets() works immediately.
INSERT INTO webhook_secrets (webhook_id, secret, is_primary)
SELECT id, secret, TRUE
FROM webhooks
ON CONFLICT DO NOTHING;

-- 3. Add delivery_id to the DLQ table (nullable for backwards compat).
ALTER TABLE webhook_dead_letter_queue
    ADD COLUMN IF NOT EXISTS delivery_id UUID;

-- 4. Unique constraint: one DLQ entry per delivery_id (upsert-safe).
CREATE UNIQUE INDEX IF NOT EXISTS idx_webhook_dlq_delivery_id
    ON webhook_dead_letter_queue(delivery_id)
    WHERE delivery_id IS NOT NULL;
