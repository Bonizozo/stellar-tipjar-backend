CREATE TABLE IF NOT EXISTS audit_logs (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    event_type   TEXT        NOT NULL,
    actor        TEXT,
    resource     TEXT        NOT NULL,
    resource_id  TEXT,
    action       TEXT        NOT NULL,
    before_data  JSONB,
    after_data   JSONB,
    metadata     JSONB       NOT NULL DEFAULT '{}',
    ip_address   TEXT,
    user_agent   TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_event_type  ON audit_logs(event_type);
CREATE INDEX IF NOT EXISTS idx_audit_logs_actor        ON audit_logs(actor);
CREATE INDEX IF NOT EXISTS idx_audit_logs_resource     ON audit_logs(resource);
CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at   ON audit_logs(created_at DESC);
