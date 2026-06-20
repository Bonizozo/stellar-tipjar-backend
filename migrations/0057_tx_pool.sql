CREATE TYPE tx_pool_status AS ENUM ('pending', 'processing', 'confirmed', 'failed');

CREATE TABLE IF NOT EXISTS tx_pool (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    transaction_hash TEXT NOT NULL UNIQUE,
    status          tx_pool_status NOT NULL DEFAULT 'pending',
    retry_count     INT NOT NULL DEFAULT 0,
    max_retries     INT NOT NULL DEFAULT 5,
    last_error      TEXT,
    metadata        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    next_retry_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tx_pool_status        ON tx_pool (status);
CREATE INDEX IF NOT EXISTS idx_tx_pool_next_retry_at ON tx_pool (next_retry_at) WHERE status = 'pending';
