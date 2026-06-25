-- Migration: rate limit quota tracking and analytics event log
--
-- Tables added:
--   api_client_quotas    — per-client daily/monthly quota state (upserted on each request)
--   rate_limit_events    — immutable log of every blocked request

-- ── Quota state ───────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS api_client_quotas (
    -- Stable identifier: "jwt:<sub>", "apikey:<prefix>", or "ip:<addr>"
    client_id       TEXT        NOT NULL,
    -- Period type: 'daily' | 'monthly'
    period          TEXT        NOT NULL CHECK (period IN ('daily', 'monthly')),
    -- ISO-8601 date of period start, e.g. '2026-06-25' or '2026-06-01'
    period_start    TEXT        NOT NULL,
    -- Maximum requests allowed in this period (tier default or admin override)
    max_requests    BIGINT      NOT NULL DEFAULT 10000,
    -- Atomically-incremented request counter
    used_requests   BIGINT      NOT NULL DEFAULT 0,
    -- False → enforcement skipped for this client (whitelist / bypass)
    enabled         BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (client_id, period, period_start)
);

CREATE INDEX IF NOT EXISTS idx_api_client_quotas_period
    ON api_client_quotas (period, period_start);

CREATE INDEX IF NOT EXISTS idx_api_client_quotas_usage
    ON api_client_quotas (used_requests DESC)
    WHERE enabled = TRUE;

-- Automatically refresh updated_at on every row change.
CREATE OR REPLACE FUNCTION set_quota_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_quota_updated_at ON api_client_quotas;
CREATE TRIGGER trg_quota_updated_at
    BEFORE UPDATE ON api_client_quotas
    FOR EACH ROW EXECUTE FUNCTION set_quota_updated_at();

-- ── Rate-limit event log ──────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS rate_limit_events (
    id              BIGSERIAL   PRIMARY KEY,
    -- Caller identity key (matches api_client_quotas.client_id format)
    client_id       TEXT        NOT NULL,
    -- Caller tier at the time of the event
    tier            TEXT        NOT NULL,
    -- Request path that was blocked
    path            TEXT        NOT NULL,
    -- Which limit was triggered: 'burst' | 'sustained' | 'quota'
    kind            TEXT        NOT NULL CHECK (kind IN ('burst', 'sustained', 'quota')),
    -- The limit that was active at the time (rpm or burst size)
    limit_value     BIGINT      NOT NULL DEFAULT 0,
    -- Request count at the time of the event
    request_count   BIGINT      NOT NULL DEFAULT 0,
    occurred_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Support time-range queries (the most common access pattern).
CREATE INDEX IF NOT EXISTS idx_rate_limit_events_occurred_at
    ON rate_limit_events (occurred_at DESC);

-- Support per-client lookups (top-offenders query).
CREATE INDEX IF NOT EXISTS idx_rate_limit_events_client_id
    ON rate_limit_events (client_id, occurred_at DESC);

-- Support per-tier breakdowns.
CREATE INDEX IF NOT EXISTS idx_rate_limit_events_tier
    ON rate_limit_events (tier, kind, occurred_at DESC);

-- Support per-path analytics.
CREATE INDEX IF NOT EXISTS idx_rate_limit_events_path
    ON rate_limit_events (path, occurred_at DESC);

-- Automatically purge events older than 30 days to prevent unbounded growth.
-- Pruning is handled by the cron scheduler (scheduler::SchedulerManager); this
-- comment documents the intent so the DBA is aware of the retention policy.
COMMENT ON TABLE rate_limit_events IS
    'Immutable log of rate-limited requests. Rows older than 30 days are pruned by the cron scheduler.';
