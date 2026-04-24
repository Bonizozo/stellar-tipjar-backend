CREATE TABLE IF NOT EXISTS scheduled_tips (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_username TEXT        NOT NULL,
    amount           TEXT        NOT NULL,
    tipper_ref       TEXT        NOT NULL,          -- opaque reference for the tipper (e.g. wallet address)
    message          TEXT,
    status           TEXT        NOT NULL DEFAULT 'pending'
                                 CHECK (status IN ('pending', 'processing', 'completed', 'failed', 'cancelled')),

    -- One-shot scheduling
    scheduled_at     TIMESTAMPTZ NOT NULL,

    -- Recurring support
    is_recurring     BOOLEAN     NOT NULL DEFAULT false,
    recurrence_rule  TEXT,                           -- 'daily' | 'weekly' | 'monthly'
    recurrence_end   TIMESTAMPTZ,                    -- NULL = recur indefinitely
    next_run_at      TIMESTAMPTZ,                    -- updated after each execution

    -- Execution tracking
    last_run_at      TIMESTAMPTZ,
    run_count        INT         NOT NULL DEFAULT 0,
    last_error       TEXT,

    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_scheduled_tips_status_next
    ON scheduled_tips(status, next_run_at)
    WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_scheduled_tips_creator
    ON scheduled_tips(creator_username);
