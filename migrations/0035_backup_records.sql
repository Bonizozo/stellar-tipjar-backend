CREATE TABLE IF NOT EXISTS backup_records (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    backup_type TEXT        NOT NULL,
    status      TEXT        NOT NULL,
    size_bytes  BIGINT,
    location    TEXT,
    checksum    TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_backup_records_created_at ON backup_records(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_backup_records_status     ON backup_records(status);
