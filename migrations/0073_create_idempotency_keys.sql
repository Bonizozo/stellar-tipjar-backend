-- Postgres fallback store for Idempotency-Key semantics (#342).
-- Redis is the primary store (fast path); this table guarantees idempotency
-- survives Redis eviction/restarts for money-adjacent mutating endpoints.
CREATE TABLE IF NOT EXISTS idempotency_keys (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- sha256("<principal>|<route>|<idempotency-key>") — the lookup/lock scope.
    scope_hash          TEXT NOT NULL,
    principal           TEXT NOT NULL,
    route               TEXT NOT NULL,
    idempotency_key     TEXT NOT NULL,
    -- sha256("<method>|<path>|<request body>") — detects key reuse with a
    -- different payload so it can be rejected with 422.
    request_fingerprint TEXT NOT NULL,
    -- In-flight marker: set while a request is executing, cleared on completion.
    -- Used as the fallback mutual-exclusion mechanism when Redis is unavailable
    -- (via pg_try_advisory_lock keyed on scope_hash for the actual locking;
    -- this column lets us detect + report stuck in-flight rows for observability).
    in_flight           BOOLEAN NOT NULL DEFAULT TRUE,
    response_status     SMALLINT,
    response_headers    JSONB NOT NULL DEFAULT '{}',
    -- gzip-compressed response body, capped at IDEMPOTENCY_MAX_BODY_BYTES.
    response_body       BYTEA,
    response_body_hash  TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at        TIMESTAMPTZ,
    expires_at          TIMESTAMPTZ NOT NULL,
    UNIQUE (scope_hash)
);

CREATE INDEX IF NOT EXISTS idx_idempotency_keys_expires_at ON idempotency_keys(expires_at);
CREATE INDEX IF NOT EXISTS idx_idempotency_keys_principal_route ON idempotency_keys(principal, route);
