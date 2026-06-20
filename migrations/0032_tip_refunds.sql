CREATE TABLE IF NOT EXISTS tip_refunds (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tip_id          UUID NOT NULL REFERENCES tips(id) ON DELETE CASCADE,
    reason          TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'approved', 'rejected', 'completed')),
    refund_tx_hash  TEXT UNIQUE,
    reviewed_by     TEXT,
    reviewed_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tip_refunds_tip_id ON tip_refunds(tip_id);
CREATE INDEX IF NOT EXISTS idx_tip_refunds_status ON tip_refunds(status);
